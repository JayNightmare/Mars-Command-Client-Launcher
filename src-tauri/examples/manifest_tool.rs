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

use std::io::Read;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

/// Directories treated as fully owned by Mars Command.
const MANAGED_DIRS: &[&str] = &["mods", "config", "kubejs", "defaultconfigs"];
/// Overrides are hashable; `mods` is pinned by id instead, so it is excluded.
const OVERRIDE_MANAGED_DIRS: &[&str] = &["config", "kubejs", "defaultconfigs"];
const MINECRAFT_VERSION: &str = "1.21.1";
const LOADER: &str = "neoforge";
const LOADER_VERSION: &str = "21.1.0";

fn main() {
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

    let mods: Vec<serde_json::Value> = cf
        .get("files")
        .and_then(|v| v.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|entry| {
                    Some(serde_json::json!({
                        "projectId": entry.get("projectID")?.as_u64()?,
                        "fileId": entry.get("fileID")?.as_u64()?,
                        "required": entry.get("required").and_then(|v| v.as_bool()).unwrap_or(true),
                    }))
                })
                .collect()
        })
        .unwrap_or_default();

    let overrides_name = cf
        .get("overrides")
        .and_then(|v| v.as_str())
        .unwrap_or("overrides");
    let overrides_root = export.join(overrides_name);

    let mut files = Vec::new();
    for dir in OVERRIDE_MANAGED_DIRS {
        collect_hashed(&overrides_root, dir, &mut files)?;
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));

    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "packVersion": pack_version,
        "minecraftVersion": minecraft_version,
        "loader": loader,
        "loaderVersion": loader_version,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "managedDirs": OVERRIDE_MANAGED_DIRS,
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
        "wrote {out}: {} override files hashed, {} mods pinned, {minecraft_version} / {loader_id}",
        files.len(),
        mods.len()
    );
    Ok(())
}

/// Hashes every file under `<root>/<dir>`, recording paths relative to `root`.
fn collect_hashed(
    root: &Path,
    dir: &str,
    out: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
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
            "mutable": dir == "config" || dir == "defaultconfigs",
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
        return Err(format!("{out} already exists; refusing to overwrite a signing key"));
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
    let (key_path, manifest_path) = (key_path.ok_or_else(usage)?, manifest_path.ok_or_else(usage)?);
    let signing = load_signing_key(key_path)?;
    let body = std::fs::read(manifest_path).map_err(|err| format!("Could not read manifest: {err}"))?;

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
                "mutable": *dir == "config" || *dir == "defaultconfigs",
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
