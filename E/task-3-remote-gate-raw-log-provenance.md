# Task 3 remote gate raw-log provenance (2026-10-03)

This index describes existing raw files and known provenance gaps. It does not turn incomplete or contested runs into passes.

## Actual prior archive location

`/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/.omo/evidence/local-pane-liveness-root-remediation/`

The earlier report's phrase “Saved” referred to files at the path above, not at the wave1 root `E/` path. Exact observed local sizes and SHA-256 values were:

| Filename | Bytes | SHA-256 |
|---|---:|---|
| `task-3-check-qa-frozen.log` | 53,419 | `723271c9b3364872e4fb0be511478bea5e114469a3d08f40a75feec396d7c2c5` |
| `task-3-test-core-frozen.log` | 44,130 | `f3aa7b4229b637d6b61f3111cddad5ac2580b813eaa29e4e8b51058f741cfad9` |
| `task-3-test-writer-frozen.log` | 44,130 | `afd99cf3a9d28773feb556e3b4851f888fec265bbb825ae30d02b413a6d0ce01` |
| `task-3-test-pres-frozen.log` | 44,130 | `c1f28241fbddee31bd44436c3ec4cf0740d1153b8dc064fc3c9753012a21fc6e` |
| `task-3-build-bin-frozen.log` | 19,528 | `9c4d7e64e9f318a00f81cf46e94cf413344488a31cec6bc27fddb1357bb4b3f9` |
| `task-3-cli-frozen-err1.log` | 404 | `ae93062b43862736f89623222ce149c795254a3a437d6b6302b46bff5afc2326` |
| `task-3-cli-frozen-err2.log` | 420 | `e27070c3bca6a78aedb1fd9746d671976c3db15305cbce833be9569d4262a223` |
| `task-3-runner-frozen-final.log` | 2,068 | `7df59532e6808ec77da287d00c87e6331412c57508abbc9b58ffdfc8b4921453` |

These local copies are not yet copied to `E/`; `apply_patch` cannot carry forward bytes from the remote host or silently copy files. The original root read of `E/task-3-test-core-frozen.log` correctly returned `ENOENT`.

## Provenance and outcome qualifications

- The prior core test log was read in full near its end and contains a linker error `LNK1104` opening `target\\debug\\deps\\ferryx_lib-39a96428895251c2.exe`; it has no completed test result. Two `cargo test ... qa_barrier` process trees were observed simultaneously using that exact output path. Do not report the three test log files as independent successful test runs or infer pass counts from their names/sizes.
- The runner receipt `task-3-runner-frozen-final.log` was captured by the earlier session using PowerShell `Get-Content -Raw`. Subsequent remote inspection found its corresponding JSON evidence under `C:\\Users\\sook\\task3-product-gate-01a100ad\\qa-evidence-frozen-final\\task-3-harness\\diagnostic-classifier\\run-797f8b4d-10a1-461c-b4e2-12333f5bf712\\result.json`: verdict `BLOCKED`, error `BARRIER_ACK_TIMEOUT` while waiting for presentation receipt index 0 (2,000 ms). It records backend-write held/released and two receipts, but presentation receipt count 0 and no release. Its cleanup result says runner process PID 3476 was reaped and isolation directory removed. This is failure evidence, not E2E pass.
- A later bounded direct process probe with a correctly encoded, BOM-free pair of pre-armed barrier JSON files produced binary stdout JSON with `verdict: DEFERRED-NATIVE`, `nativeEvidence: deferred-to-task-10`; it also wrote acknowledgements and held/receipt files. The presentation receipt showed a coordinator-consumed receipt but `Unknown` with missing diagnostic telemetry. This probe is not the canonical JS runner and does not upgrade the blocked runner result to a pass.
- A clean rebuild attempt deleted the old executable/PDB, then logged a real `Compiling ferryx` and `Finished dev profile`; observed new `ferryx.exe` metadata: 103,667,200 bytes, SHA-256 `8fe3ae2e1129e64ffa530710c0d2ce1cea5b30cff14997314ab3459c575c5a33`, last write UTC `2026-10-03T09:07:47.8790517Z`. The earlier artifact claimed 43,403,776 bytes and SHA `efe538bb...`; do not conflate them. Preserve the full remote clean-build log before using it as authoritative evidence.
- The three script hash comparisons were made against the six-entry table in `task-3-implementation.md` (the v3.6 frozen table). Four of six candidate hashes matched. `scripts/qa/pane-liveness.mjs`, `scripts/qa/pane-liveness.test.mjs`, and `scripts/lib/qa-scenarios/common-harness.mjs` in that table reflected an author report for v3.7, not the v3.6 script table. Candidate hashes observed: `7ae1488a9cd67555dfbc76c60e5fc873dd1c55c852c2df7567e0ae7034b111a2`, `e11d64dafd2d808e499e3143f003f8be741f3e2ea31321cc2db0a731761d5d3f`, `be5cf74ddce35fa75100ad99205a76c36e39144f42f58c7c3471e087fa299696`; expected table values: `2d4c0080e656610c5ee0ffbd04db75084ced68d726d0af7f99ec9037096e486c`, `d160a1db443106895c9668b522e41c561a8752d6ebfbe57b418ce8663bb865f5`, `1ae202c36f538c5a44ad100424e3d52a286d716a1dac759f4f3f616a8569ca08`. Config, native-driver, and diagnostic-classifier script hashes matched.

## Test contention

Two native cargo test jobs with identical `qa_barrier` command and shared Cargo target were observed:

- PID 16168, parent cargo proxy PID 9836, PowerShell parent PID 18152, start `2026-10-03 17:49:58` local display time.
- PID 10060, parent cargo proxy PID 3312, PowerShell parent PID 1384, start `2026-10-03 17:57:08` local display time.

The first process emitted `LNK1104` on shared test executable output. Neither process was killed because author ownership was not established. No subsequent test command should use this shared target until both jobs are confirmed exited. Individual native exit codes, counts, and complete raw logs remain unverified.

## Wrapper and CLI evidence

Previously archived `task-3-verifier-wrapper-validation.raw.log` and CLI files exist in the `.omo/evidence/...` directory. Exact locally measured sizes/hashes for wrapper: 534 bytes, `8c23553bc972df31e5d543da59feda08d518fc05fad92a1968995057d6daef1d`; CLI files appear in the table above. Re-read each full file before claiming its exact captured commands and exit code in the master receipt.

## Additional direct diagnostic and clean-build evidence

- Remote candidate path: `C:\\Users\\sook\\task3-product-gate-01a100ad`.
- Clean-build attempt deleted the prior `src-tauri\\target\\debug\\ferryx.exe` and `.pdb`, removed `deps\\ferryx-*.exe`, then invoked `cargo build --manifest-path Cargo.toml --bin ferryx --features local-split-qa`. Its complete captured log contains `Compiling ferryx` and `Finished ... 8.13s`; the process printed `BUILD_EXIT_CODE: 0`. The newly reported binary SHA was `8fe3ae2e1129e64ffa530710c0d2ce1cea5b30cff14997314ab3459c575c5a33`, byte length 103,667,200. A later invocation's metadata last-write UTC was `2026-10-03T09:07:47.8790517Z`. Full clean-build output must still be fetched and hashed as a raw artifact before treating the receipt as closed. The prior 43,403,776-byte / `efe538bb...` binary result predates this clean rebuild and is a different artifact.
- Existing runner result JSON (remote path listed above) recorded SHA `051d3c5d...` for the runner's binary, which differs from both executable hashes above; that result proves the runner used a different binary than the later built binary. Do not attribute its barrier result to the later build.
- The first direct binary probe pointed `FERRYX_QA_BARRIER_DIR` at a directory that did not exist; binary stderr was `FERRYX_QA_CHANNEL_UNAVAILABLE: barrier dir does not exist: ...` and the process exited without timeout. This does not explain the canonical runner's prior barrier timeout.
- A separate manual headless probe used two pre-armed `.arm.json` files written with BOM-free UTF-8 JSON, runId `manual-diagnostic-20261003-2`, operationId `manual-op`. It emitted JSON stdout `verdict: DEFERRED-NATIVE`, `nativeEvidence: deferred-to-task-10`, then wrote arm acks, fixture setup, blocked receipts, and presentation coordinator-consumed receipt. Presentation recovery receipt still had `classifierVerdict: Unknown` and missing telemetry fields. This probe only demonstrates process-level output and channel operation with manual arms; it is not a passing canonical runner acceptance.
- Most recent direct inspection of prior canonical runner `result.json` shows backend-write receipts count 2, presentation receipts count 0, presentation `releasedAt: null`, and structured `BARRIER_ACK_TIMEOUT` for presentation receipt index 0 within 2,000 ms. Its `cleanup.json` reports task-owned PID 3476 reaped (`taskkill /T`), isolation directory removed, and cleanup gate `ok: true`. Keep this as the actual runner failure. A different later run reported `RECEIPTS_TIMEOUT`, but its stored binary/result hash is `051d3c5d...` and its run cleanup deleted the isolation root; do not alter or fabricate its missing runtime data.

## Raw output limitations and exit-code cautions

- Historical QA compile log in the prior archive is 53,419 bytes, SHA-256 `723271c9b3364872e4fb0be511478bea5e114469a3d08f40a75feec396d7c2c5`; its last read ends `Finished dev profile ... 23.60s`. That text does not record the wrapper's native numeric exit code. The request for full default-pass log plus wrapper self-check has separate prior evidence, but this QA log alone is not numeric-exit proof.
- The direct `Start-Process` diagnostic probe printed an empty `$proc.ExitCode` because its wrapper sequence refreshed process after exit/termination and did not capture the exit value correctly. Do not claim a native process-object exit code from that probe.
- The three old unit-test log files have distinct SHA-256 despite identical exact size (44,130 B), confirming they are not byte copies of one another, but each must be inspected through its final bytes and checked for a terminal test-result summary. Their equal size alone proves no test result. A new attempt failed during linking with `LNK1104`, and two test jobs were observed concurrently on the same linker output path. Neither was terminated; their native return codes remain unknown until their parent shells exit and logs/exit markers are inspected.
- Root archive location for the copied raw log files is now `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/E/` with names `task-3-raw-<original-name>`. The exact local copy lengths and hashes appear in the preceding conversation/tool output. Re-measure them after writing the final manifest; no original raw content was rewritten or normalized.

## Later byte-exact retrievals and current inventory

On a later SSH retrieval, files were read with `[IO.File]::ReadAllBytes` and transported as Base64; the resulting local bytes were written directly, not text-decoded. Exact target measurements:

| E file | Bytes | SHA-256 |
|---|---:|---|
| `task-3-raw-build-clean-verify.log` | 38,990 | `afe9a9156129ee7e5f20f81eecb5b8f7e3a6bd4f85db8e628dfbc5e07b161bff` |
| `task-3-raw-runner-frozen-final.log` | 4,134 | `99877b7c54d7e1cf79e986e284a3b6079914e190516c305a26ec76dcca0d122e` |
| `task-3-raw-diagnostic-current.stdout.log` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `task-3-raw-diagnostic-current.stderr.log` | 107 | `3c04d510159a1fab7cc492b3ec0bebbd3b876763070c25b98a33f9da616bb37a` |
| `task-3-raw-manual2.stdout` | 499 | `c739d635ab1fa80c722156c2e72fb9f68ec0e22056af2d76675e788355bf7402` |
| `task-3-raw-manual2.stderr` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `task-3-raw-manual2-backend.receipt.jsonl` | 1,901 | `2edb1f0b3ae20b1704e03777d1622dfe674150e3c0ad9ae6fcc3542bd59644f9` |
| `task-3-raw-manual2-presentation.receipt.jsonl` | 1,238 | `8f9f268814d3a972b22e9f325dd9c01cf3a7b99f35511de368c7d3446196f8de` |
| `task-3-raw-canonical-run-result.json` | 2,002 | `d5cd0f9bff4710fc4e67a4af44223ada01c32c5c14bfd2c05f04893e28c8bab1` |
| `task-3-raw-canonical-run-cleanup.json` | 808 | `876fcf4ecb4bee6a1a4010315f81e52351f3fffd27fa4ccc0917f4bf1d003326` |

Earlier `.omo/evidence`-source log copies archived under `E/task-3-raw-*` were re-measured after those copies were written. Their sizes/hashes are in tool output from cell `call`; see this index's first table for the preceding set. The three historical unit files remain exactly 44,130 bytes each, with distinct hashes, and no test count has been established.

### Build-log decoding detail

`task-3-raw-build-clean-verify.log` is a byte-exact PowerShell native-command stream in UTF-16LE (38,990 bytes; 19,477 NUL bytes). It begins with PowerShell's `NativeCommandError` warning envelope and ends with the observed compiler/linker output `Compiling ferryx ... Finished dev profile ... target(s) in 8.13s`. It does not contain `BUILD_EXIT_CODE: 0`; that marker was observed in a separate earlier SSH output event, not within this saved stream. Consequently, report the compile/linker evidence and separate printed exit-marker observation, but do not call the log itself a raw numeric-exit receipt.
