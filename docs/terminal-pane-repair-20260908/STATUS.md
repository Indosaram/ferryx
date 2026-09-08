# Terminal pane repair status

## Current checkpoint

Code changes are committed and the reviewed delta is approved:

- `0d19660`: obsolete deferred bounds no longer restore an older width.
- `851b763`: concurrent owner's macOS terminal retention behind overlays.
- `8311967`: preserve browser child-view masking while retaining terminals.
- `57980ce`: cover both orders of concurrent deferred resize completion.

Captured checks: 204 related UI tests, 158 native terminal tests, 14 visibility
tests after retention mutation, cargo check, and the full UI build all passed.
See `COMPLETION_AUDIT.md` for command-to-artifact mapping and limitations.
`REVIEW.md` records approval of the code delta, not desktop acceptance.

**The goal is not complete.** Real signed-debug desktop C1/C2/C3 scenarios
remain unverified because Accessibility and Screen Recording permissions are
denied. A persistent permission subscription will resume verification when both
are granted. No debug/release GUI or daemon was launched or restarted by this
session. Final desktop evidence and cleanup receipts must still be added.

The sections below preserve the initial investigation checkpoint; their pending
test/build statements are superseded by this section and COMPLETION_AUDIT.md.

## Scope

Two reported failures: native terminal panes repeatedly turn black after input or
resize errors, and window/split-pane resizing sometimes retains the old terminal
width. Error and popover presentation must preserve terminal content outside the
overlay. This is ongoing work, not a completion report.

## Current evidence

- Source baseline: `a7e24d3`.
- `NativeTerminalPane.tsx` puts an opaque full-pane backing behind its error
  button. Successful geometry or input clears the error; another failure puts
  the backing back.
- `cmd_native_terminal_send_input` writes input to the daemon before querying
  a fallible terminal snapshot for its receipt. A receipt failure is therefore
  not proof that the input was not delivered.
- The UI permits only one outstanding bounds IPC and queues the latest measured
  geometry behind it. The backend waits for updates during synchronized output.
  A one-second synchronized-output expiry exists; permanent starvation is not
  established solely by reading the wait loop.
- macOS terminal child views are currently inserted below WebKit, but
  `useNativeTerminalVisibility` still suppresses native surfaces when a dialog
  or search surface is mounted. Historical Above-WebKit guidance is stale for
  current macOS native terminal targets.

## Investigation execution

Run `dag_a1bc6899-f931-4467-bac9-db4f861076d3` has separate error/overlay and
resize diagnosis lanes followed by synthesis. Its original category routes
failed before executing code: invalid Codex refresh token and usage limit.
The same run was amended to the currently serving `opencodex/gpt-6-astra`
provider through read-only explore agents. Both producer nodes were observed
running after the routing change. Their outputs require source verification;
running status is not evidence of a completed diagnosis.

## Verification gates

- C1: rejected bounds/input produces an actionable nonblocking error without
  covering terminal content; retry never duplicates already-written input.
- C2: 800x480 -> 640x480 -> 900x480 converges to the final bounds and PTY columns,
  including synchronized output.
- C3: macOS overlays preserve terminal rendering; other compositor platforms
  retain a valid fallback.
- C4: attach/detach ownership, input/IME, synchronized-output expiry and
  intentional scroll-up do not regress.

Each behavior change requires a right-reason RED before implementation, GREEN
afterward, and faithful surface evidence. Tests alone do not establish native
desktop success. No gate has passed yet.

Desktop execution is limited to `bun tauri dev`, signed debug builds with the
required Developer ID certificate. Existing application/daemon processes and
foreign working-tree changes are not QA resources and must not be terminated,
overwritten or staged.

## Tool limitations

- LSP daemon is unreachable; compiler/test diagnostics remain necessary.
- The `orca` executable is not available on PATH. Any alternate desktop evidence
  channel must be verified before use.
- The installed application's stdout/stderr do not provide the reported native
  IPC error; `/tmp/ferryx-switch-debug.jsonl` was absent during initial diagnosis.

## Durable work log

`/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/ulw-20260908-233043.XXXXXX.md.7Fwv8vscBW`
