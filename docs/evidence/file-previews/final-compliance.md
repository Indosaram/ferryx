# F1: Plan Compliance Audit

## Plan Overview
- **Plan File**: `.omo/plans/file-previews.md`
- **Feature**: Built-in read-only terminal file preview (Text, Markdown, Image, Video) with independent external editor open.
- **Worktree**: `/Users/indo/code/project/orca-lite-wt/preview` (branch `preview`)
- **Audit Date**: 2026-09-15

## Contract Traceability Matrix

### 1. Capability & Range Service (Rust Backend)
- **Requirement**: Ephemeral loopback HTTP streaming server on `127.0.0.1:0` with cryptographically random capability tokens.
- **Implementation**: `src-tauri/src/ipc/file_preview.rs` (`FilePreviewService`, `start_range_server`, `generate_capability_token`).
- **Safety**: Strict read-only semantics. No endpoints exist for writing, saving, or editing. All synchronous disk I/O executed on `tokio::task::spawn_blocking`.
- **Limits**:
  - Max text/markdown size: 2 MiB (UTF-8 boundary safe truncation + warning payload).
  - Max image size: 100 MiB, 16,384 x 16,384 px.
  - Video: HTTP Range streaming (206 Partial Content, Content-Range, Accept-Ranges: bytes).
  - TTL: 15 minutes, LRU eviction at 10 documents, max 64 child capability handles per document.
- **Verification**: `src-tauri/src/ipc/file_preview_tests.rs` (44 tests pass).

### 2. Frontend Preview Controller & Modal Shell
- **Requirement**: Single active modal, generation-tracked async request lifecycle, Escape key dismissal, and focus restoration to originating terminal pane.
- **Implementation**: `ui/src/lib/filePreview.ts` (`filePreviewController`, `useSyncExternalStore`), `ui/src/components/FilePreviewDialog.tsx`.
- **Safety**: Superseded requests discarded via monotonic generation tokens. Focus restoration targets originating terminal input sink (`[data-testid="native-terminal-focus-sink"]` inside `[data-leaf-id]`).
- **Verification**: `ui/src/lib/filePreview.test.ts` (15 tests pass), `ui/src/components/FilePreviewDialog.test.tsx` (13 tests pass).

### 3. Text & Safe Markdown Renderers
- **Requirement**: Monospace text rendering with line numbers and syntax highlighting; safe rendered Markdown with GFM, no raw HTML execution, and raw toggle.
- **Implementation**: `ui/src/components/FilePreviewText.tsx`, `ui/src/lib/previewMarkdown.ts`.
- **Safety**: Pinned `react-markdown` 10.1.0 and `remark-gfm` 4.0.1. `rehypeRaw` intentionally omitted to forbid raw HTML/scripts.
- **Verification**: `ui/src/components/FilePreviewText.test.tsx` (19 tests pass), `ui/src/lib/previewMarkdown.test.ts` (16 tests pass).

### 4. Bounded Image Renderer
- **Requirement**: Image preview with checkerboard background for transparency, fit/zoom/pan controls, and bounded dimensions.
- **Implementation**: `ui/src/components/FilePreviewImage.tsx`.
- **Verification**: `ui/src/components/FilePreviewImage.test.tsx` (13 tests pass).

### 5. Streamed Video Renderer
- **Requirement**: Video preview via HTTP Range requests with custom HUD playback controls (play/pause, timeline scrubber, volume/mute).
- **Implementation**: `ui/src/components/FilePreviewVideo.tsx`.
- **Verification**: `ui/src/components/FilePreviewVideo.test.tsx` (9 tests pass), browser run logs in `docs/evidence/file-previews/task-5/`.

### 6. Terminal Link Routing & Parity
- **Requirement**: Cmd-click (macOS) / Ctrl-click (Windows/Linux) opens preview modal; Shift+click opens external editor.
- **Implementation**: `ui/src/lib/linkRouting.ts`, `ui/src/components/NativeTerminalPane.tsx`, `ui/src/App.tsx`, `src-tauri/tauri.conf.json`.
- **Verification**: `ui/src/lib/linkRouting.test.ts` (23 tests pass), `ui/src/components/NativeTerminalPane.test.tsx` (170 tests pass), real-surface browser test suite in `docs/evidence/file-previews/task-7/`.

## Audit Verdict: APPROVED
All plan requirements and contracts are fully satisfied and substantiated by verified test suites and visual evidence artifacts.
