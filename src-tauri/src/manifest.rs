//! Signed pack manifest: schema, retrieval and Ed25519 verification.
//!
//! The manifest describes which managed files a Mars install should contain.
//! Nothing in this module downloads or mutates game files — it only establishes
//! *what is supposed to be there* and proves the description is authentic.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Base location of `manifest.json` and `manifest.json.sig`.
/// Override at build time with `MARS_MANIFEST_BASE_URL`.
pub const MANIFEST_BASE_URL: &str = match option_env!("MARS_MANIFEST_BASE_URL") {
    Some(url) => url,
    None => "https://api.nexusgit.info/api/v1",
};

/// Hex-encoded Ed25519 public key (32 bytes). Override at build time with
/// `MARS_MANIFEST_PUBLIC_KEY` to rotate without editing source.
/// This is a development key; generate a production pair before release.
pub const MANIFEST_PUBLIC_KEY_HEX: &str = match option_env!("MARS_MANIFEST_PUBLIC_KEY") {
    Some(key) => key,
    None => "2a8186165296c508fde2022fe9568ea3d692570202fc8e95496f1d8f32b8b351",
};

const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// A pack manifest is text; anything larger is a sign of a wrong URL.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileSide {
    Client,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// Forward-slash path relative to the instance root.
    pub path: String,
    /// Lowercase hex SHA-256 of the file contents.
    pub sha256: String,
    pub size: u64,
    #[serde(default = "default_true")]
    pub required: bool,
    /// User-editable files: drift is reported but never treated as corruption.
    #[serde(default)]
    pub mutable: bool,
    #[serde(default = "default_side")]
    pub side: FileSide,
    /// Direct HTTPS source for automated download. Absent for files that
    /// cannot be fetched programmatically.
    #[serde(default)]
    pub download_url: Option<String>,
    /// Set when the author forbids automated distribution (e.g. CurseForge
    /// "distribution: disallowed"); the user must fetch the file themselves.
    #[serde(default)]
    pub manual_download: bool,
    /// Human-facing page to open when `manual_download` is set.
    #[serde(default)]
    pub source_page: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_side() -> FileSide {
    FileSide::Client
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema_version: u32,
    pub pack_version: String,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
    pub generated_at: String,
    /// Directories fully owned by Mars Command; unlisted files inside them
    /// are reported as foreign.
    #[serde(default)]
    pub managed_dirs: Vec<String>,
    pub files: Vec<ManifestFile>,
    /// Mods pinned by CurseForge project/file id. These are immutable pins but
    /// carry no hash, so they are counted rather than content-verified.
    #[serde(default)]
    pub curseforge_mods: Vec<CurseForgeMod>,
    /// Directory the pinned mods install into, when `curseforge_mods` is used.
    #[serde(default)]
    pub mods_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMod {
    pub project_id: u32,
    pub file_id: u32,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub manual_download: bool,
    #[serde(default)]
    pub source_page: Option<String>,
}

/// UI-facing outcome of a manifest refresh. Never carries the file list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestStatus {
    pub available: bool,
    pub signature_valid: bool,
    pub source_url: String,
    pub pack_version: Option<String>,
    pub minecraft_version: Option<String>,
    pub loader_version: Option<String>,
    pub file_count: Option<u32>,
    pub managed_bytes: Option<u64>,
    /// Files the user must fetch themselves because automated distribution
    /// is disallowed upstream.
    pub manual_download_count: Option<u32>,
    pub mod_count: Option<u32>,
    pub generated_at: Option<String>,
    pub fetched_at: String,
    pub error: Option<String>,
}

impl ManifestStatus {
    pub fn failed(error: String) -> Self {
        Self {
            available: false,
            signature_valid: false,
            source_url: MANIFEST_BASE_URL.to_string(),
            pack_version: None,
            minecraft_version: None,
            loader_version: None,
            file_count: None,
            managed_bytes: None,
            manual_download_count: None,
            mod_count: None,
            generated_at: None,
            fetched_at: crate::now_rfc3339(),
            error: Some(error),
        }
    }

    fn from_manifest(manifest: &Manifest) -> Self {
        let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
        let hash_listed_mod_count = manifest
            .files
            .iter()
            .filter(|file| {
                file.path
                    .strip_prefix(&format!("{mods_dir}/"))
                    .is_some_and(|name| {
                        !name.contains('/') && name.to_ascii_lowercase().ends_with(".jar")
                    })
            })
            .count();
        let curseforge_mod_count = manifest
            .curseforge_mods
            .iter()
            .filter(|file| match file.install_dir.as_deref() {
                Some(dir) => dir == mods_dir,
                None => !file.manual_download,
            })
            .count();
        Self {
            available: true,
            signature_valid: true,
            source_url: MANIFEST_BASE_URL.to_string(),
            pack_version: Some(manifest.pack_version.clone()),
            minecraft_version: Some(manifest.minecraft_version.clone()),
            loader_version: Some(manifest.loader_version.clone()),
            file_count: u32::try_from(manifest.files.len()).ok(),
            managed_bytes: Some(
                manifest
                    .files
                    .iter()
                    .map(|file| file.size)
                    .chain(manifest.curseforge_mods.iter().filter_map(|file| file.size))
                    .fold(0u64, u64::saturating_add),
            ),
            manual_download_count: u32::try_from(
                manifest
                    .files
                    .iter()
                    .filter(|file| file.manual_download)
                    .count()
                    + manifest
                        .curseforge_mods
                        .iter()
                        .filter(|file| file.manual_download)
                        .count(),
            )
            .ok(),
            mod_count: u32::try_from(hash_listed_mod_count + curseforge_mod_count).ok(),
            generated_at: Some(manifest.generated_at.clone()),
            fetched_at: crate::now_rfc3339(),
            error: None,
        }
    }
}

/// Rejects anything that could escape the instance root or name a device.
/// Applied to every manifest entry before it is trusted.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 512 {
        return false;
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if path.contains(':') || path.contains('\0') || path.contains('\\') {
        return false;
    }
    path.split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported manifest schema version {}",
            manifest.schema_version
        ));
    }

    for dir in &manifest.managed_dirs {
        if !is_safe_relative_path(dir) {
            return Err(format!("Unsafe managed directory in manifest: {dir}"));
        }
    }

    if let Some(mods_dir) = &manifest.mods_dir {
        if !is_safe_relative_path(mods_dir) {
            return Err(format!("Unsafe mods directory in manifest: {mods_dir}"));
        }
    }

    let mut paths = std::collections::HashSet::new();
    for file in &manifest.files {
        if !is_safe_relative_path(&file.path) {
            return Err(format!("Unsafe file path in manifest: {}", file.path));
        }
        if !paths.insert(file.path.to_ascii_lowercase()) {
            return Err(format!("Duplicate manifest path: {}", file.path));
        }
        if file.sha256.len() != 64 || !file.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("Invalid SHA-256 for {}", file.path));
        }
        // Refuse plaintext sources so a signed manifest can never downgrade a
        // download to an interceptable transport.
        for url in [file.download_url.as_deref(), file.source_page.as_deref()]
            .into_iter()
            .flatten()
        {
            if !url.starts_with("https://") {
                return Err(format!("Non-HTTPS URL for {}", file.path));
            }
        }
    }

    for file in &manifest.curseforge_mods {
        if let Some(install_dir) = &file.install_dir {
            if !is_safe_relative_path(install_dir) {
                return Err(format!(
                    "Unsafe CurseForge install directory: {install_dir}"
                ));
            }
        }
        if let Some(name) = &file.file_name {
            if !is_safe_relative_path(name) || name.contains('/') {
                return Err(format!("Unsafe CurseForge file name: {name}"));
            }
            let install_dir = match file.install_dir.as_deref() {
                Some(dir) => Some(dir),
                None if file.manual_download => None,
                None => manifest.mods_dir.as_deref().or(Some("mods")),
            };
            if let Some(install_dir) = install_dir {
                let path = format!("{install_dir}/{name}");
                if !paths.insert(path.to_ascii_lowercase()) {
                    return Err(format!("Duplicate manifest path: {path}"));
                }
            }
        }
        if let Some(sha1) = &file.sha1 {
            if sha1.len() != 40 || !sha1.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "Invalid SHA-1 for CurseForge file {}",
                    file.file_id
                ));
            }
        }
        for url in [file.download_url.as_deref(), file.source_page.as_deref()]
            .into_iter()
            .flatten()
        {
            if !url.starts_with("https://") {
                return Err(format!(
                    "Non-HTTPS URL for CurseForge file {}",
                    file.file_id
                ));
            }
        }
        if !file.manual_download && file.file_name.is_some() && file.download_url.is_none() {
            return Err(format!(
                "Download URL missing for distributable CurseForge file {}",
                file.file_id
            ));
        }
    }

    Ok(())
}

fn verifying_key() -> Result<VerifyingKey, String> {
    let bytes = hex::decode(MANIFEST_PUBLIC_KEY_HEX)
        .map_err(|_| "Manifest public key is not valid hex".to_string())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Manifest public key must be 32 bytes".to_string())?;
    if bytes.iter().all(|byte| *byte == 0) {
        return Err("No manifest signing key is configured for this build".into());
    }
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| "Manifest public key is not a valid Ed25519 key".into())
}

/// Verifies a detached hex signature over the exact manifest bytes.
pub fn verify_signature(manifest_bytes: &[u8], signature_hex: &str) -> Result<(), String> {
    let key = verifying_key()?;
    let raw = hex::decode(signature_hex.trim())
        .map_err(|_| "Manifest signature is not valid hex".to_string())?;
    let raw: [u8; 64] = raw
        .try_into()
        .map_err(|_| "Manifest signature must be 64 bytes".to_string())?;
    key.verify(manifest_bytes, &Signature::from_bytes(&raw))
        .map_err(|_| "Manifest signature does not match the trusted signing key".into())
}

async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("Request failed: {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Server returned HTTP {}",
            response.status().as_u16()
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("Download failed: {err}"))?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("Manifest exceeds the maximum accepted size".into());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| "Manifest is not valid UTF-8".into())
}

/// Fetches and authenticates the manifest. A manifest that fails signature
/// verification or validation is discarded, never partially applied.
pub async fn fetch_verified() -> Result<(Manifest, ManifestStatus), ManifestStatus> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("mars-command-client/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| ManifestStatus::failed(format!("HTTP client unavailable: {err}")))?;

    let manifest_url = format!("{MANIFEST_BASE_URL}/manifest.json");
    let signature_url = format!("{MANIFEST_BASE_URL}/manifest.json.sig");

    let body = get_text(&client, &manifest_url)
        .await
        .map_err(ManifestStatus::failed)?;
    let signature = get_text(&client, &signature_url)
        .await
        .map_err(ManifestStatus::failed)?;

    // Verify before parsing so malformed input never reaches the deserializer.
    verify_signature(body.as_bytes(), &signature).map_err(|err| {
        let mut status = ManifestStatus::failed(err);
        status.source_url = manifest_url.clone();
        status
    })?;

    let manifest: Manifest = serde_json::from_str(&body)
        .map_err(|err| ManifestStatus::failed(format!("Malformed manifest: {err}")))?;
    validate(&manifest).map_err(ManifestStatus::failed)?;

    let status = ManifestStatus::from_manifest(&manifest);
    Ok((manifest, status))
}

#[cfg(test)]
mod tests {
    use super::{is_safe_relative_path, FileSide, Manifest, ManifestFile, ManifestStatus};

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        assert!(is_safe_relative_path("mods/example.jar"));
        assert!(!is_safe_relative_path("../secrets"));
        assert!(!is_safe_relative_path("mods/../../etc/passwd"));
        assert!(!is_safe_relative_path("/etc/passwd"));
        assert!(!is_safe_relative_path("C:/Windows/System32"));
        assert!(!is_safe_relative_path("mods\\example.jar"));
        assert!(!is_safe_relative_path(""));
    }

    #[test]
    fn counts_hash_listed_jar_files_as_mods() {
        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.2.3".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: vec!["mods".into(), "config".into()],
            files: vec![ManifestFile {
                path: "mods/create.jar".into(),
                sha256: "a".repeat(64),
                size: 1,
                required: true,
                mutable: false,
                side: FileSide::Client,
                download_url: Some("https://example.invalid/create.jar".into()),
                manual_download: false,
                source_page: None,
            }],
            curseforge_mods: Vec::new(),
            mods_dir: Some("mods".into()),
        };

        let status = ManifestStatus::from_manifest(&manifest);
        assert_eq!(status.mod_count, Some(1));
    }
}
