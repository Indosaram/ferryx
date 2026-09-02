# Native Terminal File Drag-Drop Fix (2026-09-02)

## Symptom

Dropping a file from Finder onto a terminal pane did nothing ("파일 드래그해서 프롬프트 창에 넣는거 작동 안 함").

## Root cause

macOS receives the drop event, but the pane hit test divided the coordinate by the wrong scale:

- wry 0.55.1 (macOS): `WryWebView` implements `NSDraggingDestination` and forwards AppKit
  `draggingLocation()` **verbatim — logical points**, only frame-flipped, no backingScaleFactor
  conversion (`wry-0.55.1/src/wkwebview/drag_drop.rs:40-43`).
- tauri-runtime-wry 2.11.4 wraps that payload as `PhysicalPosition` regardless
  (`tauri-runtime-wry-2.11.4/src/lib.rs:4872`).
- The drop handler added in `58fc612` treated the payload as physical pixels and divided by
  `window.devicePixelRatio`. On Retina the coordinate was halved, so any drop with real
  x < 2 x pane.left was rejected by the bounds check — roughly the left half of every pane,
  plus vertical halving for panes low in the window.

Windows (`ScreenToClient` → real device pixels, `wry webview2/drag_drop.rs:234-242`) and Linux
do send device pixels, so the conversion is correct there — the fix is platform-conditional.

Disproven during investigation (do not re-derive): "transparent:true disables drag-drop on
macOS". No such gate exists in wry 0.55.1 or tao 0.35.3 sources; `registerForDraggedTypes` is
unconditional on the tao window and `NSDraggingDestination` unconditional on WryWebView.

## Fix

1. `src-tauri/src/native_terminal/platform/macos.rs` — `FerryxNativeTerminalView::
   window_backing_scale_factor()` reads the host window's `backingScaleFactor` (1.0 fallback);
   surfaced through `MacosCompositorTarget` and `PlatformCompositorTarget`. This is the
   authoritative scale source if wry ever starts sending real physical pixels.
2. `ui/src/components/NativeTerminalPane.tsx` — exported `dragDropPositionToLogical(position,
   devicePixelRatio, isMacos)`: macOS divides by 1, Windows/Linux by DPR. The drop handler uses
   it with `isMacShortcutPlatform()`. A `terminal.surface.drop.event` switchDebug trace now logs
   every received drop (position + pathCount) to distinguish "event never arrives" from
   "bounds rejected".
3. `src-tauri/src/lib.rs` — `on_window_event` logs every backend DragDrop event to
   `/tmp/ferryx-switch-debug.jsonl` as `terminal.surface.dragdrop.window_event` (debug builds or
   `FERRYX_SWITCH_DEBUG=1`), proving AppKit → tao → tauri delivery.

## Verification

- `cargo test --test native_terminal_drag_drop_coordinates` — 2/2 (new contract: getter falls
  back to 1.0 without a window; raw(400,200)/2x => logical(200,100)).
- `cargo test --lib` — 505 passed.
- `ui`: `bun run build` (tsc + vite) clean; `NativeTerminalPane.test.tsx` 93/93 including the
  regression discriminator raw(19,100): pre-fix code misread it as (9.5,50) and wrongly rejected
  a drop inside the pane; post-fix pastes it.
- Full UI suite: 1322/1323. The single failure (`workspaceThemeContract > keeps pane-local hover
  chrome at the reference contrast`) is pre-existing: uncommitted working-tree change removed
  `backdrop-blur-md` from the `pane-toolbar` className in `TerminalSplitView.tsx` without
  updating the contract test. Not touched by this fix.

## Manual E2E (user, `bun tauri dev`)

Drag a file from Finder onto a terminal pane — the quoted path should paste into the PTY,
including drops near the left/top edges of panes. If it still fails, read
`/tmp/ferryx-switch-debug.jsonl`:

- `terminal.surface.dragdrop.window_event` present, `terminal.surface.drop.event` absent → the
  drag reached the window but the drop landed outside every pane (or a listener was torn down).
- Neither present → the drag session never reaches the window (AppKit-level; next suspect would
  be the native child surface's drag hit-test, currently believed transparent via `hitTest: nil`).
