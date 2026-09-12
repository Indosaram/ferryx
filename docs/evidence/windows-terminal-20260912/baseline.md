# Before-change baseline

Captured 2026-09-12, HEAD `99e0450d`, before this session's production changes.

Invocation:

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/TabBar.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx
```

Monitor `mon_6J2JG08TYR85MD42`, process session `bash_1`.

Result: exit 1; 3 failed, 53 passed, 56 total; 18.39 seconds.

- `TabBar.test.tsx`: 17 passed.
- `NativeTerminalPane.lifecycle.test.tsx`: 30 passed.
- `NativeTerminalPane.presentation.test.tsx`: 6 passed, 3 failed.

Failures, verbatim:

```text
FAIL native terminal presentation retention > retains a shown final frame on exit, blocks input, and releases it on unmount
AssertionError: expected [ [ …(2) ], [ …(2) ] ] to have a length of +0 but got 2
NativeTerminalPane.presentation.test.tsx:144:52
expect(commands("cmd_native_terminal_detach")).toHaveLength(0);

FAIL native terminal presentation retention > never reattaches a dead PTY when retained-frame geometry recovery is requested
TestingLibraryElementError: Unable to find an accessible element with the role "alert"
NativeTerminalPane.presentation.test.tsx:187:50
await act(async () => { fireEvent.click(view.getByRole("alert")); });

FAIL native terminal presentation retention > holds the final frame until a reconnected replacement is presented
AssertionError: expected [ [ …(2) ], [ …(2) ] ] to have a length of +0 but got 2
NativeTerminalPane.presentation.test.tsx:217:52
expect(commands("cmd_native_terminal_detach")).toHaveLength(0);
```

These are existing failures, not proof of the reported Windows startup defect. The bounds diagnosis lane was notified. The presentation test already stubs global navigator to MacIntel/Macintosh at line 55; a missing platform fixture must not be assumed without tracing which navigator object production reads.

Cleanup receipt: one-shot test process exited with status `exited_1`; no server, browser or desktop process was spawned.
