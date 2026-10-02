# Mars Command

Mars Command is a Windows desktop companion for the **Mars** modded Minecraft server. It monitors server status, verifies the signed pack manifest, syncs an isolated game directory, and registers the installation in Minecraft Launcher.

| | |
| --- | --- |
| Minecraft | 1.21.1 |
| Loader | NeoForge 21.1.250 |
| Server | `play.nexusgit.info` |
| Voice | `voice.nexusgit.info` (not yet integrated) |

## Features

### Server status

The Rust backend polls the Minecraft Java status protocol every 15 seconds and reports online state, player count, latency, MOTD, and server build. Polling avoids overlapping checks and pauses while the window is hidden. The Mars proxy requires protocol version `767` for Minecraft 1.21.1; the conventional `-1` sentinel is rejected.

### Signed pack verification

The client fetches `manifest.json` and `manifest.json.sig` from the Mars API. It verifies the detached Ed25519 signature against the public key compiled into the application before trusting the manifest. Invalid signatures fail closed.

Each managed file, including mod JARs, is verified using its signed SHA-256 and size. Unlisted JARs in managed directories are reported as foreign. Config and default-config files are mutable: local edits are preserved and reported as conflicts. Sync stages downloads, validates them before replacement, and removes obsolete files only when they still match the last installed hash.

### Isolated Minecraft installation

Setup creates a Minecraft Launcher profile named `Mars Client <pack version>` and an isolated game directory:

```text
%APPDATA%\.minecraft\mars-client\<pack-version>
```

The shared `%APPDATA%\.minecraft\mods` folder is left untouched. Setup uses the matching NeoForge version already installed in Minecraft Launcher, preserves other profiles and account data, syncs the signed pack, verifies it, and opens the official launcher. Close Minecraft Launcher before Setup or Update so its profile file is not being edited concurrently.

The dashboard remembers the installation and offers:

| Action | When it appears |
| --- | --- |
| `SETUP` | No Mars installation is registered. |
| `UPDATE` | The signed pack version, immutable file hashes, or NeoForge version changed. |
| `LAUNCH` | The installation matches the current signed manifest and passes integrity checks. |
| `BLOCKED` | Required files or mods need attention, or there is no trusted manifest. |

Launch rechecks integrity but does not rewrite the profile or sync again. Minecraft Launcher handles Microsoft sign-in; select the Mars profile and click **Play** there. Mars Command does not directly start the game process.

World-specific data packs are not placed into a save automatically; they remain manual until a world target is selected.

## Release API

The client defaults to `https://api.nexusgit.info/api/v1`. The release feed exposes the signed manifest, detached signature, and files referenced by that manifest:

- `GET /api/v1/manifest.json`
- `GET /api/v1/manifest.json.sig`
- `GET /api/v1/files/<manifest-relative-path>`

The API serves release artifacts but does not need the Ed25519 private key. Deploy `manifest.json`, `manifest.json.sig`, and every referenced file while preserving relative paths. Only host files you are authorized to redistribute. The client never embeds or sends the development API JWT.

## Development

Prerequisites: Node.js, Rust, and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/).

```powershell
npm install
npm run tauri dev
```

Run checks:

```powershell
npm run build
cd src-tauri
cargo fmt --check
cargo test --all-targets
```

Build Windows installers from the repository root:

```powershell
$env:MARS_MANIFEST_BASE_URL = "https://api.nexusgit.info/api/v1"
$env:MARS_MANIFEST_PUBLIC_KEY = "<production public key in hex>"
npm run tauri -- build
```

`MARS_MANIFEST_BASE_URL` and `MARS_MANIFEST_PUBLIC_KEY` are compile-time overrides. Rust `option_env!` does not automatically read `.env`. Bundles are written under `src-tauri\target\release\bundle\` (NSIS and MSI where available).

Before public distribution, set a product name/version in `src-tauri/tauri.conf.json`, use a production keypair whose public key is embedded in the client, and code-sign the Windows installer if you want to reduce SmartScreen warnings. Never ship the private signing key or an API JWT in the app.

## Generate A Manifest

The maintainer tool hashes these directories from the local pack instance: `mods`, `config`, `kubejs`, `defaultconfigs`, `resourcepacks`, and `shaderpacks`. Only `config` and `defaultconfigs` are marked mutable.

From PowerShell at the repository root:

```powershell
$env:MARS_SKIP_LOCAL_ENV = "1"
$env:MARS_PACKAGE_BASE_URL = "https://api.nexusgit.info/api/v1"
cargo run --manifest-path src-tauri/Cargo.toml --release --example manifest_tool -- build `
  "modpack/Mars-Client-1.2.4/Mars Client" `
  "1.2.4" `
  "manifest-dist/manifest.json"

cargo run --manifest-path src-tauri/Cargo.toml --release --example manifest_tool -- sign `
  "manifest-keys/mars-signing.key" `
  "manifest-dist/manifest.json"

cargo run --manifest-path src-tauri/Cargo.toml --release --example manifest_tool -- verify `
  "manifest-dist/manifest.json"
```

`MARS_SKIP_LOCAL_ENV=1` prevents the tool from loading `.env`. The signing command reads the private key file without printing its contents. Verify the signature against the same public key embedded in the client, then deploy the manifest, signature, and payload files to the API release directory.

### Automatic API deployment

The **Publish pack manifest** workflow publishes a GitHub release and, for non-prereleases, deploys `manifest.json` and `manifest.json.sig` to `/opt/mars-package-api/releases/1.0.0`. It stages both files, replaces the signature first and the manifest last, then fetches both public API URLs and compares them byte-for-byte with the build artifacts. Prereleases are not deployed to the live API.

Configure these repository Actions secrets:

| Secret | Purpose |
| --- | --- |
| `CURSEFORGE_API_KEY` | Approved Core API key used to resolve export metadata. |
| `MARS_SIGNING_KEY` | Ed25519 private key used to sign the manifest. |
| `MARS_DEPLOY_HOST` | SSH hostname of the Mars API server. |
| `MARS_DEPLOY_USER` | Dedicated SSH user allowed to deploy to the release directory. |
| `MARS_DEPLOY_SSH_KEY` | Private SSH deploy key. |
| `MARS_DEPLOY_KNOWN_HOSTS` | Pinned `known_hosts` entry for the server; do not discover it during the workflow. |

The deploy user must be able to create a run-specific staging directory, replace files in the release directory, and run `sudo -n systemctl restart mars-package-api`. The restart reloads the API's cached release-file inventory. Keep the SSH key limited to deployment use. The API server does not receive the manifest signing key or CurseForge API key. The workflow serializes releases so two deployments cannot overwrite each other.

## Security

- The Ed25519 signature authenticates the exact manifest bytes. Re-sign after any edit, including whitespace.
- The client verifies the signature before parsing the manifest and rejects unsafe relative paths.
- Downloads require HTTPS and are checked against signed SHA-256 hashes and sizes before installation.
- Keep the private signing key in protected release storage, never in source control or on the API server.
- Hashes detect file changes; the signature proves the trusted maintainer approved those hashes.

## Project Layout

```text
src/                         React dashboard, settings, hooks, and UI types
src-tauri/src/
  minecraft.rs               DNS/SRV resolution and Java status protocol
  manifest.rs                Manifest schema, fetch, signature verification
  integrity.rs               Hash-based file and mod verification
  settings.rs                Persistent game path and Launcher profile setup
  sync.rs                    Staged sync and installed-state tracking
  lib.rs                     Tauri commands and Setup/Update/Launch status
src-tauri/examples/
  manifest_tool.rs           Maintainer manifest generator and signer
Mars-Server-API/
  main.py                    Manifest and package-file API
  openapi.json               API contract
manifest-dist/               Release manifest and detached signature
```

## Limitations

- Automatic profile setup currently supports Windows and NeoForge. The matching NeoForge version must already be installed in Minecraft Launcher.
- Setup opens Minecraft Launcher after successful sync and verification; the user selects the Mars profile and starts the game there.
- Sync has no progress bar or cancellation yet, and file scans do not report progress.
- Rotating the manifest signing key requires rebuilding the client with the matching public key.
- Voice relay integration is not implemented.

## Roadmap

- Linux support: ship Linux installers and implement Linux-compatible Minecraft profile setup and launcher opening. These flows are currently Windows-only.
