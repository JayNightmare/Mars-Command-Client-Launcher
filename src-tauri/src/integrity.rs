//! Read-only integrity scan of a local Mars instance against a verified manifest.
//!
//! This milestone intentionally does not repair, download or delete anything —
//! it only reports drift.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
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
            drift: Vec::new(),
            error: Some(error),
        }
    }
}

/// Number of drift entries returned to the UI. Counts remain exact.
const MAX_REPORTED_DRIFT: usize = 200;

fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
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
        total_files: u32::try_from(manifest.files.len()).unwrap_or(u32::MAX),
        ok_count: 0,
        missing_count: 0,
        corrupt_count: 0,
        modified_count: 0,
        foreign_count: 0,
        unreadable_count: 0,
        mods_expected: u32::try_from(manifest.curseforge_mods.len()).unwrap_or(u32::MAX),
        mods_present: count_jars(root, manifest.mods_dir.as_deref()),
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
            Ok(metadata) if metadata.len() != entry.size && !entry.mutable => {
                FileVerdict::Corrupt
            }
            Ok(_) => match hash_file(&absolute) {
                Err(_) => FileVerdict::Unreadable,
                Ok((digest, _)) if digest.eq_ignore_ascii_case(&entry.sha256) => FileVerdict::Ok,
                Ok(_) if entry.mutable => FileVerdict::Modified,
                Ok(_) => FileVerdict::Corrupt,
            },
        };

        report.record(entry.path.clone(), verdict);
    }

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
                    entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                })
                .count() as u32
        })
        .unwrap_or(0)
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
