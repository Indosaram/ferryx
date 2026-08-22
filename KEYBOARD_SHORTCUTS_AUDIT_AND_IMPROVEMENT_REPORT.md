# Ferryx Keyboard Shortcut System: Exhaustive Audit & Improvement Report

**Document Date:** 2026-08-22  
**Target System:** Ferryx (com.ferryx.app / orca-lite)  
**Author:** omo-senpi Architecture Team  
**Status:** Completed & Approved for Implementation  

---

## 1. Executive Summary

Ferryx is a high-performance, native-speed terminal and workspace orchestrator. A keyboard-first interaction model is critical to developer velocity. This audit conducts a complete, end-to-end investigation across the codebase (`ui/src`, `src-tauri/src`, and original reference assets) to identify gaps, conflicts, platform inconsistencies, and opportunities to elevate the shortcut system to gold-standard macOS and cross-platform desktop ergonomics.

### Key Findings
1. **Engine Foundation is Clean but Limited**: `ui/src/lib/shortcuts.ts` provides a centralized action registry and capture-phase listener, but was restricted to single bindings per action, missing essential alternative/alias shortcuts (e.g. `Cmd+Shift+]` alongside `Ctrl+PageDown`).
2. **MacBook / Laptop Tab Navigation Gap**: `tab.next` and `tab.previous` were mapped solely to `Ctrl+PageDown`/`Ctrl+PageUp`. Because standard MacBook and Apple keyboards lack dedicated PageUp/PageDown keys, tab switching without a mouse was unnecessarily cumbersome.
3. **Index Limit (1–4 vs 1–9)**: Workspace and tab number selection was limited to indices 1–4, preventing quick keyboard switching for users with 5 to 9 active workspaces or tabs.
4. **Command Palette Dual Triggers (`Cmd+K` & `Cmd+P`)**: Developers transitioning from VS Code, Sublime, or Ghostty expect `Cmd+P` for quick workspace/file jumping, alongside `Cmd+K` for command palette navigation.
5. **Zoom & Font Scaling Missing**: No fast keyboard shortcuts existed for terminal font zoom in (`Cmd+=`), zoom out (`Cmd+-`), and reset (`Cmd+0`).
6. **Settings Discovery**: The Settings "Keyboard Shortcuts" pane was a flat static table without search or category filtering.
7. **Native macOS Menu Hierarchy**: `src-tauri/src/lib.rs` only registered a single `tab.newTerminal` item under a bare `Terminal` menu, missing standard macOS application menus (`App`, `File`, `Edit`, `View`, `Window`).

---

## 2. Codebase Inventory & Current Architecture

### 2.1 Frontend Shortcut Architecture (`ui/src`)

| File | Role & Mechanism | Key Interactions |
|---|---|---|
| `ui/src/lib/shortcuts.ts` | **Central Registry & Hook Engine** | Defines `ShortcutActionId`, `SHORTCUTS` array, `matchesBinding()`, `shortcutLabel()`, and `useShortcuts()`. Uses capture-phase `window.addEventListener("keydown", ..., true)` to intercept global chords before DOM elements. |
| `ui/src/App.tsx` | **Root Dispatcher** | Wires 22 `shortcutHandlers` into `useShortcuts()`. Dispatches actions to `state.layout`, `workspaceStore`, `terminalHostManager`, and modal dialog states. |
| `ui/src/components/TerminalPane.tsx` | **xterm Terminal Host** | Hosts the `@xterm/xterm` canvas/webgl container with `.terminal-host`. Non-shortcut keydowns pass through to `terminal.onData` for shell/PTY consumption; shortcut chords are captured by `useShortcuts` with `event.preventDefault()`. |
| `ui/src/components/TerminalSearchOverlay.tsx` | **In-Pane Search** | Handles `Enter` (find next), `Shift+Enter` (find previous), and `Escape` (close search). |
| `ui/src/components/CommandPalette.tsx` | **Quick Jumper & Cheat Sheet** | Handles `Escape` to close, search filtering of worktrees and tabs, and displays registered `SHORTCUTS` with `shortcutLabel()`. |
| `ui/src/components/SettingsDialog.tsx` | **Settings UI & Shortcut Viewer** | Renders `ShortcutSettings` component displaying all registered shortcuts grouped by domain. |
| `ui/src/components/NewTabPopover.tsx` | **New Tab Trigger Menu** | Provides tab creation options; had hardcoded shortcut badges. |
| `ui/src/components/TabBar.tsx` | **Tab Bar** | Handles inline renaming (`Enter`/`Escape`) and context menus. |
| `ui/src/components/Sidebar.tsx` | **Sidebar Rail** | Displays `shortcutLabel("commandPalette.open")` on search button. |

### 2.2 Backend Menu & Native Accelerator Architecture (`src-tauri`)

| File | Current Implementation | Enhancement Target |
|---|---|---|
| `src-tauri/src/lib.rs` | `install_app_menu` builds a single `Terminal` menu with `MenuItemBuilder::with_id("tab.newTerminal", ...).accelerator("CmdOrCtrl+T")`. | Build standard macOS application menus (`App`, `File`, `Edit`, `View`, `Window`) with standard system accelerators and menu event dispatching. |

---

## 3. Comparison Matrix: Original Orca vs Current Ferryx vs Proposed Standard

| Action ID | Description | Original Orca (Darwin) | Current Ferryx | Proposed Improvement (Ferryx v2) |
|---|---|---|---|---|
| `tab.newTerminal` | New terminal tab | `Mod+T` | `⌘T` (`Mod+T`) | `⌘T` (`Mod+T`) |
| `tab.newBrowser` | New browser tab | `Mod+Shift+B` | `⌘⇧B` (`Mod+Shift+B`) | `⌘⇧B` (`Mod+Shift+B`) |
| `tab.close` | Close active tab | `Mod+W` | `⌘W` (`Mod+W`) | `⌘W` (`Mod+W`) |
| `tab.next` | Next tab | `Mod+Shift+]`, `Ctrl+Tab`, `Ctrl+PageDown` | `⌃PgDn` (`Ctrl+PageDown`) | **`⌘⇧]` + `⌃Tab` + `⌃PgDn`** |
| `tab.previous` | Previous tab | `Mod+Shift+[`, `Ctrl+Shift+Tab`, `Ctrl+PageUp` | `⌃PgUp` (`Ctrl+PageUp`) | **`⌘⇧[` + `⌃⇧Tab` + `⌃PgUp`** |
| `tab.select1`–`select4` | Select tab 1–4 | `Ctrl+1`–`4` | `⌃1`–`⌃4` | `⌃1`–`⌃4` |
| `tab.select5`–`select9` | Select tab 5–9 | `Ctrl+5`–`9` | *Missing* | **`⌃5`–`⌃9` (Added)** |
| `workspace.select1`–`select4` | Select workspace 1–4 | `Mod+1`–`4` | `⌘1`–`⌘4` | `⌘1`–`⌘4` |
| `workspace.select5`–`select9` | Select workspace 5–9 | `Mod+5`–`9` | *Missing* | **`⌘5`–`⌘9` (Added)** |
| `terminal.splitRight` | Split terminal right | `Mod+D` | `⌘D` (`Mod+D`) | `⌘D` (`Mod+D`) |
| `terminal.splitDown` | Split terminal down | `Mod+Shift+D` | `⌘⇧D` (`Mod+Shift+D`) | `⌘⇧D` (`Mod+Shift+D`) |
| `terminal.unsplit` | Close active split pane | `Mod+Alt+D` / `Mod+W` | `⌘⌥D` (`Mod+Alt+D`) | **`⌘⌥D` + `⌘⇧W`** |
| `terminal.focusNext` | Focus next pane | `Mod+]` | `⌘]` (`Mod+]`) | `⌘]` (`Mod+]`) |
| `terminal.focusPrevious` | Focus previous pane | `Mod+[` | `⌘[` (`Mod+[`) | `⌘[` (`Mod+[`) |
| `terminal.search` | Find in terminal | `Mod+F` | `⌘F` (`Mod+F`) | `⌘F` (`Mod+F`) |
| `sidebar.left.toggle` | Toggle sidebar | `Mod+B` | `⌘B` (`Mod+B`) | `⌘B` (`Mod+B`) |
| `commandPalette.open` | Open command palette | `Mod+K` / `Mod+P` / `Mod+J` | `⌘K` (`Mod+K`) | **`⌘K` + `⌘P`** |
| `settings.toggle` | Toggle settings | `Mod+,` | `⌘,` (`Mod+,`) | `⌘,` (`Mod+,`) |
| `zoom.in` | Terminal zoom in | `Mod+=`, `Mod+Shift++` | *Missing* | **`⌘=` + `⌘+` (Added)** |
| `zoom.out` | Terminal zoom out | `Mod+-` | *Missing* | **`⌘-` (Added)** |
| `zoom.reset` | Reset terminal zoom | `Mod+0` | *Missing* | **`⌘0` (Added)** |

---

## 4. Conflict & Interception Analysis

### 4.1 Terminal Input Pass-through vs Global Shortcuts
- In `shortcuts.ts`:
  ```ts
  function isEditableTarget(target: EventTarget | null) {
    if (!(target instanceof HTMLElement)) return false;
    return target.isContentEditable || target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT";
  }
  function isTerminalTarget(target: EventTarget | null) {
    return target instanceof HTMLElement && target.closest(".terminal-host") !== null;
  }
  ```
- **Rule Verification**:
  - When focus is in an input field (e.g. Settings dialog search or Command Palette query), normal text typing and letters (such as `t`, `w`, `b`) are NOT intercepted. Only modal-level escapes (`commandPalette.open`, `settings.toggle`) or dialog keys trigger.
  - When focus is in xterm (`.terminal-host`), `isTerminalTarget` is `true`, so registered application modifier chords (e.g. `⌘T`, `⌘W`, `⌘D`, `⌘F`, `⌘K`, `⌃1..9`, `⌘1..9`) are cleanly intercepted before reaching PTY raw stream.
  - Raw terminal chords (like `Ctrl+C` for SIGINT, `Ctrl+D` for EOF, `Ctrl+R` for bck-i-search, `Ctrl+L` for clear, `Ctrl+Z` for suspend) do not match any `SHORTCUTS` entry and pass directly to the terminal PTY.

### 4.2 Split Pane vs Tab Navigation Symmetry
- Pane Navigation within Active Tab:
  - `⌘]` -> Next Split Pane
  - `⌘[` -> Previous Split Pane
- Tab Navigation across Tabs:
  - `⌘⇧]` -> Next Tab
  - `⌘⇧[` -> Previous Tab
- This adheres strictly to the Apple Human Interface Guidelines and Developer Terminal standards (matching Ghostty, iTerm2, and Safari/Chrome tab behavior).

---

## 5. Architectural Improvements

### 5.1 Multi-Binding / Alias Engine in `shortcuts.ts`
Enhance `ShortcutDefinition` to support primary `binding` plus optional `aliases`:
```ts
export type ShortcutDefinition = {
  id: ShortcutActionId;
  title: string;
  group: "Tabs" | "Terminal Panes" | "Global" | "Workspaces" | "View";
  binding: ShortcutBinding;
  aliases?: readonly ShortcutBinding[];
  source: "original" | "ferryx";
};
```
And update `matchesBinding` to evaluate both the primary binding and all alias bindings.

### 5.2 Dynamic Workspace & Tab 1–9 Index Traversal in `App.tsx`
Expand handlers:
- `workspace.select1` through `workspace.select9` -> `handleSelectWorktreeByIndex(0..8)`
- `tab.select1` through `tab.select9` -> `handleSelectTerminalTabByIndex(0..8)`

### 5.3 Terminal Font Zoom Controls (`zoom.in`, `zoom.out`, `zoom.reset`)
Integrate with `useTerminalSettings`:
- `zoom.in` -> increase `fontSize` by 1 (clamped to max 36)
- `zoom.out` -> decrease `fontSize` by 1 (clamped to min 10)
- `zoom.reset` -> reset `fontSize` to default 13

### 5.4 Search & Category Filtering in Settings Dialog
Upgrade `ShortcutSettings` in `SettingsDialog.tsx`:
- Search input for real-time keyword filtering across title, action ID, and group.
- Group filter tabs (`All`, `Tabs`, `Workspaces`, `Terminal Panes`, `Global`, `View`).
- Clean visual badge formatting for aliases.

### 5.5 Native macOS App Menu in `src-tauri/src/lib.rs`
Construct a complete desktop menu:
- **App Menu**: About Ferryx, Preferences (`Cmd+,`), Hide (`Cmd+H`), Hide Others (`Cmd+Alt+H`), Show All, Quit (`Cmd+Q`).
- **File Menu**: New Terminal Tab (`Cmd+T`), New Browser Tab (`Cmd+Shift+B`), Close Tab (`Cmd+W`).
- **Edit Menu**: Undo, Redo, Cut, Copy (`Cmd+C`), Paste (`Cmd+V`), Select All (`Cmd+A`).
- **View Menu**: Toggle Sidebar (`Cmd+B`), Command Palette (`Cmd+K`), Zoom In (`Cmd+=`), Zoom Out (`Cmd+-`), Reset Zoom (`Cmd+0`).
- **Window Menu**: Minimize (`Cmd+M`), Zoom, Bring All to Front.

---

## 6. Implementation Deliverables & Verification Protocol

1. `ui/src/lib/shortcuts.ts`: Full action registry expansion (1–9, aliases, zoom, tab cycling).
2. `ui/src/lib/shortcuts.test.tsx`: Comprehensive test suite verifying primary & alias matching, platform detection, label formatting, and input suppression.
3. `ui/src/App.tsx`: Handler wiring for 1–9 tabs, 1–9 workspaces, zoom actions, palette aliases.
4. `ui/src/components/SettingsDialog.tsx`: Searchable & filterable ShortcutSettings.
5. `ui/src/components/NewTabPopover.tsx`: Single-source `shortcutLabel()` integration.
6. `src-tauri/src/lib.rs`: Complete native macOS menu setup.
7. Verification: Run all 46+ Vitest test suites, Cargo check/test suites, and build validation.

---
*Report certified and stored in repo root for continuous reference.*
