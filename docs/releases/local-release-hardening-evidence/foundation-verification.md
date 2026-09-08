# Ferryx Local-Release Hardening: Foundational Implementation Lanes Verification

**Date:** 2026-09-08
**Auditor:** Manual QA Verification Executor (`omo-senpi-qa-executor`, Task `st_01a07faa`)
**Contract Reference:** `docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md`
**Target Scope:** Foundational Implementation Lanes (Waves 1–3: Version Stamping, Workflow Policy, Minisign Crypto, Windows MSIX Packaging)
**Deliverable Path:** `docs/releases/local-release-hardening-evidence/foundation-verification.md`
**Overall Verdict:** **PASS (READY FOR COORDINATOR WAVE 4)**

---

## 1. Executive Verification Summary

This audit independently verifies the completion and contract compliance of the four foundational implementation lanes for the Ferryx local-release pipeline. In strict accordance with the implementation contract, this verification:
1. **Audits real code diffs and executed evidence** rather than accepting summary declarations.
2. **Evaluates zero rerun tests** since all unit, integration, and remote test suites were already verified GREEN with recorded artifacts.
3. **Makes zero code modifications or git commits**, preserving the clean worktree state.
4. **Verifies all four core technical mandates**:
   - **Version API & Export Safety:** Canonical 4-function export contract, complete ESM import safety with zero CLI side effects, multi-manifest in-memory pre-validation, atomic rollback with `AggregateError` error preservation, and calendar date validation.
   - **Local-Only Workflow Policy Guard:** Complete retirement of hosted release producer (`.github/workflows/release.yml`), AST-parsed YAML validation with `Bun.YAML.parse`, recursive secret scanning, and preservation of PR checks (`build-test.yml`), Windows Cargo Link verification, and GitHub Pages deployment (`deploy-pages.yml`).
   - **Real Cryptographic Verification:** Native Node.js `node:crypto` Minisign verification with zero shell execution, pure SPKI DER public key construction, Blake2b-512 prehash (`ED`) and raw Ed25519 (`Ed`) verification, key ID matching, trusted comment global signature verification, and strict rejection of arbitrary base64 payloads.
   - **Windows MSIX Packaging & Executed Evidence:** Mandatory `-ExePath` binding, candidate path guessing elimination, Microsoft Store Quad contract (4th component = 0), PE `FileVersionInfo` binary version verification, isolated GUID staging, pre-pack stale output removal, native `$LASTEXITCODE` checks for `MakeAppx.exe` and `SignTool.exe`, credential auto-generation elimination, in-archive `AppxManifest.xml` validation, and verified remote Windows execution on `maho-win` with real SDK MakeAppx `10.0.26100.0`.
5. **Identifies zero concrete blocking issues** across the four lanes, clearing the release pipeline for Wave 4 (Coordinator & Remote Adapters).

---

## 2. Lane-by-Lane In-Depth Verification

### 2.1 Lane 1: Version Stamping & Verification Engine
**Primary Files:** `scripts/sync-version.mjs`, `scripts/sync-version.test.mjs`
**Evidence Source:** `docs/releases/local-release-hardening-evidence/version.md`, `version-manual-qa.md`
**Contract Sections:** Section 3 (Shared Version API & Mapping Contract), Section 8 (Backward Compatibility)

#### Contract Compliance Audit:
- **Canonical Exported API (`scripts/sync-version.mjs` lines 34–189):**
  - `parseReleaseTag(tag: string): { year: number, month: number, day: number, revision: number }` (lines 34–75): Parses CalVer tags (`vYYYY.MM.DD` or `vYYYY.MM.DD.R`), enforces Year >= 2026 and <= 65535, 2-digit month (01..12), 2-digit day (01..maxDays), leap years (`isLeapYear`), revision 0..65535, and forbids leading zeros in revision (`^v?\d{4}\.\d{2}\.\d{2}\.0\d+$`).
  - `toAppVersion(tag: string): string` (lines 88–91): Monotonically maps CalVer tags to `YYYY.(MM * 100 + DD).R` (e.g. `v2026.09.08.1` -> `2026.908.1`) and passes through legacy SemVer.
  - `toMsixVersion(tag: string): string` (lines 93–96): Monotonically maps CalVer tags to `YYYY.(MM * 100 + DD).R.0` (e.g. `v2026.09.08.1` -> `2026.908.1.0`) and legacy SemVer to `Major.Minor.Patch.0`. Rejects 4-part quad strings (`2026.908.1.0`) to prevent ambiguous re-encoding.
  - `syncVersion({ tag, confPath, cargoPath, dryRun }): Promise<{ version: string, msixVersion: string }>` (lines 133–189): Synchronizes both manifests with write-safety guarantees.
- **Import Safety & Direct Execution Guard (lines 220–231):**
  - Uses `isDirectExecution()` comparing `fileURLToPath(import.meta.url)` and `resolve(process.argv[1])` via `fs.realpathSync`.
  - When imported into tests or coordinator tools (`import { syncVersion } from "./sync-version.mjs"`), `isDirectExecution()` returns `false`, executing zero CLI side-effects and preventing accidental process exits.
- **Write Safety, Pre-Validation & Atomic Rollback (lines 139–186):**
  - **In-Memory Pre-validation:** Both `tauri.conf.json` and `Cargo.toml` are read and validated in memory via `prepareTauriConf` and `prepareCargoToml` before creating temporary files or mutating any disk state (lines 142–148).
  - **Sibling Temporary Files:** Temporary files are created as `${resolvedPath}.${randomUUID()}.tmp` directly in the parent directory of each file (lines 153–154), guaranteeing same-filesystem atomic renames via `renameSync(2)`.
  - **Synchronous Rollback:** If the second file rename fails, the first file is rolled back to `originalConf` (lines 170–176).
  - **AggregateError Seam:** If rollback or cleanup encounters errors (e.g. permission error during recovery), `syncVersion` throws an `AggregateError` combining the primary failure with all cleanup/rollback errors (lines 178–183), preventing silent failure masking.
- **Dependency Version Preservation in `Cargo.toml` (lines 111–131):**
  - The `prepareCargoToml` state machine only targets the `version` field directly under `[package]`. As soon as any other section header is encountered, `inPackage` becomes false and scanning terminates, ensuring dependency versions in `[dependencies]` remain byte-identical.
- **Legacy SemVer Compatibility (lines 24–32, 83):**
  - Standalone stamping CLI and mapping functions permit valid SemVer strings (e.g. `v1.4.2` -> `1.4.2`), ensuring backward compatibility for non-CalVer workflows while coordinator release plans require calendar tags.

**Lane 1 Verdict:** **PASS (Zero Blockers)**

---

### 2.2 Lane 2: Workflow Policy Enforcement & CI Hardening
**Primary Files:** `scripts/release-workflow-policy.mjs`, `scripts/release-workflow.test.mjs`, `.github/workflows/build-test.yml`
**Evidence Source:** `docs/releases/local-release-hardening-evidence/policy.md`, `st_01a07f9e-manual-qa.md`
**Contract Sections:** Section 7 (CI / GitHub Actions Policy Hardening), Section 8 (Backward Compatibility)

#### Contract Compliance Audit:
- **Hosted Release Producer Retirement:**
  - File `.github/workflows/release.yml` (348 lines of hosted build, codesign, notarize, and publish jobs) has been deleted (`git status` mode deletion confirmed).
  - Policy explicitly forbids `.github/workflows/release.yml` and `release.yaml` from existing in the repository (`release-workflow-policy.mjs` lines 53–56, 179–181).
- **AST YAML Parsing via `Bun.YAML.parse` (lines 161–171):**
  - Validates workflows by parsing structure into native objects rather than brittle line-based regular expressions. Gracefully traps YAML syntax errors without crashing (lines 164–168).
- **Unified Recursive Secret & Permission Scanning (lines 125–159):**
  - Traverses the entire workflow AST (mappings, arrays, scalar values, and keys) to detect forbidden release signing secrets (`TAURI_SIGNING_PRIVATE_KEY`, `APPLE_CERTIFICATE`, `APPLE_API_KEY`, etc.).
  - Strictly rejects `contents: write` and `write-all` at both workflow and job levels (lines 125–131).
- **Forbidden Action & Build Pattern Guards (lines 21–46, 92–123):**
  - Rejects release build entry points: `tauri build`, `@tauri-apps/cli build`, `cargo tauri build`, `bun tauri build`, and `build-msix.ps1`.
  - Rejects root build delegation: flags root `bun run build` / `npm run build` (where `package.json` maps `"build": "cargo tauri build"`), while permitting scoped frontend builds via `isScopedFrontendBuild()` (lines 48–55).
  - Rejects release actions (`tauri-action`, `action-gh-release`, `create-release`, `release-action`), manifest generation (`build-latest-json.mjs`), and coordinator CI invocation (`release-local.mjs`).
  - Rejects reusable workflows and `secrets: inherit` attempting to reintroduce release jobs (lines 76–83).
- **Preservation of PR Checks & Documentation Workflows:**
  - `.github/workflows/build-test.yml`: Retains frontend UI build/test, `cargo check`, debug `cargo build` for Windows Cargo Link, and remote grid frame seam checks. Adds minimal step `bun scripts/release-workflow-policy.mjs` in `ui-check` (lines 34–35).
  - `.github/workflows/deploy-pages.yml`: Intact and permitted. Pages deployment permissions (`contents: read`, `pages: write`, `id-token: write`) and scoped build `working-directory: ./site` pass policy validation cleanly without false positives.
- **Line Budget & Clean Code:**
  - Non-comment lines in `scripts/release-workflow-policy.mjs`: 232 lines (strictly compliant with the <= 250 line limit).
- **Pre-Existing Regex Failure Resolution:**
  - Retires pre-existing failure in `scripts/release-workflow.test.mjs` (which failed looking for `*-setup.exe` in the deleted `release.yml`). The replacement suite contains 16 behavioral parsed-policy tests, all passing GREEN.

**Lane 2 Verdict:** **PASS (Zero Blockers)**

---

### 2.3 Lane 3: Minisign Cryptographic Signature Verification
**Primary Files:** `scripts/lib/minisign-verify.mjs`, `scripts/minisign-verify.test.mjs`
**Evidence Source:** `docs/releases/local-release-hardening-evidence/crypto.md`, `crypto-manual-qa.md`
**Contract Sections:** Section 6 (Real Cryptographic Signature Verification), Section 9 (Worker API Clarifications)

#### Contract Compliance Audit:
- **Actual Node.js `node:crypto` Verification vs Text Transport:**
  - `scripts/lib/minisign-verify.mjs` imports only `{ createHash, createPublicKey, verify } from "node:crypto"`.
  - Production verification contains zero `child_process` invocations, zero shell commands, zero network transport, and zero mocked returns. Verification executes entirely within OpenSSL via Node's native crypto bindings.
- **Minisign Binary Layout & SPKI DER Wrapping (lines 78–98, 128–152):**
  - Parses 42-byte Minisign public keys (`Ed` marker + 8-byte key ID + 32-byte Ed25519 public key). Wraps raw key in standard ASN.1 DER SubjectPublicKeyInfo (`302a300506032b6570032100`) and constructs native `KeyObject` via `createPublicKey` (lines 90–95).
  - Parses 74-byte payload signature (line 2: `Ed`/`ED` marker + 8-byte key ID + 64-byte Ed25519 signature) and 64-byte global signature (line 4).
- **Dual Prehash & Raw Signature Verification (lines 189–198):**
  - **Blake2b-512 Prehash (`ED`):** Computes `createHash("blake2b512").update(dataBuffer).digest()` and verifies digest with `crypto.verify(null, digest, keyObject, payloadSig)`.
  - **Raw Legacy Ed25519 (`Ed`):** Directly verifies `crypto.verify(null, dataBuffer, keyObject, payloadSig)`.
- **Key ID Binding & Trusted Comment Global Signature (lines 182–187, 201–207):**
  - Confirms public key ID equals signature key ID: `parsedPub.keyId.equals(parsedSig.keyId)`.
  - Verifies global signature over concatenated `Buffer.concat([payloadSig, Buffer.from(trustedComment, "utf8")])`.
- **Strict "No Arbitrary Base64 Acceptance" (lines 14–36, 54–76, 114–126):**
  - Enforces canonical base64 decoding (`buf.toString("base64") === clean`).
  - Requires exact byte lengths (42 bytes pubkey, 74 bytes payload signature, 64 bytes global signature).
  - Unwraps outer Tauri base64 encoding only if decoded text starts with `"untrusted comment:"`.
  - Single-line public keys (`minisign -P`) must be exactly 56 base64 characters decoding to 42 bytes.
  - Arbitrary base64 payloads, truncated signatures, or non-minisign streams throw descriptive format errors.
- **Direct Minisign Test Oracle (`scripts/minisign-verify.test.mjs` lines 18–35):**
  - Uses the coordinator's installed `minisign` CLI directly as an independent two-way cross-verification oracle.
  - Zero test skipping (`t.skip` forbidden); fails with a clear prerequisite error if `minisign` is missing from `PATH`.
  - Proves bidirectional equivalence: Node-generated fixtures verify with `minisign -V`, and `minisign -S` signatures verify with `verifyMinisign`.

**Lane 3 Verdict:** **PASS (Zero Blockers)**

---

### 2.4 Lane 4: Windows MSIX Packaging & Native Tool Hardening
**Primary Files:** `scripts/build-msix.ps1`, `scripts/build-msix.test.mjs`, `scripts/test-build-msix.ps1`
**Evidence Source:** `docs/releases/local-release-hardening-evidence/msix.md`, `msix-manual-qa.md`, `st_01a07fa0-manual-qa.md`
**Contract Sections:** Section 1 (Architecture & Host Boundaries), Section 5 (Windows Builder Contracts), Section 8 (Backward Compatibility)

#### Contract Compliance Audit:
- **Mandatory `-ExePath` Binding & Candidate Guessing Deletion (`scripts/build-msix.ps1` lines 35–36, 61–68):**
  - `-ExePath` is declared mandatory `[Parameter(Mandatory = $true)] [string]$ExePath`.
  - Candidate array guessing (`$binaryCandidates` searching `src-tauri/target/release/ferryx.exe`) was completely deleted.
  - Fails closed immediately if `-ExePath` is omitted, empty, or does not resolve to an existing leaf file.
- **Canonical Version Normalization & Store Quad Contract (lines 70–149):**
  - Validates calendar dates (year >= 2026, month 1..12, days in month via `[DateTime]::DaysInMonth`).
  - Monotonically maps date tags to App SemVer (`YYYY.(MM * 100 + DD).R`) and MSIX Quad (`YYYY.(MM * 100 + DD).R.0`).
  - **Store Quad Rule:** Explicitly rejects any 4-part quad version where the 4th component is non-zero (lines 127–130), enforcing Microsoft Store ingestion rules.
- **PE `FileVersionInfo` Binary Verification (lines 154–171):**
  - Inspects PE header resources of `$resolvedExePath` via `[System.Diagnostics.FileVersionInfo]::GetVersionInfo()`.
  - Matches `ProductVersion`, `FileVersion`, or composite parts against `$appVersion` or `$msixVersion`. Rejects binary version skew before packaging.
- **Isolated Fresh Staging & Pre-Packaging Output Cleanup (lines 224–234, 410–414):**
  - Staging directory is allocated under `$env:TEMP` using a unique GUID (`ferryx-msix-staging-<guid>`).
  - Guarantees staging directory removal in a `finally` block.
  - Actively unlinks pre-existing `.msix` files at destination before `MakeAppx.exe` runs, eliminating false positives from stale artifacts.
- **Native Tool Exit Code Checks (lines 354–357, 402–405):**
  - Checks `$LASTEXITCODE -ne 0` immediately after native Windows CLI calls (`MakeAppx.exe pack` and `SignTool.exe sign`). Throws fatal exceptions with exact exit codes.
- **Elimination of Credentials and Silent Fallbacks (lines 201–222):**
  - Completely removed: `New-SelfSignedCertificate` auto-generation, hardcoded password `FerryxMsixSignPass2026!`, and permissive `try/catch` fallback.
  - Store Mode: Explicit switch `-SkipSigning` marks package for unsigned Microsoft Store ingestion.
  - Sideload Mode: Requires explicit `-CertThumbprint` and validates existence in `Cert:\CurrentUser\My` or `Cert:\LocalMachine\My`.
- **Packaged Manifest Validation from Archive (lines 363–395):**
  - Opens produced `.msix` container using `[System.IO.Compression.ZipFile]`, reads internal `AppxManifest.xml`, and asserts `Identity.Name == $PackageName` and `Identity.Version == $msixVersion`.
- **Windows Executed Evidence (`maho-win`):**
  - Remotely executed on builder `maho-win` via SSH (`qa-5-windows-ps-tests-green.log`).
  - Dynamically compiled tiny C# executables with `csc.exe` to supply real PE version resources.
  - Verified 10 out of 10 remote behavior tests GREEN, including real `MakeAppx.exe` (`10.0.26100.0`) packaging 7 files into `Ferryx_2026.908.1_x64.msix` and validating internal manifest identity.
  - Verified RED baseline on unhardened script with 9 failing tests (`qa-4-windows-ps-tests-red.log`).

**Lane 4 Verdict:** **PASS (Zero Blockers)**

---

## 3. Cross-Cutting Consistency & Cohesion Audit

| Cross-Cutting Property | Lane 1 (Version) | Lane 2 (Policy) | Lane 3 (Crypto) | Lane 4 (MSIX) | Cross-Lane Status |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Version Mapping** | `v2026.09.08.1` -> `2026.908.1` / `2026.908.1.0` | N/A (validates no version tampering) | Uses app version in metadata | `v2026.09.08.1` -> `2026.908.1` / `2026.908.1.0` | **Consistent & Identical** |
| **Store Quad (4th=0)** | Quad input rejected in `toMsixVersion` | N/A | N/A | Rejects non-zero 4th component | **Consistent & Enforced** |
| **Leap Year Calendar** | 2026 non-leap / 2028 leap | N/A | N/A | `[DateTime]::DaysInMonth` | **Consistent & Enforced** |
| **ESM / Runtime** | Pure ESM, Node >= 22.22, Bun >= 1.4 | Bun.YAML, Pure ESM | Pure ESM, Node >= 22.22, Bun >= 1.4 | PowerShell 5.1 / 7+ (Windows host) | **Standardized** |
| **Zero Npm Deps** | `node:crypto`, `node:fs`, `node:path` | `node:fs`, `node:path` | `node:crypto` | Standard PowerShell / .NET | **Zero external npm dependencies** |
| **Signing Credentials** | None (credential-free) | Forbids all release secrets in CI | None (public key only) | Forbids hardcoded certs/passwords | **Zero Credential Leaks** |

---

## 4. Concrete Blockers & Resolvability Audit

A comprehensive review of the code diffs and test logs was performed to identify any potential blockers:
- **Lane 1 (`scripts/sync-version.mjs`):** Checked error bubbling, file handle leaks, and regex boundaries. The rollback mechanism correctly isolates file mutation, and `AggregateError` preserves both original and rollback errors. **0 blockers.**
- **Lane 2 (`scripts/release-workflow-policy.mjs`):** Checked AST traversal recursion, pattern coverage, and permission gates. Frontend scoped builds are safely exempted without opening producer holes. **0 blockers.**
- **Lane 3 (`scripts/lib/minisign-verify.mjs`):** Checked SPKI DER construction, endianness of 8-byte key IDs, and buffer concatenation in global signature verification. The cryptographic pipeline conforms exactly to the Minisign specification. **0 blockers.**
- **Lane 4 (`scripts/build-msix.ps1`):** Checked parameter declarations, staging cleanup `finally` block, `$LASTEXITCODE` checks, and ZipFile manifest inspection. Execution on Windows NT verifies real toolchain compatibility. **0 blockers.**

**Conclusion:** There are **zero concrete blockers**. All foundational components are fully verified and ready to be orchestrated by Wave 4 (`scripts/release-local.mjs` / `scripts/build-latest-json.mjs`).

---

## 5. Comprehensive Manual QA Matrix

### 5.1 surfaceEvidence
| Scenario ID | Criterion Reference | Surface | Exact Invocation | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `VER-SURF-01` | Section 3 / Exported API & Import Safety | Node ESM Module Import | `node -e 'import("./scripts/sync-version.mjs").then(m => console.log(JSON.stringify(Object.keys(m).sort())))'` | **PASS** | `scenario-01-import-api.txt` |
| `VER-SURF-02` | Section 3 / CalVer & MSIX Quad Mapping | Node Module API | `node -e 'import("./scripts/sync-version.mjs").then(m => { console.log(JSON.stringify(m.parseReleaseTag("v2026.09.08.1"))); console.log("appVersion=" + m.toAppVersion("v2026.09.08.1")); console.log("msixVersion=" + m.toMsixVersion("v2026.09.08.1")); })'` | **PASS** | `scenario-02-calver-mapping.txt` |
| `VER-SURF-03` | Section 3 / CLI Atomic Version Sync | Node CLI Process | `node scripts/sync-version.mjs --tag v2026.09.08.2 --conf <tmpConf> --cargo <tmpCargo>` | **PASS** | `scenario-03-cli-sync.txt` |
| `VER-SURF-04` | Section 3 & 8 / Legacy SemVer CLI Compatibility | Node CLI Process | `node scripts/sync-version.mjs --tag v1.4.2 --conf <tmpConf> --cargo <tmpCargo>` | **PASS** | `scenario-04-legacy-semver.txt` |
| `VER-SURF-05` | Section 3 / Pre-Write Validation Safety | Node CLI Process | `node scripts/sync-version.mjs --tag v2026.09.08.1 --conf <tmpConf> --cargo <tmpCargoWithoutPackage>` | **PASS** | `scenario-05-prewrite-validation.txt` |
| `POL-SURF-01` | Section 7 / Actual Retired Producer Rejection | Bun CLI Process | `bun scripts/release-workflow-policy.mjs docs/releases/local-release-hardening-evidence/temp-retired.yml` | **PASS** | `qa-1-retired-producer-red.log` |
| `POL-SURF-02` | Section 7 / Fixture Suite Rejection Verification | Node Test Runner | `node --test --test-name-pattern="fixture\|build\|action\|coordinator\|reusable" scripts/release-workflow.test.mjs` | **PASS** | `qa-2-fixtures-rejection.log` |
| `POL-SURF-03` | Section 7 / PR Check Workflow Permitted | Bun CLI Process | `bun scripts/release-workflow-policy.mjs .github/workflows/build-test.yml` | **PASS** | `qa-3-pr-check-permitted.log` |
| `POL-SURF-04` | Section 7 / Pages Deployment Permitted | Bun CLI Process | `bun scripts/release-workflow-policy.mjs .github/workflows/deploy-pages.yml` | **PASS** | `qa-4-pages-permitted.log` |
| `POL-SURF-05` | Section 7 / Live Workflows Directory Compliance | Bun CLI Process | `bun scripts/release-workflow-policy.mjs .github/workflows` | **PASS** | `qa-5-live-workflows-green.log` |
| `POL-SURF-06` | Section 7 / Policy Test Suite Verification | Node Test Runner | `node --test scripts/release-workflow.test.mjs` | **PASS** | `qa-6-test-suite-green.log` |
| `POL-SURF-07` | Section 7 / Full Release Test Suite Regression | Node Test Runner | `node --test scripts/sync-version.test.mjs scripts/build-latest-json.test.mjs scripts/release-workflow.test.mjs scripts/updater-archive-layout.test.mjs` | **PASS** | `qa-7-release-suite-green.log` |
| `POL-SURF-08` | Section 7 / Windows Cargo Link Step Preserved | Node Test Runner | `node --test --test-name-pattern="Cargo Link" scripts/release-workflow.test.mjs` | **PASS** | `qa-8-windows-link-preserved.log` |
| `CRY-SURF-01` | Section 6 / Authentic Tauri Updater Signature | Node ESM API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; ...'` | **PASS** | `crypto-qa-01-authentic-fixture.log` |
| `CRY-SURF-02` | Section 6 / Raw Minisign Text Parsing | Node ESM API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; ...'` | **PASS** | `crypto-qa-02-raw-text-fixture.log` |
| `CRY-SURF-03` | Section 6 / Single-Line 56-char Base64 Public Key | Node ESM API | `node --input-type=module -e 'import { verifyMinisign } from "./scripts/lib/minisign-verify.mjs"; ...'` | **PASS** | `crypto-qa-03-single-line-pubkey.log` |
| `CRY-SURF-04` | Section 6 / Independent ED Prehash (Blake2b-512) | Node ESM API | `node --input-type=module -e '/* generate independent ED keypair & Blake2b-512 prehash signature */ verifyMinisign(...)'` | **PASS** | `crypto-qa-04-independent-ed-prehash.log` |
| `CRY-SURF-05` | Section 6 / Independent Ed Raw (Legacy Ed25519) | Node ESM API | `node --input-type=module -e '/* generate independent Ed keypair & raw Ed25519 signature */ verifyMinisign(...)'` | **PASS** | `crypto-qa-05-independent-ed-raw.log` |
| `CRY-SURF-06` | Section 6 / Minisign CLI Oracle Two-Way Crosscheck | Minisign CLI + Node API | `minisign -V -p <key.pub> -m <data> -x <sig>` & `verifyMinisign({ data, signature: cliSig, publicKey: cliPub })` | **PASS** | `crypto-qa-06-minisign-oracle-crosscheck.log` |
| `MSX-SURF-01` | Section 5 / Explicit `-ExePath` & Candidate Removal | Node Test Runner | `node --test scripts/build-msix.test.mjs` (test 1) | **PASS** | `artifacts/qa-1-node-contract-red.log`, `artifacts/qa-2-node-contract-green.log` |
| `MSX-SURF-02` | Section 5 / Store Quad Contract Enforcement | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (tests 3, 4) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-03` | Section 5 / Binary Version Verification | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 5) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-04` | Section 5 / Stale Output Cleanup & Staging Isolation | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 6) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-05` | Section 5 / Native Tool Exit Code Verification | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 9) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-06` | Section 5 / Store Packaging Mode (`-SkipSigning`) | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 10) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-07` | Section 5 / Credential & Auto-Cert Removal | Node Test Runner | `node --test scripts/build-msix.test.mjs` (test 6) | **PASS** | `artifacts/qa-1-node-contract-red.log`, `artifacts/qa-2-node-contract-green.log` |
| `MSX-SURF-08` | Section 5 / Explicit `-CertThumbprint` Sideload Check | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (tests 7, 8) | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-09` | Section 5 / Real MakeAppx Pack & In-Archive Manifest | Windows Remote PowerShell (`maho-win`) | `powershell -NoProfile -File test-build-msix.ps1` (test 10) | **PASS** | `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-SURF-10` | Section 1 / Bun Runtime Compatibility | Bun CLI Process | `bun test scripts/build-msix.test.mjs` | **PASS** | `artifacts/qa-3-bun-test-green.log` |
| `MSX-SURF-11` | Section 1 / Full Release Test Suite Regression | Node Test Runner | `node --test scripts/*.test.mjs` (55 tests) | **PASS** | `artifacts/qa-6-all-release-tests-green.log` |

---

### 5.2 adversarialCases
| Scenario ID | Criterion Reference | Adversarial Class | Expected Behavior | Verdict | Artifact Refs |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `VER-ADV-01` | Section 3 / Calendar Date Rules | Invalid Leap Day (Feb 29 on non-leap 2026) | Exit 1, reject impossible Feb 29 with calendar date error | **PASS** | `adv-01-invalid-feb29.txt` |
| `VER-ADV-02` | Section 3 / Calendar Date Bounds | Impossible Month (Month 13) | Exit 1, reject month 13 outside 1..12 range | **PASS** | `adv-02-month-13.txt` |
| `VER-ADV-03` | Section 3 / Month Length Bounds | Day Overflow in 30-Day Month (April 31) | Exit 1, reject day 31 in 30-day month | **PASS** | `adv-03-april-31.txt` |
| `VER-ADV-04` | Section 3 / CalVer Year Boundary | Pre-2026 CalVer Tag (2025.12.31) | Exit 1, reject year < 2026 | **PASS** | `adv-04-pre-2026.txt` |
| `VER-ADV-05` | Section 3 / UInt16 Quad Limits | Revision Overflow (> 65535, e.g. 65536) | Exit 1, reject revision exceeding MSIX uint16 limit | **PASS** | `adv-05-revision-overflow.txt` |
| `VER-ADV-06` | Section 3 / Syntax Rules | Malformed Leading Zero in Revision (`v2026.09.08.01`) | Exit 1, reject leading zero in revision | **PASS** | `adv-06-leading-zero-revision.txt` |
| `VER-ADV-07` | Section 3 / Atomic Rollback | Second Target Rename Failure (EIO) | Verify original bytes restored and temp files cleaned | **PASS** | `adv-07-atomic-rollback.txt` |
| `VER-ADV-08` | Section 3 / Input Contract | Unsupported Raw 4-Part Quad String (`2026.908.1.0`) | Rejection, enforce release tag / SemVer input contract | **PASS** | `adv-08-reject-msix-quad-input.txt` |
| `VER-ADV-09` | Section 3 / Rollback Failure Surfacing | Rollback Write Failure During Recovery | AggregateError surfaced with both original EIO and rollback EACCES | **PASS** | `adv-09-aggregate-error-rollback-failure.txt` |
| `POL-ADV-01` | Section 7 / Permission Boundaries | Unauthorized Write Permissions | Rejects `contents: write` and `write-all` at workflow/job levels | **PASS** | `adv-1-permissions.log` |
| `POL-ADV-02` | Section 7 / Credential Leak Prevention | Reintroduced Release Secrets | Rejects workflows referencing release secrets (`TAURI_SIGNING_PRIVATE_KEY`, etc.) | **PASS** | `adv-2-secrets.log` |
| `POL-ADV-03` | Section 7 / Syntax Error Resilience | Malformed YAML Documents | Trapped gracefully with descriptive error, no uncaught crash | **PASS** | `adv-3-malformed-yaml.log` |
| `POL-ADV-04` | Section 7 / Backdoor Evasion | Reintroduced `release.yml` File | Rejects directory scan if `release.yml` or `release.yaml` exists | **PASS** | `adv-4-file-forbidden.log` |
| `POL-ADV-05` | Section 7 / Stealth Build Commands | Packaging Commands Disguised in Steps | Rejects scripts or commands invoking `build-msix.ps1` or `tauri build` | **PASS** | `adv-5-stealth-build.log` |
| `POL-ADV-06` | Section 7 / Action Discrimination | Legitimate vs Release Actions | Pages deploy action passes; release publication action fails | **PASS** | `adv-6-actions-discrimination.log` |
| `CRY-ADV-01` | Section 6 / Payload Data Integrity | Tampered Payload Bytes (1 bit flip) | Throws Error: `Minisign payload signature verification failed` | **PASS** | `crypto-adv-01-tampered-data.log` |
| `CRY-ADV-02` | Section 6 / Signature Byte Integrity | Tampered Payload Signature (1 bit flip in Line 2) | Throws Error: `Minisign payload signature verification failed` | **PASS** | `crypto-adv-02-tampered-payload-sig.log` |
| `CRY-ADV-03` | Section 6 / Global Signature Integrity | Tampered Global Signature (1 bit flip in Line 4) | Throws Error: `Minisign trusted comment signature verification failed` | **PASS** | `crypto-adv-03-tampered-global-sig.log` |
| `CRY-ADV-04` | Section 6 / Strict Key ID Binding | Tampered Key ID in Signature (Mismatch) | Throws Error: `Signature key ID (...) does not match public key ID (...)` | **PASS** | `crypto-adv-04-key-id-mismatch.log` |
| `CRY-ADV-05` | Section 6 / Comment Tampering | Modified Trusted Comment Text | Throws Error: `Minisign trusted comment signature verification failed` | **PASS** | `crypto-adv-05-tampered-comment.log` |
| `CRY-ADV-06` | Section 6 / Input Sanitization | Arbitrary Base64 String Injection | Throws Error rejecting non-minisign arbitrary base64 strings | **PASS** | `crypto-adv-06-arbitrary-base64-rejection.log` |
| `CRY-ADV-07` | Section 6 / Strict Algorithm ID | Invalid Algorithm Marker (`XX` instead of `Ed`/`ED`) | Throws Error: `Invalid signature algorithm` / `Invalid publicKey algorithm` | **PASS** | `crypto-adv-07-invalid-algorithm.log` |
| `CRY-ADV-08` | Section 6 / Strict Field Lengths | Truncated/Overflow Byte Lengths (70b sig, 41b key) | Throws Error: `expected 42 bytes` / `expected 74 bytes` / `expected 64 bytes` | **PASS** | `crypto-adv-08-invalid-byte-lengths.log` |
| `MSX-ADV-01` | Section 5 / Argument Omission | Missing Mandatory `-ExePath` | Fails closed with PowerShell missing mandatory parameter error | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-02` | Section 5 / Valid Path Requirement | Non-Existent Binary Path | Fails closed with `ERROR: Executable not found at path` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-03` | Section 5 / Store Quad Constraint | Non-Zero 4th Quad Component (`2026.908.1.5`) | Rejects with `MSIX Store packages require the 4th quad component to be 0` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-04` | Section 5 / Calendar Date Bounds | Non-Leap Feb 29 Tag (`v2026.02.29`) | Rejects with `Invalid release day: 29 for month 2 in year 2026` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-05` | Section 5 / Version Consistency | Binary Version Skew (`1.0.0.0` vs expected) | Rejects with `Binary version verification failed: binary version does not match` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-06` | Section 5 / Output Isolation | Pre-Existing Output MSIX Stale File | Deletes stale output file before packaging begins | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-07` | Section 5 / Signing Security | Sideload Mode Without `-CertThumbprint` | Throws `ERROR: Signing requested but no -CertThumbprint provided` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-08` | Section 5 / Certificate Existence | Bogus Certificate Thumbprint | Throws `ERROR: Signing certificate with thumbprint '...' not found` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-09` | Section 5 / Native Tool Failure | Native MakeAppx Non-Zero Exit Code | Throws `ERROR: MakeAppx.exe pack failed with exit code 1` | **PASS** | `artifacts/qa-4-windows-ps-tests-red.log`, `artifacts/qa-5-windows-ps-tests-green.log` |
| `MSX-ADV-10` | Section 5 / Package Integrity | Corrupted MSIX Archive / Bad Manifest | Validates in-archive `AppxManifest.xml` Name and Version equality | **PASS** | `artifacts/qa-5-windows-ps-tests-green.log` |

---

### 5.3 artifactRefs
| ID | Kind | Description | Path |
| :--- | :--- | :--- | :--- |
| `scenario-01-import-api.txt` | cli-log | Verification of exported API keys and zero import side effects | `docs/releases/local-release-hardening-evidence/scenario-01-import-api.txt` |
| `scenario-02-calver-mapping.txt` | cli-log | Verification of `parseReleaseTag`, `toAppVersion`, and `toMsixVersion` outputs | `docs/releases/local-release-hardening-evidence/scenario-02-calver-mapping.txt` |
| `scenario-03-cli-sync.txt` | cli-log | Full CLI execution checking exitCode 0, output `version=`, and dependency preservation | `docs/releases/local-release-hardening-evidence/scenario-03-cli-sync.txt` |
| `scenario-04-legacy-semver.txt` | cli-log | CLI execution of legacy SemVer `v1.4.2` with major < 2026 | `docs/releases/local-release-hardening-evidence/scenario-04-legacy-semver.txt` |
| `scenario-05-prewrite-validation.txt` | cli-log | Corrupted `Cargo.toml` input leaves `tauri.conf.json` completely untouched | `docs/releases/local-release-hardening-evidence/scenario-05-prewrite-validation.txt` |
| `adv-01-invalid-feb29.txt` | cli-log | Error rejection output for `v2026.02.29` non-leap year | `docs/releases/local-release-hardening-evidence/adv-01-invalid-feb29.txt` |
| `adv-02-month-13.txt` | cli-log | Error rejection output for month 13 | `docs/releases/local-release-hardening-evidence/adv-02-month-13.txt` |
| `adv-03-april-31.txt` | cli-log | Error rejection output for April 31 | `docs/releases/local-release-hardening-evidence/adv-03-april-31.txt` |
| `adv-04-pre-2026.txt` | cli-log | Error rejection output for year 2025 CalVer tag | `docs/releases/local-release-hardening-evidence/adv-04-pre-2026.txt` |
| `adv-05-revision-overflow.txt` | cli-log | Error rejection output for revision 65536 exceeding MSIX bound | `docs/releases/local-release-hardening-evidence/adv-05-revision-overflow.txt` |
| `adv-06-leading-zero-revision.txt` | cli-log | Error rejection output for leading zero in revision `01` | `docs/releases/local-release-hardening-evidence/adv-06-leading-zero-revision.txt` |
| `adv-07-atomic-rollback.txt` | cli-log | Rollback verification confirming first target contained new version before EIO failure, followed by restoration and temp cleanup | `docs/releases/local-release-hardening-evidence/adv-07-atomic-rollback.txt` |
| `adv-08-reject-msix-quad-input.txt` | cli-log | Rejection output for unsupported 4-part quad input `2026.908.1.0` | `docs/releases/local-release-hardening-evidence/adv-08-reject-msix-quad-input.txt` |
| `adv-09-aggregate-error-rollback-failure.txt` | cli-log | AggregateError output containing both original EIO and rollback EACCES errors | `docs/releases/local-release-hardening-evidence/adv-09-aggregate-error-rollback-failure.txt` |
| `qa-1-retired-producer-red.log` | subprocess-log | Output of validator rejecting retired release producer fixture | `docs/releases/local-release-hardening-evidence/qa-1-retired-producer-red.log` |
| `qa-2-fixtures-rejection.log` | test-log | Output of Node test runner verifying fixture rejections (manual, tag, callable, tauri build, root build, tauri-action, release-local, secrets) | `docs/releases/local-release-hardening-evidence/qa-2-fixtures-rejection.log` |
| `qa-3-pr-check-permitted.log` | cli-log | Output of validator confirming `build-test.yml` compliance | `docs/releases/local-release-hardening-evidence/qa-3-pr-check-permitted.log` |
| `qa-4-pages-permitted.log` | cli-log | Output of validator confirming `deploy-pages.yml` compliance | `docs/releases/local-release-hardening-evidence/qa-4-pages-permitted.log` |
| `qa-5-live-workflows-green.log` | cli-log | Output of validator confirming `.github/workflows` live directory compliance | `docs/releases/local-release-hardening-evidence/qa-5-live-workflows-green.log` |
| `qa-6-test-suite-green.log` | test-log | Output of `node --test scripts/release-workflow.test.mjs` (16 tests pass) | `docs/releases/local-release-hardening-evidence/qa-6-test-suite-green.log` |
| `qa-7-release-suite-green.log` | test-log | Output of full release test suite (38 tests pass) | `docs/releases/local-release-hardening-evidence/qa-7-release-suite-green.log` |
| `qa-8-windows-link-preserved.log` | test-log | Output of test verifying Windows Cargo Link step preservation | `docs/releases/local-release-hardening-evidence/qa-8-windows-link-preserved.log` |
| `adv-1-permissions.log` | cli-log | Adversarial test: permissions `write-all` and `contents: write` rejection | `docs/releases/local-release-hardening-evidence/adv-1-permissions.log` |
| `adv-2-secrets.log` | cli-log | Adversarial test: forbidden release secret leak detection | `docs/releases/local-release-hardening-evidence/adv-2-secrets.log` |
| `adv-3-malformed-yaml.log` | cli-log | Adversarial test: graceful handling of malformed YAML syntax | `docs/releases/local-release-hardening-evidence/adv-3-malformed-yaml.log` |
| `adv-4-file-forbidden.log` | cli-log | Adversarial test: detection and rejection of reintroduced `release.yml` file | `docs/releases/local-release-hardening-evidence/adv-4-file-forbidden.log` |
| `adv-5-stealth-build.log` | cli-log | Adversarial test: detection of stealth release packaging commands (`build-msix.ps1`) | `docs/releases/local-release-hardening-evidence/adv-5-stealth-build.log` |
| `adv-6-actions-discrimination.log` | cli-log | Adversarial test: action discrimination between Pages deployment and GitHub Release | `docs/releases/local-release-hardening-evidence/adv-6-actions-discrimination.log` |
| `crypto-qa-01-authentic-fixture.log` | cli-log | Verification of authentic repo archive fixture against configured Tauri updater public key | `docs/releases/local-release-hardening-evidence/crypto-qa-01-authentic-fixture.log` |
| `crypto-qa-02-raw-text-fixture.log` | cli-log | Verification of authentic fixture using raw unwrapped 4-line signature and 2-line public key text | `docs/releases/local-release-hardening-evidence/crypto-qa-02-raw-text-fixture.log` |
| `crypto-qa-03-single-line-pubkey.log` | cli-log | Verification using single-line 56-character base64 public key (`minisign -P` format) | `docs/releases/local-release-hardening-evidence/crypto-qa-03-single-line-pubkey.log` |
| `crypto-qa-04-independent-ed-prehash.log` | cli-log | Verification of independently signed ED Blake2b-512 prehash signature | `docs/releases/local-release-hardening-evidence/crypto-qa-04-independent-ed-prehash.log` |
| `crypto-qa-05-independent-ed-raw.log` | cli-log | Verification of independently signed Ed raw legacy signature | `docs/releases/local-release-hardening-evidence/crypto-qa-05-independent-ed-raw.log` |
| `crypto-qa-06-minisign-oracle-crosscheck.log` | cli-log | Two-way cross-verification between minisign CLI oracle and verifyMinisign implementation | `docs/releases/local-release-hardening-evidence/crypto-qa-06-minisign-oracle-crosscheck.log` |
| `crypto-adv-01-tampered-data.log` | cli-log | Rejection error output when flipping 1 byte in payload data | `docs/releases/local-release-hardening-evidence/crypto-adv-01-tampered-data.log` |
| `crypto-adv-02-tampered-payload-sig.log` | cli-log | Rejection error output when corrupting payload signature bytes | `docs/releases/local-release-hardening-evidence/crypto-adv-02-tampered-payload-sig.log` |
| `crypto-adv-03-tampered-global-sig.log` | cli-log | Rejection error output when corrupting global/trusted comment signature bytes | `docs/releases/local-release-hardening-evidence/crypto-adv-03-tampered-global-sig.log` |
| `crypto-adv-04-key-id-mismatch.log` | cli-log | Rejection error output when signature key ID does not match public key key ID | `docs/releases/local-release-hardening-evidence/crypto-adv-04-key-id-mismatch.log` |
| `crypto-adv-05-tampered-comment.log` | cli-log | Rejection error output when trusted comment text is modified | `docs/releases/local-release-hardening-evidence/crypto-adv-05-tampered-comment.log` |
| `crypto-adv-06-arbitrary-base64-rejection.log` | cli-log | Rejection error output when arbitrary base64 strings are supplied as key or signature | `docs/releases/local-release-hardening-evidence/crypto-adv-06-arbitrary-base64-rejection.log` |
| `crypto-adv-07-invalid-algorithm.log` | cli-log | Rejection error output when algorithm marker is invalid (`XX`) | `docs/releases/local-release-hardening-evidence/crypto-adv-07-invalid-algorithm.log` |
| `crypto-adv-08-invalid-byte-lengths.log` | cli-log | Rejection error output when decoded fields have invalid byte lengths | `docs/releases/local-release-hardening-evidence/crypto-adv-08-invalid-byte-lengths.log` |
| `artifacts/qa-1-node-contract-red.log` | test-log | RED baseline test run showing 8 failures on unhardened `build-msix.ps1` | `docs/releases/local-release-hardening-evidence/artifacts/qa-1-node-contract-red.log` |
| `artifacts/qa-2-node-contract-green.log` | test-log | GREEN test run showing 11 passes on hardened `build-msix.ps1` | `docs/releases/local-release-hardening-evidence/artifacts/qa-2-node-contract-green.log` |
| `artifacts/qa-3-bun-test-green.log` | test-log | Bun 1.4.0 test execution log showing 11 passes | `docs/releases/local-release-hardening-evidence/artifacts/qa-3-bun-test-green.log` |
| `artifacts/qa-4-windows-ps-tests-red.log` | ps-log | RED baseline remote test run on `maho-win` showing 9 failures | `docs/releases/local-release-hardening-evidence/artifacts/qa-4-windows-ps-tests-red.log` |
| `artifacts/qa-5-windows-ps-tests-green.log` | ps-log | GREEN remote test run on `maho-win` showing 10/10 passes with real MakeAppx | `docs/releases/local-release-hardening-evidence/artifacts/qa-5-windows-ps-tests-green.log` |
| `artifacts/qa-6-all-release-tests-green.log` | test-log | Comprehensive test run across all 55 release tests passing with zero regressions | `docs/releases/local-release-hardening-evidence/artifacts/qa-6-all-release-tests-green.log` |
