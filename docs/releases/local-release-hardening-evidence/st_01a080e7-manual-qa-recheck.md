# Manual QA Recheck - st_01a080e7

Date: 2026-09-08
Role: `omo-senpi-qa-executor`
Overall verdict: **FAIL**

Scope of this recheck: verify the newly exported shared redaction helper, then re-execute the real remote build, signing, runner-result, and preparation-digest seams. No product files were edited.

No ULW plan exists for this child session (`ULW_LOOP_PLAN_MISSING`), so evidence remains in the caller evidence directory.

## manualQa.surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | artifactRefs |
|---|---|---|---|---|---|
| R1 | `redactProcessOutput(text,effectiveEnv)` is exported; `runProcess` masks stdout/stderr/message using the effective environment | Exported Node ESM API and a real child process emitting a generated fake signing secret on stdout/stderr before exit 7 | `node --input-type=module < redaction harness` | **PASS** - `DIRECT_MASKED=true`, `ERROR_MASKED=true`, exit code 7 is preserved, and both captured streams contain `[REDACTED]`; no fake-secret value appears in the artifact. | R-A1 |
| R2 | Focused host/coordinator/platform tests remain green after host redaction changes | Node test runner | `node --test scripts/release-hosts.test.mjs scripts/release-local.test.mjs scripts/release-platforms.test.mjs` | **PASS** - 52/52 pass, 0 failed, 0 skipped, exit 0. Includes executable SSH fixture transport, live read-only remote smoke, redaction, timeout/process-tree, and non-EPIPE input-error cases. | R-A2 |
| R3 | Linux and Windows real builds must execute through transport/staging rather than an unfinished coordinator placeholder | Real `buildHost({hostName:"omaki",runner:null})` after authentic SSH collision check against a unique `/tmp/<runId>` | `node --input-type=module < real-omaki-transport recheck harness` | **FAIL** - rejects with `Real build for remote host 'omaki' must be initiated by release coordinator.` Linux source transport/build/staging remains absent; the same branch covers Windows. | R-A3, R-A6 |
| R4 | Missing runner/build result must fail before receipt creation | Programmatic `buildHost` with owned temporary run/workspace; fixture writes plausible Mac artifacts and returns `undefined` | `node --input-type=module < missing-runner-result recheck harness` | **FAIL** - `UNEXPECTED_RECEIPT_CREATED=true`, receipt exit code is 0, receipt exists, and the harness exits 2. | R-A4, R-A6 |
| R5 | Assemble/verify/publish revalidate prepared plan and config digests before consuming release state | Programmatic `assembleReleaseRun` with valid plan and deliberately mismatched `prepare-state.json.planDigest` | `env -u CI -u GITHUB_ACTIONS node --input-type=module < assemble-tampered-digest recheck harness` | **FAIL** - first rejection is missing Mac receipt, with `DIGEST_REJECTED_FIRST=false`; assemble did not check the mismatched digest. Current source also contains no equivalent gate in verify or publish. | R-A5, R-A6 |
| R6 | Synchronous wrappers use shared redaction before exposing command output/errors; credential-bearing commands avoid inherited stdio | Source-backed synchronous command surfaces in `release-platforms.mjs` and `release-local.mjs` | Numbered source inspection preserved in R-A6 | **FAIL** - neither module imports `redactProcessOutput` or uses `runProcess` for synchronous wrappers. `release-local.mjs` interpolates raw `spawnSync` stderr/stdout into publish errors; platform build uses raw `execFileSync` failures. No `stdio: inherit` was found in these files, but shared masking is not integrated. | R-A6 |
| R7 | Central updater signing uses exact positional `cargo tauri signer sign <FILE>` with inherited secret environment, or builders prove required generated signatures | Builder source and focused tests | Search and numbered source inspection; focused test invocation R2 | **FAIL** - no signer command exists in `release-platforms.mjs`; Mac only copies a `.sig` if present, and remote builders are unimplemented. Focused tests do not execute a signer fixture. | R-A2, R-A6 |

## manualQa.adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | artifactRefs |
|---|---|---|---|---|---|
| R-ADV-1 | Process output must not expose inherited signing secrets | Credential exfiltration via stdout/stderr/nonzero error | Replace every occurrence using the effective child environment while retaining exit status and ordinary output | **PASS** - direct helper and real child-process failure are masked. | R-A1 |
| R-ADV-2 | Remote build must not bypass actual staging | Unimplemented real path | Create fresh owned remote directory before transport, copy source/Ghostty/plan inputs, execute generated platform script, retrieve exact outputs, and validate receipt | **FAIL** - real omaki path still reaches explicit throw. | R-A3, R-A6 |
| R-ADV-3 | Missing execution evidence is failure | Undefined runner result | Reject and create no receipt | **FAIL** - undefined is defaulted to exit code 0 and receipt is written. | R-A4, R-A6 |
| R-ADV-4 | Immutable preparation state is checked at each downstream stage | Plan/config tampering | Reject digest mismatch before receipt, artifact, network, or publisher operations | **FAIL** - assemble proceeds to receipt lookup. Verify and publish lack visible gates as well. | R-A5, R-A6 |
| R-ADV-5 | Updater signature cannot be inferred from optional file presence | Missing signing output | Execute positional signer command or fail if builder-required signature was not generated and validated | **FAIL** - no signer execution exists and scan permits updater artifact with `signatureRelPath: null` until later assembly. | R-A6 |
| R-ADV-6 | Synchronous child errors are redacted | Secret echoed by credential-bearing subprocess | Apply shared `redactProcessOutput` with the same effective environment before exposing stdout/stderr/message | **FAIL** - host `runProcess` is correct, but coordinator/platform synchronous wrappers have not adopted it or the helper. | R-A6 |
| R-ADV-7 | Windows generated PowerShell and Linux generated shell build paths execute with fixture tools | Mock bypass / transport correctness | Exercise actual generated scripts, native nonzero exits, fresh remote staging, absolute source/target paths, and missing outputs | **FAIL** - host transport itself is tested, but no generated platform build scripts exist in the production builder. | R-A2, R-A3, R-A6 |

## manualQa.artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| R-A1 | Execution transcript | Generated fake-secret direct redaction and real child-process redaction proof | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-redaction-recheck.log` |
| R-A2 | TAP transcript | Complete recheck of host, coordinator, and platform tests: 52/52 passing | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-focused-node-tests-recheck.log` |
| R-A3 | Real SSH/build transcript | Authentic omaki collision probe followed by unfinished remote-build rejection | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-real-omaki-transport-recheck.log` |
| R-A4 | Adversarial execution transcript | Undefined runner result incorrectly produces exit-code-0 receipt | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-adversarial-missing-runner-recheck.log` |
| R-A5 | Adversarial execution transcript | Assemble ignores mismatched prepared plan digest before receipt processing | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-adversarial-assemble-digest-recheck.log` |
| R-A6 | Numbered source evidence | Current imports and critical build/coordinator sections showing remote throw, default success, absent signer, absent downstream digest gates, and unredacted synchronous wrappers | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-source-critical-gaps-recheck.log` |

## Verdict

The lead-owned `release-hosts.mjs` change is verified **PASS**. The requested remaining platform/coordinator implementation is still **FAIL** in the currently readable working tree:

1. Linux and Windows real build paths are still an explicit throw.
2. Missing runner result still becomes success and creates a receipt.
3. Assemble, verify, and publish still do not all revalidate prepared plan/config digests.
4. No positional Tauri signer path exists and required signatures are not proved by builders.
5. Synchronous coordinator/platform wrappers have not integrated shared redaction.
6. Generated Linux shell/Windows PowerShell build/staging paths do not exist and therefore cannot be exercised.

This recheck supersedes only the redaction portion of the prior matrix; the overall release-hardening verdict remains FAIL.
