# Manual QA Matrix — `st_01a07fc9`

**Goal:** Independent manual QA executor verification of the receipt-driven release assembly engine (`scripts/build-latest-json.mjs`, `scripts/lib/release-contract.mjs`) and integration corrections against the binding contract (`docs/releases/local-pipeline-audit-2026-09-08/IMPLEMENTATION_CONTRACT.md`, Sections 4, 6, 8, 9).

**Overall Verdict: PASS (Zero Actionable Blockers)**

All criteria and adversarial cases have been independently executed against live CLI, programmatic Node.js ESM surfaces, and the host filesystem, verifying:
1. **Three Hosts & Kinds Invariant Matrix:** Exactly three host receipts (`macbook`, `omaki`, `maho-win`) strictly bound to permitted kinds.
2. **Identity & DAG Closure:** Exact equality of `commitSha`, `appVersion`, and `runId` across plan and receipts; `exitCode === 0`.
3. **Strict Schema Strictness & No Credential Storage:** Rejection of `$schema` URLs, unknown properties, and credentials in both plan and receipts.
4. **Toolchains Receipt Provenance:** Optional `toolchains` object in receipts verified against whitelisted keys (`node`, `bun`, `zig`, `rust`, `tauri`) with unknown/credential property rejection.
5. **Real Cryptographic Minisign Verification:** Updater payloads and trusted comments verified with `verifyMinisign` against authentic and synthetic Ed25519 fixtures.
6. **macOS Updater Archive Layout Check:** All `macos-updater` `.app.tar.gz` payloads validated via `scripts/assert-updater-archive-layout.mjs` ensuring bundle rooting in `Ferryx.app/` and zero AppleDouble (`._*`) entries.
7. **Post-Copy Staged Artifact Checksum Verification:** Artifact hashes verified both before copy and immediately after staging in destination before publication, preventing TOCTOU/copy-boundary data corruption.
8. **Realpath Filesystem Jail:** Inode resolution traps directory traversal (`..`) and symlink escapes outside `artifactsDir`.
9. **Collision-Free Deterministic Stable Aliases:** Exact inventory payloads and signatures staged alongside canonical aliases for standard and NSIS migration channels.
10. **Full Checksum Closure & Pre-Creation:** `latest.json` written first; `SHA256SUMS.txt` hashes 100% of published files including `latest.json`.
11. **Atomic Publication & Safe Failure Cleanup:** Pre-existing `outDir` rejected before staging; mid-process abort deletes sibling staging directory leaving zero partial files.

---

## surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | Evidence / exact result | artifactRefs |
|---|---|---|---|---|---|---|
| SURF-01 | Three Hosts & Kinds Standard Assembly | CLI Subprocess | `node scripts/build-latest-json.mjs --plan <planPath> --receipts-dir <receiptsDir> --artifacts-dir <artifactsDir> --out-dir <outDir> --pubkey <wrappedPubkey>` | **PASS** | Exit code 0; 16 publish files staged in output directory matching 3 hosts and 5 kinds; latest.json created. | ART-SURF-1 |
| SURF-02 | NSIS Migration Channel Assembly | CLI Subprocess | `node scripts/build-latest-json.mjs ... (nsisMigration: true)` | **PASS** | Exit code 0; 18+ publish files staged; includes `Ferryx_x64-setup.exe` and `.sig`; updater platforms include `windows-x86_64`. | ART-SURF-2 |
| SURF-03 | Real Checked-in Repo Archive Minisign Verification | Pure ESM / Crypto Engine | `verifyMinisign({ data: archiveBuf, signature: sigText, publicKey: pubkey })` | **PASS** | Returns `true`; authentic checked-in repo archive fixture verified against checked-in Tauri config public key. | ART-SURF-3 |
| SURF-04 | Strict Toolchains Receipt Provenance | Receipt Parser | `parseReceipt(rawMacReceipt)` | **PASS** | Successfully parsed optional `toolchains` in receipt (`node`, `bun`, `zig`, `rust`, `tauri`); preserved in receipt object. | ART-SURF-4 |
| SURF-05 | macOS Updater Archive Layout Verification | CLI Subprocess | `node scripts/assert-updater-archive-layout.mjs <archive.app.tar.gz>` | **PASS** | Exit code 0; stdout: `updater archive layout OK: 6 entries`; verifies valid gzip tarball rooted in `Ferryx.app/`. | ART-SURF-5 |
| SURF-06 | Post-Copy Checksum Verification | Programmatic Assembly | `assembleRelease({ planPath, receiptsDir, artifactsDir, outDir, publicKey })` | **PASS** | Re-computed SHA256 of every file written to staging directory; verified matching `artifact.sha256` before rename. | ART-SURF-6 |
| SURF-07 | Checksum Closure & Pre-Creation Verification | OS Utility CLI (`shasum`) | `shasum -a 256 -c SHA256SUMS.txt` | **PASS** | Exit code 0; 100% of files verified OK (including `latest.json`); pub_date matches plan `sourceDateEpoch`. | ART-SURF-7 |
| SURF-08 | Pre-Existing Directory Rejection & Atomic Publication | CLI Subprocess | `assembleRelease(...)` into pre-existing directory | **PASS** | Throws `Output directory already exists`; canary file in target untouched; zero `.staging-*` directories leaked. | ART-SURF-8 |

---

## adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | Evidence | artifactRefs |
|---|---|---|---|---|---|---|
| ADV-01 | Copy Boundary Data Mutation | Fault Injection / TOCTOU | Mutating source bytes during copy must fail post-copy verification and leave no publishable output. | **PASS** | Throws `Post-copy checksum verification failed`; `outDir` not created; staging dir deleted. | ART-ADV-1 |
| ADV-02 | macOS Updater Archive Layout | Malformed archive / AppleDouble | macOS updater payload not rooted in `Ferryx.app` or with AppleDouble entries must fail closed before staging. | **PASS** | Throws `macOS updater archive layout validation failed`; `outDir` not created. | ART-ADV-2 |
| ADV-03 | Receipt Toolchains Strictness | Embedded credential / rogue key | Supplying `apiToken` or unknown property in receipt `toolchains` must fail closed. | **PASS** | Throws `unexpected toolchain property: apiToken`; `outDir` not created. | ART-ADV-3 |
| ADV-04 | Receipt Toolchains Type Strictness | Non-string toolchain value | Supplying non-string value (e.g. integer) for toolchain must fail closed. | **PASS** | Throws `toolchains.node must be a non-empty string`; `outDir` not created. | ART-ADV-4 |
| ADV-05 | Host Matrix Completeness | Missing required host receipt | Omitting `build-receipt-omaki.json` must fail closed before creating output. | **PASS** | Throws `Missing expected build receipt file: 'build-receipt-omaki.json'`; `outDir` not created. | ART-ADV-5 |
| ADV-06 | Identity Binding Integrity | Receipt commitSha skew | Skewed `commitSha` in receipt must fail closed immediately. | **PASS** | Throws `commitSha mismatch`; `outDir` not created. | ART-ADV-6 |
| ADV-07 | Identity Binding Integrity | Receipt appVersion skew | Skewed `appVersion` in receipt must fail closed immediately. | **PASS** | Throws `appVersion mismatch`; `outDir` not created. | ART-ADV-7 |
| ADV-08 | Identity Binding Integrity | Receipt runId skew | Skewed `runId` in receipt must fail closed immediately. | **PASS** | Throws `runId mismatch`; `outDir` not created. | ART-ADV-8 |
| ADV-09 | Identity Binding Integrity | Non-zero exitCode | Receipt reporting `exitCode: 1` must fail closed immediately. | **PASS** | Throws `exitCode must be 0, got 1`; `outDir` not created. | ART-ADV-9 |
| ADV-10 | Schema Strictness | Receipt unexpected root property | Supplying unexpected property (`privateKey`) in receipt must fail closed. | **PASS** | Throws `Receipt contains unexpected property: privateKey`; `outDir` not created. | ART-ADV-10 |
| ADV-11 | Path Traversal Prevention | Safe relative path traversal (`..`) | Specifying `relPath: "../../etc/passwd"` must fail closed. | **PASS** | Throws `relPath must be a safe relative path`; `outDir` not created. | ART-ADV-11 |
| ADV-12 | Path Jail Trapping | Symlink escaping artifacts root | Symlink pointing outside `artifactsDir` must be trapped by realpath jail. | **PASS** | Throws `Artifact path escapes artifacts directory`; `outDir` not created. | ART-ADV-12 |
| ADV-13 | Cryptographic Integrity | Updater payload tampering | Modifying payload bytes after signing must fail Minisign verification. | **PASS** | Throws `Artifact SHA256 mismatch` / `Minisign verification failed`; `outDir` not created. | ART-ADV-13 |
| ADV-14 | Cryptographic Integrity | Signature line corruption | Corrupting signature lines must fail Minisign parser/signature check. | **PASS** | Throws `Minisign verification failed`; `outDir` not created. | ART-ADV-14 |
| ADV-15 | Cryptographic Integrity | Trusted comment forgery | Modifying trusted comment text while keeping payload signature intact must fail global signature verification. | **PASS** | Throws `trusted comment signature verification failed`; `outDir` not created. | ART-ADV-15 |
| ADV-16 | Target Platform Collision | Unpermitted target for host | Claiming Linux target on macOS host must fail closed. | **PASS** | Throws `invalid target 'linux-x86_64' for kind 'macos-updater'`; `outDir` not created. | ART-ADV-16 |
| ADV-17 | Destination Isolation | Pre-existing destination folder | Pre-existing output destination directory must fail closed without writing any file. | **PASS** | Throws `Output directory already exists`; canary file intact. | ART-ADV-17 |
| ADV-18 | Parameter Safety | Legacy CLI options | Supplying legacy parameters (`--version`, `--dir`, `--out`) must fail with exit code 2. | **PASS** | Exit code 2; stderr prints migration instructions. | ART-ADV-18 |
| ADV-19 | Checksum Integrity | Staged byte flip | Appending byte to published file after checksum creation must fail `shasum -c`. | **PASS** | `shasum -a 256 -c` fails with exit code 1; reports `Ferryx_amd64.deb: FAILED`. | ART-ADV-19 |

---

## artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| ART-SURF-1 | CLI Execution Log | CLI standard assembly happy path execution log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-01-standard-assembly.log` |
| ART-SURF-2 | CLI Execution Log | CLI NSIS migration channel assembly log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-02-nsis-migration.log` |
| ART-SURF-3 | CLI Execution Log | Minisign verification log of checked-in repo archive fixture | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-03-real-archive-crypto.log` |
| ART-SURF-4 | Parser Log | Receipt toolchains provenance inspection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-04-toolchains-provenance.log` |
| ART-SURF-5 | CLI Execution Log | macOS updater archive layout verification log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-05-macos-archive-layout.log` |
| ART-SURF-6 | Assembly Log | Post-copy SHA256 checksum re-verification execution log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-06-post-copy-verification.log` |
| ART-SURF-7 | Checksum Log | `shasum -a 256 -c SHA256SUMS.txt` validation log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-07-checksum-closure.log` |
| ART-SURF-8 | Isolation Log | Pre-existing destination rejection and atomic isolation log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/surf-08-atomic-isolation.log` |
| ART-ADV-1 | Adversarial Log | Copy boundary data mutation fault-injection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-01-copy-boundary-tamper.log` |
| ART-ADV-2 | Adversarial Log | Invalid macOS updater archive layout rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-02-invalid-archive-layout.log` |
| ART-ADV-3 | Adversarial Log | Receipt toolchain embedded credential rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-03-toolchains-credential-rejected.log` |
| ART-ADV-4 | Adversarial Log | Receipt toolchain non-string type rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-04-toolchains-invalid-type.log` |
| ART-ADV-5 | Adversarial Log | Missing required host receipt rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-05-missing-host-receipt.log` |
| ART-ADV-6 | Adversarial Log | Receipt commit SHA mismatch rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-06-commit-sha-skew.log` |
| ART-ADV-7 | Adversarial Log | Receipt app version mismatch rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-07-app-version-skew.log` |
| ART-ADV-8 | Adversarial Log | Receipt run ID mismatch rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-08-run-id-skew.log` |
| ART-ADV-9 | Adversarial Log | Receipt non-zero exit code rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-09-non-zero-exitcode.log` |
| ART-ADV-10 | Adversarial Log | Receipt unexpected root property rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-10-unexpected-root-property.log` |
| ART-ADV-11 | Adversarial Log | Artifact safe relative path traversal rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-11-path-traversal-rejection.log` |
| ART-ADV-12 | Adversarial Log | Symlink realpath jail escape rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-12-symlink-jail-escape.log` |
| ART-ADV-13 | Adversarial Log | Updater payload tampering rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-13-payload-tampering.log` |
| ART-ADV-14 | Adversarial Log | Signature corruption rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-14-signature-corruption.log` |
| ART-ADV-15 | Adversarial Log | Trusted comment forgery rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-15-comment-forgery.log` |
| ART-ADV-16 | Adversarial Log | Target platform collision rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-16-target-collision.log` |
| ART-ADV-17 | Adversarial Log | Pre-existing output destination rejection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-17-pre-existing-destination.log` |
| ART-ADV-18 | Adversarial Log | Legacy CLI parameter deprecation log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-18-legacy-cli-rejected.log` |
| ART-ADV-19 | Adversarial Log | Checksum mutation bit-flip detection log | `.omo/evidence/01a07f49-03be-7242-be4f-68a7e66c2166/st_01a07fc9/adv-19-checksum-bitflip-detected.log` |
