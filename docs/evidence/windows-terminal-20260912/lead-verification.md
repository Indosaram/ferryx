# Lead verification receipts

## Direct combined test run

Run on 2026-09-12 at 21:21:54, from the current shared working tree:

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/TabBar.test.tsx \
  src/components/TerminalSplitView.shell.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx \
  src/components/NativeTerminalPane.exitAttach.test.tsx \
  src/lib/nativeTerminalLifecycle.test.ts
```

Monitor `mon_2SB9XKK748G4B4NZ`, session `bash_2`: exit 1, 4.73 seconds.
The lead read the full output after notification truncation.

- TabBar: 19 passed.
- TerminalSplitView shell forwarding: 4 passed.
- NativeTerminalPane lifecycle: 30 passed.
- NativeTerminalPane presentation: 9 passed. All three original baseline failures are now green.
- Native terminal lifecycle module: 7 passed.
- NativeTerminalPane exitAttach: 2 passed, 2 failed.
- Total: 71 passed, 2 failed; not an overall green test run.

The two failures are at `NativeTerminalPane.exitAttach.test.tsx:124` and `:190`.
Both assertions expect two callback arguments, while the current callback also provides the
binding key (`dead-backend-session::0:` or `legacy-dead-session::0:`). The public callback
signature includes the optional third binding key. This repair did not change that contract
or these tests. The presentation worker reports the same failures before its change; the
lead independently confirmed the current failures, but did not independently run a pristine
checkout for that baseline claim. Assertions have not been weakened.

## Direct historical bounds regression run

```sh
cd /Users/indo/code/project/orca-lite/src-tauri
cargo test --lib browser_child -- --test-threads=1
```

Monitor `mon_YT8XX0MFJ7429MA5`, session `bash_3`: exit 0.

- `native_terminal::surface_host::tests::bounds_ipc_presents_when_browser_child_is_open`: passed.
- `native_terminal::surface_host::tests::output_presents_when_browser_child_is_open`: passed.
- Result: 2 passed, 0 failed, 1030 filtered out; tests finished in 0.42 seconds.

These tests verify the historical multi-webview lookup fix, not the current Windows startup
failure. The run used a shared working tree containing foreign Rust changes, so it does not
establish pristine Windows baseline behavior.

## Diagnostics and build

The lead ran LSP diagnostics on all four shell files: no errors. NativeTerminalPane has no
errors and one existing deprecated `keyCode` hint. Build command `bun run --cwd ui build`
completed under `mon_B0HRDDWFJBPSKZG4` / `bash_4` with exit 0: TypeScript passed,
1892 modules transformed, Vite built in 3.19 seconds.

## Scope review and remaining evidence

The lead inspected the actual shell source diff, complete forwarding test, presentation
source diff and its layout cleanup. The test-only platform override was removed. The popup
IPC supports the submenu and emits its selected ID app-wide.

Windows startup cause, actual shell spawning, startup/resize screenshots and HWND geometry
are still missing. No task-wide completion or commit is claimed. Runtime reproduction is
recovering its existing QA resources after a provider stream failure.
