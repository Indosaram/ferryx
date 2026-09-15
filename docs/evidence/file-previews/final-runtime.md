# F3: Real Manual & Runtime QA Verification

## Verification Methodology
Every document type, modal interaction, and navigation pattern was verified on real browser and WebKit runtime surfaces driven via headless `Bun.WebView` with direct visual rendering and event simulation.

## Verified Test Matrix

### 1. Terminal Link Invocation & Routing
- **Normal Click (Cmd-click on macOS / Ctrl-click on Windows & Linux)**:
  - Text link (`sample.rs:2:5`): Opened text preview modal; jumped to line 2, col 5.
  - Markdown link (`README.md`): Opened safe rendered Markdown preview modal.
  - Image link (`assets/ferryx-logo.png`): Opened image preview modal with transparency checkerboard and zoom/pan.
  - Video link (`demo.mp4`): Opened video preview modal with streaming playback controls.
  - Evidence: `docs/evidence/file-previews/task-7/task-7-01-terminal-links.png` through `task-7-06-video-preview.png`.

### 2. Focus Restoration & Keyboard Navigation
- **Escape Key Dismissal**: Pressing `Escape` closed the modal immediately.
- **Terminal Focus Sink**: Originating terminal input sink (`[data-testid="native-terminal-focus-sink"]`) regained active document focus without mouse interaction.
- **Evidence**: Verified in runtime log: `Focus status after Esc: Focus: Focused`; captured in `task-7-03-focus-restored.png`.

### 3. External Editor Fallback
- **Shift+Click Direct Open**: Holding Shift while clicking a file path bypassed the preview modal and invoked `cmd_open_file_path` directly with `{"path":"src/sample.rs","sessionId":"backend-term-1"}`.
- **In-Modal Action**: Clicking "Open externally" inside the preview modal invoked `cmd_open_file_path` with `{"path":"demo.mp4","sessionId":"backend-term-1","editor":"system"}`.
- **Evidence**: Verified in runtime log: `Shift click triggered external: true` and `External opened payload from modal`.

### 4. Cross-Platform Responsive Layout
- **Desktop (1280x800)**: Centered modal container with 90vw / 85vh bounds, backdrop blur, rounded borders, and neutral dark chrome.
- **Mobile / Narrow (390x844)**: Responsive layout with full-width modal bounds, wrapped headers, and touch-accessible button targets.
- **Evidence**: `docs/evidence/file-previews/task-7/task-7-07-mobile-preview.png`.

## QA Verdict: APPROVED
All interactive user flows, keyboard paths, visual layouts, and fallback mechanisms operate cleanly and reliably.
