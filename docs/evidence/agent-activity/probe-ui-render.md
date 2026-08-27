# Probe UI Render Chain Evidence

## VERDICT
VERDICT: RENDER_CHAIN_WORKS

## EVIDENCE
```bash
cd /Users/indo/code/project/orca-lite/ui && npx vitest run src/state/activityRenderChain.test.tsx
```

```text
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/state/activityRenderChain.test.tsx (2 tests) 46ms

 Test Files  1 passed (1)
      Tests  2 passed (2)
   Start at  01:07:09
   Duration  929ms (transform 120ms, setup 74ms, collect 341ms, tests 46ms, environment 155ms, prepare 24ms)
```

## DETAIL

The integration test in `ui/src/state/activityRenderChain.test.tsx` mounts a React component tree that connects the real `useWorkspaceStore` hook directly to the real `TabBar` component without mocking store state, selectors, tab item components, or `StatusDot`.

### Full Chain Verification Trace
1. **Native Title Event Ingress (`ui/src/state/workspaceStore.ts:248-253`)**:
   `onNativeTerminalTitle` listener in `useWorkspaceStore` receives `{ sessionId: "backend-2", title: "⠋ omo: generating code changes" }`.
2. **Session Resolution (`ui/src/state/workspaceStore.ts:236-244`)**:
   `resolveSession` matches `backendSessionId === "backend-2"` to the owning terminal session and non-active tab id (`tab2Id`).
3. **Store State Recording (`ui/src/state/workspaceStore.ts:1473-1488, 1515-1533`)**:
   `SESSION_TITLE_ACTIVITY` classifies the title into `state: "working"`, `isAgent: true`, `agentType: "omo"` and records it under `activityBySessionId`.
4. **Selector Derivation (`ui/src/state/workspaceStore.ts:887-897`)**:
   `selectTabActivitySummaries` aggregates terminal activities for `tab2Id`, producing `hasWorking: true`, `workingCount: 1`, and `agentType: "omo"`.
5. **Component Prop Ingestion (`ui/src/components/TabBar.tsx:162-176`)**:
   `TabBar` passes `activity={activityByTabId?.[tab.id]}` (`store.tabActivity[tab2Id]`) to `SortableTab`.
6. **Indicator Resolution & DOM Rendering (`ui/src/components/tab-dnd/SortableTab.tsx:44-48, 87-106`, `ui/src/components/ui/StatusDot.tsx:14-24`)**:
   - For working title: `SortableTab` resolves `resolveActivityIndicator(activity)` as `"working"` and resolves `resolveAgentIcon("omo")`. It renders `StatusDot` with `state="working"`.
   - `StatusDot` renders `<LoaderCircle data-status-state="working" className="... animate-spin ...">`.
   - Assertion verifies: `tab2Element.querySelector('[data-status-state="working"]')` exists in document, has class `animate-spin`, and is contained within `[data-testid="tab-working-indicator"]`.
7. **Completion & Attention Transition (`ui/src/state/workspaceStore.ts:1525-1533`, `ui/src/components/tab-dnd/SortableTab.tsx:45`, `ui/src/components/ui/StatusDot.tsx:44-51`)**:
   - Emitting `"omo: done"` on the background tab causes `applySessionActivity` to set `unreadTabIds[tab2Id] = true` since `!isTabVisible(state, tab2Id)`.
   - `SortableTab` resolves `activityIndicator = "unread"`.
   - Assertion verifies: working spinner is removed, and element with `data-status-state="unread"` inside `[data-testid="tab-unread-dot"]` is rendered in Tab 2.

### Regression Sensitivity
The test is sensitive to regressions across every link in the chain:
- If `ui/src/state/workspaceStore.ts:251` fails to dispatch `SESSION_TITLE_ACTIVITY`, `store.tabActivity` stays empty and no indicator renders.
- If `ui/src/state/workspaceStore.ts:893` selector logic drops or miscalculates `summarizeActivities`, `activityByTabId` is undefined and Tab 2 renders default `<TerminalSquare>`.
- If `ui/src/components/TabBar.tsx:172` fails to forward `activity={activityByTabId?.[tab.id]}`, `SortableTab` never receives activity.
- If `ui/src/components/tab-dnd/SortableTab.tsx:44` does not call `resolveActivityIndicator`, `activityIndicator` is null.
- If `ui/src/components/ui/StatusDot.tsx:20` drops `animate-spin` or `data-status-state="working"`, the DOM assertions fail.
