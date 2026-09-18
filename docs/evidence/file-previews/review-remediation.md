# Senior Engineer Code Review Remediation Report

**Date**: 2026-09-15
**Branch**: `preview`
**Base Commit**: `240da668`

All 14 findings (R1–R14) identified during the senior-engineer code review have been fully addressed, hardened, and verified with dedicated regression tests.

---

## 1. High-Priority Remediations (R1–R5)

### R1 — Markdown Local Image Re-acquisition Loop
- **Issue**: `FilePreviewDialog` created a fresh capability object on every render. `FilePreviewText` components and image effects depended on that object identity. State updates from decremented child budgets triggered re-renders, causing acquired images to unmount/reload and repeatedly consume child capability handles until budget exhaustion.
- **Remediation**:
  1. `ui/src/lib/filePreview.ts`: Cached the markdown capability object per generation/handle in `createFilePreviewController`. Invalidate on new generation or close. Exposed `remainingChildHandles` as a dynamic getter so the capability identity remains stable.
  2. `ui/src/components/FilePreviewText.tsx`: Added an acquisition guard ref in `PreviewImageNode` ensuring each relative path is requested at most once per generation, avoiding re-acquisition on component re-renders.

### R2 — Concurrent Media Range Cursor Sharing
- **Issue**: `chunk_stream` used `File::try_clone()` followed by `seek()` on the async stream. Rust `File` clones on Unix duplicate the file descriptor (`dup`), which shares the underlying OS file description's seek cursor. Concurrent range requests to the same document could interleave seeks and corrupt byte offsets.
- **Remediation**: Added `stream_lock: Arc<tokio::sync::Mutex<()>>` to `HandleRecord`. In `serve_capability`, each media stream acquires `stream_lock.clone().lock_owned().await` and holds it in `ChunkState` for the entire lifetime of the stream, serializing seeks on that document's underlying file description and guaranteeing byte-exact delivery.
- **Regression Test**: `concurrent_disjoint_ranges_on_same_document_stream_exact_bytes` verifies concurrent disjoint byte ranges (`100-199`, `300-399`, `500-599`) on the same capability stream exact bytes.

### R3 — Late Open Revoking Newer Preview
- **Issue**: Backend registration occurred after awaited blocking I/O and unconditionally replaced the window's main handle. An out-of-order sequence (A starts, B starts, B finishes, A finishes) allowed stale A to revoke B's active capability.
- **Remediation**: Added monotonic `epoch: u64` to `WindowSlot`. `open()` and `open_child_document()` allocate the epoch via `begin_main_open()` *before* awaiting blocking I/O. In `register_main()`, if the window slot's epoch differs from the request's epoch, the registration is rejected with `ExpiredHandle` and the stale descriptor is dropped without touching the active preview.
- **Regression Test**: `late_open_completion_does_not_revoke_newer_preview` simulates out-of-order registration and asserts the newer preview is retained.

### R4 — Child Budget & Parent Ownership Race
- **Issue**: Parent validity and remaining budget were checked before awaiting I/O, while registration appended afterward. Concurrent child requests could oversubscribe the budget, and children could attach after their parent had closed.
- **Remediation**:
  1. Added `pending_children: usize` to `WindowSlot`.
  2. `reserve_child_slot()` atomically reserves a budget slot before I/O, refunding on failure.
  3. `register_child()` re-verifies that the parent handle is still the window's active main before committing, refusing stale children.

### R5 — Markdown Directory Containment Hardening
- **Issue**: `resolve_contained_child` re-canonicalized stored parent path, leaving a TOCTOU window if the parent directory was renamed and replaced with a symlink.
- **Remediation**:
  1. Captured `parent_identity: Option<(u64, u64)>` (device, inode) at open time.
  2. `resolve_contained_child` verifies directory identity before path canonicalization.
  3. `open_child_image` and `open_child_document` re-verify that the boundary directory identity still holds after awaited I/O (`boundary_identity_holds_after_io`), rejecting swapped boundaries.

---

## 2. Additional Remediations (R6–R14)

### R6 — 15-Minute TTL, Periodic Sweep & Teardown
- Added `last_access_ms: AtomicU64` to `HandleRecord`. `serve_capability` updates it on every request.
- Added background sweeper in `FilePreviewService::start` running every 60s to revoke handles whose idle lease exceeds 15 minutes (`HANDLE_TTL`).
- Added window `Destroyed` event listener in `src-tauri/src/lib.rs` calling `service.close_window(window.label())`.
- Added unmount disposal effect in `FilePreviewDialog.tsx` calling `controller.close()` on component unmount.

### R7 — Enforce 2 MiB Text Limit on Actual Reads
- `open_blocking_path`: Used `(&mut file).take(limits::TEXT_MAX_BYTES + 1)` to cap the actual read to 2 MiB + 1 byte, preventing unbounded memory allocation if a file grows between stat and read.

### R8 — Linear Search Matching & Universal Newlines
- `FilePreviewText.tsx`: Replaced quadratic `matches.findIndex` line rendering with `indexMatches()`, precomputing line buckets and first-match offsets in a single O(matches) pass.
- `file_preview.rs`: Updated `count_lines` to parse universal newlines (`\r\n`, lone `\r`, lone `\n`), matching the frontend line-budget semantics for CR-only content.

### R9 — IPC Authorization Bound to Invoking Webview
- Updated all 4 IPC command handlers (`cmd_file_preview_open`, `cmd_file_preview_open_child`, `cmd_file_preview_open_child_document`, `cmd_file_preview_close`) to receive `webview: tauri::Webview<R>` and validate `webview.label()`, preventing child browser guest webviews (`browser-{uuid}`) from masquerading as the desktop root window.

### R10 — Move Metadata Check Off Async Reactor
- `serve_capability`: Moved the retained inode metadata length check off the async reactor onto `crate::ipc::run_blocking`.

### R11 — Standards-Compliant RFC 9110 Range Semantics
- Added `RangeOutcome::Ignored`.
- Unsupported multi-range, unknown units, reversed bounds (`bytes=5-2`), and malformed syntax are ignored with `200 OK` and the full document body per RFC 9110 §14.2.
- Only syntactically valid single ranges landing past EOF or with suffix length 0 return `416 Range Not Satisfiable`.
- `HEAD` requests ignore `Range` headers entirely and return `200 OK` with full `content-length` and no body.

### R12 — Image Header Probe & Per-Axis Bounds
- `open_blocking_path`: Probes up to 64 KiB (`IMAGE_HEADER_PROBE`) in a read loop to parse dimension markers past large EXIF/ICC metadata.
- Added `IMAGE_MAX_AXIS: u32 = 16_384`: images exceeding 16,384 px on width or height are rejected with `TooLarge`.
- Total pixel product (40 megapixels) preserved.
- **Regression Test**: `image_exceeding_per_axis_bound_is_rejected` verifies 20,000x100 px image rejection.

### R13 — Terminal Link Detection Media Extensions
- `ui/src/lib/linkRouting.ts`: Added `"markdown", "webp", "mp4", "m4v", "mov", "webm", "ogv"` to `KNOWN_EXTENSIONS`, enabling single-token terminal links like `clip.mp4` or `photo.webp` without leading path slashes.
- Added regression test in `linkRouting.test.ts`.

### R14 — Terminal Focus Sink Recovery on Escape
- `ui/src/components/FilePreviewDialog.tsx`: Updated `restoreFocus` to explicitly locate and focus the originating leaf's native terminal input sink (`[data-testid="native-terminal-focus-sink"]`) before falling back to generic focusable elements.

---

## 3. Verification Summary

- **Rust Backend**:
  - `cargo check --manifest-path src-tauri/Cargo.toml`: exit code 0
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib file_preview`: **47 passed, 0 failed**
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib file_preview_contract`: **7 passed, 0 failed**
- **Frontend UI**:
  - 9 preview test files: **290 passed, 0 failed**
  - `bun run --cwd ui build`: exit code 0 (clean Vite build, 2.57s)
