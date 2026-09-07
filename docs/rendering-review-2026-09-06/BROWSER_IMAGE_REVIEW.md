# Browser harness image review

An image-capable reviewer (`st_01a078fa`,
`quotio/gemini-3.8-flash-high`) opened all six named PNGs with `read`.
The lead's model could not view image attachments; this report attributes the
pixel observations to that reviewer rather than claiming lead visual inspection.

## Inspected images

Paths are relative to `.omo/evidence/ulw/rendering-review-20260906/`.

- `D2/browser-replacement.png`: visible title identifies the real
  NativeTerminalPane/lifecycle harness; owner `browser-b`, visible true,
  replacement scenario. Reviewer observed no stale alert or clipping.
- `D2/browser-hidden.png`: owner `browser-a`, visible false, hidden scenario.
  Reviewer observed no stale alert or clipping.
- `D2/browser-recovery.png`: owner `browser-b`, visible true, recovery scenario.
  Reviewer observed no stale alert or clipping.
- `D3/receipt-delta/redo/browser-fractional.png`: readable receipt/IME harness,
  red focus-sink highlight at the upper-left content region; no observed
  clipping or warning.
- `D3/receipt-delta/redo/browser-ordinary.png`: red highlight visually matches
  fractional in size/position; no observed clipping or warning.
- `D3/receipt-delta/redo/browser-legacy.png`: red highlight is visibly larger
  and offset relative to fractional/ordinary, consistent with the documented
  legacy conversion; no observed clipping.

## Verdict and boundary

The reviewer reported all six images readable, with no unexpected stale alert
or clipping in the harness. Exact anchor coordinates come from the separately
captured DOM assertions, not visual estimation.

Blank terminal regions are intentional: these are real React components with
controlled IPC, not native compositor output. The red sink is QA-only CSS.
This review does not approve native terminal text, IME candidate windows,
Wayland buffer presentation, Windows z-order or physical display transitions.
The native screenshot requirements remain pending.
