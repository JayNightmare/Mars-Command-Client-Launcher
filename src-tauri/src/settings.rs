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
