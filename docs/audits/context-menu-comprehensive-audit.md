# Comprehensive Context Menu Architecture & Code Review

**Date:** 2026-09-04  
**Scope:** Ferryx Frontend (`ui/src`) & Tauri Native Backend (`src-tauri/src`)  
**Status:** Complete Audit  

---

## 1. Executive Summary

Ferryx's context menu architecture is currently minimal and centralized in a single component (`ui/src/components/TabBar.tsx`). While the tab context menu handles tab management (splitting, pinning, renaming, closing, and browser duplication) with good integration into the workspace store, the rest of the application lacks context menu handling entirely.

Crucially, because there is no global `contextmenu` event interception in the webview, right-clicking on unhandled areas (terminal panes, sidebar items, empty tab bar areas, and toolbars) causes the default WebKit (macOS), WebView2 (Windows), or WebKitGTK (Linux) browser context menu ("Inspect Element", "Reload", "Back", "Forward") to appear. This is a noticeable desktop polish papercut.

Furthermore, the existing `TabContextMenuPopup` exhibits several edge-case bugs:
- Hardcoded height and width bounding that causes menu items to clip off-screen near window edges.
- Lack of keyboard navigation (arrow keys) and focus trap according to WAI-ARIA Menu specifications.
- Incomplete dismiss triggers (scroll, blur).
- Missing terminal right-click reporting and terminal context menu.

---

## 2. Inventory of Current Implementation

### 2.1 TabBar (`ui/src/components/TabBar.tsx` & `SortableTab.tsx`)
- **Trigger**: `onContextMenu` on each tab (`SortableTab`), calling `event.preventDefault()` and `event.stopPropagation()`.
- **State**: `contextMenu: { tabId: string, x: number, y: number } | null` managed via `useState`.
- **Component**: `TabContextMenuPopup` rendered as a `fixed` position `div`.
- **Capabilities**:
  - `BrowserDuplicateControl`: Profile selection and duplication for browser tabs.
  - Split terminal right (`onSplitRight`) - disabled for browser tabs.
  - Split terminal down (`onSplitDown`) - disabled for browser tabs.
  - Move Tab to Split (`onMoveTabToSplit`: right, bottom, left, top).
  - Pin/Unpin tab (`onTogglePin`).
  - Rename tab (`onRename`).
  - Close tab (`onCloseTab`) - disabled if tab is pinned.
  - Close other tabs (`onCloseOthers`).
  - Close tabs to right (`onCloseToRight`).
  - Close tabs to left (`onCloseToLeft`).

### 2.2 Native Backend (`src-tauri`)
- **App Menu**: `src-tauri/src/lib.rs` configures the standard desktop application menu (Ferryx, File, Edit, View, Window).
- **Native Context Menu**: No native context menu APIs (`NSMenu`, `TrackPopupMenu`, `tauri-plugin-context-menu`) are registered or used.
- **Mouse Tracking in Engine**: `src-tauri/src/native_terminal/mouse.rs` and `mouse_encoder.rs` define `MouseButton::Right = 2` and support SGR mouse reporting to the PTY, but frontend input routing in `NativeTerminalPane.tsx` drops right-click events (`event.button === 0` only).

---

## 3. In-Depth Code Quality & Bug Analysis

### 3.1 Hardcoded Boundary Clamping in `TabContextMenuPopup`
```tsx
const left = Math.min(state.x, window.innerWidth - 200);
const top = Math.min(state.y, window.innerHeight - 260);
```
- **Height Overflow**: The menu can contain up to 12 buttons, 3 separators, and profile selection controls, totaling ~350–370px in height. When right-clicking a tab in a short window or near the bottom, clamping at `window.innerHeight - 260` results in the bottom 90–110px of the menu being pushed below the viewport and clipped.
- **Width Overflow**: When long labels (e.g. "Move Tab to Split Bottom") and browser profile dropdowns render, the menu can exceed 200px in width. Clamping at `window.innerWidth - 200` causes the right side of the menu to bleed off-screen.
- **Negative Coordinate Vulnerability**: There is no lower bound (`Math.max(0, ...)`), leaving the menu vulnerable to negative coordinates on multi-monitor or boundary clicks.

### 3.2 Dismissal Lifecycle Gaps
- **Horizontal Scrolling**: If the user scrolls the tab bar using a trackpad or mouse wheel (`wheel` event), the fixed-position menu remains floating while tabs scroll underneath it.
- **Window Blur**: When the application window loses focus (`blur`), the context menu remains open.
- **Resize**: Resizing the window while the menu is open does not dismiss or reposition the menu.

### 3.3 Accessibility & Keyboard Interaction (WAI-ARIA)
- Although `role="menu"` and `role="menuitem"` attributes are present, standard keyboard navigation is missing:
  - Up/Down arrow keys do not cycle through menu items.
  - Home/End keys do not jump to the first/last menu item.
  - Focus is not trapped inside the menu; pressing `Tab` blurs the menu and navigates background DOM elements.
  - Initial focus is not set to the first enabled menu item upon opening.
  - Keyboard shortcut hints (e.g. `Cmd+W` next to Close Tab, `Cmd+D` next to Split) are not displayed.
- `BrowserDuplicateControl` places an interactive `<select>` alongside a `<button role="menuitem">` inside a `role="menu"` container, which violates the strict WAI-ARIA menu pattern.

---

## 4. Missing Context Menu Surfaces Across the Application

### 4.1 Terminal Panes (`NativeTerminalPane.tsx`, `TerminalPane.tsx`)
- **Current Behavior**: Right-clicking anywhere on the terminal pane allows the event to bubble unprevented to the webview, opening the browser's default context menu ("Inspect Element", "Back", "Forward", "Reload").
- **Expected Terminal Features**:
  - Copy selection (if selection exists).
  - Paste from clipboard.
  - Select all.
  - Clear buffer / Reset terminal.
  - Split pane right / down.
  - Close pane.
- **PTY Mouse Reporting**: When a running CLI tool requests mouse tracking (DECSET 1000/1002/1006, such as in vim, htop, or tmux), right-clicks should be dispatched to the backend via `cmd_native_terminal_mouse` (`button: "Right"`).

### 4.2 Sidebar (`Sidebar.tsx`, `WorktreeList.tsx`, `SortableProjectSection.tsx`)
- **Worktree Rows**:
  - Currently only have a hover delete button.
  - Missing context menu options: "Open in Terminal", "Open in New Tab", "Copy Worktree Path", "Copy Branch Name", "Reveal in Finder / Explorer", "Delete Worktree".
- **Project Rows**:
  - Missing context menu options: "Add Worktree", "Reveal in Finder / Explorer", "Copy Project Path", "Close Project".

### 4.3 Tab Bar Blank Area (`TabBar.tsx`)
- Right-clicking empty space in the tab bar triggers the browser default menu instead of:
  - "New Terminal Tab"
  - "New Browser Tab"
  - "Reopen Closed Tab"

### 4.4 Global App Context Menu Suppression
- In desktop mode, unhandled right-clicks across the app (header, dividers, settings, backgrounds) reveal the raw browser webview context menu.
- Recommended pattern: Install a global `contextmenu` listener that calls `event.preventDefault()` in production, while permitting developer inspection in development mode (`import.meta.env.DEV`).

---

## 5. Recommended Architecture & Action Plan

1. **Global Default Suppression Guard**:
   - Prevent default browser context menu globally across desktop webviews in production.
   - Allow exceptions for text input fields (`<input>`, `<textarea>`) and development builds.

2. **Unified ContextMenu Primitive**:
   - Create a reusable, accessible context menu component (`ui/src/components/ui/ContextMenu.tsx`) or utilize `@radix-ui/react-context-menu`.
   - Ensure dynamic viewport measurement, collision detection, keyboard arrow navigation, focus management, and escape/scroll/blur auto-dismissal.

3. **Surface-Specific Context Menus**:
   - Refactor `TabBar.tsx` to use the unified menu primitive.
   - Implement Worktree/Project context menus in `Sidebar.tsx`.
   - Implement Terminal context menu and enable right-click mouse tracking in `NativeTerminalPane.tsx`.
