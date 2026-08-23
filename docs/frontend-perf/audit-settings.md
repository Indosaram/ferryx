# Audit: settings
Repo: /Users/indo/code/project/orca-lite
Scanned: ui/src/components/SettingsDialog.tsx, ui/src/components/BrowserSettingsPanel.tsx, ui/src/components/BrowserPane.tsx, ui/src/components/BrowserToolbar.tsx, ui/src/components/ProjectDialogs.tsx, ui/src/components/WorktreeDeleteDialog.tsx, ui/src/components/MobileKeyDock.tsx, ui/src/lib/appearanceSettings.ts, ui/src/lib/browserSettings.ts, ui/src/lib/notificationSettings.ts, ui/src/lib/agentsSettings.ts, ui/src/lib/settingsRuntimeBridge.ts, ui/src/lib/shortcuts.ts, HMR_ROOT_CAUSE_AND_FIX.md, ui/src/lib/settingsRuntime.test.ts
Date: 2026-08-22

## Findings

### F-settings-01
- Severity: High
- File: ui/src/lib/settingsRuntimeBridge.ts:122
- Mechanism: `installSettingsRuntimeBridge()` installs a global `MutationObserver` on `document.documentElement` observing `{ childList: true, subtree: true }`. On every single DOM mutation throughout the entire app (e.g. xterm terminal output chunks, cursor rendering, agent state streams, active tab changes), the observer runs `syncGeneralAppearanceLabels(loadAppearanceSettings())` and `syncTerminalSourceLabel()`. This performs synchronous `localStorage.getItem` reads, `JSON.parse`, and multiple unindexed full-DOM searches (`document.querySelectorAll("div")` and `document.querySelectorAll("h2")` converted to arrays and searched via `.find()`), blocking the main thread during high-frequency terminal or UI updates.
- Hot path: yes
- Suggested fix: Remove the global `MutationObserver` subtree listener from `installSettingsRuntimeBridge()`. Reactive UI components (`SettingsDialog`, `AppearanceSettings`, `TerminalSettings`) already manage their state and labels via React props and event subscriptions rather than requiring imperative DOM scraping on every document change.
- Write scope: ui/src/lib/settingsRuntimeBridge.ts
- RED proof:
  ```ts
  const observer = new MutationObserver(() => {
    syncGeneralAppearanceLabels(loadAppearanceSettings());
    syncTerminalSourceLabel();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  ```
  Every DOM modification in the application triggers synchronous `loadAppearanceSettings()` (localStorage read + JSON parse) and `document.querySelectorAll("div")` scans over the entire document tree.

### F-settings-02
- Severity: High
- File: ui/src/components/SettingsDialog.tsx:78
- Mechanism: `SettingsDialog.tsx` is an 1,833-line monolithic component containing 10 configuration sections, complex sub-trees (Agents detection, Remote access QR generation, Browser tabs, Notifications, Terminal controls), and dozens of imported Lucide icons. It is statically imported in `App.tsx` rather than lazy-loaded with `React.lazy` and `Suspense`. This bundles ~70KB+ of modal dialog code, dependencies, and icons directly into the initial application module graph, causing unnecessary bundle bloat and parse/eval overhead during cold boot.
- Hot path: no
- Suggested fix: Wrap `SettingsDialog` in `React.lazy(() => import("./components/SettingsDialog").then(m => ({ default: m.SettingsDialog })))` with a `Suspense` boundary in `App.tsx` so the entire dialog chunk is loaded on demand only when the user opens Settings.
- Write scope: ui/src/App.tsx, ui/src/components/SettingsDialog.tsx
- RED proof:
  `ui/src/App.tsx:16` statically imports the full dialog:
  ```tsx
  import { SettingsDialog } from "./components/SettingsDialog";
  ```
  Even though `open={isSettingsOpen}` is `false` on initial startup, all 1,833 lines and its full dependency tree are parsed and evaluated during application boot.

### F-settings-03
- Severity: Medium
- File: ui/src/components/SettingsDialog.tsx:88
- Mechanism: In `SettingsDialog`, `const { settings, nativePreferences, updateSettings, refreshNativePreferences } = useTerminalSettings();` and `useState` hooks run unconditionally before the `if (!open) return null;` early exit. When `SettingsDialog` is mounted with `open={false}` in `App.tsx`, `useTerminalSettings` registers event listeners and initializes terminal preference state. Any terminal setting changes across the app cause the closed `SettingsDialog` to re-render in the background.
- Hot path: no
- Suggested fix: Move `useTerminalSettings()` down into the `TerminalSettings` section component or wrap the internal dialog content in an inner component rendered only when `open` is true.
- Write scope: ui/src/components/SettingsDialog.tsx
- RED proof:
  ```tsx
  export function SettingsDialog({
    open,
    onClose,
    projects = [],
    activeProjectId,
    activeWorktree = null,
    onSelectProject,
    onAddProject,
    onAddWorktree,
  }: SettingsDialogProps) {
    const { settings, nativePreferences, updateSettings, refreshNativePreferences } = useTerminalSettings();
    const [section, setSection] = useState<SettingsSection>("general");
    const isMac = isMacShortcutPlatform();
    ...
    if (!open) return null;
  ```
  `useTerminalSettings()` executes on every render while `open === false`, maintaining unnecessary hook subscriptions.

## Non-findings / accepted
- `BrowserPane` hidden state handling: When `visible === false`, `BrowserPane` calls `setBrowserVisible(tab.browserId, false)` and short-circuits bounds IPC without invoking expensive layout bounding client rect work. ResizeObserver and window listeners cleanly hide the native webview on unmount.
- `ProjectDialogs` modal lifecycle: `AddProjectDialog` and `AddWorktreeDialog` are conditionally mounted by the parent dialog manager only when actively open, preventing background IPC calls or idle DOM overhead.
- `WorktreeDeleteDialog`: Only mounts when a delete action is initiated; branch deletion preview IPC is cancelled cleanly if closed before resolution.
- `MobileKeyDock`: Renders purely based on props and local latching state without polling timers or continuous rAF loops.
- `useAppearanceSettings`, `useBrowserSettings`, `useNotificationSettings`: Use lazy state initialization (`useState(loadSettings)` passing function reference) avoiding `localStorage` / `JSON.parse` work on parent re-renders. Storage and window custom events are registered once per hook mount.
- `shortcuts.ts` dispatch efficiency: Global shortcut map contains 36 static chord definitions checked via single window `keydown` listener; no layout reading or per-frame cost.
- Vite `server.watch.usePolling`: Dev-only file watching configuration documented in `HMR_ROOT_CAUSE_AND_FIX.md`; not present in production builds.

## Scan coverage
- files read:
  - ui/src/components/SettingsDialog.tsx
  - ui/src/components/BrowserSettingsPanel.tsx
  - ui/src/components/BrowserPane.tsx
  - ui/src/components/BrowserToolbar.tsx
  - ui/src/components/ProjectDialogs.tsx
  - ui/src/components/WorktreeDeleteDialog.tsx
  - ui/src/components/MobileKeyDock.tsx
  - ui/src/lib/appearanceSettings.ts
  - ui/src/lib/browserSettings.ts
  - ui/src/lib/notificationSettings.ts
  - ui/src/lib/agentsSettings.ts
  - ui/src/lib/settingsRuntimeBridge.ts
  - ui/src/lib/shortcuts.ts
  - HMR_ROOT_CAUSE_AND_FIX.md
  - ui/src/lib/settingsRuntime.test.ts
- patterns checked: whole-store re-render, missing memo, inline object/fn identity, list virtualization, work in render, effect churn, rAF loops, layout reads during drag, JSON.parse on hot path, xterm recreate, code-splitting
