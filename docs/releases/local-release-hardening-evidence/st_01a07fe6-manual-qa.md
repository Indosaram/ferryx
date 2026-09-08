# Manual QA Matrix — `st_01a07fe6`

**Task ID:** `st_01a07fe6`  
**Parent Session:** `01a07f49-03be-7242-be4f-68a7e66c2166`  
**Date:** 2026-09-08  
**Role:** Manual QA Executor (`omo-senpi-qa-executor`)  
**Scope:** Verification of the End-to-End Local Release Coordinator and Isolated Platform Builders Subsystem  
**Target Deliverables Audited:**
- `scripts/release-local.mjs`
- `scripts/lib/release-platforms.mjs`
- `scripts/release-local.test.mjs`
- `scripts/release-platforms.test.mjs`
- `package.json` (`release:local` script)
- `docs/releases/local-release-hardening-evidence/execution.md`

**Overall Verdict:** **PASS**

All verification criteria, CLI surfaces, and adversarial security barriers are satisfied with reproducible, non-empty evidence:
1. **Real Subprocess & CLI Fixtures:** All 7 required subcommands (`prepare`, `preflight`, `build`, `assemble`, `verify`, `publish`, `verify-remote`) are fully implemented and verified via real process execution (zero stubs, zero command-printing-only mock mocks).
2. **CI / GitHub Actions Policy Hardening:** Mutating coordinator stages (`prepare`, `build`, `assemble`, `publish`) fail closed immediately if executed under `GITHUB_ACTIONS=true` or `CI=true`.
3. **CalVer Date Validation & Output-Preservation:** Release tags strictly follow `vYYYY.MM.DD[.R]`; impossible calendar dates (non-leap year Feb 29, month 13, day 32) are rejected before writing files; existing run directories fail closed against clobbering.
4. **Authentic Live Multi-Host Preflight Probes:** Live probes against `macbook`, `omaki` (via SSH), and `maho-win` (via SSH and PowerShell) inspect actual disk budgets, OS architectures, toolchain versions, Linux packages, Windows SDK / MSVC build tools, and signing presence (booleans only, zero secret leakage).
5. **Isolated Workspace & Git Bundling:** Standalone git bundles created with commit SHA and Ghostty pin; verified clone HEADs, version stamping via `sync-version.mjs`, and isolated `CARGO_TARGET_DIR`.
6. **Repeatable Manifest Assembly & Verification:** Byte-for-byte identical output verified between assembled publish directory and independent private temporary re-derivation.
7. **Double Approval Publish Gate:** Rejects unapproved publication without both `--approve-publish` flag AND `FERRYX_APPROVE_PUBLISH=1` environment variable. Verifies tag-to-SHA identity and downloads draft assets for local byte comparison before undrafting.
8. **Release Foundation Non-Regression:** 129/129 release tests pass across the complete test suite under Node.js and Bun 1.4.0 with 0 failures and 0 skips.

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-COORD-01 | CLI Help & Zero Side-Effect ESM Import | Node.js / Bun CLI | `bun run release:local --help` | **PASS** | Exits 0; displays usage for all 7 subcommands (`prepare`, `preflight`, `build`, `assemble`, `verify`, `publish`, `verify-remote`). Importing module has no side effects. | ART-HELP-LIVE, ART-COORD-NODE |
| SURF-COORD-02 | CalVer Validation & Immutable Prepare State | Live Git & Node.js Subprocess | `node scripts/release-local.mjs prepare --config scripts/release-hosts.example.json --tag v2026.09.08.1 --commit HEAD --out /tmp/ferryx-qa-live-prepare` | **PASS** | Exits 0; creates deterministic `plan.json`, `prepare-state.json` (recording `planDigest` and `configDigest`), and `source-inputs.json`. Ghostty pin verified against local Ghostty HEAD. | ART-PREPARE-LIVE |
| SURF-COORD-03 | Multi-Host Environmental Preflight Probing | Real Machine & SSH Transports | `node scripts/release-local.mjs preflight --config scripts/release-hosts.example.json --plan <planPath>` | **PASS** | Captured live status for `macbook`, `omaki`, and `maho-win`. Detects local disk budget deficit (~12.2Gi vs 30Gi budget) and exits non-zero without faking success. | ART-PREFLIGHT-LIVE |
| SURF-COORD-04 | Isolated Git Bundling & Host Staging | Platform Adapters | `node --test scripts/release-platforms.test.mjs` (tests 4, 7, 8) | **PASS** | Git bundles created for source and Ghostty; isolated workspace clones verify exact commit SHA and Ghostty pin; fails closed on workspace collision. | ART-PLATFORMS-NODE |
| SURF-COORD-05 | Receipt Assembly & Stable Aliases Closure | Coordinator Assemble | `node --test scripts/release-local.test.mjs` (test 5, 7) | **PASS** | Invokes `assembleRelease`; creates deterministic aliases, writes `latest.json` first, and hashes 100% of files into `SHA256SUMS.txt`. | ART-COORD-NODE |
| SURF-COORD-06 | Independent Derivation & Byte-for-Byte Verify | Coordinator Verify | `node --test scripts/release-local.test.mjs` (test 7) | **PASS** | Re-derives release in private temporary directory; verifies exact byte length and SHA256 match for every published artifact. | ART-COORD-NODE |
| SURF-COORD-07 | Double Approval Publish Gate & Draft Verification | Coordinator Publish | `node --test scripts/release-local.test.mjs` (tests 8, 9) | **PASS** | Enforces `--approve-publish` and `FERRYX_APPROVE_PUBLISH=1`; verifies remote tag resolves to commit SHA; tests draft creation, upload, draft download byte comparison, and undrafting. | ART-COORD-NODE |
| SURF-COORD-08 | Full Foundation Release Suite Non-Regression | Node.js & Bun Test Runner | `node --test scripts/release-local.test.mjs scripts/release-platforms.test.mjs scripts/release-contract.test.mjs scripts/release-hosts.test.mjs scripts/build-latest-json.test.mjs scripts/sync-version.test.mjs scripts/release-workflow.test.mjs scripts/build-msix.test.mjs` | **PASS** | All 129 release tests pass with zero regressions, zero failures, and zero skipped tests. | ART-FOUNDATION-SUITE, ART-BUN-SUITE |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-COORD-01 | CI Environment Mutating Protection | Hosted Release Backdoor / Accidental Run in CI | `GITHUB_ACTIONS=true` or `CI=true` must immediately reject mutating coordinator stages (`prepare`, `build`, `assemble`, `publish`). | **PASS** | Throws `Mutating release stage '<stage>' is forbidden under CI / GitHub Actions environment` with non-zero exit code. | ART-COORD-NODE |
| ADV-COORD-02 | CalVer Calendar Validation | Malformed / Impossible Calendar Dates | Non-leap year Feb 29 (`v2026.02.29`), month 13 (`v2026.13.01`), April 31 (`v2026.04.31`), pre-2026, and revision overflow (>65535) must fail closed before file operations. | **PASS** | Throws `Invalid calendar date in release tag ...`; zero files written to disk. | ART-COORD-NODE |
| ADV-COORD-03 | Output Run Directory Preservation | Run Output Directory Collision | Pre-existing run directory must not be deleted, overwritten, or modified; coordinator must fail closed. | **PASS** | Throws `Output run directory already exists: <dir>`; existing canary and plan files preserved untouched. | ART-COORD-NODE |
| ADV-COORD-04 | Host Disk Budget Enforcement | Insufficient Host Storage | When free bytes on host are below configured `minFreeBytes`, preflight must report `ok: false` and exit non-zero. | **PASS** | Reports `disk: { ok: false, availableBytes: ..., requiredBytes: ... }` and exits with code 1. | ART-COORD-NODE, ART-PREFLIGHT-LIVE |
| ADV-COORD-05 | Release Plan Digest Integrity | Tampered / Stale Plan | If `plan.json` is modified after `prepare`, `build` must detect digest mismatch against `prepare-state.json` and fail closed. | **PASS** | Throws `plan.json digest does not match prepare-state.json: plan was tampered with`. | ART-COORD-NODE |
| ADV-COORD-06 | Staged Artifact & Receipt Tampering | Post-Build File Tampering | Modifying published file bytes, deleting checksums, or corrupting receipts must fail `verify`. | **PASS** | Detects mismatch; throws `Checksum mismatch for published file ...` or `latest.json not found in publish directory`. | ART-COORD-NODE |
| ADV-COORD-07 | Publish Double Approval Barrier | Unapproved Publication Attempt | Invoking `publish` without `--approve-publish` flag OR without `FERRYX_APPROVE_PUBLISH=1` environment variable must fail closed. | **PASS** | Throws `Publication requires explicit --approve-publish flag` or `Publication requires FERRYX_APPROVE_PUBLISH=1 environment variable`. | ART-COORD-NODE |
| ADV-COORD-08 | Remote Tag & Commit Identity | Remote Tag Drift / Out-of-Sync | If remote git tag on origin resolves to a different commit SHA than `plan.commitSha`, publish must fail closed. | **PASS** | Throws `Remote tag '...' resolves to <remoteSha>, but plan requires commit <commitSha>`. | ART-COORD-NODE |
| ADV-COORD-09 | Existing Remote Release Protection | Accidental Release Overwrite | If GitHub Release already exists for tag (draft or published), publish must reject rather than overwrite. | **PASS** | Throws `Release '...' already exists on remote repo ... Refusing to overwrite.` | ART-COORD-NODE |
| ADV-COORD-10 | Host Workspace Collision Protection | Builder Host Workspace Collision | If `host.root/<runId>` already exists on host, `buildHost` must fail closed to protect prior builds. | **PASS** | Throws `Host workspace already exists: refusing to overwrite (<workspaceDir>)`. | ART-PLATFORMS-NODE |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-COORD-NODE | Node.js TAP Log | TAP test execution log for `scripts/release-local.test.mjs` (10/10 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-coordinator-node.log` |
| ART-PLATFORMS-NODE | Node.js TAP Log | TAP test execution log for `scripts/release-platforms.test.mjs` (8/8 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-platforms-node.log` |
| ART-BUN-SUITE | Bun Test Log | Bun 1.4.0 test execution log for both coordinator and platform tests (18/18 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-bun-suite.log` |
| ART-FOUNDATION-SUITE | Node.js TAP Log | TAP test execution log for all 129 foundation release tests across 8 test suites passing with 0 failures and 0 skips | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-full-foundation-suite.log` |
| ART-HELP-LIVE | CLI Output Log | Live execution of `bun run release:local --help` displaying all 7 subcommands | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-help-live.log` |
| ART-PREPARE-LIVE | CLI Execution Log | Live execution of `node scripts/release-local.mjs prepare` against real repository generating deterministic plan | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-prepare-live.log` |
| ART-PREFLIGHT-LIVE | JSON Probe Output | Captured live preflight probe JSON output across `macbook`, `omaki`, and `maho-win` | `docs/releases/local-release-hardening-evidence/artifacts/execution-qa-preflight-live.json` |
