# A10 receiver cleanup: NOT REPRODUCED; fixture failure cleanup validated

Worker st_01a098a6, Darwin arm64. No production files edited. Three materially different attempts did not reproduce the historical killed-child reap timeout. This is not a failing-first regression delivery and must not be represented as a fix or historical cleanup proof.

## Runtime evidence

All commands used A10-receiver-run.sh: private HOME/FERRYX/XDG/TMP before Cargo or library initialization, normal Cargo/Rustup homes, existing src-tauri/target, jobs=2, dev/test debug=0, incremental=0, empty RUSTC_WRAPPER. Script logs exact command, stage and exit and removes its exact private stage.

Common command: cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_receiver_cleanup -- --nocapture --test-threads=1.

- A10-receiver-RED.log: baseline TERM-ignoring continuously writing child PID14450; exit0, two tests passed. Despite filename, NOT RED.
- A10-receiver-TERM-baseline.log: default TERM handling, PID15025; exit0, two passed.
- A10-receiver-saturated-RED.log: exact kernel WouldBlock saturation barrier after dropping sole receiver, PID16251; exit0, two passed. Despite filename, NOT RED.
- A10-receiver-final.log: final saturated fixture PID19590 / session765f5cbe-5230-4dd4-a493-0da32373f98a; exit0, two passed. Production close returned Ok, Exited, reaped=true, reader_finished=true, registry absent, fixture root removed.
- A10-receiver-batch.log: same Cargo options with --test worktree_safety --test pty_input_cancellation --test pty_receiver_cleanup; exit0: 9 worktree, 3 input, 2 receiver tests. Receiver PID16477 production reaped and reader finished.
- A10-receiver-lifecycle.log: same Cargo options with --lib terminal::pty::tests; exit0, 5 passed. Existing lifecycle tests contain fixed sleeps; they were not edited or cited as deterministic receiver-drop proof.

Final focused file LSP diagnostics: none. Existing unrelated library warnings remain in logs; concurrent journal E0658 did not block these runs. No standalone daemon/desktop invocation or release/build was performed.

## Mechanism and limitation

terminal/service.rs close delegates to PtyManager close. start_lifecycle_watcher takes the session reader JoinHandle before entering its loop; join_reader_bounded subsequently finds no handle when the watcher owns it. Thus successful close does not prove a join. The watcher notices dropped output or reader_finished and closes; the reader itself stops immediately when blocking_send reports receiver loss. Handle ownership alone is not a behavioral failure: reader_finished uses Release at the end of the reader closure and the fixture reads it with Acquire. All owned runtime variants reached successful bounded close/reap; no ownership redesign is warranted. No speculative production change was made.

The final test uses a real PTY child running this integration binary. A private Unix listener is bound before spawn, child PID is captured before triggering and matched over the control channel, output receipt provides readiness, then receiver is dropped and child reports actual kernel WouldBlock. No sleeps or polling delays are used in the new test. Assertions demand close success, reaped state, reader finished, Exited state and registry absence. The original emergency duplicate/drain/waitpid path was defective (unbounded blocking work, initial WouldBlock exit, and a competing reaper) and has been removed. Both scenarios now unconditionally await production close outside the caught scenario future, with a ten-second watchdog. There is no fixture-created worker to detach or join and no second reaper. The injected test panics with a typed sentinel after the exact receiver-drop/kernel-saturation barrier. Only that sentinel is accepted, and only after the same close/reap/reader/registry/root assertions used by ordinary success. Unexpected panics resume after cleanup. This proves cleanup for the actual injected scenario failure, not for hypothetical future production close defects.

No identity-specific receipt exists for historical session5455c4a5-6328-44bf-aa9b-a446ebbd3217. No broad process absence claim or broad kill was made. All new recorded PIDs have production reaping receipts.

## Files / hashes

Only new src-tauri/tests/pty_receiver_cleanup.rs and A10-receiver-* evidence were written. Everything remains uncommitted.

Final SHA256:
- src-tauri/tests/pty_receiver_cleanup.rs: e37207240deb5ffa7ad3768b3e35d01db5cc98da71aee76ed2b0c298823da278
- read-only src-tauri/src/terminal/pty.rs: 037d147ab61effd5cad67821f1670a447210b245ddb3618cc11dc4a3d30dc522
- read-only src-tauri/src/terminal/session.rs: 78652d1e883749c5360dc6a655700ef59b9d3a5612562f96ce0d413de5ae8a2b

Project AGENTS instructions and current diffs were read before writes. CORRECTION: the previous claim that applicable skills were unavailable was wrong. This follow-up read programming/SKILL.md, programming/references/rust/README.md, rust/async-tokio.md, rust/unsafe-discipline.md, rust-ub/README.md and both rust-ub reference files, debugging/SKILL.md and debugging/references/runtimes/rust.md at /Users/indo/.bun/install/global/node_modules/omo-ai/plugin/skills/. Initial normal git status failed on the existing ghostty symlink; status/diff with ignore-submodules=all succeeded without changing it.

## Follow-up validation receipts

- A10-receiver-injected-cleanup.log: focused command above, exit0, 3 passed. Ordinary PID25051 and injected PID25055 both production reaped, reader finished, roots removed. Typed injected failure reached and accepted only after cleanup.
- A10-receiver-cleanup-final-batch.log: cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_receiver_cleanup --test pty_input_cancellation --test worktree_safety -- --nocapture --test-threads=1; exit0, 15 passed (3 receiver including helper, 3 input, 9 worktree). Final ordinary PID30785/sessionfadb1ca3-77b6-4bf4-9e8a-b42f86c1fdb3 and injected PID30786/session00ed7c3f-0ce5-4f2c-8e21-d4bde9225c53 both production reaped with reader_finished=true and roots removed.
- Final file rustfmt completed; LSP diagnostics returned none. Production hashes unchanged.
- A10-receiver-miri-preflight.log: cargo +nightly miri --version, exit0. A10-receiver-miri.log: cargo +nightly miri test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test pty_receiver_cleanup -- --nocapture --test-threads=1, exit1 at Tauri startup: unsupported operation _NSGetExecutablePath with isolation enabled. No Miri or sanitizer pass is claimed. Real native runtime tests above passed.

Adjudication closes as NOT REPRODUCED, not a production repair. Historical missing-PID receipt remains explicitly unresolved.
