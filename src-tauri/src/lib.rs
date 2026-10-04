mod integrity;
pub mod manifest;
mod minecraft;
mod process_detection;
mod repair;
mod settings;
mod sync;

use std::path::PathBuf;
#[cfg(any(windows, target_os = "linux"))]
use std::process::Command;
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
async fn check_installation_repair() -> Result<repair::InstallationRepairStatus, String> {
    repair::check().await
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

#[tauri::command]
fn set_preserve_persistent_data(app: AppHandle, preserve: bool) -> Result<ClientSettings, String> {
    let mut preferences = settings::load(&app);
    preferences.preserve_persistent_data = preserve;
    settings::save(&app, &preferences)?;
    Ok(preferences)
}

#[tauri::command]
fn auto_detect_instance_root(app: AppHandle) -> Result<Option<String>, String> {
    let mut current = settings::load(&app);
    if let Some(root) = current.instance_root.as_ref() {
        let root = PathBuf::from(root);
        if settings::is_minecraft_game_dir(&root) {
            return Ok(Some(root.to_string_lossy().to_string()));
        }
        let nested_game_dir = root.join("minecraft");
        if settings::is_minecraft_game_dir(&nested_game_dir) {
            let nested_game_dir = nested_game_dir.to_string_lossy().to_string();
            current.instance_root = Some(nested_game_dir.clone());
            settings::save(&app, &current)?;
            return Ok(Some(nested_game_dir));
        }
    }

    if let Some(root) = settings::detect_mars_launcher_instance() {
        let root = root.to_string_lossy().to_string();
        current.instance_root = Some(root.clone());
        settings::save(&app, &current)?;
        return Ok(Some(root));
    }

    let Some(root) = settings::detect_curseforge_mars_instance() else {
        return Ok(None);
    };
    let root = root.to_string_lossy().to_string();
    current.instance_root = Some(root.clone());
    settings::save(&app, &current)?;
    Ok(Some(root))
}

#[tauri::command]
fn select_curseforge_instance(app: AppHandle) -> Result<Option<String>, String> {
    let Some(root) = settings::detect_curseforge_mars_instance() else {
        return Ok(None);
    };
    let root = root.to_string_lossy().to_string();
    let mut current = settings::load(&app);
    current.instance_root = Some(root.clone());
    settings::save(&app, &current)?;
    Ok(Some(root))
}

/// Opens a native folder picker and persists the result. `None` means the user
/// cancelled; the previous value is left untouched.
#[tauri::command]
async fn choose_instance_root(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Select the Minecraft game folder containing mods or config")
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

    let preferences = settings::load(&app);
    let Some(root) = preferences.instance_root else {
        return Ok(IntegrityReport::failed(
            String::new(),
            manifest.pack_version,
            "No instance folder selected".into(),
        ));
    };

    let preserve_persistent_data = preferences.preserve_persistent_data;
    let root = PathBuf::from(root);
    tauri::async_runtime::spawn_blocking(move || {
        integrity::scan(&root, &manifest, preserve_persistent_data)
    })
    .await
    .map_err(|err| format!("Integrity scan failed: {err}"))
}

#[tauri::command]
async fn sync_instance(
    app: AppHandle,
    state: State<'_, ManifestState>,
) -> Result<sync::SyncResult, String> {
    let Some(manifest) = state.0.lock().unwrap().clone() else {
        return Ok(sync::SyncResult::failed(
            String::new(),
            "No verified manifest is loaded".into(),
        ));
    };

    let preferences = settings::load(&app);
    let Some(root) = preferences.instance_root else {
        return Ok(sync::SyncResult::failed(
            manifest.pack_version,
            "No Minecraft game folder selected".into(),
        ));
    };

    Ok(sync::sync_pack(
        PathBuf::from(root),
        manifest,
        preferences.preserve_persistent_data,
    )
    .await)
}

fn require_launch_ready(report: &IntegrityReport) -> Result<(), String> {
    if let Some(error) = &report.error {
        return Err(format!("Integrity scan failed: {error}"));
    }
    if report.missing_count + report.corrupt_count + report.unreadable_count > 0 {
        return Err("Pack files are missing, corrupt, or unreadable. Sync and scan again.".into());
    }
    if report.manual_unresolved > 0 {
        return Err("Resolve all manual assets before opening the launcher.".into());
    }
    if !report.mods_fully_verified || report.mods_present != report.mods_expected {
        return Err("Every required mod must be identified and checksum-verified.".into());
    }
    if report.mods_foreign > 0 {
        return Err("Unlisted mod JARs are present. Remove them before continuing.".into());
    }
    Ok(())
}

#[tauri::command]
async fn setup_minecraft_installation(
    app: AppHandle,
    state: State<'_, ManifestState>,
) -> Result<serde_json::Value, String> {
    let manifest = state
        .0
        .lock()
        .map_err(|_| "Manifest state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "No trusted manifest is loaded".to_string())?;
    if !manifest.loader.eq_ignore_ascii_case("neoforge") {
        return Err(format!(
            "Automatic installation setup does not support loader {} yet",
            manifest.loader
        ));
    }

    let root = settings::setup_minecraft_installation(
        &manifest.pack_version,
        &manifest.minecraft_version,
        &manifest.loader,
        &manifest.loader_version,
    )?;
    let mut preferences = settings::load(&app);
    preferences.instance_root = Some(root.to_string_lossy().to_string());
    settings::save(&app, &preferences)?;
    let preserve_persistent_data = preferences.preserve_persistent_data;

    let sync_result =
        sync::sync_pack(root.clone(), manifest.clone(), preserve_persistent_data).await;
    let root_text = root.to_string_lossy().to_string();
    if !sync_result.complete {
        let message = sync_result
            .error
            .clone()
            .or_else(|| sync_result.issues.first().map(|issue| issue.reason.clone()))
            .unwrap_or_else(|| "Pack sync did not complete".into());
        return Ok(serde_json::json!({
            "instanceRoot": root_text,
            "syncResult": sync_result,
            "launcherOpened": false,
            "message": message,
        }));
    }

    let report = tauri::async_runtime::spawn_blocking(move || {
        integrity::scan(&root, &manifest, preserve_persistent_data)
    })
    .await
    .map_err(|err| format!("Integrity scan failed: {err}"))?;
    if let Err(message) = require_launch_ready(&report) {
        return Ok(serde_json::json!({
            "instanceRoot": root_text,
            "syncResult": sync_result,
            "launcherOpened": false,
            "message": message,
        }));
    }

    match open_minecraft_launcher() {
        Ok(()) => Ok(serde_json::json!({
            "instanceRoot": root_text,
            "syncResult": sync_result,
            "launcherOpened": true,
            "message": null,
        })),
        Err(message) => Ok(serde_json::json!({
            "instanceRoot": root_text,
            "syncResult": sync_result,
            "launcherOpened": false,
            "message": message,
        })),
    }
}

#[tauri::command]
async fn get_mars_installation_action(
    app: AppHandle,
    state: State<'_, ManifestState>,
) -> Result<String, String> {
    let Some(manifest) = state
        .0
        .lock()
        .map_err(|_| "Manifest state is unavailable".to_string())?
        .clone()
    else {
        return Ok("setup".into());
    };
    let mut preferences = settings::load(&app);
    let saved_root = preferences.instance_root.clone().map(PathBuf::from);
    let root = saved_root
        .filter(|root| settings::mars_launcher_profile_version(root).is_some())
        .or_else(settings::detect_mars_launcher_instance);
    let Some(root) = root else {
        return Ok("setup".into());
    };
    if preferences.instance_root.as_deref() != Some(root.to_string_lossy().as_ref()) {
        preferences.instance_root = Some(root.to_string_lossy().to_string());
        settings::save(&app, &preferences)?;
    }
    let preserve_persistent_data = preferences.preserve_persistent_data;
    let expected_loader_version = format!("{}-{}", manifest.loader, manifest.loader_version);
    if settings::mars_launcher_profile_version(&root).as_deref()
        != Some(expected_loader_version.as_str())
    {
        return Ok("update".into());
    }
    if !sync::manifest_matches_installed_state(&root, &manifest, preserve_persistent_data) {
        return Ok("update".into());
    }

    let scan_root = root.clone();
    let scan_manifest = manifest.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        integrity::scan(&scan_root, &scan_manifest, preserve_persistent_data)
    })
    .await
    .map_err(|err| format!("Integrity scan failed: {err}"))?;
    if report.missing_count + report.corrupt_count + report.unreadable_count > 0 {
        return Ok("update".into());
    }
    Ok(if require_launch_ready(&report).is_ok() {
        "launch"
    } else {
        "blocked"
    }
    .into())
}

#[tauri::command]
async fn launch_minecraft_installation(
    app: AppHandle,
    state: State<'_, ManifestState>,
) -> Result<(), String> {
    let manifest = state
        .0
        .lock()
        .map_err(|_| "Manifest state is unavailable".to_string())?
        .clone()
        .ok_or_else(|| "No trusted manifest is loaded".to_string())?;
    let preferences = settings::load(&app);
    let root = preferences
        .instance_root
        .map(PathBuf::from)
        .ok_or_else(|| "Set up the Mars installation before launching".to_string())?;
    let preserve_persistent_data = preferences.preserve_persistent_data;
    let expected_loader_version = format!("{}-{}", manifest.loader, manifest.loader_version);
    if settings::mars_launcher_profile_version(&root).as_deref()
        != Some(expected_loader_version.as_str())
    {
        return Err("The Mars Launcher installation is missing. Set it up first.".into());
    }
    if !sync::manifest_matches_installed_state(&root, &manifest, preserve_persistent_data) {
        return Err("The server manifest changed. Update the Mars installation first.".into());
    }

    let report = tauri::async_runtime::spawn_blocking(move || {
        integrity::scan(&root, &manifest, preserve_persistent_data)
    })
    .await
    .map_err(|err| format!("Integrity scan failed: {err}"))?;
    require_launch_ready(&report)?;
    open_minecraft_launcher()
}

#[tauri::command]
async fn wait_for_minecraft_client(app: AppHandle) -> Result<bool, String> {
    let root = settings::load(&app)
        .instance_root
        .map(PathBuf::from)
        .ok_or_else(|| "Set up the Mars installation before waiting for Minecraft".to_string())?;
    let root = process_detection::canonical_instance_root(&root)?;
    let started_at = tokio::time::Instant::now();
    let timeout = std::time::Duration::from_secs(120);

    loop {
        let scan_root = root.clone();
        let found = tauri::async_runtime::spawn_blocking(move || {
            process_detection::has_minecraft_client(&scan_root)
        })
        .await
        .map_err(|err| format!("Minecraft process check failed: {err}"))??;
        if found {
            return Ok(true);
        }
        if started_at.elapsed() >= timeout {
            return Ok(false);
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[cfg(windows)]
fn open_minecraft_launcher() -> Result<(), String> {
    Command::new("explorer.exe")
        .args(["shell:AppsFolder\\Microsoft.4297127D64EC6_8wekyb3d8bbwe!Minecraft"])
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open Minecraft Launcher: {err}"))
}

#[cfg(target_os = "linux")]
fn open_minecraft_launcher() -> Result<(), String> {
    Command::new("minecraft-launcher")
        .spawn()
        .map(|_| ())
        .map_err(|err| {
            format!(
                "Could not start `minecraft-launcher` from PATH. Install the official Linux launcher with its conventional command available: {err}"
            )
        })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn open_minecraft_launcher() -> Result<(), String> {
    Err("Opening the installed Minecraft Launcher is only supported on Windows and Linux.".into())
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
            check_installation_repair,
            refresh_manifest,
            get_client_settings,
            set_preserve_persistent_data,
            auto_detect_instance_root,
            select_curseforge_instance,
            choose_instance_root,
            clear_instance_root,
            scan_instance,
            sync_instance,
            setup_minecraft_installation,
            get_mars_installation_action,
            launch_minecraft_installation,
            wait_for_minecraft_client
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod launch_tests {
    use super::{require_launch_ready, IntegrityReport};

    fn clean_report() -> IntegrityReport {
        IntegrityReport {
            root: "instance".into(),
            checked_at: "2026-01-01T00:00:00Z".into(),
            pack_version: "1.2.3".into(),
            total_files: 1,
            ok_count: 1,
            missing_count: 0,
            corrupt_count: 0,
            modified_count: 0,
            foreign_count: 0,
            unreadable_count: 0,
            manual_unresolved: 0,
            mods_expected: 1,
            mods_present: 1,
            mods_foreign: 0,
            mods_fully_verified: true,
            drift: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn handoff_requires_a_clean_verified_mod_set() {
        assert!(require_launch_ready(&clean_report()).is_ok());

        let mut report = clean_report();
        report.mods_present = 0;
        assert!(require_launch_ready(&report).is_err());

        let mut report = clean_report();
        report.mods_foreign = 1;
        assert!(require_launch_ready(&report).is_err());
    }
}
