//! Minecraft Java Edition server-list ping (modern protocol, 1.7+).
//!
//! Resolution order: `_minecraft._tcp.<host>` SRV record, then the literal
//! host/port supplied by the caller. Every failure path is converted into a
//! well-formed offline result so the UI never has to handle a rejected promise.

use std::time::{Duration, Instant};

use hickory_resolver::TokioAsyncResolver;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Upper bound for the whole status exchange (DNS excluded).
const PING_TIMEOUT: Duration = Duration::from_secs(6);
const SRV_TIMEOUT: Duration = Duration::from_secs(3);
/// Protocol 767 (1.21.1). The `-1` status sentinel is rejected outright by the
/// proxy in front of Mars, so a concrete version is sent instead.
const PROTOCOL_VERSION: i32 = 767;
/// Guards against a hostile/broken server announcing a huge status payload.
const MAX_PACKET_BYTES: i32 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftServerStatus {
    pub online: bool,
    pub host: String,
    pub port: u16,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub latency_ms: Option<u64>,
    pub motd: Option<String>,
    pub version_name: Option<String>,
    pub checked_at: String,
    pub error: Option<String>,
}

impl MinecraftServerStatus {
    fn offline(host: String, port: u16, error: String) -> Self {
        Self {
            online: false,
            host,
            port,
            players_online: None,
            players_max: None,
            latency_ms: None,
            motd: None,
            version_name: None,
            checked_at: now_rfc3339(),
            error: Some(error),
        }
    }
}

fn now_rfc3339() -> String {
    crate::now_rfc3339()
}

/// Never returns `Err`: transport problems are reported as an offline status.
pub async fn fetch_status(host: String, port: u16) -> MinecraftServerStatus {
    let (target_host, target_port) = resolve_srv(&host, port).await;

    match timeout(PING_TIMEOUT, ping(&host, port, &target_host, target_port)).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => MinecraftServerStatus::offline(host, port, err),
        Err(_) => MinecraftServerStatus::offline(host, port, "Status request timed out".into()),
    }
}

async fn resolve_srv(host: &str, port: u16) -> (String, u16) {
    let fallback = (host.to_string(), port);

    // A literal IP can never carry an SRV record; skip the lookup entirely.
    if host.parse::<std::net::IpAddr>().is_ok() {
        return fallback;
    }

    let Ok(resolver) = TokioAsyncResolver::tokio_from_system_conf() else {
        return fallback;
    };

    let query = format!("_minecraft._tcp.{host}.");
    match timeout(SRV_TIMEOUT, resolver.srv_lookup(query)).await {
        Ok(Ok(lookup)) => lookup
            .iter()
            .min_by_key(|record| record.priority())
            .map(|record| {
                (
                    record.target().to_utf8().trim_end_matches('.').to_string(),
                    record.port(),
                )
            })
            .unwrap_or(fallback),
        _ => fallback,
    }
}

async fn ping(
    announced_host: &str,
    announced_port: u16,
    target_host: &str,
    target_port: u16,
) -> Result<MinecraftServerStatus, String> {
    let mut stream = TcpStream::connect((target_host, target_port))
        .await
        .map_err(|err| format!("Connection failed: {err}"))?;
    stream.set_nodelay(true).ok();

    // Handshake: announce the SRV-resolved endpoint, matching what a vanilla
    // client sends after its own SRV lookup.
    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00);
    write_varint(&mut handshake, PROTOCOL_VERSION);
    write_string(&mut handshake, target_host);
    handshake.extend_from_slice(&target_port.to_be_bytes());
    write_varint(&mut handshake, 1); // next state: status
    write_framed(&mut stream, &handshake).await?;

    let mut status_request = Vec::new();
    write_varint(&mut status_request, 0x00);
    write_framed(&mut stream, &status_request).await?;

    let payload = read_framed(&mut stream).await?;
    let mut cursor = Cursor::new(&payload);
    let packet_id = cursor.read_varint()?;
    if packet_id != 0x00 {
        return Err(format!("Unexpected status packet id {packet_id}"));
    }
    let json = cursor.read_string()?;

    let latency_ms = measure_latency(&mut stream).await;

    let parsed: serde_json::Value =
        serde_json::from_str(&json).map_err(|err| format!("Malformed status payload: {err}"))?;

    Ok(MinecraftServerStatus {
        online: true,
        host: announced_host.to_string(),
        port: announced_port,
        players_online: read_u32(&parsed, &["players", "online"]),
        players_max: read_u32(&parsed, &["players", "max"]),
        latency_ms,
        motd: parse_motd(parsed.get("description")),
        version_name: parsed
            .pointer("/version/name")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        checked_at: now_rfc3339(),
        error: None,
    })
}

/// Round-trips a ping packet. Latency is best-effort: some proxies drop it.
async fn measure_latency(stream: &mut TcpStream) -> Option<u64> {
    let mut packet = Vec::new();
    write_varint(&mut packet, 0x01);
    packet.extend_from_slice(&0i64.to_be_bytes());

    let started = Instant::now();
    write_framed(stream, &packet).await.ok()?;
    read_framed(stream).await.ok()?;
    Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64)
}

fn read_u32(value: &serde_json::Value, path: &[&str]) -> Option<u32> {
    let mut node = value;
    for key in path {
        node = node.get(key)?;
    }
    node.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// Flattens a legacy string or a chat component tree, dropping `§` codes.
fn parse_motd(description: Option<&serde_json::Value>) -> Option<String> {
    let mut out = String::new();
    flatten_chat(description?, &mut out);
    let cleaned = strip_legacy_codes(&out);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn flatten_chat(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(text) => out.push_str(text),
        serde_json::Value::Array(items) => {
            for item in items {
                flatten_chat(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("text") {
                out.push_str(text);
            }
            if let Some(extra) = map.get("extra") {
                flatten_chat(extra, out);
            }
        }
        _ => {}
    }
}

fn strip_legacy_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{00a7}' {
            chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

// --- packet framing -------------------------------------------------------

async fn write_framed(stream: &mut TcpStream, body: &[u8]) -> Result<(), String> {
    let mut framed = Vec::with_capacity(body.len() + 5);
    write_varint(&mut framed, body.len() as i32);
    framed.extend_from_slice(body);
    stream
        .write_all(&framed)
        .await
        .map_err(|err| format!("Write failed: {err}"))
}

async fn read_framed(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let length = read_varint_async(stream).await?;
    if !(0..=MAX_PACKET_BYTES).contains(&length) {
        return Err(format!("Invalid packet length {length}"));
    }
    let mut buffer = vec![0u8; length as usize];
    stream
        .read_exact(&mut buffer)
        .await
        .map_err(|err| format!("Read failed: {err}"))?;
    Ok(buffer)
}

async fn read_varint_async(stream: &mut TcpStream) -> Result<i32, String> {
    let mut result: i32 = 0;
    for index in 0..5 {
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|err| format!("Read failed: {err}"))?;
        result |= ((byte[0] & 0x7F) as i32) << (7 * index);
        if byte[0] & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err("VarInt exceeded 5 bytes".into())
}

fn write_varint(buffer: &mut Vec<u8>, value: i32) {
    let mut remaining = value as u32;
    loop {
        let mut byte = (remaining & 0x7F) as u8;
        remaining >>= 7;
        if remaining != 0 {
            byte |= 0x80;
        }
        buffer.push(byte);
        if remaining == 0 {
            break;
        }
    }
}

fn write_string(buffer: &mut Vec<u8>, value: &str) {
    write_varint(buffer, value.len() as i32);
    buffer.extend_from_slice(value.as_bytes());
}

struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn read_varint(&mut self) -> Result<i32, String> {
        let mut result: i32 = 0;
        for index in 0..5 {
            let byte = *self
                .data
                .get(self.position)
                .ok_or_else(|| "Truncated packet".to_string())?;
            self.position += 1;
            result |= ((byte & 0x7F) as i32) << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(result);
            }
        }
        Err("VarInt exceeded 5 bytes".into())
    }

    fn read_string(&mut self) -> Result<String, String> {
        let length = self.read_varint()?;
        if length < 0 {
            return Err("Negative string length".into());
        }
        let end = self
            .position
            .checked_add(length as usize)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "Truncated string".to_string())?;
        let text = String::from_utf8_lossy(&self.data[self.position..end]).into_owned();
        self.position = end;
        Ok(text)
    }
}
