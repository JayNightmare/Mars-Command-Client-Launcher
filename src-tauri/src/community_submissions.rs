//! Private remote submissions. Never connected to local staging or installation.
use super::{api_base, CommunityState, Session};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tokio::io::AsyncReadExt;
use url::Url;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const INVALID: &str = "The community API returned an invalid capsule response.";
const CHANGED: &str = "Desktop session changed. Sign in and refresh submissions.";

#[derive(Clone)]
pub(super) struct SelectedJar {
    path: PathBuf,
    preview: JarSelection,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JarSelection {
    selection_id: String,
    file_name: String,
    size: u64,
    sha256: String,
    mod_ids: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapsuleInput {
    project: String,
    version: String,
    source_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CapsuleState {
    Reserved,
    Uploading,
    Quarantined,
    ScanPending,
    ScanBlocked,
    Rejected,
    Publishable,
    Expired,
    Withdrawn,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Verdict {
    Accepted,
    Rejected,
    Blocked,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    version: u64,
    artifact_sha256: String,
    provider: String,
    provider_result_id: String,
    policy_version: String,
    scanned_at: String,
    expires_at: String,
    verdict: Verdict,
    summary: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum QueueStatus {
    Pending,
    Leased,
    Complete,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Queue {
    status: QueueStatus,
    attempts: u64,
    max_attempts: u64,
    next_attempt_at: Option<String>,
    last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Capsule {
    release_id: String,
    owner_id: String,
    project: String,
    version: String,
    source_url: String,
    created_at: String,
    artifact_sha256: Option<String>,
    state: CapsuleState,
    revision: u64,
    updated_at: String,
    evidence: Option<Evidence>,
    queue: Option<Queue>,
    public_download_available: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapsuleList {
    capsules: Vec<Capsule>,
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control)
}

fn hexadecimal(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn https_source(value: &str) -> bool {
    value.len() <= 2048
        && !value
            .chars()
            .any(|c| c == '\\' || c <= ' ' || c == '\u{7f}')
        && Url::parse(value).is_ok_and(|u| {
            let public_host = u.host_str().is_some_and(|host| {
                host != "localhost"
                    && !host.ends_with(".localhost")
                    && !host.ends_with(".local")
                    && host.contains('.')
            });
            u.scheme() == "https"
                && public_host
                && u.username().is_empty()
                && u.password().is_none()
                && u.fragment().is_none()
                && u.port().is_none_or(|port| port == 443)
        })
}

fn timestamp(value: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(value).is_ok()
}

impl CapsuleInput {
    fn validate(&self) -> Result<(), String> {
        if !text(&self.project, 120) || !text(&self.version, 80) || !https_source(&self.source_url)
        {
            return Err("Provide a project (120 characters), version (80 characters), and credential-free HTTPS source URL.".into());
        }
        Ok(())
    }
}

impl Capsule {
    fn validate(&self, owner: &str, id: Option<&str>) -> Result<(), String> {
        if !hexadecimal(&self.release_id, 32)
            || id.is_some_and(|id| id != self.release_id)
            || self.owner_id != owner
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
            || self.public_download_available
            || !timestamp(&self.created_at)
            || !timestamp(&self.updated_at)
        {
            return Err(INVALID.into());
        }
        CapsuleInput {
            project: self.project.clone(),
            version: self.version.clone(),
            source_url: self.source_url.clone(),
        }
        .validate()
        .map_err(|_| INVALID)?;
        if self
            .artifact_sha256
            .as_ref()
            .is_some_and(|hash| !hexadecimal(hash, 64))
        {
            return Err(INVALID.into());
        }
        if matches!(
            self.state,
            CapsuleState::Quarantined
                | CapsuleState::ScanPending
                | CapsuleState::ScanBlocked
                | CapsuleState::Rejected
                | CapsuleState::Publishable
        ) && self.artifact_sha256.is_none()
        {
            return Err(INVALID.into());
        }
        if let Some(evidence) = &self.evidence {
            if evidence.version == 0
                || Some(&evidence.artifact_sha256) != self.artifact_sha256.as_ref()
                || !text(&evidence.provider, 120)
                || !text(&evidence.provider_result_id, 256)
                || !text(&evidence.policy_version, 120)
                || !timestamp(&evidence.scanned_at)
                || !timestamp(&evidence.expires_at)
                || evidence.summary.trim().is_empty()
                || evidence.summary.len() > 4096
            {
                return Err(INVALID.into());
            }
        }
        if self.state == CapsuleState::Publishable
            && self
                .evidence
                .as_ref()
                .is_none_or(|e| e.verdict != Verdict::Accepted)
        {
            return Err(INVALID.into());
        }
        if let Some(queue) = &self.queue {
            if queue.max_attempts == 0
                || queue.attempts > queue.max_attempts
                || queue
                    .next_attempt_at
                    .as_ref()
                    .is_some_and(|v| !timestamp(v))
                || queue.last_error.as_ref().is_some_and(|v| v.len() > 4096)
            {
                return Err(INVALID.into());
            }
        }
        Ok(())
    }
}

struct Auth {
    generation: u64,
    owner: String,
    token: String,
}

fn auth(state: &CommunityState) -> Result<Auth, String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?;
    Ok(Auth {
        generation: session.generation,
        owner: session
            .user
            .as_ref()
            .ok_or("Sign in to manage private submissions.")?
            .id
            .clone(),
        token: session
            .token
            .clone()
            .ok_or("Sign in to manage private submissions.")?,
    })
}

fn current(session: &Arc<Mutex<Session>>, generation: u64) -> Result<(), String> {
    if session
        .lock()
        .map_err(|_| "Account state unavailable.")?
        .generation
        != generation
    {
        return Err(CHANGED.into());
    }
    Ok(())
}

fn capsule_url(api: &Url, id: Option<&str>, action: Option<&str>) -> Result<Url, String> {
    let mut url = api
        .join("api/community/capsules")
        .map_err(|_| "Invalid API configuration.")?;
    if let Some(id) = id {
        if !hexadecimal(id, 32) {
            return Err("Invalid capsule release ID.".into());
        }
        url.path_segments_mut()
            .map_err(|_| "Invalid API configuration.")?
            .push(id);
    }
    if let Some(action) = action {
        url.path_segments_mut()
            .map_err(|_| "Invalid API configuration.")?
            .push(action);
    }
    Ok(url)
}

async fn response<T: serde::de::DeserializeOwned>(
    state: &CommunityState,
    auth: &Auth,
    request: reqwest::RequestBuilder,
) -> Result<T, String> {
    let result = request.bearer_auth(&auth.token).send().await;
    current(&state.session, auth.generation)?;
    let mut result = result.map_err(|_| "Community transfer interrupted. Refresh status before retrying; the reservation may already exist.")?;
    let status = result.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        if session.generation == auth.generation {
            *session = Session {
                generation: auth.generation + 1,
                ..Session::default()
            };
        }
        return Err("Your desktop session expired. Sign in again.".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = result
        .chunk()
        .await
        .map_err(|_| "Community response interrupted. Refresh status.")?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(INVALID.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    current(&state.session, auth.generation)?;
    if !status.is_success() {
        // Do not forward arbitrary server messages (which could echo credentials or paths).
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        let code = value["detail"]["code"].as_str().unwrap_or("");
        let reason = match code {
            "release_not_found" => "Submission not found.",
            "idempotency_conflict" => "Reservation key belongs to different metadata.",
            "upload_in_progress" => "An upload is already in progress. Refresh status.",
            "release_already_bound" => "This release is already bound to different bytes.",
            "invalid_state" => "This action is not allowed in the current state. Refresh status.",
            "submission_limit" => "Active submission limit reached. Withdraw an unused submission.",
            "retry_not_eligible" => "This scan cannot be retried yet.",
            "artifact_too_large" => "The server's JAR size limit was exceeded.",
            "artifact_empty" | "artifact_invalid" => "The server rejected the JAR.",
            "invalid_input" => "The server rejected the metadata.",
            "unsupported_media_type" => "The server rejected the upload media type.",
            "storage_unavailable" => "Private storage is unavailable. Retry later.",
            "upload_timeout" => {
                "Upload timed out. Refresh status before retrying the same release."
            }
            _ => "Community request failed. Refresh or retry later.",
        };
        return Err(format!("{reason} (HTTP {})", status.as_u16()));
    }

    serde_json::from_slice(&bytes).map_err(|_| INVALID.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SECRET: &str = "native-only-bearer";

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn signed_in() -> Arc<CommunityState> {
        let state = Arc::new(CommunityState::default());
        {
            let mut session = state.session.lock().unwrap();
            session.token = Some(SECRET.into());
            session.user = Some(super::super::User {
                id: "crew".into(),
                username: "crew".into(),
                avatar_url: String::new(),
                roles: Vec::new(),
            });
        }
        state
    }

    fn reserved() -> serde_json::Value {
        serde_json::json!({
            "releaseId": ID, "ownerId": "crew", "project": "Mod", "version": "1",
            "sourceUrl": "https://example.test/mod", "createdAt": "2026-10-10T10:00:00Z",
            "artifactSha256": null, "state": "reserved", "revision": 1, "updatedAt": "2026-10-10T10:00:00Z",
            "evidence": null, "queue": null, "publicDownloadAvailable": false
        })
    }

    fn input() -> CapsuleInput {
        CapsuleInput {
            project: "Mod".into(),
            version: "1".into(),
            source_url: "https://example.test/mod".into(),
        }
    }

    fn mock(
        status: u16,
        body: serde_json::Value,
        on_request: impl FnOnce() + Send + 'static,
    ) -> (Url, std::thread::JoinHandle<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut data = Vec::new();
            let (headers, payload) = loop {
                let mut buffer = [0; 64 * 1024];
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                data.extend_from_slice(&buffer[..count]);
                if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8(data[..end].to_vec()).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break (headers, data[end + 4..].to_vec());
                    }
                }
            };
            on_request();
            let json = serde_json::to_string(&body).unwrap();
            write!(socket, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len()).unwrap();
            (headers, payload)
        });
        (url, handle)
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, Vec<u8>) {
        let dir = tempfile::Builder::new()
            .prefix("capsule-submission-test-")
            .tempdir_in(".")
            .unwrap();
        let path = dir.path().join("mod.jar");
        let bytes = crate::personal_mods::tests::jar(&crate::personal_mods::tests::metadata(
            "example",
            "[1.21.1]",
            "[21.1.250]",
        ));
        std::fs::write(&path, &bytes).unwrap();
        (dir, path, bytes)
    }

    fn select(state: &CommunityState, path: &Path) -> JarSelection {
        let (_, mut preview) = open_jar(path).unwrap();
        preview.selection_id = "native-selection".into();
        state.session.lock().unwrap().selection = Some(SelectedJar {
            path: path.into(),
            preview: preview.clone(),
        });
        preview
    }

    #[test]
    fn selection_reuses_neoforge_metadata_without_installation_and_rejects_limits() {
        let (dir, path, _) = fixture();
        let (_, preview) = open_jar(&path).unwrap();
        assert_eq!(preview.mod_ids, ["example"]);
        assert!(!dir.path().join("mods").exists());
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains(&path.display().to_string()));
        std::fs::write(&path, []).unwrap();
        assert!(open_jar(&path).is_err());
        File::create(&path)
            .unwrap()
            .set_len(crate::personal_mods::MAX_JAR_BYTES + 1)
            .unwrap();
        assert!(open_jar(&path).is_err());
        std::fs::write(&path, b"not an archive").unwrap();
        assert!(open_jar(&path).is_err());
        let other = dir.path().join("mod.zip");
        std::fs::write(&other, b"not a JAR").unwrap();
        assert!(open_jar(&other).is_err());
    }

    #[test]
    fn malformed_foreign_and_public_capsules_fail_closed() {
        let valid: Capsule = serde_json::from_value(reserved()).unwrap();
        valid.validate("crew", Some(ID)).unwrap();
        for (field, value) in [
            ("revision", serde_json::json!(0)),
            ("ownerId", serde_json::json!("other")),
            ("releaseId", serde_json::json!("../escape")),
            ("publicDownloadAvailable", serde_json::json!(true)),
            ("artifactSha256", serde_json::json!("bad")),
            ("updatedAt", serde_json::json!("bad")),
            ("state", serde_json::json!("publishable")),
        ] {
            let mut body = reserved();
            body[field] = value;
            assert!(serde_json::from_value::<Capsule>(body)
                .map_or(true, |c| c.validate("crew", Some(ID)).is_err()));
        }
        let mut body = reserved();
        body["downloadUrl"] = serde_json::json!("https://example.test/artifact.jar");
        assert!(serde_json::from_value::<Capsule>(body).is_err());
    }

    #[test]
    fn reserve_keeps_bearer_native_and_sends_idempotency_header() {
        let state = signed_in();
        let (api, server) = mock(201, reserved(), || {});
        let capsule = runtime()
            .block_on(reserve(&state, &api, input(), "retry-key"))
            .unwrap();
        let (headers, body) = server.join().unwrap();
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer native-only-bearer"));
        assert!(headers
            .to_ascii_lowercase()
            .contains("idempotency-key: retry-key"));
        assert!(!serde_json::to_string(&capsule).unwrap().contains(SECRET));
        assert!(!String::from_utf8(body).unwrap().contains(SECRET));
        assert!(runtime()
            .block_on(reserve(&state, &api, input(), "bad key"))
            .is_err());
    }

    #[test]
    fn upload_streams_exact_raw_bytes_and_binds_backend_observed_hash() {
        let state = signed_in();
        let (_dir, path, bytes) = fixture();
        let preview = select(&state, &path);
        let mut body = reserved();
        body["artifactSha256"] = serde_json::json!(preview.sha256);
        body["state"] = serde_json::json!("scan_pending");
        body["revision"] = serde_json::json!(3);
        let (api, server) = mock(200, body, || {});
        let capsule = runtime()
            .block_on(upload(
                &state,
                &api,
                ID,
                &preview.selection_id,
                "operation",
                |_| {},
            ))
            .unwrap();
        let (headers, sent) = server.join().unwrap();
        assert!(headers.starts_with(&format!("PUT /api/community/capsules/{ID}/artifact ")));
        assert!(headers
            .to_ascii_lowercase()
            .contains("content-type: application/java-archive"));
        assert_eq!(sent, bytes);
        assert_eq!(
            capsule.artifact_sha256.as_deref(),
            Some(preview.sha256.as_str())
        );
        assert!(!state.session.lock().unwrap().uploading);
        assert!(path.exists());
    }

    #[test]
    fn changed_selection_and_streaming_failure_preserve_retry_and_release_lock() {
        let state = signed_in();
        let (_dir, path, _) = fixture();
        let preview = select(&state, &path);
        std::fs::write(&path, b"changed").unwrap();
        let api = Url::parse("http://127.0.0.1:1/").unwrap();
        assert!(runtime()
            .block_on(upload(
                &state,
                &api,
                ID,
                &preview.selection_id,
                "operation",
                |_| {}
            ))
            .is_err());
        assert!(!state.session.lock().unwrap().uploading);
        let (_dir2, path2, _) = fixture();
        let preview = select(&state, &path2);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            drop(socket);
        });
        let err = runtime()
            .block_on(upload(
                &state,
                &api,
                ID,
                &preview.selection_id,
                "operation",
                |_| {},
            ))
            .unwrap_err();
        server.join().unwrap();
        assert!(!err.contains(SECRET));
        assert!(!state.session.lock().unwrap().uploading);
        assert!(state.session.lock().unwrap().selection.is_some());
    }

    #[test]
    fn midstream_file_change_aborts_before_complete_body_and_preserves_selection() {
        let state = signed_in();
        let (dir, path, _) = fixture();
        let mut jar = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        jar.start_file("META-INF/neoforge.mods.toml", options)
            .unwrap();
        jar.write_all(
            crate::personal_mods::tests::metadata("example", "[1.21.1]", "[21.1.250]").as_bytes(),
        )
        .unwrap();
        jar.start_file("payload.bin", options).unwrap();
        jar.write_all(&vec![42; 200_000]).unwrap();
        std::fs::write(&path, jar.finish().unwrap().into_inner()).unwrap();
        let preview = select(&state, &path);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut received = Vec::new();
            let _ = socket.read_to_end(&mut received);
            received.len()
        });
        let changed_path = path.clone();
        let error = runtime()
            .block_on(upload(
                &state,
                &api,
                ID,
                &preview.selection_id,
                "operation",
                move |progress| {
                    if progress.sent_bytes == 64 * 1024 {
                        File::create(&changed_path).unwrap().set_len(1).unwrap();
                    }
                },
            ))
            .unwrap_err();
        assert!(server.join().unwrap() < preview.size as usize);
        assert!(!error.contains(SECRET));
        let session = state.session.lock().unwrap();
        assert!(!session.uploading && session.selection.is_some());
        assert!(!dir.path().join("mods").exists());
    }

    #[test]
    fn blocked_retry_and_withdraw_are_status_only_and_backend_errors_are_sanitized() {
        let state = signed_in();
        let mut body = reserved();
        body["state"] = serde_json::json!("scan_blocked");
        body["artifactSha256"] = serde_json::json!("b".repeat(64));
        body["queue"] = serde_json::json!({"status":"blocked","attempts":1,"maxAttempts":3,"nextAttemptAt":null,"lastError":"scanning_not_configured"});
        let (api, server) = mock(200, body, || {});
        let capsule = runtime()
            .block_on(operation(&state, &api, ID, Some("retry")))
            .unwrap();
        assert_eq!(capsule.state, CapsuleState::ScanBlocked);
        assert!(server
            .join()
            .unwrap()
            .0
            .starts_with(&format!("POST /api/community/capsules/{ID}/retry ")));
        let mut withdrawn = reserved();
        withdrawn["state"] = serde_json::json!("withdrawn");
        let (api, server) = mock(200, withdrawn, || {});
        assert_eq!(
            runtime()
                .block_on(operation(&state, &api, ID, Some("withdraw")))
                .unwrap()
                .state,
            CapsuleState::Withdrawn
        );
        server.join().unwrap();
        let (api, server) = mock(
            409,
            serde_json::json!({"detail":{"code":"retry_not_eligible","message":SECRET}}),
            || {},
        );
        let error = runtime()
            .block_on(operation(&state, &api, ID, Some("retry")))
            .unwrap_err();
        server.join().unwrap();
        assert!(!error.contains(SECRET));
    }

    #[test]
    fn logout_relogin_and_unauthorized_clear_native_selection_and_ignore_late_status() {
        let state = signed_in();
        let (_dir, path, _) = fixture();
        select(&state, &path);
        let switch = state.clone();
        let (api, server) = mock(200, reserved(), move || {
            super::super::clear_session(&switch).unwrap();
            let mut session = switch.session.lock().unwrap();
            session.token = Some("different-session".into());
            session.user = Some(super::super::User {
                id: "crew".into(),
                username: "crew".into(),
                avatar_url: String::new(),
                roles: Vec::new(),
            });
        });
        assert_eq!(
            runtime()
                .block_on(operation(&state, &api, ID, None))
                .unwrap_err(),
            CHANGED
        );
        server.join().unwrap();
        assert!(state.session.lock().unwrap().selection.is_none());
        select(&state, &path);
        let (api, server) = mock(401, serde_json::json!({"detail":SECRET}), || {});
        assert!(runtime()
            .block_on(operation(&state, &api, ID, None))
            .is_err());
        server.join().unwrap();
        let session = state.session.lock().unwrap();
        assert!(session.token.is_none() && session.user.is_none() && session.selection.is_none());
    }
}
fn open_jar(path: &Path) -> Result<(File, JarSelection), String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
    {
        return Err("Select a .jar file.".into());
    }
    let info = std::fs::symlink_metadata(path).map_err(|_| "Cannot inspect the selected JAR.")?;
    if !info.is_file() || info.file_type().is_symlink() {
        return Err("Select a regular, non-redirected JAR file.".into());
    }
    let mut file = File::open(path).map_err(|_| "Cannot open the selected JAR.")?;
    let size = file
        .metadata()
        .map_err(|_| "Cannot inspect the selected JAR.")?
        .len();
    if size == 0 || size > crate::personal_mods::MAX_JAR_BYTES {
        return Err("JAR must be non-empty and at most 64 MiB.".into());
    }
    let mod_ids = crate::personal_mods::submission_mod_ids(&mut file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot read the selected JAR.")?;
    let mut hash = Sha256::new();
    let mut read = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Cannot read the selected JAR.")?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > size {
            return Err("Selected JAR changed during validation.".into());
        }
        hash.update(&buffer[..count]);
    }
    if read != size {
        return Err("Selected JAR changed during validation.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot rewind the selected JAR.")?;
    Ok((
        file,
        JarSelection {
            selection_id: String::new(),
            file_name: path
                .file_name()
                .and_then(|v| v.to_str())
                .filter(|v| text(v, 255))
                .ok_or("Unsupported JAR filename.")?
                .into(),
            size,
            sha256: hex::encode(hash.finalize()),
            mod_ids,
        },
    ))
}

#[tauri::command]
pub async fn community_capsule_select(
    app: AppHandle,
    state: State<'_, CommunityState>,
) -> Result<Option<JarSelection>, String> {
    let auth = auth(&state)?;
    let picker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let selected = picker
            .dialog()
            .file()
            .set_title("Select a private submission JAR (maximum 64 MiB)")
            .add_filter("NeoForge mod JAR", &["jar"])
            .blocking_pick_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|_| "Unsupported native file selection.")?;
        let (_, mut preview) = open_jar(&path)?;
        preview.selection_id = format!(
            "{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        Ok::<_, String>(Some(SelectedJar { path, preview }))
    })
    .await
    .map_err(|_| "Native JAR selection failed.")??;
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?;
    if session.generation != auth.generation {
        return Err(CHANGED.into());
    }
    if session.uploading {
        return Err("Wait for the current upload before selecting another JAR.".into());
    }
    if let Some(selected) = result {
        let preview = selected.preview.clone();
        session.selection = Some(selected);
        return Ok(Some(preview));
    }
    Ok(None)
}

#[tauri::command]
pub async fn community_capsule_reserve(
    state: State<'_, CommunityState>,
    input: CapsuleInput,
    idempotency_key: String,
) -> Result<Capsule, String> {
    reserve(&state, &api_base()?, input, &idempotency_key).await
}

async fn reserve(
    state: &CommunityState,
    api: &Url,
    input: CapsuleInput,
    key: &str,
) -> Result<Capsule, String> {
    input.validate()?;
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err("Invalid reservation idempotency key.".into());
    }
    let auth = auth(state)?;
    let capsule: Capsule = response(
        state,
        &auth,
        state
            .client
            .post(capsule_url(api, None, None)?)
            .header("Idempotency-Key", key)
            .json(&input),
    )
    .await?;
    capsule.validate(&auth.owner, None)?;
    if capsule.project != input.project
        || capsule.version != input.version
        || capsule.source_url != input.source_url
    {
        return Err(INVALID.into());
    }
    Ok(capsule)
}

async fn operation(
    state: &CommunityState,
    api: &Url,
    id: &str,
    action: Option<&str>,
) -> Result<Capsule, String> {
    let auth = auth(state)?;
    let url = capsule_url(api, Some(id), action)?;
    let request = if action.is_some() {
        state.client.post(url)
    } else {
        state.client.get(url)
    };
    let capsule: Capsule = response(state, &auth, request).await?;
    capsule.validate(&auth.owner, Some(id))?;
    Ok(capsule)
}

#[tauri::command]
pub async fn community_capsules(state: State<'_, CommunityState>) -> Result<CapsuleList, String> {
    let auth = auth(&state)?;
    let list: CapsuleList = response(
        &state,
        &auth,
        state
            .client
            .get(capsule_url(&api_base()?, None, Some("mine"))?),
    )
    .await?;
    let mut ids = std::collections::HashSet::new();
    for capsule in &list.capsules {
        capsule.validate(&auth.owner, None)?;
        if !ids.insert(&capsule.release_id) {
            return Err(INVALID.into());
        }
    }
    Ok(list)
}

#[tauri::command]
pub async fn community_capsule_status(
    state: State<'_, CommunityState>,
    id: String,
) -> Result<Capsule, String> {
    operation(&state, &api_base()?, &id, None).await
}

#[tauri::command]
pub async fn community_capsule_retry(
    state: State<'_, CommunityState>,
    id: String,
) -> Result<Capsule, String> {
    operation(&state, &api_base()?, &id, Some("retry")).await
}

#[tauri::command]
pub async fn community_capsule_withdraw(
    state: State<'_, CommunityState>,
    id: String,
) -> Result<Capsule, String> {
    operation(&state, &api_base()?, &id, Some("withdraw")).await
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadProgress {
    operation_id: String,
    release_id: String,
    sent_bytes: u64,
    total_bytes: u64,
}

struct UploadGuard {
    session: Arc<Mutex<Session>>,
    generation: u64,
}

impl Drop for UploadGuard {
    fn drop(&mut self) {
        if let Ok(mut session) = self.session.lock() {
            if session.generation == self.generation {
                session.uploading = false;
            }
        }
    }
}

#[tauri::command]
pub async fn community_capsule_upload(
    app: AppHandle,
    state: State<'_, CommunityState>,
    id: String,
    selection_id: String,
    operation_id: String,
) -> Result<Capsule, String> {
    upload(
        &state,
        &api_base()?,
        &id,
        &selection_id,
        &operation_id,
        move |progress| {
            let _ = app.emit("community-capsule-upload-progress", progress);
        },
    )
    .await
}

async fn upload(
    state: &CommunityState,
    api: &Url,
    id: &str,
    selection_id: &str,
    operation_id: &str,
    emit: impl Fn(UploadProgress) + Clone + Send + 'static,
) -> Result<Capsule, String> {
    if !text(operation_id, 128) {
        return Err("Invalid upload operation ID.".into());
    }
    let auth = auth(state)?;
    let url = capsule_url(api, Some(id), Some("artifact"))?;
    let selected = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        if session.generation != auth.generation {
            return Err(CHANGED.into());
        }
        if session.uploading {
            return Err("An upload is already in progress.".into());
        }
        let selected = session
            .selection
            .clone()
            .filter(|s| s.preview.selection_id == selection_id)
            .ok_or("Select the JAR again before uploading.")?;
        session.uploading = true;
        selected
    };
    let _guard = UploadGuard {
        session: state.session.clone(),
        generation: auth.generation,
    };
    let path = selected.path.clone();
    let (file, checked) = tauri::async_runtime::spawn_blocking(move || open_jar(&path))
        .await
        .map_err(|_| "JAR validation failed.")??;
    current(&state.session, auth.generation)?;
    if checked.sha256 != selected.preview.sha256 || checked.size != selected.preview.size {
        return Err("Selected JAR changed. Select it again; nothing was uploaded.".into());
    }
    let size = checked.size;
    let expected = checked.sha256.clone();
    let session = state.session.clone();
    let generation = auth.generation;
    let progress = UploadProgress {
        operation_id: operation_id.into(),
        release_id: id.into(),
        sent_bytes: 0,
        total_bytes: size,
    };
    let stream = futures_util::stream::try_unfold(
        (tokio::fs::File::from_std(file), Sha256::new(), progress),
        move |(mut file, mut hash, mut progress)| {
            let session = session.clone();
            let emit = emit.clone();
            let expected = expected.clone();
            async move {
                let failure =
                    || std::io::Error::other("JAR transfer interrupted or selected bytes changed.");
                current(&session, generation).map_err(|_| failure())?;
                if progress.sent_bytes == size {
                    return Ok(None);
                }
                let mut bytes = vec![0u8; (size - progress.sent_bytes).min(64 * 1024) as usize];
                file.read_exact(&mut bytes).await.map_err(|_| failure())?;
                hash.update(&bytes);
                progress.sent_bytes += bytes.len() as u64;
                if progress.sent_bytes == size {
                    let mut extra = [0];
                    if file.read(&mut extra).await.map_err(|_| failure())? != 0
                        || hex::encode(hash.clone().finalize()) != expected
                    {
                        return Err(failure());
                    }
                }
                current(&session, generation).map_err(|_| failure())?;
                emit(progress.clone());
                Ok(Some((bytes, (file, hash, progress))))
            }
        },
    );
    let request = state
        .client
        .put(url)
        .timeout(Duration::from_secs(300))
        .header(reqwest::header::CONTENT_TYPE, "application/java-archive")
        .header(reqwest::header::CONTENT_LENGTH, size)
        .body(reqwest::Body::wrap_stream(stream));
    let capsule: Capsule = response(state, &auth, request).await?;
    capsule.validate(&auth.owner, Some(id))?;
    if capsule.artifact_sha256.as_ref() != Some(&checked.sha256) {
        return Err(INVALID.into());
    }
    Ok(capsule)
}
