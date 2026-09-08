# Manual QA Matrix — `st_01a080d4`

**Task ID:** `st_01a080d4`  
**Parent Session:** `01a07f49-03be-7242-be4f-68a7e66c2166`  
**Date:** 2026-09-08  
**Role:** Manual QA Executor (`omo-senpi-qa-executor`)  
**Scope:** Independent verification and live scenario execution of the Local Release Coordinator and Platform Builders subsystem (`scripts/release-local.mjs`, `scripts/lib/release-platforms.mjs`, `scripts/release-local.test.mjs`, `scripts/release-platforms.test.mjs`, `package.json`).

**Overall Verdict:** **PASS (Zero Actionable Blockers)**

All 9 surface scenarios and 12 adversarial test cases executed against real processes, filesystems, cryptographic engines, and network hosts with reproducible, non-empty evidence recorded.

---

## Audit Findings & Verification Summary

1. **CLI Surface & Zero Side-Effect ESM Import:**
   - `node scripts/release-local.mjs --help` and `bun run release:local --help` exit 0, documenting all 7 subcommands (`prepare`, `preflight`, `build`, `assemble`, `verify`, `publish`, `verify-remote`).
   - Dynamic import (`import("./scripts/release-local.mjs")`) exports all functions cleanly without triggering execution or process termination.
2. **Deterministic Prepare Stage & CalVer Validation:**
   - Live execution against the real repository HEAD generated `plan.json`, `prepare-state.json`, and `source-inputs.json`.
   - Release tags strictly validate calendar dates (2026+ boundary, month 1..12, days matching month length, leap year rules). Impossible dates (Feb 29 on non-leap year, month 13, April 31, pre-2026, revision overflow > 65535) are rejected before writing to disk.
   - Ghostty pin (`6a508fd5e34c7e222c052a6d00bb3891ff3feace`) was extracted from `build_ghostty.rs` and verified against local vendor submodule HEAD.
3. **Live Multi-Host Environmental Preflight Probes:**
   - Executed live probes against `macbook` (local), `omaki` (remote Linux via SSH), and `maho-win` (remote Windows via SSH/PowerShell).
   - Captured machine architectures, toolchains, disk availability, Linux packages (`webkit2gtk-4.1`, `gtk+-3.0`, `alsa`), and Windows tools (`vswhere.exe`, MSVC `link.exe`, `MakeAppx.exe`).
   - Developer ID and notarization availability are probed safely, reporting booleans only with zero credentials printed or logged.
   - Preflight correctly detects MacBook available disk space deficit (~8.4 GiB free vs 32.2 GiB budget) and exits non-zero (exit code 1) without falsely reporting readiness.
4. **Platform Git Bundling & Workspace Isolation:**
   - Standalone Git bundles for source and Ghostty were created, cloned into an isolated workspace, and verified to check out the exact commit SHA.
   - Pre-existing workspace directories on target hosts fail closed against clobbering.
5. **Receipt Assembly Closure & Deterministic Aliases:**
   - Validated authentic receipt inputs and staged publish artifacts, generating deterministic stable aliases (`Ferryx_universal.dmg`, `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `Ferryx_x64.msix`).
   - `latest.json` is created first; `SHA256SUMS.txt` hashes 100% of published files, explicitly including `latest.json`.
   - Verified via `shasum -a 256 -c SHA256SUMS.txt` with 100% match.
6. **Repeatable Verification Re-derivation:**
   - Coordinator `verify` subcommand inspects publish directory checksums and independently re-derives the release in a private temporary directory (`mkdtempSync`), verifying byte-for-byte identity.
   - Any mutated artifact byte or altered receipt fails closed.
7. **Double Approval Publish Gate & Draft Flow:**
   - Coordinator `publish` enforces both the explicit `--approve-publish` flag and the `FERRYX_APPROVE_PUBLISH=1` environment variable.
   - Remote tag is verified against `plan.commitSha` before initiating GitHub operations.
   - Exercises real draft creation (`gh release create --draft`), asset upload (`gh release upload`), asset download (`gh release download`), byte verification against local publish directory, and undrafting (`gh release edit --draft=false`).
8. **Automated Test Suites Non-Regression:**
   - `scripts/release-local.test.mjs` and `scripts/release-platforms.test.mjs` pass 18/18 tests with zero failures under both Node.js (v22.22.3) and Bun (v1.4.0).

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-COORD-01 | CLI Help & Zero Side-Effect ESM Import | Node.js & Bun CLI | `node scripts/release-local.mjs --help`, `bun run release:local --help`, `node -e 'import("./scripts/release-local.mjs")'` | **PASS** | Exit 0; displays complete usage for all 7 subcommands (`prepare`, `preflight`, `build`, `assemble`, `verify`, `publish`, `verify-remote`). Module import has zero side-effects and exports 7 functions. | ART-SURF-01A, ART-SURF-01B, ART-SURF-01C |
| SURF-COORD-02 | CalVer Validation & Immutable Prepare State | Live Git & Node Subprocess | `node scripts/release-local.mjs prepare --config scripts/release-hosts.example.json --tag v2026.09.08.1 --commit HEAD --out <tmpDir>` | **PASS** | Exit 0; generated valid `plan.json`, `prepare-state.json`, and `source-inputs.json`. Verified Ghostty pin matches local submodule HEAD. | ART-SURF-02 |
| SURF-COORD-03 | Multi-Host Environmental Preflight Probes | Real Machine & SSH/PowerShell Transports | `node scripts/release-local.mjs preflight --config scripts/release-hosts.example.json --plan <planPath>` | **PASS** | Exit 1 (disk deficit). Captured authentic machine status across `macbook`, `omaki`, and `maho-win`. Reported disk deficit on macbook (~8.4Gi free vs 32.2Gi required). | ART-SURF-03 |
| SURF-COORD-04 | Platform Git Bundling & Clone Verification | Platform Library Subprocess | `node -e 'import("./scripts/lib/release-platforms.mjs").then(...) createGitBundles(...) git clone ...'` | **PASS** | Exit 0; generated valid `source.bundle` and `ghostty.bundle`; isolated git clone verified HEAD matches commit SHA `3a19bdf7...`. | ART-SURF-04 |
| SURF-COORD-05 | Receipt Assembly Closure & Checksum Verification | Coordinator Assemble CLI | `node scripts/release-local.mjs assemble --run <runDir> --pubkey <testKey>`, `shasum -a 256 -c SHA256SUMS.txt` | **PASS** | Exit 0; assembled 12 release files, created deterministic aliases, generated `latest.json` first, and verified 100% of files including `latest.json` with `shasum -c`. | ART-SURF-05A, ART-SURF-05B |
| SURF-COORD-06 | Independent Re-derivation & Byte-for-Byte Verify | Coordinator Verify CLI | `node scripts/release-local.mjs verify --run <runDir> --pubkey <testKey>` | **PASS** | Exit 0; verified 11 release files and confirmed byte-for-byte identity against fresh independent derivation in private temp directory. | ART-SURF-06 |
| SURF-COORD-07 | Double Approval Publish Gate & Draft Lifecycle | Coordinator Publish Pipeline | `node -e 'import("./scripts/release-local.mjs").then(...) publishRelease({ approvePublish: true, ... })'` | **PASS** | Exit 0; rejected calls without flag or env var; executed full draft release creation, asset upload, draft download byte comparison, and undrafting. | ART-SURF-07 |
| SURF-COORD-08 | Verify Remote Release Public Endpoint | Coordinator Remote Verification | `node -e 'import("./scripts/release-local.mjs").then(...) verifyRemoteRelease({ runDir, baseUrl })'` | **PASS** | Exit 0; queried public release endpoint, verified HTTP 200 response and exact version match. | ART-SURF-08 |
| SURF-COORD-09 | Release Coordinator & Platform Test Suites | Node.js & Bun Test Runners | `node --test scripts/release-local.test.mjs scripts/release-platforms.test.mjs`, `bun test scripts/release-local.test.mjs scripts/release-platforms.test.mjs` | **PASS** | 18/18 tests pass with 0 failures under Node.js (v22.22.3) and Bun (v1.4.0). | ART-SUITE-NODE, ART-SUITE-BUN |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-COORD-01 | CI Environment Mutating Protection | Hosted Release Backdoor / Accidental Run in CI | `GITHUB_ACTIONS=true` or `CI=true` must reject mutating coordinator stages (`prepare`, `assemble`, `publish`). | **PASS** | Throws `Mutating release stage '<stage>' is forbidden under CI / GitHub Actions environment.` with non-zero exit code. | ART-ADV-01 |
| ADV-COORD-02 | CalVer Calendar Validation | Malformed / Impossible Calendar Dates | Impossible dates (`v2026.02.29` non-leap, month 13, April 31, pre-2026, revision overflow >65535) must fail closed before file operations. | **PASS** | Throws `Invalid calendar date in release tag ...` or `Invalid release tag ...`; zero files written to disk. | ART-ADV-02 |
| ADV-COORD-03 | Output Run Directory Preservation | Run Output Directory Collision | Pre-existing run directory must not be overwritten or modified; coordinator must fail closed. | **PASS** | Throws `Output run directory already exists: <dir>`; existing canary file left untouched. | ART-ADV-03 |
| ADV-COORD-04 | Host Disk Budget Enforcement | Insufficient Host Storage | When free bytes on host are below configured `minFreeBytes`, preflight must report `ok: false` and exit non-zero. | **PASS** | Reports `disk: { ok: false, availableBytes: ..., requiredBytes: ... }` and exits with code 1. | ART-ADV-04 |
| ADV-COORD-05 | Release Plan Digest Integrity | Tampered / Stale Plan | If `plan.json` is modified after `prepare`, `build` must detect digest mismatch against `prepare-state.json` and fail closed. | **PASS** | Throws `plan.json digest does not match prepare-state.json: plan was tampered with`. | ART-ADV-05 |
| ADV-COORD-06 | Host Workspace Collision Protection | Builder Host Workspace Collision | If `host.root/<runId>` already exists on host, `buildHost` must fail closed to protect prior builds. | **PASS** | Throws `Host workspace already exists: refusing to overwrite (<workspaceDir>)`. | ART-ADV-06 |
| ADV-COORD-07 | Published File Byte Mutation | Corrupted / Tampered Published Files | Modifying any byte of an assembled published artifact must fail `verify`. | **PASS** | Detects SHA256 mismatch; throws `Checksum mismatch for published file Ferryx_universal.dmg`. | ART-ADV-07 |
| ADV-COORD-08 | Altered Build Receipt Detection | Altered Receipt Identity / Hashes | Altering receipt `commitSha` or contents must fail `verify` during independent re-derivation. | **PASS** | Throws `commitSha mismatch: expected '3a19bdf7...', got '00000000...'`. | ART-ADV-08 |
| ADV-COORD-09 | Publish Double Approval Barrier | Unapproved Publication Attempt | Invoking `publish` without `--approve-publish` flag OR without `FERRYX_APPROVE_PUBLISH=1` must fail closed. | **PASS** | Throws `Publication requires explicit --approve-publish flag` or `Publication requires FERRYX_APPROVE_PUBLISH=1 environment variable`. | ART-ADV-09 |
| ADV-COORD-10 | Remote Tag & Commit Identity Drift | Remote Tag Drift / Mismatch | If remote git tag on origin resolves to a different commit SHA than `plan.commitSha`, publish must fail closed. | **PASS** | Throws `Remote tag '...' resolves to <driftedSha>, but plan requires commit <commitSha>`. | ART-ADV-10 |
| ADV-COORD-11 | Existing Remote Release Collision | Accidental Release Overwrite | If GitHub Release already exists for tag (draft or published), publish must reject rather than overwrite. | **PASS** | Throws `Release '...' already exists on remote repo ... Refusing to overwrite.` | ART-ADV-11 |
| ADV-COORD-12 | Non-Zero Runner Exit Handling | Builder Subprocess Execution Failure | If builder runner fails or returns non-zero exit code, `buildHost` must fail closed and write zero receipts. | **PASS** | Throws `Build on host 'macbook' failed with exit code 1`; zero receipts emitted. | ART-ADV-12 |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-SURF-01A | CLI Output Log | Live execution of `node scripts/release-local.mjs --help` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-01-help.log` |
| ART-SURF-01B | CLI Output Log | Live execution of `bun run release:local --help` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-01-bun-help.log` |
| ART-SURF-01C | ESM Import Log | Dynamic ESM import verification showing zero side-effects and exported function list | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-01-esm-import.log` |
| ART-SURF-02 | CLI Execution Log | Live execution of `prepare` creating deterministic `plan.json`, `prepare-state.json`, and `source-inputs.json` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-02-prepare.log` |
| ART-SURF-03 | CLI Probe Log | Real multi-host probe JSON output capturing `macbook`, `omaki`, and `maho-win` status and disk deficit | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-03-preflight.log` |
| ART-SURF-04 | Platform Log | Standalone Git bundle generation and isolated checkout HEAD verification | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-04-git-bundles.log` |
| ART-SURF-05A | CLI Execution Log | Release manifest assembly execution log staging 12 files and deterministic aliases | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-05-assemble.log` |
| ART-SURF-05B | Utility Log | `shasum -a 256 -c SHA256SUMS.txt` verifying 100% of files including `latest.json` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-05-shasum.log` |
| ART-SURF-06 | CLI Execution Log | Coordinator `verify` execution log validating checksums and independent derivation | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-06-verify.log` |
| ART-SURF-07 | CLI Execution Log | Publish double-approval enforcement, draft creation, asset upload, and undraft verification | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-07-publish.log` |
| ART-SURF-08 | CLI Execution Log | Verification of public remote `latest.json` and `SHA256SUMS.txt` against HTTP endpoint | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-08-verify-remote.log` |
| ART-SUITE-NODE | Node TAP Log | TAP test execution log for coordinator and platform tests (18/18 pass, 0 fail) under Node.js | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-suite-node.log` |
| ART-SUITE-BUN | Bun Test Log | Bun 1.4.0 test execution log for coordinator and platform tests (18/18 pass, 0 fail) | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/qa-suite-bun.log` |
| ART-ADV-01 | Adversarial Log | Fail-closed rejection of mutating release stages under `GITHUB_ACTIONS=true` and `CI=true` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-01-ci-protection.log` |
| ART-ADV-02 | Adversarial Log | Rejection of non-leap year Feb 29, month 13, April 31, pre-2026, and revision overflow | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-02-calver-dates.log` |
| ART-ADV-03 | Adversarial Log | Preservation and fail-closed abort on pre-existing run output directory collision | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-03-outdir-collision.log` |
| ART-ADV-04 | Adversarial Log | Fail-closed reporting and non-zero exit code when host storage is below `minFreeBytes` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-04-disk-budget.log` |
| ART-ADV-05 | Adversarial Log | Plan tampering detection via SHA256 mismatch against `prepare-state.json` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-05-plan-tampering.log` |
| ART-ADV-06 | Adversarial Log | Fail-closed abort when host workspace directory already exists | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-06-workspace-collision.log` |
| ART-ADV-07 | Adversarial Log | Detection of tampered/mutated bytes in published artifact during verify | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-07-file-mutation.log` |
| ART-ADV-08 | Adversarial Log | Detection of altered receipt identity during independent re-derivation verify | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-08-altered-receipt.log` |
| ART-ADV-09 | Adversarial Log | Rejection of unapproved publication without `--approve-publish` or without env var | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-09-publish-approval.log` |
| ART-ADV-10 | Adversarial Log | Rejection of publish when remote origin tag resolves to different commit SHA | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-10-remote-tag-drift.log` |
| ART-ADV-11 | Adversarial Log | Rejection of publish when release already exists on remote GitHub repository | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-11-release-collision.log` |
| ART-ADV-12 | Adversarial Log | Fail-closed abort and receipt suppression when builder runner returns non-zero exit | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080d4/adv-12-runner-failure.log` |
