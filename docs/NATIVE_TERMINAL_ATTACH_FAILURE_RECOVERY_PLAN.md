# Native Terminal Attach Failure Recovery Plan

## Symptom Analysis
- User occasionally observes a red error banner: `Failed to attach native terminal` in the bottom-right corner of a terminal pane.
- When splitting the tab (creating a horizontal/vertical pane split), the failed terminal immediately comes back to life.

## Cause Chain & Mechanism

1. **Failure Threshold & Exhaustion** (`ui/src/components/NativeTerminalPane.tsx`):
   - `attemptAttach` uses an exponential backoff retry loop (0 to 5 retries: 250ms, 500ms, 1000ms, 2000ms, 4000ms).
   - `bannerRetryThreshold = 2`: Retries 0 and 1 are silent (~750ms). If failure persists through retry 2, `setError("Failed to attach native terminal")` is displayed.
   - Once all 5 retries fail (elapsed ~7.75s), `willRetry = false`. The loop terminates permanently. No further retry timers are set.
   - Crucially, `ResizeObserver` is only registered inside the `try` block upon successful attach. If initial attach fails, `ResizeObserver` never runs to trigger `reportBounds` or re-attach when layout finishes.

2. **Root Causes of the Initial Attach Failure**:
   - **Transient Daemon Spawn Race**: During tab creation or cold workspace restore, the UI's `NativeTerminalPane` can mount and dispatch `cmd_native_terminal_attach` before the daemon finishes spawning the shell or registering the PTY in `terminal_service`. The daemon returns `Session '{session_id}' not found`.
   - **Initial Zero/Sub-Cell Dimension Rejection**: When DOM layout/flexbox is not settled (e.g. animation, tab transition, sidebar shift), `measureGeometry()` may yield subpixel coordinates with negative values (e.g. `x = -0.5`) or heights smaller than 1 cell row (`physicalHeight < cell_metrics.height_px`). In Rust backend (`SurfaceCompositionLayout::compute`), this returns `InvalidDimensions(cols, 0)` or `InvalidValue("non-negative")`, which rejects `cmd_native_terminal_attach` with an internal IPC error.
   - **No Fallback in Rust**: When `cmd_native_terminal_attach` receives an initial bounds error in `attach_daemon_attachment_with_bounds`, it fails the entire attach rather than falling back to default dimensions (80x24) for the stream and waiting for subsequent `cmd_native_terminal_set_bounds`.

3. **Why Tab Splitting Revived the Terminal**:
   - In `TerminalSplitView.tsx`, a single-leaf pane tree (`LeafNode`) is replaced by a split node (`SplitNode`) rendering `<div className="pane-split"> ... </div>`.
   - React unmounts the previous `NativeTerminalPane` instance and mounts a fresh one.
   - Mounting a new component resets `error` to `null` and restarts `attemptAttach(0)`.
   - By the time the user splits the tab (seconds later), the daemon session is fully ready and DOM dimensions are non-zero, so the fresh attach succeeds immediately.

---

## 3-Part Solution & Implementation Plan

### Part 1: Frontend Dimension & Bounds Hardening (`NativeTerminalPane.tsx`)
- In `measureGeometry()`:
  - Clamp origin coordinates to non-negative: `Math.max(0, left)`, `Math.max(0, top)`.
  - Only return geometry if `physicalWidth >= 16 && physicalHeight >= 16` (or minimum cell size), otherwise return `null` so attach uses safe daemon defaults without erroring.

### Part 2: Attach Lifecycle Self-Healing & Interactive Recovery (`NativeTerminalPane.tsx`)
- Always register `ResizeObserver` on mount regardless of whether initial attach succeeded. When dimensions resize and a pane is in an errored/unattached state, trigger `attemptAttach(0)`.
- Auto-retry on pane focus/click: In `onPointerDown` / `inputRef` focus handlers, if `error !== null`, trigger `attemptAttach(0)` so simply clicking the terminal revives it.
- Make the error banner interactive: Remove `pointer-events-none`, style it as a clickable badge (`cursor-pointer hover:bg-destructive/20`), and allow clicking it to immediately retry `attemptAttach(0)`.

### Part 3: Rust Backend Attach Graceful Degradation (`surface_host.rs`)
- In `attach_daemon_attachment_with_bounds`:
  - If `prepare_session_layout` fails due to `InvalidDimensions` or bounds calculation error, log a warning and proceed with default attachment dimensions instead of aborting the entire attach. The subsequent `cmd_native_terminal_set_bounds` from the frontend will set the correct geometry once DOM layout stabilizes.
