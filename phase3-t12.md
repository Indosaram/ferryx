# Phase 3 — Todo 12: Remote App Plan Limit Notice and Account Suspension Flow

- Worktree: `/Volumes/T9-Mac/project/ferryx-relay-monetization`
- Branch / base: `feat/relay-monetization` @ `016936d4`
- Status: **source + tests landed; nothing executed locally — no vitest, no `tsc`, no vite, no local dev server, no git commit. Every command below is UNRUN.**
- Plan: `.omo/plans/relay-monetization.md` todo 12. Closes GAP-9.

## Changed paths (exact revision to verify)

| Path | Change | Lines | SHA-256 |
| --- | --- | --- | --- |
| `ui/src/remote/PlanLimitNotice.tsx` | NEW — PlanLimitNotice component, 360px-safe responsive layout, accessible markup | 157 | `6557667076121659c83e00ef4dfa1a47e5ae84de4b04d416fc99252f0421c29e` |
| `ui/src/remote/PlanLimitNotice.test.tsx` | NEW — 5 component unit tests (360px layout, structured usage rendering, ISO datetime attributes, upgrade link, retry action) | 97 | `679e35433926703fbdcd19535f7ad6f7a66e7522c53c589c9fd69654882f3116` |
| `ui/src/remote/PlanLimitNotice.flow.test.tsx` | NEW — 8 end-to-end flow tests covering 402 notice rendering, exact close suspension, tunnel closure, late probe dropping, retry recovery | 674 | `027ac92cd3ca9349e721313fe880e849ea49c2290e616fa3b511ec0aa23bf397` |
| `ui/src/remote/accountSession.ts` | MODIFIED — Structured error types (`PLAN_LIMIT_REACHED`, `REMOTE_SUSPENDED`), parsers (`planLimitStateFromError`, `planLimitStateFromCloseEvent`), snapshot cache helpers | 1079 | `dd9aac7fa261aeb70d92c19ae42b34bf0e7ae722e1e4549509035290c23721b0` |
| `ui/src/remote/accountSession.test.ts` | MODIFIED — 34 unit tests verifying structured error translation, snapshot persistence, session change events | 1203 | `ae547e3b9b44ba6568e59a5e15336357bd1adba59d1983b0cf7cb3655e6eeea9` |
| `ui/src/remote/RemoteApp.tsx` | MODIFIED — Wire `PlanLimitNotice` when remote account is suspended/limited, stop retries, retain direct worktree selection | 2021 | `82b0cbea72a38d3afe4e86eea7bc710a9a1b8b50ce5599194a4b566f1b8b4b83` |
| `ui/src/remote/attachTunnel.ts` | MODIFIED — Close event handling: fail-fast on exact 1008/1012/REMOTE_SUSPENDED without retry loops | 1071 | `8200729757e2e794edcbbbf1628c0896c2c0c0abe8bbd62757f1f819026f45fd` |
| `ui/src/remote/useAccountWorktrees.ts` | MODIFIED — Machine discovery pipeline parses structured errors into `onPlanLimit` callback; closes tunnels on suspension | 446 | `01a2904fd8f4bce6ab63d5dd22602a1ef8e64b8afebec83029944669f9686eba` |
| `ui/src/remote/useAccountWorktrees.test.ts` | REWRITTEN — Deterministic discovery test without `waitFor`, no `as any`, accurate DTOs matching `/api/v1/workspace/state` production hook contract, pre-armed deferred promises and exact act flushes | 303 | `ccfac9e7a79dd7eaf4751e735856a6c247884b6c1fd5a38be066705237f59c05` |

## Implemented Components and Features

### 1. `PlanLimitNotice.tsx`
- **Location**: `ui/src/remote/PlanLimitNotice.tsx`
- **Design & Layout**: Responsive layout engineered for 360px viewport widths and larger.
  - Semantic container with `role="alert"`, `aria-labelledby="remote-plan-limit-title"`.
  - Data attributes: `data-testid="remote-plan-limit-notice"`, `data-code`, `data-plan`.
  - Structured error details rendered without invented values:
    - `data-testid="remote-plan-limit-usage"` for computer limits and usage counts (`limit`, `used`).
    - `data-testid="remote-plan-limit-grace-ends"` with valid ISO datetime timestamps.
    - `data-testid="remote-plan-limit-stopped"` with valid ISO datetime timestamps.
    - `data-testid="remote-plan-limit-upgrade"` with external pricing link (`https://ferryx.dev/docs/pricing/`).
    - `data-testid="remote-plan-limit-retry"` for explicit connection retry.
- **Design Token Discipline**: Uses semantic tokens (`text-status-warning`, `text-chat-foreground`, `text-chat-foreground-secondary`, `bg-chat-screen`, `border-chat-border`).

### 2. `accountSession.ts` & `attachTunnel.ts` Handling
- **Structured Error Constants & Types**:
  - `PLAN_LIMIT_REACHED = "PLAN_LIMIT_REACHED"` (HTTP 402 / WebSocket close 1008/1012).
  - `REMOTE_SUSPENDED = "REMOTE_SUSPENDED"` (HTTP 402 / WebSocket close 1012 / `REMOTE_SUSPENDED_CLOSE_REASON`).
  - `planLimitStateFromError(error)` and `planLimitStateFromCloseEvent(event)` parsers extracting structured `{ code, plan, limit, used, graceEndsAt, stoppedAt }`.
- **Termination & Non-Retry Discipline**:
  - On `REMOTE_SUSPENDED` close or attach refusal, automatic retries and reconnect timers are halted.
  - Active tunnels and sockets are cleaned up deterministically without resurrecting state or retrying.
  - In `attachTunnel.ts`, WebSocket closure with `1008` / `1012` / `REMOTE_SUSPENDED` fails fast with `ATTACH_TUNNEL_REFUSED` instead of hanging or retrying.

### 3. `RemoteApp.tsx` Integration
- Renders `PlanLimitNotice` full-screen when remote account connection is limited or suspended.
- **Direct Worktree Selection Intact**: Preserves direct worktree selection (`useAccountWorktrees`) without local/LAN gating. Never displays an Account Machines screen.
- **Explicit Recovery**: Clicking "Retry connection" re-runs discovery and resets the notice state cleanly.

### 4. Deterministic Pre-Armed Test Suites
- **`useAccountWorktrees.test.ts`**:
  - Completely rewritten without `waitFor` polling and without `as any` type suppressions.
  - Accurately matches production hook contract querying `/api/v1/workspace/state` only.
  - Uses typed `TunnelTransport` and `createDeferred` signals to step through lifecycle transitions in explicit `act(async () => ...)` boundaries.
  - Tests machine discovery, `PLAN_LIMIT_REACHED`, and `REMOTE_SUSPENDED` callback triggers.
- **`PlanLimitNotice.test.tsx`**:
  - 5 tests verifying 360px layout, structured usage rendering, ISO datetime attributes, upgrade link, and explicit retry action.
- **`accountSession.test.ts`**:
  - 34 tests verifying structured error translation, snapshot persistence, session change events.
- **`PlanLimitNotice.flow.test.tsx`**:
  - 8 flow tests verifying notice rendering, tunnel closure on suspension, dropping late probes, halting automatic retries on exact suspension reason, ignoring non-suspension close events, and recovering on retry.

## Unrun Notice

Per instructions ("no local tests/builds/servers/desktop/commits/children", "Source only until root remote execution"), all tests, builds, and typechecks remain **UNRUN** locally on macOS. Root will run remote verification on `maho-win`.
