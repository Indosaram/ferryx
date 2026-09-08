# Manual QA Matrix — `st_01a080da`

**Task ID:** `st_01a080da`  
**Parent Session:** `01a07f49-03be-7242-be4f-68a7e66c2166`  
**Date:** 2026-09-08  
**Role:** Manual QA Executor (`omo-senpi-qa-executor`)  
**Scope:** Independent verification and live scenario execution of the Local Release Coordinator and Platform Builders subsystem (`scripts/release-local.mjs`, `scripts/lib/release-platforms.mjs`, `scripts/lib/release-hosts.mjs`, `scripts/release-local.test.mjs`, `scripts/release-platforms.test.mjs`, `package.json`).

**Overall Verdict:** **FAIL / CONDITIONAL PASS WITH CRITICAL BLOCKERS**

While several core coordinator stages (`prepare`, `preflight` reachability, `assemble`, `verify`, and `publish` gate) are implemented and functional, manual verification reveals **3 critical defects and unverified wiring**:
1. `planDigest` newline discrepancy breaks all live untampered builds out of the box (`scripts/release-local.mjs:151,175,257`).
2. Remote builder execution for `omaki` and `maho-win` is completely unwritten in the coordinator and platform builders, relying on faked unit test runners (`scripts/lib/release-platforms.mjs:688-691`).
3. Preflight probes check tool presence only and ignore toolchain version constraints and tool exit codes (`scripts/lib/release-platforms.mjs:86-90, 152-161, 284-287`).

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-EXEC-01 | CLI Help Surface & Subcommand Matrix | Node.js & Bun CLI | `node scripts/release-local.mjs --help`, `bun run release:local --help` | **PASS** | Exits 0; displays complete usage for all 7 subcommands (`prepare`, `preflight`, `build`, `assemble`, `verify`, `publish`, `verify-remote`). | ART-SURF-01 |
| SURF-EXEC-02 | Zero-Side-Effect ESM Import & Export Surface | Node.js ESM Import | `node --input-type=module -e 'import * as mod from "./scripts/release-local.mjs"; ...'` | **PASS** | Exits 0; exports 7 functions with zero execution side-effects or process termination on import. | ART-SURF-02 |
| SURF-EXEC-03 | Prepare Stage Execution & Immutable Plan Generation | Coordinator Prepare CLI | `node scripts/release-local.mjs prepare --config scripts/release-hosts.example.json --tag v2026.09.08.1 --commit HEAD --out <tmpDir>` | **PASS** | Exits 0; generates valid `plan.json`, `prepare-state.json`, and `source-inputs.json`. Verifies Ghostty pin extracted from `build_ghostty.rs` matches local submodule HEAD. | ART-SURF-03 |
| SURF-EXEC-04 | Live Multi-Host Environmental Preflight Probes | Coordinator Preflight CLI & Network Transports | `node scripts/release-local.mjs preflight --config scripts/release-hosts.example.json` | **PASS** | Exits 1 (disk deficit). Captured authentic machine status across `macbook`, `omaki`, and `maho-win`. Reported disk deficit on macbook (~7.3Gi free vs 30Gi required). | ART-SURF-04 |
| SURF-EXEC-05 | Platform Git Bundling & Isolated Checkout | Platform Adapter Library | `node --input-type=module -e 'import { createGitBundles } ...'` | **PASS** | Exits 0; generated valid `source.bundle` and `ghostty.bundle` with SHA256 digests; isolated clone verified HEAD matches commit SHA. | ART-SURF-05 |
| SURF-EXEC-06 | Release Manifest Assembly & Checksum Closure | Coordinator Assemble CLI | `node scripts/release-local.mjs assemble --run <runDir> --pubkey <testKey>` | **PASS** | Exits 0; assembled 16 release files, created deterministic aliases, generated `latest.json` first, and verified 100% of files including `latest.json` with `shasum -a 256 -c SHA256SUMS.txt`. | ART-SURF-06 |
| SURF-EXEC-07 | Independent Re-derivation & Byte-for-Byte Verify | Coordinator Verify CLI | `node scripts/release-local.mjs verify --run <runDir> --pubkey <testKey>` | **PASS** | Exits 0; verified 15 release files and confirmed byte-for-byte identity against fresh independent derivation in private temp directory (`mkdtempSync`). | ART-SURF-07 |
| SURF-EXEC-08 | Double Approval Publish Gate & Draft Lifecycle | Coordinator Publish Pipeline | `node --input-type=module -e 'import { publishRelease } ...'` | **PASS** | Exits 0; rejected calls without `--approve-publish` flag or `FERRYX_APPROVE_PUBLISH=1`; executed full draft release creation, asset upload, draft download byte comparison, and undrafting. | ART-SURF-08 |
| SURF-EXEC-09 | Verify Remote Release Public Endpoint | Coordinator Remote Verification | `node --input-type=module -e 'import { verifyRemoteRelease } ...'` | **PASS** | Exits 0; queried public release endpoint, verified HTTP 200 response and exact version match. | ART-SURF-09 |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-EXEC-01 | Mutation Prevention Under CI / Actions Environment | Hosted Release Backdoor / Accidental Run in CI | `GITHUB_ACTIONS=true` or `CI=true` must reject mutating coordinator stages (`prepare`, `build`, `assemble`, `publish`). | **PASS** | Throws `Mutating release stage '<stage>' is forbidden under CI / GitHub Actions environment.` with non-zero exit code. | ART-ADV-01 |
| ADV-EXEC-02 | Release Tag Date Validation (CalVer Boundaries) | Malformed / Impossible Calendar Dates | Impossible dates (`v2026.02.29` non-leap, month 13, April 31, pre-2026, revision overflow >65535) must fail closed before file operations. | **PASS** | Throws `Invalid calendar date in release tag ...` or `Invalid release tag ...`; zero files written to disk. | ART-ADV-02 |
| ADV-EXEC-03 | Run Output Directory Collision Preservation | Run Output Directory Collision | Pre-existing run directory must not be overwritten or modified; coordinator must fail closed. | **PASS** | Throws `Output run directory already exists: <dir>`; existing canary file left untouched. | ART-ADV-03 |
| ADV-EXEC-04 | Plan Digest Tampering / Formatting Trapping | Tampered / Stale Plan vs prepare-state.json | If `plan.json` is modified after `prepare`, `build` must detect digest mismatch against `prepare-state.json` and fail closed. | **PASS** | Detected digest mismatch. Also exposed critical newline bug where fresh untouched `plan.json` fails build. | ART-ADV-04 |
| ADV-EXEC-05 | Host Workspace Collision Prevention | Builder Host Workspace Collision | If `host.root/<runId>` already exists on host, `buildHost` must fail closed to protect prior builds. | **PASS** | Throws `Host workspace already exists: refusing to overwrite (<workspaceDir>)`. | ART-ADV-05 |
| ADV-EXEC-06 | Published Artifact Byte Corruption Detection | Corrupted / Tampered Published Binary | Modifying any byte of an assembled published artifact must fail `verify`. | **PASS** | Detects SHA256 mismatch; throws `Checksum mismatch for published file Ferryx_universal.dmg`. | ART-ADV-06 |
| ADV-EXEC-07 | Altered Build Receipt Detection in Re-derivation | Altered Receipt Identity / Hashes | Altering receipt `commitSha` or contents must fail `assemble` / `verify`. | **PASS** | Throws `Missing expected build receipt file: 'build-receipt-omaki.json'` or commit mismatch. | ART-ADV-07 |
| ADV-EXEC-08 | Publish Double Approval Barrier | Unapproved Publication Attempt | Invoking `publish` without `--approve-publish` flag OR without `FERRYX_APPROVE_PUBLISH=1` must fail closed. | **PASS** | Throws `Publication requires explicit --approve-publish flag` or `Publication requires FERRYX_APPROVE_PUBLISH=1 environment variable`. | ART-ADV-08 |
| ADV-EXEC-09 | Remote Tag & Commit SHA Drift Protection | Remote Tag Drift / Mismatch | If remote git tag on origin resolves to a different commit SHA than `plan.commitSha`, publish must fail closed. | **PASS** | Throws `Remote tag '...' resolves to <driftedSha>, but plan requires commit <commitSha>`. | ART-ADV-09 |
| ADV-EXEC-10 | Pre-existing Remote Release Overwrite Prevention | Accidental Release Overwrite | If GitHub Release already exists for tag (draft or published), publish must reject rather than overwrite. | **PASS** | Throws `Release '...' already exists on remote repo ... Refusing to overwrite.` | ART-ADV-10 |
| ADV-EXEC-11 | Unimplemented Remote Build Dispatch Defect | Missing Coordinator Remote Orchestration | Coordinator must execute builds on remote hosts `omaki` and `maho-win`. | **FAIL** | Throws `Real build for remote host 'omaki' must be initiated by release coordinator`. Remote execution logic is completely missing. | ART-ADV-11 |
| ADV-EXEC-12 | Preflight Presence-Only vs Version Enforcement Defect | Preflight Tool Version Constraints | Preflight probes must enforce configured tool versions and plan toolchains. | **FAIL** | Probes check boolean existence only; configured impossible Bun version (`999.999.999`) passes as `ok: true`. Plan toolchains are discarded. | ART-ADV-12 |
| ADV-EXEC-13 | Partial Valid Receipt Generation Defect | Incomplete Artifact Set Handling | Build producing only partial artifacts (e.g. dmg without updater) must not emit a valid receipt. | **FAIL** | `buildHost` emits `build-receipt-macbook.json` with `exitCode: 0` despite missing `macos-updater`. | ART-ADV-13 |
| ADV-EXEC-14 | Unsafe Overwrite Boundaries & Shell Interpolation | Receipt Clobbering & Script Escaping | File operations and script compositions must prevent clobbering and injection. | **FAIL** | `buildHost` silently overwrites existing `build-receipt-macbook.json`; unquoted `${hostConfig.root}` in shell scripts. | ART-ADV-14 |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-SURF-01 | CLI Output Log | Live execution of `node scripts/release-local.mjs --help` and `bun run release:local --help` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-01-help.log` |
| ART-SURF-02 | ESM Import Log | Dynamic ESM import verification showing zero side-effects and 7 exported functions | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-02-esm-import.log` |
| ART-SURF-03 | CLI Execution Log | Live execution of `prepare` creating `plan.json`, `prepare-state.json`, and `source-inputs.json` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-03-prepare.log` |
| ART-SURF-04 | CLI Probe Log | Real multi-host probe JSON output capturing `macbook`, `omaki`, and `maho-win` status and disk deficit | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-04-preflight.log` |
| ART-SURF-05 | Platform Log | Standalone Git bundle generation and isolated checkout HEAD verification | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-05-git-bundles.log` |
| ART-SURF-06 | CLI Execution Log | Release manifest assembly log staging 16 files, aliases, and `shasum -c SHA256SUMS.txt` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-06-assemble.log` |
| ART-SURF-07 | CLI Execution Log | Coordinator `verify` execution log validating checksums and independent derivation | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-07-verify.log` |
| ART-SURF-08 | Execution Log | Publish double-approval enforcement, draft creation, asset upload, and undraft verification | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-08-publish.log` |
| ART-SURF-09 | Execution Log | Verification of public remote `latest.json` and `SHA256SUMS.txt` against HTTP endpoint | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/surf-09-verify-remote.log` |
| ART-ADV-01 | Adversarial Log | Fail-closed rejection of mutating release stages under `GITHUB_ACTIONS=true` and `CI=true` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-01-ci-protection.log` |
| ART-ADV-02 | Adversarial Log | Fail-closed rejection of invalid CalVer calendar dates and revision overflow | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-02-calver-dates.log` |
| ART-ADV-03 | Adversarial Log | Fail-closed preservation of pre-existing run output directory | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-03-outdir-collision.log` |
| ART-ADV-04 | Adversarial Log | Trapping of plan digest mismatch and demonstration of the newline bug on fresh runs | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-04-plan-digest-defect.log` |
| ART-ADV-05 | Adversarial Log | Fail-closed protection against host workspace collision | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-05-workspace-collision.log` |
| ART-ADV-06 | Adversarial Log | Detection of single-bit corruption in published artifact during verify | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-06-file-mutation.log` |
| ART-ADV-07 | Adversarial Log | Rejection of altered commit SHA in build receipt | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-07-altered-receipt.log` |
| ART-ADV-08 | Adversarial Log | Rejection of publish invocation without double approvals | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-08-publish-approval.log` |
| ART-ADV-09 | Adversarial Log | Rejection of publish when remote tag drifts from release plan commit SHA | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-09-remote-tag-drift.log` |
| ART-ADV-10 | Adversarial Log | Rejection of publish when release already exists on remote GitHub repo | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-10-release-collision.log` |
| ART-ADV-11 | Adversarial Log | Verification of the missing remote build dispatch logic for `omaki` and `maho-win` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-11-remote-dispatch-defect.log` |
| ART-ADV-12 | Adversarial Log | Demonstration that preflight tool probes check presence only and ignore version constraints | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-12-presence-only-defect.log` |
| ART-ADV-13 | Adversarial Log | Demonstration that `buildHost` emits a validated receipt for an incomplete build missing `macos-updater` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-13-partial-valid-receipt.log` |
| ART-ADV-14 | Adversarial Log | Demonstration of silent receipt clobbering by `buildHost` | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080da/adv-14-unsafe-overwrites.log` |
