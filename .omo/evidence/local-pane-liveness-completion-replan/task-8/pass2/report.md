# Task 8 pass 2 verification report

**Candidate rejected; Task8 verification evidence and teardown complete.** Frozen commit `172baa874f5e320ef08f4ed1dc5f11b391898477`, tree `a0ac605669047d15f184b395018924f38aaf71b0`, base `d82b35e43f208b53adf6310f4e3c89cbde8814f4`. No product repair or code commit.

## Platform verdicts

| Gate | Mac | Windows | Linux |
| --- | --- | --- | --- |
| UI build | Native0 | Native0 | Native0 |
| Scoped split/lifecycle |19/19 and112/112 pass |19/19 and112/112 pass |19/19 and112/112 pass |
| Exact QA runner |NOT_RUN GUI_BOUNDARY |Native1;5F/21P;26selected |Native1;5F/21P;26selected |
| Rust |10 attempts native101 |10 attempts native101 |14 attempts native101 |
| Full UI |TIMED_OUT1200s,SIGKILL;native/count unknown |Native1;199F/6317P/5skip;6521selected |Native1;1226F/5295P;6521selected |
| Failure-file base A/B |10 observed files covered |11/11 covered |99/99 covered |

Mac emitted353 file results/4341 test results/200 failures before timeout, not a final suite total. Ten supplemental candidate file runs recovered assertions without replacing the timeout. The exact stalled test is unknown; the command and owned pid69662 are captured. All34 Rust attempts failed compilation before test bodies; selected counts unknown, not zero or passed. Mac runner would perform real Darwin focus/click with fake PIDs, contrary to dispatch; no filtering or test weakening substituted.

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

## Grouped findings / reopened owners

**Backend daemon/terminal/ipc-terminal:** unresolved fs2/lock_exclusive in split_journal, missing HandoverManager imports, IpcErrorCode Display, stale protocol fixtures, service inference, pty PathBuf comparisons, shell Fn/FnOnce lifetimes, Windows cfg-excluded handover_transaction. **Native native_terminal/ipc-native_terminal:** missing active_presentation_generation/epoch and QA-barrier schedule_cancellation_receipt/try_claim/release_claim callers. All verbatim errors are in per-host compiler JSONL, mac-rust-all-gate-diagnostics.jsonl, completed-rust-all-gate-diagnostics.jsonl and raw logs; all-platform-rust-diagnostic-routing-index.json binds196 distinct diagnostic routes.

**Frontend ui/src:** presentation retention candidate3fail/base14pass on all applicable recovered comparisons; paired App candidate1fail/base8pass on Mac/Windows; new startup epoch/HMR replacement failures in App on Mac/Windows; App.remote changed missing-mock-export assertion. Linux App mixed baseline masking remains unattributed. Frontend gate also reopened for Mac full-suite timeout; no causal diagnosis claimed. Exact named blocks and ownership are in mac/linux/windows-ab-classification.jsonl, frontend-regressions.md and mac-frontend-recovery-findings.md.

**Scripts adapters:** runner digest reads wrong relative scripts/qa source path;4 fake-mock tests dispatch real Darwin driver (osascript ENOENT on Linux/Windows, unsafe GUI actions on Mac). Scripts gate reopened; scripts-findings.md contains evidence.

## Baseline attribution

Linux97 baseline files,1 candidate-caused,1 mixed/unattributed. Windows7 baseline,2 candidate-caused,2 mixed/unattributed. Mac6 baseline,2 candidate-caused,2 mixed changed assertions among10 observed files. Every observed failure file has matching-host immutable-base evidence. Mandatory paneHandleReach h-3/h-5 and pairedDaemonRollout future-capability assertions reproduce on all3 hosts. Protected Mac TerminalSearchOverlay:332/updater:141 reproduce both pass1 and pass2 and remain untouched/out of scope. File classification is not inferred solely from base exit1. Tables: baseline-classification.md; exact120 AB command verdicts: baseline-command-verdicts.md; supplemental10 Mac verdicts: candidate-recovery-verdicts.md.

## Provenance and boundaries

Root commands.jsonl contains228 records:49 preserved pass1,49 pass2 candidate,120 base A/B,10 candidate assertion recovery. pass2/commands.jsonl holds49 original candidate records. manifest.json/base-manifest.json/host-provenance.txt bind locks, Ghostty pin6a508fd5e34c7e222c052a6d00bb3891ff3feace and fresh hosts; current-frozen-source-audit.json records clean source. Windows immediate native probes7/0,186.12GB free, transferred PS1 and list-before-run/UI-build ordering are recorded. Raw logs are local and command records carry localEvidence paths. No source/cache moving-tree staging.

No product edits, code commits, GUI actions, production touches or daemon mutation. A foreign Mac helper_setup run on separate target violated requested exclusivity; untouched and disclosed in exclusive-verifier-audit.md. Task9 native scenarios and Task10 packaging remain outside this dispatch, not accepted by Task8. The Mac full-suite timeout and runner boundary skip are explicitly unproven acceptance, not incomplete reporting disguised as a pass.

## Teardown

All3 owned source/base/target/archive staging copies are removed after complete local raw-log archival, native0 receipts in cleanup.json. Mac Ghostty symlink detached first and shared clone preserved; Linux private clone removed/shared bundle preserved; Windows shared Ghostty and foreign fixture roots preserved. Two local generated source tar archives removed with explicit receipt. No spawned verifier test executables remain.

