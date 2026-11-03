# Supervisor panel contract (frozen)

Plan: `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md`. Types and signatures only; no behaviour.
Code: hub `crates/ferryx-hub/src/supervisor/panel_contract.rs` (serde camelCase), UI `ui/src/lib/supervisorPanel/contract.ts` (+ `fixtures.ts`). Change both files together; do not fork a type.

## Types

| Type | Shape |
|---|---|
| `SessionRef` | string `<hostId>:<backendSessionId>`; Rust newtype with `new`, `parse`, `as_str`, `Display`. A paired computer's host id is its URL (`https://relay/host/<machine>`), so the ref splits at the first `:` after `/host/`. In a request path the whole ref is ONE percent-encoded segment (`sessions/https%3A%2F%2Frelay%2Fhost%2Fm%3As1/stop`); the relay validates and forwards the raw path, and `%2F` is never a separator. |
| `HealthKind` | `{type:"healthy"}` / `upstreamError{status?}` / `quota{resetAtMs?}` / `stalled{sinceMs}` / `failed{reason}` |
| `SessionHealth` | `{sessionRef, kind, observedAtMs}` |
| `PendingPrompt` | `{promptId, kind: question\|permission\|approval, question, command?, options[], screenRevision?}` |
| `FleetSession` | `{sessionRef, hostId, hostLabel, title, cwd, agent, state: working\|blocked\|idle\|unknown, stateSource, stateSinceMs?, screenChangedAtMs?, pendingPrompt?, lastAssistantLine?, updatedAtMs, stale}` |
| `ActionKind` | answered, retried, resumed, cancelled, stopped, ruleChanged, finished, escalated |
| `EvidenceRef` / `UndoHandle` | `{kind, id}` / `{kind, payload}` |
| `ActionEntry` | `{id, incidentKey, sessionRef?, kind, summary, basis, evidence[], undo?, live, createdAtMs, updatedAtMs}`; one row per `incidentKey`, updated in place |
| `Escalation` | `{sessionRef, prompt, recommendedIndex, reason, createdAtMs}`; recommendation mandatory |
| `AnswerDecision` | `{type:"answer", optionIndex, basis}` / `{type:"escalate", recommendedIndex, reason}` |
| `RuleKind` | upstreamRetry, stallRestart, quotaResume, askBeforeIrreversible |
| `RuleScope` | `{repo?, hostId?, agent?}` (null = any) |
| `StandingRule` | `{id, scope, kind, params, createdFromMessageId?, createdAtMs}` |
| `PanelSnapshot` | `{decisions: Escalation[], handling: ActionEntry[] (live), sessions: FleetSession[], log: ActionEntry[], rules, paused, setupRequired}` |

Default rules (`default_rules()`, seconds): `upstreamRetry {backoffSecs:[60,120,300,600], thenEverySecs:600}`; `stallRestart {thresholdSecs:300, maxRestarts:1}`; `quotaResume {fallbackSecs:600}`; `askBeforeIrreversible {enabled:true}`.

`resume_text(agent)`: `claude` / `claude-code` / `codex` -> `"continue"`; others `None` (escalate).

Hub traits (all in `panel_contract.rs`, `async_trait`): `ActionLog` (`upsert_incident`, `list_today`, `mark_undone`), `RuleStore` (`list`, `upsert`), `HealthClassifier` (`classify(&FleetSession, tail, rules, now_ms) -> HealthKind`, sync), `SessionControl` (`stop`, `send`, `answer`, `read_tail`). Errors: `crate::error::HubError`.

## Daemon fields (no code in this task)

Agent-control list/read entries and the slim event gain two nullable ms-epoch fields:
- `stateSince`: when the current state was entered.
- `screenChangedAt`: when `screen_revision` last changed.

The hub maps them to `FleetSession.stateSinceMs` and `screenChangedAtMs`. No new `AgentControlState` variant.

## HTTP routes (hub host, owner-scoped)

| Route | Body -> response |
|---|---|
| `GET /api/v1/fleet/sessions` | `{sessions: FleetSession[]}` |
| `GET /api/v1/supervisor/panel` | `PanelSnapshot` |
| `POST /api/v1/supervisor/converse` | `{text, mentions[], requestId}` |
| `POST /api/v1/sessions/{sessionRef}/stop` | direct to agent control, not via the engine |
| `POST /api/v1/supervisor/pause` | `{paused}` |
| `POST /api/v1/supervisor/decisions/{sessionRef}/answer` | `{promptId, screenRevision, optionIndex}`; stale revision is rejected |
| `POST /api/v1/supervisor/actions/{id}/undo` | none |

## Events

Owner-scoped frames on `/api/v1/events`, `type` one of:
- `panel.decision`: an `Escalation` appeared or was resolved.
- `panel.action`: an `ActionEntry` was created or updated.
- `panel.session`: a `FleetSession` changed.
- `panel.rules`: the rules list changed.

Non-owner subscribers never receive prompt bodies.

## Tauri commands

`cmd_supervisor_panel`, `cmd_supervisor_converse`, `cmd_session_stop`, `cmd_supervisor_pause`, `cmd_supervisor_answer`, `cmd_supervisor_action_undo`, `cmd_supervisor_dispatch`.

Note: `PanelSnapshot.sessions` lists every session; the UI reducer places each session in exactly one section (decisions, handling, sessions) per plan D5.

## T9 direct controls (implemented)

All of these skip the hub engine. Auth is the existing agent-control gate: machine scope, Control permission, agent control on, device admitted; a mirror, view-only or excluded device gets 403 before any body is read.

| Route | Body -> response |
|---|---|
| `POST /api/v1/sessions/{sessionRef}/stop` | optional `{requestId}`; the provider interrupt key via agent control. A ref for a paired computer is forwarded once through the daemon's agent-control path to that computer (same device authorization, per-device audit; a lost answer is 409 `OPERATION_OUTCOME_UNKNOWN` and is not resent; an unknown paired host is 404); a malformed ref 400. |
| `POST /api/v1/supervisor/decisions/{sessionRef}/answer` | `{promptId, screenRevision, optionIndex[, requestId]}`; an owner tap, no answer gate; stale revision is 409 `REQUEST_CONFLICT` and nothing is typed. A ref for a paired computer is forwarded exactly like the stop route, and the tap joins this computer's answer history under that computer's session refs only. |
| `POST /api/v1/supervisor/pause` | `{paused}` pauses every supervisor action (hub flag `supervisor_paused`); `{laneKey, paused}` keeps pausing one lane. Same path, one handler. |
| `POST /api/v1/supervisor/actions/{id}/undo` | runs the entry's undo handle; `restoreRule` is supported, other kinds 422 `UNSUPPORTED`, a spent or missing handle 404. |

Tauri: `cmd_session_stop`, `cmd_supervisor_answer`, `cmd_supervisor_pause`, `cmd_supervisor_action_undo`, `cmd_supervisor_settings`, `cmd_supervisor_dispatch` (explicit `projectId`; no guessing), `cmd_agent_control_enable_owner` (agent control on, only the owner's own paired machine Control devices admitted).

Daemon wire (capability `supervisorControlV1`): `supervisorGlobalPause`, `supervisorSettings`, `supervisorActionUndo`, `agentControlEnableOwner`. Each that changes state is refused from a process inside a Ferryx pane.

Supervisor settings live in hub_meta key `supervisor_settings`: `{enabled, removeAll, ownerHome}`. `enabled` is effective only while the daemon resolves the same owner home (`OMO_HOME`, else `<home>/.omo`) that turned it on. `removeAll` is off by default.
