# Task 7: End-to-End Link & Surface Parity Verification

## Objectives & Verdict
- **Terminal Link Click Routing**: Verified Cmd/Ctrl-click on terminal file tokens opens the read-only file preview modal across all 4 supported document types: text, safe markdown, image, and video.
- **Escape Key & Focus Restoration**: Verified pressing `Escape` closes the preview modal immediately and restores focus to the originating terminal pane's input sink (`[data-testid="native-terminal-focus-sink"]` inside `[data-leaf-id]`).
- **External Editor Access**:
  - Verified Shift+Cmd/Ctrl-click directly bypasses the preview modal and invokes the external editor (`cmd_open_file_path`).
  - Verified the in-modal "Open externally" button invokes `cmd_open_file_path` with the exact path, session, and line/col.
- **Cross-Platform & Responsive Safety**:
  - Modal bounds verified on Desktop (1280x800) with max bounds and backdrop blur.
  - Modal bounds verified on Mobile (390x844) with full-width responsive adaptations.
  - Zero editor or modification capabilities present (strictly read-only).

## Evidence Artifacts
- `task-7-01-terminal-links.png`: Terminal pane displaying file paths (`.rs`, `.md`, `.png`, `.mp4`).
- `task-7-02-text-preview.png`: Text preview modal with line numbers, syntax highlighting, and initial line jumping.
- `task-7-03-focus-restored.png`: Modal closed via `Escape`, focus indicator showing "Focused" on terminal sink.
- `task-7-04-markdown-preview.png`: Safe rendered Markdown preview modal showing headers, lists, code block, and raw toggle.
- `task-7-05-image-preview.png`: Image preview modal showing checkerboard transparency canvas, zoom/pan controls, and dimensions.
- `task-7-06-video-preview.png`: Streamed video preview modal with custom playback controls, timeline bar, and volume.
- `task-7-07-mobile-preview.png`: Responsive 390x844 mobile viewport layout.
