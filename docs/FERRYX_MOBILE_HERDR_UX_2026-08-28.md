# Ferryx mobile Herdr-grade remote UX — implementation and verification

Date: 2026-08-28
Plan: `.omo/plans/ferryx-mobile-herdr-ux-mass-ulw.md`
Evidence: `docs/evidence/mobile-herdr-ux/`

A phone browser attached to the Ferryx remote gateway now shows per-tab and per-worktree agent
activity state with real agent brand identity, traverses tabs by swipe as well as buttons,
resizes the terminal font by pinch, and offers an attention affordance that jumps straight to
the tab whose agent is waiting.

## Root cause the run had to fix first

Live daemon evidence (socket `/private/tmp/rorca-501/daemon.sock`, epoch 1787745906018):
`listSessions` returned 9 running PTYs while `remoteGetActiveSelection` returned
`{"selection": null}`. `get_active_running_sessions` (`src-tauri/src/remote/server.rs:367`)
returns an empty vec when `active_selection` or its `session_id` is absent, so the mobile client
could never render a terminal regardless of UI work. `publishFocusedTerminal`
(`ui/src/lib/tauri.ts`) ended with `.catch(() => undefined)`, swallowing every publish failure —
the defect was invisible by construction. The remote UI was never hiding anything.

The agent-state subsystem itself was already shipped by concurrent work
(`src-tauri/src/agent_detect/`, 1165 LOC; `ui/src/lib/activity.ts`; `ui/src/lib/agentIcon.ts`)
but had **zero** references anywhere under `src-tauri/src/remote/` or `ui/src/remote/`. The
Desktop knew which agent was waiting; the phone did not. Closing that gap is this run's substance.

## Wire contract

camelCase on the wire, every field additive and optional so existing clients are unaffected
(`#[serde(default, skip_serializing_if = "Option::is_none")]`).

| Type | Fields |
| --- | --- |
| `RemoteTerminalTabInfo` | `id`, `label`, `activityState?`, `agentType?` |
| `RemoteWorktreeInfo` | `worktreeSlug`, `worktreeLabel`, `attention?` |

`activityState` and `attention` are exactly one of `"working" | "waiting" | "done"`. The rollup
rank mirrors `ui/src/lib/activity.ts` `stateRank`: waiting(3) > working(2) > done(1). The Rust
enum `AgentActivity` (`src-tauri/src/agent_detect/engine.rs:10`) keeps its own
`Working|Blocked|Idle` naming; the wire uses the shipped TypeScript vocabulary because the remote
client is the consumer, so `blocked` maps to `waiting` at the boundary
(`sanitize_activity_state`, `src-tauri/src/ipc/remote.rs`).

## Criteria and evidence

| # | Criterion | Evidence |
| --- | --- | --- |
| 1 | Publication carries a real `backendSessionId` | `ui/src/App.test.tsx` "publishes real backendSessionId after session backend is bound or rebound"; mutation forcing `backendSessionId: null` fails the suite |
| 2 | Publish failures reach `reportRuntimeError` | `.catch(() => undefined)` removed from `publishFocusedTerminal`; publish effect is `.catch(reportRuntimeError)` (`ui/src/App.tsx:688`); `ui/src/lib/tauri.test.ts` "propagates publishFocusedTerminal rejection instead of swallowing errors" |
| 3 | Gateway serves `activityState` + `attention` | `src-tauri/src/remote/tests.rs` `test_workspace_state_agent_activity_and_worktree_attention_rollup`; mutation setting the waiting rank to 0 → FAILED (48 passed, 1 failed), reverted → 49 passed |
| 4 | Desktop publishes per-tab state + agentType | `deriveFocusedTerminal` reads `state.activityBySessionId` (`ui/src/App.tsx:189-201`), spreading fields only when present; absence asserted at `App.test.tsx:2446-2448`; mutation ignoring the map → 2 tests FAILED |
| 5 | Mobile renders badges + attention rollup | `ui/src/remote/RemoteUI.test.tsx` (23 tests); mutation dropping `"waiting"` from the normalizer whitelist → 3 tests FAILED |
| 6 | Swipe tab traversal + pinch font sizing | `ui/src/remote/RemoteTerminalGestures.test.tsx` (5 tests); mutation disabling the horizontal axis → FAILED; real surface: font 13px → 36px (MAX clamp) → 10px (MIN clamp) → 20px |
| 7 | Attention surface deep-selects the waiting tab | `ui/src/remote/RemoteAttention.test.tsx` (9 tests); mutations suppressing the affordance → 4 FAILED and removing swipe wiring → 2 FAILED |
| 8 | Adjacent-surface regression | 4 `StatusCode::FORBIDDEN` sites and `focus_watcher` intact in `server.rs`; `test_remote_active_selection_safe_tab_descriptors_and_no_path_leakage`, `test_active_desktop_terminal_contract_and_safe_selection_bridge`, `test_connected_terminal_websocket_closed_when_active_selection_changes` all still pass unweakened; `sanitize_agent_type` drops path-like and over-64-char values |
| 9 | Full validation | `bun --cwd ui test` → 111 files / 973 tests passed; `bunx tsc --noEmit` → exit 0; `bun run --cwd ui build` → exit 0; `cargo test --lib` → 326 passed, 0 failed |
| 10 | This report | `docs/FERRYX_MOBILE_HERDR_UX_2026-08-28.md` + `docs/evidence/mobile-herdr-ux/` |

## Real-surface proof

`agent-browser` at iPhone viewport 390x844 against the production `ui/dist` build, served by a
stub gateway on `127.0.0.1:4199` emitting wire payloads in the exact shipped contract shape.

Accessibility tree from the rendered page:

```
- banner:
  - heading "Ferryx Remote"
  - button "codex (2 waiting)"
  - button "Disconnect"
- button "Change workspace context": orca-lite / main
- button "Previous terminal tab" [disabled]
- text: 1 / 3
- button "Next terminal tab"
- tablist "Terminal tabs":
  - tab "claude (working)" [selected]
  - tab "codex (waiting)"
  - tab "shell"
```

The unknown-agent tab renders `tab-terminal-icon` while `claude` and `codex` render
`tab-agent-icon` from the bundled brand logos, satisfying the terminal-fallback requirement.

Both interaction paths were captured issuing exactly one selection request each, recorded by the
gateway (`docs/evidence/mobile-herdr-ux/selection-requests.log`):

```
SELECT_POST {"workspaceId":"orca-lite","tabId":"tab-2"}
```

tab-2 is the waiting `codex` tab, reached both by tapping the attention badge and by swiping left
(312px → 182px, past the 40px threshold). A swipe at either end of the tab list issues no request.

## Why there is no Web Push

The remote gateway serves plain HTTP over a LAN address, which is not a secure context, so
service workers and the `Notification` permission API are unavailable. Lock-screen push in the
style of Herdr Mobile therefore cannot exist without TLS on the gateway. The attention surface
rides the existing `/api/v1/events` WebSocket as an in-app affordance instead. Real push remains
a TLS-and-certificate story for a separate change.

## Live end-to-end verification (headless daemon, no GUI)

The stub gateway proves the client but never the wire. The same checks were re-run against the
REAL daemon and the REAL Axum gateway. No Desktop GUI and no desktop automation were involved:
Ferryx's daemon is headless by design and owns both the PTYs and the gateway.

```
./src-tauri/target/debug/ferryx --daemon        # prints FERRYX_DAEMON_READY
```

Every step used the documented UDS protocol at `/private/tmp/rorca-501/daemon.sock`
(`RegisterWorkspace`, `Spawn`, `RemoteCreatePairingCode`, `RemoteSetActiveSelection`,
`RemoteGetActiveSelection`) plus real HTTP against `127.0.0.1:43821`.

| Step | Result |
| --- | --- |
| `RemoteGetStatus` | `mode localNetwork, port 43821, allowControl true, bound 0.0.0.0:43821` |
| `Spawn` a real PTY | session `2e3b10f1-0e46-430b-bc88-3962b0fe417d`, `running: true` |
| `RemoteCreatePairingCode` | code minted with no GUI |
| `POST /api/v1/pair/exchange` | `HTTP/1.1 200 OK`, real bearer token, `permission: "control"` |
| `GET /api/v1/workspace/state` | per-tab `activityState` + `agentType` present; worktree `attention: "waiting"` correctly outranking the `working` tab |
| Remote client at 390x844 | status `Live`, real shell prompt streaming from the PTY, `codex (waiting)` badge, `claude (working)` / `codex (waiting)` / `shell` tabs |
| Typed `echo FERRYX_LIVE_MOBILE_OK` from the mobile viewport | output returned in the grid |
| Attention tap and swipe | `POST /api/v1/workspace/select` → `200`; the client correctly holds at `1 / 3` with tabs disabled until the Desktop confirms |
| Desktop-role re-publish of `tab-2` | client advances to `2 / 3`, `codex (waiting)` becomes `[selected]`, attention badge disappears because the waiting tab is now active |

Evidence: `docs/evidence/mobile-herdr-ux/live/`.

### Bug the stub had hidden

The first live run published `activityState: "working"` / `agentType: "claude"` and the readback
returned only `{id, label}` — both new fields silently dropped. Root cause was a stale binary
(built 23:07, `src-tauri/src/remote/protocol.rs` changed 00:22), not a code defect. After
`cargo build --bin ferryx` the round-trip preserved both fields. A stub gateway validates the
client contract only; always re-run against a freshly built daemon before claiming the wire works.

## Known limits

- The selection confirmation loop requires a Desktop client to activate the tab and re-publish.
  With no GUI running, the gateway accepts the request (`200`) and the mobile client correctly
  waits — verified above by having the daemon play the Desktop's re-publish role.
- The pinch font size lives in component state and is not yet persisted across reconnects.
- A check on physical phone hardware over the LAN was not performed; the client was exercised at
  an iPhone viewport in Chrome against the live gateway.
