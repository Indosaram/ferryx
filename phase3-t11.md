# Phase 3 Task 11: PlanSection Test Corrections

## Scope & Objective
Eliminate the test failures and vitest transform error observed in `src/components/settings/PlanSection.test.tsx` within the worktree `/Volumes/T9-Mac/project/ferryx-relay-monetization`. The changes are strictly targeted, deterministic, and avoid microtask-count luck (`flushFetch(turns = 10)`), while preserving all existing test assertions, stale action coverage, machine-consumed DOM attributes, and exact trigger ordering. Local builds/tests were strictly forbidden and not executed.

## Changed Files
- `/Volumes/T9-Mac/project/ferryx-relay-monetization/ui/src/components/settings/PlanSection.test.tsx`

## Root Cause Analysis & Refined Implementation
1. **Transform Failure Resolution (Duplicate Identifier)**:
   - Vitest failed with esbuild error: `The symbol "jsonResponse" has already been declared`.
   - Removed the duplicate `jsonResponse` definition, retaining the single top-level helper.

2. **Deterministic Promise Tracking Without Unhandled Rejection**:
   - Tracking `activeFetchOperations: Set<Promise<unknown>>` on both outer `fetch` and inner `res.json()`.
   - Used `void promise.then(() => activeFetchOperations.delete(promise), () => activeFetchOperations.delete(promise))` instead of `promise.finally()`, avoiding creating an unhandled rejected child promise on fetch errors while preserving the caller's rejection without swallowing.
   - Applied identical clean removal logic on `jsonPromise`.

3. **Deadlock-Free, Bounded Loop in `flushFetch`**:
   - In tests with deferred requests (e.g. `ignores a deferred checkout response after origin replacement`, `ignores a deferred quantity response`, and deferred invite/remove), promises are intentionally kept unresolved until manual resolution.
   - Guarded against deadlocks with bounded execution (`maxPasses = 50`) that drains queued `pendingResolvers`, awaits already-settled operations fast via `Promise.race([Promise.all(snapshot.map(p => p.catch(() => undefined))), new Promise(r => setTimeout(r, 0))])`, yields microtasks (`Promise.resolve()`), and terminates cleanly when no further progress can be made.

4. **State Matrix & Team Test Settlements**:
   - Added missing initial settlement in `keeps the plan actions enabled and renders no local control in the %s state` (4 tests) via `observeDom` on `plan-summary`, `await flushFetch()`, and `await settle()`.
   - Added initial settlement in `keeps plan and member controls hidden for a plain member`.
   - Added initial settlement in `lets an admin see members, invite, and change seats but never remove`.
   - Added initial settlement and post-confirm `flushFetch()` in `surfaces a structured remove refusal and keeps the member row`.

5. **API Match for Remove Member (HTTP 200)**:
   - Updated remove member responses in `protects the owner row from removal...` and `ignores deferred invite and remove responses...` from `204` to `jsonResponse({ ok: true }, 200)`.

6. **Async Action & External URL Trigger Flush**:
   - In `opens the returned checkout url...` and `opens the manage url...`, added `await flushFetch()` after clicking to allow external URL bridge dispatch to complete deterministically.

7. **404 Unavailability Handling**:
   - Confirmed `hides itself and reports unavailability when the account server answers 404` awaits `flushFetch()`, `unavailability`, and `removalSignal` before verifying `section === null` and `plan-summary === null`.

## Verification Status
- Verified all disk changes in `/Volumes/T9-Mac/project/ferryx-relay-monetization/ui/src/components/settings/PlanSection.test.tsx` via direct reads.
- Exactly one `jsonResponse` declaration exists.
- All 32 tests and their assertions are intact.
- No local build, test, or server was run.
- Changes are frozen on disk and ready for root remote verification.
