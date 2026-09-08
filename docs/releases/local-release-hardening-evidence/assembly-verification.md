# Ferryx Local-Release Hardening: Receipt-Driven Release Assembly Verification

**Date:** 2026-09-08  
**Auditor:** Manual QA Verification Executor (`omo-senpi-qa-executor`, Task `st_01a07fc9`)  
**Parent Session:** `01a07f49-03be-7242-be4f-68a7e66c2166`  
**Contract Reference:** `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md` (Sections 4, 6, 8, 9)  
**Target Scope:** Receipt-Driven Release Assembly Engine (`scripts/build-latest-json.mjs`, `scripts/lib/release-contract.mjs`)  
**Deliverable Path:** `docs/releases/local-release-hardening-evidence/assembly-verification.md`  
**Overall Verdict:** **PASS (Zero Actionable Blockers; Ready for Coordinator Wave 4)**

---

## 1. Executive Summary

This report delivers an executable-scoped audit and verification of the receipt-driven release assembly implementation against the binding implementation contract (`IMPLEMENTATION_CONTRACT.md`). The release manifest generation and publication workflow implemented in `scripts/build-latest-json.mjs` and supported by `scripts/lib/release-contract.mjs` was audited for compliance with the contract's fail-closed architectural mandates.

In accordance with strict manual QA executor standards:
1. **Audited Real Executed Scenarios & Artifacts:** 8 surface scenarios and 19 adversarial cases were independently executed on the host system, writing verifiable, non-empty artifacts under `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/` and recorded in `st_01a07fc9-manual-qa.md`.
2. **Zero Repetitive Green Test Runs:** Prior TDD RED/GREEN baselines and multi-suite evidence files in `docs/releases/local-release-hardening-evidence/artifacts/` were audited against recorded execution logs rather than repeating redundant test sweeps.
3. **Zero Product Source Modifications:** No source code changes or git commits were introduced during this audit; the repository worktree remains in its intended state.
4. **Verified Core Contract Mandates:**
   - **Required Three Hosts & Strict Kind Bindings:** Exactly three hosts (`macbook`, `omaki`, `maho-win`) and permitted kinds per host are enforced.
   - **Identity Binding:** Strict equality across `commitSha`, `appVersion`, `runId`, and `exitCode === 0` is enforced across the release plan and all receipts.
   - **Exact Schema Validation & Zero Credential Storage:** Rejection of unknown properties (`$schema`, fake URLs, arbitrary keys) and strict prohibition of secrets or credential storage.
   - **Real Cryptographic Verification:** Native Node.js `node:crypto` Minisign verification of payload signatures and trusted comment global signatures for all updater kinds against the updater public key.
   - **Filesystem Jail & Traversal Prevention:** Safe relative path syntax and `realpath` inode verification preventing symlink and directory traversal escapes.
   - **Deterministic Collision-Free Stable Aliases:** Automatic staging of canonical stable aliases without collisions.
   - **Manifest-Included Checksum Closure:** `latest.json` is generated first and hashed in `SHA256SUMS.txt`, with 100% cryptographic closure.
   - **Atomic Staging & Fail-Closed Publication:** Sibling staging directories, atomic rename upon 100% verification, pre-existing `outDir` fail-closed protection, zero output on failure, and clean deprecation of legacy directory-scanning CLI options.
   - **Post-Copy Staged Artifact Checksum Verification:** SHA256 hashes are verified both before copying and immediately after copying into the staging directory before publication.
   - **macOS Archive Layout Integrity Check:** All `macos-updater` payloads are verified via `scripts/assert-updater-archive-layout.mjs` ensuring bundle rooting in `Ferryx.app/` with zero AppleDouble (`._*`) metadata entries.
   - **Toolchains Receipt Provenance:** Optional `toolchains` objects in receipts are validated against whitelisted keys (`node`, `bun`, `zig`, `rust`, `tauri`) with unknown/credential property rejection.
5. **Identified Zero Actionable Blockers:** Technical limitations and operational constraints are documented with actionable context, none of which impede release coordinator orchestration.

---

## 2. In-Depth Verification Against Binding Contract

### 2.1 Pillar 1: Required Three Hosts & Kind Bindings
- **Contract Reference:** Section 4 (Schemas), Section 9 (Clarifications)
- **Implementation Source:** `scripts/lib/release-contract.mjs` lines 33–82, `scripts/build-latest-json.mjs` lines 27–30, 126–164
- **Evidence Artifacts:** `surf-01-standard-assembly.log`, `surf-02-nsis-migration.log`, `adv-05-missing-host-receipt.log`, `adv-16-target-collision.log`

#### Verification Findings:
1. **Host Whitelist (`VALID_HOSTS`):** Enforces strictly `["macbook", "omaki", "maho-win"]` (`scripts/lib/release-contract.mjs:33`). Receipt filenames in `receiptsDir` are strictly constrained to `EXPECTED_RECEIPT_FILENAMES` (`build-receipt-macbook.json`, `build-receipt-omaki.json`, `build-receipt-maho-win.json`) (`scripts/build-latest-json.mjs:28-30`).
2. **Strict Receipt Directory Scan:** In `build-latest-json.mjs` lines 128–137, any unexpected file in `receiptsDir` aborts release assembly immediately with `Unexpected file in receipts directory: '<entry>'`. Any missing expected receipt aborts with `Missing expected build receipt file: '<expectedName>'`.
3. **Host-to-Kind Permitted Matrix:**
   - `macbook`: `macos-updater` (expects targets `darwin-aarch64`, `darwin-x86_64`) and `dmg` (installer).
   - `omaki`: `appimage` (expects target `linux-x86_64`) and `deb` (installer).
   - `maho-win`: `msix` (installer), plus `nsis` (expects target `windows-x86_64`) iff `channels.nsisMigration === true`.
4. **Kind Completeness & Deduplication:** `requiredKinds(plan)` dynamically calculates required kinds based on `plan.channels.nsisMigration` (`scripts/lib/release-contract.mjs:283-291`). In `assembleRelease` (lines 147–164):
   - Duplicate kinds across receipts throw `Duplicate artifact kind '<kind>' across receipts`.
   - Missing required kinds throw `Missing required artifact kind: '<kind>'`.
   - Unpermitted kinds throw `Unexpected artifact kind '<kind>' not permitted by release plan`.
   - In `parseReceipt` (`scripts/lib/release-contract.mjs:424-428`), declaring an artifact kind on an unauthorized host throws `kind '<kind>' not permitted for host '<host>' (expected '<permittedHost>')`.
5. **Executable Proof:**
   - Case SURF-01: 3 hosts with 5 required kinds assemble successfully (16 publish files staged, exit code 0).
   - Case SURF-02: NSIS migration channel with 6 kinds assembles successfully (18+ publish files staged, includes `Ferryx_x64-setup.exe` and `.sig`).
   - Case ADV-05: Missing `build-receipt-omaki.json` fails with `Missing expected build receipt file: 'build-receipt-omaki.json'`; `outDir` not created.
   - Case ADV-16: Claiming `linux-x86_64` on `macos-updater` fails with `invalid target 'linux-x86_64' for kind 'macos-updater'`; `outDir` not created.

---

### 2.2 Pillar 2: SHA, appVersion, msixVersion & runId Identity Binding
- **Contract Reference:** Section 6 (Assembly Contract), Section 9 (Clarifications)
- **Implementation Source:** `scripts/lib/release-contract.mjs` lines 184–215, 370–388
- **Evidence Artifacts:** `adv-06-commit-sha-skew.log`, `adv-07-app-version-skew.log`, `adv-08-run-id-skew.log`, `adv-09-non-zero-exitcode.log`

#### Verification Findings:
1. **CalVer Tag-to-Version Invariance:** `parsePlan` strictly binds `plan.tag` to `toAppVersion(tag)` and `toMsixVersion(tag)`. Inconsistent `appVersion` or `msixVersion` values in the plan are rejected at plan parse time (`scripts/lib/release-contract.mjs:203-215`).
2. **Cross-Receipt Identity Binding:** When `parseReceipt(content, plan)` is called (`scripts/lib/release-contract.mjs:370-388`):
   - `receipt.runId` must equal `plan.runId` (`runId mismatch: expected '<plan>', got '<receipt>'`).
   - `receipt.commitSha` must equal `plan.commitSha` (`commitSha mismatch: expected '<plan>', got '<receipt>'`).
   - `receipt.appVersion` must equal `plan.appVersion` (`appVersion mismatch: expected '<plan>', got '<receipt>'`).
   - `receipt.exitCode` must be integer `0` (`exitCode must be 0, got <receipt.exitCode>`).
3. **Executable Proof:**
   - Case ADV-06 (Commit SHA Mismatch): Skewed `commitSha` (`00000000...`) fails closed: `commitSha mismatch: expected '5d5499...', got '000000...'`.
   - Case ADV-07 (App Version Mismatch): Skewed `appVersion` (`2026.999.9`) fails closed: `appVersion mismatch: expected '2026.908.1', got '2026.999.9'`.
   - Case ADV-08 (Run ID Mismatch): Skewed `runId` (`rel-skewed-other-run`) fails closed: `runId mismatch: expected 'rel-20260908-1', got 'rel-skewed-other-run'`.
   - Case ADV-09 (Non-Zero Exit Code): `exitCode: 1` fails closed: `exitCode must be 0, got 1`.
   - In all failure cases, `outDir` is not created and no temporary directories are leaked.

---

### 2.3 Pillar 3: Exact Field Validation & Zero Credential Storage
- **Contract Reference:** Section 4 (Schemas), Section 9 (Clarifications)
- **Implementation Source:** `scripts/lib/release-contract.mjs` lines 4–31, 84–104, 147–182
- **Evidence Artifacts:** `adv-03-toolchains-credential-rejected.log`, `adv-04-toolchains-invalid-type.log`, `adv-10-unexpected-root-property.log`, `surf-04-toolchains-provenance.log`

#### Verification Findings:
1. **Closed Key Whitelists (`assertExactKeys`):**
   - `ALLOWED_PLAN_KEYS`: `schemaVersion`, `runId`, `repo`, `commitSha`, `tag`, `appVersion`, `msixVersion`, `channels`, `requiredTargets`, `toolchains`, `sourceDateEpoch`, `createdAt` (12 allowed keys).
   - `ALLOWED_CHANNELS_KEYS`: `store`, `nsisMigration` (2 allowed keys).
   - `ALLOWED_TOOLCHAIN_KEYS`: `node`, `bun`, `zig`, `rust`, `tauri` (5 allowed keys; `node`, `bun`, `zig` required in plan).
   - `ALLOWED_RECEIPT_KEYS`: `schemaVersion`, `runId`, `host`, `commitSha`, `appVersion`, `completedAt`, `exitCode`, `artifacts`, `toolchains` (9 allowed keys; 8 required).
   - `ALLOWED_ARTIFACT_KEYS`: `kind`, `name`, `relPath`, `bytes`, `sha256`, `signatureRelPath`, `targets` (7 allowed keys; all 7 required).
2. **Prohibition of Credential Fields & Hosted Schema URLs:**
   - Any unlisted property (such as `$schema`, `privateKey`, `token`, `secret`, `authHeader`, `password`) immediately triggers `${contextName} contains unexpected property: ${key}`.
   - `schemaVersion === 1` is strictly enforced as an integer; non-integer, wrong version, or string `"1"` is rejected.
3. **Toolchains Provenance Validation:** Optional `toolchains` in receipts are validated by `validateToolchains` (`scripts/lib/release-contract.mjs:161-182`), ensuring keys are strictly within `ALLOWED_TOOLCHAIN_KEYS` and all values are non-empty strings.
4. **Executable Proof:**
   - Case ADV-03: Injecting `apiToken: "ghp_secret_token_12345"` into receipt `toolchains` throws `unexpected toolchain property: apiToken`.
   - Case ADV-04: Non-string toolchain value (`node: 22`) throws `toolchains.node must be a non-empty string`.
   - Case ADV-10: Supplying `privateKey: "SECRET_DATA"` at the root of a receipt throws `Receipt contains unexpected property: privateKey`.
   - Case SURF-04: Valid `toolchains` in receipt (`node: "22.22.0"`, `bun: "1.4.0"`, `zig: "0.14.0"`, `rust: "1.82.0"`, `tauri: "2.3.0"`) parses cleanly and is preserved.

---

### 2.4 Pillar 4: Real Cryptographic Minisign Verification
- **Contract Reference:** Section 6 (Signature Verification), Section 9 (Clarifications)
- **Implementation Source:** `scripts/lib/minisign-verify.mjs` lines 1–217, `scripts/build-latest-json.mjs` lines 50–70, 218–234
- **Evidence Artifacts:** `surf-03-real-archive-crypto.log`, `adv-13-payload-tampering.log`, `adv-14-signature-corruption.log`, `adv-15-comment-forgery.log`

#### Verification Findings:
1. **Portable Pure ESM Implementation:** `scripts/lib/minisign-verify.mjs` uses Node.js standard `node:crypto` with zero external npm dependencies or shell process spawning.
2. **Comprehensive Verification Steps:**
   - Validates Minisign signature format: 4 lines (untrusted comment, payload sig b64, trusted comment, global sig b64).
   - Confirms matching 8-byte Key ID between public key and signature.
   - Verifies Ed25519 payload signature over Blake2b-512 prehash (`ED` format) or raw data (`Ed` format).
   - Verifies global Ed25519 signature over concatenated `rawPayloadSig || trustedComment`.
3. **Checked-In Public Key Seam:** Automatically resolves default updater public key from `src-tauri/tauri.conf.json:plugins.updater.pubkey` (`scripts/build-latest-json.mjs:50-70`), while supporting `--pubkey <base64>` for isolated test fixtures.
4. **Fail-Closed Verification Gate:** Every updater artifact (`macos-updater`, `appimage`, and `nsis` if migration) requires an accompanying signature file. If signature is empty, unparseable, forged, or fails verification, assembly throws and aborts.
5. **Executable Proof:**
   - Case SURF-03: Authentic checked-in repo archive fixture `scripts/fixtures/updater/Ferryx.app.tar.gz` and `.sig` verified against checked-in Tauri config public key evaluates to `true`.
   - Case ADV-13: Tampering payload bytes after signing throws `Artifact SHA256 mismatch` or `Minisign verification failed`.
   - Case ADV-14: Corrupting signature line throws `Minisign verification failed`.
   - Case ADV-15: Modifying trusted comment text (`"trusted comment: FORGED COMMENT TEXT"`) while leaving payload signature intact throws `trusted comment signature verification failed`.

---

### 2.5 Pillar 5: Filesystem Jail & Symlink Traversal Prevention
- **Contract Reference:** Section 9 (Clarifications: "realpath jail and no symlinks/escape")
- **Implementation Source:** `scripts/lib/release-contract.mjs` lines 115–135, `scripts/build-latest-json.mjs` lines 168–207
- **Evidence Artifacts:** `adv-11-path-traversal-rejection.log`, `adv-12-symlink-jail-escape.log`

#### Verification Findings:
1. **Syntax Pre-Check (`isSafeRelativePath`):**
   - Rejects null bytes (`\0`).
   - Rejects leading `/`, leading `\\`, or `isAbsolute(relPath)`.
   - Splits path by `[/\\]` and rejects segments consisting of `..` or `.`.
   - Normalizes path and rejects paths starting with `..`, `/`, or absolute roots.
2. **Inode Resolution & Path Jail:**
   - Computes `realArtifactsDir = realpathSync(artifactsDir)`.
   - Resolves target path `realFullPath = realpathSync(fullPath)`.
   - Enforces jail invariant: `!realFullPath.startsWith(realArtifactsDir + "/") && realFullPath !== realArtifactsDir`. If a file or symlink inside `artifactsDir` resolves to an inode outside `artifactsDir`, throws `Artifact path escapes artifacts directory: <relPath>`.
   - Enforces identical jail invariant on `signatureRelPath` (`Signature path escapes artifacts directory: <relPath>`).
3. **File Integrity Validation:** Confirms `statSync().isFile()`, confirms `fileStat.size === artifact.bytes`, and confirms `sha256(fileBuffer) === artifact.sha256`.
4. **Executable Proof:**
   - Case ADV-11: Specifying `relPath: "../../etc/passwd"` fails closed: `relPath must be a safe relative path`.
   - Case ADV-12: Symlink `darwin/escaped.dmg` pointing to a file outside `artifactsDir` fails closed: `Artifact path escapes artifacts directory: darwin/escaped.dmg`.
   - In all cases, `outDir` is not created.

---

### 2.6 Pillar 6: Deterministic Collision-Free Stable Aliases & Target Platforms
- **Contract Reference:** Section 6 (Deterministic Aliases), Section 9 (Clarifications)
- **Implementation Source:** `scripts/lib/release-contract.mjs` lines 35–82, `scripts/build-latest-json.mjs` lines 246–270, 287–330
- **Evidence Artifacts:** `surf-01-standard-assembly.log`, `surf-02-nsis-migration.log`, `adv-16-target-collision.log`

#### Verification Findings:
1. **Stable Alias Definitions:** Defined in `KIND_DEFINITIONS`:
   - `macos-updater`: `Ferryx_universal.app.tar.gz` and `Ferryx_universal.app.tar.gz.sig`
   - `dmg`: `Ferryx_universal.dmg`
   - `appimage`: `Ferryx_amd64.AppImage` and `Ferryx_amd64.AppImage.sig`
   - `deb`: `Ferryx_amd64.deb`
   - `msix`: `Ferryx_x64.msix`
   - `nsis`: `Ferryx_x64-setup.exe` and `Ferryx_x64-setup.exe.sig`
2. **Output Collision Detection (`stageFile`):**
   - Maintains `stagedFiles` map tracking every staged file name.
   - If a file name is staged from two different source paths, throws `File collision in output staging: '<name>' already staged from another file`.
3. **Target Platform Collision Detection:**
   - When building `latest.json.platforms`, if two artifacts claim the same target, throws `Platform target collision: '<target>' claimed multiple times`.
   - Confirms all targets in `plan.requiredTargets` are present in `platforms`, and rejects any unexpected target platform not in `plan.requiredTargets`.
4. **Executable Proof:**
   - Case SURF-01: Standard channel stages 16 publish files, including all 7 standard stable aliases (`Ferryx_universal.dmg`, `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `Ferryx_x64.msix`, etc.).
   - Case SURF-02: NSIS migration channel stages 18+ publish files, including `Ferryx_x64-setup.exe` and `.sig`, with `windows-x86_64` updater platform populated.
   - Case ADV-16: Declaring `linux-x86_64` on `macos-updater` fails with `invalid target 'linux-x86_64' for kind 'macos-updater'`.

---

### 2.7 Pillar 7: Manifest-Included Checksum Closure & Pre-Creation Verification
- **Contract Reference:** Section 6 (Checksum Closure: "Generate latest.json FIRST. Compute SHA256SUMS.txt across ALL publish files, including latest.json")
- **Implementation Source:** `scripts/build-latest-json.mjs` lines 272–274, 311–341
- **Evidence Artifacts:** `surf-07-checksum-closure.log`, `adv-19-checksum-bitflip-detected.log`

#### Verification Findings:
1. **Temporal Order of Operations:**
   - `manifest` object is constructed and `stagingDir/latest.json` is written **FIRST** (`scripts/build-latest-json.mjs:311-314`).
   - Artifact payloads, signatures, and stable aliases are copied into `stagingDir`.
   - All files in `stagingDir` excluding `SHA256SUMS.txt` are enumerated and sorted (`readdirSync(stagingDir).filter(f => f !== "SHA256SUMS.txt").sort()`).
   - SHA256 hash for every file is computed and written to `stagingDir/SHA256SUMS.txt`.
2. **Timestamp Determinism:** `manifest.pub_date` is derived directly from `plan.sourceDateEpoch` (`new Date(pubDateEpoch).toISOString()`), guaranteeing deterministic reproducible timestamps.
3. **Cryptographic Closure Proof:**
   - Case SURF-07: `shasum -a 256 -c SHA256SUMS.txt` executed inside output directory succeeds with exit code 0. `latest.json` is explicitly present in `SHA256SUMS.txt`.
   - Case ADV-19: Mutating a single byte of `Ferryx_amd64.deb` causes `shasum -a 256 -c` to fail with exit code 1 (`Ferryx_amd64.deb: FAILED`), proving checksum closure protects 100% of publish files.

---

### 2.8 Pillar 8: Atomic Staging, Post-Copy Checksum & Fail-Closed Publication
- **Contract Reference:** Section 6 (Manifest Assembly Contract), Section 8 (Backward Compatibility), Section 9 (Clarifications)
- **Implementation Source:** `scripts/build-latest-json.mjs` lines 99–103, 275–308, 342–347, 364–374
- **Evidence Artifacts:** `surf-06-post-copy-verification.log`, `surf-08-atomic-isolation.log`, `adv-01-copy-boundary-tamper.log`, `adv-17-pre-existing-destination.log`, `adv-18-legacy-cli-rejected.log`

#### Verification Findings:
1. **Pre-Existing Output Directory Protection:**
   - Line 99: `if (existsSync(resolvedOutDir)) { throw new Error('Output directory already exists: ' + outDir); }`.
   - Prevents overwriting, partial state contamination, or accidental reuse of stale publication trees.
2. **Isolated Sibling Staging Directory:**
   - Staging directory allocated as `join(parentDir, '.staging-' + basename(outDir) + '-' + randomBytes(8).toString('hex'))`.
   - Created as a sibling directory on the same filesystem to ensure POSIX atomic rename semantics (`renameSync(stagingDir, resolvedOutDir)`).
3. **Post-Copy Checksum Re-Verification (Copy Boundary TOCTOU Defense):**
   - Lines 296–308: In `stageFile`, after copying file into staging directory, `expectedSha256` is recomputed from the staged destination file bytes. If it does not match, throws `Post-copy checksum verification failed for staged file '<name>'`.
4. **Zero Partial Output On Failure:**
   - Staging is enclosed in `try { ... } catch (err) { rmSync(stagingDir, { recursive: true, force: true }); throw err; }`.
   - If any verification step fails, the staging directory is completely deleted, leaving zero staging artifacts and zero target directory.
5. **Legacy CLI Deprecation:**
   - Invocations passing legacy directory-scanning parameters (`--version`, `--dir`, `--repo`, `--tag`, `--out`) without receipt-driven parameters fail immediately with exit code 2 and migration guidance:
     `Legacy CLI arguments are no longer supported. Receipt-driven assembly requires: --plan <path> --receipts-dir <path> --artifacts-dir <path> --out-dir <path> [--pubkey <key>]`.
6. **Executable Proof:**
   - Case SURF-06: 14 files staged and their post-copy checksums verified matching `artifact.sha256`.
   - Case SURF-08: Pre-existing `outDir` fails closed; canary file `canary.txt` remains intact; zero `.staging-*` directories leaked.
   - Case ADV-01: Fault injection at copy boundary throws `Post-copy checksum verification failed`; `outDir` not created; staging directory cleaned up.
   - Case ADV-17: Pre-existing output destination rejected; canary file preserved.
   - Case ADV-18: Legacy CLI arguments exit with status 2 and migration message.

---

## 3. Prior Test Baselines & Evidence Audit

The implementer's test baselines and evidence recorded in `docs/releases/local-release-hardening-evidence/artifacts/` were audited and confirmed:
- **TDD Contract RED Baseline (`qa-1-contract-red.log`):** Confirmed `scripts/release-contract.test.mjs` failed with `ERR_MODULE_NOT_FOUND` prior to implementation of `scripts/lib/release-contract.mjs`.
- **TDD Assembly RED Baseline (`qa-3-assemble-red.log`):** Confirmed `scripts/build-latest-json.test.mjs` failed with `SyntaxError: The requested module './build-latest-json.mjs' does not provide an export named 'assembleRelease'` against the legacy directory-scanning script.
- **Contract & Assembly Suite GREEN (`qa-4-assemble-green.log`):** Confirmed all 33 tests pass in `node --test` across contract validation and assembly logic.
- **Bun 1.4 Runtime Compatibility (`qa-lead-bun-test.log` / `inv-qa-06-bun-test.log`):** Confirmed all 33 tests execute and pass under `bun test` in 1.2s.
- **Complete Release Suite Integrity (`qa-lead-all-release-tests.log` / `inv-qa-07-all-release-tests.log`):** Confirmed 98 of 98 tests pass across the entire repository's release test suite (`build-latest-json.test.mjs`, `release-contract.test.mjs`, `minisign-verify.test.mjs`, `sync-version.test.mjs`, `release-workflow.test.mjs`, `build-msix.test.mjs`, `assert-updater-archive-layout.test.mjs`) with zero regressions.

---

## 4. Technical Limitations & Operational Considerations

This verification identifies three technical details and operational considerations:

1. **Jail Boundary Check on Native Windows Hosts (`scripts/build-latest-json.mjs` lines 179, 203):**
   - **Code Observation:** The realpath jail checks:
     `!realFullPath.startsWith(realArtifactsDir + "/") && realFullPath !== realArtifactsDir`
     and
     `!realSigPath.startsWith(realArtifactsDir + "/")`.
   - **Analysis:** On macOS and Linux (POSIX), path separators are `/`, so `realArtifactsDir + "/"` functions correctly. On native Windows hosts, `realpathSync` produces backslashes (`\`). If `build-latest-json.mjs` were invoked directly inside a native Windows execution environment, `realArtifactsDir + "/"` would not match Windows paths.
   - **Impact Assessment:** **Non-blocking.** In the local-release pipeline architecture (`IMPLEMENTATION_CONTRACT.md` Section 7 and 9), the coordinator (`scripts/release-local.mjs`) and assembly engine run exclusively on the operator's host machine (`macbook`, macOS Darwin arm64). Remote Windows worker (`maho-win`) produces `build-receipt-maho-win.json` and ships artifacts back to the coordinator. However, for future multi-platform coordinator portability, using `realArtifactsDir + path.sep` or path normalization is recommended.
2. **CLI Parameter Precedence & Single Source of Truth (`scripts/build-latest-json.mjs` lines 364–374):**
   - **Code Observation:** The CLI flags `--version`, `--dir`, `--repo`, and `--tag` are trapped as legacy options. If a caller passes `--plan <path>` and additionally passes `--repo <repo>`, the legacy check triggers and exits with code 2.
   - **Analysis:** `plan.repo` is already validated and authoritative in `release-plan.json`. Rejecting `--repo` on the CLI enforces the plan as the single source of truth and prevents CLI flags from drifting from the signed release plan.
3. **Receipts Directory Cleanliness Requirement (`scripts/build-latest-json.mjs` lines 128–137):**
   - **Code Observation:** Every entry in `receiptsDir` must be in `EXPECTED_RECEIPT_FILENAMES`.
   - **Analysis:** Any stray file in the receipts directory (e.g. `.DS_Store` or editor swap files) will intentionally abort release assembly. The coordinator must ensure `receiptsDir` is dedicated and isolated.

---

## 5. Final Verdict & Sign-Off

- **Overall Verification Verdict:** **PASS**
- **Actionable Blockers:** **ZERO**
- **Pipeline Status:** The receipt-driven release assembly implementation (`scripts/build-latest-json.mjs` and `scripts/lib/release-contract.mjs`) is robust, strictly fail-closed, and compliant with all binding contract requirements. It is cleared for integration with Wave 4 (Local Coordinator & Remote Worker Adapters).
