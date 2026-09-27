# Mars Command Client

A desktop companion client for the **Mars** modded Minecraft server, built with Tauri v2, React, TypeScript and Tailwind CSS v4.

The client provides live server telemetry, signed pack verification, and a **Sync / update pack** action that installs or updates verified mods and overrides. Launching Minecraft is not implemented yet.

|           |                                            |
| --------- | ------------------------------------------ |
| Server    | `play.nexusgit.info`                       |
| Minecraft | 1.21.1                                     |
| Loader    | NeoForge 21.1.250                          |
| Voice     | `voice.nexusgit.info` *(not yet wired up)* |

---

## Features

### Live server telemetry

Polls the Mars server every 15 seconds from the Rust backend using the Minecraft Java server-list protocol, and reports online state, player count, latency, MOTD and server build.

Polling pauses while the window is hidden, never overlaps requests, and resolves every failure into a well-formed offline result — the dashboard stays usable with the server down or the network unplugged.

> **Note**
> The proxy in front of Mars rejects the conventional `-1` protocol-version sentinel and closes the connection. The client sends `767` (1.21.1) instead. Changing this back will make the server appear permanently offline.

### Signed pack manifest

The launcher fetches a manifest describing every managed file, verifies an **Ed25519 detached signature** against a public key compiled into the binary, and rejects anything that fails. A manifest that does not verify is discarded rather than partially applied.

### Integrity scan

Compares a local instance against the verified manifest and reports drift:

| Verdict      | Meaning                                  |
| ------------ | ---------------------------------------- |
| `ok`         | Matches the manifest                     |
| `missing`    | Required file absent                     |
| `corrupt`    | Present but wrong contents               |
| `modified`   | Differs, but declared user-editable      |
| `foreign`    | Unlisted file inside a managed directory |
| `unreadable` | Could not be read                        |

CurseForge mods are checked by exact filename, file size, and the SHA-1 checksum published by CurseForge. The scanner also reports unlisted `.jar` files; matching jar counts alone are not considered sufficient.

### Sync and update

Sync downloads only missing or outdated files, stages each file beside its destination, validates its size and checksum, and replaces the destination only after verification. Downloads come directly from the HTTPS URLs in the signed manifest; the CurseForge API key is used only by the release workflow, never by the launcher.

For `mutable` config files, an update is applied only when the local file still matches the version previously installed by Mars Command. Locally edited configs are preserved and reported as conflicts. Removed managed files are deleted only when they still match the last installed checksum. Files that CurseForge does not allow to be distributed must be fetched manually from their source page.

### Launch gating

The launch control stays locked until every condition holds:

```text
manifest signature valid
  AND instance folder selected
  AND scan completed without error
  AND missing + corrupt + unreadable == 0
  AND mods present == mods pinned
```

The button names the specific blocker, e.g. `LAUNCH LOCKED // MOD COUNT MISMATCH`.

---

## Development

```bash
npm install
npm run tauri dev     # run the app
npm run build         # tsc + vite build
```

```bash
cd src-tauri
cargo check           # compile the backend
cargo test --lib      # unit tests (path-traversal safety)
```

Requires a Rust toolchain and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/).

### Project layout

```text
src/
  components/      Panel, TitleBar, ServerStatusPanel, DeploymentPanel
  hooks/           useMarsServerStatus, usePackIntegrity
  lib/             mars (config), format, launch (gating rules)
  types/           mars, manifest
src-tauri/src/
  minecraft.rs     SRV resolution + Java status protocol
  manifest.rs      Schema, fetch, Ed25519 verification
  integrity.rs     Local scan and drift report
  settings.rs      Persisted client settings
  sync.rs          Staged downloads, checksum verification, conflict handling
  lib.rs           Tauri commands and managed state
src-tauri/examples/
  manifest_tool.rs Maintainer CLI (not shipped in the app)
```

### Tauri commands

| Command                            | Returns                 |
| ---------------------------------- | ----------------------- |
| `get_minecraft_status(host, port)` | `MinecraftServerStatus` |
| `refresh_manifest()`               | `ManifestStatus`        |
| `scan_instance()`                  | `IntegrityReport`       |
| `sync_instance()`                  | `SyncResult`            |
| `get_client_settings()`            | `ClientSettings`        |
| `choose_instance_root()`           | `string` or `null`      |
| `clear_instance_root()`            | —                       |

### Build-time configuration

| Variable | Default |
| -------------------------- | --------------------------------------------------------------------------------------- |
| `MARS_MANIFEST_BASE_URL` | `https://github.com/JayNightmare/Mars-Command-Client-Launcher/releases/latest/download` |
| `MARS_MANIFEST_PUBLIC_KEY` | Hex Ed25519 key compiled into `manifest.rs` |
| `CURSEFORGE_API_KEY` | Maintainer-only API key used by the manifest tool; never shipped in the app |
| `CURSEFORGE_API_URL` | `https://api.curseforge.com` |
| `MARS_OVERRIDE_BASE_URL` | HTTPS base URL for the committed CurseForge `overrides/` files |

Useful for testing against a local manifest server:

```bash
MARS_MANIFEST_BASE_URL=http://127.0.0.1:8799 npm run tauri dev
```

---

## Publishing a pack update

The client reads `releases/latest/download`, so **publishing a release is what ships an update**. Users pick it up on their next sync.

### Automated

Run the **Publish pack manifest** workflow (`workflow_dispatch`) with a pack version. It resolves CurseForge project/file IDs, records exact filenames, sizes, SHA-1 checksums and distribution permissions, hashes the override files, signs the manifest, verifies it, and publishes the release. Override download URLs are pinned to the source commit.

Requires repository secrets **`CURSEFORGE_API_KEY`** (an approved key for the launcher/third-party API use) and **`MARS_SIGNING_KEY`** (the hex contents of the signing private key). Store the API key in the local `.env` for maintainer CLI runs; never commit `.env` or put the API key in client build settings.

### Manual

```bash
cd src-tauri

# 1. Convert the CurseForge export (requires CURSEFORGE_API_KEY in the environment or root .env)
cargo run --example manifest_tool -- cf-pack "../modpack/Mars Client" 1.2.0 ../manifest-dist/manifest.json

# 2. Sign it
cargo run --example manifest_tool -- sign ../manifest-keys/mars-signing.key ../manifest-dist/manifest.json

# 3. Confirm it verifies against the key in this build
cargo run --example manifest_tool -- verify ../manifest-dist/manifest.json
```

Upload `manifest.json` and `manifest.json.sig` as release assets.

Other subcommands:

```bash
manifest_tool keygen <private-key-out>                          # new Ed25519 keypair
manifest_tool build <instance-root> <version> <manifest-out>    # hash a working instance
```

---

## Manifest schema

```jsonc
{
  "schemaVersion": 1,
  "packVersion": "1.0.0",
  "minecraftVersion": "1.21.1",
  "loader": "neoforge",
  "loaderVersion": "21.1.250",
  "generatedAt": "2026-09-27T16:08:16Z",

  // Fully owned by Mars Command; unlisted files here are reported as foreign.
  "managedDirs": ["config", "kubejs", "defaultconfigs"],

  "files": [
    {
      "path": "config/example.toml",   // forward slashes, relative, no traversal
      "sha256": "...",
      "size": 129,
      "required": true,
      "mutable": true,                 // drift reported, never treated as corruption
      "side": "client",
      "downloadUrl": "https://...",    // HTTPS only; raw override URL
      "manualDownload": false,         // upstream forbids automated download
      "sourcePage": null
    }
  ],

  // CurseForge file details resolved at publish time. Mods use the API's SHA-1.
  "curseforgeMods": [
    {
      "projectId": 401648,
      "fileId": 5873258,
      "required": true,
      "fileName": "example-mod.jar",
      "size": 123456,
      "sha1": "...",
      "downloadUrl": "https://...",
      "manualDownload": false,
      "sourcePage": "https://www.curseforge.com/..."
    }
  ],
  "modsDir": "mods"
}
```

Signed with a detached `manifest.json.sig` containing a hex Ed25519 signature over the **exact bytes** of `manifest.json`. Re-sign after any edit, including whitespace.

---

## Security model

- **Signature is checked before parsing.** Malformed input never reaches the deserializer.
- **Fails closed.** A bad signature clears the cached manifest instead of leaving stale data scannable.
- **Path traversal is rejected** at manifest load *and* again at path-join time. `..`, absolute paths, drive letters, backslashes and NUL are all refused.
- **HTTPS is enforced** for every `downloadUrl` and `sourcePage`, so a signed manifest cannot downgrade a download to an interceptable transport.
- **Private keys are never committed.** `manifest-keys/` and `.env` are gitignored; CI reads the key from a secret, writes it outside the workspace, and shreds it.

> **Warning**
> The key currently compiled into the client is a **development key**. Generate a fresh pair and rotate `MARS_MANIFEST_PUBLIC_KEY` before any public release.

---

## Why manifests are built at publish time

CurseForge's Core API requires an `x-api-key` header. Shipping that key in the launcher would expose it to every user (trivially extractable from the binary) and breaches the CurseForge for Studios Terms of Use, which prohibit providing API access to third parties.

Resolving CurseForge at publish time instead keeps the key in CI, removes the runtime dependency on CurseForge being up, avoids per-user rate limits, and lets the result be signed.

---

## Known limitations

- **Older manifests are not installable.** A manifest containing only CurseForge IDs lacks the filenames, checksums, and URLs required for safe sync; publish a refreshed manifest before using Sync.
- **CurseForge SHA-1 is used for mod files.** Config and override files use SHA-256. All expected checksums are covered by the signed manifest.
- **Non-distributable mods require manual installation.** The launcher will not bypass CurseForge's `allowModDistribution` setting.
- **Sync has no progress bar or cancellation yet.** It processes downloads on a worker thread and reports results when complete.
- **No launching.** The gate is enforced, but the launch path is unimplemented.
- **Scanning is unthrottled** — roughly 14 s for ~1300 files, with no progress reporting.
- **No key rotation path.** A rotated signing key requires a client rebuild.
- Latency comes from the ping packet round-trip and shows `—` where a proxy drops it.
- MOTD is flattened to a single line; colour codes are stripped, not rendered.

---

## Roadmap

1. Progress reporting, cancellation, and conflict resolution UI for pack sync.
2. Prism / vanilla launcher integration and the launch path itself.
3. Voice relay (`voice.nexusgit.info`).
4. Diagnostics screen surfacing the preserved error detail.

---

## Recommended IDE setup

[VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
