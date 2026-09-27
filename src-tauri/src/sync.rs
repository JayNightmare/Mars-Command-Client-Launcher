//! Downloads and applies files from a verified pack manifest.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::manifest::{is_safe_relative_path, CurseForgeMod, Manifest, ManifestFile};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const IO_CHUNK_BYTES: usize = 64 * 1024;
const STATE_DIR: &str = ".mars-command";
const STATE_FILE: &str = "installed-state.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncIssue {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub pack_version: String,
    pub installed_count: u32,
    pub updated_count: u32,
    pub unchanged_count: u32,
    pub removed_count: u32,
    pub conflict_count: u32,
    pub manual_count: u32,
    pub failed_count: u32,
    pub complete: bool,
    pub issues: Vec<SyncIssue>,
    pub error: Option<String>,
}

impl SyncResult {
    fn new(pack_version: String) -> Self {
        Self {
            pack_version,
            installed_count: 0,
            updated_count: 0,
            unchanged_count: 0,
            removed_count: 0,
            conflict_count: 0,
            manual_count: 0,
            failed_count: 0,
            complete: false,
            issues: Vec::new(),
            error: None,
        }
    }

    pub fn failed(pack_version: String, error: String) -> Self {
        let mut result = Self::new(pack_version);
        result.error = Some(error);
        result.failed_count = 1;
        result
    }

    fn issue(&mut self, path: String, reason: impl Into<String>) {
        if self.issues.len() < 200 {
            self.issues.push(SyncIssue {
                path,
                reason: reason.into(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum HashAlgorithm {
    Sha1,
    Sha256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppliedFile {
    hash_algorithm: HashAlgorithm,
    hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct InstalledState {
    instance_root: String,
    pack_version: String,
    managed_roots: Vec<String>,
    files: HashMap<String, AppliedFile>,
}

struct PackFile {
    path: String,
    size: Option<u64>,
    hash_algorithm: HashAlgorithm,
    hash: String,
    url: Option<String>,
    manual: bool,
    required: bool,
    mutable: bool,
}

impl From<&ManifestFile> for PackFile {
    fn from(file: &ManifestFile) -> Self {
        Self {
            path: file.path.clone(),
            size: Some(file.size),
            hash_algorithm: HashAlgorithm::Sha256,
            hash: file.sha256.clone(),
            url: file.download_url.clone(),
            manual: file.manual_download,
            required: file.required,
            mutable: file.mutable,
        }
    }
}

fn mod_pack_file(file: &CurseForgeMod, mods_dir: &str) -> Result<PackFile, String> {
    let file_name = file.file_name.as_deref().ok_or_else(|| {
        format!(
            "CurseForge file {} is not resolved; publish a refreshed manifest",
            file.file_id
        )
    })?;
    if !is_safe_relative_path(file_name) || file_name.contains('/') {
        return Err(format!(
            "Unsafe CurseForge filename for file {}",
            file.file_id
        ));
    }
    let hash = file
        .sha1
        .as_deref()
        .ok_or_else(|| format!("CurseForge file {} has no SHA-1 checksum", file.file_id))?;

    Ok(PackFile {
        path: format!("{mods_dir}/{file_name}"),
        size: file.size,
        hash_algorithm: HashAlgorithm::Sha1,
        hash: hash.to_string(),
        url: file.download_url.clone(),
        manual: file.manual_download,
        required: file.required,
        mutable: false,
    })
}

fn desired_files(manifest: &Manifest) -> Result<Vec<PackFile>, String> {
    let mut desired: Vec<PackFile> = manifest.files.iter().map(PackFile::from).collect();
    if !manifest.curseforge_mods.is_empty() {
        let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
        if !is_safe_relative_path(mods_dir) {
            return Err("Unsafe mods directory in manifest".into());
        }
        for file in &manifest.curseforge_mods {
            desired.push(mod_pack_file(file, mods_dir)?);
        }
    }
    let mut paths = HashSet::with_capacity(desired.len());
    for file in &desired {
        if !is_safe_relative_path(&file.path) {
            return Err(format!("Unsafe path in manifest: {}", file.path));
        }
        if !paths.insert(file.path.to_ascii_lowercase()) {
            return Err(format!("Duplicate path in manifest: {}", file.path));
        }
    }
    Ok(desired)
}

fn roots_for(manifest: &Manifest) -> Vec<String> {
    let mut roots = manifest.managed_dirs.clone();
    if !manifest.curseforge_mods.is_empty() {
        roots.push(manifest.mods_dir.clone().unwrap_or_else(|| "mods".into()));
    }
    roots.sort();
    roots.dedup();
    roots
}

fn is_under_root(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|root| {
        is_safe_relative_path(root) && (path == root || path.starts_with(&format!("{root}/")))
    })
}

fn state_path(root: &Path) -> Result<PathBuf, String> {
    let state_dir = root.join(STATE_DIR);
    match std::fs::symlink_metadata(&state_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("The launcher state directory is not a regular directory".into());
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&state_dir)
                .map_err(|err| format!("Could not create launcher state directory: {err}"))?;
        }
        Err(err) => return Err(format!("Could not inspect launcher state directory: {err}")),
    }
    let path = state_dir.join(STATE_FILE);
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Installed state is not a regular file".into());
        }
    }
    Ok(path)
}

fn read_state(path: &Path) -> InstalledState {
    std::fs::read(path)
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .unwrap_or_default()
}

fn write_state(path: &Path, state: &InstalledState) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(state)
        .map_err(|err| format!("Could not serialise installed state: {err}"))?;
    let mut temporary = NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|err| format!("Could not stage installed state: {err}"))?;
    temporary
        .write_all(&body)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|err| format!("Could not write installed state: {err}"))?;
    temporary
        .persist(path)
        .map_err(|err| format!("Could not replace installed state: {}", err.error))?;
    Ok(())
}

fn destination(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if !is_safe_relative_path(relative) {
        return Err(format!("Unsafe file path: {relative}"));
    }
    let segments: Vec<_> = relative.split('/').collect();
    let mut parent = root.to_path_buf();
    for segment in &segments[..segments.len().saturating_sub(1)] {
        parent.push(segment);
        match std::fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!("Unsafe directory in install path: {relative}"));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&parent)
                    .map_err(|err| format!("Could not create install directory: {err}"))?;
            }
            Err(err) => return Err(format!("Could not inspect install directory: {err}")),
        }
    }
    parent.push(segments[segments.len() - 1]);
    if let Ok(metadata) = std::fs::symlink_metadata(&parent) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("Unsafe destination file: {relative}"));
        }
    }
    Ok(parent)
}

fn hash_path(path: &Path, algorithm: HashAlgorithm) -> Result<Option<(String, u64)>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Could not inspect {}: {err}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }

    let mut file = std::fs::File::open(path)
        .map_err(|err| format!("Could not read {}: {err}", path.display()))?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = vec![0u8; IO_CHUNK_BYTES];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|err| format!("Could not hash {}: {err}", path.display()))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        match algorithm {
            HashAlgorithm::Sha1 => sha1.update(&buffer[..count]),
            HashAlgorithm::Sha256 => sha256.update(&buffer[..count]),
        }
    }
    let digest = match algorithm {
        HashAlgorithm::Sha1 => hex::encode(sha1.finalize()),
        HashAlgorithm::Sha256 => hex::encode(sha256.finalize()),
    };
    Ok(Some((digest, total)))
}

fn download_file(
    client: &reqwest::blocking::Client,
    root: &Path,
    file: &PackFile,
) -> Result<(), String> {
    let url = file
        .url
        .as_deref()
        .ok_or_else(|| "No automatic download URL".to_string())?;
    if !url.starts_with("https://") {
        return Err("Refusing a non-HTTPS download URL".into());
    }
    let target = destination(root, &file.path)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|err| format!("Download request failed: {err}"))?;
    if response.url().scheme() != "https" {
        return Err("Download redirected to a non-HTTPS URL".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Download server returned HTTP {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_FILE_BYTES)
    {
        return Err("Download exceeds the size limit".into());
    }
    if file.size.is_some_and(|size| size > MAX_FILE_BYTES) {
        return Err("Manifest file exceeds the size limit".into());
    }

    let mut temporary = NamedTempFile::new_in(target.parent().unwrap_or(root))
        .map_err(|err| format!("Could not stage download: {err}"))?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = vec![0u8; IO_CHUNK_BYTES];
    let mut total = 0u64;
    loop {
        let count = response
            .read(&mut buffer)
            .map_err(|err| format!("Download stream failed: {err}"))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_FILE_BYTES || file.size.is_some_and(|size| total > size) {
            return Err("Downloaded file exceeded its expected size".into());
        }
        temporary
            .write_all(&buffer[..count])
            .map_err(|err| format!("Could not write staged download: {err}"))?;
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    if file.size.is_some_and(|expected| expected != total) {
        return Err(format!(
            "Downloaded size mismatch: expected {:?}, received {total}",
            file.size
        ));
    }
    let actual_hash = match file.hash_algorithm {
        HashAlgorithm::Sha1 => hex::encode(sha1.finalize()),
        HashAlgorithm::Sha256 => hex::encode(sha256.finalize()),
    };
    if !actual_hash.eq_ignore_ascii_case(&file.hash) {
        return Err("Downloaded file checksum did not match the signed manifest".into());
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|err| format!("Could not flush staged download: {err}"))?;
    temporary
        .persist(&target)
        .map_err(|err| format!("Could not install verified file: {}", err.error))?;
    Ok(())
}

fn sync_blocking(root: &Path, manifest: &Manifest) -> SyncResult {
    let mut result = SyncResult::new(manifest.pack_version.clone());
    if !root.is_dir() {
        return SyncResult::failed(
            manifest.pack_version.clone(),
            "Minecraft game folder does not exist".into(),
        );
    }
    let root = match root.canonicalize() {
        Ok(root) => root,
        Err(err) => {
            return SyncResult::failed(
                manifest.pack_version.clone(),
                format!("Could not open Minecraft game folder: {err}"),
            )
        }
    };
    let desired = match desired_files(manifest) {
        Ok(files) => files,
        Err(err) => return SyncResult::failed(manifest.pack_version.clone(), err),
    };
    let state_path = match state_path(&root) {
        Ok(path) => path,
        Err(err) => return SyncResult::failed(manifest.pack_version.clone(), err),
    };
    let mut previous = read_state(&state_path);
    let canonical_root = root.to_string_lossy().to_string();
    if !previous.instance_root.is_empty() && previous.instance_root != canonical_root {
        previous = InstalledState::default();
    }

    let client = match reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .user_agent(concat!("mars-command-client/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            return SyncResult::failed(
                manifest.pack_version.clone(),
                format!("Download client unavailable: {err}"),
            )
        }
    };

    let mut next_state = InstalledState {
        instance_root: canonical_root,
        pack_version: manifest.pack_version.clone(),
        managed_roots: roots_for(manifest),
        files: previous.files.clone(),
    };
    let mut desired_paths = HashSet::with_capacity(desired.len());

    for file in &desired {
        desired_paths.insert(file.path.clone());
        let target = match destination(&root, &file.path) {
            Ok(target) => target,
            Err(err) => {
                result.failed_count += 1;
                result.issue(file.path.clone(), err);
                continue;
            }
        };
        let current = match hash_path(&target, file.hash_algorithm) {
            Ok(current) => current,
            Err(err) => {
                result.failed_count += 1;
                result.issue(file.path.clone(), err);
                continue;
            }
        };
        if current.as_ref().is_some_and(|(hash, size)| {
            hash.eq_ignore_ascii_case(&file.hash)
                && file.size.is_none_or(|expected| expected == *size)
        }) {
            result.unchanged_count += 1;
            next_state.files.insert(
                file.path.clone(),
                AppliedFile {
                    hash_algorithm: file.hash_algorithm,
                    hash: file.hash.clone(),
                },
            );
            continue;
        }

        if !file.required && current.is_none() {
            next_state.files.remove(&file.path);
            continue;
        }
        if file.manual {
            if file.required {
                result.manual_count += 1;
                result.issue(
                    file.path.clone(),
                    "This file must be installed manually from its source page",
                );
            }
            continue;
        }
        if file.url.is_none() {
            if file.required {
                result.failed_count += 1;
                result.issue(
                    file.path.clone(),
                    "No download URL is present in the signed manifest",
                );
            }
            continue;
        }
        if file.mutable && current.is_some() {
            let unchanged_since_last_sync = previous.files.get(&file.path).is_some_and(|applied| {
                applied.hash_algorithm == file.hash_algorithm
                    && current
                        .as_ref()
                        .is_some_and(|(hash, _)| hash.eq_ignore_ascii_case(&applied.hash))
            });
            if !unchanged_since_last_sync {
                result.conflict_count += 1;
                result.issue(
                    file.path.clone(),
                    "Local config was changed; preserved instead of overwriting",
                );
                continue;
            }
        }

        match download_file(&client, &root, file) {
            Ok(()) => {
                if current.is_some() {
                    result.updated_count += 1;
                } else {
                    result.installed_count += 1;
                }
                next_state.files.insert(
                    file.path.clone(),
                    AppliedFile {
                        hash_algorithm: file.hash_algorithm,
                        hash: file.hash.clone(),
                    },
                );
            }
            Err(err) => {
                result.failed_count += 1;
                result.issue(file.path.clone(), err);
            }
        }
    }

    for (path, prior) in &previous.files {
        if desired_paths.contains(path)
            || !is_safe_relative_path(path)
            || !is_under_root(path, &previous.managed_roots)
        {
            continue;
        }
        let target = match destination(&root, path) {
            Ok(target) => target,
            Err(err) => {
                result.failed_count += 1;
                result.issue(path.clone(), err);
                continue;
            }
        };
        match hash_path(&target, prior.hash_algorithm) {
            Ok(Some((hash, _))) if hash.eq_ignore_ascii_case(&prior.hash) => {
                match std::fs::remove_file(&target) {
                    Ok(()) => {
                        result.removed_count += 1;
                        next_state.files.remove(path);
                    }
                    Err(err) => {
                        result.failed_count += 1;
                        result.issue(
                            path.clone(),
                            format!("Could not remove obsolete managed file: {err}"),
                        );
                    }
                }
            }
            Ok(Some(_)) => {
                result.conflict_count += 1;
                result.issue(
                    path.clone(),
                    "Obsolete file was locally modified; preserved",
                );
            }
            Ok(None) => {
                next_state.files.remove(path);
            }
            Err(err) => {
                result.failed_count += 1;
                result.issue(path.clone(), err);
            }
        }
    }

    next_state.managed_roots = roots_for(manifest);
    if let Err(err) = write_state(&state_path, &next_state) {
        result.failed_count += 1;
        result.issue(STATE_FILE.into(), err);
    }
    result.complete =
        result.failed_count == 0 && result.conflict_count == 0 && result.manual_count == 0;
    result
}

/// Runs file I/O and blocking network reads off the Tauri async executor.
pub async fn sync_pack(root: PathBuf, manifest: Manifest) -> SyncResult {
    tauri::async_runtime::spawn_blocking(move || sync_blocking(&root, &manifest))
        .await
        .unwrap_or_else(|err| SyncResult::failed(String::new(), format!("Sync task failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::FileSide;

    fn config_manifest(version: &str, content: &[u8]) -> Manifest {
        Manifest {
            schema_version: 1,
            pack_version: version.into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: vec!["config".into()],
            files: vec![ManifestFile {
                path: "config/mars.toml".into(),
                sha256: hex::encode(Sha256::digest(content)),
                size: content.len() as u64,
                required: true,
                mutable: true,
                side: FileSide::Client,
                download_url: Some("https://example.invalid/mars.toml".into()),
                manual_download: false,
                source_page: None,
            }],
            curseforge_mods: Vec::new(),
            mods_dir: Some("mods".into()),
        }
    }

    #[test]
    fn matching_config_is_recorded_without_download() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("config")).unwrap();
        std::fs::write(root.path().join("config/mars.toml"), b"approved").unwrap();

        let result = sync_blocking(root.path(), &config_manifest("1.0.0", b"approved"));

        assert!(result.complete, "{result:?}");
        assert_eq!(result.unchanged_count, 1);
        assert_eq!(result.conflict_count, 0);
    }

    #[test]
    fn user_modified_config_is_preserved_as_conflict() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("config")).unwrap();
        std::fs::write(root.path().join("config/mars.toml"), b"approved-v1").unwrap();

        let first = sync_blocking(root.path(), &config_manifest("1.0.0", b"approved-v1"));
        assert!(first.complete, "{first:?}");

        std::fs::write(root.path().join("config/mars.toml"), b"my local edit").unwrap();
        let next = sync_blocking(root.path(), &config_manifest("1.1.0", b"approved-v2"));

        assert_eq!(next.conflict_count, 1);
        assert_eq!(
            std::fs::read(root.path().join("config/mars.toml")).unwrap(),
            b"my local edit"
        );
    }

    #[test]
    fn id_only_curseforge_manifest_is_not_installable() {
        let root = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: Vec::new(),
            files: Vec::new(),
            curseforge_mods: vec![CurseForgeMod {
                project_id: 1,
                file_id: 2,
                required: true,
                file_name: None,
                size: None,
                sha1: None,
                download_url: None,
                manual_download: false,
                source_page: None,
            }],
            mods_dir: Some("mods".into()),
        };

        let result = sync_blocking(root.path(), &manifest);
        assert!(!result.complete);
        assert_eq!(result.failed_count, 1);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("not resolved"));
    }

    #[test]
    fn staged_file_replaces_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("existing.jar");
        std::fs::write(&target, b"old bytes").unwrap();
        let mut staged = NamedTempFile::new_in(directory.path()).unwrap();
        staged.write_all(b"verified bytes").unwrap();
        staged.persist(&target).unwrap();

        assert_eq!(std::fs::read(target).unwrap(), b"verified bytes");
    }
}
