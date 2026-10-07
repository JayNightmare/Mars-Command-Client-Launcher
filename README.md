# Mars Command

Mars Command is a Windows and Linux desktop companion for the **Mars** modded Minecraft server. It monitors server status, verifies the signed pack manifest, syncs an isolated game directory, and registers the installation in Minecraft Launcher.

| | |
| --- | --- |
| Minecraft | 1.21.1 |
| Loader | NeoForge 21.1.250 |
| Server | `play.nexusgit.info:25565` |
| Voice | `voice.nexusgit.info` (not yet integrated) |

## Features

### Server status

The Rust backend polls the Minecraft Java status protocol every 15 seconds and reports online state, player count, latency, MOTD, and server build. Polling avoids overlapping checks and pauses while the window is hidden. The Mars proxy requires protocol version `767` for Minecraft 1.21.1; the conventional `-1` sentinel is rejected.

Mission Control lets users change the host and port used for dashboard status polling; the target is stored locally and does not change the Minecraft Launcher server entry. The Crew Channel currently shows only aggregate player presence. The server API does not yet provide roster or messaging endpoints.

### Signed pack verification

The client fetches `manifest.json` and `manifest.json.sig` from the Mars API. It verifies the detached Ed25519 signature against the public key compiled into the application before trusting the manifest. Invalid signatures fail closed.

Each managed file, including mod JARs, is verified using its signed SHA-256 and size. Unlisted JARs in managed directories are reported as foreign unless they are explicitly tracked personal mods with matching local checksums and passing metadata checks. Personal mods never contribute to signed verification counts. Config and default-config files are mutable: local edits are preserved and reported as conflicts. Sync stages downloads, validates them before replacement, and removes obsolete files only when they still match the last installed hash.

The **Preserve personal data** setting is enabled by default. During updates it keeps existing worlds, screenshots, mod configuration, shader packs, server-list entries, and selected Minecraft options. Missing baseline files are still installed. Turn the setting off to let normal pack updates replace or remove managed files in those locations.

### Isolated Minecraft installation

Setup creates a Minecraft Launcher profile named `Mars Client <pack version>` and an isolated game directory:

```text
Windows: %APPDATA%\.minecraft\mars-client\<pack-version>
Linux:   ~/.minecraft/mars-client/<pack-version>
```

The shared `.minecraft/mods` folder is left untouched. Setup uses the matching NeoForge version already installed in Minecraft Launcher, preserves other profiles and account data, adds or updates the Mars server entry in the isolated profile's server list, syncs the signed pack, verifies it, and opens the launcher. On Linux, the launcher must be available as `minecraft-launcher` on `PATH`. Close Minecraft Launcher before Setup or Update so its profile file is not being edited concurrently. The default maximum Java heap allocation is 6 GB.

The dashboard remembers the installation and offers:

| Action | When it appears |
| --- | --- |
| `SETUP` | No Mars installation is registered. |
| `UPDATE` | The signed pack version, immutable file hashes, or NeoForge version changed. |
| `LAUNCH` | The installation matches the current signed manifest and passes integrity checks. |
| `BLOCKED` | Required files or mods need attention, or there is no trusted manifest. |

Launch rechecks integrity but does not rewrite the profile or sync again. Minecraft Launcher handles Microsoft sign-in; select the Mars profile and click **Play** there. When enabled in Settings, Mars Command waits up to two minutes for the Java process using the configured Mars game directory, then closes itself. It does not directly start the game process.

Settings also provides reduced-motion and larger-text accessibility options, plus quick actions to open the installation folder, report a bug, and visit the project links. **Fund This Project** opens GitHub Sponsors; payments are handled by GitHub, not Mars Command.

World-specific data packs are not placed into a save automatically; they remain manual until a world target is selected.

### Personal client mods

After setting up the current Mars installation, open **Settings -> Personal Mods -> Select local mod JAR**. The native file picker imports one local JAR at a time; nothing is uploaded to a server. Review its mod IDs, size, compatibility warnings, and trust notice, acknowledge the risks, then choose **Install personal mod**. Select **Remove** and confirm to remove a tracked mod from the current instance. Close Minecraft first; changes are blocked while its Mars game process is running.

- Limits: a non-empty `.jar`, at most **64 MiB** per file and **128 personal JARs** per instance. Filenames must contain only letters, numbers, dots, hyphens, and underscores; hidden names and Windows device names are rejected.
- Validation reads bounded NeoForge TOML metadata inside the JAR. Fabric/Quilt/plain JARs, malformed archives/metadata, Forge-only requirements, signed-pack filename collisions, duplicate filenames/checksums/mod IDs, and known incompatible client Minecraft/NeoForge/FML ranges or required dependencies are blocked before copying. Existing mods' declared conflicts with the new mod are also checked. FML is checked against the installed NeoForge launcher metadata when available.
- Missing constraints, unresolved version expressions/non-numeric Maven qualifiers, unavailable FML metadata, and bundled Jar-in-Jar modules are disclosed as compatibility warnings. Bundled dependency resolution is not certified; unresolved required top-level dependencies are blocked. Metadata cannot prove that a mod is client-only, safe to execute, or accepted by the server. The acknowledgement is not a signature or malware scan.
- Files are copied only into the standard `mods` folder of `.minecraft/mars-client/<pack-version>`. Arbitrary/CurseForge/shared game folders and redirected instance/mod/state directories are not personal-mod install targets. Copies are staged without overwriting existing files and revalidated against the preview checksum. The local inventory is stored separately in `.mars-command/personal-mods.json`, never in the signed manifest or signed installed-file list.
- Normal signed updates preserve tracked personal JARs, even when **Preserve personal data** is off. Both dashboard **UPDATE** and Settings **Sync / update pack** create the new versioned Mars instance when needed and copy its personal inventory/JARs without deleting the previous version. A filename collision or changed/unreadable source stops migration with an error; remove the conflicting personal mod from the old instance and retry, or inspect changed files manually. A newly incompatible mod remains preserved but blocks launch until removed.
- Integrity results distinguish **installed (local checksum matches, unsigned)**, **missing**, **changed**, **unreadable**, and **incompatible** personal mods. Unknown/untracked JARs still remain foreign and block launch. A damaged inventory fails closed. Removal never deletes untracked/signed pack files or a personal JAR whose bytes changed; inspect that file manually, and then remove its missing inventory entry in Settings if appropriate.

Personal mod management requires a trusted signed manifest. Remote storage, submission, and community browsing are not part of this local flow.

## Client Releases

The **Build client installers** workflow runs when a `client-vX.Y.Z` tag is pushed. Before tagging, set the same `X.Y.Z` version in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`. The workflow validates those versions, builds a Windows NSIS installer and Linux Debian package, then publishes them as `setup-X.Y.Z.exe` and `setup-X.Y.Z.deb` assets on the GitHub release. Building the tagged release publishes it; do not push a tag until both installers are intended for release.

The in-app client update check uses stable, non-prerelease GitHub releases and selects the newest installer compatible with the current platform: `setup-X.Y.Z.exe` on Windows or `setup-X.Y.Z.deb` on Linux. For older releases it also accepts `setup.exe` or `setup.deb` when the release tag contains a valid version; a versioned asset is preferred when both names are published. Drafts, prereleases, unsupported platforms, and assets for another platform are not offered.

When an update is available, choose **Open installer** to open its download in your browser. After it finishes, close Mars Command, open the downloaded installer, follow its prompts, and relaunch Mars Command. No installer runs or installs silently. If the release check cannot reach GitHub, retry after checking your connection or open the [GitHub releases page](https://github.com/JayNightmare/Mars-Command-Client-Launcher/releases). If no compatible stable asset is published, check that page for `setup-X.Y.Z.exe` or `setup-X.Y.Z.deb` (or the legacy generic name), then retry the check later.

## Release API

The client defaults to `https://api.nexusgit.info/api/v1`. The release feed exposes the signed manifest, detached signature, and files referenced by that manifest:

- `GET /api/v1/manifest.json`
- `GET /api/v1/manifest.json.sig`
- `GET /api/v1/files/<manifest-relative-path>`

The API serves release artifacts but does not need the Ed25519 private key. Deploy `manifest.json`, `manifest.json.sig`, and every referenced file while preserving relative paths. Only host files you are authorized to redistribute. The client never embeds or sends the development API JWT.

## Development

Prerequisites: Node.js, Rust, and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your platform.

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

`MARS_MANIFEST_BASE_URL` and `MARS_MANIFEST_PUBLIC_KEY` are compile-time overrides. Rust `option_env!` does not automatically read `.env`. On Linux, install the Tauri Linux prerequisites and build a Debian package with `npm run tauri -- build --bundles deb`. Bundles are written under `src-tauri/target/release/bundle/` (NSIS/MSI on Windows and `.deb` on Linux).

Before public distribution, use a production keypair whose public key is embedded in the client, and code-sign the Windows installer if you want to reduce SmartScreen warnings. Never ship the private signing key or an API JWT in the app.

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
  settings.rs                Persistent settings, server list, and Launcher profiles
  process_detection.rs       Windows/Linux Minecraft process detection
  repair.rs                  GitHub release installer version checks
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

- Automatic profile setup supports Windows and Linux with NeoForge. The matching NeoForge version must already be installed in Minecraft Launcher.
- Linux launcher opening expects `minecraft-launcher` on `PATH`; other launcher locations and distributions are not auto-detected.
- The Crew Channel has aggregate player count only until the server exposes a roster and messaging API.
- Sync has no progress bar or cancellation yet, and file scans do not report progress.
- Rotating the manifest signing key requires rebuilding the client with the matching public key.
- Voice relay integration is not implemented.
