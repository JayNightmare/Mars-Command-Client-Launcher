//! Small JSON-backed client settings in the app config directory.

use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, fs, io::Read, io::Write};

use fastnbt::Value as NbtValue;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const MARS_SERVER_NAME: &str = "Mars Command";
const MARS_SERVER_HOST: &str = "play.nexusgit.info";
const MARS_SERVER_PORT: u16 = 25565;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientSettings {
    /// Absolute path to the managed Mars instance chosen by the user.
    pub instance_root: Option<String>,
    pub preserve_persistent_data: bool,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            instance_root: None,
            preserve_persistent_data: true,
        }
    }
}

pub fn is_minecraft_game_dir(path: &std::path::Path) -> bool {
    path.is_dir()
        && ["mods", "config", "kubejs", "defaultconfigs"]
            .iter()
            .any(|name| path.join(name).is_dir())
}

fn launcher_minecraft_root() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|app_data| app_data.join(".minecraft"));
    }

    #[cfg(target_os = "linux")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".minecraft"));
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// Finds an installed Mars Client profile under CurseForge's default directory.
pub fn detect_curseforge_mars_instance() -> Option<PathBuf> {
    let user_home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let instances_dir = PathBuf::from(user_home)
        .join("curseforge")
        .join("minecraft")
        .join("Instances");

    find_curseforge_mars_instance(&instances_dir)
}

pub fn detect_mars_launcher_instance() -> Option<PathBuf> {
    let minecraft_root = launcher_minecraft_root()?;
    let mars_root = minecraft_root.join("mars-client").canonicalize().ok()?;
    let bytes = fs::read(minecraft_root.join("launcher_profiles.json")).ok()?;
    let profiles: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let profiles = profiles.get("profiles")?.as_object()?;

    profiles
        .values()
        .filter_map(|profile| {
            let name = profile.get("name")?.as_str()?;
            if !name.to_ascii_lowercase().starts_with("mars client ") {
                return None;
            }
            let path = PathBuf::from(profile.get("gameDir")?.as_str()?)
                .canonicalize()
                .ok()?;
            if !path.starts_with(&mars_root) || !is_minecraft_game_dir(&path) {
                return None;
            }
            Some((
                profile
                    .get("lastUsed")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                path,
            ))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, path)| path)
}

fn find_curseforge_mars_instance(instances_dir: &std::path::Path) -> Option<PathBuf> {
    let mut profiles: Vec<_> = std::fs::read_dir(instances_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    profiles.sort();

    for profile in profiles {
        let manifest_name = std::fs::read_to_string(profile.join("manifest.json"))
            .ok()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
            .and_then(|manifest| {
                let is_modpack = manifest
                    .get("manifestType")
                    .and_then(serde_json::Value::as_str)
                    == Some("minecraftModpack");
                is_modpack
                    .then(|| manifest.get("name")?.as_str().map(str::to_owned))
                    .flatten()
            });
        let profile_name = profile
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let is_mars_client = [Some(profile_name), manifest_name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .any(|name| {
                name.eq_ignore_ascii_case("Mars Client") || name.eq_ignore_ascii_case("Mars")
            });
        if !is_mars_client {
            continue;
        }

        let game_dir = profile.join("minecraft");
        if is_minecraft_game_dir(&game_dir) {
            return Some(game_dir);
        }
        if is_minecraft_game_dir(&profile) {
            return Some(profile);
        }
    }

    None
}

pub fn setup_minecraft_installation(
    pack_version: &str,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
) -> Result<PathBuf, String> {
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (pack_version, minecraft_version, loader, loader_version);
        return Err(
            "Minecraft Launcher profile setup is only supported on Windows and Linux.".into(),
        );
    }

    #[cfg(any(windows, target_os = "linux"))]
    {
        #[cfg(windows)]
        ensure_minecraft_launcher_closed()?;
        let minecraft_root = launcher_minecraft_root()
            .ok_or_else(|| "Minecraft Launcher profile directory is unavailable".to_string())?;
        let version_id = format!("{loader}-{loader_version}");
        register_minecraft_profile(
            &minecraft_root,
            pack_version,
            minecraft_version,
            &version_id,
        )
    }
}

pub fn mars_launcher_profile_version(game_dir: &std::path::Path) -> Option<String> {
    let profile_path = launcher_minecraft_root()?.join("launcher_profiles.json");
    let bytes = fs::read(profile_path).ok()?;
    let launcher_profiles = serde_json::from_slice::<serde_json::Value>(&bytes).ok()?;
    let expected_game_dir = game_dir.canonicalize().ok()?;
    let profiles = launcher_profiles
        .get("profiles")
        .and_then(serde_json::Value::as_object)?;

    profiles.values().find_map(|profile| {
        let is_mars_profile = profile["name"]
            .as_str()
            .is_some_and(|name| name.to_ascii_lowercase().starts_with("mars client "));
        let registered_game_dir = profile["gameDir"]
            .as_str()
            .and_then(|path| PathBuf::from(path).canonicalize().ok());
        (is_mars_profile && registered_game_dir.as_ref() == Some(&expected_game_dir))
            .then(|| profile["lastVersionId"].as_str().map(str::to_owned))
            .flatten()
    })
}

fn register_minecraft_profile(
    minecraft_root: &std::path::Path,
    pack_version: &str,
    minecraft_version: &str,
    version_id: &str,
) -> Result<PathBuf, String> {
    if !safe_component(pack_version) || !safe_component(version_id) {
        return Err("Pack or loader version contains unsafe characters".into());
    }

    let version_metadata_path = minecraft_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    let version_metadata = fs::read(&version_metadata_path).map_err(|err| {
        format!("NeoForge version {version_id} is not installed in Minecraft Launcher: {err}")
    })?;
    let version_metadata: serde_json::Value = serde_json::from_slice(&version_metadata)
        .map_err(|err| format!("Installed NeoForge metadata is invalid: {err}"))?;
    if version_metadata["id"].as_str() != Some(version_id)
        || version_metadata["inheritsFrom"].as_str() != Some(minecraft_version)
    {
        return Err(format!(
            "Installed version {version_id} does not match Minecraft {minecraft_version}"
        ));
    }

    let profile_path = minecraft_root.join("launcher_profiles.json");
    let mut launcher_profiles: serde_json::Value = serde_json::from_slice(
        &fs::read(&profile_path)
            .map_err(|err| format!("Could not read launcher profile settings: {err}"))?,
    )
    .map_err(|err| format!("Launcher profile settings are invalid JSON: {err}"))?;
    let profiles = launcher_profiles
        .get_mut("profiles")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| "Launcher profile settings have no profiles object".to_string())?;

    let profile_id = format!("mars-client-{pack_version}");
    let profile_name = format!("Mars Client {pack_version}");
    let game_dir = minecraft_root.join("mars-client").join(pack_version);
    let game_dir_text = game_dir.to_string_lossy().to_string();
    if let Some(existing) = profiles.get(&profile_id) {
        if existing["name"].as_str() != Some(&profile_name)
            || existing["gameDir"].as_str() != Some(&game_dir_text)
        {
            return Err(format!(
                "Launcher profile ID {profile_id} is already used by another installation"
            ));
        }
        if existing["lastVersionId"].as_str() == Some(version_id)
            && game_dir.is_dir()
            && existing.get("javaArgs").is_some()
        {
            ensure_mars_server_in_list(&game_dir)?;
            return Ok(game_dir);
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut profile = profiles
        .remove(&profile_id)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    profile.insert("created".into(), serde_json::Value::String(now.clone()));
    profile.insert("gameDir".into(), serde_json::Value::String(game_dir_text));
    profile.insert("icon".into(), serde_json::Value::String(String::new()));
    profile.insert("lastUsed".into(), serde_json::Value::String(now));
    profile.insert(
        "lastVersionId".into(),
        serde_json::Value::String(version_id.to_string()),
    );
    profile.insert("name".into(), serde_json::Value::String(profile_name));
    profile
        .entry("javaArgs")
        .or_insert_with(|| serde_json::Value::String("-Xmx6G".into()));
    profile.insert("type".into(), serde_json::Value::String("custom".into()));
    profiles.insert(profile_id, serde_json::Value::Object(profile));

    fs::create_dir_all(&game_dir)
        .map_err(|err| format!("Could not create isolated game directory: {err}"))?;
    let serialized = serde_json::to_vec_pretty(&launcher_profiles)
        .map_err(|err| format!("Could not serialize launcher profiles: {err}"))?;
    write_file_atomically(&profile_path, &serialized, "launcher_profiles")?;
    ensure_mars_server_in_list(&game_dir)?;

    Ok(game_dir)
}

fn ensure_mars_server_in_list(game_dir: &std::path::Path) -> Result<(), String> {
    let path = game_dir.join("servers.dat");
    let mut root = match fs::read(&path) {
        Ok(compressed) => {
            let mut decoder = GzDecoder::new(compressed.as_slice());
            let mut nbt = Vec::new();
            decoder
                .read_to_end(&mut nbt)
                .map_err(|err| format!("Could not decompress Minecraft server list: {err}"))?;
            fastnbt::from_bytes::<HashMap<String, NbtValue>>(&nbt)
                .map_err(|err| format!("Minecraft server list contains invalid NBT: {err}"))?
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(err) => return Err(format!("Could not read Minecraft server list: {err}")),
    };

    let servers = root
        .entry("servers".to_string())
        .or_insert_with(|| NbtValue::List(Vec::new()));
    let NbtValue::List(servers) = servers else {
        return Err("Minecraft server list has an invalid servers tag".into());
    };

    let server_ip = format!("{MARS_SERVER_HOST}:{MARS_SERVER_PORT}");
    let mut changed = false;
    let existing = servers.iter_mut().find_map(|entry| {
        let NbtValue::Compound(fields) = entry else {
            return None;
        };
        let name_matches = fields
            .get("name")
            .and_then(NbtValue::as_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(MARS_SERVER_NAME));
        let ip_matches = fields
            .get("ip")
            .and_then(NbtValue::as_str)
            .is_some_and(|ip| {
                ip.eq_ignore_ascii_case(MARS_SERVER_HOST) || ip.eq_ignore_ascii_case(&server_ip)
            });
        (name_matches || ip_matches).then_some(fields)
    });

    if let Some(fields) = existing {
        changed |= set_nbt_string(fields, "name", MARS_SERVER_NAME);
        changed |= set_nbt_string(fields, "ip", &server_ip);
    } else {
        let mut fields = HashMap::new();
        fields.insert("name".into(), NbtValue::String(MARS_SERVER_NAME.into()));
        fields.insert("ip".into(), NbtValue::String(server_ip));
        servers.push(NbtValue::Compound(fields));
        changed = true;
    }

    if !changed {
        return Ok(());
    }

    let nbt = fastnbt::to_bytes(&root)
        .map_err(|err| format!("Could not encode Minecraft server list: {err}"))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&nbt)
        .map_err(|err| format!("Could not compress Minecraft server list: {err}"))?;
    let compressed = encoder
        .finish()
        .map_err(|err| format!("Could not finish Minecraft server list: {err}"))?;
    write_file_atomically(&path, &compressed, "servers")
}

fn set_nbt_string(fields: &mut HashMap<String, NbtValue>, key: &str, value: &str) -> bool {
    if fields.get(key) == Some(&NbtValue::String(value.to_string())) {
        return false;
    }
    fields.insert(key.into(), NbtValue::String(value.into()));
    true
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}

#[cfg(windows)]
fn ensure_minecraft_launcher_closed() -> Result<(), String> {
    for image_name in ["Minecraft.exe", "MinecraftLauncher.exe"] {
        let output = Command::new("tasklist")
            .arg("/FI")
            .arg(format!("IMAGENAME eq {image_name}"))
            .args(["/FO", "CSV", "/NH"])
            .output()
            .map_err(|err| format!("Could not check whether Minecraft Launcher is open: {err}"))?;
        if !output.status.success() {
            return Err("Could not check whether Minecraft Launcher is open".into());
        }
        let result = String::from_utf8_lossy(&output.stdout);
        if result
            .to_ascii_lowercase()
            .contains(&image_name.to_ascii_lowercase())
        {
            return Err("Close Minecraft Launcher before setting up the Mars installation.".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn write_file_atomically(
    path: &std::path::Path,
    contents: &[u8],
    temporary_prefix: &str,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = path.with_file_name(format!("{temporary_prefix}.mars-{suffix}.tmp"));
    let write_result = (|| {
        let mut temporary = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|err| format!("Could not create temporary launcher profile: {err}"))?;
        temporary
            .write_all(contents)
            .map_err(|err| format!("Could not write temporary launcher profile: {err}"))?;
        temporary
            .sync_all()
            .map_err(|err| format!("Could not flush temporary launcher profile: {err}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    let source: Vec<u16> = temporary_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        let error = std::io::Error::last_os_error();
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "Could not update launcher profiles safely: {error}"
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn write_file_atomically(
    path: &std::path::Path,
    contents: &[u8],
    temporary_prefix: &str,
) -> Result<(), String> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = path.with_file_name(format!("{temporary_prefix}.mars-{suffix}.tmp"));
    let mut temporary = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|err| format!("Could not create temporary launcher profile: {err}"))?;
    temporary
        .write_all(contents)
        .map_err(|err| format!("Could not write temporary launcher profile: {err}"))?;
    temporary
        .sync_all()
        .map_err(|err| format!("Could not flush temporary launcher profile: {err}"))?;
    fs::rename(&temporary_path, path)
        .map_err(|err| format!("Could not update launcher profiles safely: {err}"))
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|err| format!("Config directory unavailable: {err}"))?;
    Ok(dir.join("settings.json"))
}

/// Missing or corrupt settings fall back to defaults rather than failing.
pub fn load(app: &AppHandle) -> ClientSettings {
    let Ok(path) = settings_path(app) else {
        return ClientSettings::default();
    };
    let Ok(body) = std::fs::read_to_string(path) else {
        return ClientSettings::default();
    };
    serde_json::from_str(&body).unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &ClientSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Could not create config directory: {err}"))?;
    }
    let body = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Could not serialise settings: {err}"))?;
    std::fs::write(&path, body).map_err(|err| format!("Could not write settings: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{ensure_mars_server_in_list, MARS_SERVER_HOST, MARS_SERVER_NAME, MARS_SERVER_PORT};
    use super::{find_curseforge_mars_instance, register_minecraft_profile, ClientSettings};
    use fastnbt::Value as NbtValue;
    use flate2::read::GzDecoder;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::collections::HashMap;
    use std::io::{Read, Write};

    fn decode_servers_dat(path: &std::path::Path) -> HashMap<String, NbtValue> {
        let compressed = std::fs::read(path).unwrap();
        assert_eq!(&compressed[..2], &[0x1f, 0x8b]);
        let mut decoder = GzDecoder::new(compressed.as_slice());
        let mut nbt = Vec::new();
        decoder.read_to_end(&mut nbt).unwrap();
        fastnbt::from_bytes(&nbt).unwrap()
    }

    fn encode_servers_dat(root: &HashMap<String, NbtValue>) -> Vec<u8> {
        let nbt = fastnbt::to_bytes(root).unwrap();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&nbt).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn missing_persistent_data_setting_defaults_to_preserve() {
        let settings: ClientSettings = serde_json::from_str(r#"{"instanceRoot":null}"#).unwrap();

        assert!(settings.preserve_persistent_data);
        assert!(ClientSettings::default().preserve_persistent_data);
    }

    #[test]
    fn detects_official_mars_profile_and_uses_its_isolated_game_directory() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("Mars Client");
        let game_dir = profile.join("minecraft");
        std::fs::create_dir_all(game_dir.join("mods")).unwrap();
        std::fs::write(
            profile.join("manifest.json"),
            r#"{"manifestType":"minecraftModpack","name":"Mars"}"#,
        )
        .unwrap();

        assert_eq!(find_curseforge_mars_instance(root.path()), Some(game_dir));
    }

    #[test]
    fn updates_mars_server_and_preserves_other_nbt_entries() {
        let root = tempfile::tempdir().unwrap();
        let server_list_path = root.path().join("servers.dat");
        let mut other_server = HashMap::new();
        other_server.insert("name".into(), NbtValue::String("Other Server".into()));
        other_server.insert("ip".into(), NbtValue::String("other.example:25566".into()));
        other_server.insert("custom".into(), NbtValue::Int(42));
        let other_server = NbtValue::Compound(other_server);
        let mut mars_server = HashMap::new();
        mars_server.insert("name".into(), NbtValue::String(MARS_SERVER_NAME.into()));
        mars_server.insert("ip".into(), NbtValue::String("old.example".into()));
        mars_server.insert("custom".into(), NbtValue::String("preserve me".into()));
        let mars_server = NbtValue::Compound(mars_server);
        let mut original = HashMap::new();
        original.insert(
            "servers".into(),
            NbtValue::List(vec![other_server.clone(), mars_server]),
        );
        original.insert("unrelatedRootTag".into(), NbtValue::Long(123));
        std::fs::write(&server_list_path, encode_servers_dat(&original)).unwrap();

        ensure_mars_server_in_list(root.path()).unwrap();

        let updated = decode_servers_dat(&server_list_path);
        let NbtValue::List(servers) = &updated["servers"] else {
            panic!("servers tag must remain a list");
        };
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0], other_server);
        let NbtValue::Compound(mars_server) = &servers[1] else {
            panic!("Mars entry must remain a compound");
        };
        assert_eq!(
            mars_server["name"],
            NbtValue::String(MARS_SERVER_NAME.into())
        );
        assert_eq!(
            mars_server["ip"],
            NbtValue::String(format!("{MARS_SERVER_HOST}:{MARS_SERVER_PORT}"))
        );
        assert_eq!(
            mars_server["custom"],
            NbtValue::String("preserve me".into())
        );
        assert_eq!(updated["unrelatedRootTag"], NbtValue::Long(123));

        let first_write = std::fs::read(&server_list_path).unwrap();
        ensure_mars_server_in_list(root.path()).unwrap();
        assert_eq!(std::fs::read(&server_list_path).unwrap(), first_write);
    }

    #[test]
    fn creates_mars_server_in_missing_server_list_only_once() {
        let root = tempfile::tempdir().unwrap();
        let server_list_path = root.path().join("servers.dat");

        ensure_mars_server_in_list(root.path()).unwrap();
        let first_write = std::fs::read(&server_list_path).unwrap();
        ensure_mars_server_in_list(root.path()).unwrap();

        let servers_dat = decode_servers_dat(&server_list_path);
        let NbtValue::List(servers) = &servers_dat["servers"] else {
            panic!("servers tag must be a list");
        };
        assert_eq!(servers.len(), 1);
        let NbtValue::Compound(server) = &servers[0] else {
            panic!("Mars entry must be a compound");
        };
        assert_eq!(server["name"], NbtValue::String(MARS_SERVER_NAME.into()));
        assert_eq!(
            server["ip"],
            NbtValue::String(format!("{MARS_SERVER_HOST}:{MARS_SERVER_PORT}"))
        );
        assert_eq!(std::fs::read(&server_list_path).unwrap(), first_write);
    }

    #[test]
    fn registers_isolated_profile_without_changing_existing_launcher_data() {
        let root = tempfile::tempdir().unwrap();
        let version_id = "neoforge-21.1.250";
        let version_dir = root.path().join("versions").join(version_id);
        std::fs::create_dir_all(&version_dir).unwrap();
        std::fs::write(
            version_dir.join(format!("{version_id}.json")),
            serde_json::json!({
                "id": version_id,
                "inheritsFrom": "1.21.1"
            })
            .to_string(),
        )
        .unwrap();

        let launcher_profiles_path = root.path().join("launcher_profiles.json");
        let original = serde_json::json!({
            "profiles": {
                "existing-profile": {
                    "name": "Existing",
                    "lastVersionId": "1.21.1"
                }
            },
            "authenticationDatabase": {
                "preserved-value": "must-not-be-changed"
            },
            "settings": {"keep": true}
        });
        std::fs::write(
            &launcher_profiles_path,
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();

        let game_dir =
            register_minecraft_profile(root.path(), "1.2.3", "1.21.1", version_id).unwrap();
        let updated: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&launcher_profiles_path).unwrap()).unwrap();

        assert_eq!(game_dir, root.path().join("mars-client/1.2.3"));
        assert!(game_dir.is_dir());
        assert_eq!(
            updated["authenticationDatabase"],
            original["authenticationDatabase"]
        );
        assert_eq!(updated["settings"], original["settings"]);
        assert_eq!(
            updated["profiles"]["existing-profile"],
            original["profiles"]["existing-profile"]
        );
        assert_eq!(
            updated["profiles"]["mars-client-1.2.3"]["lastVersionId"],
            version_id
        );
        assert_eq!(
            updated["profiles"]["mars-client-1.2.3"]["javaArgs"],
            "-Xmx6G"
        );
        assert_eq!(
            updated["profiles"]["mars-client-1.2.3"]["gameDir"],
            game_dir.to_string_lossy().as_ref()
        );

        let mut legacy_profiles: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&launcher_profiles_path).unwrap()).unwrap();
        legacy_profiles["profiles"]["mars-client-1.2.3"]
            .as_object_mut()
            .unwrap()
            .remove("javaArgs");
        std::fs::write(
            &launcher_profiles_path,
            serde_json::to_vec(&legacy_profiles).unwrap(),
        )
        .unwrap();

        assert_eq!(
            register_minecraft_profile(root.path(), "1.2.3", "1.21.1", version_id).unwrap(),
            game_dir
        );
        let upgraded: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&launcher_profiles_path).unwrap()).unwrap();
        assert_eq!(
            upgraded["profiles"]["mars-client-1.2.3"]["javaArgs"],
            "-Xmx6G"
        );

        let first_write = std::fs::read(&launcher_profiles_path).unwrap();
        assert_eq!(
            register_minecraft_profile(root.path(), "1.2.3", "1.21.1", version_id).unwrap(),
            game_dir
        );
        let second_write = std::fs::read(&launcher_profiles_path).unwrap();
        assert_eq!(first_write, second_write);
    }
}
