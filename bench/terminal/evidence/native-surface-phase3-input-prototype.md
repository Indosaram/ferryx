# Phase 3 Native Input and IME Prototype Evidence

**Captured:** 2026-08-24  
**Scope:** macOS desktop native terminal input and IME handling prototype  
**Status:** Incomplete; Phase 3 pending manual acceptance  

## Automated Refresh (2026-08-24)

### RED Evidence
- Plain ASCII textarea input was not forwarded.
- Ctrl/Alt/Meta character payloads such as `Ctrl+C` could not deserialize into Rust `KeyCode`.
- `Cmd/Ctrl+V` was prevented.
- Standalone modifier and lock keys were sent to IPC.
- Cancelled composition left stale input.

### GREEN Behavior
- Textarea `onInput` forwards ordinary text exactly once and clears.
- Composition commits only at `compositionend`.
- Browser-key DTO maps all current named keys plus one Unicode scalar and rejects unsupported multi-character keys.
- `Ctrl+C` frontend JSON deserializes and feeds through Ghostty encoder.
- `Cmd/Ctrl+V` remains unprevented for textarea input/paste path.
- Standalone modifiers and lock keys are suppressed.
- Blur clears and resets composition state.

### Verification Exact Counts
- `NativeTerminalPane` Vitest: 13/13.
- `native_terminal::input` tests: 5/5.
- Focused combined UI suite: 37/37.
- `cargo check --manifest-path src-tauri/Cargo.toml` passes with only four pre-existing unrelated dead-code warnings.
- `bun run --cwd ui build` passes.
- Touched `input.rs` `rustfmt --check` passes.
- An earlier LSP pass reported no diagnostics for `NativeTerminalPane.tsx` and
  `input.rs`; later fresh-diagnostics requests are recorded below.

## Pointer-focus regression repair (2026-08-25)

The user reported that the native Ghostty/WGPU pane accepted no input in the
running macOS app. The focused diagnosis found that the only pointer activation
path focused a 1px, `pointer-events-none` textarea but did not cancel WebKit's
default pointer activation. That can immediately blur the sink after it is
focused, leaving later keyboard and input events on the document instead of the
native-terminal IPC path.

### RED

`bun run --cwd ui test src/components/NativeTerminalPane.test.tsx` failed the
new retained-focus contract:

- the pane dispatched a cancelable `pointerdown`;
- `pointerEvent.defaultPrevented` was `false`;
- 12 existing tests passed and the new contract failed.

### GREEN

`NativeTerminalPane` now cancels the pointer default action before focusing its
hidden textarea. The regression contract asserts both that the event is
default-prevented and that the textarea remains `document.activeElement`.

- `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: 13/13
  passed.
- `bun run --cwd ui build`: passed.
- LSP reported no diagnostics for `NativeTerminalPane.tsx`.

This is an automated repair, **not** real-surface acceptance. The macOS native
pane must still visibly accept input in the running app before this failure is
considered resolved.

## Native IPC rejection reporting (2026-08-25)

The frontend previously discarded rejected Tauri invocations for native bounds,
focus, and input. That could make a real geometry or surface-host failure
indistinguishable from a dropped keystroke.

### RED

`bun run --cwd ui test src/components/NativeTerminalPane.test.tsx` failed the
new rejected-input contract: a rejected `cmd_native_terminal_send_input`
promise produced no `console.error` report.

### GREEN

Each native command now reports `Native terminal IPC command failed` with its
command name and error at the UI boundary.

- `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: 14/14
  passed.
- `bun run --cwd ui build`: passed.

Fresh LSP diagnostics for both touched TypeScript files exceeded the server's
3-second freshness deadline, but each response contained zero diagnostics and
the production `tsc` build completed successfully. This is a tooling
availability limitation, not an LSP clean pass.

## Dynamic bounds preserve local Ghostty state (2026-08-25)

The native surface host previously redrew `canonical_scenario()` for every
bounds update. After the first local input, a resize could therefore replace
the live Ghostty snapshot with the static demo instead of resizing the active
terminal state.

### RED

`cargo test --manifest-path src-tauri/Cargo.toml
native_terminal::input::tests::local_input_snapshot_survives_layout_resize`
failed to compile because `NativeTerminalInputState` had no
`snapshot_for_layout` operation.

### GREEN

`NativeTerminalInputState::snapshot_for_layout` now resizes an existing VT
instance, preserves its focus cursor style, and returns its current snapshot.
The surface host uses that snapshot on a bounds update, falling back to the
static scenario only before local input exists.

- `cargo test --manifest-path src-tauri/Cargo.toml native_terminal::input::tests`:
  6/6 passed. This compiles the updated surface host as part of the library.
- A broader `native_terminal_surface_host_contract` run is currently blocked
  by unrelated concurrent worktree changes: `WorktreeManager.git_backed` and
  `WorktreeError::NotAGitRepository` are referenced but absent.
- `rustfmt --check` reported only pre-existing import ordering in
  `surface_host.rs`; it reported no formatting diff for this repair's bodies.

This deterministic proof strengthens the resize path but does **not** prove
live tear-free split resizing.

## IME candidate-window cursor anchor (2026-08-25)

The hidden textarea used for WebKit IME events was permanently positioned at
the native pane origin. macOS candidate windows would therefore anchor at the
top-left of the pane rather than the rendered Ghostty cursor.

### RED

- `NativeTerminalPane`'s new cursor-anchor contract timed out because receipt
  data never changed the textarea's position.
- `cargo test --manifest-path src-tauri/Cargo.toml
  native_terminal::surface_host::tests::surface_receipt_carries_render_snapshot_cursor_geometry`
  failed because native surface receipts exposed no cursor geometry.

### GREEN

Native surface receipts and their camelCase Tauri IPC counterpart now include
the rendered cursor cell plus physical cell metrics. `NativeTerminalPane`
converts those physical values to CSS pixels using the current display scale
and places the transparent IME sink at that logical cell.

- `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: 16/16
  passed.
- `cargo test --quiet --manifest-path src-tauri/Cargo.toml
  native_terminal::surface_host::tests::surface_receipt_carries_render_snapshot_cursor_geometry`:
  passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed with four
  unrelated pre-existing dead-code warnings.
- `bun run --cwd ui build`: passed.
- `rustfmt --edition 2024 --check` on the two Rust files: passed.

Fresh LSP checks were clean for the two Rust files and the test file. The
frontend component server exceeded its freshness deadline but returned zero
diagnostics; the production TypeScript build passed.

This is not live IME acceptance. A real macOS CJK composition and candidate
window must still visibly follow the native cursor after typing and after the
pane moves.

The browser contract also verifies high-DPI conversion: at a 2x display scale,
a 10x20 physical-pixel native cell positions the IME sink as a 5x10 CSS-pixel
cell at the corresponding logical cursor coordinates.

## Live ASCII keyboard receipt (2026-08-25)

- User-supplied macOS artifact:
  `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-25-073459-4B3F8C54.png`
- SHA-256:
  `ea1354e927007f59de564c9689f6afb62ac71b274e60ef89aea1b711e0779c57`
- Observed result: after clicking the right native Ghostty/WGPU pane and
  typing `abc`, the native surface displayed `abc` with a block cursor.

This is real-surface evidence that click focus, ordinary text forwarding,
Ghostty state mutation, and a WGPU redraw all succeeded. It also shows that
the static demo snapshot was replaced by live terminal state. It does **not**
prove Enter, modified keys, paste, focus transitions, IME, resize, DPI, or
overlay behavior.

## Exit-gate evidence matrix (2026-08-25)

| Phase 3 exit requirement | Evidence held | Status |
| --- | --- | --- |
| React pane geometry and native rect are pixel-accurate | Static macOS screenshot plus `ResizeObserver` geometry tests | Partial: dynamic movement and resize still unproven |
| Split resizing has no tearing | None | Pending live scenario |
| Focus does not lose or steal | Retained-focus unit regression, typed focus IPC coverage, and one successful native click-to-text receipt | Partial: focus transitions still unproven |
| Modal/dialog layering strategy is proven | None | Pending live scenario |
| IME composition is possible | Browser composition lifecycle tests | Pending live macOS IME scenario |

The plan names macOS, Windows, and Linux X11/Wayland as target platforms.
Current real-surface evidence covers macOS only. This matrix records the
Phase 3 architecture gate, not cross-platform production acceptance; no later
phase may begin until the macOS gate scenarios above are manually accepted.

### Runtime Receipt
- The last successful `target/debug/ferryx` build was launched directly for the
  live receipt above. It exited cleanly after the user captured the result.
- A later `cargo tauri dev` rebuild was temporarily blocked by unrelated shared
  worktree edits; that build condition is not used as evidence for this pass.

## Pending Manual Phase 3 Gates

Automated suites do not replace live interactive verification. Phase 3 remains pending manual acceptance across the following explicit scenarios:

1. **Keyboard Input:** ASCII passed in the live receipt above. Enter, modified character keys such as Ctrl+C, and Cmd/Ctrl+V multi-line paste must still reach the local native-terminal prototype exactly once. This phase does not assert foreground-process signal behavior because daemon/PTY integration is later work.
2. **Standalone Modifiers:** Standalone modifier/lock key presses (Shift, Ctrl, Alt, Meta, CapsLock) suppress IPC dispatch without spurious characters.
3. **Dead Key:** Multi-keystroke accented character composition and dead key layouts.
4. **Korean/Japanese IME:** Candidate window positioning, inline preedit composition, and multi-stage candidate selection/commit.
5. **Focus:** Seamless click and keyboard focus transitions between native terminal panes and React web views.
6. **Resize/Split/DPI:** Dynamic window resize, multi-pane splits, and multi-monitor DPI scaling changes.
7. **Overlay/Modal Stacking:** Proper visual stacking and z-index ordering when React overlays, menus, palettes, or modal dialogs appear over native terminal surfaces.

Phase 3 status: Incomplete; pending manual acceptance.
