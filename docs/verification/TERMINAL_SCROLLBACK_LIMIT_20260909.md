# Terminal Scrollback Truncation Fix & Settings Control

**Date:** 2026-09-09  
**Branch:** `main`  
**Status:** Verified

---

## 1. Problem Statement

When output exceeding several hundred lines (e.g. running `cat test.txt` multiple times) was printed to the Ferryx native terminal, older scrollback content was truncated and inaccessible, keeping only ~300–500 lines despite expected scrollback capacity of 10,000 lines. Furthermore, the Settings dialog lacked a user-facing control to view or adjust the scrollback capacity.

---

## 2. Root Cause Analysis

1. **`libghostty-vt` Default 10KB Byte Ceiling (`max_scrollback_bytes`)**:
   - `libghostty-vt` manages scrollback memory using a `PageList`.
   - Each time new rows overflow the active screen grid into scrollback, `Screen.zig` checks both `max_scrollback_lines` and `max_scrollback_bytes`.
   - If either limit is exceeded, pages are immediately pruned from the head of the scrollback.
   - Ferryx previously initialized `libghostty-vt` without configuring `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES` (27). As a result, Ghostty's internal default of 10,000 bytes (10 KiB) remained active.
   - ANSI escape codes, colors, unicode characters, and long rows quickly exceeded 10 KiB in 2–3 pages (~300–500 rows), causing continuous eviction of history.

2. **Absence of Dynamic Line Configuration & UI Control**:
   - The native engine had no API to configure scrollback capacity at runtime.
   - Ghostty configuration parsing (`parse_ghostty_config`) did not recognize `scrollback-limit`.
   - Settings UI had no field to inspect or customize scrollback lines, and overrides were not pushed to active PTY sessions.

---

## 3. Implementation Details

### A. C ABI & Native Terminal Engine (`src-tauri`)
- **`src-tauri/src/native_terminal/sys/constants.rs`**:
  - Defined `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES = 27`.
  - Defined `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES = 28`.
- **`src-tauri/src/native_terminal/lifecycle.rs`**:
  - Configured `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES` to `null` (clearing the 10KB ceiling).
  - Configured `GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES` to `DEFAULT_SCROLLBACK_LINES` (10,000).
- **`src-tauri/src/native_terminal/terminal.rs` & `engine.rs`**:
  - Implemented `set_scrollback_limit_lines(Option<usize>)` and `set_scrollback_limit_bytes(Option<usize>)` using `ghostty_terminal_set_options`.
- **`src-tauri/src/terminal/preferences.rs`**:
  - Added `scrollback: usize` to `TerminalPreferences` (default 10,000).
  - Added `scrollback: Option<usize>` to `TerminalPreferenceOverrides`.
  - Parsed `scrollback-limit` and `scrollback-limit-lines` in `parse_ghostty_config`.
- **`src-tauri/src/native_terminal/surface_host.rs` & `src-tauri/src/ipc/preferences.rs`**:
  - Added `reapply_scrollback_to_sessions` to push updated scrollback limits to all active sessions in real time without restarting.
  - Wired `scrollback` through `cmd_terminal_apply_overrides`.

### B. Frontend UI & IPC (`ui/`)
- **`ui/src/lib/tauri.ts` & `ui/src/lib/terminalSettings.ts`**:
  - Added `scrollback` to `TerminalPreferences`, `TerminalOverrides`, and `TerminalSettings`.
  - Wired `scrollback` into `syncNativeOverrides` payload.
- **`ui/src/components/settings/TerminalSection.tsx` & `ui/src/components/SettingsDialog.tsx`**:
  - Added "Scrollback lines" number input control (range: 1,000 – 100,000 lines).
  - Implemented draft buffering with commit on blur or Enter, including clamping.
  - Supported "Use imported" button resetting scrollback to default.

---

## 4. Verification Evidence

### A. Realistic Output Retention Benchmark (`test.txt`)
Fed `test.txt` (~404 lines per run) sequentially into `NativeTerminal`:
- **Before Fix**:
  - 1x `test.txt`: 480 rows
  - 2x `test.txt`: 316 rows (pruned by 10KB ceiling)
  - 4x `test.txt`: 328 rows (pruned by 10KB ceiling)
- **After Fix**:
  - 1x `test.txt`: 480 rows
  - 2x `test.txt`: 994 rows
  - 3x `test.txt`: 1,509 rows
  - 4x `test.txt`: 2,023 rows
  - 10x `test.txt` (~4,000 lines): **5,109 rows retained without truncation**

### B. Automated Contract & Unit Tests
- `cargo test --test native_terminal_capability_contract`: 7 passed, 0 failed.
- `cargo test --test native_terminal_surface_host_contract`: 18 passed, 0 failed.
- `cargo test --lib terminal::preferences`: 7 passed, 0 failed.
- `bun run --cwd ui test src/components/settings/TerminalSection.test.tsx`: 8 passed, 0 failed.
- `bun run --cwd ui test src/lib/terminalSettings.test.tsx`: 15 passed, 0 failed.
- `bun run --cwd ui build`: TypeScript and Vite production bundle compiled cleanly with 0 errors.
