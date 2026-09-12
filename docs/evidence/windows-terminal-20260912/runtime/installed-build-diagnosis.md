# Installed Windows bounds failure: remaining diagnostic seam

Date: 2026-09-12. Task: st_01a0963e. Source/history-only investigation.
No production changes, remote actions, or repeated runtime tests were performed.

## Outcome and attribution

The missing evidence is the **raw rejection of `cmd_native_terminal_set_bounds`
from the installed app**, not another current-build startup test.
The exact bare banner does NOT establish an unstructured IPC error: the Aug 28
frontend discarded the cause for every non-detached bounds failure.

- `runtime/artifacts/bounds-owner.json:2-14` records the exact banner, WebView
  PID 19396, parent 17288, and the installed `com.ferryx.app` WebView profile.
- Task-supplied executable metadata: `C:\Users\sook\AppData\Local\Ferryx\ferryx.exe`,
  35,909,632 bytes, ProductVersion `0.1.0`, LastWriteTime
  `2026-08-28T20:02:28+09:00`. This is not an exact source identity.
- Existing `runtime/FRESH-RUN.md` and `runtime/fresh-visual-review.md` report
  successful cold-loaded shell menu, cmd echo, startup and resize at `7f7ecd8e`.
  The bottom-right banner belongs to the other installed process, not that QA app.
  This is existing evidence, not execution or image inspection by this task.
- The isolated QA profile/build differs from the installed app. Its success does
  not prove a particular historical defect caused the installed failure.

## Historical call chain and information loss

Use `fee4a1e1` (Aug 28 15:32 KST) as a concrete historical source specimen,
NOT as the installed executable's asserted commit. References below are
`commit:path:lines`; the full paths are given on first use.

1. `fee4a1e1:ui/src/components/NativeTerminalPane.tsx:562-590` invokes attach,
   then calls `reportBounds()` after successful attachment; ResizeObserver is
   installed there. Thus this UI failure follows attachment and bounds dispatch,
   not the shell-menu or default-profile selection path.
2. Same file `:478-500` clears failed geometry, ignores recognized
   `SESSION_NOT_FOUND`, logs `terminal.surface.bounds.error` using `String(error)`,
   calls `reportNativeTerminalIpcFailure`, then unconditionally sets the bare
   `Failed to update native terminal bounds` string.
3. Same file `:134-136` preserves the raw object in
   `console.error("Native terminal IPC command failed", { command, error })`.
   The debug-file string conversion can instead reduce a structured error to
   `[object Object]`; absence of a useful debug record does not exonerate IPC.
4. `fee4a1e1:src-tauri/src/ipc/native_terminal.rs:177-223` first looks up
   `get_webview_window("main")` (`:195-197`), dispatches `state.render` on the main
   thread (`:202-214`), then awaits its result (`:215-217`). Distinct failures:
   - `Main Ferryx window is unavailable` (lookup).
   - `Could not dispatch native terminal render: ...` (dispatch).
   - `Main thread stopped before native terminal render completed` (channel).
   - A converted native render error (layout/snapshot/surface/renderer).
   The daemon PTY resize result is ignored at `:219-221`, so its rejection
   cannot cause this handler's bounds banner in this specimen.
5. `fee4a1e1:src-tauri/src/native_terminal/surface_host.rs:988-1035` checks attached
   state, prepares layout, obtains the terminal snapshot, lazily constructs a
   surface host, updates renderer configuration, then renders the snapshot.
   Any propagated error here shares the same historical UI text.
   `fee4a1e1:src-tauri/src/ipc/error.rs:56-63,211-222` serializes code/message/details;
   detached sessions map to `SESSION_NOT_FOUND`, other native errors to
   `INTERNAL_ERROR`. The bare text cannot distinguish those renderer branches
   from window lookup or an invoke/serialization rejection before handler entry.

## Established defects already fixed, versus installed-app hypotheses

**Established historical information loss:** `ba90ceb7` (Sep 7) changed the
bounds catch to retain structured debug errors and display the cause;
`331f8005` (Sep 8), `ui/src/components/NativeTerminalPane.tsx` bounds catch,
then limited the displayed suffix to structured errors. Therefore applying the
current bare-text interpretation to an Aug 28 UI is invalid. Nor does the bare
text uniquely date the build: modern unstructured failures also produce it.

**Established historical window-lookup defect:** adding a differently labelled
browser child makes Tauri's `get_webview_window("main")` return None despite an
existing native main window. The recorded failing/passing integration evidence
is `docs/verification/NATIVE_TERMINAL_BROWSER_WINDOW_20260909.md:12-56`.
Git attribution matters: `6ead7827` carries the IPC/output-pump migration and
browser-child regression tests; `eeb9e1dc` carries platform `Window<R>` migration
and the verification document, not the IPC diff itself.
`7f7ecd8e:src-tauri/src/ipc/native_terminal.rs:565-567` uses `get_window`.

**Hypotheses, not findings about PID 17288:** an old lookup with a restored
browser child is a specific plausible cause, but no captured raw error or child
inventory proves it here. Native surface/GPU/layout failure, main-thread failure,
and IPC bridge/argument rejection remain alternatives. The previous report's
wgpu-30 leading hypothesis was framed for a failing current debug build; that
premise is now disproven by attribution. It must not be inherited for an
unidentified installed binary merely because both show a bounds banner.

## Minimal next read-only runtime probe

Read the installed WebView's **existing console error record**, scoped to its
verified GUI parent (17288 only while still that process) and WebView 19396.
If an already available inspector/CDP target exposes it, retrieve the preserved
`Native terminal IPC command failed` entry whose `command` is
`cmd_native_terminal_set_bounds`, including raw error type, code, message and
details. Read properties without invoking getters or evaluating application code.
Do not launch with new flags, reload, click Retry, resize, invoke bounds manually,
or attach to the QA CDP target; these are not a read-only probe of this incident.

`INTERNAL_ERROR / Main Ferryx window is unavailable` selects the lookup/lifecycle
branch; then a read-only existing child-webview inventory distinguishes the known
browser-child trigger from a genuinely missing main window. A GPU/layout/native
message selects render; dispatch/channel text selects main-thread transport;
an unstructured command/argument/permission error selects the invoke bridge.
If no inspector or retained raw console record is available, report that exact
observability blocker, not a new root cause. The existing `FRESH-RUN.md` notes
1,806 debug records but no bounds-error records; rereading that file alone is not
the missing discriminator. Binary SHA-256 can later establish artifact identity
against a known release hash, but size/version/mtime alone cannot replace the
raw rejection or prove that a known fix resolves this installed incident.

## Lead runtime follow-up

The installed WebView had no listening CDP port or DevToolsActivePort file.
GUI port61948 returned BROWSER_CLI_REQUEST_INVALID to an HTTP probe; the
correct NDJSON `{"command":"list"}` returned `{"type":"list","sessions":[]}`.
Thus no live restored browser session supports the child-browser hypothesis.
No installed app reload, resize, retry, or mutation was performed.
The raw console rejection is not exposed through this read-only interface.
The user was asked for that retained Console record; the root cause remains
unconfirmed rather than assigned to an already-fixed historical defect.
