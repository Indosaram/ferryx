# Ferryx Local-Release Hardening: Implementation Contract

Date: 2026-09-08. Status: Approved Implementation Contract. Scope: Local multi-host release pipeline.
Deliverable: Production hardening across Node/Bun scripts, Windows packaging, and coordinator tooling.

## 1. Architecture, Runtimes, and Host Boundaries
- **Runtime:** Pure ESM (`.mjs`), compatible with Node.js >= 22.22 and Bun >= 1.4. Zero external npm dependencies; use standard library (`node:crypto`, `node:fs`, `node:path`, `node:child_process`). Tests run via `node --test`.
- **Coordinator:** Local MacBook (Apple Silicon arm64). No local PowerShell; coordinates DAG stages, builds macOS universal packages, verifies signatures, and manages draft publishing.
- **Remote Builders via SSH:**
  - **Linux:** `omaki` (`indo@100.91.254.71`). Builds x86_64 AppImage and DEB. SSH flags: `BatchMode=yes`, `ConnectTimeout=10`. Dedicated run dir: `/home/indo/ferryx-builds/<runId>`.
  - **Windows:** `maho-win` (`DESKTOP-1LAPJMP`). Runs PowerShell remotely; builds x64 binary and Store MSIX using verified MSVC link.exe (`14.44.35207`) and Windows SDK MakeAppx (`10.0.26100.0`). Dedicated run dir: `C:\Users\sook\ferryx-builds\<runId>`.
- **Resource & Disk Safety:** Disk requirements are explicit per-host configuration, not a proven 8Gi universal-build threshold. The example macOS budget is conservatively 30Gi, configurable after measuring a real isolated build. Preflight fails closed below the configured budget; success is not a build certification. Never wipe user caches or alter installed apps/daemons.

## 2. Independent Write Scopes and Wave Ordering
Parallel implementation must proceed in strict wave order with isolated file ownership:
- **Wave 1 — Version & Policy:**
  - Files: `scripts/sync-version.mjs`, `scripts/sync-version.test.mjs`, `scripts/release-workflow-policy.mjs`, `.github/workflows/release.yml`, `.github/workflows/build-test.yml`.
  - Scope: Strict calendar version validation, atomic multi-file stamping, CI release producer retirement, workflow policy enforcement (PR checks & Pages intact).
- **Wave 2 — Artifact Verification & Manifest:**
  - Files: `scripts/lib/minisign-verify.mjs`, `scripts/build-latest-json.mjs`, `scripts/build-latest-json.test.mjs`, `scripts/assert-updater-archive-layout.mjs`.
  - Scope: Cryptographic signature verification, complete platform matrix gate, collision rejection, and comprehensive `SHA256SUMS.txt` generation.
- **Wave 3 — Windows Packaging:**
  - Files: `scripts/build-msix.ps1`, `scripts/build-msix.test.mjs`.
  - Scope: Explicit `-ExePath` binding, Quad version parsing, strict `$LASTEXITCODE` checks, removal of hardcoded credentials, and explicit Store mode.
- **Wave 4 — Coordinator & Remote Adapters:**
  - Files: `scripts/release-coordinator.mjs`, `scripts/lib/platform-adapters/{mac,linux,windows}.mjs`, `docs/releases/LOCAL_RELEASE_RUNBOOK.md`.
  - Scope: End-to-end stage orchestration (`prepare`, `preflight`, `build`, `verify`, `assemble`, `publish`), receipt collection, and draft publish gate.

## 3. Shared Version API & Mapping Contract
- **Input Release Tag:** `vYYYY.MM.DD` or `vYYYY.MM.DD.R` (e.g. `v2026.09.08.1`).
- **Validation Rules:** Strictly validate calendar dates: Year >= 2026, Month 1..12, Day 1..31 (respecting month lengths and leap years), optional revision R >= 0. Reject malformed syntax and impossible calendar dates (e.g. month 13, day 32).
- **Format Mappings:**
  - **App / Updater SemVer:** `YYYY.(MM * 100 + DD).R` (e.g. `v2026.09.08.1` -> `2026.908.1`).
  - **Windows MSIX Quad:** `YYYY.(MM * 100 + DD).R.0` (e.g. `2026.908.1.0`).
- **Exported API (`scripts/sync-version.mjs`):**
  - `parseReleaseTag(tag: string): { year: number, month: number, day: number, revision: number }`
  - `toAppVersion(tag: string): string`
  - `toMsixVersion(tag: string): string`
  - `syncVersion({ tag: string, confPath?: string, cargoPath?: string, dryRun?: boolean }): Promise<{ version: string, msixVersion: string }>`
- **Write safety:** Validate and prepare both documents before any mutation. Use unique sibling temporary files, not fixed `.tmp` names, and roll back completed replacements on a synchronous write/rename error. No cross-filesystem crash-atomicity claim. Keep legacy valid SemVer support in the standalone stamping CLI; coordinator release plans require date tags.

## 4. Schemas: Release Plan & Build Receipt (Credential-Free)
All schemas are strict JSON. Credentials, tokens, private keys, or passwords MUST NOT appear in plans, receipts, or logs.
- **Release Plan (`release-plan.json`):**
  ```json
  {
    "$schema": "https://ferryx.dev/schemas/release-plan-v1.json",
    "runId": "rel-20260908-1",
    "commitSha": "5d5499806a1b207849778f488b4e3e7b821a751b",
    "tag": "v2026.09.08.1",
    "appVersion": "2026.908.1",
    "msixVersion": "2026.908.1.0",
    "channels": { "store": true, "nsisMigration": false },
    "requiredTargets": ["darwin-aarch64", "darwin-x86_64", "linux-x86_64"],
    "toolchains": { "node": ">=22.0.0", "bun": ">=1.4.0", "zig": "0.16.0" },
    "createdAt": "2026-09-08T12:00:00.000Z"
  }
  ```
  *(Note: If `channels.nsisMigration` is true, `windows-x86_64` is added to `requiredTargets`)*.
- **Build Receipt (`build-receipt-<host>.json`):**
  ```json
  {
    "$schema": "https://ferryx.dev/schemas/build-receipt-v1.json",
    "runId": "rel-20260908-1",
    "host": "macbook",
    "commitSha": "5d5499806a1b207849778f488b4e3e7b821a751b",
    "appVersion": "2026.908.1",
    "completedAt": "2026-09-08T12:30:00.000Z",
    "exitCode": 0,
    "artifacts": [
      {
        "name": "Ferryx.app.tar.gz",
        "relPath": "darwin/Ferryx.app.tar.gz",
        "bytes": 28491024,
        "sha256": "4f53cda...32",
        "signatureRelPath": "darwin/Ferryx.app.tar.gz.sig",
        "targets": ["darwin-aarch64", "darwin-x86_64"]
      }
    ]
  }
  ```

## 5. Platform Builder Contracts & Verification
- **MacBook (Host / Darwin):**
  - Invocation: `cargo tauri build --target universal-apple-darwin`.
  - Verification: `lipo -info` confirms `x86_64` and `arm64` slices. `codesign --verify --deep --strict` checks Developer ID. Notarize via `xcrun notarytool submit --wait` and staple with `xcrun stapler staple`. Validate archive layout via `assert-updater-archive-layout.mjs`.
- **omaki (Linux Builder):**
  - Invocation via SSH: execute within isolated `/home/indo/ferryx-builds/<runId>`. Fresh shallow clone pinned to `plan.commitSha`.
  - Build AppImage and DEB using pinned Zig 0.16.0 and WebKitGTK 4.1 / GTK3 / ALSA.
  - Verification: verify non-empty binaries, check package metadata and dependencies. Produce `build-receipt-omaki.json`.
- **maho-win (Windows Builder):**
  - Invocation via SSH: execute within `C:\Users\sook\ferryx-builds\<runId>`. Pin checkout to `plan.commitSha`.
  - Windows Store is PRIMARY: Run `build-msix.ps1 -ExePath <explicitPath> -Version <msixVersion> -OutputDir <out> -SkipSigning`. Check native `$LASTEXITCODE == 0` for `MakeAppx.exe`.
  - NSIS Migration Channel (Secondary/Optional): If `channels.nsisMigration` is true, build NSIS installer as legacy update bridge.
  - Verification: Validate MSIX manifest Identity `Name="ProjectMaho.Ferryx"` and `Version="<msixVersion>"`. Produce `build-receipt-maho-win.json`.

## 6. Real Cryptographic Signature Verification & Manifest Assembly
- **Signature Algorithm:** Minisign Ed25519 signatures wrapping Blake2b-512 (prehashed `ED` format) or raw messages (`Ed`), base64-encoded.
- **Verification Engine (`scripts/lib/minisign-verify.mjs`):**
  - Public Key: Base64 string in `src-tauri/tauri.conf.json:plugins.updater.pubkey`.
  - One portable implementation: native `node:crypto` with strict Minisign parsing, matching key id, Ed/ED algorithm handling, Ed25519 payload verification and trusted-comment/global signature verification. Export `verifyMinisign({data, signature, publicKey})` returning true or throwing on invalid input. No platform-dependent executable fallback; the installed minisign CLI is an independent QA oracle, not a runtime dependency.
  - **Fixture Test Seam:** Support parameter `--pubkey <base64>` in `build-latest-json.mjs` and verification library so test suites can exercise verification without modifying repository secrets.
- **Manifest Assembly Contract (`scripts/build-latest-json.mjs`):**
  - Required Inputs: `--plan <plan.json>`, `--receipts-dir <dir>`, `--artifacts-dir <dir>`, `--out <path>`, `--repo <repo>`.
  - Rules: Reject if any receipt has `commitSha` != `plan.commitSha` or `appVersion` != `plan.appVersion`.
  - Platform Completeness: Every target in `plan.requiredTargets` must have exactly one valid payload. Overwriting a target is a fatal collision error.
  - Verification Gate: Every updater artifact must cryptographically verify against `pubkey`. If signature missing, empty, or invalid, abort with exit 1.
  - Deterministic Aliases: Copy artifacts to stable names (`Ferryx_universal.dmg`, `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `Ferryx_x64.msix`, and `Ferryx_x64-setup.exe` if migration).
  - Checksum Closure: Generate `latest.json` FIRST. Compute `SHA256SUMS.txt` across ALL publish files, including `latest.json` (excluding only `SHA256SUMS.txt` itself).

## 7. CLI Commands, Workflow Boundaries, and Publish Approval
- **Coordinator CLI (`scripts/release-coordinator.mjs`):**
  - `prepare --tag <tag> [--sha <sha>] [--nsis-migration]`: Creates isolated `./release-run/<runId>/` and writes `release-plan.json`.
  - `preflight --plan <path> --config <hosts.json>`: Checks configured disk budgets, SSH connectivity, pinned toolchains and source access.
  - `build --plan <path>`: Triggers remote builds via SSH and local Mac build in dedicated directories.
  - `verify --plan <path>`: Gathers receipts, checks commit SHA and version identity, verifies artifact hashes.
  - `assemble --plan <path>`: Invokes `build-latest-json.mjs`, validates signatures, creates aliases, generates `SHA256SUMS.txt`.
  - `publish --plan <path> --approve-publish`:
    - **Approval Gate:** Publication fails immediately unless `--approve-publish` is passed AND confirmed via prompt or `FERRYX_APPROVE_PUBLISH=1`.
    - Creates GitHub Release as a DRAFT. Uploads all assembled assets.
    - Draft verification uses authenticated downloads, validates the exact asset inventory and bytes against local checksums, then undrafts. Public latest/download URL verification occurs only after publication. Never poll with fixed sleeps; bound subprocess/network calls. No actual GitHub mutation in this implementation session.
- **CI / GitHub Actions Policy Hardening:**
  - Remove hosted release build, sign, and publish jobs from `.github/workflows/release.yml`.
  - Retain PR check workflow (`.github/workflows/build-test.yml`) and documentation workflow (`.github/workflows/deploy-pages.yml`) untouched.
  - Add policy gate test (`scripts/release-workflow-policy.mjs`) failing CI if release build/sign/publish steps are reintroduced into GitHub Actions.

## 8. Backward Compatibility and Migration Deltas
- `scripts/sync-version.mjs`: Extends existing CLI (`--tag <tag>`) while adding calendar validation and atomic rollback. Existing caller contracts remain functional.
- `scripts/build-latest-json.mjs`: Requires receipt-driven validation (`--plan`). Legacy directory-glob arguments fail with migration guidance; do not preserve an alternate permissive publishing path.
- `scripts/build-msix.ps1`: Deprecates candidate path guessing; requires `-ExePath`. Adds mandatory `$LASTEXITCODE` checks after `MakeAppx` and `SignTool`. Replaces silent signing fallback with explicit `-SkipSigning` Store mode.

## 9. Binding worker API clarifications
- Coordinator entry point is `scripts/release-local.mjs` (not release-coordinator.mjs); modules remain Node/Bun compatible.
- Plan adds `schemaVersion: 1`, `repo: "Indosaram/ferryx"` and `sourceDateEpoch` (integer Git commit timestamp); createdAt may be derived from it. Local hosts config is separate from the credential-free plan. Plan validation rejects unknown properties.
- Receipt host enum is `macbook|omaki|maho-win`; artifact entries add `kind: macos-updater|dmg|appimage|deb|msix|nsis`. Kind determines permitted host, filename suffix, stable alias and updater targets; do not trust arbitrary declared targets.
- Every release requires all three host receipts, macos-updater+dmg, appimage+deb, and msix. NSIS is present iff nsisMigration=true. Require unique receipt host and artifact kind, exact runId/commitSha/appVersion and exitCode=0; hashes/bytes must match real files. Artifact and signature paths are relative under the artifacts root with realpath jail and no symlinks/escape.
- `build-latest-json.mjs` exports `assembleRelease({planPath, receiptsDir, artifactsDir, outDir, publicKey})`; CLI uses `--plan --receipts-dir --artifacts-dir --out-dir [--pubkey]`. It writes an entirely new outDir only after validation; any failure leaves no publishable partial output. Public key defaults to checked-in Tauri config.
- Publish verification must reject changed plan/receipts/files since assembly, confirm tag-to-SHA identity, and never overwrite an existing public release. Credentials are inherited process environment only and never serialized/logged.
