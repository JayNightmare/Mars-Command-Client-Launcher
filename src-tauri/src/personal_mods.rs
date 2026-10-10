use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::manifest::Manifest;

pub const MAX_JAR_BYTES: u64 = 64 * 1024 * 1024;
const MAX_METADATA_BYTES: u64 = 256 * 1024;
const MAX_PERSONAL_MODS: usize = 128;
const INVENTORY: &str = ".mars-command/personal-mods.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMod {
    pub file_name: String,
    pub sha256: String,
    pub size: u64,
    pub mod_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub source_path: String,
    pub instance_root: String,
    pub pack_version: String,
    pub file: PersonalMod,
    pub warnings: Vec<String>,
}

fn protect_inventory_path(manifest: &Manifest) -> Result<(), String> {
    if signed_paths(manifest).iter().any(|path| {
        path == ".mars-command" || path == INVENTORY || path.starts_with(&format!("{INVENTORY}/"))
    }) {
        return Err("Signed pack paths overlap the reserved personal mod inventory".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStatus {
    pub file: PersonalMod,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Descriptor {
    mod_loader: String,
    loader_version: String,
    mods: Vec<ModInfo>,
    #[serde(default)]
    dependencies: HashMap<String, Vec<Dependency>>,
    #[serde(skip)]
    has_bundled_jars: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModInfo {
    mod_id: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dependency {
    mod_id: String,
    version_range: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    mandatory: Option<bool>,
    side: Option<String>,
}

fn error(context: &str, err: impl std::fmt::Display) -> String {
    format!("{context}: {err}")
}

fn regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| error("Could not inspect file", e))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("Not a regular file: {}", path.display()));
    }
    Ok(())
}

fn safe_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        });
    !name.is_empty()
        && name.len() <= 180
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && !name.starts_with('.')
        && !reserved
        && name.to_ascii_lowercase().ends_with(".jar")
}

// Refuse redirected ancestors as well as symlinked final files.
pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if !crate::manifest::is_safe_relative_path(relative) {
        return Err("Unsafe personal mod path".into());
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("Refusing redirected path: {}", path.display()));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error("Could not inspect personal mod path", e)),
        }
    }
    Ok(path)
}

pub fn isolated_root(root: &Path, minecraft_root: &Path) -> Result<PathBuf, String> {
    let mars = minecraft_root.join("mars-client");
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid instance version")?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        return Err("Invalid instance version".into());
    }
    let expected = mars.join(name);
    // Canonical equality alone would allow a junction to redirect outside the instance.
    for path in [minecraft_root, mars.as_path(), expected.as_path()] {
        let m = fs::symlink_metadata(path)
            .map_err(|e| error("Set up the isolated Mars instance first", e))?;
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err("The Mars instance must use regular, non-redirected directories".into());
        }
    }
    let actual = fs::canonicalize(root).map_err(|e| error("Could not resolve instance", e))?;
    if actual
        != fs::canonicalize(expected).map_err(|e| error("Could not resolve Mars instance", e))?
    {
        return Err(
            "Personal mods are only supported in .minecraft/mars-client/<pack-version>".into(),
        );
    }
    Ok(actual)
}

pub fn inventory(root: &Path) -> Result<Vec<PersonalMod>, String> {
    let path = safe_path(root, INVENTORY)?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(error("Could not inspect personal mod inventory", e)),
        Ok(m) if m.len() > 1024 * 1024 => return Err("Personal mod inventory is too large".into()),
        Ok(_) => {}
    }
    regular_file(&path)?;
    let entries: Vec<PersonalMod> = serde_json::from_slice(
        &fs::read(path).map_err(|e| error("Could not read personal mod inventory", e))?,
    )
    .map_err(|e| error("Invalid personal mod inventory", e))?;
    let mut names = HashSet::new();
    if entries.len() > MAX_PERSONAL_MODS {
        return Err("A Mars instance supports at most 128 personal mod JARs".into());
    }
    for entry in &entries {
        if !safe_name(&entry.file_name)
            || entry.size == 0
            || entry.size > MAX_JAR_BYTES
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || !names.insert(entry.file_name.to_ascii_lowercase())
        {
            return Err("Invalid or duplicate entry in personal mod inventory".into());
        }
    }
    Ok(entries)
}

fn save_inventory(root: &Path, entries: &[PersonalMod]) -> Result<(), String> {
    let path = safe_path(root, INVENTORY)?;
    fs::create_dir_all(path.parent().ok_or("Missing inventory directory")?)
        .map_err(|e| error("Could not create personal mod inventory directory", e))?;
    let mut staged = NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| error("Could not stage personal mod inventory", e))?;
    staged
        .write_all(
            &serde_json::to_vec_pretty(entries)
                .map_err(|e| error("Could not serialize inventory", e))?,
        )
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| error("Could not write personal mod inventory", e))?;
    staged
        .persist(path)
        .map_err(|e| error("Could not save personal mod inventory", e.error))?;
    Ok(())
}

fn jar_bytes(path: &Path) -> Result<Vec<u8>, String> {
    regular_file(path)?;
    let size = fs::metadata(path)
        .map_err(|e| error("Could not inspect JAR size", e))?
        .len();
    if size == 0 || size > MAX_JAR_BYTES {
        return Err("Mod JAR must be non-empty and no larger than 64 MiB".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| error("Could not open JAR", e))?
        .take(MAX_JAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error("Could not read JAR", e))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_JAR_BYTES {
        return Err("Mod JAR must be non-empty and no larger than 64 MiB".into());
    }
    Ok(bytes)
}

fn descriptor(bytes: &[u8]) -> Result<Descriptor, String> {
    descriptor_reader(std::io::Cursor::new(bytes))
}

fn descriptor_reader(reader: impl Read + std::io::Seek) -> Result<Descriptor, String> {
    let mut jar =
        zip::ZipArchive::new(reader).map_err(|e| error("Not a valid JAR/ZIP archive", e))?;
    if jar.len() > 50_000 {
        return Err("JAR contains too many entries".into());
    }
    let names: Vec<_> = (0..jar.len())
        .map(|index| {
            jar.by_index(index)
                .map(|entry| entry.name().to_owned())
                .map_err(|err| error("Invalid JAR entry", err))
        })
        .collect::<Result<_, _>>()?;
    let metadata_name = if names.iter().any(|n| n == "META-INF/neoforge.mods.toml") {
        "META-INF/neoforge.mods.toml"
    } else if names.iter().any(|n| n == "META-INF/mods.toml") {
        "META-INF/mods.toml"
    } else {
        return Err(
            "Missing NeoForge mod metadata; Fabric/Quilt and plain JARs are not supported".into(),
        );
    };
    if names.iter().filter(|n| *n == metadata_name).count() != 1 {
        return Err("Duplicate mod metadata in JAR".into());
    }
    let mut metadata = jar
        .by_name(metadata_name)
        .map_err(|e| error("Could not read mod metadata", e))?;
    if metadata.size() > MAX_METADATA_BYTES {
        return Err("Mod metadata exceeds 256 KiB".into());
    }
    let mut text = String::new();
    metadata
        .by_ref()
        .take(MAX_METADATA_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| error("Invalid mod metadata", e))?;
    if text.len() as u64 > MAX_METADATA_BYTES {
        return Err("Mod metadata exceeds 256 KiB".into());
    }
    let mut parsed: Descriptor =
        toml::from_str(&text).map_err(|e| error("Invalid NeoForge metadata", e))?;
    parsed.has_bundled_jars = names
        .iter()
        .any(|name| name == "META-INF/jarjar/metadata.json");
    if !matches!(parsed.mod_loader.as_str(), "javafml" | "lowcodefml") || parsed.mods.is_empty() {
        return Err("Unsupported NeoForge mod loader or empty mod list".into());
    }
    let mut ids = HashSet::new();
    for info in &parsed.mods {
        if !info
            .mod_id
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase)
            || info.mod_id.len() < 2
            || info.mod_id.len() > 64
            || !info
                .mod_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            || !ids.insert(&info.mod_id)
            || matches!(info.mod_id.as_str(), "minecraft" | "neoforge" | "forge")
        {
            return Err("Invalid, reserved, or duplicate mod ID".into());
        }
    }
    if metadata_name == "META-INF/mods.toml"
        && !parsed
            .dependencies
            .values()
            .flatten()
            .any(|d| d.mod_id == "neoforge")
    {
        return Err("Forge-only metadata does not declare NeoForge compatibility".into());
    }
    Ok(parsed)
}

pub(crate) fn submission_mod_ids(file: &mut fs::File) -> Result<Vec<String>, String> {
    descriptor_reader(file).map(|metadata| metadata.mods.into_iter().map(|m| m.mod_id).collect())
}

fn numeric_version(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|p| p.parse().ok()).collect()
}

fn compare_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let mut left = numeric_version(left)?;
    let mut right = numeric_version(right)?;
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    Some(left.cmp(&right))
}

// Maven interval unions and exact versions. Qualifiers/expressions are explicitly unknown.
fn range_matches(range: &str, version: &str) -> Option<bool> {
    let range = range.trim();
    if !range.starts_with(['[', '(']) {
        return Some(compare_versions(version, range)? == std::cmp::Ordering::Equal);
    }
    let mut rest = range;
    let mut matched = false;
    loop {
        let end = rest.find([']', ')'])?;
        let interval = &rest[..=end];
        let body = &interval[1..end];
        let matches = if let Some((lower, upper)) = body.split_once(',') {
            let lower = lower.trim();
            let upper = upper.trim();
            let lower_ok = lower.is_empty()
                || match compare_versions(version, lower)? {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Equal => interval.starts_with('['),
                    _ => false,
                };
            let upper_ok = upper.is_empty()
                || match compare_versions(version, upper)? {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Equal => interval.ends_with(']'),
                    _ => false,
                };
            lower_ok && upper_ok
        } else {
            if !interval.starts_with('[') || !interval.ends_with(']') {
                return None;
            }
            compare_versions(version, body.trim())? == std::cmp::Ordering::Equal
        };
        matched |= matches;
        rest = rest[end + 1..].trim();
        if rest.is_empty() {
            return Some(matched);
        }
        rest = rest.strip_prefix(',')?.trim();
        if !rest.starts_with(['[', '(']) {
            return None;
        }
    }
}

fn check_range(
    id: &str,
    range: &str,
    version: &str,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    match range_matches(range, version) {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "{id} requires {range}; this instance uses {version}"
        )),
        None => {
            warnings.push(format!(
                "Cannot conclusively compare {id} range {range} with {version}."
            ));
            Ok(())
        }
    }
}

pub fn signed_paths(manifest: &Manifest) -> HashSet<String> {
    let mods = manifest.mods_dir.as_deref().unwrap_or("mods");
    manifest
        .files
        .iter()
        .map(|f| f.path.to_ascii_lowercase())
        .chain(manifest.curseforge_mods.iter().filter_map(|f| {
            let dir = f
                .install_dir
                .as_deref()
                .or_else(|| (!f.manual_download).then_some(mods))?;
            Some(format!("{dir}/{}", f.file_name.as_ref()?).to_ascii_lowercase())
        }))
        .collect()
}

fn mod_path(root: &Path, manifest: &Manifest, name: &str) -> Result<PathBuf, String> {
    if !safe_name(name) {
        return Err(
            "Use a .jar filename containing only letters, numbers, dots, hyphens and underscores"
                .into(),
        );
    }
    safe_path(
        root,
        &format!("{}/{name}", manifest.mods_dir.as_deref().unwrap_or("mods")),
    )
}

struct InstalledJar {
    name: String,
    hash: Option<String>,
    metadata: Result<Descriptor, String>,
}

fn installed_jars(root: &Path, manifest: &Manifest) -> Result<Vec<InstalledJar>, String> {
    let dir = safe_path(root, manifest.mods_dir.as_deref().unwrap_or("mods"))?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(error("Could not inspect installed mods", err)),
    };
    let mut jars = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| error("Could not inspect installed mod", err))?;
        if !entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let (hash, metadata) = match jar_bytes(&entry.path()) {
            Ok(bytes) => (
                Some(hex::encode(Sha256::digest(&bytes))),
                descriptor(&bytes),
            ),
            Err(err) => (None, Err(err)),
        };
        jars.push(InstalledJar {
            name,
            hash,
            metadata,
        });
    }
    Ok(jars)
}

fn validate_bytes(
    root: &Path,
    manifest: &Manifest,
    name: &str,
    bytes: &[u8],
    own_name: Option<&str>,
    jars: &[InstalledJar],
    entries: &[PersonalMod],
) -> Result<(PersonalMod, Vec<String>), String> {
    validate_bytes_with_fml(root, manifest, name, bytes, own_name, jars, entries, None)
}

fn validate_bytes_with_fml(
    root: &Path,
    manifest: &Manifest,
    name: &str,
    bytes: &[u8],
    own_name: Option<&str>,
    jars: &[InstalledJar],
    entries: &[PersonalMod],
    fml_override: Option<&str>,
) -> Result<(PersonalMod, Vec<String>), String> {
    protect_inventory_path(manifest)?;
    if !manifest.loader.eq_ignore_ascii_case("neoforge")
        || manifest.mods_dir.as_deref().unwrap_or("mods") != "mods"
    {
        return Err(
            "Personal mods require NeoForge and the standard isolated mods directory".into(),
        );
    }
    mod_path(root, manifest, name)?;
    let relative = format!("{}/{name}", manifest.mods_dir.as_deref().unwrap_or("mods"));
    if signed_paths(manifest).contains(&relative.to_ascii_lowercase())
        || crate::sync::installed_managed_paths(root)?.contains(&relative.to_ascii_lowercase())
    {
        return Err(
            "This filename belongs to the signed base pack; personal mods cannot replace it".into(),
        );
    }
    let parsed = descriptor(bytes)?;
    let digest = hex::encode(Sha256::digest(bytes));
    if own_name.is_none() && entries.len() >= MAX_PERSONAL_MODS {
        return Err(
            "This instance already has 128 personal mod JARs; remove one before adding another"
                .into(),
        );
    }
    if entries.iter().any(|e| {
        Some(e.file_name.as_str()) != own_name
            && (e.file_name.eq_ignore_ascii_case(name) || e.sha256 == digest)
    }) {
        return Err("This personal mod filename or checksum is already installed".into());
    }
    let mut installed = HashMap::new();
    let mut existing_dependencies = Vec::new();
    for info in &parsed.mods {
        installed.insert(info.mod_id.clone(), info.version.clone());
    }
    for jar in jars {
        if Some(jar.name.as_str()) == own_name {
            continue;
        }
        if jar.name.eq_ignore_ascii_case(name) {
            return Err("A file with this name already exists; nothing will be overwritten".into());
        }
        if jar.hash.as_deref() == Some(digest.as_str()) {
            return Err("Identical JAR content is already installed under another filename".into());
        }
        let existing = jar
            .metadata
            .as_ref()
            .map_err(|err| format!("Cannot check conflicts with {}: {err}", jar.name))?;
        existing_dependencies.extend(existing.dependencies.values().flatten());
        for info in &existing.mods {
            if parsed
                .mods
                .iter()
                .any(|mod_info| mod_info.mod_id == info.mod_id)
            {
                return Err(format!(
                    "Mod ID {} is already provided by {}",
                    info.mod_id, jar.name
                ));
            }
            installed.insert(info.mod_id.clone(), info.version.clone());
        }
    }
    let mut warnings = vec![
        "Personal mods are unsigned executable code, not approved by Mars. Only install JARs from sources you trust.".into(),
        "Metadata cannot prove client-only behavior or server acceptance. Test locally; the server may reject this mod.".into(),
    ];
    if parsed.has_bundled_jars {
        warnings.push("Bundled Jar-in-Jar modules are not inspected. NeoForge must resolve their dependencies and any additional conflicts at runtime.".into());
    }
    let fml_version = match fml_override {
        Some(version) => Some(version.to_string()),
        None => crate::settings::installed_fml_version(manifest)?,
    };
    match fml_version {
        Some(version) => check_range("FML", &parsed.loader_version, &version, &mut warnings)?,
        None => warnings.push(format!(
            "Cannot determine installed FML version for loader range {}.",
            parsed.loader_version
        )),
    }
    for dep in existing_dependencies {
        if dep.side.as_deref() == Some("SERVER") {
            continue;
        }
        let Some(candidate) = parsed.mods.iter().find(|info| info.mod_id == dep.mod_id) else {
            continue;
        };
        match dep
            .kind
            .as_deref()
            .unwrap_or(if dep.mandatory == Some(false) {
                "optional"
            } else {
                "required"
            }) {
            "required" | "optional" => check_range(
                &dep.mod_id,
                &dep.version_range,
                &candidate.version,
                &mut warnings,
            )?,
            "incompatible"
                if range_matches(&dep.version_range, &candidate.version) != Some(false) =>
            {
                return Err(format!(
                    "An installed mod declares a conflict with {}",
                    candidate.mod_id
                ))
            }
            "discouraged" => warnings.push(format!(
                "An installed mod discourages use with {}",
                candidate.mod_id
            )),
            _ => {}
        }
    }
    let mut declared = HashSet::new();
    for (owner, dependencies) in &parsed.dependencies {
        if !parsed.mods.iter().any(|m| &m.mod_id == owner) {
            return Err(format!(
                "Dependency metadata references unknown mod {owner}"
            ));
        }
        for dep in dependencies {
            let side = dep.side.as_deref().unwrap_or("BOTH");
            if !matches!(side, "BOTH" | "CLIENT" | "SERVER") {
                return Err(format!("Unknown dependency side {side}"));
            }
            if side == "SERVER" {
                continue;
            }
            let kind = dep
                .kind
                .as_deref()
                .unwrap_or(if dep.mandatory == Some(false) {
                    "optional"
                } else {
                    "required"
                });
            if !matches!(
                kind,
                "required" | "optional" | "incompatible" | "discouraged"
            ) {
                return Err(format!("Unknown dependency type {kind}"));
            }
            let version = match dep.mod_id.as_str() {
                "minecraft" => Some(manifest.minecraft_version.as_str()),
                "neoforge" => Some(manifest.loader_version.as_str()),
                "forge" if kind == "required" => {
                    return Err("This mod requires Forge, not NeoForge".into())
                }
                _ => installed.get(&dep.mod_id).map(String::as_str),
            };
            if matches!(dep.mod_id.as_str(), "minecraft" | "neoforge") {
                declared.insert(dep.mod_id.as_str());
            }
            match (kind, version) {
                ("required", None) => {
                    return Err(format!(
                        "Missing required client dependency: {}",
                        dep.mod_id
                    ))
                }
                ("required" | "optional", Some(version)) => {
                    check_range(&dep.mod_id, &dep.version_range, version, &mut warnings)?
                }
                ("incompatible", Some(version)) => match range_matches(&dep.version_range, version)
                {
                    Some(false) => {}
                    _ => {
                        return Err(format!(
                            "Mod declares a conflict with {} {}",
                            dep.mod_id, version
                        ))
                    }
                },
                ("discouraged", Some(_)) => {
                    warnings.push(format!("Mod discourages use with {}", dep.mod_id))
                }
                _ => {}
            }
        }
    }
    for id in ["minecraft", "neoforge"] {
        if !declared.contains(id) {
            warnings.push(format!(
                "No client {id} version constraint is declared; compatibility is unknown."
            ));
        }
    }
    Ok((
        PersonalMod {
            file_name: name.into(),
            sha256: digest,
            size: bytes.len() as u64,
            mod_ids: parsed.mods.into_iter().map(|m| m.mod_id).collect(),
        },
        warnings,
    ))
}

pub(crate) fn capsule_compatibility(
    root: &Path,
    manifest: &Manifest,
    source: &Path,
    fml_version: &str,
) -> Result<(), String> {
    if root.file_name().and_then(|name| name.to_str()) != Some(&manifest.pack_version) {
        return Err("Capsules require the current versioned Mars instance".into());
    }
    let bytes = jar_bytes(source)?;
    let parsed = descriptor(&bytes)?;
    for info in &parsed.mods {
        for (id, version) in [
            ("minecraft", manifest.minecraft_version.as_str()),
            ("neoforge", manifest.loader_version.as_str()),
        ] {
            let supported = parsed
                .dependencies
                .get(&info.mod_id)
                .is_some_and(|dependencies| {
                    dependencies.iter().any(|dependency| {
                        dependency.mod_id == id
                            && dependency.side.as_deref() != Some("SERVER")
                            && dependency.kind.as_deref().unwrap_or(
                                if dependency.mandatory == Some(false) {
                                    "optional"
                                } else {
                                    "required"
                                },
                            ) == "required"
                            && range_matches(&dependency.version_range, version) == Some(true)
                    })
                });
            if !supported {
                return Err(format!(
                    "Capsule mod {} needs an explicit supported client {id} requirement",
                    info.mod_id
                ));
            }
        }
    }
    let (_, warnings) = validate_bytes_with_fml(
        root,
        manifest,
        "capsule.jar",
        &bytes,
        None,
        &installed_jars(root, manifest)?,
        &inventory(root)?,
        Some(fml_version),
    )?;
    if warnings.iter().any(|warning| {
        warning.starts_with("Cannot conclusively")
            || warning.starts_with("No client ")
            || warning.starts_with("Bundled Jar-in-Jar")
    }) {
        return Err("Capsule compatibility is unresolved; explicit supported constraints and no bundled modules are required".into());
    }
    Ok(())
}

pub fn preview(root: &Path, manifest: &Manifest, source: &Path) -> Result<Preview, String> {
    if !manifest.loader.eq_ignore_ascii_case("neoforge") {
        return Err("Personal mods currently require NeoForge".into());
    }
    if manifest.mods_dir.as_deref().unwrap_or("mods") != "mods" {
        return Err("Personal mods currently require the standard isolated mods directory".into());
    }
    if root.file_name().and_then(|n| n.to_str()) != Some(manifest.pack_version.as_str()) {
        return Err(
            "Update the Mars instance to the current signed pack before adding mods".into(),
        );
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid JAR filename")?;
    let bytes = jar_bytes(source)?;
    let (file, warnings) = validate_bytes(
        root,
        manifest,
        name,
        &bytes,
        None,
        &installed_jars(root, manifest)?,
        &inventory(root)?,
    )?;
    Ok(Preview {
        source_path: source.to_string_lossy().into(),
        instance_root: root.to_string_lossy().into(),
        pack_version: manifest.pack_version.clone(),
        file,
        warnings,
    })
}

pub fn install(
    root: &Path,
    manifest: &Manifest,
    source: &Path,
    expected_hash: &str,
    accept_warnings: bool,
) -> Result<(), String> {
    let checked = preview(root, manifest, source)?;
    if checked.file.sha256 != expected_hash {
        return Err("Selected JAR changed since validation; select it again".into());
    }
    if !accept_warnings {
        return Err("Acknowledge the compatibility and trust warnings before installing".into());
    }
    let mut entries = inventory(root)?;
    let path = mod_path(root, manifest, &checked.file.file_name)?;
    fs::create_dir_all(path.parent().ok_or("Missing mods directory")?)
        .map_err(|e| error("Could not create mods directory", e))?;
    let bytes = jar_bytes(source)?;
    if hex::encode(Sha256::digest(&bytes)) != expected_hash {
        return Err("Selected JAR changed during install".into());
    }
    let mut staged = NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|e| error("Could not stage personal mod", e))?;
    staged
        .write_all(&bytes)
        .and_then(|_| staged.as_file().sync_all())
        .map_err(|e| error("Could not stage personal mod", e))?;
    staged.persist_noclobber(&path).map_err(|e| {
        error(
            "Could not install personal mod without overwriting",
            e.error,
        )
    })?;
    entries.push(checked.file);
    if let Err(err) = save_inventory(root, &entries) {
        fs::remove_file(&path).map_err(|e| format!("{err}; rollback failed: {e}"))?;
        return Err(err);
    }
    Ok(())
}

pub fn remove(root: &Path, manifest: &Manifest, name: &str) -> Result<(), String> {
    let mut entries = inventory(root)?;
    let index = entries
        .iter()
        .position(|e| e.file_name == name)
        .ok_or("This file is not a tracked personal mod")?;
    let relative = format!("{}/{name}", manifest.mods_dir.as_deref().unwrap_or("mods"));
    if crate::sync::installed_managed_paths(root)?.contains(&relative.to_ascii_lowercase()) {
        return Err("Refusing to remove a signed base-pack file".into());
    }
    let path = mod_path(root, manifest, name)?;
    let mut removed_bytes = None;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error("Could not inspect personal mod", e)),
        Ok(_) => {
            let bytes = jar_bytes(&path)?;
            let sha256 = hex::encode(Sha256::digest(&bytes));
            let signed_file = manifest.files.iter().any(|file| {
                file.path.eq_ignore_ascii_case(&relative)
                    && file.sha256.eq_ignore_ascii_case(&sha256)
            });
            let signed_pin = manifest.curseforge_mods.iter().any(|file| {
                file.file_name
                    .as_deref()
                    .is_some_and(|file_name| file_name.eq_ignore_ascii_case(name))
                    && file.sha1.as_ref().is_some_and(|hash| {
                        use sha1::Sha1;
                        hash.eq_ignore_ascii_case(&hex::encode(Sha1::digest(&bytes)))
                    })
            });
            if signed_file || signed_pin {
                return Err("Refusing to remove content belonging to the signed base pack".into());
            }
            if hex::encode(Sha256::digest(&bytes)) != entries[index].sha256 {
                return Err(
                    "Personal mod has changed on disk; inspect it manually before removal".into(),
                );
            }
            fs::remove_file(&path).map_err(|e| error("Could not remove personal mod", e))?;
            removed_bytes = Some(bytes);
        }
    }
    entries.remove(index);
    if let Err(err) = save_inventory(root, &entries) {
        if let Some(bytes) = removed_bytes {
            let mut backup = NamedTempFile::new_in(path.parent().ok_or("Missing mods directory")?)
                .map_err(|e| format!("{err}; rollback staging failed: {e}"))?;
            backup
                .write_all(&bytes)
                .and_then(|_| backup.as_file().sync_all())
                .map_err(|e| format!("{err}; rollback write failed: {e}"))?;
            backup
                .persist_noclobber(path)
                .map_err(|e| format!("{err}; rollback failed: {}", e.error))?;
        }
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::manifest::{FileSide, ManifestFile};
    use zip::write::SimpleFileOptions;

    pub(crate) fn manifest(version: &str) -> Manifest {
        Manifest {
            schema_version: 1,
            pack_version: version.into(),
            minecraft_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21.1.250".into(),
            generated_at: String::new(),
            managed_dirs: vec!["mods".into()],
            files: vec![],
            curseforge_mods: vec![],
            mods_dir: Some("mods".into()),
        }
    }

    pub(crate) fn metadata(id: &str, mc: &str, neo: &str) -> String {
        format!(
            r#"modLoader = "javafml"
loaderVersion = "[4,)"
[[mods]]
modId = "{id}"
version = "1.0.0"
[[dependencies.{id}]]
modId = "minecraft"
type = "required"
versionRange = "{mc}"
side = "CLIENT"
[[dependencies.{id}]]
modId = "neoforge"
type = "required"
versionRange = "{neo}"
side = "BOTH"
"#
        )
    }

    pub(crate) fn jar(text: &str) -> Vec<u8> {
        jar_with_name("META-INF/neoforge.mods.toml", text)
    }

    fn jar_with_name(name: &str, text: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    pub(crate) fn signed_file(name: &str, bytes: &[u8]) -> ManifestFile {
        ManifestFile {
            path: format!("mods/{name}"),
            sha256: hex::encode(Sha256::digest(bytes)),
            size: bytes.len() as u64,
            required: true,
            mutable: false,
            side: FileSide::Client,
            download_url: None,
            manual_download: false,
            source_page: None,
        }
    }

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, Manifest) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join(".minecraft")
            .join("mars-client")
            .join("1.0.0");
        fs::create_dir_all(root.join("mods")).unwrap();
        let source = temp.path().join("personal.jar");
        fs::write(&source, jar(&metadata("personal", "[1.21.1]", "[21.1,22)"))).unwrap();
        (temp, root, source, manifest("1.0.0"))
    }

    fn install_fixture(root: &Path, source: &Path, manifest: &Manifest) {
        let preview = preview(root, manifest, source).unwrap();
        install(root, manifest, source, &preview.file.sha256, true).unwrap();
    }

    #[test]
    fn checks_multiple_personal_mods_and_reports_removed_dependencies() {
        let (temp, root, source, manifest) = setup();
        install_fixture(&root, &source, &manifest);
        let dependent = temp.path().join("dependent.jar");
        let metadata = format!(
            "{}\n[[dependencies.dependent]]\nmodId = \"personal\"\ntype = \"required\"\nversionRange = \"[1.0,2)\"\nside = \"CLIENT\"\n",
            metadata("dependent", "[1.21.1]", "[21.1,22)")
        );
        fs::write(&dependent, jar(&metadata)).unwrap();
        install_fixture(&root, &dependent, &manifest);
        let report = statuses(&root, &manifest).unwrap();
        assert_eq!(report.len(), 2);
        assert!(report.iter().all(|entry| entry.status == "installed"));

        remove(&root, &manifest, "personal.jar").unwrap();
        let report = statuses(&root, &manifest).unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].status, "incompatible");
        assert!(report[0].message.as_deref().unwrap().contains("personal"));
    }

    #[test]
    fn maven_ranges_cover_bounds_unions_exact_and_unknown_versions() {
        for (range, version, expected) in [
            ("[1.21.1]", "1.21.1", Some(true)),
            ("[1.21.1]", "1.21", Some(false)),
            ("[21.1,22)", "21.1.250", Some(true)),
            ("[21.1,22)", "22", Some(false)),
            ("(21.1,22]", "21.1", Some(false)),
            ("(21.1,22]", "22", Some(true)),
            ("[4,)", "4.0.42", Some(true)),
            ("(,4)", "4", Some(false)),
            ("[1,2),[3,4]", "3.5", Some(true)),
            ("[1,2),[3,4]", "2.5", Some(false)),
            ("1.0", "1.0.0", Some(true)),
            ("${version}", "1.0", None),
            ("[1.0-beta,2)", "1.0", None),
            ("[1,2", "1", None),
        ] {
            assert_eq!(
                range_matches(range, version),
                expected,
                "{range} / {version}"
            );
        }
    }

    #[test]
    fn validates_type_real_metadata_and_exact_size_limit() {
        let (_temp, root, source, manifest) = setup();
        assert!(preview(&root, &manifest, &source).is_ok());
        let invalid = source.with_extension("exe");
        fs::copy(&source, &invalid).unwrap();
        assert!(preview(&root, &manifest, &invalid)
            .unwrap_err()
            .contains(".jar"));
        for bytes in [
            b"not a zip".to_vec(),
            jar_with_name("fabric.mod.json", "{}"),
            jar("not valid TOML"),
            jar_with_name(
                "META-INF/mods.toml",
                &metadata("personal", "[1.21.1]", "[21.1,22)").replace("neoforge", "forge"),
            ),
            jar(&"x".repeat(MAX_METADATA_BYTES as usize + 1)),
        ] {
            fs::write(&source, bytes).unwrap();
            assert!(preview(&root, &manifest, &source).is_err());
        }
        let file = fs::File::create(&source).unwrap();
        file.set_len(MAX_JAR_BYTES).unwrap();
        assert_eq!(jar_bytes(&source).unwrap().len() as u64, MAX_JAR_BYTES);
        file.set_len(MAX_JAR_BYTES + 1).unwrap();
        assert!(jar_bytes(&source).unwrap_err().contains("64 MiB"));
        file.set_len(0).unwrap();
        assert!(jar_bytes(&source).is_err());
    }

    #[test]
    fn enforces_personal_jar_count_and_reserved_inventory_paths() {
        let (_temp, root, source, mut manifest) = setup();
        let entries: Vec<_> = (0..MAX_PERSONAL_MODS)
            .map(|index| PersonalMod {
                file_name: format!("mod-{index}.jar"),
                sha256: "a".repeat(64),
                size: 1,
                mod_ids: vec![format!("mod_{index}")],
            })
            .collect();
        save_inventory(&root, &entries).unwrap();
        assert_eq!(inventory(&root).unwrap().len(), 128);
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("128"));
        remove(&root, &manifest, "mod-0.jar").unwrap();
        assert!(preview(&root, &manifest, &source).is_ok());
        let mut too_many = entries.clone();
        too_many.push(PersonalMod {
            file_name: "over-limit.jar".into(),
            ..entries[0].clone()
        });
        save_inventory(&root, &too_many).unwrap();
        assert!(inventory(&root).unwrap_err().contains("128"));
        save_inventory(&root, &[]).unwrap();
        let mut reserved = signed_file("reserved.jar", b"signed");
        reserved.path = INVENTORY.into();
        manifest.files.push(reserved);
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("reserved"));
        assert!(protect_sync(&root, &manifest).is_err());
    }

    #[test]
    fn checks_game_loader_dependencies_and_reverse_conflicts() {
        let (_temp, root, source, manifest) = setup();
        for text in [
            metadata("personal", "[1.20,1.21)", "[21.1,22)"),
            metadata("personal", "[1.21.1]", "[22,)"),
            metadata("personal", "[1.21.1]", "[21.1,22)") + "\n[[dependencies.personal]]\nmodId = 'missing'\ntype = 'required'\nversionRange = '[1,)'\nside = 'CLIENT'\n",
        ] {
            fs::write(&source, jar(&text)).unwrap();
            assert!(preview(&root, &manifest, &source).is_err());
        }
        let text = metadata("personal", "${mc}", "[21.1,22)");
        fs::write(&source, jar(&text)).unwrap();
        assert!(preview(&root, &manifest, &source)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("Cannot conclusively")));
        let existing = metadata("existing", "[1.21.1]", "[21.1,22)")
            + "\n[[dependencies.existing]]\nmodId = 'personal'\ntype = 'incompatible'\nversionRange = '[1,)'\nside = 'CLIENT'\n";
        fs::write(root.join("mods/existing.jar"), jar(&existing)).unwrap();
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("conflict"));
    }

    #[test]
    fn rejects_filename_checksum_mod_id_and_signed_path_duplicates() {
        let (_temp, root, source, mut manifest) = setup();
        let bytes = fs::read(&source).unwrap();
        manifest.files.push(signed_file("PERSONAL.jar", &bytes));
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("signed base pack"));
        manifest.files.clear();
        fs::write(root.join("mods/PERSONAL.jar"), &bytes).unwrap();
        assert!(preview(&root, &manifest, &source).is_err());
        fs::remove_file(root.join("mods/PERSONAL.jar")).unwrap();
        fs::write(root.join("mods/renamed.jar"), &bytes).unwrap();
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("Identical"));
        fs::write(
            root.join("mods/renamed.jar"),
            jar(&metadata("personal", "[1.21.1,)", "[21.1,22)")),
        )
        .unwrap();
        assert!(preview(&root, &manifest, &source)
            .unwrap_err()
            .contains("Mod ID"));
    }

    #[test]
    fn installs_only_after_confirmation_and_revalidates_changed_source() {
        let (_temp, root, source, manifest) = setup();
        let checked = preview(&root, &manifest, &source).unwrap();
        assert!(install(&root, &manifest, &source, &checked.file.sha256, false).is_err());
        assert!(!root.join("mods/personal.jar").exists());
        fs::write(
            &source,
            jar(&metadata("personal", "[1.21.1,)", "[21.1,22)")),
        )
        .unwrap();
        assert!(
            install(&root, &manifest, &source, &checked.file.sha256, true)
                .unwrap_err()
                .contains("changed")
        );
        install_fixture(&root, &source, &manifest);
        assert_eq!(inventory(&root).unwrap().len(), 1);
        assert!(preview(&root, &manifest, &source).is_err());
    }

    #[test]
    fn personal_statuses_do_not_inflate_signed_counts_or_allow_foreign_mods() {
        let (_temp, root, source, mut manifest) = setup();
        let base = jar(&metadata("base_mod", "[1.21.1]", "[21.1,22)"));
        fs::write(root.join("mods/base.jar"), &base).unwrap();
        manifest.files.push(signed_file("base.jar", &base));
        install_fixture(&root, &source, &manifest);
        let report = crate::integrity::scan(&root, &manifest, true);
        assert_eq!(
            (
                report.mods_present,
                report.mods_expected,
                report.ok_count,
                report.total_files
            ),
            (1, 1, 1, 1)
        );
        assert_eq!((report.mods_foreign, report.foreign_count), (0, 0));
        assert_eq!(report.personal_mods[0].status, "installed");
        fs::write(
            root.join("mods/foreign.jar"),
            jar(&metadata("foreign_mod", "[1.21.1]", "[21.1,22)")),
        )
        .unwrap();
        assert_eq!(
            crate::integrity::scan(&root, &manifest, true).mods_foreign,
            1
        );
        fs::write(root.join("mods/personal.jar"), b"changed").unwrap();
        let report = crate::integrity::scan(&root, &manifest, true);
        assert_eq!(report.personal_mods[0].status, "changed");
        assert_eq!(report.mods_foreign, 2);
        assert!(remove(&root, &manifest, "personal.jar").is_err());
        fs::remove_file(root.join("mods/personal.jar")).unwrap();
        assert_eq!(statuses(&root, &manifest).unwrap()[0].status, "missing");
        remove(&root, &manifest, "personal.jar").unwrap();
        assert!(inventory(&root).unwrap().is_empty());
        assert_eq!(fs::read(root.join("mods/base.jar")).unwrap(), base);
        assert!(remove(&root, &manifest, "base.jar").is_err());
    }

    #[test]
    fn corrupt_inventory_fails_closed_and_cannot_reclassify_signed_files() {
        let (_temp, root, source, mut manifest) = setup();
        install_fixture(&root, &source, &manifest);
        let bytes = fs::read(&source).unwrap();
        manifest.files.push(signed_file("personal.jar", &bytes));
        assert_eq!(
            statuses(&root, &manifest).unwrap()[0].status,
            "incompatible"
        );
        assert!(remove(&root, &manifest, "personal.jar").is_err());
        fs::write(root.join(INVENTORY), b"invalid").unwrap();
        assert!(crate::integrity::scan(&root, &manifest, true)
            .error
            .is_some());
        assert!(protect_sync(&root, &manifest).is_err());
    }

    #[test]
    fn version_migration_preserves_files_and_reports_new_incompatibility() {
        let (temp, root, source, manifest) = setup();
        install_fixture(&root, &source, &manifest);
        let target = temp.path().join(".minecraft/mars-client/2.0.0");
        fs::create_dir_all(&target).unwrap();
        let mut next = tests::manifest("2.0.0");
        migrate(&root, &target, &next).unwrap();
        migrate(&root, &target, &next).unwrap();
        assert_eq!(
            fs::read(root.join("mods/personal.jar")).unwrap(),
            fs::read(target.join("mods/personal.jar")).unwrap()
        );
        next.minecraft_version = "1.22".into();
        assert_eq!(statuses(&target, &next).unwrap()[0].status, "incompatible");
        remove(&target, &next, "personal.jar").unwrap();
        assert!(root.join("mods/personal.jar").exists());
    }

    #[test]
    fn migration_conflicts_are_non_destructive_and_old_instance_can_remove_mod() {
        let (temp, root, source, manifest) = setup();
        install_fixture(&root, &source, &manifest);
        let target = temp.path().join(".minecraft/mars-client/2.0.0");
        fs::create_dir_all(target.join("mods")).unwrap();
        let mut next = tests::manifest("2.0.0");
        fs::write(target.join("mods/personal.jar"), b"do not overwrite").unwrap();
        assert!(migrate(&root, &target, &next).is_err());
        assert_eq!(
            fs::read(target.join("mods/personal.jar")).unwrap(),
            b"do not overwrite"
        );
        fs::remove_file(target.join("mods/personal.jar")).unwrap();
        next.files
            .push(signed_file("personal.jar", b"signed replacement"));
        assert!(migrate(&root, &target, &next).is_err());
        assert!(root.join("mods/personal.jar").exists());
        remove(&root, &next, "personal.jar").unwrap();
    }

    #[test]
    fn isolated_root_rejects_shared_or_arbitrary_folders() {
        let (temp, root, _source, _manifest) = setup();
        let minecraft = temp.path().join(".minecraft");
        assert!(isolated_root(&root, &minecraft).is_ok());
        fs::create_dir_all(minecraft.join("mods")).unwrap();
        assert!(isolated_root(&minecraft, &minecraft).is_err());
        assert!(isolated_root(&minecraft.join("mods"), &minecraft).is_err());
        assert!(isolated_root(temp.path(), &minecraft).is_err());
        for name in [
            "../evil.jar",
            "mod.jar:evil",
            ".hidden.jar",
            "mod.exe",
            "NUL.jar",
            "COM1.jar",
        ] {
            assert!(!safe_name(name));
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_redirected_mods_and_inventory_directories() {
        use std::os::unix::fs::symlink;
        let (temp, root, source, manifest) = setup();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::remove_dir(root.join("mods")).unwrap();
        symlink(&outside, root.join("mods")).unwrap();
        assert!(preview(&root, &manifest, &source).is_err());
        fs::remove_file(root.join("mods")).unwrap();
        symlink(&outside, root.join(".mars-command")).unwrap();
        assert!(inventory(&root).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_junctions_without_touching_shared_files() {
        let (temp, root, source, manifest) = setup();
        let minecraft = temp.path().join(".minecraft");
        let shared = minecraft.join("mods");
        fs::create_dir(&shared).unwrap();
        fs::write(shared.join("shared.jar"), b"shared sentinel").unwrap();
        fs::remove_dir(root.join("mods")).unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("mods"))
            .arg(&shared)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(preview(&root, &manifest, &source).is_err());
        assert_eq!(
            fs::read(shared.join("shared.jar")).unwrap(),
            b"shared sentinel"
        );
        fs::remove_dir(root.join("mods")).unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(root.join(".mars-command"))
            .arg(&shared)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(inventory(&root).is_err());
        fs::remove_dir(root.join(".mars-command")).unwrap();
    }
}

pub fn statuses(root: &Path, manifest: &Manifest) -> Result<Vec<ModStatus>, String> {
    let entries = inventory(root)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let jars = installed_jars(root, manifest)?;
    entries
        .iter()
        .cloned()
        .map(|file| {
            let path = mod_path(root, manifest, &file.file_name)?;
            let (status, message) = if !path.exists() {
                ("missing", Some("Personal mod is missing from disk".into()))
            } else {
                match jar_bytes(&path) {
                    Err(err) => ("unreadable", Some(err)),
                    Ok(bytes)
                        if hex::encode(Sha256::digest(&bytes)) != file.sha256
                            || bytes.len() as u64 != file.size =>
                    {
                        (
                            "changed",
                            Some("Personal mod differs from its local inventory checksum".into()),
                        )
                    }
                    Ok(bytes) => match validate_bytes(
                        root,
                        manifest,
                        &file.file_name,
                        &bytes,
                        Some(&file.file_name),
                        &jars,
                        &entries,
                    ) {
                        Ok(_) => ("installed", None),
                        Err(err) => ("incompatible", Some(err)),
                    },
                }
            };
            Ok(ModStatus {
                file,
                status: status.into(),
                message,
            })
        })
        .collect()
}

pub fn protect_sync(root: &Path, manifest: &Manifest) -> Result<(), String> {
    protect_inventory_path(manifest)?;
    let signed = signed_paths(manifest);
    let mods = manifest.mods_dir.as_deref().unwrap_or("mods");
    let entries = inventory(root)?;
    if !entries.is_empty() && mods != "mods" {
        return Err("Personal mods cannot be migrated to a nonstandard mods directory".into());
    }
    let installed = if entries.is_empty() {
        HashSet::new()
    } else {
        crate::sync::installed_managed_paths(root)?
    };
    for file in entries {
        if installed.contains(&format!("{mods}/{}", file.file_name).to_ascii_lowercase()) {
            return Err(
                "Personal and signed ownership overlap; update stopped for manual inspection"
                    .into(),
            );
        }
        if signed.contains(&format!("{mods}/{}", file.file_name).to_ascii_lowercase()) {
            return Err(format!(
                "Signed update conflicts with personal mod {}. Remove the personal mod first.",
                file.file_name
            ));
        }
    }
    Ok(())
}

pub fn migrate(source: &Path, target: &Path, manifest: &Manifest) -> Result<(), String> {
    if source == target {
        return protect_sync(target, manifest);
    }
    protect_sync(source, manifest)?;
    protect_sync(target, manifest)?;
    let mut target_entries = inventory(target)?;
    let mut staged = Vec::new();
    for file in inventory(source)? {
        let bytes = jar_bytes(&mod_path(source, manifest, &file.file_name)?)?;
        if hex::encode(Sha256::digest(&bytes)) != file.sha256 || bytes.len() as u64 != file.size {
            return Err(format!(
                "Personal mod {} changed; update stopped without deleting it",
                file.file_name
            ));
        }
        let path = mod_path(target, manifest, &file.file_name)?;
        if target_entries
            .iter()
            .any(|e| e.file_name.eq_ignore_ascii_case(&file.file_name) && e.sha256 == file.sha256)
        {
            if jar_bytes(&path)? == bytes {
                continue;
            }
        }
        if path.exists()
            || target_entries
                .iter()
                .any(|e| e.file_name.eq_ignore_ascii_case(&file.file_name))
        {
            return Err(format!(
                "Personal mod migration conflict: {}",
                file.file_name
            ));
        }
        fs::create_dir_all(path.parent().ok_or("Missing mods directory")?)
            .map_err(|e| error("Could not create mods directory", e))?;
        let mut temp = NamedTempFile::new_in(path.parent().unwrap())
            .map_err(|e| error("Could not stage personal mod migration", e))?;
        temp.write_all(&bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|e| error("Could not copy personal mod", e))?;
        staged.push((path, temp));
        target_entries.push(file);
        if target_entries.len() > MAX_PERSONAL_MODS {
            return Err("Migration would exceed the limit of 128 personal mod JARs".into());
        }
    }
    let mut copied = Vec::new();
    for (path, temp) in staged {
        let result = temp
            .persist_noclobber(&path)
            .map_err(|e| error("Personal mod migration would overwrite a file", e.error));
        if let Err(err) = result {
            for path in copied {
                fs::remove_file(path).map_err(|e| format!("{err}; rollback failed: {e}"))?;
            }
            return Err(err);
        }
        copied.push(path);
    }
    if let Err(err) = save_inventory(target, &target_entries) {
        for path in copied {
            fs::remove_file(path).map_err(|e| format!("{err}; rollback failed: {e}"))?;
        }
        return Err(err);
    }
    Ok(())
}
