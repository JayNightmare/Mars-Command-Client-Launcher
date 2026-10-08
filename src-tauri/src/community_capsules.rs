//! Local, unsigned community artifact storage, not a backend wire schema.
//!
//! Callers must supply a release reference from authenticated, publication-gated
//! backend metadata, a trusted base manifest and the installed FML version.
//! No existing profile entry proves publication or scan evidence. Consequently
//! this foundation deliberately has no Tauri command, HTTP or launcher wiring.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::{Builder, NamedTempFile};

use crate::{manifest::Manifest, personal_mods};

const STORE: &str = ".mars-command/community-capsules";
const MAX_RECORD_BYTES: u64 = 4096;
const MAX_VERSIONS: usize = 4;
pub const MAX_ARTIFACT_BYTES: u64 = personal_mods::MAX_JAR_BYTES;

/// Local selection, not an asserted server API format. Identity is kept separate
/// from the artifact hash, including when two releases have identical bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReference {
    pub release_id: String,
    pub sha256: String,
}

impl ReleaseReference {
    fn validate(&self) -> Result<(), String> {
        if self.release_id.trim().is_empty()
            || self.release_id.len() > 256
            || self.release_id.chars().any(char::is_control)
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid capsule release identity or lowercase SHA-256".into());
        }
        Ok(())
    }

    fn key(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.release_id.as_bytes());
        digest.update([0]);
        digest.update(self.sha256.as_bytes());
        hex::encode(digest.finalize())
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    current: Option<ReleaseReference>,
    previous: Option<ReleaseReference>,
}

/// Returned only after revalidating the activated artifact against the supplied
/// current base/loader. This is an overlay input for a future isolated launcher,
/// never a request to copy it into the signed instance's mods folder.
#[derive(Debug)]
pub struct ActiveCapsule {
    pub release: ReleaseReference,
    pub artifact: PathBuf,
}

pub struct CapsuleStore {
    root: PathBuf,
    minecraft_root: PathBuf,
}

struct Lock {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Close before removing on Windows.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

fn io_error(err: impl std::fmt::Display) -> String {
    format!("Community capsule storage: {err}")
}

fn read_record<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES
    {
        return Err("Invalid or redirected capsule record".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("Capsule record exceeds its limit".into());
    }
    serde_json::from_slice(&bytes).map_err(io_error)
}

impl CapsuleStore {
    pub fn open(root: &Path, minecraft_root: &Path) -> Result<Self, String> {
        Ok(Self {
            root: personal_mods::isolated_root(root, minecraft_root)?,
            minecraft_root: minecraft_root.to_path_buf(),
        })
    }

    fn directory(&self, manifest: &Manifest) -> Result<PathBuf, String> {
        personal_mods::isolated_root(&self.root, &self.minecraft_root)?;
        let overlaps = |path: &str| {
            let path = path.to_ascii_lowercase();
            path == STORE
                || path.starts_with(&format!("{STORE}/"))
                || STORE.starts_with(&format!("{path}/"))
        };
        if manifest.files.iter().any(|file| overlaps(&file.path))
            || manifest.managed_dirs.iter().any(|dir| overlaps(dir))
            || personal_mods::signed_paths(manifest)
                .iter()
                .any(|path| overlaps(path))
            || crate::sync::installed_managed_paths(&self.root)?
                .iter()
                .any(|path| overlaps(path))
        {
            return Err("Signed base pack overlaps reserved community capsule storage".into());
        }
        let directory = personal_mods::safe_path(&self.root, STORE)?;
        fs::create_dir_all(&directory).map_err(io_error)?;
        if !fs::symlink_metadata(&directory).map_err(io_error)?.is_dir() {
            return Err("Capsule storage is not a directory".into());
        }
        Ok(directory)
    }

    fn lock(&self, directory: &Path) -> Result<Lock, String> {
        let path = directory.join("mutation.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| {
                "Capsule storage is busy or has an interrupted mutation lock".to_string()
            })?;
        Ok(Lock {
            path,
            file: Some(file),
        })
    }

    fn state(&self, directory: &Path) -> Result<Activation, String> {
        let path = personal_mods::safe_path(&self.root, &format!("{STORE}/activation.json"))?;
        match fs::symlink_metadata(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Activation::default()),
            Err(err) => Err(io_error(err)),
            Ok(_) => {
                let state: Activation = read_record(&directory.join("activation.json"))?;
                for release in [&state.current, &state.previous].into_iter().flatten() {
                    release.validate()?;
                }
                if state.previous.is_some() && state.current.is_none() {
                    return Err("Invalid capsule activation state".into());
                }
                Ok(state)
            }
        }
    }

    fn verify(
        &self,
        release: &ReleaseReference,
        manifest: &Manifest,
        fml_version: &str,
    ) -> Result<PathBuf, String> {
        release.validate()?;
        let relative = format!("{STORE}/{}", release.key());
        let dir = personal_mods::safe_path(&self.root, &relative)?;
        let entries = fs::read_dir(&dir)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        if entries.len() != 2
            || entries.iter().any(|entry| {
                entry.file_name() != "capsule.jar" && entry.file_name() != "release.json"
            })
        {
            return Err("Capsule contains unexpected or missing files".into());
        }
        let record = personal_mods::safe_path(&self.root, &format!("{relative}/release.json"))?;
        if read_record::<ReleaseReference>(&record)? != *release {
            return Err("Capsule release identity changed".into());
        }
        let artifact = personal_mods::safe_path(&self.root, &format!("{relative}/capsule.jar"))?;
        verify_hash(&artifact, &release.sha256)?;
        personal_mods::capsule_compatibility(&self.root, manifest, &artifact, fml_version)?;
        Ok(artifact)
    }

    /// Consume at most 64 MiB + one overflow-detection byte. Partial streams,
    /// wrong hashes and incompatible JARs are discarded, never activated.
    pub fn stage(
        &self,
        release: &ReleaseReference,
        stream: impl Read,
        manifest: &Manifest,
        fml_version: &str,
    ) -> Result<(), String> {
        release.validate()?;
        let directory = self.directory(manifest)?;
        let _lock = self.lock(&directory)?;
        self.state(&directory)?;
        let destination =
            personal_mods::safe_path(&self.root, &format!("{STORE}/{}", release.key()))?;
        if destination.exists() {
            return self.verify(release, manifest, fml_version).map(|_| ());
        }
        // Include abandoned staging directories in the quota; fail closed rather
        // than deleting potentially recoverable bytes after an interrupted run.
        let count = fs::read_dir(&directory)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?
            .iter()
            .filter(|entry| {
                entry.file_name() != "activation.json" && entry.file_name() != "mutation.lock"
            })
            .count();
        for entry in fs::read_dir(&directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
                let path =
                    personal_mods::safe_path(&self.root, &format!("{STORE}/{name}/release.json"))?;
                let existing: ReleaseReference = read_record(&path)?;
                existing.validate()?;
                if existing.key() != name {
                    return Err("Stored capsule identity does not match its directory".into());
                }
                if existing.release_id == release.release_id && existing.sha256 != release.sha256 {
                    return Err("An immutable capsule release identity cannot be rebound to different bytes".into());
                }
            }
        }
        if count >= MAX_VERSIONS {
            return Err(
                "Capsule store is full (four retained versions); no files were removed".into(),
            );
        }
        let staged = Builder::new()
            .prefix("staging-")
            .tempdir_in(&directory)
            .map_err(io_error)?;
        let artifact = staged.path().join("capsule.jar");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&artifact)
            .map_err(io_error)?;
        let size =
            std::io::copy(&mut stream.take(MAX_ARTIFACT_BYTES + 1), &mut file).map_err(io_error)?;
        if size == 0 || size > MAX_ARTIFACT_BYTES {
            return Err("Capsule JAR must be non-empty and no larger than 64 MiB".into());
        }
        file.sync_all().map_err(io_error)?;
        drop(file);
        verify_hash(&artifact, &release.sha256)?;
        personal_mods::capsule_compatibility(&self.root, manifest, &artifact, fml_version)?;
        let mut record = File::create(staged.path().join("release.json")).map_err(io_error)?;
        record
            .write_all(&serde_json::to_vec(release).map_err(io_error)?)
            .map_err(io_error)?;
        record.sync_all().map_err(io_error)?;
        drop(record);
        fs::rename(staged.path(), &destination).map_err(|err| {
            format!(
                "Could not publish staged capsule {}: {err}",
                release.release_id
            )
        })?;
        Ok(())
    }

    fn commit(&self, directory: &Path, state: &Activation) -> Result<(), String> {
        personal_mods::safe_path(&self.root, &format!("{STORE}/activation.json"))?;
        let mut staged = NamedTempFile::new_in(directory).map_err(io_error)?;
        staged
            .write_all(&serde_json::to_vec(state).map_err(io_error)?)
            .map_err(io_error)?;
        staged.as_file().sync_all().map_err(io_error)?;
        staged
            .persist(directory.join("activation.json"))
            .map_err(|err| io_error(err.error))?;
        Ok(())
    }

    pub fn activate(
        &self,
        release: &ReleaseReference,
        manifest: &Manifest,
        fml_version: &str,
    ) -> Result<(), String> {
        let directory = self.directory(manifest)?;
        let _lock = self.lock(&directory)?;
        let state = self.state(&directory)?;
        self.verify(release, manifest, fml_version)?;
        if state.current.as_ref() == Some(release) {
            return Ok(());
        }
        if let Some(previous) = &state.current {
            self.verify(previous, manifest, fml_version)?;
        }
        self.commit(
            &directory,
            &Activation {
                current: Some(release.clone()),
                previous: state.current,
            },
        )
    }

    pub fn rollback(&self, manifest: &Manifest, fml_version: &str) -> Result<(), String> {
        let directory = self.directory(manifest)?;
        let _lock = self.lock(&directory)?;
        let state = self.state(&directory)?;
        let previous = state
            .previous
            .ok_or("No prior capsule is available for explicit rollback")?;
        self.verify(&previous, manifest, fml_version)?;
        // A damaged current release must not prevent recovery to verified bytes.
        let current = state
            .current
            .filter(|current| self.verify(current, manifest, fml_version).is_ok());
        self.commit(
            &directory,
            &Activation {
                current: Some(previous),
                previous: current,
            },
        )
    }

    pub fn active(
        &self,
        manifest: &Manifest,
        fml_version: &str,
    ) -> Result<Option<ActiveCapsule>, String> {
        let directory = self.directory(manifest)?;
        let _lock = self.lock(&directory)?;
        let state = self.state(&directory)?;
        state
            .current
            .map(|release| {
                let artifact = self.verify(&release, manifest, fml_version)?;
                Ok(ActiveCapsule { release, artifact })
            })
            .transpose()
    }
}

fn verify_hash(path: &Path, expected: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Capsule artifact is not a regular file".into());
    }
    let mut stream = File::open(path)
        .map_err(io_error)?
        .take(MAX_ARTIFACT_BYTES + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let read = stream.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        digest.update(&buffer[..read]);
    }
    if size == 0 || size > MAX_ARTIFACT_BYTES || hex::encode(digest.finalize()) != expected {
        return Err("Capsule SHA-256 or size verification failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::personal_mods::tests::{jar, manifest, metadata, signed_file};
    use std::io::{self, Cursor};

    struct Fixture {
        _directory: tempfile::TempDir,
        store: CapsuleStore,
        manifest: Manifest,
        bytes: Vec<u8>,
        shared_mod: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = Builder::new()
                .prefix("capsule-test-")
                .tempdir_in(env!("CARGO_MANIFEST_DIR"))
                .unwrap();
            let minecraft = directory.path().join(".minecraft");
            let root = minecraft.join("mars-client").join("1.0.0");
            fs::create_dir_all(root.join("mods")).unwrap();
            fs::create_dir_all(minecraft.join("mods")).unwrap();
            let shared_mod = minecraft.join("mods").join("untouched.jar");
            fs::write(&shared_mod, b"shared bytes").unwrap();
            let base = jar(&metadata("base", "[1.21.1]", "[21.1,22)"));
            fs::write(root.join("mods").join("base.jar"), &base).unwrap();
            let mut manifest = manifest("1.0.0");
            manifest.files.push(signed_file("base.jar", &base));
            Self {
                store: CapsuleStore::open(&root, &minecraft).unwrap(),
                _directory: directory,
                manifest,
                bytes: jar(&metadata("community", "[1.21.1]", "[21.1,22)")),
                shared_mod,
            }
        }

        fn release(&self, id: &str) -> ReleaseReference {
            ReleaseReference {
                release_id: id.into(),
                sha256: hex::encode(Sha256::digest(&self.bytes)),
            }
        }

        fn stage(&self, release: &ReleaseReference) {
            self.store
                .stage(release, Cursor::new(&self.bytes), &self.manifest, "4")
                .unwrap_or_else(|err| panic!("Staging {}: {err}", release.release_id));
        }

        fn activate(&self, release: &ReleaseReference) {
            self.store.activate(release, &self.manifest, "4").unwrap();
        }

        fn active(&self) -> Option<ActiveCapsule> {
            self.store.active(&self.manifest, "4").unwrap()
        }

        fn activation_bytes(&self) -> Vec<u8> {
            fs::read(self.store.root.join(STORE).join("activation.json")).unwrap()
        }
    }

    #[test]
    fn staging_is_inactive_and_activation_and_rollback_are_persistent_and_isolated() {
        let fixture = Fixture::new();
        let first = fixture.release("author-a/release-1");
        let second = fixture.release("author-b/release-2");
        let base_path = fixture.store.root.join("mods").join("base.jar");
        let base_before = fs::read(&base_path).unwrap();
        fixture.stage(&first);
        assert!(fixture.active().is_none());
        fixture.activate(&first);
        fixture.stage(&second);
        assert_eq!(fixture.active().unwrap().release, first);
        fixture.activate(&second);
        assert_eq!(fixture.active().unwrap().release, second);
        assert_ne!(
            first.key(),
            second.key(),
            "identical bytes do not merge release identities"
        );
        let reopened =
            CapsuleStore::open(&fixture.store.root, &fixture.store.minecraft_root).unwrap();
        reopened.rollback(&fixture.manifest, "4").unwrap();
        assert_eq!(
            reopened
                .active(&fixture.manifest, "4")
                .unwrap()
                .unwrap()
                .release,
            first
        );
        assert_eq!(fs::read(base_path).unwrap(), base_before);
        assert_eq!(fs::read(&fixture.shared_mod).unwrap(), b"shared bytes");
        assert!(!fixture.store.root.join("mods").join("capsule.jar").exists());
        assert!(!fixture
            .store
            .root
            .join(".mars-command")
            .join("personal-mods.json")
            .exists());
    }

    #[test]
    fn rejects_wrong_hash_empty_and_malformed_jars_without_publishing_staging() {
        let fixture = Fixture::new();
        let release = fixture.release("selected");
        for bytes in [
            Vec::new(),
            b"not a jar".to_vec(),
            jar(&metadata("other", "[1.21.1]", "[21.1,22)")),
        ] {
            assert!(fixture
                .store
                .stage(&release, Cursor::new(bytes), &fixture.manifest, "4")
                .is_err());
            assert!(fixture.active().is_none());
            assert!(!fixture.store.root.join(STORE).join(release.key()).exists());
        }
        let malformed = ReleaseReference {
            release_id: "malformed".into(),
            sha256: hex::encode(Sha256::digest(b"not a jar")),
        };
        assert!(fixture
            .store
            .stage(
                &malformed,
                Cursor::new(b"not a jar"),
                &fixture.manifest,
                "4"
            )
            .is_err());
        assert_eq!(
            fs::read_dir(fixture.store.root.join(STORE))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn stream_overflow_and_interruption_are_bounded_and_leave_prior_activation_unchanged() {
        let fixture = Fixture::new();
        let first = fixture.release("first");
        fixture.stage(&first);
        fixture.activate(&first);
        let before = fixture.activation_bytes();
        let next = fixture.release("next");
        let mut endless = io::repeat(1);
        assert!(fixture
            .store
            .stage(&next, &mut endless, &fixture.manifest, "4")
            .is_err());
        struct Interrupted;
        impl Read for Interrupted {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "interrupted fixture",
                ))
            }
        }
        assert!(fixture
            .store
            .stage(&next, Interrupted, &fixture.manifest, "4")
            .is_err());
        assert_eq!(fixture.activation_bytes(), before);
        assert_eq!(
            fs::read_dir(fixture.store.root.join(STORE))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn blocks_incompatible_unknown_fml_and_missing_required_dependencies() {
        let fixture = Fixture::new();
        let valid = metadata("community", "[1.21.1]", "[21.1,22)");
        for text in [
            metadata("community", "[1.20.1]", "[21.1,22)"),
            metadata("community", "[1.21.1]", "[22,)"),
            metadata("community", "unknown", "[21.1,22)"),
            "modLoader=\"javafml\"\nloaderVersion=\"[4,)\"\n[[mods]]\nmodId=\"community\"\nversion=\"1\"".into(),
            format!("{valid}\n[[dependencies.community]]\nmodId=\"absent\"\ntype=\"required\"\nversionRange=\"[1,)\"\n"),
            metadata("base", "[1.21.1]", "[21.1,22)"),
        ] {
            let bytes = jar(&text);
            let release = ReleaseReference { release_id: "bad-compatibility".into(), sha256: hex::encode(Sha256::digest(&bytes)) };
            assert!(fixture.store.stage(&release, Cursor::new(bytes), &fixture.manifest, "4").is_err());
        }
        assert!(fixture
            .store
            .stage(
                &fixture.release("no-fml"),
                Cursor::new(&fixture.bytes),
                &fixture.manifest,
                ""
            )
            .is_err());
    }

    #[test]
    fn rechecks_staged_bytes_active_bytes_and_current_compatibility() {
        let fixture = Fixture::new();
        let first = fixture.release("first");
        fixture.stage(&first);
        fixture.activate(&first);
        let before = fixture.activation_bytes();
        let second = fixture.release("second");
        fixture.stage(&second);
        let artifact = fixture
            .store
            .root
            .join(STORE)
            .join(second.key())
            .join("capsule.jar");
        fs::write(&artifact, b"tampered").unwrap();
        assert!(fixture
            .store
            .activate(&second, &fixture.manifest, "4")
            .is_err());
        assert_eq!(fixture.activation_bytes(), before);
        let mut incompatible = fixture.manifest.clone();
        incompatible.minecraft_version = "1.22.0".into();
        assert!(fixture.store.active(&incompatible, "4").is_err());
        let active = fixture.active().unwrap();
        fs::write(active.artifact, b"tampered").unwrap();
        assert!(fixture.store.active(&fixture.manifest, "4").is_err());
    }

    #[test]
    fn rollback_revalidates_prior_and_recovers_even_when_current_is_damaged() {
        let fixture = Fixture::new();
        let first = fixture.release("first");
        let second = fixture.release("second");
        fixture.stage(&first);
        fixture.activate(&first);
        assert!(fixture.store.rollback(&fixture.manifest, "4").is_err());
        fixture.stage(&second);
        fixture.activate(&second);
        fs::write(fixture.active().unwrap().artifact, b"corrupted").unwrap();
        fixture.store.rollback(&fixture.manifest, "4").unwrap();
        assert_eq!(fixture.active().unwrap().release, first);
        assert!(fixture.store.rollback(&fixture.manifest, "4").is_err());
        fixture.stage(&fixture.release("third"));
        fixture.activate(&fixture.release("third"));
        let before = fixture.activation_bytes();
        fs::write(
            fixture
                .store
                .root
                .join(STORE)
                .join(first.key())
                .join("capsule.jar"),
            b"corrupted",
        )
        .unwrap();
        assert!(fixture.store.rollback(&fixture.manifest, "4").is_err());
        assert_eq!(fixture.activation_bytes(), before);
    }

    #[test]
    fn capacity_is_bounded_and_existing_identity_cannot_be_rebound() {
        let fixture = Fixture::new();
        let original = fixture.release("release-0");
        fixture.stage(&original);
        let changed_bytes = jar(&metadata("community", "[1.21.1]", "[21.1,23)"));
        let rebound = ReleaseReference {
            release_id: original.release_id.clone(),
            sha256: hex::encode(Sha256::digest(&changed_bytes)),
        };
        assert!(fixture
            .store
            .stage(&rebound, Cursor::new(changed_bytes), &fixture.manifest, "4")
            .is_err());
        for index in 0..MAX_VERSIONS {
            fixture.stage(&fixture.release(&format!("release-{index}")));
        }
        let first = fixture.release("release-0");
        fixture.stage(&first);
        assert!(fixture
            .store
            .stage(
                &fixture.release("fifth"),
                Cursor::new(&fixture.bytes),
                &fixture.manifest,
                "4"
            )
            .is_err());
        let record = fixture
            .store
            .root
            .join(STORE)
            .join(first.key())
            .join("release.json");
        fs::write(
            record,
            serde_json::to_vec(&fixture.release("another-owner")).unwrap(),
        )
        .unwrap();
        assert!(fixture
            .store
            .activate(&first, &fixture.manifest, "4")
            .is_err());
    }

    #[test]
    fn an_actual_version_update_keeps_previous_bytes_for_rollback() {
        let mut fixture = Fixture::new();
        let first = fixture.release("community/1.0");
        let original_bytes = fixture.bytes.clone();
        fixture.stage(&first);
        fixture.activate(&first);
        fixture.bytes =
            jar(&metadata("community", "[1.21.1]", "[21.1,22)").replace("1.0.0", "2.0.0"));
        let second = fixture.release("community/2.0");
        assert_ne!(first.sha256, second.sha256);
        fixture.stage(&second);
        fixture.activate(&second);
        assert_eq!(
            fs::read(fixture.active().unwrap().artifact).unwrap(),
            fixture.bytes
        );
        fixture.store.rollback(&fixture.manifest, "4").unwrap();
        assert_eq!(
            fs::read(fixture.active().unwrap().artifact).unwrap(),
            original_bytes
        );
    }

    #[test]
    fn rejects_bad_state_extra_payload_files_and_reserved_base_paths() {
        let mut fixture = Fixture::new();
        let first = fixture.release("first");
        fixture.stage(&first);
        let directory = fixture.store.root.join(STORE);
        fs::write(directory.join("activation.json"), b"invalid").unwrap();
        assert!(fixture.store.active(&fixture.manifest, "4").is_err());
        assert!(fixture
            .store
            .activate(&first, &fixture.manifest, "4")
            .is_err());
        fs::remove_file(directory.join("activation.json")).unwrap();
        fs::write(directory.join(first.key()).join("extra.jar"), b"unexpected").unwrap();
        assert!(fixture
            .store
            .activate(&first, &fixture.manifest, "4")
            .is_err());
        fixture.manifest.managed_dirs.push(".mars-command".into());
        assert!(fixture
            .store
            .stage(
                &fixture.release("next"),
                Cursor::new(&fixture.bytes),
                &fixture.manifest,
                "4"
            )
            .is_err());
    }

    #[test]
    fn commit_failure_preserves_prior_record_and_exclusive_lock_blocks_mutations() {
        let fixture = Fixture::new();
        let first = fixture.release("first");
        fixture.stage(&first);
        fixture.activate(&first);
        let before = fixture.activation_bytes();
        let directory = fixture.store.directory(&fixture.manifest).unwrap();
        let lock = fixture.store.lock(&directory).unwrap();
        assert!(fixture
            .store
            .activate(&first, &fixture.manifest, "4")
            .is_err());
        drop(lock);
        assert!(fixture
            .store
            .commit(&directory.join("nonexistent"), &Activation::default())
            .is_err());
        assert_eq!(fixture.activation_bytes(), before);
        fixture.activate(&first);
    }

    #[test]
    fn selection_paths_are_never_used_as_filesystem_paths_and_shared_roots_are_rejected() {
        let fixture = Fixture::new();
        let reference = fixture.release("../../author/release");
        fixture.stage(&reference);
        fixture.activate(&reference);
        assert!(fixture
            .active()
            .unwrap()
            .artifact
            .starts_with(fixture.store.root.join(STORE)));
        assert!(
            CapsuleStore::open(&fixture.store.minecraft_root, &fixture.store.minecraft_root)
                .is_err()
        );
        for id in ["", " ", "\n", &"a".repeat(257)] {
            assert!(fixture
                .store
                .stage(
                    &fixture.release(id),
                    Cursor::new(&fixture.bytes),
                    &fixture.manifest,
                    "4"
                )
                .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn redirected_storage_and_artifacts_are_rejected() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let first = fixture.release("first");
        fixture.stage(&first);
        fixture.activate(&first);
        let artifact = fixture.active().unwrap().artifact;
        fs::remove_file(&artifact).unwrap();
        symlink(&fixture.shared_mod, &artifact).unwrap();
        assert!(fixture.store.active(&fixture.manifest, "4").is_err());
        fs::remove_dir_all(fixture.store.root.join(STORE)).unwrap();
        symlink(
            fixture.shared_mod.parent().unwrap(),
            fixture.store.root.join(STORE),
        )
        .unwrap();
        assert!(fixture
            .store
            .stage(&first, Cursor::new(&fixture.bytes), &fixture.manifest, "4")
            .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_redirected_store_is_rejected() {
        let fixture = Fixture::new();
        let directory = fixture.store.directory(&fixture.manifest).unwrap();
        fs::remove_dir(&directory).unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&directory)
            .arg(fixture.shared_mod.parent().unwrap())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(fixture
            .store
            .stage(
                &fixture.release("redirected"),
                Cursor::new(&fixture.bytes),
                &fixture.manifest,
                "4"
            )
            .is_err());
        // Remove the junction itself, never recursively traverse its target.
        fs::remove_dir(directory).unwrap();
        assert_eq!(fs::read(&fixture.shared_mod).unwrap(), b"shared bytes");
    }
}
