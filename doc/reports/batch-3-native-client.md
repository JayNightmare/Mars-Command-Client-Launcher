# Batch 3 native client implementation — 1.4.0

## Scope and boundaries

Implemented the approved native client submission/status scope only. Backend and website sources were not modified. Community now supports native JAR selection, immutable release reservation, permission acknowledgement, bounded streaming upload with transport progress, private status/list refresh, eligible scan retry, and confirmed withdrawal.

Bearer credentials and selected absolute paths stay in the Rust session. Frontend IPC receives only an opaque selection identity, filename, size, preview digest, mod IDs, validated owner-scoped capsule metadata, and progress counters. Logout/re-login generation checks discard late operations, including same-user re-login; HTTP 401 clears native credentials/selection. Frontend remounts clear reservations, selected metadata, progress and results.

Submission never invokes local capsule staging/activation, Personal Mods installation, signed base-pack writes, public downloads, or launch composition. Metadata validation is not malware scanning. `scan_blocked` is private/unpublished; `publishable` remains private and is only eligible for a future publication step.

## Files changed

- `src-tauri/src/community_submissions.rs`: typed contracts/commands, bounded responses, native picker, same-handle validation/hash/stream, progress, safe error mapping and eight regression tests.
- `src-tauri/src/community.rs`: session-held selection/upload lock and nested submission module; login clears selection.
- `src-tauri/src/lib.rs`: seven native command registrations.
- `src-tauri/src/personal_mods.rs`: reusable bounded archive descriptor reader and submission mod-ID validation; existing install behavior preserved.
- `src/types/capsules.ts`, `src/lib/capsules.ts`, `src/lib/capsuleValidation.ts`: typed frontend boundary, runtime guards, revision merges, state/action eligibility and progress validation.
- `src/components/CapsuleSubmissions.tsx`, `src/components/CommunityPage.tsx`, `src/hooks/useCommunityAccount.ts`: Community UI, operation serialization, guarded lifecycle and account epoch cleanup.
- `src/lib/capsuleValidation.test.mjs`, `package.json`: focused frontend regressions and `test:community` runner.
- `package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`: all six existing release version values synchronized to 1.4.0.
- Cargo enables existing Tokio filesystem and Reqwest streaming features and directly declares futures-util; no frontend dependency added.
- `README.md`, `.env.example`, this report: contract/deployment guidance and fail-closed boundaries.

## Validation and results

Commands ran in the client root unless noted:

- `npm.cmd run test:community`: **14 passed** (eight capsule tests plus existing account/profile guards). Covers selection bounds/cancellation, malformed and foreign responses, blocked/rejected/terminal states, evidence/digest validation, stale revisions, unrelated/account-switched progress and metadata constraints.
- `npm.cmd run build`: **passed** (`tsc` and Vite production build).
- In `src-tauri`: `cargo test --lib --quiet`: **78 passed**, including eight new native submission tests and all existing account, Personal Mods and local capsule regressions. Local mock HTTP verifies bearer-only native headers, idempotency, exact raw upload bytes/digest binding, disconnects, changed selection, midstream truncation, lock release, blocked retry/withdraw, sanitized errors, 401 cleanup and same-user session replacement.
- In `src-tauri`: `cargo fmt --check` and `cargo check --lib --quiet`: **passed**.
- `npm.cmd run tauri -- build --debug --no-bundle`: **passed**, producing `src-tauri/target/debug/mars-command-client.exe`; no installer, release/tag or artifact execution performed.
- Node version-surface comparison: **all six values 1.4.0**.
- `git diff --check`: **passed**. Editor Problems reported no errors in the changed frontend component, guard or account hook.

Initial validation found a missing Panel icon, unsupported test helper method, and nested tests not discovered by the Rust harness; these were corrected. The first actually linked HTTP upload tests exposed a Windows common-controls loader dependency through concrete AppHandle emission. Transport now takes a typed progress callback; the production command emits Tauri events while native transport tests use a non-GUI callback. The full discovered suite then passed.

## Authoritative cross-repository contract

Backend implementation owner confirmed:

- API base excludes `/api`; production HTTPS, existing loopback HTTP development allowance, no redirects.
- `POST /api/community/capsules` body `{project,version,sourceUrl}`, required `Idempotency-Key` (1–128 ASCII alphanumeric or `._:-`), 201 including replay.
- `GET /api/community/capsules/mine` returns `{capsules:[Capsule]}`; `GET /api/community/capsules/{releaseId}` returns Capsule.
- `PUT /api/community/capsules/{releaseId}/artifact` is raw `application/java-archive`; identical-byte replay returns status, different bytes conflict.
- Bodyless `POST` `/{releaseId}/retry` and `/{releaseId}/withdraw` return Capsule. All mutations accept the existing desktop bearer.
- Capsule has releaseId (32 lowercase hex), ownerId, project, version, sourceUrl, createdAt, artifactSha256 (64 lowercase hex or null), state, positive revision, updatedAt, evidence/queue or null, and `publicDownloadAvailable:false`; singular responses are not envelopes.
- Evidence binds version, artifact digest, provider/result ID, policy version, scanned/expires timestamps, verdict and nonempty summary. Queue exposes status, attempts/maxAttempts, nextAttemptAt and lastError, never worker lease or storage paths.
- Metadata project/version bounds are 120/80 trimmed characters. Source is at most 2048 characters, public HTTPS, no credentials/fragment/backslash/control/whitespace, absent/443 port, dotted hostname, no localhost/.localhost/.local. Backend additionally rejects non-global literal IPs; no DNS resolution or source fetching occurs. Desktop performs helpful syntax/hostname checks; backend public-IP validation remains authoritative.
- Default limits: 64 MiB/JAR, 10 active/account, seven-day abandoned/failed expiry; operator-configurable server limits, no runtime capabilities endpoint. Desktop ceiling remains 64 MiB.
- Rejected is terminal. Retry requires scan_blocked and attempts remaining; UI additionally requires blocked/failed queue. Pending replay is idempotent. Withdraw accepts uploading/publishable; expired rejects, withdrawn replays.
- Stable error envelope `{detail:{code,message}}`; native maps known codes to safe local messages and never reflects arbitrary server messages. Supported codes include release_not_found, idempotency_conflict, upload_in_progress, release_already_bound, invalid_state, submission_limit, retry_not_eligible, artifact_too_large/empty/invalid, invalid_input, unsupported_media_type, storage_unavailable, upload_timeout, plus existing auth/rate failures.

## Deployment and acceptance limitations

No live OAuth/device approval, deployed backend upload/worker flow, mounted React interaction suite, production scanner, installer execution, Windows/Linux release installer packaging, or cross-host network operation was certified. Tests use project-local fixtures/mock HTTP and clean them automatically. Progress is transport-read progress, not acknowledgement; server status/digest response is authoritative.

Private bucket permissions, durable worker deployment, backup/restore, policy, scanner evidence and expiry cleanup are backend operational requirements, not desktop configuration. Production scanning remains fail closed unless explicitly configured. Parent review/cross-repository integration acceptance is pending; roadmap milestones were not marked complete.
