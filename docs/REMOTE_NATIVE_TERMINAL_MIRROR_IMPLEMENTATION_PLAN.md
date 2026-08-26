# Remote / Mobile Native Terminal Mirror - Implementation Plan

**Date:** 2026-08-26
**Baseline commit:** 93dcaab (working tree dirty with unrelated `.omo` journal churn)
**Baseline evidence:** `cargo test --manifest-path src-tauri/Cargo.toml --lib remote` -> 29 passed, 0 failed

## Problem

Remote/mobile can already reach a live Ferryx terminal, but it cannot *see* it correctly.

Verified facts:

1. The desktop terminal is native Ghostty (`src-tauri/vendor/ghostty` submodule, FFI in
   `src-tauri/src/native_terminal/`) plus a WGPU renderer. xterm.js is gone: zero references in
   `ui/src` and `ui/package.json`.
2. The remote gateway runs inside the daemon process (`RemoteGatewayManager::from_daemon`) and
   shares the daemon's `Arc<TerminalService>`, so PTY bytes and ring-buffer replay are already
   reachable from Axum.
3. `/api/v1/terminal/{sessionId}` sends **Binary** frames only: `ESC ] 777 ; ferryx ; {json} BEL`
   metadata envelope followed by raw PTY bytes.
4. `ui/src/remote/RemoteTerminal.tsx` appends those raw bytes into a `<pre>` with no VT
   interpretation. Cursor motion, alt-screen, and SGR sequences land as literal text, so any
   full-screen TUI (agent CLIs, vim, htop) is unreadable.
5. Nothing sends `resize`. `WebSocketTerminalTransport.resize` exists but only its own test calls
   it, so the mobile viewport is stuck with the desktop grid geometry.
6. `RenderSnapshot` / `CellSnapshot` / `CursorSnapshot` / `ColorRgb` already derive
   `Serialize + Deserialize`, and `capture_render_snapshot` is pure VT FFI with no GPU dependency.
7. `src-tauri/tauri.conf.json` has no `bundle.resources`, so `resolve_dist_dir()` finds no
   `Resources/ui/dist` in a packaged app and the gateway serves `EMBEDDED_FALLBACK_HTML`.

## Decision

Do **not** reintroduce a client-side VT emulator. The authoritative parser is already server-side:
Ghostty. Add a headless Ghostty mirror inside the remote gateway, stream serialized grid state, and
render that grid in the remote client.

Consequences: desktop and remote observe the same parser output, CJK width is decided server-side by
`CellWide`, and the browser bundle gains no terminal dependency.

## Scope

In scope:

- **A** Server-side headless Ghostty mirror + JSON grid frames over the existing WebSocket.
- **B** Remote client grid renderer replacing the append-only `<pre>`.
- **C** Client-driven `resize` so the mirrored grid matches the phone viewport.
- **D** `bundle.resources` so a packaged app actually serves the remote SPA.
- **E** `?render=grid` opt-in that suppresses redundant binary PTY frames for grid clients.

Out of scope (explicitly unchanged):

- The Active Desktop Lock. `ws_terminal_handler` still 403s any session that is not
  `active_selection.session_id`, and `focus_watcher` still disconnects on desktop focus change.
  Remote stays a follower mirror, not an independent multi-session client.
- Remote scrollback. `RenderSnapshot` is viewport-only; mobile scrollback is a later feature.
- Mouse reporting from remote.
- Any change under `src-tauri/src/native_terminal/` or `src-tauri/vendor/ghostty`.

## Wire contract v1 (binding for both lanes)

Server -> client uses WebSocket **Text** frames for grid state. Client -> server Text frames remain
`ClientControlMessage`. Existing Binary frames are unchanged, so an older client keeps working.

```
Full frame (sent on attach, on resize, and after a replay gap):
{"type":"grid","cols":120,"rows":30,"cursor":{...},"lines":[Line,...]}

Diff frame (sent when only some lines changed):
{"type":"gridDiff","cols":120,"rows":30,"cursor":{...},"lines":[Line,...]}

Cursor:
{"x":5,"y":2,"visible":true,"blinking":false,"wideTail":false,
 "visualStyle":"bar"|"block"|"underline"|"blockHollow"}

Line:
{"index":0,"runs":[Run,...]}

Run:
{"text":"hello ","fg":[255,0,0]|null,"bg":[0,0,0]|null,"attrs":0}
```

Rules:

- `attrs` is a bitmask: `1` bold, `2` italic, `4` underline, `8` inverse.
- A run is a maximal span of horizontally adjacent cells with identical `fg`, `bg`, and `attrs`.
- A `CellSnapshot` with empty `text` contributes one space to its run.
- `CellWide::SpacerTail` cells are skipped: the wide grapheme already occupies its own cell, so a
  CJK line yields run text `"한글"`, never embedded padding.
- A trailing run that is all spaces with `fg: null`, `bg: null`, `attrs: 0` is omitted; the client
  pads the remaining columns with blanks.
- `gridDiff.lines` carries only lines whose runs changed since the last frame sent on that socket.
  A frame with unchanged lines but a moved cursor is a `gridDiff` with `lines: []`.
- Cursor `visualStyle` is lower-camel on the wire. Remote owns its own DTOs; engine types are
  mapped, never re-annotated.

## Lane A - backend (`src-tauri/`)

Files owned: `src-tauri/src/remote/mirror.rs` (new), `src-tauri/src/remote/protocol.rs`,
`src-tauri/src/remote/server.rs`, `src-tauri/src/remote/tests.rs`, `src-tauri/tauri.conf.json`.

1. `mirror.rs`: `RemoteTerminalMirror` owning one `NativeTerminal` (via the `TerminalEngine` trait)
   plus the last emitted line runs.
   - `new(cols, rows)`.
   - `feed(&mut self, bytes: &[u8]) -> Result<GridFrame, _>`: feed the VT stream, capture
     `render_snapshot()`, build runs, diff against the previous state, return a full or diff frame.
   - `resize(&mut self, cols, rows) -> Result<GridFrame, _>`: resize the engine (pixel metrics are
     nominal, e.g. 8x16, because remote does not rasterize), clear the diff baseline, return a full
     frame.
   - `full_frame(&mut self)` for attach and post-gap resync.
   - Deterministic and synchronous: no timers, no async, unit-testable directly.
2. `protocol.rs`: wire DTOs `RemoteGridFrame`, `RemoteGridLine`, `RemoteGridRun`,
   `RemoteGridCursor`, serialized exactly as the contract above.
3. `server.rs`:
   - Create the mirror in `handle_terminal_socket` sized from `PtySession::get_size()`, shared as
     `Arc<Mutex<RemoteTerminalMirror>>` between the send and recv tasks.
   - Feed the attach replay history into the mirror, then send the full grid frame before streaming.
   - Feed every subsequent `OutputChunk` and send the resulting frame, coalescing to at most one
     frame per ~33 ms so a burst of PTY chunks cannot spam the socket.
   - On replay-gap recovery, reset the mirror, feed the recovered snapshot, send a full frame.
   - `ClientControlMessage::Resize` resizes both the PTY (existing behavior) and the mirror, then
     sends a full frame.
   - Accept `?render=grid`; when present, skip the Binary output frames and send grid frames only.
     Without it, behavior is byte-identical to today.
4. `tauri.conf.json`: add `bundle.resources` mapping `../ui/dist` to `ui/dist` so the packaged app
   contains the SPA that `resolve_dist_dir()` probes at `Resources/ui/dist`.

Tests (lane A owns them):

- Unit, in-module: plain text run; SGR bold+red produces `attrs: 1` and `fg: [r,g,b]`; CJK line
  yields `"한글"` with no spacer artifact; a second feed touching one line emits a diff carrying
  only that line index; resize emits a full frame with the new `cols`/`rows`; cursor position after
  `ESC[2J ESC[H`.
- Integration, in `tests.rs`, reusing the existing raw-TCP WebSocket helper: attach to the active
  session and assert the first Text frame parses as `"type":"grid"` with the session's dimensions.
  Bound the wait with `tokio::time::timeout`; never sleep to synchronize.
- Green: `cargo test --manifest-path src-tauri/Cargo.toml --lib remote`.

## Lane B - frontend (`ui/`)

Files owned: `ui/src/remote/terminalGridProtocol.ts` (new),
`ui/src/remote/terminalGridProtocol.test.ts` (new), `ui/src/remote/RemoteTerminal.tsx`,
`ui/src/remote/RemoteTerminal.contract.test.tsx`, `ui/src/remote/RemoteUI.test.tsx`.

1. `terminalGridProtocol.ts`: pure, framework-free types plus
   `parseGridFrame(text: string)` and `applyGridFrame(state, frame)` returning the next grid state
   (full frame replaces, diff frame patches by line index), and an attrs-bitmask decoder.
2. `RemoteTerminal.tsx`:
   - Parse Text frames through the protocol module; ignore Binary frames for rendering.
   - Render the grid as rows of styled spans inside a `white-space: pre` monospace surface using the
     existing `useTerminalSettings` theme (font family, size, fg/bg), with `fg`/`bg`/bold/italic/
     underline/inverse applied per run and a cursor overlay at `cursor.x/y` when visible.
   - Append `?render=grid` to the WebSocket URL.
   - Measure one monospace cell, derive `cols`/`rows` from the surface box, and send
     `{"type":"resize","cols":N,"rows":M}` on mount and on container resize (ResizeObserver),
     skipping sends when the derived geometry is unchanged.
   - Keep the existing key/paste encoding and `MobileKeyDock` behavior exactly as-is: those
     assertions in `RemoteTerminal.contract.test.tsx` stay green.
3. Tests: protocol unit tests (parse, diff patch, attrs decode, wide text preserved); a component
   test that a `grid` Text frame renders its cell text and a following `gridDiff` patches only that
   line; a resize test that asserts the exact resize JSON was sent given a stubbed cell measurement
   and container box. Drive async behavior through `waitFor` on the rendered result or an explicit
   event, never a fixed delay.
4. Green: `bun run --cwd ui test` and `bunx tsc --noEmit -p ui/tsconfig.json`.

## Verification gate

Run after both lanes report terminal:

1. `cargo test --manifest-path src-tauri/Cargo.toml --lib remote` -> exit 0.
2. `cargo test --manifest-path src-tauri/Cargo.toml --lib mirror` -> exit 0.
3. `bun run --cwd ui test` -> exit 0.
4. `bun run --cwd ui build` -> exit 0 (runs `tsc` then `vite build`).

Anything red goes back to the owning retained Web scope via `--resume-scope`, never to a local
rewrite.

## Cross-lane seam coverage (added after the lanes landed)

The lanes were implemented in isolation, so each proved only its own half: lane A asserted Rust
structs, lane B asserted hand-written JSON fixtures. A casing or enum-spelling mismatch between them
would pass both suites and still render a blank remote screen. Two non-lane files close that seam:

- `src-tauri/examples/remote_grid_frame_dump.rs` drives the real `RemoteTerminalMirror` and emits
  seven serialized frames as NDJSON: ascii, sgr_bold_red, cjk, cursor_addressed, diff, resize_full,
  full_resync.
- `ui/src/remote/gridFrameSeam.check.ts` runs that example and feeds every emitted frame through the
  real `parseGridFrame` / `applyGridFrame` / `decodeGridAttrs`. It is a standalone script, not a
  vitest case, and runs in the `rust-check` CI job: the `ui-check` job has neither the Rust toolchain
  nor `submodules: recursive`, so a vitest version would have skipped silently in CI while reporting
  green. Invocation: `bun run --cwd ui src/remote/gridFrameSeam.check.ts` (exit 0 on pass, non-zero
  with named failures otherwise).

Confirmed against real backend output: `wideTail` and `visualStyle` are camelCase, `visualStyle`
values are lower-camel (`"block"`), `fg` is a 3-element array (`[204,102,102]`), a CJK line arrives
as `"한글 테스트"` with no spacer padding, and the diff frame carries only the changed line index.

## Known coverage gap (open follow-up)

All seven success criteria pass, but a coverage audit found the grid WebSocket has exactly ONE
integration test - `test_grid_render_attach_sends_full_frame_with_session_dimensions`, covering the
attach path only. Three behaviors a real mobile client exercises every session have no integration
coverage in `src-tauri/src/remote/tests.rs`:

1. **Live output after attach.** The 33 ms coalescing loop (`server.rs`, `frame_interval`) is what
   forwards ongoing PTY output. Untested: a regression there shows the initial snapshot and then
   freezes, while every current test still passes.
2. **Client resize over the socket.** Sending `{"type":"resize","cols":N,"rows":M}` must yield a FULL
   `grid` frame reporting exactly the requested geometry. This is the path that makes the phone
   viewport authoritative; only `mirror.rs`'s unit test covers resize, not the socket round trip.
3. **Legacy mode stays binary.** Attaching WITHOUT `render=grid` must deliver Binary frames and no
   grid Text frame. The backward-compatibility guarantee currently rests on reading the `render_grid`
   branch, not on a test that fails if grid frames are ever made unconditional.

Each test must bound its wait with `tokio::time::timeout` and await the exact expected frame; fixed
sleeps would make a coalescing-loop test pass by timing luck. The ready-to-dispatch worker prompt is
kept in the repository at `docs/remote-grid-socket-coverage-followup.md`.

A local probe attempt (a non-lane `examples/` binary driving the real socket) was abandoned after five
failed diagnoses of a `ConnectionReset` in the probe harness itself; the same code path passes in
`test_grid_render_attach_sends_full_frame_with_session_dimensions` on every run, so the fault was in
the scratch harness, not the product. The scratch files were deleted rather than left in the tree.
Writing these tests directly is out of bounds for the coordinator: they belong to the backend lane.

This is additive hardening, not a regression: shipped behavior is verified by 36 remote tests, 6
mirror unit tests, the cross-lane seam check, and the two Active Desktop Lock tests.

## Manual E2E (requires the user; not automatable here)

1. Desktop: Settings -> Remote -> enable, scan the QR from a phone on the same LAN.
2. Open an agent CLI or `htop` in the focused desktop terminal.
3. Phone should show the same full-screen layout, not escape-sequence soup, and reflow to the phone's
   own column count.
4. Switching the focused desktop terminal should still disconnect the phone (Active Desktop Lock
   intact).

## Delegation

Implementation runs through scoped ChatGPT Web workers under a native DAG:

- One helper batch (`--batch-stdin --json --progress-json`) dispatches lane A and lane B with the
  disjoint file ownership above; both prompts carry the wire contract verbatim, so the lanes never
  negotiate it.
- A verification node depends on the batch and runs the four commands above locally.
- Failures are repaired by resuming the exact retained scope, and scopes are closed only after the
  verifier accepts.
