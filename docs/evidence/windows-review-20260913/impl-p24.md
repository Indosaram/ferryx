# P24 watcher recovery - execution handoff

Owner: st_01a09a0a. Status: regression staging complete; implementation and RED/GREEN blocked on mandatory lead-issued exclusive Darwin Cargo slot. This is not a completion claim for P24 or the aggregate goal.

## Scope and mechanism

Read root/src-tauri directory instructions, programming Rust skill, repair-packets.md, gap-packet-addendum.md, dag-remaining-register.md, gap-backend.md and gap-verification.md; inspected watcher, journal DTOs, IPC registration/hydration caller and current git diff. watcher.rs was clean at entry; foreign dirty files were left untouched.

GB-02: Windows notify may silently unwatch a deleted watched DAG directory. Successful initial arming permanently disables the current polling branch; a recreated directory cannot emit updates once the final delete-triggered scan completes. Recovery must not recursively watch a project tree.

GB-05: async loop executes directory resolution, metadata, reading, parsing and native watcher creation/arming inline. IPC inventory offloading does not offload the separately spawned loop.

## Registration

Official executeAgentToolkit called with resolveCwd=/Users/indo/code/project/orca-lite and resolveSessionId=01a0983f-c995-753d-afa9-593f6d118788. steer/revise_criterion C002 returned ok=true, preserving the entire prior scenario and appending exact P24 test/binary conditions and addendum digest. Full request/response: p24/registration.json.

## Staged source and tests

Only src-tauri/src/dag/watcher.rs changed. Added per-watcher observation hooks, an explicit channel modeling Windows backend silent native-handle loss, and two embedded regressions:

- test_dag_watcher_recovers_after_silent_watch_loss: waits for actual armed/scan completion, deletes entire owned DAG directory, drops native watch, awaits final empty scan, recreates journal and requires Running then Completed snapshots without notifications/restart. Task is cancelled and awaited before asserting the expected recovery result.
- test_dag_scan_runs_off_async_worker: current-thread Tokio runtime invokes the real production scan; per-loop scan hook records worker identity and waits for an external release; controller records async sentinel before release with bounded failure deadline and releases/joins even on intended RED. Requires scan thread != async thread.

The original immutable polling behavior and inline scan are deliberately retained until intended RED is observed. No production repair is claimed. Existing tests still require explicit cancellation/await cleanup improvement in the repair phase; an early setup assertion in the new recovery test also needs a cancellation guard so failure cleanup is unconditional.

Staged source SHA256 after rustfmt: f396b69fb2e29eaa8fefb568bfa1664522cc32255184783c6c2f5f93ef9601fe. Staged diff: p24/staged-regression.diff.

## Actual validation receipts

- lsp_diagnostics(src-tauri/src/dag/watcher.rs, all): No diagnostics found before rustfmt.
- bash -n p24/run-cargo.sh: exit 0.
- Initial rustfmt --check: exit 1 for formatting only, output p24/rustfmt-check.log; rustfmt --edition 2021 then executed successfully.
- git diff --check -- src-tauri/src/dag/watcher.rs: exit 0 after formatting.
- Cargo tests/build: NOT EXECUTED. No lead slot grant was present. This is not behavioral RED, GREEN, or compile verification.
- Native Windows/GUI execution: NOT EXECUTED. No native host mutated.
- Cleanup: no Cargo/build/GUI/daemon/test processes launched; only tool inspection processes and owned report files. No branch/worktree, commit/push, install or global environment mutation.

## Exact next action required from lead

Relay an exclusive shared Darwin Cargo slot grant to st_01a09a0a (or create p24-slot-grant.txt naming st_01a09a0a), then resume this task. Owned safe runner is p24/run-cargo.sh and refuses execution without receipt:

```sh
bash docs/evidence/windows-review-20260913/p24/run-cargo.sh red
```

It executes only:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib dag::watcher::tests:: -- --nocapture --test-threads=1
```

It retains command, compiler/Cargo versions, source/lock hashes, exact test executable hash when emitted, full output and exit code. Nonzero selected test counts and the intended assertions must be checked; unrelated compilation failure is not RED. After intended RED, offload scan/arming through blocking workers, retain bounded async snapshot delivery/dedup, enable periodic reconciliation regardless of initial native arm success, and complete unconditional task cleanup; rerun identical runner with green. No requirement to rearm is needed for correctness if periodic reconciliation remains authoritative, but native error observation should request prompt reconciliation.

Native handoff to st_01a099f8 is recorded in p24-cargo-slot.md: same focused tests on Windows plus actual owned NTFS DAG-directory deletion/recreation and subsequent update visible in debug bun tauri dev without restart, with slow scan async-sentinel evidence, source/binary provenance and owned cleanup. Do not mutate installed app or user daemons.

All work is uncommitted in the shared tree.
