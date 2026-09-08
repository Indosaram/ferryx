# Manual QA Matrix - st_01a080e7

Date: 2026-09-08
Role: `omo-senpi-qa-executor`
Overall verdict: **FAIL**

The focused tests pass, but they do not exercise or enforce several required real seams. Direct execution confirms three release-blocking implementation defects: remote builds are unimplemented, a runner that returns no result is treated as exit code 0 and receives a receipt, and assemble does not revalidate the prepared plan digest before processing release inputs. The requested end state is therefore not implemented or verified.

No ULW plan exists for this child session (`ULW_LOOP_PLAN_MISSING`), so artifacts are stored in the caller's evidence directory, `docs/releases/local-release-hardening-evidence/`.

## manualQa.surfaceEvidence

| Scenario ID | Criterion reference | Surface | Exact invocation | Verdict | artifactRefs |
|---|---|---|---|---|---|
| S1 | Deliverable focused tests; generated/real seams must catch missing staging, outputs, and process results | Node test runner | `node --test scripts/release-local.test.mjs scripts/release-platforms.test.mjs` | **PASS (test command only)** - 18/18 tests pass in one run, with no skips. This does not establish product completion because S5-S7 directly expose uncovered defects. | A1 |
| S2 | `package.json` exposes `release:local`; all seven stages are addressable | Package CLI | `bun run release:local --help` | **PASS** - exits 0 and lists prepare, preflight, build, assemble, verify, publish, and verify-remote. | A2 |
| S3 | Prepare creates a fresh owned run, resolves exact source SHA/Ghostty pin, and writes plan/config digest sidecars | Real package CLI and filesystem | `env -u CI -u GITHUB_ACTIONS bun run release:local prepare --config scripts/release-hosts.example.json --tag v2026.09.08.65000 --commit HEAD --out /Users/indo/code/project/orca-lite/docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-prepare-run` followed by Python filesystem/JSON inspection | **PASS** - exits 0; `plan.json`, `prepare-state.json`, and `source-inputs.json` are non-empty; resolved config path and inspected sidecar paths are absolute; source SHA is a full 40-character commit. | A3, A4 |
| S4 | Real read-only preflight probes macOS, Linux, and Windows; checks configured Developer ID/notary and actual x64 linker/MakeAppx; fails closed on unmet prerequisites | Real package CLI over local process and SSH/PowerShell host transport | `bun run release:local preflight --config scripts/release-hosts.example.json` | **PASS** for fail-closed preflight behavior - authentic probes reached all hosts. macOS reported 5,520,875,520 bytes free versus 32,212,254,720 required and missing notary profile; omaki reported missing Zig in noninteractive PATH; maho-win reported x64, `hasLinker: true`, and `hasMakeAppx: true`. Overall exit is 1 as required. This is an environmental blocker to a full release build, not evidence that builders work. | A5 |
| S5 | Linux and Windows real build paths operate end-to-end through actual transport; no unfinished throw | Programmatic `buildHost` with `runner:null`, using real SSH collision probe to omaki and unique `/tmp/<runId>` workspace | `node --input-type=module < real-omaki-transport harness` (full harness preserved in shell transcript) | **FAIL** - after the authentic SSH collision check, `buildHost` rejects with `Real build for remote host 'omaki' must be initiated by release coordinator.` No source transport, generated shell, build, artifact staging, signing, or receipt path exists. The same shared branch covers Windows. | A6, A9 |
| S6 | Validate an actual runner/build command result; a missing result is failure and cannot create a receipt | Programmatic `buildHost` with owned temporary run/workspace and artifact-writing fixture runner that returns `undefined` | `node --input-type=module < missing-runner-result harness` (full harness preserved in shell transcript) | **FAIL** - output records `UNEXPECTED_RECEIPT_CREATED=true`, `exitCode=0`, and `receiptExists=true`; harness exits 2. Missing process evidence is incorrectly defaulted to success. | A7, A9 |
| S7 | Assemble, verify, and publish always revalidate prepared plan/config digests before consuming release data | Programmatic `assembleReleaseRun` over a temporary run containing a valid plan and deliberately mismatched `prepare-state.json.planDigest` | `env -u CI -u GITHUB_ACTIONS node --input-type=module < assemble-tampered-digest harness` (full harness preserved in shell transcript) | **FAIL** - first rejection is `Missing expected build receipt file`, and `DIGEST_REJECTED_FIRST=false`. Assemble ignored the mismatched prepared digest and proceeded into receipt processing. Source inspection also shows digest validation only in build, not assemble/verify/publish. | A8, A9 |
| S8 | Publisher requires dual approval and performs draft create/upload/download-byte-verification/undraft through a real executable process seam without mutating GitHub | Node test runner invoking an executable fake `gh` fixture | Included in `node --test scripts/release-local.test.mjs scripts/release-platforms.test.mjs`; subtests `publish: enforces double approval gate...` and `publish: full pipeline with fake gh executable and byte verification` | **PASS** - both subtests pass; fake executable persists uploaded bytes, download copies them back, and final state is undrafted. No real GitHub publication occurred. Plan/config digest revalidation remains failed separately in S7. | A1 |
| S9 | Verify-remote checks public metadata availability and version | Node HTTP fixture server plus exported verifier | Included in focused Node invocation; subtest `verify-remote: verifies release availability against local HTTP server` | **PASS** - local HTTP fixture returned metadata and checksums and the verifier accepted the matching version. This does not prove production GitHub availability, which was intentionally not exercised. | A1 |

## manualQa.adversarialCases

| Scenario ID | Criterion reference | Adversarial class | Expected behavior | Verdict | artifactRefs |
|---|---|---|---|---|---|
| ADV-1 | Missing runner/process result is failure | Missing subprocess evidence | Reject before receipt creation when runner returns `undefined` or omits a numeric exit code | **FAIL** - success receipt was written with exit code 0. | A7 |
| ADV-2 | Real remote Linux/Windows build paths are complete | Real-path bypass / unfinished branch | Execute actual SSH/SCP/generated platform script and validate outputs; never stop at a coordinator placeholder | **FAIL** - real omaki invocation reaches the explicit unfinished throw. | A6, A9 |
| ADV-3 | Every downstream stage revalidates immutable plan/config | Input tampering / TOCTOU | Assemble rejects the plan digest mismatch before reading receipts or artifacts | **FAIL** - missing receipt error occurred first; digest mismatch was ignored. | A8, A9 |
| ADV-4 | Fresh run/workspace ownership and collision safety | Existing-output collision | Refuse to overwrite an existing prepare output or host workspace | **PASS** - focused tests exercise both prepare output collision and build workspace collision and pass. | A1 |
| ADV-5 | Publication requires explicit double approval | Missing authorization | Reject without `--approve-publish`; reject without `FERRYX_APPROVE_PUBLISH=1` | **PASS** - both rejection cases pass. | A1 |
| ADV-6 | Disk preflight fails closed and does not certify a build | Resource exhaustion | Return structured failures and nonzero status when configured free-space budget is unmet | **PASS** - real macOS probe reports exact required/available bytes and CLI exits 1. | A5 |
| ADV-7 | Actual generated shell/PowerShell transport must be exercised | Mock-bypass insufficiency | Tests execute fixture tools through generated scripts/transport, including missing output and native nonzero exits | **FAIL** - platform tests contain only one top-level injected runner build test; no Linux or Windows build script/transport test exists, and production remote dispatch throws. | A1, A6, A9 |
| ADV-8 | Updater signatures are produced using positional `cargo tauri signer sign <FILE>` and inherited secret env | Signing boundary | Builder either proves generated required signatures or invokes the exact signer command without `-f` artifact misuse | **FAIL** - no signer invocation exists in `release-platforms.mjs`; macOS merely copies a `.sig` if Tauri happened to generate one, while remote builders are absent. | A9 |
| ADV-9 | Mac universal build correctness and notarization approval | Platform authenticity / approval | Verify both slices, codesign with configured identity, require approval before notary submission, submit/staple, and validate updater archive | **FAIL** - current Mac branch runs Tauri and copies output but contains no lipo, codesign verification, notarytool, stapler, or archive-layout invocation. | A9 |
| ADV-10 | Full release build under current host prerequisites | Environmental prerequisite | All three preflights pass before any full build/notarization | **FAIL (environment blocker)** - macOS lacks configured free-space budget and notary profile; omaki lacks Zig in noninteractive PATH. A full build was correctly not attempted. | A5 |

## manualQa.artifactRefs

| ID | Kind | Description | Path |
|---|---|---|---|
| A1 | TAP transcript | Complete focused Node test run, 18 passed/0 failed/0 skipped, exit 0 | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-focused-node-tests.log` |
| A2 | CLI transcript | Package-script help invocation and exit status | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-cli-help.log` |
| A3 | CLI/filesystem transcript | Real prepare invocation, plan output, sidecar existence/size/absolute-path inspection | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-prepare-cli.log` |
| A4 | Generated run files | Non-empty prepare output containing plan, immutable-state, source-input, and owned directory structure | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-prepare-run/` |
| A5 | CLI/SSH transcript | Real three-host preflight JSON and exit 1 with exact host prerequisite failures | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-preflight-cli.log` |
| A6 | Real transport transcript | Real omaki buildHost invocation showing unfinished remote build rejection | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-real-omaki-transport.log` |
| A7 | Adversarial execution transcript | Missing runner result incorrectly produces a successful receipt; harness exit 2 | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-adversarial-missing-runner.log` |
| A8 | Adversarial execution transcript | Tampered assemble digest ignored before receipt processing | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-adversarial-assemble-digest.log` |
| A9 | Source excerpt | Numbered critical implementation excerpts showing missing-result default, remote throw, and downstream stage bodies | `docs/releases/local-release-hardening-evidence/artifacts/st_01a080e7-source-critical-gaps.log` |

## Blockers and verdict

1. **Product blocker:** `buildHost` has no real Linux or Windows implementation. This is directly reproducible and is not an environmental excuse.
2. **Product blocker:** `runResult?.exitCode ?? 0` treats missing execution evidence as success and writes a receipt.
3. **Product blocker:** assemble ignores prepared plan/config digests; source inspection indicates verify and publish do as well.
4. **Product blocker:** required signer, universal-architecture, codesign/notary/staple, generated remote shell/PowerShell, transport staging, and scoped cleanup behaviors are not implemented or tested in the delivered platform module.
5. **Environment blocker:** current macOS disk/notary prerequisites and omaki noninteractive Zig prerequisite prevent a legitimate full build even after code is corrected.

The existing `execution.md` status `COMPLETE & VERIFIED GREEN` and its claim that remote builders execute isolated platform builds are contradicted by A6-A9. This QA run rejects that evidence summary.
