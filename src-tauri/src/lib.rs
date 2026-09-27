mod integrity;
pub mod manifest;
mod minecraft;
mod settings;

use std::path::PathBuf;
use std::sync::Mutex;

use integrity::IntegrityReport;
use manifest::{Manifest, ManifestStatus};
use minecraft::MinecraftServerStatus;
use settings::ClientSettings;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Holds the most recently *verified* manifest. Unverified manifests never
/// reach this state, so every consumer can treat it as trusted.
#[derive(Default)]
struct ManifestState(Mutex<Option<Manifest>>);

#[tauri::command]
async fn get_minecraft_status(host: String, port: u16) -> MinecraftServerStatus {
    minecraft::fetch_status(host, port).await
}

#[tauri::command]
async fn refresh_manifest(state: State<'_, ManifestState>) -> Result<ManifestStatus, ()> {
    match manifest::fetch_verified().await {
        Ok((manifest, status)) => {
            *state.0.lock().unwrap() = Some(manifest);
            Ok(status)
        }
        Err(status) => {
            // Drop any previously trusted manifest so stale data cannot be scanned against.
            *state.0.lock().unwrap() = None;
            Ok(status)
        }
    }
}

#[tauri::command]
fn get_client_settings(app: AppHandle) -> ClientSettings {
    settings::load(&app)
}

/// Opens a native folder picker and persists the result. `None` means the user
/// cancelled; the previous value is left untouched.
#[tauri::command]
async fn choose_instance_root(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Select the managed Mars instance folder")
        .blocking_pick_folder();

    let Some(folder) = picked else {
        return Ok(None);
    };

    let path = folder
        .into_path()
        .map_err(|err| format!("Unsupported folder selection: {err}"))?;
    let path = path.to_string_lossy().to_string();

    let mut current = settings::load(&app);
    current.instance_root = Some(path.clone());
    settings::save(&app, &current)?;

    Ok(Some(path))
}

#[tauri::command]
fn clear_instance_root(app: AppHandle) -> Result<(), String> {
    let mut current = settings::load(&app);
    current.instance_root = None;
    settings::save(&app, &current)
}

#[tauri::command]
async fn scan_instance(
    app: AppHandle,
    state: State<'_, ManifestState>,
) -> Result<IntegrityReport, String> {
    let cached = state.0.lock().unwrap().clone();
    let Some(manifest) = cached else {
        return Ok(IntegrityReport::failed(
            String::new(),
            String::new(),
            "No verified manifest is loaded".into(),
        ));
    };

    let Some(root) = settings::load(&app).instance_root else {
        return Ok(IntegrityReport::failed(
            String::new(),
            manifest.pack_version,
            "No instance folder selected".into(),
        ));
    };

    let root = PathBuf::from(root);
    tauri::async_runtime::spawn_blocking(move || integrity::scan(&root, &manifest))
        .await
        .map_err(|err| format!("Integrity scan failed: {err}"))
}

/// Best-effort native backdrop. Silently no-ops where the effect is
/// unsupported so the CSS acrylic fallback stays intact.
#[cfg(target_os = "windows")]
fn apply_window_effects(window: &tauri::WebviewWindow) {
    use tauri::utils::WindowEffect;
    use tauri::window::EffectsBuilder;

    for effect in [WindowEffect::Acrylic, WindowEffect::Blur] {
        if window
            .set_effects(EffectsBuilder::new().effect(effect).build())
            .is_ok()
        {
            return;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_window_effects(_window: &tauri::WebviewWindow) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ManifestState::default())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                apply_window_effects(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_minecraft_status,
            refresh_manifest,
            get_client_settings,
            choose_instance_root,
            clear_instance_root,
            scan_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
