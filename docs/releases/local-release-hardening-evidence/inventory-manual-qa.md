# Manual QA Matrix — `st_01a07fb0`

**Goal:** Implement fail-closed, receipt-driven release assembly replacing permissive directory scanning in Ferryx release tooling. Deliver `scripts/lib/release-contract.mjs`, `scripts/release-contract.test.mjs`, `scripts/build-latest-json.mjs`, `scripts/build-latest-json.test.mjs`, and `docs/releases/local-release-hardening-evidence/inventory.md`. Enforce release plan and build receipt schemas (`schemaVersion: 1`), unknown-key rejection, host/kind invariant bindings, full cryptographic updater verification via Minisign, realpath jail to prevent traversal/symlink escape, deterministic stable aliases without collisions, checksum closure across all publish files including `latest.json`, isolated staging with atomic rename, and clean legacy CLI deprecation.

**Overall verdict: PASS**

All criteria are fully satisfied with non-empty verifiable evidence:
1. `scripts/lib/release-contract.mjs` strictly validates release plans and host build receipts with unknown property rejection, `schemaVersion: 1`, and canonical tag versions via `toAppVersion` / `toMsixVersion`.
2. Release plans strictly enforce channel/target consistency (`requiredTargets` must match `channels.nsisMigration` toggle) and reject unauthorized toolchain keys.
3. Three host build receipts are mandatory (`macbook`, `omaki`, `maho-win`) bound by exact `runId`, `commitSha`, and `appVersion`, each having `exitCode == 0`.
4. Host-to-kind bindings are strictly enforced (`macbook` -> `macos-updater`, `dmg`; `omaki` -> `appimage`, `deb`; `maho-win` -> `msix`, plus `nsis` iff `channels.nsisMigration == true`).
5. All updater artifacts (`macos-updater`, `appimage`, `nsis`) require signatures that cryptographically verify against the updater public key via `verifyMinisign` (Ed25519 payload and trusted comment).
6. Filesystem integrity is enforced: exact byte count and SHA256 checksum match the receipt, with a `realpath` jail preventing any traversal or symlink escape beyond `artifactsDir`.
7. Flat publish directory contains exact inventory payloads/signatures plus deterministic stable aliases (`Ferryx_universal.dmg`, `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`, `Ferryx_x64.msix`, and `Ferryx_x64-setup.exe` if migration) with zero collision tolerance.
8. Checksum closure: `latest.json` is generated first with `pub_date` derived from `plan.sourceDateEpoch`, and `SHA256SUMS.txt` is computed across all staged publish files (including `latest.json`).
9. Atomic publication: Sibling temporary staging directory is used; atomic rename occurs only after 100% verification passes. Pre-existing `outDir` fails closed, and any failure leaves zero partial or publishable output.
10. Legacy directory-scanning CLI options (`--version`, `--dir`, `--repo`, `--tag`, `--out`) are rejected with exit code 2 and explicit migration guidance.
11. 33 contract and assembly tests pass in both Node test runner and Bun 1.4.0.
12. 98 of 98 release tests pass across all release test files in the repository with zero regressions.

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| QA-1 | Happy Path Release Assembly CLI | Node CLI Subprocess | `node scripts/build-latest-json.mjs --plan <planPath> --receipts-dir <receiptsDir> --artifacts-dir <artifactsDir> --out-dir <outDir> --pubkey <key>` | **PASS** | Exit code 0; generated valid `latest.json` first, staged all 5 required artifacts, signatures, and stable aliases (`Ferryx_universal.dmg`, `Ferryx_amd64.AppImage`, etc.); `SHA256SUMS.txt` created with 100% closure. | A6, A4 |
| QA-2 | NSIS Migration Channel CLI | Node CLI Subprocess | `node scripts/build-latest-json.mjs --plan <nsisPlan> --receipts-dir <receiptsDir> --artifacts-dir <artifactsDir> --out-dir <outDir> --pubkey <key>` | **PASS** | Exit code 0; `windows-x86_64` included in `latest.json.platforms`; `Ferryx_x64-setup.exe` and `.sig` staged and hashed. | A7, A4 |
| QA-3 | Legacy CLI Parameter Rejection | Node CLI Subprocess | `node scripts/build-latest-json.mjs --version 2026.09.08.1 --dir /tmp --out /tmp/latest.json --repo Indosaram/ferryx --tag v2026.09.08.1` | **PASS** | Exit code 2; stderr: `Legacy CLI arguments are no longer supported. Receipt-driven assembly requires: --plan <path> ...`. | A8, A4 |
| QA-4 | Authentic Repo Signed Archive Layout & Minisign Verification | Pure ESM / Crypto Engine | `verifyMinisign({ data: readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz"), signature: readFileSync("scripts/fixtures/updater/Ferryx.app.tar.gz.sig", "utf8"), publicKey })` | **PASS** | Evaluated authentic repo fixture archive against checked-in Tauri public key; returns `true`. | A9, A4 |
| QA-5 | Stable Aliases & Checksum Closure Verification | OS Utility CLI (`shasum`) | `shasum -a 256 -c SHA256SUMS.txt` within output directory | **PASS** | Exit code 0; 100% of published files in `outDir` (including `latest.json` and all aliases) verified valid. | A10, A4 |
| QA-6 | Bun Runtime Compatibility | Bun Test CLI (`bun test`) | `bun test scripts/build-latest-json.test.mjs scripts/release-contract.test.mjs` | **PASS** | 33 of 33 tests pass under Bun 1.4.0 in 1197ms. | A11, A4 |
| QA-7 | Release Test Suite Zero-Regression Verification | Node Test Runner | `node --test scripts/build-latest-json.test.mjs scripts/release-contract.test.mjs scripts/minisign-verify.test.mjs scripts/sync-version.test.mjs scripts/release-workflow.test.mjs scripts/build-msix.test.mjs scripts/assert-updater-archive-layout.test.mjs` | **PASS** | 98 of 98 tests pass across all release test files with zero regressions. | A12, A5 |

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-1 | Output Isolation | Destination pre-existence | If destination `outDir` already exists on disk, assembly must fail closed to prevent accidental overwrite or partial contamination. | **PASS** | Throws `Output directory already exists: ...`; exit code 1; no files modified. | A13, A4 |
| ADV-2 | Integrity Verification | Artifact content tampering | Modifying payload bytes after receipt generation must cause SHA256 mismatch, failing closed with zero published files. | **PASS** | Throws `Artifact SHA256 mismatch for 'Ferryx.app.tar.gz'`; `outDir` does not exist. | A14, A4 |
| ADV-3 | Signature Verification | Corrupted Minisign signature | Modifying or corrupting updater signature must fail Ed25519 payload verification with zero published output. | **PASS** | Throws `Minisign verification failed ... Minisign payload signature verification failed`; `outDir` does not exist. | A15, A4 |
| ADV-4 | Signature Verification | Trusted comment tampering | Forging or altering the trusted comment text while leaving payload signature intact must fail global comment verification. | **PASS** | Throws `Minisign trusted comment signature verification failed`; `outDir` does not exist. | A16, A4 |
| ADV-5 | Build Skew Detection | Receipt commit/version skew | Receipt with skewed `commitSha`, `runId`, or `appVersion` differing from plan must fail closed immediately. | **PASS** | Throws `commitSha mismatch: expected '5d549980...', got '11111111...'`; `outDir` does not exist. | A17, A4 |
| ADV-6 | Host Matrix Completeness | Missing host receipt | Release plan requires 3 hosts (`macbook`, `omaki`, `maho-win`); omitting any receipt must abort release assembly. | **PASS** | Throws `Missing expected build receipt file: 'build-receipt-omaki.json'`; `outDir` does not exist. | A18, A4 |
| ADV-7 | Receipts Directory Hygiene | Rogue receipt file injection | Placing unwhitelisted files (e.g. `rogue-file.txt` or unknown receipt) into `receiptsDir` must fail closed. | **PASS** | Throws `Unexpected file in receipts directory: 'rogue-file.txt'`; `outDir` does not exist. | A19, A4 |
| ADV-8 | Filesystem Security | Traversal & symlink jail escape | Artifact pointing via `..` or symlink outside `artifactsDir` must be trapped by realpath jail. | **PASS** | Throws `Artifact path escapes artifacts directory: darwin/symlink.dmg`; `outDir` does not exist. | A20, A4 |
| ADV-9 | Delivery Deduplication | Duplicate artifact kind | Two receipts declaring the same artifact kind (e.g. duplicate `msix`) must fail closed with collision error. | **PASS** | Throws `Duplicate artifact kind 'msix' across receipts`; `outDir` does not exist. | A21, A4 |
| ADV-10 | Plan Consistency | Channel/target inconsistency | Setting `channels.nsisMigration: false` while including `windows-x86_64` in `requiredTargets` must fail closed. | **PASS** | Throws `target 'windows-x86_64' requires channels.nsisMigration to be true`; `outDir` does not exist. | A22, A4 |
| ADV-11 | Strict Schema Enforcement | Unknown properties & fake URL | Supplying `$schema` or unexpected properties in plan/receipt must fail closed. | **PASS** | Throws `Plan contains unexpected property: $schema`; `outDir` does not exist. | A23, A4 |

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| A1 | Node Test Runner Log | RED baseline for contract tests prior to `release-contract.mjs` implementation | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/qa-1-contract-red.log` |
| A2 | Node Test Runner Log | GREEN test run for `scripts/release-contract.test.mjs` (22/22 pass) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/qa-2-contract-green.log` |
| A3 | Node Test Runner Log | RED baseline for assembly tests prior to `build-latest-json.mjs` hardening | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/qa-3-assemble-red.log` |
| A4 | Node Test Runner Log | GREEN test run for `build-latest-json.test.mjs` and `release-contract.test.mjs` (33/33 pass) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/qa-4-assemble-green.log` |
| A5 | Node Test Runner Log | Comprehensive 98/98 green run across all release test files | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/qa-5-all-release-tests-green.log` |
| A6 | CLI Subprocess Log | Real CLI happy path execution with synthetic signed fixtures, aliases, and latest.json | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-01-cli-happy-path.log` |
| A7 | CLI Subprocess Log | Real CLI execution with NSIS migration channel active, verifying windows updater target | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-02-nsis-migration.log` |
| A8 | CLI Subprocess Log | Real CLI rejection of legacy parameters with exit code 2 and migration message | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-03-legacy-cli-rejected.log` |
| A9 | Node Execution Log | Cryptographic verification of authentic repo archive fixture against Tauri public key | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-04-repo-archive-minisign.log` |
| A10 | CLI Checksum Log | Execution of `shasum -a 256 -c SHA256SUMS.txt` proving 100% checksum closure | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-05-checksum-closure.log` |
| A11 | Bun Test Runner Log | Bun 1.4 test runner execution log confirming 33/33 passes | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-06-bun-test.log` |
| A12 | Node Test Runner Log | Multi-suite Node test runner execution log (98/98 pass) | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-qa-07-all-release-tests.log` |
| A13 | CLI Subprocess Log | Adversarial case: pre-existing outDir rejected before mutation | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-01-existing-outdir.log` |
| A14 | CLI Subprocess Log | Adversarial case: payload modified after receipt generation fails SHA256 check | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-02-payload-tampering.log` |
| A15 | CLI Subprocess Log | Adversarial case: corrupted Minisign signature rejected with zero partial output | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-03-signature-tampering.log` |
| A16 | CLI Subprocess Log | Adversarial case: forged trusted comment rejected by global signature verification | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-04-comment-tampering.log` |
| A17 | CLI Subprocess Log | Adversarial case: receipt commit SHA mismatch rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-05-commit-version-skew.log` |
| A18 | CLI Subprocess Log | Adversarial case: missing host receipt rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-06-missing-host-receipt.log` |
| A19 | CLI Subprocess Log | Adversarial case: rogue unwhitelisted file in receiptsDir rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-07-unexpected-receipt-file.log` |
| A20 | CLI Subprocess Log | Adversarial case: symlink escaping artifacts directory caught by realpath jail | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-08-path-jail-escape.log` |
| A21 | CLI Subprocess Log | Adversarial case: duplicate artifact kind across receipts rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-09-duplicate-kind.log` |
| A22 | CLI Subprocess Log | Adversarial case: inconsistent plan channels and requiredTargets rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-10-channel-target-inconsistency.log` |
| A23 | CLI Subprocess Log | Adversarial case: unexpected property and `$schema` URL rejected | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fb0/inv-adv-11-unknown-props-rejection.log` |
