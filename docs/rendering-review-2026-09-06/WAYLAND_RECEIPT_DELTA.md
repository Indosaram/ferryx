# D3 adjacent receipt and IME scale correction

## Observed integration gap

The geometry repair correctly stores Wayland scale 2 for raw WebView DPR 1.5
and derives physical glyph cell dimensions at that scale.
`NativeTerminalPane.updateImeAnchor` still divides returned `cellWidthPx` and
`cellHeightPx` by `scaleFactorRef.current`, populated from raw
`window.devicePixelRatio` in `measureGeometry`.

For cursor (2,3), physical cells 16x32 and effective native scale 2, the correct
local CSS anchor is (16,48), size 8x16. The current component at DPR 1.5 computes
(21.333...,64), size 10.666...x21.333....
The source receipt currently does not carry the effective presentation scale.
This is a source-proven integration gap in the new D3 path; its native pixels
have not been observed.

`sendMouse` was also read: it sends CSS-local positions, rather than dividing by
the frontend cell scale. Do not assert that mouse scale is broken from the IME
finding or rewrite mouse routing without its own concrete failing evidence.

## Owned delta

Extend the completed D3 workflow with one `wayland-ime-repair` node, then rerun
only its verifier with the delta evidence. The original geometry producer and
its captured REDs remain intact.

Allowed writes in the existing isolated repair worktree:

- `src-tauri/src/native_terminal/surface_host.rs` and its receipt tests.
- `src-tauri/src/ipc/native_terminal.rs`, restricted to the additive optional
  effective-scale receipt field, its manual converter, necessary `Eq` derive
  adjustment on float-containing receipts, and receipt serialization tests.
- `ui/src/components/NativeTerminalPane.tsx`.
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`.
- A directly affected native receipt contract test only if necessary.

The prior D2 producer has finished and committed `bce59b4`; retain all its owner
guards. Windows source is owned by D6 and renderer source by the separate atlas
worktree. This worker cannot stage/commit. The lead serializes Git operations.

## RED before delta production edits

Add a Rust receipt regression proving the effective scale comes from the actual
stored presentation bounds, including DPR 1.5 resolved to Wayland scale 2:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::receipt_reports_effective_presentation_scale -- --exact --nocapture
```

Add a real component/lifecycle regression with WebView DPR 1.5, effective native
scale 2 and physical cell dimensions 16x32:

```sh
bun run --cwd ui test -- src/components/NativeTerminalPane.lifecycle.test.tsx -t 'uses effective native scale for IME anchor at fractional webview density'
```

The assertion is on the actual focus-sink CSS anchor, not a duplicate formula.
Capture the current mismatch before changing production. Keep raw WebView DPR
as the geometry request input; consume the effective native scale for receipt
pixel-to-CSS conversion. An additive optional scale field can preserve the
existing response contract when no presentation geometry is available or the
debug frontend is temporarily paired with an older native host during HMR.
Do not round returned pixel metrics to manufacture agreement.

## GREEN and regression proof

Require the same tests GREEN, ordinary equal-scale behavior and legacy/no-scale
receipt handling, all focused D2 lifecycle tests, relevant D3 geometry/host
contracts, changed-file diagnostics and the UI/Rust checks affected by this delta.
Exercise the actual component in an isolated browser with the fractional-DPR /
native-scale boundary and retain its DOM action/geometry log and screenshot.
Label it as component/IPC proof, not native Wayland display evidence.

Avoid a new host-lock acquisition order when forming receipts: use stored
geometry already available at each existing receipt construction point.
Trace set-bounds, direct receipt, focus, input and mouse receipt paths.

Write `wayland-ime-repair.md` and D3 receipt-delta artifacts, preserving earlier
evidence. Native Wayland screenshots and interactive acceptance remain pending.

The first delta worker correctly identified that the internal host receipt is
not the serialized wire receipt. Its no-edit scope blocker is resolved by the
explicit IPC path above. Keep the original geometry producer cached; resume only
the receipt delta and its dependent verification. No IPC routing or mouse
coordinate rewrite is authorized.
