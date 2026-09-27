//! Read-only integrity scan of a local Mars instance against a verified manifest.
//!
//! This milestone intentionally does not repair, download or delete anything —
//! it only reports drift.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::manifest::{is_safe_relative_path, Manifest};

const HASH_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileVerdict {
    Ok,
    Missing,
    /// Present but the contents do not match the manifest hash.
    Corrupt,
    /// Differs from the manifest, but the file is declared user-editable.
    Modified,
    /// Inside a managed directory but absent from the manifest.
    Foreign,
    /// Could not be read (permissions, lock, I/O error).
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDrift {
    pub path: String,
    pub verdict: FileVerdict,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub root: String,
    pub checked_at: String,
    pub pack_version: String,
    pub total_files: u32,
    pub ok_count: u32,
    pub missing_count: u32,
    pub corrupt_count: u32,
    pub modified_count: u32,
    pub foreign_count: u32,
    pub unreadable_count: u32,
    /// Mods pinned by CurseForge id. These carry no hash, so only presence is
    /// compared by count.
    pub mods_expected: u32,
    pub mods_present: u32,
    pub mods_foreign: u32,
    pub mods_fully_verified: bool,
    /// Capped list for display; counts above are always complete.
    pub drift: Vec<FileDrift>,
    pub error: Option<String>,
}

impl IntegrityReport {
    pub fn failed(root: String, pack_version: String, error: String) -> Self {
        Self {
            root,
            checked_at: crate::now_rfc3339(),
            pack_version,
            total_files: 0,
            ok_count: 0,
            missing_count: 0,
            corrupt_count: 0,
            modified_count: 0,
            foreign_count: 0,
            unreadable_count: 0,
            mods_expected: 0,
            mods_present: 0,
            mods_foreign: 0,
            mods_fully_verified: false,
            drift: Vec::new(),
            error: Some(error),
        }
    }
}

/// Number of drift entries returned to the UI. Counts remain exact.
const MAX_REPORTED_DRIFT: usize = 200;

fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_CHUNK_BYTES];
    let mut total: u64 = 0;

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        hasher.update(&buffer[..read]);
    }

    Ok((hex::encode(hasher.finalize()), total))
}

/// Blocking: call from `spawn_blocking`.
pub fn scan(root: &Path, manifest: &Manifest) -> IntegrityReport {
    let root_display = root.display().to_string();
    let pack_version = manifest.pack_version.clone();

    if !root.is_dir() {
        return IntegrityReport::failed(
            root_display,
            pack_version,
            "Instance folder does not exist".into(),
        );
    }

    let mut report = IntegrityReport {
        root: root_display,
        checked_at: crate::now_rfc3339(),
        pack_version,
        total_files: u32::try_from(
            manifest.files.len()
                + manifest
                    .curseforge_mods
                    .iter()
                    .filter(|file| file.required)
                    .count(),
        )
        .unwrap_or(u32::MAX),
        ok_count: 0,
        missing_count: 0,
        corrupt_count: 0,
        modified_count: 0,
        foreign_count: 0,
        unreadable_count: 0,
        mods_expected: u32::try_from(
            manifest
                .curseforge_mods
                .iter()
                .filter(|file| file.required)
                .count(),
        )
        .unwrap_or(u32::MAX),
        mods_present: 0,
        mods_foreign: 0,
        mods_fully_verified: false,
        drift: Vec::new(),
        error: None,
    };

    let mut expected: HashSet<&str> = HashSet::with_capacity(manifest.files.len());

    for entry in &manifest.files {
        // Defence in depth: the manifest was validated, but never join unchecked.
        if !is_safe_relative_path(&entry.path) {
            continue;
        }
        expected.insert(entry.path.as_str());

        let absolute = resolve(root, &entry.path);
        let verdict = match std::fs::metadata(&absolute) {
            Err(_) if entry.required => FileVerdict::Missing,
            Err(_) => FileVerdict::Ok, // optional and absent is a valid state
            Ok(metadata) if !metadata.is_file() => FileVerdict::Corrupt,
            Ok(metadata) if metadata.len() != entry.size && !entry.mutable => FileVerdict::Corrupt,
            Ok(_) => match hash_file(&absolute) {
                Err(_) => FileVerdict::Unreadable,
                Ok((digest, _)) if digest.eq_ignore_ascii_case(&entry.sha256) => FileVerdict::Ok,
                Ok(_) if entry.mutable => FileVerdict::Modified,
                Ok(_) => FileVerdict::Corrupt,
            },
        };

        report.record(entry.path.clone(), verdict);
    }

    scan_mods(root, manifest, &mut report);

    for dir in &manifest.managed_dirs {
        if !is_safe_relative_path(dir) {
            continue;
        }
        collect_foreign(root, dir, &expected, &mut report);
    }

    report
}

impl IntegrityReport {
    fn record(&mut self, path: String, verdict: FileVerdict) {
        match verdict {
            FileVerdict::Ok => {
                self.ok_count += 1;
                return;
            }
            FileVerdict::Missing => self.missing_count += 1,
            FileVerdict::Corrupt => self.corrupt_count += 1,
            FileVerdict::Modified => self.modified_count += 1,
            FileVerdict::Foreign => self.foreign_count += 1,
            FileVerdict::Unreadable => self.unreadable_count += 1,
        }

        if self.drift.len() < MAX_REPORTED_DRIFT {
            self.drift.push(FileDrift { path, verdict });
        }
    }
}

fn resolve(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}

fn count_jars(root: &Path, mods_dir: Option<&str>) -> u32 {
    let Some(dir) = mods_dir.filter(|dir| is_safe_relative_path(dir)) else {
        return 0;
    };
    let base = resolve(root, dir);
    if !base.is_dir() {
        return 0;
    }

    std::fs::read_dir(base)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                })
                .count() as u32
        })
        .unwrap_or(0)
}

fn hash_file_sha1(path: &Path) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "not a regular file",
        ));
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; HASH_CHUNK_BYTES];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn scan_mods(root: &Path, manifest: &Manifest, report: &mut IntegrityReport) {
    let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
    if !is_safe_relative_path(mods_dir) {
        report.error = Some("Unsafe mods directory in manifest".into());
        return;
    }

    let has_complete_pins = manifest
        .curseforge_mods
        .iter()
        .all(|file| !file.required || (file.file_name.is_some() && file.sha1.is_some()));
    report.mods_fully_verified = has_complete_pins;
    let has_any_pin_metadata = manifest
        .curseforge_mods
        .iter()
        .any(|file| file.file_name.is_some() || file.sha1.is_some());
    if !has_any_pin_metadata {
        report.mods_present = count_jars(root, Some(mods_dir));
        report.ok_count += report.mods_present.min(report.mods_expected);
        report.mods_foreign = report.mods_present.saturating_sub(report.mods_expected);
        return;
    }

    let mut expected_names = HashSet::new();
    for file in &manifest.curseforge_mods {
        let Some(file_name) = file.file_name.as_deref() else {
            if file.required {
                report.record(
                    format!("{mods_dir}/unresolved-{}.jar", file.file_id),
                    FileVerdict::Unreadable,
                );
            }
            continue;
        };
        expected_names.insert(file_name.to_ascii_lowercase());
        if !file.required {
            continue;
        }
        let relative = format!("{mods_dir}/{file_name}");
        if !is_safe_relative_path(&relative) {
            report.error = Some(format!("Unsafe CurseForge path: {relative}"));
            continue;
        }

        let path = resolve(root, &relative);
        let Some(expected_hash) = file.sha1.as_deref() else {
            if file.required {
                report.record(relative, FileVerdict::Unreadable);
            }
            continue;
        };
        match std::fs::metadata(&path) {
            Err(_) if file.required => report.record(relative, FileVerdict::Missing),
            Err(_) => {}
            Ok(metadata) if !metadata.is_file() => report.record(relative, FileVerdict::Corrupt),
            Ok(metadata) if file.size.is_some_and(|size| metadata.len() != size) => {
                report.record(relative, FileVerdict::Corrupt)
            }
            Ok(_) => match hash_file_sha1(&path) {
                Ok(hash) if hash.eq_ignore_ascii_case(expected_hash) => {
                    if file.required {
                        report.mods_present += 1;
                    }
                    report.record(relative, FileVerdict::Ok);
                }
                Ok(_) => report.record(relative, FileVerdict::Corrupt),
                Err(_) => report.record(relative, FileVerdict::Unreadable),
            },
        }
    }

    let mods_path = resolve(root, mods_dir);
    let Ok(entries) = std::fs::read_dir(mods_path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if !expected_names.contains(&name) {
            report.mods_foreign += 1;
            report.record(
                format!("{mods_dir}/{}", entry.file_name().to_string_lossy()),
                FileVerdict::Foreign,
            );
        }
    }
}

fn collect_foreign(
    root: &Path,
    managed_dir: &str,
    expected: &HashSet<&str>,
    report: &mut IntegrityReport,
) {
    let base = resolve(root, managed_dir);
    if !base.is_dir() {
        return;
    }

    for entry in WalkDir::new(&base)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        if !expected.contains(relative.as_str()) {
            report.record(relative, FileVerdict::Foreign);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scan;
    use crate::manifest::{CurseForgeMod, Manifest};
    use sha1::{Digest, Sha1};

    fn mod_entry(project_id: u32, file_id: u32, name: &str, content: &[u8]) -> CurseForgeMod {
        CurseForgeMod {
            project_id,
            file_id,
            required: true,
            file_name: Some(name.into()),
            size: Some(content.len() as u64),
            sha1: Some(hex::encode(Sha1::digest(content))),
            download_url: Some("https://example.invalid/mod.jar".into()),
            manual_download: false,
            source_page: None,
        }
    }

    #[test]
    fn exact_pins_reject_wrong_mods_with_matching_jar_count() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("mods")).unwrap();
        std::fs::write(root.path().join("mods/expected-a.jar"), b"mod-a").unwrap();
        std::fs::write(root.path().join("mods/wrong.jar"), b"mod-b").unwrap();

        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: Vec::new(),
            files: Vec::new(),
            curseforge_mods: vec![
                mod_entry(1, 10, "expected-a.jar", b"mod-a"),
                mod_entry(2, 20, "expected-b.jar", b"mod-b"),
            ],
            mods_dir: Some("mods".into()),
        };

        let report = scan(root.path(), &manifest);
        assert_eq!(report.mods_expected, 2);
        assert_eq!(report.mods_present, 1);
        assert_eq!(report.mods_foreign, 1);
        assert_eq!(report.missing_count, 1);
        assert!(report.mods_fully_verified);
    }
}
