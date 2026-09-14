# Review: agent lifecycle library modules (`ui/src/lib`)

Scope reviewed: `agentResume.ts`, `agentAutoResume.ts`, `agentReconnect.ts`, `agentSessionDiscovery.ts`, `activity.ts`, `notificationCoordinator.ts`, plus the minimum call-site confirmation needed (`App.tsx`, `state/workspaceStore.ts`, `agentResumeAffordance.ts`, `state/workspaceRestore.ts`, `src-tauri/src/terminal/shell.rs`, `src-tauri/src/daemon/server.rs`).

**Focus item cleared, not a finding:** no code in scope mints or injects a session id. Every resume builder consumes an id that came back from the agent (`ui/src/lib/agentResume.ts:41` — `omo: { key: "session_id", binary: "omo", args: (id) => ["--session", id] }`), and the Rust side mirrors it (`src-tauri/src/terminal/shell.rs:100` `resolve_agent_resume_plan` validates then forwards `provider_session.id`). `rg -n -- "--session-id" ui/src src-tauri/src` returns only two regression tests and no production use. The id-minting hazard is absent.

---

### [P1] Terminal bell and agent-completion fire two notifications for one agent finish
- Location: `ui/src/lib/notificationCoordinator.ts:97` and `ui/src/lib/notificationCoordinator.ts:165`
- Observed: the bell handler defends itself against a just-fired agent completion — `const lastCompletion = this.lastAgentCompletionTimestamp.get(key) ?? 0;` then `if (now - lastCompletion < this.bellAgentSuppressionMs) { return { accepted: false, suppressed: true }; }`. The reverse direction has no guard: `handleAgentStateChange` (line 165) never reads `lastBellTimestamp`; it goes straight from the completion-edge test to `this.lastAgentCompletionTimestamp.set(key, now)` (line 192) and then dispatches its own sound + `dispatchNotification({ source: 'agent-task-complete', ... })`.
- Why it is wrong: most TUI agents ring the terminal bell *as* they finish a turn. The bell event arrives a few hundred ms before the store's activity transition to `waiting`/`done`. Bell-then-completion is therefore the common ordering, and it is exactly the unguarded one: the user gets two OS banners and two notification sounds for a single agent completion, and the tab/worktree gets marked unread twice. The 1500 ms `bellAgentSuppressionMs` window only ever helps in the rarer completion-then-bell ordering.
- Minimal fix: make the suppression symmetric — in `handleAgentStateChange`, after computing `isCompletionEdge`, drop the notification (return `{ accepted: false, suppressed: true }`) when `now - (this.lastBellTimestamp.get(key) ?? 0) < this.bellAgentSuppressionMs`, keeping the `lastAgentCompletionTimestamp.set` so subsequent bells stay suppressed.

### [P1] `scheduleAgentAutoResume`'s `maxCandidates` option is silently clamped to 8
- Location: `ui/src/lib/agentAutoResume.ts:163` (with `ui/src/lib/agentAutoResume.ts:133`)
- Observed: the scheduler computes `const candidates = collectAutoResumeCandidates(state).slice(0, maxCandidates);`, but `collectAutoResumeCandidates` has already truncated its own result: `return orderedIds.slice(0, MAX_AUTO_RESUME_CANDIDATES);` where `MAX_AUTO_RESUME_CANDIDATES = 8` (line 5). The caller's `maxCandidates` can only ever narrow, never widen.
- Why it is wrong: a workspace restored with more than 8 exited agent panes silently leaves panes 9+ dead — no reconnect attempt, no error, no lifecycle transition, so the affordance stays `idle` and the pane just looks blank until the user clicks reconnect on each one. The `maxCandidates` option advertised in `ScheduleAgentAutoResumeOptions` (line 142) is a broken contract: passing `maxCandidates: 20` yields 8.
- Minimal fix: pass the limit down instead of double-slicing — give `collectAutoResumeCandidates` an optional `limit` parameter defaulting to `MAX_AUTO_RESUME_CANDIDATES` and call it as `collectAutoResumeCandidates(state, undefined, maxCandidates)`, dropping the caller-side `.slice`.

### [P2] `NotificationCoordinator`'s per-session maps grow for the lifetime of the app
- Location: `ui/src/lib/notificationCoordinator.ts:57` (and `:254`)
- Observed: three `Map<string, ...>` keyed by session id — `private lastBellTimestamp = new Map<string, number>();`, `lastAgentState`, `lastAgentCompletionTimestamp` — are written on every bell and every agent state change (`this.lastAgentState.set(key, next)` at line 174). The only eviction is the wholesale `reset(): void` at line 254, and `rg` over `ui/src` excluding tests shows its single caller is `ui/src/devtools/ActivitySurfaceHarness.tsx:219`. The shipped singleton `defaultNotificationCoordinator` (line 261) is never reset.
- Why it is wrong: this is a long-lived desktop app. Every terminal session ever opened, closed, or resumed leaves three permanent entries behind; nothing removes a key when the session is destroyed. Over a multi-day session with heavy pane churn this is a slow, unbounded heap leak in the always-mounted coordinator.
- Minimal fix: add a `forgetSession(sessionId: string)` that deletes the key from all three maps, and call it from the store path that removes a session (the same place that drops `activityBySessionId[sessionId]`).

### [P2] `executedRestoreTokens` accumulates a string per distinct session signature and is never bounded
- Location: `ui/src/lib/agentAutoResume.ts:8` (written at `:24`, consulted at `:158`)
- Observed: `const executedRestoreTokens = new Set<string>();` is module-global; `markRestoreTokenExecuted(token)` (line 161 in the scheduler) inserts a token built by `computeRestoreToken`, which concatenates *every* session's `id:agentType:agentSessionId:providerSession.id:backendSessionId` (line 15). The only removal is `resetAgentAutoResumeGuard`, whose sole production caller is `ui/src/state/workspaceRestore.ts:52` (`resetWorkspaceRestore`), and that only clears tokens prefixed with one workspace id.
- Why it is wrong: the token changes whenever any session's `backendSessionId` or provider session id changes — i.e. after every reconnect, every `/new` in an agent, every pane spawn. Each variation is retained forever, and the token itself is O(number of sessions) characters long. Workspaces the user visits but never explicitly resets keep their entire token history resident.
- Minimal fix: key the guard per workspace instead of accumulating tokens — replace the `Set<string>` with a `Map<workspaceId, string>` holding only the most recent executed token, so each workspace costs one entry.

### [P2] `inFlightReconnects` is keyed by a session id that can be reused, so a stale attempt can be handed to a new session
- Location: `ui/src/lib/agentReconnect.ts:21` (used at `:35`-`:37` and `:104`)
- Observed: `const inFlightReconnects = new Map<string, Promise<SpawnTerminalResult>>();` is module-global and `reconnectAgentSession` short-circuits on it unconditionally: `const existing = inFlightReconnects.get(localSessionId); if (existing) return existing;`. The entry is only removed in the `finally` at line 106, and only if it is still the same promise.
- Why it is wrong: local session ids are persisted and restored verbatim (they are the keys of `state.sessions`), so the same `localSessionId` can be torn down and re-created inside one process run — e.g. switching projects away and back while a reconnect is still awaiting `spawn`. The second call then receives the *first* attempt's promise, which resolves to a backend session bound to the destroyed pane. The caller reports success while the new pane never gets a backend at all: a stuck "reconnecting-looking" pane with no pending work behind it.
- Minimal fix: include a liveness discriminator in the key, e.g. store `{ promise, session: initialSessionObject }` and treat the cached entry as absent when `dependencies.getSessions()[localSessionId]` is not the object the in-flight attempt captured.

### [P3] The bell throttle window is armed by bells that are then thrown away
- Location: `ui/src/lib/notificationCoordinator.ts:95`
- Observed: `this.lastBellTimestamp.set(key, now);` executes before the agent-completion suppression check on line 97, so a bell that immediately returns `{ accepted: false, suppressed: true }` still consumes the 1000 ms throttle slot.
- Why it is wrong: a bell that produced no user-visible effect blocks the next bell for a further second, so a genuine, notifiable bell right after a suppressed one is silently dropped. Low impact because the two windows mostly overlap, but the state write is in the wrong place.
- Minimal fix: move `this.lastBellTimestamp.set(key, now)` below the `lastAgentCompletionTimestamp` suppression check so only bells that survive the filters arm the throttle.

### [P3] Dead unsupported-discovery machinery in `agentSessionDiscovery`
- Location: `ui/src/lib/agentSessionDiscovery.ts:26`
- Observed: `export const UNSUPPORTED_DISCOVERY_AGENTS: ReadonlySet<string> = new Set([]);` and the companion `const UNSUPPORTED_DISCOVERY_REASONS: Readonly<Record<string, string>> = {};` (line 29) are both empty, yet `discoverAgentSessionId` still branches on them at line 171 and builds a `console.warn` message from a reason that can never be found.
- Why it is wrong: pure dead code — the branch is unreachable, and the exported empty set invites callers to depend on a guarantee nothing enforces. No user impact.
- Minimal fix: delete the two empty constants and the unreachable block at lines 171-178, or populate the set if agents genuinely lack discovery support.

### [P3] `ResumeCapability` has three unreachable members
- Location: `ui/src/lib/agentResume.ts:147`
- Observed: `export function agentResumeCapability(agentType: string): ResumeCapability { return isResumableTuiAgent(agentType) ? "uuid" : "none"; }` — the declared union (line 2) also contains `"positional-index"` and `"chat-id"`, which no code path can produce.
- Why it is wrong: consumers must handle variants that cannot occur, and exhaustive switches carry dead arms. No runtime impact.
- Minimal fix: narrow `ResumeCapability` to `"uuid" | "none"` until a builder actually needs the other shapes.

---

**Not findings (checked and clean):** `activity.ts` — `summarizeActivities`/`combineActivitySummaries`/`resolveActivityIndicator` are consistent with the precedence documented at line 130, and `seen` is correctly excluded from working counts only. `agentResume.ts` argv construction is safe: `normalizeSessionId` rejects empty, over-length, leading-`-`, `latest`, and control-character ids (lines 76-90), and `getAgentResumeArgv` enforces the provider key match before building argv. The unawaited promises in scope are deliberately terminated, not swallowed: `agentReconnect.ts:108` attaches `.catch(() => undefined)` to a bookkeeping `finally` chain while the real `attempt` is returned to the caller, and `agentAutoResume.ts:175` (`void reconnect(sessionId).catch(() => undefined)`) is fire-and-forget by design since the lifecycle failure is already dispatched to the store.

**Note on first-observed attention (deliberate, flagged for awareness):** `notificationCoordinator.ts:178` requires `effectivePrev !== undefined` for a completion edge, so a session whose very first observed activity state is `waiting` never notifies. This is pinned by an existing test ("ignores the first observed state when no previous state is known"), so it is treated as intended behavior rather than a finding.

**Summary: P0=0, P1=2, P2=3, P3=3**
