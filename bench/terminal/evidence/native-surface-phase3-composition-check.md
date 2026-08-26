# Phase 3 Native Surface Composition Check — Partial Evidence

**Captured:** 2026-08-24  
**Scope:** macOS desktop, one active native terminal rectangle  
**Status:** partial pass; Phase 3 exit gate remains open

## Fresh real-surface artifact

- User-supplied screenshot:
  `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-24-225928-627815F0.png`
- SHA-256:
  `6e2cd01b8cf7f5e8c8f27434289d322bdee346ae5c0c0f4db640365cb5c16223`
- Launch command: `cargo tauri dev`
- Observed launch result: `Running target/debug/ferryx`
- The former macOS transparent-window error did not recur after enabling
  `app.macOSPrivateApi: true`.

## Observed result

The screenshot shows the canonical WGPU terminal scenario through the
transparent WebView, clipped to the active right-side terminal pane:

- the terminal rectangle is nonblank and aligns beneath the tab bar and right
  of the sidebar;
- native output does not cover the sidebar or tab chrome;
- ANSI styling, selection/cursor treatment, and the supplied Unicode/CJK row
  are visible;
- no opaque WebView rectangle or native-surface bleed is visible.

Two independent read-only visual reviews returned `PASS` for this static
single-pane composition state.

## Deliberately unaccepted requirements

This artifact does **not** prove dynamic resize/tearing, focus, keyboard,
IME composition, modal/overlay stacking, or multi-pane clipping. Those
criteria require separate live interaction evidence, so Phase 3 must not be
accepted and Phase 4 must not begin from this static composition artifact.

## Cleanup receipt

The app remains running in the `cargo tauri dev` session for follow-up
verification; no desktop UI input was automated.
