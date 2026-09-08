# Ctrl+C and exit recovery implementation

Date: 2026-09-08

## Scope and outcome

Implemented the approved F1, F2, and F3 fixes from
`docs/AGENT_CTRL_C_EXIT_REVIEW_2026-09-08.md`.

- A confirmed PTY exit clears its backend binding while retaining the provider
  conversation reference. Reconnect is available, and an old exit event cannot
  clear a replacement backend binding.
- Native input is retried only when the backend explicitly reports
  `details.inputWritten === false`. This flag is produced at the native input
  encoding boundary, before any daemon write. Unclassified, transport, and
  post-write receipt errors do not replay non-idempotent input.
- Every physical plain Ctrl+C is forwarded unchanged. Removed Ctrl+U
  substitution, the 700 ms double-press policy, and the delayed synthetic Ctrl+C.
  Agent programs retain responsibility for their own cancel and exit semantics.

## Changed production files

- `ui/src/state/workspaceStore.ts`: release backend ID on `exited`.
- `ui/src/components/NativeTerminalPane.tsx`: gate retries on proof of no write;
  replace the agent-dependent Ctrl+C gesture with direct forwarding.
- `src-tauri/src/ipc/native_terminal.rs`: expose structured pre-write errors
  from the existing input boundary and propagate them from the command.

Regression coverage was added or updated in:

- `ui/src/lib/agentResumeAffordance.test.ts`
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`
- `ui/src/components/NativeTerminalPane.test.tsx`
- `src-tauri/tests/native_terminal_input_boundary_contract.rs`
- The existing unit test module in `src-tauri/src/ipc/native_terminal.rs`

No dependencies, daemon protocol changes, commits, application launches, or
daemon restarts were required.

## Red-to-green evidence

1. Live exit to reconnect eligibility:
   - RED: `expected 'pty-running' to be null`.
   - GREEN: all 20 tests in `agentResumeAffordance.test.ts` passed.
2. Accepted input followed by IPC failure:
   - RED: all three error variants produced `['c', 'c']` instead of `['c']`.
   - GREEN: the input recovery/lifecycle selection passed 19 tests.
3. Native pre-write error contract:
   - RED: the error lacked `details.inputWritten`.
   - GREEN: `rejected_native_input_reports_that_no_pty_write_occurred` passed.
4. Ctrl+C passthrough:
   - RED: waiting/completed/metadata-only and repeated-key scenarios still
     produced Ctrl+U or an extra Ctrl+C.
   - GREEN: the affected component suites passed after removing the gesture.

## Verification

The following eight-file regression command passed: 281 tests, exit code 0.

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/components/NativeTerminalPane.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/lib/agentResumeAffordance.test.ts \
  src/state/workspaceStore.test.tsx \
  src/state/workspaceActivity.test.tsx \
  src/state/workspaceNativeActivity.test.tsx \
  src/lib/agentReconnect.test.ts \
  src/lib/agentReconnect.integration.test.ts \
  --maxWorkers=1 --reporter=dot
```

The first build caught an invalid test-only `idle` activity fixture. The frontend
uses `working | waiting | done`; backend idle maps to the completion state.
The invalid case was removed, retaining all supported states and the existing
missing-activity test. After this correction, the following final component
command passed: 171 tests, exit code 0. Counts overlap with the command above.

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/components/NativeTerminalPane.test.tsx \
  src/components/TerminalPane.test.tsx \
  src/components/TerminalPane.nativeAlways.test.tsx \
  --maxWorkers=1 --reporter=dot
```

Additional checks:

```sh
bun run --cwd ui build
cargo test --manifest-path src-tauri/Cargo.toml \
  --lib rejected_native_input_reports_that_no_pty_write_occurred
cargo test --manifest-path src-tauri/Cargo.toml \
  --test native_terminal_input_boundary_contract -- --test-threads=1
```

- Frontend typecheck and production build: passed, exit code 0.
- Native regression unit test: 1 passed, exit code 0.
- Native input boundary integration target: 15 passed, exit code 0. Its
  in-memory attachment fixture was inspected before running; it does not
  connect to or shut down the user's daemon.
- Scoped `git diff --check`: passed.
- LSP diagnostics could not run because the LSP daemon was unreachable.
  Compiler/typechecker validation completed successfully instead.

## Browser behavior verification

Used isolated headless Chrome 152 with the actual `TerminalPane`,
`NativeTerminalPane`, workspace reducer, and compiled application CSS. Only the
Tauri IPC boundary was controlled; it recorded accepted input and injected
pre-write versus post-write failures. No desktop mouse/keyboard input was sent.

Observed in the final run:

- Waiting agent, two physical Control+C presses: `['c', 'c']`.
- Post-write receipt error: one accepted input, one original attachment,
  visible error, no replay.
- Explicit pre-write failure: one accepted input after recovery, two total
  attachments, error cleared.
- Exit event: backend ID became null, lifecycle became `exited`, provider ID
  remained `qa-provider`.
- The enabled "Reconnect Claude Code session" button invoked its callback
  with local pane ID `qa-pane`.
- Unhandled page errors: zero.

The first browser setup needed its React development runtime and Tauri event
cleanup shim completed. The final run above used the corrected fixture. The
isolated browser and Vite server were closed after verification.

## Remaining boundaries

- Real agent shutdown, native WGPU rendering, and cross-platform desktop
  interaction were not exercised. This is not proof that every agent exits
  after one Ctrl+C; some agents intentionally cancel first and exit later.
- F4 (normal exit versus disconnected wording/reason) and F5 (release of
  agent-owned status after process exit inside a surviving shell) remain
  separate findings outside this approved increment.
- A user-owned desktop check should use the debug application through exactly
  `bun tauri dev`: verify Ctrl+C in working and approval-waiting states, then
  exit a resumed agent and confirm that Reconnect is available.
- All implementation changes remain uncommitted in the shared working tree.
  Other sessions can modify them until they are committed.
