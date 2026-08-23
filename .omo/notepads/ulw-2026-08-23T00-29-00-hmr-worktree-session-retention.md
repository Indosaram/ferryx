# HMR Worktree Session Retention Evidence

## Problem

The user observed that a Vite HMR update erases Ferryx tabs/sessions even though a full app restart persistence repair had already passed. This is a different lifecycle: React Fast Refresh can remount `useWorkspaceStore` before the 500 ms native session debounce fires.

## Root cause hypotheses and result

1. **Confirmed** — `useWorkspaceStore` initializes a new `useReducer` state on Fast Refresh remount. The state begins empty (`layout`, `worktreeLayouts`, `sessions`) and `useWorkspaceRuntime` immediately refreshes worktrees/ensures a fallback tab.
2. **Confirmed risk** — the normal native persistence save is debounced 500 ms and cleanup cancels it on HMR unmount, so it cannot be the HMR handoff.
3. **Confirmed risk** — an asynchronous `loadSession()` on the replacement mount could restore stale native data over a freshly handed-off in-memory state.

## RED proof

The new HMR-boundary test constructs an active main-worktree terminal/browser layout and a parked feature-worktree terminal/browser layout, including split leaf mapping, terminal CWD, worktree identity, backend IDs, lifecycle, and unread metadata. With the HMR hydration initializer removed, the replacement store starts empty and fails:

```text
FAIL  src/state/workspaceStore.hmrRetention.test.tsx
AssertionError: expected null to be '/repo/main'
Expected: "/repo/main"
Received: null
```

The initial implementation also exposed a real test/runtime issue: Vitest can expose `import.meta.hot` with undefined `.data`; the HMR bridge must therefore never dereference hot data without a guard.

## Implementation under verification

- Store each workspace's in-memory `WorkspaceState` synchronously at the reducer dispatch seam in a development-only HMR registry.
- On Vite dispose, include the same workspace-state registry in `import.meta.hot.data`; hydrate from it before runtime effects run.
- Capture a boolean at reducer initialization indicating an actual HMR recovery.
- During only that HMR recovery, skip stale native `loadSession()` restore; ordinary cold startup keeps native restoration.
- HMR state is keyed by `workspaceId`; another project cannot consume it.

## Current automated evidence

The first green verification was:

```text
cd ui && bunx vitest run src/state/workspaceStore.hmrRetention.test.tsx \
  src/state/workspaceStore.test.tsx src/state/projectWorkspaceScope.test.tsx \
  src/lib/sessionPersistence.test.ts src/App.test.tsx --config vitest.config.ts

Test Files  5 passed (5)
Tests      66 passed (66)
```

`cd ui && bun run build` also passed. A final focused regression/build run will be recorded after the HMR helper extraction (to keep the store module focused) completes.

## Final completion audit

| Requirement | Concrete artifact and current evidence | Verdict |
| --- | --- | --- |
| Preserve active-worktree HMR state | `ui/src/state/hmrWorkspaceState.ts` captures dev-only workspace state; `useWorkspaceStore` hydrates it synchronously before effects. Mutation proof: forcing `initWorkspaceState` to ignore the handoff made `workspaceStore.hmrRetention.test.tsx` fail with expected `/repo/main`, received `null`. | PASS |
| Preserve parked worktree tabs/layouts | `ui/src/state/workspaceStore.hmrRetention.test.tsx` asserts a parked feature terminal + browser layout survives remount/runtime refresh. | PASS |
| Preserve split panes and terminal metadata | The same test asserts active split-tree leaf mapping, CWD, worktree identities, backend IDs, and lifecycle for all three terminal sessions. | PASS |
| Do not overwrite HMR state from stale disk | `useWorkspaceStore` exposes the immutable init-time `recoveredFromHmr`; `App.tsx` skips `loadSession()` only when true. `App.test.tsx` covers the skip. | PASS |
| Keep ordinary cold boot native restore | Test isolation clears the HMR registry before each test; `App.test.tsx` existing persisted-session restoration tests passed in the final run (33/33). | PASS |
| Keep project/workspace HMR state isolated | HMR registry is keyed by `workspaceId`; HMR regression test mounts `ws-beta` after storing `ws-alpha` and asserts no state is inherited. | PASS |
| Required regression coverage | `cd ui && bunx vitest run src/state/workspaceStore.hmrRetention.test.tsx src/state/workspaceStore.test.tsx src/state/projectWorkspaceScope.test.tsx src/lib/sessionPersistence.test.ts src/App.test.tsx --config vitest.config.ts` — 5 files, 66/66 passed. | PASS |
| Production TypeScript/build gate | `cd ui && bun run build` — `tsc && vite build` passed; 1714 modules transformed. | PASS |
| Static integrity | Scoped `git diff --check` passed. LSP: `hmrWorkspaceState.ts` and `App.tsx` have no diagnostics. `workspaceStore.ts` fresh LSP request timed out; passing `tsc` is the authoritative TypeScript check. | PASS |

## Durable receipt

- Final implementation: `ui/src/state/hmrWorkspaceState.ts`, `ui/src/state/workspaceStore.ts`, `ui/src/App.tsx`.
- Regression coverage: `ui/src/state/workspaceStore.hmrRetention.test.tsx`, `ui/src/App.test.tsx`, `ui/src/test/setup.ts`.
- No dev server, browser/webview, desktop automation, port, or debugger process was started by this session.
- The local `.debug-journal.md` was an ephemeral mutation-proof ledger and is removed during cleanup; this notepad is the durable evidence record.

## Manual QA required

Desktop automation is prohibited. User should run `cd /Users/indo/code/project/orca-lite && bun tauri dev`, create distinct main/feature tabs, edit a frontend file to trigger HMR, then verify both active and parked worktree tabs/CWDs remain exactly intact. This request is issued in the final handoff after final automated evidence is clean.
