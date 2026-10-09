# Task 3 remote gate evidence package

Date: 2026-10-03
Host: `maho-win` (`sook@100.126.171.58`)
Candidate root: `C:\Users\sook\task3-product-gate-01a100ad`
Evidence root: `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/E/`

## Summary and status

This receipt records evidence and known failures. It does **not** claim the whole Task 3 gate passed. Remote Rust compilation of the `local-split-qa` target and binary build were separately reported exit 0 in PowerShell output, and the invalid CLI invocations previously reported exit 2. The canonical runner result is blocked on the presentation receipt; the later direct process probe is not canonical runner E2E. The three unit-test attempts cannot currently be given reliable individual exit codes/counts because two same-command cargo jobs overlapped on the shared Cargo target and the first observed linker failure was `LNK1104`.

Do not infer a test pass from earlier prose, identical test-log sizes, a named raw file, or a built binary existing. Do not kill either competing remote process without verified ownership. See `task-3-remote-gate-raw-log-provenance.md` for exact artifact locations, hashes, mismatches, and limitations.

## Prompt-to-artifact checklist

| Requirement | Evidence | Status |
|---|---|---|
| Cargo executable path/version and native failure exit | Prior raw wrapper/cargo audit under `.omo/evidence/local-pane-liveness-root-remediation/`; cargo resolved `C:\Users\sook\.cargo\bin\cargo.exe`, Cargo/rustc v1.97.0, failure process exit 101. | Verified in prior session evidence; exact raw audit still needs archival here. |
| Wrapper controls exit 7 and exit 0 | `task-3-raw-task-3-verifier-wrapper-validation.raw.log` | Raw copy archived; inspect full bytes before citing individual line values. |
| Full default feature `cargo check --all-targets` log | `task-3-raw-task-3-cargo-check-default-v3.log` | Raw log copied, 52,819 bytes; historic runner envelope includes NativeCommandError text. Numeric wrapper exit is not present in the log. |
| Six v3.6 script hashes | Candidate hash output described in provenance index; versioned v3.6 receipt directories under `.omo/evidence/.../task3-windows-v36/`, `task3-windows-v36-rerun/`, and `task3-mac-v36/` | The six-table comparison in `task-3-implementation.md` is not reconciled with versioned v3.6 receipts. Do not conclude a v3.6 mismatch until those receipt artifacts are compared; candidate-run provenance remains unresolved. |
| QA-feature `cargo check --all-targets` | `task-3-raw-task-3-check-qa-frozen.log` | Full raw log archived; ends `Finished dev profile ... 23.60s`; exact wrapper/native exit not in log. |
| Three nonzero `qa_barrier` unit test filters | `task-3-raw-task-3-test-core-frozen.log`, `...writer...`, `...pres...` | Not passed/verified. All three archived log endings contain linker `LNK1104`; no `test result:` appears. Two overlapping same-command cargo processes use the same target. Individual terminal counts/exit markers unavailable. |
| Fresh binary build after removing prior exe/PDB | `task-3-raw-build-clean-verify.log` and separate remote metadata output described in provenance index | Command deleted old exe/PDB and dependency exe; byte-exact UTF-16LE raw build output (38,990 bytes) ends `Compiling ferryx` and `Finished dev profile ... 8.13s`. Reported binary SHA `8fe3ae2e1129e64ffa530710c0d2ce1cea5b30cff14997314ab3459c575c5a33`, size 103,667,200 bytes. Raw build log does not contain the separate `BUILD_EXIT_CODE: 0` console marker. |
| Invalid CLI argv exits 2 | `task-3-raw-task-3-cli-frozen-err1.log`, `...err2.log` | Earlier run reported each exit 2; runner used a different binary SHA than later clean build. These negative cases do not prove runner E2E. |
| Canonical headless classifier runner | `task-3-raw-task-3-runner-frozen-final.log` plus remote `result.json` / `cleanup.json` described in provenance index | **BLOCKED**, `BARRIER_ACK_TIMEOUT` at presentation receipt 0 (2s); actual runner evidence identifies binary SHA `051d3c5d...`, not the later clean build. Not a pass. |
| Actual binary stdout/stderr and barrier behavior | Remote direct-process probes described in provenance index | First probe's barrier directory absent; separate manually pre-armed BOM-free arms emitted `DEFERRED-NATIVE` and receipts, but not a canonical runner pass. |
| Full raw logs archived in wave1 E | `E/task-3-raw-*` files | Archived by byte-preserving copy from the actual prior archive, with exact sizes/hashes recorded in prior tool output and index. |

## Archived files

All listed logs are byte-preserving copies of files previously present at `/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-w1/.omo/evidence/local-pane-liveness-root-remediation/`:

- `task-3-raw-task-3-cargo-check-default-v3.log`
- `task-3-raw-task-3-check-qa-frozen.log`
- `task-3-raw-task-3-test-core-frozen.log`
- `task-3-raw-task-3-test-writer-frozen.log`
- `task-3-raw-task-3-test-pres-frozen.log`
- `task-3-raw-task-3-build-bin-frozen.log`
- `task-3-raw-task-3-cli-frozen-err1.log`
- `task-3-raw-task-3-cli-frozen-err2.log`
- `task-3-raw-task-3-runner-frozen-final.log`
- `task-3-raw-task-3-verifier-wrapper-validation.raw.log`
- `task-3-raw-build-clean-verify.log`
- `task-3-raw-runner-frozen-final.log`
- `task-3-raw-canonical-run-result.json`
- `task-3-raw-canonical-run-cleanup.json`
- `task-3-raw-diagnostic-current.stdout.log`
- `task-3-raw-diagnostic-current.stderr.log`
- `task-3-raw-manual2.stdout`
- `task-3-raw-manual2.stderr`
- `task-3-raw-manual2-backend.receipt.jsonl`
- `task-3-raw-manual2-presentation.receipt.jsonl`

The contents were copied without parsing, newline changes, or summary substitution. Their actual sizes and SHA-256 values are in the provenance index and tool transcript. The root `E/` path was initially missing these files; this receipt corrects that location discrepancy. The clean-build raw log, later canonical runner capture, remote result/cleanup JSON, and manual probe files were retrieved as Base64 from remote `[IO.File]::ReadAllBytes` and archived as exact byte sequences.

## Test status disclosure

An attempt at `cargo test --manifest-path Cargo.toml --lib --features local-split-qa -- --nocapture --test-threads=1 qa_barrier` found two concurrent process trees on the same candidate and target output, PIDs 16168 and 10060, each with an identical command line and separate PowerShell parents. The first linker diagnostic observed was `LNK1104` opening `ferryx_lib-39a96428895251c2.exe`. The three old archived logs' terminal sections independently show `LNK1104` and no test-result summary. No reliable per-filter test counts or individual native exits are established; one attempted `core` raw log was zero bytes when checked. The other filters have no fresh, ownership-clean execution. No affirmative unit-test completion is asserted.

## Remaining verification boundary

This package documents an unsuccessful/incomplete runtime gate. A new test run requires both observed remote cargo trees to exit and an exclusive target directory (or other explicit non-overlap proof). A successful canonical runner run requires matching frozen script hashes and a fresh binary tied to that candidate, then actual required receipts and cleanup evidence. No desktop GUI or production daemon was used.
