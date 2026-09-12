# Fresh Windows Terminal Visual Review

Visual inspection of 4 runtime screenshots from `docs/evidence/windows-terminal-20260912/runtime/artifacts/`. All image payloads were directly delivered and visually inspected.

## 1. fresh-shell-menu.png (3840x1600)
- **Image delivered**: Yes, full image payload rendered.
- **Menu Hierarchy & Labels**:
  - Main menu: `New Terminal` (`Ctrl+T`), `New Terminal Profile` (`>`), `New Browser Tab` (`Ctrl+Shift+B`), `Agent settings`.
  - Submenu (`New Terminal Profile`): `PowerShell`, `Windows PowerShell`, `Command Prompt`, `WSL`.
- **Obscuration Judgment**: The popup menu cleanly overlays the tab bar/content. Terminal content does not obscure Ferryx chrome (tab bar, sidebar, window controls). Window sits above a background `cmd.exe` window. A desktop toast `Failed to update native terminal bounds` appears in the bottom-right corner.

## 2. fresh-cmd-output.png (3840x1600)
- **Image delivered**: Yes, full image payload rendered.
- **Active Tab**: `main (2)` (Command Prompt).
- **Echo Output**:
  - Command: `echo FERRYX_WIN_SHELL_OK`
  - Visible output: `FERRYX_WIN_SHELL_OK`
  - Prompt: `C:\Users\sook\ferryx-qa-fresh-0912\orca-lite>`
- **Obscuration Judgment**: Terminal buffer renders inside the content pane. Neither terminal obscures chrome nor is obscured by window chrome.

## 3. fresh-start-output.png (3072x1280)
- **Image delivered**: Yes, full image payload rendered.
- **Active Tab**: `main` (PowerShell 7.6.6).
- **Echo Output**:
  - Command: `echo FERRYX_WIN_START_OK`
  - Visible output: `FERRYX_WIN_START_OK`
  - Prompt: `orca-lite on ^ HEAD (7f7ecd8) via 🦀 v1.4.0`
- **Obscuration Judgment**: Clean layout. Tab bar, sidebar, and terminal content area are clearly separated without overlapping or clipping.

## 4. fresh-resize-output.png (3072x1280)
- **Image delivered**: Yes, full image payload rendered.
- **Active Tab**: `main` (PowerShell 7.6.6) after window resize.
- **Echo Output**:
  - Previous: `FERRYX_WIN_START_OK`
  - Command: `echo FERRYX_WIN_RESIZE_OK`
  - Visible output: `FERRYX_WIN_RESIZE_OK`
- **Obscuration Judgment**: Terminal adapts to the resized window boundaries. Text wraps and aligns cleanly within the pane; no chrome obscuration or clipping detected.

## Summary Verdict
All four screenshots were visually verified from raw image payloads. Shell menu options and expected echo sentinel strings (`FERRYX_WIN_SHELL_OK`, `FERRYX_WIN_START_OK`, `FERRYX_WIN_RESIZE_OK`) are clearly rendered. Terminal panes respect window boundaries without obscuring application chrome.

## 5. Detailed Inspection: Bottom-Right Toast in fresh-shell-menu.png
- **Exact Visible Text**: `Failed to update native terminal bounds`
- **Title / Suffix**: None visible (single-line white text in red pill; no title bar, prefix icon, suffix, or close button).
- **Approximate Pixel Bounding Box (3840x1600)**:
  - `x`: ~3556 to ~3812 (width ~256 px)
  - `y`: ~1505 to ~1533 (height ~28 px)
  - (Displayed 2000x833 space: x ~1852–1985, y ~784–798; resting directly above the taskbar at y ~1540).
- **Window Association**: Strictly **OUTSIDE** the fresh QA window. The floating fresh QA window spans roughly [x: 160–1770, y: 170–1260]. The toast sits far to the right in the bottom-right corner of the full desktop / maximized background workspace. Origin is not assumed.
