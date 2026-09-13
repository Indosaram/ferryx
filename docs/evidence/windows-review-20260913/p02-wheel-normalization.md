# P02 bounded frontend wheel normalization

Date: 2026-09-13. Child st_01a09947. Host: macOS arm64.

## Delivered scope

Only NativeTerminalPane.tsx, its existing .test.tsx, and this receipt were edited.
Initial scoped git diff was empty; the shared tree already contained unrelated
foreign changes, which were left untouched. Read the original root brief,
repair-packets P02, and wheel-regression-seam.md before implementation.

The actual mounted component onWheel handler now:

- Ignores zero/horizontal-only vertical deltas (and nonfinite input).
- Converts line units to three rows per notch, page units to scrollbar.len
  visible rows, and retains the existing 20 CSS pixels per row conversion.
- Retains fractional pixel movement, including opposite-direction cancellation.
- Saturates emitted whole rows to [-32768, 32767], retaining only fractional
  pixel remainder, not excess whole rows that could replay on later input.
- Resets its per-component pixel remainder in a layout effect on binding key,
  pane identity, and interactive visibility changes. The existing binding key
  includes backend ID, daemon epoch, remote generation and connection state.
- Preserves existing session ID/generation payloads and hidden/non-Tauri gates.

Page conversion uses the existing backend scrollbar metrics, not guessed pixel
height. Before those metrics exist, page movement emits nothing (zero known
visible rows). This is an explicit bounded choice, not a new geometry contract.
Line/page events do not consume or reset a pixel-device fractional remainder.

The backend inspection confirmed cmd_native_terminal_scroll casts rows to i16
before compute_wheel_outcome. That adapter still replaces coordinates/modifiers;
this patch does not change its contract, backend implementation, engine sign or
tick policy, mouse/key/IME/drop/debug behavior, or add another native wheel route.

## Registered command and actual execution

All three invocations used exactly the pre-registered C002 command:

```sh
bun run --cwd ui test src/components/NativeTerminalPane.test.tsx
```

Vitest 3.2.7, existing script maxWorkers=1. No alternate invocation, filters,
broad suite, build, desktop, daemon, SSH or native runtime was launched.

### Initial fixture failure (not behavioral RED)

14:43:16, 3.37s, exit 1: 15 failed, 152 passed, 167 total.
The new readiness assertion expected data-native-terminal-presented=true but
received false. The bounds mock returned undefined instead of the actual
presentation receipt consumed by the component. This was a new fixture defect,
not a production defect or pre-existing failure. Only that mock was corrected
to return { presented: true, renderDeferred: false }; production stayed clean.
Session output marker: P02_RED_EXIT=1.

### Intended behavioral RED

14:43:44, 3.40s, exit 1 (P02_BEHAVIORAL_RED_EXIT=1):

```text
Test Files  1 failed (1)
Tests       15 failed | 152 passed (167)
error: script "test" exited with code 1
```

All fifteen new normalization cases reached intended IPC assertions:

- Zero and horizontal-only expected no calls, received rows=-1.
- Line notches expected [3,-6], received [1,-1].
- Pages expected [24,-48], received [1,-1].
- Positive/negative fractional pixel sequences expected three unit-row calls,
  but received a forced row on every sub-row event.
- Direction cancellation expected [-1], but received forced intermediate rows.
- Pixel/line/page extreme magnitudes exceeded the expected i16 limits.
- Updated visible rows/sibling control expected rows=40, received rows=1.
- Session, generation, daemon epoch and hidden transitions expected no command
  before accumulating a fresh complete row, but received early rows=1 calls.

Actual assertion locations at RED: test lines 565 (table payload equality),
577 (updated visible rows), 603 (transition empty payload list). All 151
pre-existing tests passed, as did the new non-Tauri control. No pre-existing
failure was observed or repaired. Existing polling/fake-time tests were kept
unchanged, not rewritten or used in the new fixtures.

### GREEN, identical behavioral assertions

After the minimal production change, with no test edits after behavioral RED:

```text
src/components/NativeTerminalPane.test.tsx (167 tests) 2539ms
Test Files  1 passed (1)
Tests       167 passed (167)
Start at    14:44:44
Duration    3.17s
P02_GREEN_EXIT=0
```

One GREEN invocation, no retries, no skipped tests. Existing attach-failure
fixtures emitted their expected IPC error logs on stderr; no tests failed.

## Deterministic mounted surface coverage

Added 16 tests, not a standalone normalizer mock. Real React component mounts,
real DOM WheelEvent dispatch via fireEvent, real onWheel and actual invoke
payload capture. The fixture subscribes to the precise set_bounds request
before mount; async act awaits that signal and commits the presentation receipt.
It asserts exactly one scrollbar listener registration, a callable listener,
and the committed presented attribute before wheel dispatch. Vitest's bounded
test timeout is only a failure deadline; no polling, waitFor, fixed sleep or
scheduler delays were added. Session/visibility rerenders complete through act,
with input-enabled state asserted before dispatch. The existing genuine
scrollbar callback path supplies resized metrics and rejects sibling metrics.

The test harness mocks Tauri IPC only; it does not claim actual viewport or PTY
bytes. New fixtures clean up component mounts, global ResizeObserver stubs,
geometry overrides and lifecycle/session state. Existing component cleanup
owns scrollbar hide timers and listener teardown.

## Diagnostics, diff and cleanup

LSP diagnostics before GREEN on both changed code files: no errors or warnings.
Existing deprecation hints only: product keyCode line 190; test navigator.platform
lines 2817, 3829, 4051, 4118, 6301. No suppression added.
Scoped git diff --check passed. Product diff was inspected: 21 changed/additional
lines confined to accumulator lifetime and onWheel; two code files together
142 insertions, 2 deletions. No existing tests were deleted or modified beyond
an added provider import and the isolated new describe block.

Command output was captured in child-owned /tmp/st_01a09947-{red,
red-assertions,green}.log during execution; relevant receipts are retained here
and full tool outputs in the child session. Temporary logs were removed after
recording this report. No branches/worktrees, commits, user data changes,
background runtime resources or installed binaries were created.

## Acceptance still pending

This closes only the bounded local frontend normalization implementation.
The wheel problem is NOT fully fixed. Lead independently verifies the combined
batch and owns any authorized build. Windows HWND -> DOM wheel delivery,
backend coordinate/modifier context preservation, viewport/PTY effects,
up/down directions, split/sibling isolation, tracking and alternate screen,
Shift policy, unfocused pane and DPI runtime acceptance all remain pending
through the registered real Windows debug GUI surface. No Windows runtime
acceptance or parent completion is claimed here.
