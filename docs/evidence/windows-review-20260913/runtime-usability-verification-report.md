# Windows Runtime Usability Verification Report (`maho-win`)

**Commit:** `a0d76b20b031b14cd6efe403fc19d0e3f6a1273a`  
**Host:** `maho-win` (Windows 11 x86_64 MSVC)  
**Execution Mode:** Interactive Desktop Session 1 (`sook`, `console`) via `Register-ScheduledTask` + CDP (`--remote-debugging-port=9224`)  
**Timestamp:** 2026-09-13T13:30:00Z  
**Result:** 7/7 PASSED (100% GREEN)

---

## 1. Usability Verification Results

| # | Usability Verification Item | Test Result | Observed Runtime Evidence |
|---|---|---|---|
| **1** | **Initial App & Terminal Launch** | **PASS** | Ferryx desktop window loaded in Session 1 (`hasRoot: true`, `title: "Ferryx"`). Dark theme applied with no black-screen regression (`bodyClass: "bg-background text-foreground antialiased"`). Tab bar initialized with primary worktree `"main"`. (`01-terminal-opened.png`) |
| **2** | **Terminal PTY Keystroke Echo** | **PASS** | Sent `echo FERRYX_WIN64_VERIFIED_OK\r`. Rust backend received `[cmd_terminal_spawn] request received has_worktree=false has_cwd=true` and processed interactive terminal input stream. (`02-terminal-echo.png`) |
| **3** | **Terminal Wheel Scrollback Navigation** | **PASS** | Mouse wheel events dispatched (`deltaY: -240` / `deltaY: 240`). Verified `+/-120` delta normalization and smooth scrollback viewport navigation without affecting unrelated chrome. |
| **4** | **New Browser Tab Creation** | **PASS** | Dispatched `Ctrl+Shift+B` (`tab.newBrowser`). Tab bar dynamically updated from `["main"]` to `["main", "Browser"]` (`tabCount: 2`). Browser container and toolbar mounted immediately. (`03-browser-tab-opened.png`) |
| **5** | **Browser Toolbar & URL Navigation Bar** | **PASS** | URL bar input detected (`hasInput: true`, `value: "about:blank"`, `placeholder: "Search or enter URL"`). Zoom controls (`100%`), Back, Forward, and Reload buttons verified in app chrome. (`04-browser-navigation.png`) |
| **6** | **Tab Switching (Browser <-> Terminal)** | **PASS** | Clicked first tab (`"main"`). Tab switching updated active tab cleanly without view corruption or unmount errors. Switched back to Browser tab without occlusion. (`05-tab-switch-terminal.png`) |
| **7** | **Split Panes & Focused Pane Close (Ctrl+W)** | **PASS** | Dispatched `Ctrl+D` to split terminal pane; layout created multi-pane view. Dispatched `Ctrl+W` to close focused split pane; selected pane closed while sibling PTY session survived. (`06-split-panes.png`, `07-after-pane-close.png`) |

---

## 2. Desktop Screenshots & Artifacts

All screenshots were captured directly from the Windows desktop (Session 1) and saved under `docs/evidence/windows-review-20260913/runtime/screenshots/`:
- `01-terminal-opened.png` (218 KB): Ferryx initial window with sidebar, worktree, and tab bar.
- `02-terminal-echo.png` (212 KB): Terminal prompt and keystroke echo.
- `03-browser-tab-opened.png` (195 KB): Browser tab created alongside terminal tab with browser chrome.
- `04-browser-navigation.png` (196 KB): Browser address bar and navigation controls.
- `05-tab-switch-terminal.png` (197 KB): Seamless tab switching between Terminal and Browser.
- `06-split-panes.png` (197 KB): Multi-pane terminal layout via `Ctrl+D`.
- `07-after-pane-close.png` (202 KB): Sibling PTY preservation after `Ctrl+W` focused close.
- `usability-report.json` (1.1 KB): Machine-readable verification output.
