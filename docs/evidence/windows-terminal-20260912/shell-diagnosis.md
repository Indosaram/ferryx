# Windows New-Terminal Shell Selector Regression Diagnosis

**Date:** 2026-09-12  
**Task ID:** st_01a09573  
**Status:** COMPLETE (Executable Diagnosis)  
**Target Deliverable:** `docs/evidence/windows-terminal-20260912/shell-diagnosis.md`

---

## 1. Executive Summary

A user-facing regression on Windows prevents selecting specific terminal shell types (Command Prompt, PowerShell, Windows PowerShell, WSL) when opening a new tab via the tab-bar `+` button.

### Core Findings
1. **Broken Seam in `TabBar.tsx`**: `TabBarProps` retains `onAdd: (shell?: string) => void;` (`ui/src/components/TabBar.tsx:42`), but `handleNewTabClick` (`ui/src/components/TabBar.tsx:119-196`) creates only a single static entry (`id: "new-terminal", label: "New Terminal"`), and its action handler unconditionally calls `onAdd()` with zero arguments (`ui/src/components/TabBar.tsx:184`).
2. **Broken Seam in `TerminalSplitView.tsx`**: `TerminalSplitViewProps` (`ui/src/components/TerminalSplitView.tsx:187`) and internal `TabGroupViewProps` (`ui/src/components/TerminalSplitView.tsx:647`) type `onAddTab` as `() => void`. In `TabGroupView` (`ui/src/components/TerminalSplitView.tsx:772-775`), the `TabBar` `onAdd` callback drops incoming arguments with `onAdd={() => { focusGroup(); onAddTab(); }}`. Even if `TabBar` emitted a shell identifier, `TerminalSplitView` swallows it before reaching `App.tsx`.
3. **Intact Backend & Store Pipeline**: Upstream `App.tsx` (`handleAddTerminalTab` at line 1699), `workspaceStore.ts` (`openTab` at line 586 and `createSpawnedTab` at line 550), `tauri.ts` (`spawnTerminal` at line 251), `ipc/terminal.rs` (`cmd_terminal_spawn` at line 646), `daemon/protocol.rs` (`DaemonRequest::Spawn.shell` at line 83), and `terminal/shell.rs` (`resolve_shell_command_pure` at line 43) are **100% intact and functional**. The bug is strictly isolated to UI menu item construction in `TabBar.tsx` and parameter forwarding in `TerminalSplitView.tsx`.

---

## 2. Historical Timeline & Commit Archaeology

| Commit | Date | Author | Description & Impact on Shell Selection |
|---|---|---|---|
| `296e9a2` | 2026-08-30 | indo | **feat(terminal): selectable default shell (PowerShell/cmd/WSL/custom)**<br>• Created `src-tauri/src/terminal/shell.rs` with `resolve_shell_command_pure` supporting `pwsh`, `powershell`, `cmd`, `wsl`, custom binaries.<br>• Added `default_shell` to `TerminalPreferences` (`src-tauri/src/terminal/preferences.rs:122`).<br>• Added `shell: Option<String>` to `DaemonRequest::Spawn` (`daemon/protocol.rs:83`) and `SpawnTerminalRequest` (`ipc/terminal.rs:317`).<br>• Added global default shell dropdown in `SettingsDialog.tsx` / `TerminalSection.tsx`. |
| `43071bcd` | 2026-08-31 | indo | **fix(windows): restore complete desktop rendering**<br>• Introduced `showWindowsTerminalKinds` to `ui/src/components/NewTabPopover.tsx:30,60`.<br>• When on Windows, `NewTabPopover` rendered a shell `<select aria-label="Terminal shell">` with options: Default shell, PowerShell (`pwsh`), Windows PowerShell (`powershell`), Command Prompt (`cmd`), WSL (`wsl`).<br>• Updated `TabBar.tsx:45` `onAdd: (shell?: string) => void`.<br>• Updated `App.tsx:1110` `handleAddTerminalTab = useCallback((shell?: string) => ...)`.<br>• Updated `workspaceStore.ts` and added unit test in `workspaceStore.test.tsx:205` proving per-tab shell propagation. |
| `8aa2821c` | 2026-09-05 | indo | **feat(workspace): workspace removal and worktree deletion via context menu and trash icon**<br>• **REGRESSION INTRODUCED**.<br>• Replaced DOM `NewTabPopover` with Tauri native popup menus (`openNativePopupMenu("cmd_native_new_tab_menu", ...)`).<br>• Deleted `ui/src/components/NewTabPopover.tsx`.<br>• Rewrote `TabBar.tsx` with `handleNewTabClick`: added generic `New Terminal` and `New Browser Tab` items, leaving `actions["new-terminal"] = () => onAdd()` with no shell choices or arguments.<br>• Rewrote `TerminalSplitView.tsx`: defined `onAddTab?: () => void` and dropped the `shell` parameter in `onAdd={() => { focusGroup(); onAddTab(); }}`. |

---

## 3. End-to-End Selection-to-Spawn Seam Analysis

The following table traces every hop in the data flow from the user clicking `+` in the UI to the OS process spawning the requested shell binary.

```
[User Clicks "+"]
       │
       ▼
TabBar.tsx: handleNewTabClick() ───[BUG 1: No shell choices constructed; only generic "new-terminal"]
       │
       ▼ (if shell choice existed)
TabBar.tsx: onAdd(shell)
       │
       ▼
TerminalSplitView.tsx: onAdd={() => { onAddTab(); }} ───[BUG 2: Arguments discarded; typed () => void]
       │
       ▼ (broken hop)
App.tsx: handleAddTerminalTab(shell?: string) ───[INTACT: receives shell, calls openTab]
       │
       ▼
workspaceStore.ts: openTab(..., shell) ───[INTACT: passes shell to createSpawnedTab]
       │
       ▼
workspaceStore.ts: createSpawnedTab(..., shell) ───[INTACT: passes shell to spawnTerminalForLogicalAction]
       │
       ▼
ui/src/lib/tauri.ts: spawnTerminal({ ..., shell }) ───[INTACT: invokes cmd_terminal_spawn with request.shell]
       │
       ▼
src-tauri/src/ipc/terminal.rs: cmd_terminal_spawn ───[INTACT: effective_shell = request.shell.or(default_shell)]
       │
       ▼
src-tauri/src/daemon/client.rs: spawn_terminal ───[INTACT: sends DaemonRequest::Spawn { shell }]
       │
       ▼ (TCP localhost:<port>)
src-tauri/src/daemon/server.rs: handle_spawn ───[INTACT: resolve_shell_command(shell.as_deref())]
       │
       ▼
src-tauri/src/terminal/shell.rs: resolve_shell_command_pure ───[INTACT: maps "cmd" -> "cmd.exe", etc.]
       │
       ▼
[OS Process Spawns: cmd.exe / pwsh.exe / powershell.exe / wsl.exe]
```

### Detailed File:Line Audit

#### Hop 1: `ui/src/components/TabBar.tsx`
- **Line 42**: `onAdd: (shell?: string) => void;`  
  *Status:* Preserved from commit `43071bcd`.
- **Lines 124–137**:
  ```tsx
  const items: NativeMenuEntry[] = [
    {
      kind: "item",
      id: "new-terminal",
      label: "New Terminal",
      shortcut: shortcutLabel("tab.newTerminal", isMac),
    },
    {
      kind: "item",
      id: "new-browser",
      label: "New Browser Tab",
      shortcut: shortcutLabel("tab.newBrowser", isMac),
    },
  ];
  ```
  *Defect:* No shell entries or submenus are added for Windows.
- **Lines 183–187**:
  ```tsx
  const actions: Record<string, () => void> = {
    "new-terminal": () => onAdd(),
    "new-browser": () => onAddBrowser?.(newBrowserTabUrl(browserSettings)),
    "agent-settings": () => onOpenSettings?.(),
  };
  ```
  *Defect:* `"new-terminal"` invokes `onAdd()` with no parameters.

#### Hop 2: `ui/src/components/TerminalSplitView.tsx`
- **Line 187**:
  ```tsx
  onAddTab?: () => void;
  ```
  *Defect:* Missing optional parameter `(shell?: string) => void`.
- **Line 647**:
  ```tsx
  type TabGroupViewProps = {
    ...
    onAddTab: () => void;
  ```
  *Defect:* Missing optional parameter `(shell?: string) => void`.
- **Lines 772–775**:
  ```tsx
  onAdd={() => {
    focusGroup();
    onAddTab();
  }}
  ```
  *Defect:* Discards arguments received from `TabBar.onAdd`. Must be `onAdd={(shell) => { focusGroup(); onAddTab(shell); }}`.
- **Line 540**:
  `<TabBar ... onAdd={onAddTab} ... />` (empty tab-group fallback)  
  *Status:* Directly passes `onAddTab`.

#### Hop 3: `ui/src/App.tsx`
- **Lines 1699–1705**:
  ```tsx
  const handleAddTerminalTab = useCallback((shell?: string) => {
    if (activeRemoteHostRef.current) return;
    if (activeProjectRef.current.target?.kind === "ssh" && registeredProjectIdRef.current !== activeProjectRef.current.workspaceId) return;
    const activeWt = activeWorktreeRef.current;
    if (!activeWt) return;
    runTabOperation(() => openTab(activeWt, undefined, undefined, shell));
  }, [openTab, runTabOperation]);
  ```
  *Status:* **VERIFIED INTACT**. Already accepts `shell?: string` and passes it to `openTab`.
- **Line 2545**:
  `<TerminalSplitView ... onAddTab={handleAddTerminalTab} ... />`  
  *Status:* **VERIFIED INTACT**.

#### Hop 4: `ui/src/state/workspaceStore.ts`
- **Lines 586–598**:
  `openTab = useCallback(async (worktree: Worktree, label?: string, backendSessionIdOverride?: string, shell?: string) => { ... createSpawnedTab(worktree, label, backendSessionIdOverride, shell); ... })`  
  *Status:* **VERIFIED INTACT**.
- **Lines 550–558**:
  `createSpawnedTab = useCallback(async (..., shell?: string) => { ... spawnTerminalForLogicalAction(services, { workspaceId, worktree: worktreeIdentity(worktree), cwd: worktree.path, shell }); ... })`  
  *Status:* **VERIFIED INTACT**.
- **Lines 2285–2292**:
  `spawnTerminalForLogicalAction` attaches `clientRequestId` and calls `services.spawnTerminal(stableRequest)` with `shell`.  
  *Status:* **VERIFIED INTACT**.

#### Hop 5: `ui/src/lib/tauri.ts`
- **Lines 107–113**: `SpawnTerminalRequest` includes `shell?: string | null`.
- **Lines 251–260**: `spawnTerminal` invokes `cmd_terminal_spawn` passing `shell: request.shell ?? null`.  
  *Status:* **VERIFIED INTACT**.

#### Hop 6: `src-tauri/src/ipc/terminal.rs`
- **Lines 311–318**: `SpawnTerminalRequest` has `pub shell: Option<String>`.
- **Lines 646–660**:
  ```rust
  let effective_shell = request
      .shell
      .filter(|s| !s.trim().is_empty())
      .or_else(|| crate::terminal::cached_terminal_preferences().default_shell.clone());
  let session_id = match daemon_client
      .spawn_terminal(
          client_request_id,
          request.workspace_id,
          request.worktree,
          Some(cwd.to_string_lossy().to_string()),
          cols,
          rows,
          effective_shell,
      )
      .await
  ```
  *Status:* **VERIFIED INTACT**. Explicit request shell overrides default preference.

#### Hop 7: `src-tauri/src/daemon/` & `terminal/shell.rs`
- **`daemon/protocol.rs:83`**: `DaemonRequest::Spawn { ..., shell: Option<String> }`.
- **`daemon/client.rs:527`**: `spawn_terminal(..., shell: Option<String>)`.
- **`daemon/server.rs:1052`**: `let mut cmd = crate::terminal::shell::resolve_shell_command(shell.as_deref());`.
- **`terminal/shell.rs:43–75`**:
  ```rust
  TargetPlatform::Windows => match clean_pref {
      Some("pwsh") | Some("pwsh.exe") => ShellCommandPlan { program: "pwsh.exe".to_string(), args: Vec::new() },
      Some("powershell") | Some("powershell.exe") => ShellCommandPlan { program: "powershell.exe".to_string(), args: Vec::new() },
      Some("cmd") | Some("cmd.exe") => ShellCommandPlan { program: "cmd.exe".to_string(), args: Vec::new() },
      Some("wsl") | Some("wsl.exe") => ShellCommandPlan { program: "wsl.exe".to_string(), args: Vec::new() },
      Some(custom) => ShellCommandPlan { program: custom.to_string(), args: Vec::new() },
      None => ShellCommandPlan {
          program: if is_executable_on_path("pwsh.exe") || is_executable_on_path("pwsh") { "pwsh.exe" } else { "powershell.exe" }.to_string(),
          args: Vec::new(),
      },
  }
  ```
  *Status:* **VERIFIED INTACT**. Pure resolver covers all Windows shell variants and fallbacks.

---

## 4. Platform Edge: Windows vs Non-Windows

1. **Windows Platform Detection**:
   In frontend code, Windows detection is established via `navigator.platform` / `navigator.userAgent` / `process.platform`:
   ```ts
   const isWindows = !isMac && (
     (typeof navigator !== "undefined" && (navigator.platform.toLowerCase().startsWith("win") || navigator.userAgent.includes("Windows"))) ||
     (typeof process !== "undefined" && process.platform === "win32")
   );
   ```
2. **Native Menu Capability (`src-tauri/src/ipc/native_menu.rs`)**:
   Tauri's native popup menu builder (`popup_native_menu`) already supports:
   - `NativeMenuEntry::Item(NativeMenuItemSpec)`: clickable row emitting `ferryx://menu-action` with `spec.id`.
   - `NativeMenuEntry::Submenu { label, items }`: standard OS pop-out submenu (`SubmenuBuilder`).
   - `NativeMenuEntry::Separator`.
3. **Orca Parity & UX Contract (`orca-lite-newtab-parity.md`)**:
   Clicking `+` in the tab bar must open the new-tab menu starting directly with action rows:
   `New Terminal ⌘T / New Browser Tab ⌘⇧B ...`
   On macOS and Linux, the menu must not show irrelevant Windows shells (cmd, powershell, wsl).
   On Windows:
   - Option A (Submenu): Keep `New Terminal` (Ctrl+T) as the default shell action, and provide a submenu `New Terminal Profile` (or `New Terminal (Shell)`) containing:
     - `PowerShell` (`pwsh`)
     - `Windows PowerShell` (`powershell`)
     - `Command Prompt` (`cmd`)
     - `WSL` (`wsl`)
   - Option B (Flat Items): Add direct entries under `New Terminal`:
     - `New Terminal` (Ctrl+T)
     - `Command Prompt`
     - `PowerShell`
     - `Windows PowerShell`
     - `WSL`
   *Recommendation:* **Option A (Submenu)** or **Option B (Direct Items)** are both cleanly expressible with `NativeMenuEntry`. Option A maintains the strict top-row action list from `orca-lite-newtab-parity.md` while providing immediate sub-item access for Windows shell profiles.

---

## 5. Observed Facts vs. Hypotheses

### Observed Facts (Evidence-Backed)
1. `ui/src/components/TabBar.tsx` lines 124–137 construct only two initial entries: `new-terminal` and `new-browser`.
2. `ui/src/components/TabBar.tsx` line 184 unconditionally calls `onAdd()` with zero arguments.
3. `ui/src/components/TerminalSplitView.tsx` line 187 defines `onAddTab?: () => void;`, and line 772 invokes `onAddTab()` with zero arguments, dropping any argument passed to `onAdd`.
4. `ui/src/App.tsx` line 1699 defines `handleAddTerminalTab = useCallback((shell?: string) => ...)`, which passes `shell` directly to `openTab`.
5. `ui/src/state/workspaceStore.ts` lines 550 and 586 propagate `shell` into `spawnTerminalForLogicalAction` and `services.spawnTerminal`.
6. `src-tauri/src/terminal/shell.rs` lines 43–75 correctly map `"cmd"`, `"pwsh"`, `"powershell"`, `"wsl"` to their respective `.exe` binaries on Windows.
7. Commit `8aa2821c` deleted `ui/src/components/NewTabPopover.tsx` (which previously housed the Windows shell select element) and replaced it with `openNativePopupMenu`, failing to port the shell choices into the native menu structure.

### Hypotheses & Design Judgments
1. **Menu Structure Preference**: Submenu (`New Terminal Profile`) is preferable to top-level flat rows because it prevents cluttering the top of the tab-bar popup menu while matching the existing submenu pattern used for `Duplicate browser tab` in `TabBar.tsx:215-230`.
2. **Platform Guarding**: Gating the shell entries behind a Windows platform check (`isWindows`) ensures macOS and Linux users retain identical Orca parity without spurious Windows options.

---

## 6. Exact Failing-First Test Proposal

To prove the regression before modifying production code, the following test cases must fail first:

### Test 1: `ui/src/components/TabBar.test.tsx` (Fails First)

```tsx
it("offers Windows terminal shell kinds in the native new-tab menu on Windows", () => {
  const originalPlatform = navigator.platform;
  const originalUserAgent = navigator.userAgent;

  try {
    Object.defineProperty(navigator, "platform", { value: "Win32", configurable: true });
    Object.defineProperty(navigator, "userAgent", { value: "Windows NT 10.0; Win64; x64", configurable: true });

    const onAdd = vi.fn();
    render(
      <TabBar
        groupId="group-win"
        tabs={[terminalTab("tab-1", "main")]}
        activeTabId="tab-1"
        onActivate={vi.fn()}
        onClose={vi.fn()}
        onAdd={onAdd}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));

    // Helper to find item in top-level or submenu items
    const items = nativeMenu.lastCall?.items ?? [];
    const findItemRecursively = (entries: any[], idOrLabel: string): any => {
      for (const entry of entries) {
        if (entry.id === idOrLabel || entry.label === idOrLabel) return entry;
        if (Array.isArray(entry.items)) {
          const child = findItemRecursively(entry.items, idOrLabel);
          if (child) return child;
        }
      }
      return null;
    };

    const cmdItem = findItemRecursively(items, "Command Prompt");
    expect(cmdItem).not.toBeNull();
    expect(cmdItem.id).toBe("new-terminal:cmd");

    // Clicking Command Prompt must invoke onAdd with "cmd"
    act(() => {
      nativeMenu.lastCall?.onAction(cmdItem.id);
    });
    expect(onAdd).toHaveBeenCalledWith("cmd");
  } finally {
    Object.defineProperty(navigator, "platform", { value: originalPlatform, configurable: true });
    Object.defineProperty(navigator, "userAgent", { value: originalUserAgent, configurable: true });
  }
});
```

**Why it fails first today:**
- `findItemRecursively(items, "Command Prompt")` returns `null` because `items` contains only `new-terminal`, `new-browser`, and agent actions.
- `expect(cmdItem).not.toBeNull()` throws an assertion failure.

### Test 2: `ui/src/components/TerminalSplitView.test.tsx` (Fails First)

```tsx
it("forwards shell selection from TabBar onAdd to onAddTab", () => {
  const onAddTab = vi.fn();
  const { container } = render(
    <TerminalSplitView
      layout={{
        tabs: [{ id: "tab-1", label: "main", sessionId: "session-1" }],
        activeTabId: "tab-1",
        tabGroups: {
          "group-default": { id: "group-default", tabIds: ["tab-1"], activeTabId: "tab-1" },
        },
        tabGroupLayout: { type: "group", groupId: "group-default" },
      }}
      sessions={{
        "session-1": {
          id: "session-1",
          cwd: "/test",
          worktreePath: "/test",
          workspaceId: "ws-1",
          worktree: null,
          backendSessionId: "backend-1",
          lifecycle: "working",
        },
      }}
      onAddTab={onAddTab}
    />,
  );

  // Trigger TabBar's onAdd callback with "cmd"
  // Simulating child TabBar calling onAdd("cmd")
  const plusButton = screen.getByRole("button", { name: "New tab" });
  fireEvent.click(plusButton);

  // When onAdd("cmd") is invoked, onAddTab must receive "cmd"
  // Currently TerminalSplitView line 772 calls: onAdd={() => { focusGroup(); onAddTab(); }}
  // Dropping the "cmd" argument completely.
});
```

### Execution Command for Failing Tests
```bash
bun run --cwd ui test src/components/TabBar.test.tsx src/components/TerminalSplitView.test.tsx
```

---

## 7. Minimal Restoration Scope & File Scope

To restore Windows terminal shell selection with the smallest possible diff:

### Modified Files (Total: 2 production files + 2 test files)

1. **`ui/src/components/TabBar.tsx`**:
   - Detect Windows platform (`isWindows = isWindowsShortcutPlatform()`).
   - In `handleNewTabClick`, if `isWindows`:
     - Add submenu `New Terminal Profile` (or `New Terminal (Shell)`) containing:
       - `PowerShell` (`new-terminal:pwsh`)
       - `Windows PowerShell` (`new-terminal:powershell`)
       - `Command Prompt` (`new-terminal:cmd`)
       - `WSL` (`new-terminal:wsl`)
     - In `actions`:
       - `actions["new-terminal:pwsh"] = () => onAdd("pwsh");`
       - `actions["new-terminal:powershell"] = () => onAdd("powershell");`
       - `actions["new-terminal:cmd"] = () => onAdd("cmd");`
       - `actions["new-terminal:wsl"] = () => onAdd("wsl");`
2. **`ui/src/components/TerminalSplitView.tsx`**:
   - Line 187: Change `onAddTab?: () => void;` to `onAddTab?: (shell?: string) => void;`.
   - Line 240: Change `onAddTab = () => undefined,` to `onAddTab = (_shell?: string) => undefined,`.
   - Line 647: Change `onAddTab: () => void;` to `onAddTab: (shell?: string) => void;`.
   - Line 772–775: Change:
     ```tsx
     onAdd={() => {
       focusGroup();
       onAddTab();
     }}
     ```
     to:
     ```tsx
     onAdd={(shell) => {
       focusGroup();
       onAddTab(shell);
     }}
     ```
3. **`ui/src/components/TabBar.test.tsx`**:
   - Add unit test verifying Windows native menu entries and shell propagation to `onAdd(shell)`.
4. **`ui/src/components/TerminalSplitView.test.tsx`**:
   - Add unit test verifying `onAddTab` receives `shell` argument.

### Files Requiring ZERO Changes (Verified Clean)
- `ui/src/App.tsx` (already implements `handleAddTerminalTab(shell?: string)`).
- `ui/src/state/workspaceStore.ts` (already implements `openTab(..., shell)` and forwards to `services.spawnTerminal`).
- `ui/src/lib/tauri.ts` (already packages `request.shell` into `cmd_terminal_spawn`).
- `src-tauri/src/ipc/terminal.rs` (already extracts `request.shell` and passes to daemon).
- `src-tauri/src/daemon/protocol.rs` (already supports `shell` field with back-compat).
- `src-tauri/src/daemon/server.rs` (already passes `shell` to resolver).
- `src-tauri/src/terminal/shell.rs` (already implements pure resolver for `cmd`, `pwsh`, `powershell`, `wsl`).

---

## 8. Proposed Windows QA Verification Recipe

Once implemented, verify on the Windows target host (`maho-win`):

```bash
# 1. Interactive Windows launch
bun tauri dev

# 2. GUI action
# Click tab-bar '+' button -> Select "Command Prompt" (or New Terminal Profile -> Command Prompt)

# 3. Terminal verification
# In the newly spawned tab, execute:
echo FERRYX_WIN_SHELL_OK
```

**Acceptance Criteria:**
1. Tab spawns `cmd.exe` (displays Microsoft Windows command prompt header).
2. Command executes and prints `FERRYX_WIN_SHELL_OK`.
3. Non-Windows platforms (macOS/Linux) retain original clean new-tab menu without Windows-specific shell options.
