//! Small JSON-backed client settings in the app config directory.

use std::path::PathBuf;

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
        let is_mars_client = manifest_name
            .as_deref()
            .unwrap_or(profile_name)
            .trim()
            .eq_ignore_ascii_case("Mars Client");
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
