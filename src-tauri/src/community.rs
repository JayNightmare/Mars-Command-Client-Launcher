use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::State;
use url::Url;

#[path = "community_submissions.rs"]
pub mod submissions;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    id: String,
    username: String,
    avatar_url: String,
    roles: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMod {
    name: String,
    version: String,
    source_url: String,
    sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    id: String,
    name: String,
    description: String,
    visibility: Visibility,
    owner: User,
    mods: Vec<ProfileMod>,
    source_profile_id: Option<String>,
    updated_at: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Private,
    Public,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceResponse {
    device_code: String,
    request_id: String,
    verification_uri: String,
    expires_in: u64,
    poll_interval: u64,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum PollStatus {
    Pending,
    Approved,
    Expired,
    Denied,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollResponse {
    status: PollStatus,
    access_token: Option<String>,
    user: Option<User>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    verification_uri: String,
    expires_in: u64,
    poll_interval: u64,
}

#[derive(Serialize)]
pub struct LoginPoll {
    status: PollStatus,
    user: Option<User>,
}

struct PendingLogin {
    device_code: String,
    expires: Instant,
    interval: Duration,
    next_poll: Instant,
    polling: bool,
}

#[derive(Default)]
struct Session {
    generation: u64,
    pending: Option<PendingLogin>,
    token: Option<String>,
    user: Option<User>,
    selection: Option<submissions::SelectedJar>,
    uploading: bool,
}

pub struct CommunityState {
    session: Arc<Mutex<Session>>,
    client: reqwest::Client,
}

impl Default for CommunityState {
    fn default() -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::default())),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("HTTP client initialization"),
        }
    }
}

fn configured_url(value: Option<String>, name: &str) -> Result<Url, String> {
    let mut url = Url::parse(
        value
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| format!("Configure {name} before using community accounts."))?
            .trim(),
    )
    .map_err(|_| format!("{name} must be a valid URL."))?;
    if !(url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("{name} requires HTTPS (HTTP only for loopback), without credentials, query or fragment."));
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

fn api_base() -> Result<Url, String> {
    configured_url(
        std::env::var("MARS_COMMUNITY_API_BASE")
            .ok()
            .or_else(|| option_env!("MARS_COMMUNITY_API_BASE").map(str::to_string)),
        "MARS_COMMUNITY_API_BASE",
    )
}

fn website_base() -> Result<Url, String> {
    configured_url(
        std::env::var("MARS_WEBSITE_BASE")
            .ok()
            .or_else(|| option_env!("MARS_WEBSITE_BASE").map(str::to_string)),
        "MARS_WEBSITE_BASE",
    )
}

fn verification_url(website: &Url, response: &DeviceResponse) -> Result<String, String> {
    let uri = Url::parse(&response.verification_uri)
        .map_err(|_| "The server returned an invalid website login URL.")?;
    let expected = website
        .join("auth/login")
        .map_err(|_| "Invalid website base.")?;
    let pairs: Vec<_> = uri.query_pairs().collect();
    if uri.origin() != website.origin()
        || uri.path() != expected.path()
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.fragment().is_some()
        || pairs.len() != 1
        || pairs[0].0 != "requestId"
        || pairs[0].1 != response.request_id
        || response.request_id.is_empty()
        || response.device_code.is_empty()
        || response.request_id == response.device_code
        || response.verification_uri.contains(&response.device_code)
    {
        return Err("The server returned an unsafe website login URL.".into());
    }
    Ok(uri.to_string())
}

fn endpoint(base: &Url, path: &str) -> Result<Url, String> {
    base.join(path)
        .map_err(|_| "Invalid API configuration.".into())
}

async fn json_response<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
    auth: bool,
) -> Result<T, String> {
    let response = request
        .send()
        .await
        .map_err(|_| "Cannot reach the community API. Check configuration and network.")?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        if auth {
            return Err(format!(
                "Website login request failed (HTTP {status}). Please retry."
            ));
        }
        return Err(api_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The community API returned an invalid response.".into())
}

async fn api_error(response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    if status == 401 {
        return "Your desktop session expired or was rejected. Sign in again.".into();
    }
    let fallback = format!("Community request failed (HTTP {status}).");
    // Only contractual errors are displayed; never include request headers or transport errors.
    let Ok(body) = response.json::<serde_json::Value>().await else {
        return fallback;
    };
    match &body["detail"] {
        serde_json::Value::String(message) => format!(
            "{fallback} {}",
            message.chars().take(500).collect::<String>()
        ),
        serde_json::Value::Object(detail) => {
            let message = detail
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("No further details.");
            let code = detail
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("request_failed");
            format!(
                "{fallback} {}: {}",
                code.chars().take(80).collect::<String>(),
                message.chars().take(500).collect::<String>()
            )
        }
        _ => fallback,
    }
}

#[tauri::command]
pub async fn desktop_login_start(state: State<'_, CommunityState>) -> Result<LoginStart, String> {
    start_login(&state, &api_base()?, &website_base()?).await
}

async fn start_login(
    state: &CommunityState,
    api: &Url,
    website: &Url,
) -> Result<LoginStart, String> {
    let generation = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        session.generation += 1;
        session.pending = None;
        session.token = None;
        session.user = None;
        session.selection = None;
        session.uploading = false;
        session.generation
    };
    let response: DeviceResponse = json_response(
        state
            .client
            .post(endpoint(api, "api/auth/desktop")?)
            .json(&serde_json::json!({})),
        true,
    )
    .await?;
    let uri = verification_url(website, &response)?;
    if response.expires_in == 0
        || response.poll_interval == 0
        || response.poll_interval > response.expires_in
    {
        return Err("The server returned invalid login expiry or polling timing.".into());
    }
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?;
    if session.generation != generation {
        return Err("Login was cancelled.".into());
    }
    let now = Instant::now();
    let expires = now
        .checked_add(Duration::from_secs(response.expires_in))
        .ok_or("The server returned an unsupported login expiry.")?;
    let next_poll = now
        .checked_add(Duration::from_secs(response.poll_interval))
        .ok_or("The server returned an unsupported polling interval.")?;
    session.pending = Some(PendingLogin {
        device_code: response.device_code,
        expires,
        interval: Duration::from_secs(response.poll_interval),
        next_poll,
        polling: false,
    });
    Ok(LoginStart {
        verification_uri: uri,
        expires_in: response.expires_in,
        poll_interval: response.poll_interval,
    })
}

#[tauri::command]
pub async fn desktop_login_poll(state: State<'_, CommunityState>) -> Result<LoginPoll, String> {
    poll_login(&state, &api_base()?).await
}

async fn poll_login(state: &CommunityState, api: &Url) -> Result<LoginPoll, String> {
    let (generation, code) = {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        let generation = session.generation;
        let pending = session
            .pending
            .as_mut()
            .ok_or("No active login. Please retry.")?;
        let now = Instant::now();
        if now >= pending.expires {
            session.pending = None;
            return Ok(LoginPoll {
                status: PollStatus::Expired,
                user: None,
            });
        }
        if pending.polling || now < pending.next_poll {
            return Ok(LoginPoll {
                status: PollStatus::Pending,
                user: None,
            });
        }
        pending.polling = true;
        pending.next_poll = now.checked_add(pending.interval).unwrap_or(pending.expires);
        (generation, pending.device_code.clone())
    };
    let result: Result<PollResponse, String> = json_response(
        state
            .client
            .post(endpoint(api, "api/auth/desktop/poll")?)
            .json(&serde_json::json!({"deviceCode": code})),
        true,
    )
    .await;
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?;
    if generation != session.generation {
        return Err("Login was cancelled.".into());
    }
    if session
        .pending
        .as_ref()
        .is_none_or(|p| Instant::now() >= p.expires)
    {
        session.pending = None;
        return Ok(LoginPoll {
            status: PollStatus::Expired,
            user: None,
        });
    }
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            session.pending = None;
            return Err(error);
        }
    };
    if response.status == PollStatus::Pending {
        if let Some(pending) = session.pending.as_mut() {
            pending.polling = false;
        }
    } else {
        session.pending = None;
    }
    if response.status == PollStatus::Approved {
        let token = response
            .access_token
            .filter(|t| !t.is_empty())
            .ok_or("Approved login did not provide a desktop session.")?;
        let user = response
            .user
            .ok_or("Approved login did not provide an identity.")?;
        session.token = Some(token);
        session.user = Some(user.clone());
        return Ok(LoginPoll {
            status: PollStatus::Approved,
            user: Some(user),
        });
    }
    Ok(LoginPoll {
        status: response.status,
        user: None,
    })
}

#[tauri::command]
pub fn desktop_logout(state: State<'_, CommunityState>) -> Result<(), String> {
    clear_session(&state)
}

fn clear_session(state: &CommunityState) -> Result<(), String> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?;
    let generation = session.generation + 1;
    *session = Session {
        generation,
        ..Session::default()
    };
    Ok(())
}

#[tauri::command]
pub fn desktop_account(state: State<'_, CommunityState>) -> Result<Option<User>, String> {
    Ok(state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?
        .user
        .clone())
}

#[tauri::command]
pub fn desktop_sponsors_url(state: State<'_, CommunityState>) -> Result<String, String> {
    if state
        .session
        .lock()
        .map_err(|_| "Account state unavailable.")?
        .user
        .is_none()
    {
        return Err("Sign in through the website before opening Sponsors.".into());
    }
    sponsors_url(
        std::env::var("MARS_SPONSORS_URL")
            .ok()
            .or_else(|| option_env!("MARS_SPONSORS_URL").map(str::to_string)),
    )
}

fn sponsors_url(value: Option<String>) -> Result<String, String> {
    let url = configured_url(value, "MARS_SPONSORS_URL")?;
    let segments = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || segments.len() != 2
        || segments[0] != "sponsors"
        || segments[1].is_empty()
        || segments[1].starts_with('-')
        || segments[1].ends_with('-')
        || !segments[1]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Configure an explicit https://github.com/sponsors/<recipient> URL.".into());
    }
    Ok(url.to_string())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    name: String,
    description: String,
    mods: Vec<ProfileMod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_profile_id: Option<String>,
}

#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfilePatch {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    name: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    description: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    mods: Option<Vec<ProfileMod>>,
}

fn non_null<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, Serialize)]
pub struct ProfileList {
    profiles: Vec<Profile>,
}

async fn community_request(
    state: &CommunityState,
    method: reqwest::Method,
    id: Option<&str>,
    suffix: Option<&str>,
    query: Option<&str>,
    body: Option<serde_json::Value>,
    authenticated: bool,
) -> Result<reqwest::Response, String> {
    send_community_request(
        state,
        &api_base()?,
        method,
        id,
        suffix,
        query,
        body,
        authenticated,
    )
    .await
}

async fn send_community_request(
    state: &CommunityState,
    api: &Url,
    method: reqwest::Method,
    id: Option<&str>,
    suffix: Option<&str>,
    query: Option<&str>,
    body: Option<serde_json::Value>,
    authenticated: bool,
) -> Result<reqwest::Response, String> {
    let mut url = endpoint(api, "api/community/profiles")?;
    if let Some(id) = id {
        if id.is_empty() || matches!(id, "." | "..") {
            return Err("Profile ID is missing or invalid.".into());
        }
        url.path_segments_mut()
            .map_err(|_| "Invalid API base.")?
            .push(id);
    }
    if let Some(suffix) = suffix {
        url.path_segments_mut()
            .map_err(|_| "Invalid API base.")?
            .push(suffix);
    }
    if let Some(query) = query {
        url.query_pairs_mut().append_pair("q", query);
    }
    let (generation, token) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        let token = if authenticated {
            Some(
                session
                    .token
                    .clone()
                    .ok_or("Sign in to manage personal profiles.")?,
            )
        } else {
            None
        };
        (session.generation, token)
    };
    let mut request = state.client.request(method, url);
    if let Some(token) = &token {
        request = request.bearer_auth(token);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Cannot reach the community API. Check configuration and network.")?;
    if authenticated {
        let mut session = state
            .session
            .lock()
            .map_err(|_| "Account state unavailable.")?;
        if session.generation != generation {
            return Err("Desktop session changed. Refresh profiles.".into());
        }
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            *session = Session {
                generation: generation + 1,
                ..Session::default()
            };
        }
    }
    if !response.status().is_success() {
        return Err(api_error(response).await);
    }
    Ok(response)
}

async fn profile_response(response: reqwest::Response) -> Result<Profile, String> {
    response
        .json()
        .await
        .map_err(|_| "The community API returned an invalid profile.".into())
}

#[tauri::command]
pub async fn community_profiles(
    state: State<'_, CommunityState>,
    query: String,
    mine: bool,
) -> Result<ProfileList, String> {
    community_request(
        &state,
        reqwest::Method::GET,
        None,
        mine.then_some("mine"),
        (!mine).then_some(query.as_str()),
        None,
        mine,
    )
    .await?
    .json()
    .await
    .map_err(|_| "The community API returned an invalid profile list.".into())
}

#[tauri::command]
pub async fn community_create(
    state: State<'_, CommunityState>,
    input: ProfileInput,
) -> Result<Profile, String> {
    if input.name.trim().is_empty() {
        return Err("Profile name cannot be empty.".into());
    }
    profile_response(
        community_request(
            &state,
            reqwest::Method::POST,
            None,
            None,
            None,
            Some(serde_json::to_value(input).map_err(|_| "Invalid profile fields.")?),
            true,
        )
        .await?,
    )
    .await
}

#[tauri::command]
pub async fn community_update(
    state: State<'_, CommunityState>,
    id: String,
    patch: ProfilePatch,
) -> Result<Profile, String> {
    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err("Profile name cannot be empty.".into());
    }
    profile_response(
        community_request(
            &state,
            reqwest::Method::PATCH,
            Some(&id),
            None,
            None,
            Some(serde_json::to_value(patch).map_err(|_| "Invalid profile fields.")?),
            true,
        )
        .await?,
    )
    .await
}

#[tauri::command]
pub async fn community_delete(state: State<'_, CommunityState>, id: String) -> Result<(), String> {
    let response = community_request(
        &state,
        reqwest::Method::DELETE,
        Some(&id),
        None,
        None,
        None,
        true,
    )
    .await?;
    if response.status() != reqwest::StatusCode::NO_CONTENT {
        return Err("The community API did not confirm deletion.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn community_submit(
    state: State<'_, CommunityState>,
    id: String,
) -> Result<Profile, String> {
    profile_response(
        community_request(
            &state,
            reqwest::Method::POST,
            Some(&id),
            Some("submit"),
            None,
            Some(serde_json::json!({})),
            true,
        )
        .await?,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    fn mock_api(
        responses: Vec<(u16, serde_json::Value)>,
        on_request: impl Fn(usize) + Send + 'static,
    ) -> (Url, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (index, (status, body)) in responses.into_iter().enumerate() {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0, "Incomplete mock request");
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                on_request(index);
                let body = serde_json::to_string(&body).unwrap();
                write!(socket, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (base, worker)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn approved() -> serde_json::Value {
        serde_json::json!({
            "status": "approved", "accessToken": "mock-session-secret",
            "user": { "id": "user-id", "username": "crew", "avatarUrl": "", "roles": [] }
        })
    }

    fn ready_login(state: &CommunityState) {
        state.session.lock().unwrap().pending = Some(PendingLogin {
            device_code: "mock-device-secret".into(),
            expires: Instant::now() + Duration::from_secs(60),
            interval: Duration::from_secs(5),
            next_poll: Instant::now(),
            polling: false,
        });
    }

    fn device(uri: &str) -> DeviceResponse {
        DeviceResponse {
            device_code: "secret-device".into(),
            request_id: "request-123".into(),
            verification_uri: uri.into(),
            expires_in: 60,
            poll_interval: 5,
        }
    }

    #[test]
    fn validates_website_origin_and_never_navigates_device_secret() {
        let website = Url::parse("https://website.example/").unwrap();
        assert!(verification_url(
            &website,
            &device("https://website.example/auth/login?requestId=request-123")
        )
        .is_ok());
        for uri in [
            "https://evil.example/auth/login?requestId=request-123",
            "https://website.example/auth/login?requestId=request-123&deviceCode=secret-device",
            "https://website.example/auth/login?requestId=other",
            "https://user@website.example/auth/login?requestId=request-123",
            "https://website.example/other?requestId=request-123",
        ] {
            assert!(verification_url(&website, &device(uri)).is_err());
        }
    }

    #[test]
    fn configuration_requires_secure_explicit_bases() {
        assert!(configured_url(None, "API").is_err());
        assert!(configured_url(Some("http://remote.example".into()), "API").is_err());
        assert!(configured_url(Some("https://user:secret@example.com".into()), "API").is_err());
        let base = configured_url(Some("http://127.0.0.1:8000/service".into()), "API").unwrap();
        assert_eq!(
            endpoint(&base, "api/auth/desktop").unwrap().as_str(),
            "http://127.0.0.1:8000/service/api/auth/desktop"
        );
        assert_eq!(
            sponsors_url(Some("https://github.com/sponsors/mars-command".into())).unwrap(),
            "https://github.com/sponsors/mars-command/"
        );
        for url in [
            "https://github.com/sponsors/",
            "https://github.com/sponsors/mars-command/extra",
            "https://github.com/not-sponsors/mars-command",
            "https://example.com/sponsors/mars-command",
            "http://github.com/sponsors/mars-command",
        ] {
            assert!(sponsors_url(Some(url.into())).is_err());
        }
    }

    #[test]
    fn partial_patches_reject_null_and_unapproved_fields() {
        let patch: ProfilePatch =
            serde_json::from_value(serde_json::json!({"description": "changed"})).unwrap();
        assert_eq!(
            serde_json::to_value(patch).unwrap(),
            serde_json::json!({"description": "changed"})
        );
        for body in [
            serde_json::json!({"name": null}),
            serde_json::json!({"description": null}),
            serde_json::json!({"mods": null}),
            serde_json::json!({"visibility": "public"}),
            serde_json::json!({"owner": {}}),
        ] {
            assert!(serde_json::from_value::<ProfilePatch>(body).is_err());
        }
    }

    #[test]
    fn frontend_login_payloads_exclude_tokens_and_device_codes() {
        let response = LoginPoll {
            status: PollStatus::Approved,
            user: Some(User {
                id: "id".into(),
                username: "crew".into(),
                avatar_url: "".into(),
                roles: vec![],
            }),
        };
        let value = serde_json::to_value(response).unwrap();
        assert!(value.get("accessToken").is_none());
        assert!(value.get("deviceCode").is_none());
    }

    #[test]
    fn profile_copy_metadata_is_independent_data() {
        let input: ProfileInput = serde_json::from_value(serde_json::json!({"name": "copy", "description": "", "mods": [], "sourceProfileId": "original"})).unwrap();
        assert_eq!(input.source_profile_id.as_deref(), Some("original"));
        assert!(input.mods.is_empty());
        let without_source: ProfileInput = serde_json::from_value(serde_json::json!({
            "name": "draft", "description": "", "mods": []
        }))
        .unwrap();
        assert!(serde_json::to_value(without_source)
            .unwrap()
            .get("sourceProfileId")
            .is_none());
    }

    #[test]
    fn device_login_posts_empty_body_polls_and_keeps_token_in_backend() {
        runtime().block_on(async {
            let state = CommunityState::default();
            let (api, server) = mock_api(vec![
                (200, serde_json::json!({
                    "deviceCode": "mock-device-secret", "requestId": "request-123",
                    "verificationUri": "https://website.example/auth/login?requestId=request-123",
                    "expiresIn": 60, "pollInterval": 5
                })),
                (200, serde_json::json!({"status": "pending"})),
                (200, approved()),
            ], |_| {});
            let start = start_login(&state, &api, &Url::parse("https://website.example/").unwrap()).await.unwrap();
            assert!(!serde_json::to_string(&start).unwrap().contains("mock-device-secret"));
            // Enforce the server-supplied interval without sending an early request.
            assert!(matches!(poll_login(&state, &api).await.unwrap().status, PollStatus::Pending));
            state.session.lock().unwrap().pending.as_mut().unwrap().next_poll = Instant::now();
            assert!(matches!(poll_login(&state, &api).await.unwrap().status, PollStatus::Pending));
            state.session.lock().unwrap().pending.as_mut().unwrap().next_poll = Instant::now();
            let poll = poll_login(&state, &api).await.unwrap();
            assert!(matches!(poll.status, PollStatus::Approved));
            assert!(!serde_json::to_string(&poll).unwrap().contains("mock-session-secret"));
            assert_eq!(state.session.lock().unwrap().token.as_deref(), Some("mock-session-secret"));
            let requests = server.join().unwrap();
            assert!(requests[0].starts_with("POST /api/auth/desktop HTTP/1.1"));
            assert!(requests[0].ends_with("{}"));
            assert!(requests[1].starts_with("POST /api/auth/desktop/poll HTTP/1.1"));
            assert!(requests[1].ends_with("{\"deviceCode\":\"mock-device-secret\"}"));
            clear_session(&state).unwrap();
            assert!(state.session.lock().unwrap().token.is_none());
            assert!(state.session.lock().unwrap().user.is_none());
        });
    }

    #[test]
    fn starting_a_new_login_clears_the_previous_identity_before_transport() {
        runtime().block_on(async {
            let state = CommunityState::default();
            {
                let mut session = state.session.lock().unwrap();
                session.token = Some("old-session".into());
                session.user = Some(User {
                    id: "old-user".into(),
                    username: "old".into(),
                    avatar_url: String::new(),
                    roles: vec![],
                });
            }
            let (api, server) = mock_api(
                vec![(500, serde_json::json!({"detail": "unavailable"}))],
                |_| {},
            );
            assert!(start_login(
                &state,
                &api,
                &Url::parse("https://website.example/").unwrap()
            )
            .await
            .is_err());
            let session = state.session.lock().unwrap();
            assert!(session.token.is_none());
            assert!(session.user.is_none());
            assert!(session.pending.is_none());
            drop(session);
            server.join().unwrap();
        });
    }

    #[test]
    fn cancellation_and_expiry_ignore_late_approvals() {
        runtime().block_on(async {
            for expire in [false, true] {
                let state = Arc::new(CommunityState::default());
                ready_login(&state);
                let server_state = state.clone();
                let (api, server) = mock_api(vec![(200, approved())], move |_| {
                    if expire {
                        server_state
                            .session
                            .lock()
                            .unwrap()
                            .pending
                            .as_mut()
                            .unwrap()
                            .expires = Instant::now();
                    } else {
                        clear_session(&server_state).unwrap();
                    }
                });
                let result = poll_login(&state, &api).await;
                if expire {
                    assert!(matches!(result.unwrap().status, PollStatus::Expired));
                } else {
                    assert_eq!(result.err().as_deref(), Some("Login was cancelled."));
                }
                assert!(state.session.lock().unwrap().token.is_none());
                server.join().unwrap();
            }
        });
    }

    #[test]
    fn denied_expired_and_malformed_poll_responses_fail_closed() {
        runtime().block_on(async {
            for body in [
                serde_json::json!({"status": "denied"}),
                serde_json::json!({"status": "expired"}),
                serde_json::json!({"status": "approved", "accessToken": "mock-secret"}),
                serde_json::json!({"status": "unexpected"}),
            ] {
                let state = CommunityState::default();
                ready_login(&state);
                let (api, server) = mock_api(vec![(200, body)], |_| {});
                let _ = poll_login(&state, &api).await;
                assert!(state.session.lock().unwrap().pending.is_none());
                assert!(state.session.lock().unwrap().token.is_none());
                server.join().unwrap();
            }
            let state = CommunityState::default();
            ready_login(&state);
            state
                .session
                .lock()
                .unwrap()
                .pending
                .as_mut()
                .unwrap()
                .expires = Instant::now();
            // Expired requests do not reach the network.
            let api = Url::parse("http://127.0.0.1:1/").unwrap();
            assert!(matches!(
                poll_login(&state, &api).await.unwrap().status,
                PollStatus::Expired
            ));
        });
    }

    #[test]
    fn public_requests_are_anonymous_and_401_clears_desktop_identity() {
        runtime().block_on(async {
            let state = CommunityState::default();
            {
                let mut session = state.session.lock().unwrap();
                session.token = Some("mock-session-secret".into());
                session.user = Some(User { id: "id".into(), username: "crew".into(), avatar_url: "".into(), roles: vec![] });
            }
            let (api, server) = mock_api(vec![
                (200, serde_json::json!({"profiles": []})),
                (401, serde_json::json!({"detail": "expired"})),
            ], |_| {});
            send_community_request(&state, &api, reqwest::Method::GET, None, None, Some("Mars & Crew"), None, false).await.unwrap();
            let result = send_community_request(&state, &api, reqwest::Method::GET, None, Some("mine"), None, None, true).await;
            assert!(result.err().unwrap().contains("Sign in again"));
            assert!(state.session.lock().unwrap().token.is_none());
            assert!(state.session.lock().unwrap().user.is_none());
            let requests = server.join().unwrap();
            assert!(requests[0].starts_with("GET /api/community/profiles?q=Mars+%26+Crew HTTP/1.1"));
            assert!(!requests[0].to_lowercase().contains("authorization"));
            assert!(requests[1].starts_with("GET /api/community/profiles/mine HTTP/1.1"));
            assert!(requests[1].contains("Bearer mock-session-secret"));
        });
    }

    #[test]
    fn submit_scan_errors_and_string_errors_remain_explicit() {
        runtime().block_on(async {
            let state = CommunityState::default();
            state.session.lock().unwrap().token = Some("mock-session-secret".into());
            let (api, server) = mock_api(vec![
                (409, serde_json::json!({"detail": {"code": "scanning_not_configured", "message": "Submission is unavailable."}})),
                (403, serde_json::json!({"detail": "Owner only."})),
            ], |_| {});
            let result = send_community_request(&state, &api, reqwest::Method::POST, Some("profile-id"), Some("submit"), None, Some(serde_json::json!({})), true).await;
            assert!(result.err().unwrap().contains("scanning_not_configured: Submission is unavailable."));
            let result = send_community_request(&state, &api, reqwest::Method::PATCH, Some("profile-id"), None, None, Some(serde_json::json!({"name": "edited"})), true).await;
            assert!(result.err().unwrap().contains("Owner only."));
            let requests = server.join().unwrap();
            assert!(requests[0].starts_with("POST /api/community/profiles/profile-id/submit HTTP/1.1"));
            assert!(requests[1].starts_with("PATCH /api/community/profiles/profile-id HTTP/1.1"));
        });
    }

    #[test]
    fn auth_errors_never_expose_provider_error_details() {
        runtime().block_on(async {
            let state = CommunityState::default();
            ready_login(&state);
            let (api, server) = mock_api(
                vec![(
                    500,
                    serde_json::json!({"detail": "provider-secret-error-token"}),
                )],
                |_| {},
            );
            let message = poll_login(&state, &api).await.err().unwrap();
            assert!(message.contains("HTTP 500"));
            assert!(!message.contains("provider-secret"));
            assert!(state.session.lock().unwrap().pending.is_none());
            server.join().unwrap();
        });
    }
}
