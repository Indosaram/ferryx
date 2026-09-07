# Rendering verification baseline - 2026-09-06

All four required invocations passed on the existing dirty working tree. Rust executed 111 tests; UI executed 157 tests across three files. This is a test/typecheck/build baseline, **not native pixel, display-switch, or desktop rendering success**.

## Capture identity and scope

- Task: `st_01a07714`; working directory: `/Users/indo/code/project/orca-lite`.
- Required commands ran once each, between `2026-09-06T14:19:48Z` and `2026-09-06T14:20:22Z` (local UTC+09:00: 23:19:48-23:20:22).
- Host: Darwin 25.6.0, arm64; Bun 1.4.0; cargo/rustc 1.92.0. Exact versions: `environment.txt` below.
- Existing Cargo cache reused at `src-tauri/target`; `CARGO_TARGET_DIR` and `RUSTFLAGS` unset. No clean, dependency installation, desktop launch, release-profile Cargo build, app restart, or user-data operation was issued.
- Authored only this report and baseline evidence files. The explicitly requested commands also generated their normal Cargo cache and `ui/dist` build output; source/tests/configuration were not edited. Existing atlas, row-cache, vendored WGPU, onboarding, and other foreign dirty work was left in place.
- Root, Rust, native-terminal, UI, and component instructions and relevant manifests/configuration were read. Programming and applicable debugging setup/runtime references were consulted. A dedicated `bun-1-4` skill was not available in the inspected skill roots; actual Bun 1.4.0 and the repository's Vitest script were used, without substituting `bun test`.

## Artifact root

All artifact filenames in this report are relative to:

```text
/Users/indo/code/project/orca-lite/.omo/ulw-loop/rendering-review-20260906/baseline/
```

This directory was already started when the lead supplied the newer criterion directory `.omo/evidence/ulw/rendering-review-20260906/G001-review-and-resolve-ferryx-intermitte/a1`; existing artifacts were retained rather than moved or duplicated.

## Exact invocations and observed results

Each invocation ran from the working directory above, through shell `eval` in an owned `monitor` wrapper. Rust tests preceded Cargo check; UI tests preceded UI build. The two lanes ran concurrently. Monitoring used child-process completion via `wait`, not polling or sleeps; the enclosing execution had a 1,800-second timeout and completed normally. A dedicated harness `monitor`/`eval` tool was not exposed to this child, so this was a shell implementation.

| ID | Exact invocation | Exit | Observed pass/fail counts | UTC start-end | Raw output |
| --- | --- | ---: | --- | --- | --- |
| `rust-tests` | `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal` | 0 | 111 passed, 0 failed, 0 ignored, 0 measured, 489 filtered out | 14:19:48-14:19:58 | `rust-tests.log` |
| `ui-tests` | `bun run --cwd ui test -- src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/lib/nativeTerminalLifecycle.test.ts` | 0 | 157 passed, 0 failed; 3 passed files, 0 failed files | 14:19:48-14:20:11 | `ui-tests.log` |
| `cargo-check` | `cargo check --manifest-path src-tauri/Cargo.toml` | 0 | 1 successful check, 0 failed checks; test counts not applicable | 14:19:59-14:20:03 | `cargo-check.log` |
| `ui-build` | `bun run --cwd ui build` | 0 | 1 successful build, 0 failed builds; test counts not applicable | 14:20:12-14:20:22 | `ui-build.log` |

Per-command evidence: `<ID>.invocation.txt`, `<ID>.exit-code.txt`, `<ID>.monitor.txt`, `<ID>.before.state.txt`, `<ID>.after.state.txt`, and corresponding `.diff`, `.status.txt`, and `.untracked.sha256` snapshots. No required command was retried.

Actual result excerpts (ANSI styling omitted):

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 6.95s
Running unittests src/lib.rs (src-tauri/target/debug/deps/ferryx_lib-f277dedbac8184e5)
test result: ok. 111 passed; 0 failed; 0 ignored; 0 measured; 489 filtered out; finished in 1.36s

src/components/NativeTerminalPane.test.tsx (139 tests) 3731ms
src/components/NativeTerminalPane.lifecycle.test.tsx (11 tests) 508ms
src/lib/nativeTerminalLifecycle.test.ts (7 tests) 12ms
Test Files  3 passed (3)
Tests  157 passed (157)
Duration  21.02s

Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.16s

$ tsc && vite build
vite v6.4.3 building for production...
1864 modules transformed.
built in 2.56s
```

The UI build's Vite production bundle is the explicitly requested frontend build, not a desktop release build. Generated bundle: `/Users/indo/code/project/orca-lite/ui/dist/index.html`; generated Rust test executable is the path in the excerpt. Full bundle filenames and sizes are retained in `ui-build.log`.

## Actual errors and warnings, without repairs or suppression

No required invocation failed. Both Rust commands reported `wgpu-hal` generating **50 warnings** and `ferryx` generating **7 warnings** per command. These were retained, not suppressed. Representative actual output:

```text
warning: unexpected `cfg` condition value: `cargo-clippy`
   --> vendor/wgpu-hal/src/metal/adapter.rs:540:17
warning: `wgpu-hal` (lib) generated 50 warnings
warning: unused variable: `super_key`
   --> src/native_terminal/input.rs:366:13
warning: unused import: `Manager`
  --> src/ipc/notifications.rs:21:24
warning: variable does not need to be mutable
   --> src/native_terminal/renderer/font_manager.rs:155:13
```

`super_key` appeared in the test compilation; `Manager` appeared in Cargo check. Other Ferryx warnings in both logs concern unread `app`, unused `wait_and_reap`, unconstructed `WriterLeaseGuard`, unused `new`/`canonical_path`/`owner_id`, and unused `acquire_writer_lease`. Locations and full diagnostics are in the raw logs and `result-summary-and-cleanup.txt`.

The passing UI suite emitted the following error twice from its deliberate mount-attach rejection scenario:

```text
Native terminal IPC command failed {
  command: 'cmd_native_terminal_attach',
  error: Error: mount attach failed
      at .../ui/src/components/NativeTerminalPane.test.tsx:4689:19
}
```

This is an exercised error path, not a failed test: the test at lines 4679-4725 uses fake timers, throws on the first two attach attempts, and asserts success on the third. Full stacks remain in `ui-tests.log`. UI build emitted no error or warning output.

An initial monitor setup attempt failed in both lanes with `/bin/bash: line 16: BASHPID: unbound variable` before launching any required invocation. macOS Bash 3 does not expose that variable. Only the inline monitoring wrapper was corrected; the four commands above subsequently launched once each. The first wrapper's unconditional completion message was invalid and is not used as evidence. Receipt: `initial-monitor-failure.txt`; preliminary `baseline.before.*`/`baseline.after.*` describe that setup attempt, while `run.before.*`/`run.after.*` describe the actual execution.

## LSP diagnostics before builds

All calls used `functions.lsp_diagnostics` with `severity: "all"`, before the four required invocations. These results are limited to the scanned files, not a whole-workspace clean bill of health:

| `filePath` (relative to working directory; absolute paths supplied) | Actual tool result |
| --- | --- |
| `src-tauri/src/native_terminal` | `.rs`, 50 files scanned (tool cap); 0 files with errors; 40 diagnostics, all inactive-code/unlinked-file hints for platform-gated code |
| `ui/src` | `.ts`, 50 files scanned (tool cap); 0 files with errors; 0 diagnostics |
| `ui/src/components/NativeTerminalPane.tsx` | Hint 6385 at 180:19: `'keyCode' is deprecated.` |
| `ui/src/components/NativeTerminalPane.test.tsx` | Five hint 6385 diagnostics: `'platform' is deprecated.` at 2498:39, 3279:39, 3501:39, 3568:39, 5751:39 |
| `ui/src/components/NativeTerminalPane.lifecycle.test.tsx` | `No diagnostics found` |
| `ui/src/lib/nativeTerminalLifecycle.test.ts` | `No diagnostics found` |
| `ui/src/lib/nativeTerminalLifecycle.ts` | `No diagnostics found` |

The later Cargo warnings above demonstrate why zero LSP errors must not be described as warning-free compilation. No diagnostics settings were changed.

## Tree identity and dirty-work preservation

`run.before.state.txt` (14:19:47Z), `run.after.state.txt` (14:20:22Z), and every required command's before/after snapshot agree:

| Identity | Before and after value |
| --- | --- |
| HEAD | `b8f82d707f0cb99907e3d79c0c9cdc75053ef931` |
| HEAD tree | `7c1bb89729859ef59cb928fb99008f31174338f5` |
| SHA-256 of working diff against HEAD | `785c7b5e0838edc451b9a247e4bad8ffc38f9de0acdcc99ff4876dc3db144153` |
| SHA-256 of untracked-content hash manifest | `9ad8c693a611aa82d9fd368c9c5b2efbb560b5a79116f948f7fda4aa8b6b3903` |

Identity capture commands:

```sh
git rev-parse HEAD
git rev-parse 'HEAD^{tree}'
git diff --no-ext-diff --binary HEAD > "$BASE/$name.diff"
shasum -a 256 "$BASE/$name.diff"
git status --short > "$BASE/$name.status.txt"
git ls-files --others --exclude-standard -z -- . ':!.omo' ':!docs/rendering-review-2026-09-06' | xargs -0 shasum -a 256 > "$BASE/$name.untracked.sha256"
shasum -a 256 "$BASE/$name.untracked.sha256"
```

The untracked hash intentionally excludes evidence/review directories and Git-ignored files, including generated build caches. It includes foreign untracked source such as vendored WGPU and onboarding. Tracked diff hashing includes all tracked modifications/deletions against HEAD, not untracked content. Before/after status files are identical. These bounded observations establish the captured baseline's identity, not immunity to later concurrent edits or transient edits between snapshots. This report remains uncommitted in a shared working tree.

## Owned-process cleanup receipt

- Full live-process inventory was inspected before execution; retained inventories: `processes-before.txt`, `processes-after.txt`.
- Rust/UI lane PIDs `91304`/`91305` were waited and reaped with exit 0 (`lanes.monitor.txt`). Required command wrapper PIDs: Rust tests `91400`, UI tests `91404`, Cargo check `91652`, UI build `91890`; all were waited and reaped with the recorded command exits.
- Final `ps -p 91304,91305,91400,91404,91652,91890 -o pid,ppid,state,command` returned only the header and exit 1: none of these owned processes remained. Saved in `result-summary-and-cleanup.txt`.
- Existing foreign dev launcher `36614`, Bun/Tauri processes `36617`/`36620`, frontend `36742`, daemon `37221`, and desktop `60274` were still present in the final receipt. No foreign process was killed or restarted; no signals were sent by this task.
- An initial broad instruction search timed out after 30 seconds before the baseline runner; no required command was launched by that search. No debugger, browser, desktop app, watcher, or persistent server was started by this task. Evidence artifacts and ordinary requested build outputs were retained, not cleaned away.
