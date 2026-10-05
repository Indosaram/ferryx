# Task 8 pass 2 verification report — incomplete Mac evidence

Candidate **REJECTED**, not repaired. Frozen source: `172baa874f5e320ef08f4ed1dc5f11b391898477`, tree `a0ac605669047d15f184b395018924f38aaf71b0`, base `d82b35e43f208b53adf6310f4e3c89cbde8814f4`. Current source audit is clean. This is a Task8 evidence/reporting deliverable; Task9 native scenarios and Task10 packaging were not dispatched.

## Platform summary

| Gate | Mac | Windows | Linux |
| --- | --- | --- | --- |
| UI build | Native0 | Native0 | Native0 |
| Scoped split | 19/19 pass | 19/19 pass | 19/19 pass |
| Scoped lifecycle | 112/112 pass | 112/112 pass | 112/112 pass |
| Exact runner | NOT_RUN GUI_BOUNDARY | Native1;5F/21P,26selected | Native1;5F/21P,26selected |
| Rust attempts | 10 attempts native101; archived | 10 attempts native101 | 14 attempts native101 |
| Full UI | TIMED_OUT1200s;SIGKILL;native/count unknown | Native1;199F/6317P/5skip,6521selected | Native1;1226F/5295P,6521selected |
| Failed-file A/B | 10/10 observed files; candidate recovery finishing | 11/11 files | 99/99 files |
| Owned teardown | Pending | Completed native0 | Completed native0 |

All Rust list counts are UNKNOWN_COMPILE_FAILED. No zero-selection or test-body pass is inferred from compilation failure. Windows probes captured7 then0 immediately and186.12GB free before staging; native exits come from transferred PS1 files. The Mac exact runner would call real Darwin focus/click actions with fake PIDs, so its boundary skip is not a pass.

## Exact completed-platform verdicts

| Exact command | Host | Native exit | Selected | Verdict | Asserted line / reason |
| --- | --- | --- | --- | --- | --- |
| `bun run --cwd ui build` | linux | 0 | UNKNOWN / N/A | RAN_PASSED | ✓ built in 5.14s |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | linux | 0 | 19 | RAN_PASSED |  Test Files  1 passed (1);       Tests  19 passed (19) |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | linux | 0 | 112 | RAN_PASSED |  Test Files  4 passed (4);       Tests  112 passed (112) |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | linux | 1 | 26 | RAN_FAILED |       Tests  5 failed / 21 passed (26); error: script "test" exited with code 1 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 74 previous errors; 49 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 74 previous errors; 49 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error[E0599]: no method named `active_presentation_epoch` found for reference `&NativeTerminalSurfaceHost` in the current scope; error: could not compile `ferryx` (lib) due to 10 previous errors; 24 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error[E0599]: no method named `active_presentation_epoch` found for reference `&NativeTerminalSurfaceHost` in the current scope; error: could not compile `ferryx` (lib) due to 10 previous errors; 24 warnings emitted |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `bun run --cwd ui test` | linux | 1 | 6521 | RAN_FAILED |       Tests  1226 failed / 5295 passed (6521); error: script "test" exited with code 1 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib suspension -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib suspension -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib split_journal -- --list` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib split_journal -- --nocapture --test-threads=1` | linux | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 70 previous errors; 47 warnings emitted |
| `bun run --cwd ui build` | windows | 0 | UNKNOWN / N/A | RAN_PASSED | ✓ built in 5.27s; NATIVE_EXIT=0 |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | windows | 0 | 19 | RAN_PASSED |       Tests  19 passed (19); NATIVE_EXIT=0 |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | windows | 0 | 112 | RAN_PASSED |       Tests  112 passed (112); NATIVE_EXIT=0 |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | windows | 1 | 26 | RAN_FAILED | error: script "test" exited with code 1; NATIVE_EXIT=1 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 29 previous errors; 44 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 29 previous errors; 44 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib) due to 13 previous errors; 14 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib) due to 13 previous errors; 14 warnings emitted; NATIVE_EXIT=101 |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | windows | 101 | UNKNOWN / N/A | RAN_FAILED | error: could not compile `ferryx` (lib test) due to 25 previous errors; 42 warnings emitted; NATIVE_EXIT=101 |
| `bun run --cwd ui test` | windows | 1 | 6521 | RAN_FAILED | error: script "test" exited with code 1; NATIVE_EXIT=1 |
| `bun run --cwd ui build` | mac | 0 | UNKNOWN / N/A | RAN_PASSED | ✓ built in 55.79s |
| `bun run --cwd ui test src/lib/localSplitLifecycle.test.ts` | mac | 0 | 19 | RAN_PASSED |  Test Files  1 passed (1);       Tests  19 passed (19) |
| `bun run --cwd ui test src/lib/sessionPersistence.test.ts src/lib/sessionLifecycle.test.ts src/lib/nativeTerminalLifecycle.test.ts src/components/NativeTerminalPane.lifecycle.test.tsx` | mac | 0 | 112 | RAN_PASSED |  Test Files  4 passed (4);       Tests  112 passed (112) |
| `bun run --cwd ui test --config ../scripts/qa/pane-liveness-vitest.config.mjs` | mac | NOT CAPTURED | UNKNOWN / N/A | NOT_RUN | Exact test suite calls real Darwin focus/click driver using fake PIDs; dispatch prohibits GUI actions |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --list` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib local_split_reliability_ -- --nocapture --test-threads=1` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --list` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib pane_liveness_ -- --nocapture --test-threads=1` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --list` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 76 previous errors; 54 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib --features local-split-qa qa_barrier -- --nocapture --test-threads=1` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 76 previous errors; 54 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --list` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error[E0599]: no method named `active_presentation_epoch` found for reference `&NativeTerminalSurfaceHost` in the current scope; error: could not compile `ferryx` (lib) due to 10 previous errors; 32 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --test daemon_handover_transfer_contract -- --nocapture --test-threads=1` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error[E0599]: no method named `active_presentation_epoch` found for reference `&NativeTerminalSurfaceHost` in the current scope; error: could not compile `ferryx` (lib) due to 10 previous errors; 32 warnings emitted |
| `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1` | mac | 101 | UNKNOWN / N/A | RAN_FAILED | error: implementation of `FnOnce` is not general enough; error: could not compile `ferryx` (lib test) due to 72 previous errors; 52 warnings emitted |
| `bun run --cwd ui test` | mac | NOT CAPTURED | UNKNOWN / N/A | RAN_FAILED |   error: {;   error: { |

## Findings and repair owners

Backend daemon/terminal/ipc-terminal: split_journal fs2 and lock_exclusive; HandoverManager imports; IpcErrorCode Display; stale protocol and IPC fixture constructors/patterns; service inference; pty PathBuf comparison; shell Fn/FnOnce lifetimes; Windows cfg-excluded handover_transaction references. See per-host compiler-findings.jsonl, completed-rust-all-gate-diagnostics.jsonl and completed-rust-diagnostic-routing-index.json. Each original raw log remains authoritative.

Native native_terminal/ipc-native_terminal: missing active_presentation_generation/active_presentation_epoch; QA-feature callers schedule_cancellation_receipt/try_claim/release_claim missing from QaBarrierChannel. Owning lanes reopened; verifier made no repairs.

Frontend ui/src: NativeTerminalPane.presentation base14/14pass vs candidate3fail on Linux and Windows; Windows paired App base8pass vs candidate unexpected split-call argument; Windows App startup/HMR new failures; changed App.remote mock-export assertion; Linux App HMR attribution masked by baseline storage errors. Mixed files remain explicitly unattributed where exact causal proof is unavailable. See frontend-regressions.md and platform AB classification JSONL.

Scripts adapters: H6 relative digest path ENOENT plus4 fake-mock tests invoking real Darwin driver. Linux/Windows26selected demonstrates repaired config coverage but not passing behavior. Mac exact gate not run under no-GUI dispatch. See scripts-findings.md.

## Baseline attribution

Linux97 files reproduce baseline failures,1 candidate-caused,1 mixed/unattributed. Windows7 baseline,2 candidate-caused,2 mixed/unattributed. Every failed file has a matching-host immutable-base run. Mandatory paneHandleReach h-3/h-5 and pairedDaemonRollout future-capability failures reproduce exact assertions on both platforms. Protected Mac TerminalSearchOverlay:332 and updater:141 remain out of scope under pass1 A/B evidence; Mac pass2 base comparisons reproduce both protected failures; candidate assertions are being archived. See baseline-classification.md and mandatory-ab-classification.md.

## Evidence and boundaries

manifest.json, base-manifest.json, host-provenance.txt and current-frozen-source-audit.json bind hashes and clean source. Root commands.jsonl preserves49 pass1 records and currently159 pass2 candidate/base records without duplicate keys. Linux/Windows raw final and base logs are archived locally. cleanup.json contains native0 receipts for fixtures and staging; Mac cleanup and local archive removal remain pending.

No product edits, commits, GUI/production actions or daemon mutation by this verifier. A foreign Mac helper_setup command was observed on a separate target and left untouched; exclusivity finding is in exclusive-verifier-audit.md. This report is not final: Mac full-suite attempted and timed out; final candidate logs are archived. Base A/B for10 observed failed files is active, followed serially by candidate per-file assertion recovery. These supplemental checks do not replace the timed-out full-suite verdict. Attribution and cleanup remain required.



