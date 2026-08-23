# Mass ULW: Settings and Remote Reliability

## Scope

Resolve the seven user-reported settings and Remote Access failures without modifying unrelated shared-worktree changes.

## Tier

HEAVY: persistence, Remote Access lifecycle across application restarts, and several user-facing Settings domains cross frontend and native runtime boundaries.

## Topology

### Investigation DAG — parallel read-only wave

1. **Settings persistence** (`quick`) — trace storage and startup restoration for Appearance and related Settings.
2. **Remote lifecycle and pairing** (`deep`) — trace native gateway enable/disable persistence, restart initialization, and pairing-PIN UI/API paths.
3. **Settings information architecture** (`quick`) — audit General, Quick Commands, and Default Agent behavior/copy.
4. **Workspace state** (`unspecified-low`) — trace project registration through App, Tauri bridge, and Settings Workspace display.
5. **Existing proof surface** (`quick`) — inventory relevant test seams and browser/desktop QA constraints.
6. **Cross-check** (`unspecified-low`, depends on 1–5) — independently validate paths and identify write-scope-safe implementation slices.

The top-level components are independent during investigation. A DAG is required because the cross-check depends on the parallel evidence wave. No investigation node may edit source.

### Implementation DAG — derived after evidence

1. **Native Remote lifecycle** (`unspecified-low`) owns `src-tauri/src/ipc/remote.rs`, `src-tauri/src/remote/state.rs`, `src-tauri/src/remote/tests.rs`, and native command tests. It must preserve the existing security posture: enabled Remote Access is session-only and turns off when Ferryx exits; test and expose that explicit behavior rather than silently creating an auto-start remote listener.
2. **Settings behavior and information architecture** (`unspecified-low`) owns `ui/src/components/SettingsDialog.tsx`, `ui/src/components/SettingsDialog.test.tsx`, and `ui/src/lib/settingsRuntime.test.ts`. It must use the canonical appearance persistence API, provide PIN click-to-copy feedback, explain Default Agent correctly, remove Quick Commands and its persisted key, and replace the empty General page with useful status/navigation content.
3. **Workspace Settings wiring** (`quick`) owns `ui/src/App.tsx` and `ui/src/App.test.tsx`. It must pass the real registered `projects`, active project, active worktree, and existing callbacks to `SettingsDialog`; the failing-first test must prove a Settings invocation receives registered workspace state.
4. **Final verification** (`unspecified-low`) depends on the three producers and runs the exact focused tests, build, diagnostics, and static token scan. It makes no source changes.

5. **Default Agent completion lane** (`unspecified-low`) owns the New Tab launcher prop chain (`NewTabPopover`, `TabBar`, `TerminalSplitView`, `App`) and focused tests. It will turn `defaultAgentId` from a write-only preference into an observable default: the selected enabled/available agent displays as **Default** and is first in the New Tab agent list. This lane is scheduled after the initial Settings producer and before final verification.

The producers have disjoint write scopes; fan-out pays off. The final node depends on all producers and is the only integration gate.

## Success criteria and QA scenarios

1. **Settings persist**: Write a failing test named for settings survival across a remount/restart seam; run `cd ui && npx vitest run <new persistence test>` and capture RED, implement the smallest correction, and capture GREEN. Manual desktop scenario: Settings → Appearance → select `Dark` → fully quit/relaunch Ferryx → Settings → Appearance still shows `Dark`. PASS iff the selected values survive relaunch.
2. **Pairing PIN copies**: Write a failing Settings test that enables Remote Access, clicks `[data-testid="remote-pairing-code"]`, and expects `navigator.clipboard.writeText(pin)` plus visible `Copied` feedback; capture RED then GREEN. Manual desktop scenario: Settings → Remote Access → enable → click the PIN. PASS iff the exact displayed PIN enters the clipboard and feedback appears.
3. **Remote restart lifecycle**: Write a failing native/runtime test for the chosen gateway-init behavior; capture RED then GREEN. Manual desktop scenario: enable Remote Access → quit/relaunch Ferryx → reopen Remote Access. PASS iff actual gateway status matches the explicit Settings copy and persistent preference.
4. **Coherent Settings IA**: Write failing tests for Default Agent explanation, absence of Quick Commands navigation/section/storage, non-empty General content, and workspace fixtures rendering registered projects. Capture RED then GREEN. Manual desktop scenario: Settings navigation has no Quick Commands; General has useful content; Agent default explains what it launches; Workspace lists the registered workspace. PASS iff all four are observable.
5. **Regression / build**: `cd ui && npx vitest run <affected tests>` plus `cd ui && npm run build` exit 0 and LSP errors 0. Manual desktop validation will be explicitly requested from the user instead of desktop automation, honoring their standing no-desktop-control constraint.

## Stop condition

Stop immediately when all seven requested behaviors meet these criteria, RED→GREEN and permitted surface evidence are captured, cleanup is complete, and the audit report is saved.
