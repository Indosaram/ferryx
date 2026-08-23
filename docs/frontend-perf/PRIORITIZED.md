# Prioritized frontend perf fixes
Date: 2026-08-22

## Packets
### P-bridge
- Severity: High
- Findings: F-settings-01
- Write scope (exclusive): ui/src/lib/settingsRuntimeBridge.ts, ui/src/lib/settingsRuntime.test.ts
- Must-do: 1) Add a RED test that installSettingsRuntimeBridge does not attach a document-wide MutationObserver (or that a DOM mutation does not call querySelectorAll/loadAppearanceSettings). 2) Remove the subtree MutationObserver; keep change/click/event listeners. 3) GREEN the test.
- RED proof: settingsRuntime.test.ts asserts no MutationObserver on documentElement after install, or mutation does not scan all divs.
- GREEN proof: bun test src/lib/settingsRuntime.test.ts
- UX invariant: appearance/terminal labels still update when the user changes settings controls.

### P-backlog
- Severity: High
- Findings: F-terminal-01
- Write scope (exclusive): ui/src/lib/terminalEvents.ts, ui/src/lib/terminalEvents.test.ts, ui/src/lib/terminalEvents.bus.test.ts
- Must-do: 1) RED test that publishing many small chunks after 512KB does not grow backlog past MAX and does not require quadratic concat of the whole buffer each time (assert chunk-array or equivalent + replay join). 2) Replace string concat+slice with a chunk ring. 3) GREEN tests. Preserve subscribe replay and lifecycle APIs.
- RED proof: new characterization + mutation-sensitive test in terminalEvents.test.ts
- GREEN proof: bun test src/lib/terminalEvents.test.ts src/lib/terminalEvents.bus.test.ts
- UX invariant: new subscribers still receive backlog tail; max size still 512KiB.

### P-decode
- Severity: High
- Findings: F-terminal-02
- Write scope (exclusive): ui/src/lib/terminalOutput.ts, ui/src/lib/terminalOutput.test.ts
- Must-do: 1) RED test that decode of a known base64 payload still yields the same utf8 (pin output bytes). 2) Replace per-char loop with Uint8Array.fromBase64 when present, else a faster path that keeps identical decode. 3) GREEN test.
- RED proof: terminalOutput.test.ts pins decoded string for a multi-chunk base64 fixture
- GREEN proof: bun test src/lib/terminalOutput.test.ts
- UX invariant: streaming decode + finish() behavior unchanged.

### P-entry
- Severity: High
- Findings: F-remote-01, F-bundle-01
- Write scope (exclusive): ui/src/main.tsx, ui/src/index-html.test.ts, ui/src/devRuntimeContract.test.ts, ui/src/manifest.test.ts
- Must-do: 1) RED test or static assertion that main.tsx does not statically import both App and RemoteApp. 2) Conditional dynamic import() so Tauri loads App only and web loads RemoteApp only. 3) Keep installSettingsRuntimeBridge + applyCachedTerminalBackground + SW register behavior. 4) GREEN existing main-related tests; update them if they pin the old static imports.
- RED proof: a test fails while both static imports exist
- GREEN proof: bun test src/index-html.test.ts src/devRuntimeContract.test.ts src/manifest.test.ts
- UX invariant: Tauri still boots App; browser still boots RemoteApp; no branding change.

### P-app
- Severity: Medium
- Findings: F-app-store-02, F-app-store-03, F-app-store-04, F-app-store-05, F-settings-02
- Write scope (exclusive): ui/src/App.tsx, ui/src/App.test.tsx
- Must-do: 1) RED tests for close-guard not depending on full state identity; SettingsDialog/CommandPalette not mounted when closed; SettingsDialog loaded via React.lazy. 2) state-in-ref for close guard; stabilize shortcut handlers via refs so useShortcuts does not rebind on SET_PANE_RATIO; wrap inline handlers in useCallback; conditionally mount CommandPalette/SettingsDialog; lazy-import SettingsDialog. 3) GREEN App.test.tsx.
- RED proof: App.test.tsx new cases
- GREEN proof: bun test src/App.test.tsx
- UX invariant: shortcuts, settings, command palette, close-save still work. Do not rewrite workspaceStore.

### P-split
- Severity: Medium
- Findings: F-terminal-05, F-terminal-06
- Write scope (exclusive): ui/src/components/TerminalSplitView.tsx, ui/src/components/TerminalSplitView.test.tsx
- Must-do: 1) RED test that collisionDetection builds a map once (or does not linearly find per comparison) and fallback session object is stable. 2) Map lookup + module-level fallback session; memo PaneRenderer/PaneLeafView if needed. 3) GREEN existing TerminalSplitView tests plus new ones. Do not edit other TerminalSplitView.*.test.tsx files.
- RED proof: TerminalSplitView.test.tsx
- GREEN proof: bun test src/components/TerminalSplitView.test.tsx
- UX invariant: pane/tab drag-drop still prefers correct edges.

### P-tabbar
- Severity: Medium
- Findings: F-shell-01
- Write scope (exclusive): ui/src/components/TabBar.tsx, ui/src/components/tab-dnd/SortableTab.tsx, ui/src/components/TabBar.test.tsx
- Must-do: 1) RED: SortableTab is memoized / items list memoized / onCancelRename stable. 2) Implement. 3) GREEN TabBar.test.tsx.
- RED proof: TabBar.test.tsx
- GREEN proof: bun test src/components/TabBar.test.tsx
- UX invariant: tab rename, activate, close, dnd still work.

### P-worktree
- Severity: Medium
- Findings: F-shell-02, F-shell-03
- Write scope (exclusive): ui/src/components/WorktreeList.tsx, ui/src/components/Sidebar.tsx, ui/src/components/WorktreeList.test.tsx, ui/src/components/Sidebar.test.tsx, ui/src/components/Sidebar.activity.test.tsx
- Must-do: 1) RED: agent lookup is a Map; worktree rows memoized; Sidebar does not allocate Set on every render for useRef init. 2) Implement. 3) GREEN those tests.
- RED proof: WorktreeList.test.tsx / Sidebar.test.tsx
- GREEN proof: bun test src/components/WorktreeList.test.tsx src/components/Sidebar.test.tsx src/components/Sidebar.activity.test.tsx
- UX invariant: worktree select/delete/activity dots unchanged. No "orca-lite" folder name in UI.

### P-resize
- Severity: Medium
- Findings: F-terminal-03, F-terminal-04
- Write scope (exclusive): ui/src/components/TerminalPane.tsx, ui/src/components/TerminalPane.test.tsx, ui/src/lib/terminalHostManager.ts, ui/src/lib/terminalSettings.ts
- Must-do: 1) RED tests for single resize observer path and applySettings not fitting every instance on each pane mount. 2) Dedup ResizeObserver/rAF; apply settings only to the new instance on mount. 3) GREEN TerminalPane.test.tsx. Do not "fix" terminalHostManager.test.ts pre-existing mock failures except if your change requires a new method — then add it to the mock without skipping tests.
- RED proof: TerminalPane.test.tsx
- GREEN proof: bun test src/components/TerminalPane.test.tsx
- UX invariant: terminals still fit on resize and apply font/theme settings.

### P-settings-hooks
- Severity: Medium
- Findings: F-settings-03
- Write scope (exclusive): ui/src/components/SettingsDialog.tsx, ui/src/components/SettingsDialog.test.tsx, ui/src/components/SettingsDialog.workspace.test.tsx
- Must-do: 1) RED: hooks that subscribe to terminal settings do not run when open=false (inner component gated on open). 2) Implement. 3) GREEN those tests. Do not skip the pre-existing Browser-section failure; do not change Browser labels.
- RED proof: SettingsDialog.test.tsx
- GREEN proof: bun test src/components/SettingsDialog.test.tsx src/components/SettingsDialog.workspace.test.tsx
- UX invariant: settings UI unchanged when open.

### P-favicon
- Severity: Low
- Findings: F-bundle-02
- Write scope (exclusive): ui/index.html
- Must-do: Point favicon at existing ferryx-icon.png (NOT orca-logo.svg, NOT a missing svg). No other HTML changes.
- RED proof: index-html tests may live in P-entry; here just the href exists on disk.
- GREEN proof: test -f ui/src/assets/ferryx-icon.png or ui/public/ferryx-icon.png and grep the new href
- UX invariant: Ferryx branding only.

## Deferred / accepted
- F-app-store-01: full useSyncExternalStore/selector rewrite deferred — high blast radius; contained by P-app + P-tabbar + P-worktree + P-split memos.
- F-shell-04: command palette virtualization — lists are small; slice-limit later.
- F-remote-02: TextEncoder per key — fold only if touching RemoteTerminal; otherwise accepted (tiny alloc).
- F-app-store-04 handled inside P-app.

## Dropped
- Vite usePolling: dev-only, documented in HMR_ROOT_CAUSE_AND_FIX.md.
