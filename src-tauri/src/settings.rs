//! Small JSON-backed client settings in the app config directory.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io::Write};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientSettings {
    /// Absolute path to the managed Mars instance chosen by the user.
    pub instance_root: Option<String>,
}

pub fn is_minecraft_game_dir(path: &std::path::Path) -> bool {
    path.is_dir()
        && ["mods", "config", "kubejs", "defaultconfigs"]
            .iter()
            .any(|name| path.join(name).is_dir())
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
    #[cfg(not(windows))]
    {
        None
    }

    #[cfg(windows)]
    {
        let app_data = std::env::var_os("APPDATA").map(PathBuf::from)?;
        let minecraft_root = app_data.join(".minecraft");
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
    #[cfg(not(windows))]
    {
        let _ = (pack_version, minecraft_version, loader, loader_version);
        return Err("Minecraft Launcher profile setup is only supported on Windows.".into());
    }

    #[cfg(windows)]
    {
        ensure_minecraft_launcher_closed()?;
        let app_data = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "Windows AppData directory is unavailable".to_string())?;
        let minecraft_root = app_data.join(".minecraft");
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
    #[cfg(not(windows))]
    {
        let _ = game_dir;
        None
    }

    #[cfg(windows)]
    {
        let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) else {
            return None;
        };
        let profile_path = app_data.join(".minecraft").join("launcher_profiles.json");
        let Ok(bytes) = fs::read(profile_path) else {
            return None;
        };
        let Ok(launcher_profiles) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return None;
        };
        let expected_game_dir = game_dir.canonicalize().ok();
        let Some(profiles) = launcher_profiles
            .get("profiles")
            .and_then(serde_json::Value::as_object)
        else {
            return None;
        };
        let Some(expected_game_dir) = expected_game_dir else {
            return None;
        };

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
        if existing["lastVersionId"].as_str() == Some(version_id) && game_dir.is_dir() {
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
    profile.insert("type".into(), serde_json::Value::String("custom".into()));
    profiles.insert(profile_id, serde_json::Value::Object(profile));

    fs::create_dir_all(&game_dir)
        .map_err(|err| format!("Could not create isolated game directory: {err}"))?;
    let serialized = serde_json::to_vec_pretty(&launcher_profiles)
        .map_err(|err| format!("Could not serialize launcher profiles: {err}"))?;
    write_launcher_profiles_atomically(&profile_path, &serialized)?;

    Ok(game_dir)
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
fn write_launcher_profiles_atomically(
    path: &std::path::Path,
    contents: &[u8],
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = path.with_file_name(format!("launcher_profiles.mars-{suffix}.tmp"));
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
fn write_launcher_profiles_atomically(
    path: &std::path::Path,
    contents: &[u8],
) -> Result<(), String> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = path.with_file_name(format!("launcher_profiles.mars-{suffix}.tmp"));
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
    use super::{find_curseforge_mars_instance, register_minecraft_profile};

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
            serde_json::from_slice(&std::fs::read(launcher_profiles_path).unwrap()).unwrap();

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
            updated["profiles"]["mars-client-1.2.3"]["gameDir"],
            game_dir.to_string_lossy().as_ref()
        );

        let first_write = std::fs::read(root.path().join("launcher_profiles.json")).unwrap();
        assert_eq!(
            register_minecraft_profile(root.path(), "1.2.3", "1.21.1", version_id).unwrap(),
            game_dir
        );
        let second_write = std::fs::read(root.path().join("launcher_profiles.json")).unwrap();
        assert_eq!(first_write, second_write);
    }
}
