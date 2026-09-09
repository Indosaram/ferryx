# Common shortcut diagnostics

## Scope

The user reports that all app shortcuts fail while an ordinary terminal is
active. Browser-specific routing does not explain this report. The user
authorized temporary common-path instrumentation and a `bun tauri dev` run.
The user physically exercised the debug app and confirmed all tested shortcuts
worked. The original installed-app failure did not reproduce; its cause remains
unconfirmed.

## Evidence before instrumentation

- The installed application is version `2026.909.1`.
- Its resource bundle contains the common shortcut hook and a call to that hook.
- The current source registers shortcuts on window capture, before terminal
  document capture and textarea handlers. Terminal input is explicitly exempt
  from the general editable-target exclusion.
- The installed GUI PID 26679 was in the normal AppKit run loop during a
  one-second sample, not a sustained application-lock wait.
- `/tmp/rorca-501/boot-trace.log` recorded a Tauri listener error at
  2026-09-09 18:25:40 KST:
  `TypeError: undefined is not an object (evaluating 'listeners[eventId].handlerId')`.
  Its causal relationship to the shortcut failure is not established.
- The release key trace is disabled by default. An unchanged
  `/tmp/ferryx-switch-debug.jsonl` is not evidence that key input failed to arrive.

## Temporary instrumentation

All added diagnostics are marked `TEMPORARY-DIAGNOSTIC`. They use the existing
development/explicit-opt-in logging gate. New shortcut records omit typed
characters, field values, and DOM text. Existing general terminal tracing is
unchanged.

- Native stderr: `shortcut.native.keydown` is emitted before routing and the
  focused-session lock. `shortcut.native.focus.start` and `.return` bracket that
  lookup. Native menu, forwarding, copy, and paste decisions are also recorded.
- WebView console and `/tmp/ferryx-switch-debug.jsonl`: `shortcut.webview.*`
  records receipt independently of the mounted application hook.
- `shortcut.hook.*` records registration, receipt, match, rejection reasons,
  and unhandled chords.
- `shortcut.native.register.*` and `.receipt` cover native menu event
  subscriptions and callbacks.
- `shortcut.action.invoke`, `.return`, and `.error` bracket synchronous handler
  invocation. Return does not prove asynchronous completion. Returned promises
  are unchanged; global rejection logging observes otherwise unhandled failures.

The macOS callback's older synchronous copy/paste file logging was replaced by
stderr traces. No shortcut matching or event-consumption decision was changed.

## Reproduction

Run exactly `bun tauri dev` from the repository root. Its existing macOS runner
signs the debug app with the Developer ID Application certificate. Debug daemon
endpoints use `/tmp/rorca-501-dev`, separate from release endpoints.

Do not stop the installed GUI or either existing daemon. Do not replace the
installed application. The user performs physical keyboard input in the debug
window; the agent does not inject desktop input.

Interpret one reproduction by following the same chord through native receipt,
WebView receipt, hook receipt, and action invocation. Missing native receipt,
missing WebView receipt, a missing hook registration, a rejection reason, and an
action error are different findings. Do not replace this evidence with synthetic
DOM tests or process existence.

## Validation

The implementation run reported 74 focused diagnostics/shortcut/logger tests
passing, three selected native-menu wrapper tests passing, successful TypeScript
and Vite checks, and successful `cargo check --lib -j 4`.

LSP diagnostics are unavailable because the LSP daemon is unreachable. The
expanded IPC wrapper suite has three unrelated existing DTO expectation
failures: two `attentionInventory` expectations and one notification `sound`
expectation. Those assertions were left unchanged.

The lead independently verified 74/74 focused tests and a successful
`bun tauri dev` build/launch. The debug app PID was 57934 and codesign reported
`Developer ID Application: Indo Yoon (5DUM8WPB4C)`. Existing release GUI and
daemon PIDs remained present.

## Physical run outcome

At approximately 22:22:36-22:22:43 KST, run
`89d1aed3-3b4f-482d-bf5c-40c1043b047e` recorded:

- Cmd+T: native receipt and menu dispatch, frontend callback invocation, and
  workspace tab count increasing from two to three.
- Ctrl+1/2/3/4: trusted DOM key events, `tab.selectN` matches, and handler
  invocation/return.
- Cmd+1/2: native worktree-selection forwarding and frontend callbacks.
- The user explicitly confirmed that everything worked in this run.

TypeError unhandled-rejection records also occurred while these shortcuts
worked. Therefore the existence of such an error alone cannot establish that it
caused the original global shortcut failure.

This is a successful diagnostic reproduction attempt, not a verified fix.
The fresh debug process, frontend build, and isolated debug daemon/session
state differ from the installed release instance. The instrumentation itself
also changes timing. None of these variables has been isolated as the cause.
If the failure recurs, capture it in the same instrumented process before
restarting. Temporary diagnostics remain uncommitted for that purpose; foreign
browser/App changes were preserved.
