# Lead verification receipts

These commands were executed independently by the lead after reading the SSH worker report. They do not establish native desktop rendering or screenshot appearance.

## SSH focused suite

```sh
bun run --cwd ui test src/components/RemoteDirectoryPicker.test.tsx src/components/ProjectDialogs.test.tsx src/lib/remoteProject.test.ts
```

Monitor: `mon_WAK2K30Q70WY1FGE`, session `bash_3`.

```text
status: completed exit_code: 0
 Test Files  3 passed (3)
      Tests  61 passed (61)
```

## Windows and Linux live backend

```sh
FERRYX_SSH_BROWSE_HOST=maho-win cargo test --manifest-path src-tauri/Cargo.toml --test ssh_browse_live -- --ignored --nocapture
FERRYX_SSH_BROWSE_HOST=omarchy cargo test --manifest-path src-tauri/Cargo.toml --test ssh_browse_live -- --ignored --nocapture
```

The two commands were chained with `&&`. Monitor: `mon_4ST7ACPDCWS0PKMD`, session `bash_4`.

```text
status: completed exit_code: 0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 12.18s
SSH_BROWSE_HOME_CHILD_PARENT_REGISTRATION_OK
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 7.18s
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.81s
SSH_BROWSE_HOME_CHILD_PARENT_REGISTRATION_OK
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
```

The lead read `src-tauri/tests/ssh_browse_live.rs` before execution. It calls the actual backend directory listing and registration functions, checks nonempty home/children, selected child registration, parent navigation and `~` home resolution. It creates a temporary local host store and drops that fixture after validating the registered identity. It does not write the remote filesystem or restart a daemon.

Cleanup: both monitor commands exited with code 0; the Rust test drops its temporary store. No persistent server or browser was started by these lead checks.

## Diagnostics and screenshot boundary

`lsp_diagnostics` on `ui/src/components/RemoteDirectoryPicker.tsx` returned:

```text
LSP daemon unreachable: LSP daemon did not become reachable at /Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock.
```

This is an unavailable diagnostic service, not a clean LSP result. The combined compiler/build check remains a separate integration requirement.

The lead opened all nine SSH screenshot paths through `read` and `display`, but the current lead model rejected image attachments with `model does not support images`. No personal visual verdict is claimed. The independent Flash verifier must inspect the actual screenshots; machine-readable DOM/action results remain distinct evidence.

## Commit ownership

Original source-session patch records establish:

- Sidebar request: `Sidebar.tsx` expansion-condition hunks and `Sidebar.test.tsx`. The `projectGrouping` rewrite and shortcut attributes belong to other work.
- Terminal request: full-height constants/styles, top/bottom backing-strip removal, attention decoration and related geometry/reach tests plus `docs/DESIGN.md`. New error-overlay changes and lifecycle error assertions belong to another session.
- SSH request: `RemoteDirectoryPicker.tsx`, its tests, `ProjectDialogs.test.tsx`, picker design documentation and its QA harness. Backend SSH platform, registration identity and persistence changes remain foreign.

No commits have been made by this continuation at the time of this receipt.

## Terminal focused and presentation regressions

```sh
bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/components/TerminalPane.test.tsx src/components/TerminalSplitView.paneHandleReach.test.tsx src/components/TerminalSplitView.paneHandleDrop.runtime.test.tsx src/components/TerminalSplitView.dragFeedbackVisibility.test.tsx src/components/NativeTerminalPane.presentation.test.tsx
```

Monitor: `mon_917FY8DZWA62CSBD`, session `bash_6`.

```text
 Test Files  7 passed (7)
      Tests  215 passed (215)
watcher completed (exit code 0)
```

This current shared-tree run includes drag-feedback visibility and native presentation regression tests. The terminal worker previously observed a transient concurrent visibility failure; it was not reproduced by this later lead run. The lead did not edit the foreign visibility helper.

Cleanup: the test command exited 0, with no persistent QA server/browser created by this check.

## Combined frontend build

```sh
bun run --cwd ui build
```

Monitor: `mon_ETAK3A5SVJ02DZM2`, session `bash_9`.

```text
1872 modules transformed.
built in 2.18s
watcher completed (exit code 0)
```

The package script runs `tsc && vite build`. This proves current frontend type checking and bundling, not native desktop rendering. No release bundle was built and no desktop/daemon process was restarted.

## Original RED provenance correction

Original source transcripts store monitor outputs as `custom_message` events, not normal `message` events. The lead's first search excluded those events and incorrectly treated some historical RED output as unavailable.

`original-red-notifications.json` now preserves the exact original event objects and their source transcript paths:

- Sidebar: `c27d8599`, three empty-workspace failures, `2026-09-08T14:54:02.783Z`.
- SSH: `837cd36f`, eight autocomplete failures, `2026-09-08T14:49:15.642Z`; `dce63196`, three edge failures, `2026-09-08T14:55:28.635Z`.
- Terminal: `73305e94`, two height/reach failures, `2026-09-08T14:53:04.528Z`; `c721a5ae`, opaque backing regression, `2026-09-08T14:56:36.511Z`.

The SSH worker's later isolated regression reproduction is additional proof. It is not a historical run. The sidebar continuation's new restoration guard has separately described mutation proof; the original notification covers the earlier empty-expansion correction.

## Final affected test union

The lead independently ran the same 18-file related test union as the verifier. Exact output is preserved in `lead-final-test-union.log`.

```text
 Test Files  1 failed | 17 passed (18)
      Tests  2 failed | 376 passed (378)
 Duration  15.57s
error: script "test" exited with code 1
```

Both failures are the unrelated remote grouping fixtures named in `RESULT.md`. The broader suite is not labeled green.

## Final deterministic browser run

```sh
bun run docs/session-continuation-20260908/integration/qa-integration-runner.ts
```

The lead compared the final runner's `logEvidence` call sites against the previous verified runner: no scenario assertion was removed or added. The cleanup helper alone stopped polling.

```text
All 6 integration scenarios PASSED!
Vite server (PID 18823) sent SIGTERM, awaiting process exit...
Vite server (PID 18823) exited.
Port 5214 freed: true
Written integration-qa-evidence.json
watcher completed (exit code 0)
```

The evidence JSON contains 21 scenario assertions plus three cleanup checks. Bun WebView closure, process exit and a single socket refusal check completed. The final run removed its restored temporary harness files and integration cache; all QA resources it created were cleaned.
