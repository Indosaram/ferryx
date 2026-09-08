# Terminal background and resize redraw repair

## Scope and observation

The reported symptoms were intermittent transparent terminal backgrounds and a
resize that appeared to scroll down from older content. The user clarified that
the final content returns without scrolling or typing. This distinguishes the
reported behavior from a viewport that remains permanently pinned in history.

The existing two-row bottom tolerance and explicit `ScrollViewport::Bottom`
restoration remain in place. This repair does not force a user who intentionally
scrolled up back to the bottom on every output chunk.

## Background ownership

The macOS WebView must remain transparent along the terminal ancestor chain:
native terminal views are below it so that DOM overlays can remain above them.
Previously, the NSWindow was also transparent. Hiding a native surface, removing
it, or resizing it before the next drawable therefore removed the only opaque
content below the WebView.

The NSWindow now owns an opaque background. Setup uses the renderer's default
without performing a new synchronous preferences import on the AppKit thread.
The configured terminal theme is applied when a terminal target is created and
refreshed during viewport updates. The backing survives child-view hiding,
removal, drawable replacement, and theme changes. Changing a theme's alpha cannot
turn the window backing transparent. DOM opacity and native view ordering are
unchanged.

The platform-specific change is isolated to macOS. Windows and Linux retain
their existing native window/compositor implementations.

## Resize and synchronized output

The custom renderer previously presented snapshots while DEC private mode 2026
(synchronized output) was active. libghostty-vt exposes the mode but does not
enforce the renderer's presentation policy. Ghostty's own renderer checks this
mode before rendering; Ferryx did not.

The repair reads the mode through Ghostty's existing typed C mode API. It does
not scan or parse output bytes a second time. While the mode is set, direct and
scheduled rendering retain the previously presented drawable without acquiring
or presenting an incomplete frame.

Ghostty's resize API also clears mode 2026. Ferryx now preserves an in-progress
application transaction across geometry changes, including its original
deadline. A reset still clears the mode and deadline.

An incomplete synchronized transaction has a one-second deadline. The output
pump subscribes to that deadline and ends the transaction even if no more output
arrives. The deadline is not extended by additional chunks or repeated resizes.
Stream termination also ends the transaction. These are protocol recovery paths,
not a resize debounce or an arbitrary delay before every frame.

Deferred output and a dropped GPU frame have different completion semantics.
`renderDeferred` crosses the receipt boundary, and deferred frames do not enter
the dropped-frame retry loop. Bounds IPC subscribes to session updates before
dispatching rendering and waits for a real completed presentation. This preserves
the frontend's outgoing-surface lifetime: it must not release an old surface on
an acknowledgment for a frame that has not appeared. Detach wakes this wait.

Queued PTY resize requests are coalesced per session before each dispatch. A
resize already in flight is allowed to finish; queued intermediate dimensions
are replaced by each session's latest dimensions. Pending sessions retain their
first-seen ordering.

## Regression evidence

The following failures were observed before their corresponding fixes:

- Native macOS background contract: the window remained non-opaque after a child
  was hidden, resized, and removed.
- Direct renderer contract: an unfinished synchronized redraw reached
  presentation.
- VT resize contract: resizing cleared synchronized output before the application
  completed its redraw.
- PTY queue contract: four intermediate requests were applied instead of the
  latest two per-session requests.
- Frontend contract: a deferred frame scheduled another animation-frame retry.
- Bounds IPC contract: a deferred replacement was acknowledged before presentation.

Each of these focused regressions subsequently passed. Additional tests exercise
split escape sequences, bounded timeout recovery with a paused Tokio clock,
output-pump presentation, stream termination, detach cancellation, serialized
deferral status, and resize requests arriving while an earlier request is in
flight.

The native background test uses real AppKit objects on the process main thread.
Its window is never ordered on screen; it does not manipulate the user's desktop.
The host tests use real Ghostty VT state and production output/IPC/scheduling
paths, substituting only the native frame target.

## Validation environment

The shared checkout is based on `799e0e4`. During validation another session
modified SSH and remote APIs, temporarily introducing unrelated compilation
errors: missing `PathBuf`, missing `ssh_store_path` initializers, and a
non-generic `AppHandle` command argument. Those files were not changed by this
repair.

An isolated verification worktree was created at
`/Users/indo/code/project/orca-lite-render-root-qa-20260908`, based on `799e0e4`,
with only this repair applied. The unrelated SSH command registration hunk in
`src-tauri/src/lib.rs` was explicitly excluded from that copy.

Final verification results:

- `cargo check --manifest-path src-tauri/Cargo.toml`: exit 0.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal`:
  156 passed.
- `native_terminal_surface_host_contract`: 18 passed.
- `native_terminal_renderer_contract`: 26 passed.
- `native_terminal_macos_background`: the real AppKit contract passed.
- The four related frontend suites (`NativeTerminalPane`,
  `nativeTerminalLifecycle`, `nativeTerminalVisibility`, and platform
  transparency): 163 passed.
- `bun run --cwd ui build`: exit 0 in both the shared and isolated trees.
- `bun test scripts/macos-dev-runner.test.mjs`: 2 passed in the final single run.
- `git diff --check`: clean.

The standalone renderer was executed against Apple M4 Max / Metal, generating
an 800 by 480 pixel PNG at `/tmp/ferryx-render-root-fix-20260908.png`.
Its 50-frame sample measured p50 3.590 ms and p95 7.063 ms. This is execution
evidence, not a before/after performance comparison or a live desktop visual
verdict.

The renderer contract initially lacked its expected standalone executable path.
The existing example was built, and its binary was linked at the test's expected
location in the isolated target directory. The existing tests were not changed.
The surface-host suite also caught a regression in the first cancellation
implementation: notifying the output channel on detach made a subscriber mistake
detach for processed output. Cancellation now has its own channel, and the
unchanged reattach contract passes.

The signer subprocess test hit Bun's default five-second timeout during
concurrent builds. It still awaits actual subprocess completion, with a bounded
30-second test budget and no sleeps, retries, or relaxed functional assertions.

Miri was attempted but stopped in a dependency constructor before the regression
ran: `_NSGetExecutablePath` is unavailable with Miri isolation enabled. This is
not a Miri pass. The native tests execute the actual AppKit and Ghostty APIs.
The LSP daemon was unreachable, so compiler diagnostics are the available Rust
diagnostic evidence. Independent agent review was unavailable due to provider
authentication and rate-limit errors.

## Debug execution

The existing `bun tauri dev` runner used ad-hoc signing and ignored signing
errors. To comply with the required development workflow, it now signs with
`Developer ID Application: Indo Yoon (5DUM8WPB4C)` and stops on a signing failure.
An isolated subprocess test verifies the actual signer arguments and that a
failed signature prevents application execution. It also checks the runner's
executable permission. Both cases passed after RED.

The repaired debug application was launched from the isolated tree using
exactly `bun tauri dev`; Vite reported `FERRYX_FRONTEND_READY`, and the GUI process
started as PID 26951. The assembled debug bundle passed
`codesign --verify --strict` with exit 0. Its signing authority is
`Developer ID Application: Indo Yoon (5DUM8WPB4C)`, team `5DUM8WPB4C`, identifier
`com.ferryx.app`. The existing production and debug daemons retained their older
start times; the debug daemon was still PID 91220.

No release build, installed application replacement, or daemon restart is part
of this repair.

## Desktop acceptance

Native compositor interaction must still be checked in the signed debug app:

1. Resize a terminal containing the affected agent in both dimensions. Its last
   complete frame should remain visible until the replacement is complete.
2. Stop resizing. The final content should appear without replaying incomplete
   synchronized frames from the top.
3. Scroll up deliberately, resize, and confirm that reading history is preserved.
4. Open and close Settings, switch tabs, split and close panes, and change the
   terminal theme. No uncovered area should reveal the desktop.
5. Verify overlays remain above terminal text and terminal input/IME still works.

Automated headless tests do not establish a visual pass for the user's live
desktop. The final report must keep this acceptance boundary explicit.
