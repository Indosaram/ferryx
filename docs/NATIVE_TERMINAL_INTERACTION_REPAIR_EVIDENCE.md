# Native Terminal Interaction & macOS Child-Frame Repair Evidence

## Objective
Recover from the syntax failure in `src-tauri/src/native_terminal/surface_host.rs`, enforce physical AppKit child `NSView` geometry scoping to the active terminal viewport rather than full-window coverage, verify pointer-transparent hit testing and WGPU drop-order safety, and establish comprehensive RED→GREEN contract verification.

---

## Exact Source Seams & Files
- `src-tauri/src/native_terminal/surface_host.rs`: Restored valid Rust syntax (eliminated escaped `\n` tokens and unescaped quote corruptions in `NativeTerminalSurfaceHost` and test seams), resolved missing `PhysicalBounds` import, ensured `update_viewport(None)` zeroes/hides child view when no active layout exists, and preserved strict field drop ordering (`surface` -> `target` -> `renderer`).
- `src-tauri/src/native_terminal/composition.rs`: Pure coordinate conversion implementation in `LogicalBounds::to_appkit_frame(superview_height, is_superview_flipped)` converting DOM top-left coordinates `(x, y, w, h)` to AppKit bottom-left coordinates `(x, superview_height - (y + h), w, h)` or flipped top-left coordinates `(x, y, w, h)`.
- `src-tauri/src/native_terminal/platform/macos.rs`: Layer-backed, pointer-transparent `FerryxNativeTerminalView` initialized with zero frame and hidden state, without full-window autoresizing masks, and frame/visibility dynamically synchronized on main thread via `update_viewport`.
- `src-tauri/tests/native_terminal_composition_contract.rs`: Geometry contract `test_child_view_frame_strictly_scoped_to_terminal_viewport_without_covering_chrome` asserting exclusion of tab bar, sidebar, pane split controls, and bottom window areas.
- `src-tauri/tests/native_terminal_surface_host_contract.rs`: Surface host and session attachment contracts verifying child view target requirements and active layout cleanup.
- `src-tauri/tests/native_terminal_input_boundary_contract.rs`: Production input boundary lifecycle contracts rejecting invalid and detached sessions without phantom state.

---

## Independently Observed Root Cause
Previously, native child NSViews were initialized at `content_view.bounds` with full-window coverage and autoresizing. Even though `SurfaceCompositionLayout` correctly constrained WGPU rasterization to the terminal viewport, the child NSView physically occupied full-window coordinates. By decoupling child NSView frame positioning from window bounds and binding `FerryxNativeTerminalView.setFrame` exclusively to `bounds.to_appkit_frame(superview_height, is_flipped)`, the child view is physically restricted to the terminal rectangle and zeroed/hidden when detached or unassigned.

---

## Parent-Captured RED→GREEN Mutation Evidence

### 1. Child-Frame Viewport Coordinate Conversion Mutation
- **Mutation Applied**: In `LogicalBounds::to_appkit_frame` (`src-tauri/src/native_terminal/composition.rs`), unflipped branch mutated to return `self.y` instead of `superview_height - (self.y + self.height)`.
- **Captured RED Failure**:
  ```
  thread 'test_child_view_frame_strictly_scoped_to_terminal_viewport_without_covering_chrome' panicked at tests/native_terminal_composition_contract.rs:384:5:
  assertion `left == right` failed
    left: 38.0
   right: 162.0
  ```
- **Restoration & GREEN Outcome**:
  Restored exact formula `superview_height - (self.y + self.height)`.
  ```
  running 7 tests
  test test_child_view_frame_strictly_scoped_to_terminal_viewport_without_covering_chrome ... ok
  test test_composition_bounds_valid_conversion_and_grid_calculation ... ok
  test test_composition_sub_rectangle_preserves_nonzero_origin ... ok
  test test_viewport_containment_and_interaction_boundary_seam ... ok
  test test_composition_bounds_validation_rejects_zero_and_negative_dimensions ... ok
  test test_composition_bounds_validation_rejects_non_finite_values ... ok
  test test_composition_bounds_validation_rejects_excessive_dimensions ... ok

  test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  ```

### 2. React Native-Surface Ownership Mutation
- **Mutation Applied**: Three current production seams were deliberately regressed:
  1. The outer `NativeTerminalPane` pointer activation handler again called
     `event.preventDefault()`.
  2. `DIALOG_SELECTOR` matched only the Settings dialog rather than every
     mounted `[role="dialog"]` surface.
  3. `TerminalSplitView` passed `headerHovered={false}` instead of its existing
     16px top-hotspot state.
- **Captured RED Failure**:
  ```
  Test Files  3 failed (3)
       Tests  4 failed | 44 passed (48)
  ```
  The failures were exactly the pointer-default contract, header-hover propagation
  contract, and both Settings/New Tab dialog detach-and-input-block contracts.
- **Restoration & GREEN Outcome**: The three expressions were restored to:
  `inputRef.current?.focus()` without pointer default prevention,
  `DIALOG_SELECTOR = '[role="dialog"]'`, and
  `headerHovered={isHoveredTop}`. The focused suite then passed:
  ```
  Test Files  3 passed (3)
       Tests  48 passed (48)
  ```

### 3. Detached Native-Session Input-Guard Mutation
- **Mutation Applied**: `encode_attached_native_input` in
  `src-tauri/src/ipc/native_terminal.rs` was temporarily changed to call
  `state.encode_input(session_id, input)` directly, bypassing its
  `snapshot_for_session` attachment guard.
- **Captured RED Failure**:
  ```
  running 3 tests
  test production_input_boundary_rejects_invalid_session_id ... ok
  test production_input_boundary_rejects_unattached_session_without_creating_native_state ... FAILED
  test production_input_boundary_rejects_detached_session_without_resurrecting_native_state ... FAILED

  test result: FAILED. 1 passed; 2 failed
  ```
  The failed assertions prove that an unattached or detached native session would
  otherwise create local VT state instead of rejecting input.
- **Restoration & GREEN Outcome**: Restored the
  `snapshot_for_session(session_id)?.is_none()` check returning
  `NativeTerminalError::NoValue` before calling `state.encode_input`.
  ```
  running 3 tests
  test production_input_boundary_rejects_invalid_session_id ... ok
  test production_input_boundary_rejects_unattached_session_without_creating_native_state ... ok
  test production_input_boundary_rejects_detached_session_without_resurrecting_native_state ... ok

  test result: ok. 3 passed; 0 failed
  ```

---

## Existing Lifecycle / Input RED→GREEN Contract Evidence

### Surface Host Contracts (`native_terminal_surface_host_contract.rs`)
- `native_terminal_host_composition_target_must_be_platform_child_view_not_root_window` (ok)
- `native_surface_request_rejects_missing_session_id` (ok)
- `native_surface_request_preserves_session_and_nonzero_viewport` (ok)
- `native_session_handles_replay_gaps_and_session_cleanup_deterministically` (ok)
- `native_terminal_active_session_detach_resets_host_layout_and_preserves_isolation` (ok)
- `native_session_renders_bytes_from_daemon_attachment_history_and_live_stream` (ok)
- **Result**: 6 passed, 0 failed.

### Input Boundary Contracts (`native_terminal_input_boundary_contract.rs`)
- `production_input_boundary_rejects_invalid_session_id` (ok)
- `production_input_boundary_rejects_unattached_session_without_creating_native_state` (ok)
- `production_input_boundary_rejects_detached_session_without_resurrecting_native_state` (ok)
- **Result**: 3 passed, 0 failed.

### Native Terminal Unit Tests (`cargo test --lib native_terminal`)
- `surface_host_drop_order_guarantees_surface_drops_before_target` (ok)
- `surface_receipt_carries_render_snapshot_cursor_geometry` (ok)
- `native_surface_timeout_drops_frame_without_terminal_failure` (ok)
- `key_event_input_advances_terminal_state` (ok)
- `local_input_snapshot_survives_layout_resize` (ok)
- `local_ime_input_updates_real_terminal_snapshot_with_wide_cells` (ok)
- `ctrl_c_frontend_payload_deserializes_and_encodes_control_character` (ok)
- `parse_browser_key_handles_named_keys_and_unicode_and_rejects_unsupported` (ok)
- `key_modifiers_deserialize_from_camel_case_json` (ok)
- `key_event_deserialize_from_json` (ok)
- `native_terminal_input_deserializes_from_frontend_payloads` (ok)
- **Result**: 11 passed, 0 failed.

---

## Machine-Gate Commands & Outcomes

1. `cargo check --manifest-path src-tauri/Cargo.toml`
   - Outcome: Finished with 0 errors and exactly 4 pre-existing project warnings.
2. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_composition_contract`
   - Outcome: 7 passed, 0 failed.
3. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`
   - Outcome: 6 passed, 0 failed.
4. `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_input_boundary_contract`
   - Outcome: 3 passed, 0 failed.
5. `cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal`
   - Outcome: 11 passed, 0 failed.
6. `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/components/TerminalSplitView.test.tsx`
   - Outcome: 3 files / 48 tests passed after the parent-captured UI RED mutation was restored.
7. `bun run --cwd ui test`
   - Outcome: 82 files / 634 tests passed.
8. `bun run --cwd ui build`
   - Outcome: Frontend TypeScript/React build passed cleanly with 0 errors.
9. `Language Server Diagnostics`:
   - Outcome: No diagnostics across touched UI source/test files and the native IPC,
     macOS compositor, and surface-host Rust sources.

---

## Pre-Existing Cargo Warnings (Expected Baseline)
The build emits exactly 4 pre-existing warnings in unrelated modules:
1. `warning: method wait_and_reap is never used` (`src/terminal/session.rs:266:19`)
2. `warning: struct WriterLeaseGuard is never constructed` (`src/worktree/manager.rs:57:19`)
3. `warning: associated items new, canonical_path, and owner_id are never used` (`src/worktree/manager.rs:64:8`)
4. `warning: method acquire_writer_lease is never used` (`src/worktree/manager.rs:285:19`)

---

## Manual Desktop QA Checklist

- [ ] **[PENDING] Scenario 1: Exact Native Terminal Input/Output, Navigation & Chrome Isolation**
  1. **Click & Active Input/Output**: Click into the active native terminal pane; execute `printf 'native bridge ok\n'`. Confirm output text renders correctly via the native GPU surface and cursor updates.
  2. **Settings Modal Occlusion / Detach**: Open Settings dialog. Confirm terminal pixels/cursor are zeroed / occluded (`update_viewport(None)` hides the child NSView) and typing produces no command side-effects in the underlying terminal.
  3. **Return & Focus Resume**: Close Settings dialog and return to the terminal pane. Repeat `printf 'native bridge ok\n'` command; verify execution succeeds and rendered output continues seamlessly.
  4. **New Tab & Pane Split Routing**: Click "+" (New Tab) button in the tab bar and split pane controls. Verify all clicks immediately register and trigger UI actions without pointer interception from the native child view.
