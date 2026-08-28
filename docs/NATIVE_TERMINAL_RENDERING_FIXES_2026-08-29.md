# Native terminal rendering fixes — 2026-08-29

Commit: `f157685` — *fix(terminal): render every SGR text attribute and keep PTY winsize in sync*

Two independent defects made the native terminal (libghostty-vt + wgpu) render
unfaithfully: five text attributes were dropped between the VT engine and the
GPU, and the PTY window size could fall out of step with the ghostty grid.

## Defect 1 — five SGR attributes never reached the screen

`cell_extractor` decoded only bold / italic / inverse / underline, while the
engine already exposed faint, blink, invisible, strikethrough and overline. The
renderer made it worse: `renderer/instances.rs` hashed `cell.underline` into the
row hash, but no code anywhere in `renderer/` emitted an underline rect, so even
the one decoration that *was* decoded could never be drawn.

Observable symptoms: faint text rendered identically to normal text,
strikethrough and overline drew no line at all, and text marked hidden
(`ESC[8m`) stayed fully readable — a genuine information-disclosure bug for
anything that relies on `ESC[8m` to mask a secret while typing.

### Fix

| Layer | Change |
|---|---|
| `native_terminal/cell_extractor.rs` | decode faint, blink, invisible, strikethrough, overline |
| `native_terminal/snapshot.rs`, `surface_snapshot.rs` | carry the new attributes on the cell snapshot and fold them into the row hash |
| `native_terminal/renderer/instances.rs` | emit underline / strikethrough / overline rects, blend faint foregrounds toward the cell background, skip glyph emission for invisible cells while keeping their background rect |

Folding the attributes into the row hash matters as much as decoding them: rows
are cached, so an attribute-only edit to an otherwise identical row would
otherwise reuse the stale instance buffer and never repaint.

## Defect 2 — PTY winsize could diverge from the ghostty grid

The grid is resized from the layout on **any** render path whenever
`terminal.dimensions() != (layout.cols, layout.rows)`
(`native_terminal/input.rs:213`, `:233`; `native_terminal/surface_host.rs:432`,
`:502`). But the PTY winsize was pushed from exactly one place —
`cmd_native_terminal_set_bounds` in `ipc/native_terminal.rs` — and even there the
result was thrown away:

```rust
let _ = daemon_client.resize_terminal(&session_id, receipt.cols, receipt.rows);
```

No other receipt-returning command propagated it: `cmd_native_terminal_attach`,
`cmd_native_terminal_set_focus`, `cmd_native_terminal_scroll`,
`cmd_native_terminal_mouse`. The UI drives `set_bounds` from a `ResizeObserver`
(`ui/src/components/NativeTerminalPane.tsx:759`, `:775`, `:895`) whose own
comment at `:213-214` records that those callbacks fire asynchronously and can
dispatch for a session the compositor already detached.

So whenever geometry changed through a path other than a delivered `set_bounds`
— or when a `resize_terminal` call simply failed — the grid reflowed while the
shell kept its old winsize. The shell's line editor then wrapped at a column
that no longer existed, producing mis-wrapped echo and stale cells to the right
of the output.

### Fix

Propagation is now compositor-owned: every ghostty grid creation or dimension
change emits its exact `(cols, rows)`, covering initial attach, ordinary render
resize, font-driven resize and live-session reattach. Requests are serialized
through a single dispatcher so resize ordering is preserved, and daemon or queue
failures are logged with the session id and dimensions instead of being
discarded. The one-off `set_bounds` call and its silent `let _ =` are gone.

## Why this was not caught earlier

The row-hash already referenced `underline`, which reads like attribute support
and hid the fact that nothing consumed it downstream. And the winsize bug is
invisible to unit tests that construct a grid directly, because the divergence
only exists between two components that no test previously observed together.

Both gaps are now pinned by tests rather than by inspection:

- `native_terminal::cell_extractor::tests::*` feed **real SGR bytes** into a live
  `NativeTerminal` (`ESC[2m`, `ESC[5m`, `ESC[8m`, `ESC[9m`, `ESC[53m`) and assert
  both the set and the reset (`22`, `25`, `28`, `29`, `55`) round-trip into the
  cell snapshot. These are engine-level, not synthetic style structs.
- `native_terminal::renderer::instances::tests::*` assert the underline rect
  lands in the lower band, strikethrough mid-cell, overline at the top, that an
  invisible cell emits zero glyph instances, that a faint foreground differs
  from a normal one, and that the row hash changes on an attribute-only edit.
- `native_terminal::surface_host::tests::ghostty_grid_resize_notifies_pty_with_matching_dimensions`
  pins the contract that a grid resize propagates the identical `(cols, rows)`
  to the PTY.

## Blink is decoded but deliberately not animated

`blink` (SGR 5) is decoded and folded into the row hash like every other
attribute, but no renderer code consumes it, so blinking cells draw as ordinary
text. That is intentional, not an oversight of the same kind as the underline
bug: animating it needs a timer-driven repaint loop the render-on-demand surface
host does not have, it would burn CPU and battery on an idle terminal, and
blinking text is discouraged for accessibility. The field carries a comment
saying so, because a decoded-yet-unconsumed attribute is precisely what made the
underline defect easy to miss.

## Verification performed

- `cargo test --lib`: **365 passed, 0 failed** (baseline before this work: 353).
- Every criterion was captured RED before GREEN. The winsize test's RED was a
  compile error (`E0599: no method named set_pty_resize_sink`), the attribute
  tests' RED were real assertion failures (e.g. *invisible cell produces zero
  GlyphInstances (left: 1, right: 0)*).
- LSP diagnostics: no errors on any of the nine changed files.
- Pixel evidence: `src-tauri/src/bin/terminal_attribute_evidence.rs` renders one
  row per attribute through the real wgpu offscreen renderer to a PNG. Inspection
  of the output confirms faint is visibly dimmer than normal, underline draws
  below the glyphs, strikethrough through them, overline above them, inverse
  swaps foreground and background, and the invisible row renders nothing.
- Release bundle rebuilt (`cargo tauri build`, exit 0) for in-app confirmation.

## Reproducing the evidence

```bash
# headless per-attribute PNG through the real renderer
cd src-tauri && cargo run --features native-terminal \
  --bin terminal_attribute_evidence -- /tmp/ferryx-attribute-evidence.png

# SGR probe to run inside a Ferryx native terminal pane
bash scripts/terminal-render-probe.sh
```

## Note on the typing verifier

`scripts/verify-terminal-typing.mjs` reports `VERDICT: FAIL` when
`/tmp/ferryx-switch-debug.jsonl` is stale — it analyses whatever trace exists,
even one written hours earlier by a previous bundle. Check the file's mtime
against the current time before treating that verdict as a regression; a trace
containing a single capture/sent pair is a dead trace, not a broken input path.

## Final rebuilt-release verification

The rebuilt macOS release app was relaunched after the bundle timestamp and the
probe was run in a real native terminal pane. Screenshot evidence:

`/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-29-075220-674D7BB7.png`

Inspection confirms every required observation:

- faint text is visibly dimmer than normal text;
- underline is below the glyphs;
- strikethrough crosses the glyphs;
- overline is above the glyphs;
- the concealed payload between the visible prefix and suffix is absent;
- the 200-column wrap stressor returns to a clean prompt without stale cells on
  the right side.

Typing was then rechecked against the relaunched daemon (PID 30199) with the
deterministic headless verifier. Its throwaway PTY ring advanced from sequence 8
to 54 after a 34-byte input burst (`+46`), and the probe session was closed.

Final checks after the release-app proof:

- `cargo test --lib`: **366 passed, 0 failed**;
- native-terminal language-server diagnostics: **0 errors**;
- renderer contract: **22 passed**;
- surface-host contract: **16 passed**;
- IPC hardening contract: **7 passed**;
- serialized daemon persistence contract: **9 passed**.
