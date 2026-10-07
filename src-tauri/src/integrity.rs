//! Read-only integrity scan of a local Mars instance against a verified manifest.
//!
//! Signed pack verification and personal inventory checks remain separate.
//! Scanning reports drift without repairing, downloading or deleting files.

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
    pub manual_unresolved: u32,
    /// Required mods represented by hash-listed files or CurseForge pins.
    pub mods_expected: u32,
    pub mods_present: u32,
    pub mods_foreign: u32,
    pub mods_fully_verified: bool,
    /// Local inventory checks are never included in signed verification counts.
    pub personal_mods: Vec<crate::personal_mods::ModStatus>,
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
            manual_unresolved: 0,
            mods_expected: 0,
            mods_present: 0,
            mods_foreign: 0,
            mods_fully_verified: false,
            personal_mods: Vec::new(),
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
pub fn scan(root: &Path, manifest: &Manifest, preserve_persistent_data: bool) -> IntegrityReport {
    let root_display = root.display().to_string();
    let pack_version = manifest.pack_version.clone();

    if !root.is_dir() {
        return IntegrityReport::failed(
            root_display,
            pack_version,
            "Instance folder does not exist".into(),
        );
    }

    let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
    let hash_manifest_mods = manifest
        .files
        .iter()
        .filter(|file| file.required && manifest_mod_file_name(&file.path, mods_dir).is_some())
        .count();
    let mut report = IntegrityReport {
        root: root_display,
        checked_at: crate::now_rfc3339(),
        pack_version,
        total_files: u32::try_from(
            manifest.files.len()
                + manifest
                    .curseforge_mods
                    .iter()
                    .filter(|file| {
                        file.required && (file.install_dir.is_some() || !file.manual_download)
                    })
                    .count(),
        )
        .unwrap_or(u32::MAX),
        ok_count: 0,
        missing_count: 0,
        corrupt_count: 0,
        modified_count: 0,
        foreign_count: 0,
        unreadable_count: 0,
        manual_unresolved: 0,
        mods_expected: u32::try_from(hash_manifest_mods).unwrap_or(u32::MAX),
        mods_present: 0,
        mods_foreign: 0,
        mods_fully_verified: true,
        personal_mods: Vec::new(),
        drift: Vec::new(),
        error: None,
    };

    let mut expected: HashSet<String> = HashSet::with_capacity(manifest.files.len());

    for entry in &manifest.files {
        // Defence in depth: the manifest was validated, but never join unchecked.
        if !is_safe_relative_path(&entry.path) {
            continue;
        }
        expected.insert(entry.path.clone());

        let absolute = resolve(root, &entry.path);
        let mutable = entry.mutable
            || (preserve_persistent_data && crate::sync::is_persistent_data_path(&entry.path));
        let verdict = match std::fs::metadata(&absolute) {
            Err(_) if entry.required => FileVerdict::Missing,
            Err(_) => FileVerdict::Ok, // optional and absent is a valid state
            Ok(metadata) if !metadata.is_file() => FileVerdict::Corrupt,
            Ok(metadata) if metadata.len() != entry.size && !mutable => FileVerdict::Corrupt,
            Ok(_) => match hash_file(&absolute) {
                Err(_) => FileVerdict::Unreadable,
                Ok((digest, _)) if digest.eq_ignore_ascii_case(&entry.sha256) => FileVerdict::Ok,
                Ok(_) if mutable => FileVerdict::Modified,
                Ok(_) => FileVerdict::Corrupt,
            },
        };

        if entry.required
            && manifest_mod_file_name(&entry.path, mods_dir).is_some()
            && verdict == FileVerdict::Ok
        {
            report.mods_present += 1;
        }
        report.record(entry.path.clone(), verdict);
    }

    let mut personal_paths = HashSet::new();
    match crate::personal_mods::statuses(root, manifest) {
        Ok(statuses) => {
            for status in &statuses {
                if status.status == "installed" {
                    personal_paths.insert(format!("{mods_dir}/{}", status.file.file_name));
                }
            }
            report.personal_mods = statuses;
        }
        Err(err) => report.error = Some(err),
    }
    scan_curseforge_assets(root, manifest, &mut report, &mut expected, &personal_paths);
    expected.extend(personal_paths);

    let mut managed_dirs = manifest.managed_dirs.clone();
    managed_dirs.extend(manifest.curseforge_mods.iter().filter_map(|file| {
        file.install_dir.clone().or_else(|| {
            (!file.manual_download)
                .then(|| manifest.mods_dir.clone().unwrap_or_else(|| "mods".into()))
        })
    }));
    managed_dirs.sort();
    managed_dirs.dedup();
    for dir in &managed_dirs {
        if !is_safe_relative_path(dir) {
            continue;
        }
        collect_foreign(root, dir, &expected, &mut report);
    }

    report
}

fn manifest_mod_file_name<'a>(path: &'a str, mods_dir: &str) -> Option<&'a str> {
    let name = path.strip_prefix(&format!("{mods_dir}/"))?;
    (!name.contains('/') && name.to_ascii_lowercase().ends_with(".jar")).then_some(name)
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

fn required_mods<'a>(
    manifest: &'a Manifest,
) -> impl Iterator<Item = &'a crate::manifest::CurseForgeMod> {
    let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
    manifest.curseforge_mods.iter().filter(move |file| {
        file.required
            && match file.install_dir.as_deref() {
                Some(dir) => dir == mods_dir,
                None => !file.manual_download,
            }
    })
}

fn scan_curseforge_assets(
    root: &Path,
    manifest: &Manifest,
    report: &mut IntegrityReport,
    expected: &mut HashSet<String>,
    personal_paths: &HashSet<String>,
) {
    let mods_dir = manifest.mods_dir.as_deref().unwrap_or("mods");
    let mod_files: Vec<_> = required_mods(manifest).collect();
    report.mods_expected = report
        .mods_expected
        .saturating_add(u32::try_from(mod_files.len()).unwrap_or(u32::MAX));
    report.mods_fully_verified &= mod_files
        .iter()
        .all(|file| file.file_name.is_some() && file.sha1.is_some());

    let mut expected_mod_names: HashSet<String> = manifest
        .files
        .iter()
        .filter_map(|file| manifest_mod_file_name(&file.path, mods_dir))
        .map(str::to_ascii_lowercase)
        .collect();
    for file in &manifest.curseforge_mods {
        let install_dir = match file.install_dir.as_deref() {
            Some(dir) => Some(dir),
            None if file.manual_download => None,
            None => Some(mods_dir),
        };
        let Some(install_dir) = install_dir else {
            if file.required && file.manual_download {
                report.manual_unresolved += 1;
            }
            continue;
        };
        if !is_safe_relative_path(install_dir) {
            report.error = Some(format!(
                "Unsafe CurseForge install directory: {install_dir}"
            ));
            continue;
        }
        let Some(file_name) = file.file_name.as_deref() else {
            if file.required && file.manual_download {
                report.manual_unresolved += 1;
            } else if file.required {
                report.record(
                    format!("{install_dir}/unresolved-{}.jar", file.file_id),
                    FileVerdict::Unreadable,
                );
            }
            continue;
        };
        let relative = format!("{install_dir}/{file_name}");
        if !is_safe_relative_path(&relative) {
            report.error = Some(format!("Unsafe CurseForge path: {relative}"));
            continue;
        }
        expected.insert(relative.clone());
        if install_dir == mods_dir {
            expected_mod_names.insert(file_name.to_ascii_lowercase());
        }
        if !file.required {
            continue;
        }
        let Some(expected_hash) = file.sha1.as_deref() else {
            if file.required && file.manual_download {
                report.manual_unresolved += 1;
            } else if file.required {
                report.record(relative, FileVerdict::Unreadable);
            }
            continue;
        };
        let path = resolve(root, &relative);
        let verdict = match std::fs::metadata(&path) {
            Err(_) => FileVerdict::Missing,
            Ok(metadata) if !metadata.is_file() => FileVerdict::Corrupt,
            Ok(metadata) if file.size.is_some_and(|size| metadata.len() != size) => {
                FileVerdict::Corrupt
            }
            Ok(_) => match hash_file_sha1(&path) {
                Ok(hash) if hash.eq_ignore_ascii_case(expected_hash) => {
                    if install_dir == mods_dir {
                        report.mods_present += 1;
                    }
                    FileVerdict::Ok
                }
                Ok(_) => FileVerdict::Corrupt,
                Err(_) => FileVerdict::Unreadable,
            },
        };
        report.record(relative, verdict);
    }

    let mods_path = resolve(root, mods_dir);
    if let Ok(entries) = std::fs::read_dir(mods_path) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if !expected_mod_names.contains(&name)
                && !personal_paths
                    .iter()
                    .any(|path| path.eq_ignore_ascii_case(&format!("{mods_dir}/{name}")))
            {
                report.mods_foreign += 1;
            }
        }
    }
}

fn collect_foreign(
    root: &Path,
    managed_dir: &str,
    expected: &HashSet<String>,
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
    use crate::manifest::{CurseForgeMod, FileSide, Manifest, ManifestFile};
    use sha1::{Digest, Sha1};
    use sha2::Sha256;

    fn mod_entry(project_id: u32, file_id: u32, name: &str, content: &[u8]) -> CurseForgeMod {
        CurseForgeMod {
            project_id,
            file_id,
            required: true,
            install_dir: Some("mods".into()),
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

        let report = scan(root.path(), &manifest, false);
        assert_eq!(report.mods_expected, 2);
        assert_eq!(report.mods_present, 1);
        assert_eq!(report.mods_foreign, 1);
        assert_eq!(report.missing_count, 1);
        assert!(report.mods_fully_verified);
    }

    #[test]
    fn verifies_hash_listed_mods_and_reports_unlisted_jars_as_foreign() {
        let root = tempfile::tempdir().unwrap();
        let mods = root.path().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("verified.jar"), b"verified bytes").unwrap();
        std::fs::write(mods.join("corrupt.jar"), b"wrong bytes").unwrap();
        std::fs::write(mods.join("foreign.jar"), b"unlisted bytes").unwrap();

        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: vec!["mods".into()],
            files: vec![
                ManifestFile {
                    path: "mods/verified.jar".into(),
                    sha256: hex::encode(Sha256::digest(b"verified bytes")),
                    size: b"verified bytes".len() as u64,
                    required: true,
                    mutable: false,
                    side: FileSide::Client,
                    download_url: Some("https://example.invalid/verified.jar".into()),
                    manual_download: false,
                    source_page: None,
                },
                ManifestFile {
                    path: "mods/corrupt.jar".into(),
                    sha256: hex::encode(Sha256::digest(b"expected bytes")),
                    size: b"expected bytes".len() as u64,
                    required: true,
                    mutable: false,
                    side: FileSide::Client,
                    download_url: Some("https://example.invalid/corrupt.jar".into()),
                    manual_download: false,
                    source_page: None,
                },
            ],
            curseforge_mods: Vec::new(),
            mods_dir: Some("mods".into()),
        };

        let report = scan(root.path(), &manifest, false);
        assert_eq!(report.mods_expected, 2);
        assert_eq!(report.mods_present, 1);
        assert_eq!(report.mods_foreign, 1);
        assert_eq!(report.corrupt_count, 1);
        assert!(report.mods_fully_verified);
    }

    #[test]
    fn verifies_assets_in_their_category_directory() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("resourcepacks")).unwrap();
        std::fs::write(
            root.path().join("resourcepacks/icons.zip"),
            b"resource pack",
        )
        .unwrap();
        let hash = hex::encode(Sha1::digest(b"resource pack"));
        let mut asset = mod_entry(3, 30, "icons.zip", b"resource pack");
        asset.install_dir = Some("resourcepacks".into());
        asset.sha1 = Some(hash);

        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: vec!["resourcepacks".into()],
            files: Vec::new(),
            curseforge_mods: vec![asset],
            mods_dir: Some("mods".into()),
        };

        let report = scan(root.path(), &manifest, false);
        assert_eq!(report.ok_count, 1);
        assert_eq!(report.mods_expected, 0);
        assert_eq!(report.mods_present, 0);
        assert_eq!(report.manual_unresolved, 0);
    }

    #[test]
    fn leaves_world_specific_data_packs_manual_and_unresolved() {
        let root = tempfile::tempdir().unwrap();
        let mut asset = mod_entry(4, 40, "mars-world.zip", b"world datapack");
        asset.install_dir = None;
        asset.manual_download = true;

        let manifest = Manifest {
            schema_version: 1,
            pack_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: "2026-01-01T00:00:00Z".into(),
            managed_dirs: Vec::new(),
            files: Vec::new(),
            curseforge_mods: vec![asset],
            mods_dir: Some("mods".into()),
        };

        let report = scan(root.path(), &manifest, false);
        assert_eq!(report.manual_unresolved, 1);
        assert_eq!(report.mods_expected, 0);
        assert!(report.mods_fully_verified);
    }
}
