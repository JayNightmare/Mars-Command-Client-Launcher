//! Maintainer tooling for the signed pack manifest. Not shipped in the app.
//!
//!   cargo run --example manifest_tool -- keygen <private-key-out>
//!   cargo run --example manifest_tool -- build <instance-root> <pack-version> <manifest-out>
//!   cargo run --example manifest_tool -- cf-pack <cf-export-dir> <pack-version> <manifest-out>
//!   cargo run --example manifest_tool -- sign <private-key> <manifest.json>
//!   cargo run --example manifest_tool -- verify <manifest.json>
//!
//! `build` hashes a working instance; `cf-pack` converts a CurseForge export
//! (hashing `overrides/` and pinning mods by project/file id). `sign` writes a
//! detached `<manifest>.sig` over the exact bytes of the manifest file.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};

/// Directories treated as fully owned by Mars Command.
const MANAGED_DIRS: &[&str] = &["mods", "config", "kubejs", "defaultconfigs"];
/// Overrides are hashable; `mods` is pinned by id instead, so it is excluded.
const OVERRIDE_MANAGED_DIRS: &[&str] = &["config", "kubejs", "defaultconfigs"];
const MINECRAFT_VERSION: &str = "1.21.1";
const LOADER: &str = "neoforge";
const LOADER_VERSION: &str = "21.1.250";
const CURSEFORGE_GAME_ID: u32 = 432;

#[derive(Debug, Clone)]
struct ModlistLink {
    url: String,
    path: String,
    slug: String,
    install_dir: Option<String>,
}

fn main() {
    load_local_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(args.get(1)),
        Some("build") => build(args.get(1), args.get(2), args.get(3)),
        Some("cf-pack") => cf_pack(args.get(1), args.get(2), args.get(3)),
        Some("sign") => sign(args.get(1), args.get(2)),
        Some("verify") => verify(args.get(1)),
        _ => Err(usage()),
    };

    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn usage() -> String {
    concat!(
        "usage:\n",
        "  manifest_tool keygen <private-key-out>\n",
        "  manifest_tool build <instance-root> <pack-version> <manifest-out>\n",
        "  manifest_tool cf-pack <cf-export-dir> <pack-version> <manifest-out>\n",
        "  manifest_tool sign <private-key> <manifest.json>\n",
        "  manifest_tool verify <manifest.json>"
    )
    .to_string()
}

/// Converts a CurseForge modpack export directory into the Mars manifest.
fn cf_pack(
    export_dir: Option<&String>,
    pack_version: Option<&String>,
    out: Option<&String>,
) -> Result<(), String> {
    let export = PathBuf::from(export_dir.ok_or_else(usage)?);
    let pack_version = pack_version.ok_or_else(usage)?;
    let out = out.ok_or_else(usage)?;

    let cf_raw = std::fs::read_to_string(export.join("manifest.json"))
        .map_err(|err| format!("Could not read CurseForge manifest: {err}"))?;
    let cf: serde_json::Value =
        serde_json::from_str(&cf_raw).map_err(|err| format!("Malformed CF manifest: {err}"))?;

    let minecraft_version = cf
        .pointer("/minecraft/version")
        .and_then(|v| v.as_str())
        .unwrap_or(MINECRAFT_VERSION)
        .to_string();

    // CF encodes the loader as e.g. "neoforge-21.1.250".
    let loader_id = cf
        .pointer("/minecraft/modLoaders/0/id")
        .and_then(|v| v.as_str())
        .unwrap_or("neoforge-21.1.0");
    let (loader, loader_version) = loader_id
        .split_once('-')
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap_or_else(|| (LOADER.to_string(), LOADER_VERSION.to_string()));

    let mod_entries: Vec<serde_json::Value> = cf
        .get("files")
        .and_then(|v| v.as_array())
        .map(|files| files.iter().cloned().collect())
        .unwrap_or_default();
    let modlist = parse_modlist(&export.join("modlist.html"))?;
    let mods = resolve_curseforge_mods(&mod_entries, &modlist)?;

    let overrides_name = cf
        .get("overrides")
        .and_then(|v| v.as_str())
        .unwrap_or("overrides");
    let overrides_root = export.join(overrides_name);

    let mut files = Vec::new();
    for dir in OVERRIDE_MANAGED_DIRS {
        collect_hashed(&overrides_root, dir, &mut files)?;
    }
    let override_base = std::env::var("MARS_OVERRIDE_BASE_URL").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/JayNightmare/Mars-Command-Client-Launcher/main/modpack/Mars%20Client/overrides".to_string()
    });
    for file in &mut files {
        let path = file["path"]
            .as_str()
            .ok_or_else(|| "Override path is missing".to_string())?;
        file["downloadUrl"] = serde_json::Value::String(append_url_path(&override_base, path)?);
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    let mut managed_dirs: Vec<String> = ["config", "kubejs", "defaultconfigs"]
        .into_iter()
        .map(str::to_string)
        .collect();
    managed_dirs.extend(modlist.iter().filter_map(|link| link.install_dir.clone()));
    managed_dirs.sort();
    managed_dirs.dedup();

    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "packVersion": pack_version,
        "minecraftVersion": minecraft_version,
        "loader": loader,
        "loaderVersion": loader_version,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "managedDirs": managed_dirs,
        "files": files,
        "curseforgeMods": mods,
        "modsDir": "mods",
    });

    std::fs::write(
        out,
        serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;

    println!(
        "wrote {out}: {} override files hashed, {} CurseForge links resolved and pinned, {minecraft_version} / {loader_id}",
        files.len(),
        mods.len()
    );
    Ok(())
}

fn parse_modlist(path: &Path) -> Result<Vec<ModlistLink>, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|err| format!("Could not read exported modlist.html: {err}"))?;
    let document = Html::parse_document(&body);
    let selector = Selector::parse("a[href]")
        .map_err(|_| "Could not create modlist link selector".to_string())?;
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    for element in document.select(&selector) {
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        let url =
            url::Url::parse(href).map_err(|_| format!("Invalid URL in modlist.html: {href}"))?;
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("curseforge.com" | "www.curseforge.com")
            )
        {
            continue;
        }
        let segments: Vec<_> = url
            .path_segments()
            .ok_or_else(|| format!("Invalid CurseForge link path: {href}"))?
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() < 3 || segments[0] != "minecraft" {
            continue;
        }
        let category = segments[1].to_ascii_lowercase();
        let slug = segments[2].to_ascii_lowercase();
        if slug.is_empty() || !seen.insert((category.clone(), slug.clone())) {
            return Err(format!(
                "Duplicate or empty project link in modlist: {href}"
            ));
        }
        let install_dir = match category.as_str() {
            "mc-mods" => Some("mods".to_string()),
            "texture-packs" => Some("resourcepacks".to_string()),
            "shaders" => Some("shaderpacks".to_string()),
            // A datapack must be installed into a specific world's datapacks directory.
            "data-packs" => None,
            other => return Err(format!("Unsupported CurseForge modlist category: {other}")),
        };
        let canonical_path = format!("/minecraft/{category}/{slug}");
        links.push(ModlistLink {
            url: format!("https://www.curseforge.com{canonical_path}"),
            path: canonical_path,
            slug,
            install_dir,
        });
    }

    if links.is_empty() {
        return Err("modlist.html contains no CurseForge project links".into());
    }
    Ok(links)
}

fn load_local_env() {
    for path in [Path::new(".env"), Path::new("../.env")] {
        if path.is_file() {
            let _ = dotenvy::from_path(path);
            break;
        }
    }
}

fn append_url_path(base: &str, relative: &str) -> Result<String, String> {
    let mut url = url::Url::parse(base).map_err(|_| "Invalid override base URL".to_string())?;
    if url.scheme() != "https" {
        return Err("Override base URL must use HTTPS".into());
    }
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| "Invalid override base URL".to_string())?;
    segments.pop_if_empty();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err("Unsafe override path".into());
        }
        segments.push(segment);
    }
    drop(segments);
    Ok(url.to_string())
}

fn resolve_curseforge_mods(
    entries: &[serde_json::Value],
    links: &[ModlistLink],
) -> Result<Vec<serde_json::Value>, String> {
    if entries.is_empty() || links.is_empty() {
        if entries.is_empty() && links.is_empty() {
            return Ok(Vec::new());
        }
        return Err(
            "CurseForge export manifest and modlist.html must both contain project entries".into(),
        );
    }
    if entries.len() != links.len() {
        return Err(format!(
            "CurseForge export has {} file pins but modlist.html has {} project links",
            entries.len(),
            links.len()
        ));
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let key = std::env::var("CURSEFORGE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "CURSEFORGE_API_KEY is required to resolve CurseForge files".to_string())?;
    let base = std::env::var("CURSEFORGE_API_URL")
        .unwrap_or_else(|_| "https://api.curseforge.com".to_string());
    if !base.starts_with("https://") {
        return Err("CURSEFORGE_API_URL must use HTTPS".into());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("mars-manifest-tool/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|err| format!("Could not create CurseForge API client: {err}"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Could not start CurseForge API runtime: {err}"))?;
    runtime.block_on(resolve_curseforge_mods_async(
        &client, &base, &key, entries, links,
    ))
}

async fn lookup_modlist_project(
    client: reqwest::Client,
    base: String,
    key: String,
    link: ModlistLink,
) -> Result<(u64, ModlistLink), String> {
    let response = client
        .get(format!("{}/v1/mods/search", base.trim_end_matches('/')))
        .header("x-api-key", key)
        .query(&[
            ("gameId", CURSEFORGE_GAME_ID.to_string()),
            ("slug", link.slug.clone()),
            ("pageSize", "50".to_string()),
        ])
        .send()
        .await
        .map_err(|err| format!("CurseForge lookup failed for {}: {err}", link.url))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge lookup returned HTTP {} for {}",
            response.status().as_u16(),
            link.url
        ));
    }
    let response: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("CurseForge returned invalid JSON for {}: {err}", link.url))?;
    let exact: Vec<_> = response["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["slug"]
                .as_str()
                .is_some_and(|slug| slug.eq_ignore_ascii_case(&link.slug))
                && item["links"]["websiteUrl"]
                    .as_str()
                    .and_then(|url| url::Url::parse(url).ok())
                    .is_some_and(|url| {
                        url.path()
                            .trim_end_matches('/')
                            .eq_ignore_ascii_case(&link.path)
                    })
        })
        .collect();
    if exact.len() != 1 {
        return Err(format!(
            "CurseForge link {} resolved to {} exact projects; refusing an ambiguous match",
            link.url,
            exact.len()
        ));
    }
    let id = exact[0]["id"]
        .as_u64()
        .ok_or_else(|| format!("CurseForge returned no project ID for {}", link.url))?;
    Ok((id, link))
}

async fn resolve_modlist_projects(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    links: &[ModlistLink],
) -> Result<HashMap<u64, ModlistLink>, String> {
    let mut projects = HashMap::with_capacity(links.len());
    for batch in links.chunks(6) {
        let mut tasks = tokio::task::JoinSet::new();
        for link in batch.iter().cloned() {
            tasks.spawn(lookup_modlist_project(
                client.clone(),
                base.to_string(),
                key.to_string(),
                link,
            ));
        }
        while let Some(result) = tasks.join_next().await {
            let (project_id, link) =
                result.map_err(|err| format!("CurseForge lookup task failed: {err}"))??;
            if projects.insert(project_id, link).is_some() {
                return Err(format!(
                    "Multiple modlist links resolved to CurseForge project {project_id}"
                ));
            }
        }
    }
    Ok(projects)
}

async fn api_post(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let response = client
        .post(format!("{}{}", base.trim_end_matches('/'), path))
        .header("x-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("CurseForge request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge API returned HTTP {} for {path}",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|err| format!("CurseForge returned invalid JSON for {path}: {err}"))
}

async fn resolve_curseforge_mods_async(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    entries: &[serde_json::Value],
    links: &[ModlistLink],
) -> Result<Vec<serde_json::Value>, String> {
    let projects_from_links = resolve_modlist_projects(client, base, key, links).await?;
    let mut project_ids = HashSet::new();
    let mut file_ids = HashSet::new();
    for entry in entries {
        project_ids.insert(
            entry
                .get("projectID")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| "CurseForge export contains an invalid projectID".to_string())?,
        );
        file_ids.insert(
            entry
                .get("fileID")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| "CurseForge export contains an invalid fileID".to_string())?,
        );
    }

    let pinned_projects: HashSet<_> = project_ids.iter().copied().collect();
    let linked_projects: HashSet<_> = projects_from_links.keys().copied().collect();
    if pinned_projects != linked_projects {
        let missing_links = pinned_projects.difference(&linked_projects).count();
        let unpinned_links = linked_projects.difference(&pinned_projects).count();
        return Err(format!(
            "modlist.html does not match the CurseForge export: {missing_links} pinned projects lack links, {unpinned_links} links are not pinned"
        ));
    }

    let mut mods_by_id = HashMap::new();
    for chunk in project_ids.into_iter().collect::<Vec<_>>().chunks(50) {
        let response = api_post(
            client,
            base,
            key,
            "/v1/mods",
            serde_json::json!({"modIds": chunk, "filterPcOnly": true}),
        )
        .await?;
        for item in response["data"].as_array().into_iter().flatten() {
            if let Some(id) = item.get("id").and_then(serde_json::Value::as_u64) {
                mods_by_id.insert(id, item.clone());
            }
        }
    }

    let mut files_by_id = HashMap::new();
    for chunk in file_ids.into_iter().collect::<Vec<_>>().chunks(50) {
        let response = api_post(
            client,
            base,
            key,
            "/v1/mods/files",
            serde_json::json!({"fileIds": chunk}),
        )
        .await?;
        for item in response["data"].as_array().into_iter().flatten() {
            if let Some(id) = item.get("id").and_then(serde_json::Value::as_u64) {
                files_by_id.insert(id, item.clone());
            }
        }
    }

    entries
        .iter()
        .map(|entry| {
            let project_id = entry["projectID"].as_u64().unwrap_or_default();
            let file_id = entry["fileID"].as_u64().unwrap_or_default();
            let mod_info = mods_by_id
                .get(&project_id)
                .ok_or_else(|| format!("CurseForge project {project_id} was not found"))?;
            let project_link = projects_from_links
                .get(&project_id)
                .ok_or_else(|| format!("CurseForge project {project_id} has no modlist link"))?;
            let file = files_by_id
                .get(&file_id)
                .ok_or_else(|| format!("CurseForge file {file_id} was not found"))?;
            if file["modId"].as_u64() != Some(project_id) {
                return Err(format!("CurseForge file {file_id} does not belong to project {project_id}"));
            }
            if file["isAvailable"].as_bool() == Some(false) {
                return Err(format!("CurseForge file {file_id} is unavailable"));
            }

            let file_name = file["fileName"].as_str().unwrap_or_default();
            if file_name.is_empty() || file_name.contains(['/', '\\']) {
                return Err(format!("CurseForge file {file_id} has an unsafe filename"));
            }
            let sha1 = file["hashes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|hash| hash["algo"].as_u64() == Some(1))
                .and_then(|hash| hash["value"].as_str());
            let allowed = mod_info["allowModDistribution"].as_bool() == Some(true);
            let download_url = file["downloadUrl"].as_str().filter(|url| url.starts_with("https://"));
            let manual_download = project_link.install_dir.is_none()
                || !allowed
                || download_url.is_none()
                || sha1.is_none();
            let source_page = format!("{}/files/{file_id}", project_link.url);

            Ok(serde_json::json!({
                "projectId": project_id,
                "fileId": file_id,
                "required": entry["required"].as_bool().unwrap_or(true),
                "installDir": project_link.install_dir,
                "fileName": file_name,
                "size": file["fileLength"].as_u64(),
                "sha1": sha1,
                "downloadUrl": if manual_download { None::<String> } else { download_url.map(str::to_owned) },
                "manualDownload": manual_download,
                "sourcePage": source_page,
            }))
        })
        .collect()
}

/// Hashes every file under `<root>/<dir>`, recording paths relative to `root`.
fn collect_hashed(root: &Path, dir: &str, out: &mut Vec<serde_json::Value>) -> Result<(), String> {
    let base = root.join(dir);
    if !base.is_dir() {
        return Ok(());
    }

    for entry in walkdir::WalkDir::new(&base)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|err| err.to_string())?
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");

        let (digest, size) = hash_file(entry.path()).map_err(|err| err.to_string())?;
        out.push(serde_json::json!({
            "path": relative,
            "sha256": digest,
            "size": size,
            "required": true,
            // Config is expected to be tweaked locally; scripts are not.
            "mutable": true,
            "side": "client",
            "downloadUrl": serde_json::Value::Null,
            "manualDownload": false,
            "sourcePage": serde_json::Value::Null,
        }));
    }

    Ok(())
}

/// Checks a manifest against the public key compiled into the client.
fn verify(manifest_path: Option<&String>) -> Result<(), String> {
    let manifest_path = manifest_path.ok_or_else(usage)?;
    let body = std::fs::read(manifest_path).map_err(|err| err.to_string())?;
    let signature = std::fs::read_to_string(format!("{manifest_path}.sig"))
        .map_err(|err| format!("Could not read detached signature: {err}"))?;

    mars_command_client_lib::manifest::verify_signature(&body, &signature)?;
    println!("signature OK against the key embedded in this build");
    Ok(())
}

fn keygen(out: Option<&String>) -> Result<(), String> {
    let out = out.ok_or_else(usage)?;
    let signing = SigningKey::generate(&mut rand_core::OsRng);

    if Path::new(out).exists() {
        return Err(format!(
            "{out} already exists; refusing to overwrite a signing key"
        ));
    }
    if let Some(parent) = Path::new(out).parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(out, hex::encode(signing.to_bytes())).map_err(|err| err.to_string())?;

    println!("private key written to {out}  (keep this out of version control)");
    println!(
        "MARS_MANIFEST_PUBLIC_KEY={}",
        hex::encode(signing.verifying_key().to_bytes())
    );
    Ok(())
}

fn load_signing_key(path: &str) -> Result<SigningKey, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("Could not read signing key: {err}"))?;
    let bytes = hex::decode(raw.trim()).map_err(|_| "Signing key is not valid hex".to_string())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "Signing key must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn sign(key_path: Option<&String>, manifest_path: Option<&String>) -> Result<(), String> {
    let (key_path, manifest_path) = (
        key_path.ok_or_else(usage)?,
        manifest_path.ok_or_else(usage)?,
    );
    let signing = load_signing_key(key_path)?;
    let body =
        std::fs::read(manifest_path).map_err(|err| format!("Could not read manifest: {err}"))?;

    let signature = signing.sign(&body);
    let out = format!("{manifest_path}.sig");
    std::fs::write(&out, hex::encode(signature.to_bytes())).map_err(|err| err.to_string())?;

    println!("signed {manifest_path} -> {out}");
    Ok(())
}

fn build(
    root: Option<&String>,
    pack_version: Option<&String>,
    out: Option<&String>,
) -> Result<(), String> {
    let root = PathBuf::from(root.ok_or_else(usage)?);
    let pack_version = pack_version.ok_or_else(usage)?;
    let out = out.ok_or_else(usage)?;

    if !root.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }

    let mut files = Vec::new();
    for dir in MANAGED_DIRS {
        let base = root.join(dir);
        if !base.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&base)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let relative = entry
                .path()
                .strip_prefix(&root)
                .map_err(|err| err.to_string())?
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");

            let (digest, size) = hash_file(entry.path()).map_err(|err| err.to_string())?;
            files.push(serde_json::json!({
                "path": relative,
                "sha256": digest,
                "size": size,
                "required": true,
                // Config is expected to be tweaked locally; mods are not.
                "mutable": *dir != "mods",
                "side": "client",
                // Populated by a CurseForge/Modrinth importer, not by a local scan.
                "downloadUrl": serde_json::Value::Null,
                "manualDownload": false,
                "sourcePage": serde_json::Value::Null,
            }));
        }
    }

    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "packVersion": pack_version,
        "minecraftVersion": MINECRAFT_VERSION,
        "loader": LOADER,
        "loaderVersion": LOADER_VERSION,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "managedDirs": MANAGED_DIRS,
        "files": files,
    });

    let body = serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?;
    std::fs::write(out, body).map_err(|err| err.to_string())?;

    println!(
        "wrote {out} ({} files). Sign it next: manifest_tool sign <key> {out}",
        manifest["files"].as_array().map(Vec::len).unwrap_or(0)
    );
    Ok(())
}

fn hash_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;

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

#[cfg(test)]
mod tests {
    use super::parse_modlist;

    #[test]
    fn parses_modlist_links_into_install_categories() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("modlist.html");
        std::fs::write(
            &path,
            r#"<ul>
                <li><a href="https://www.curseforge.com/minecraft/mc-mods/create">Create</a></li>
                <li><a href="https://www.curseforge.com/minecraft/texture-packs/icons">Icons</a></li>
                <li><a href="https://www.curseforge.com/minecraft/shaders/example">Shader</a></li>
                <li><a href="https://www.curseforge.com/minecraft/data-packs/example">Data pack</a></li>
                <li><a href="https://example.com/not-a-project">External</a></li>
            </ul>"#,
        )
        .unwrap();

        let links = parse_modlist(&path).unwrap();
        assert_eq!(links.len(), 4);
        assert_eq!(links[0].slug, "create");
        assert_eq!(links[0].install_dir.as_deref(), Some("mods"));
        assert_eq!(links[1].install_dir.as_deref(), Some("resourcepacks"));
        assert_eq!(links[2].install_dir.as_deref(), Some("shaderpacks"));
        assert_eq!(links[3].install_dir, None);
    }

    #[test]
    fn rejects_duplicate_modlist_project_links() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("modlist.html");
        std::fs::write(
            &path,
            r#"<a href="https://www.curseforge.com/minecraft/mc-mods/create">Create</a>
               <a href="https://www.curseforge.com/minecraft/mc-mods/create">Create again</a>"#,
        )
        .unwrap();

        assert!(parse_modlist(&path).is_err());
    }
}
