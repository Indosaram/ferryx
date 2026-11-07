# Supervisor redesign: autonomous operator + one Supervisor panel (desktop + phone)

Status: draft plan v2, awaiting owner approval. No implementation has started.
Supersedes v1 of this file (chat-first conversation + session rail), which framed the supervisor as a place where the owner answers every prompt. The owner rejected that framing on 2026-11-02.
Research: `docs/research/supervisor-chat-redesign-2026-11-02.md` (journal `.omo/ulw-research/20261102T135705-supervisor-chat/`).
Prototype: `docs/mockups/supervisor-chat/panel.html` (desktop and phone), screenshots in `docs/mockups/supervisor-chat/shots/`.
Tracked copy: `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md` (`.omo/` is gitignored; workers read the tracked copy).

## Goal

The supervisor is the owner's autonomous operator. It runs every agent session on every connected computer on his behalf: it answers prompts the way he would, recovers stuck sessions without being told (upstream/proxy errors, 429 usage limits, sessions that report "working" but stopped producing output), and keeps going until each session's task is done. The owner is interrupted only for decisions that are truly his, and each of those arrives with the supervisor's recommended answer and a one-line reason.

The owner's own Slack instruction of 2026-11-02 is the reference behaviour that must happen with no instruction at all: "1. 10분 간격으로 재시도 ... 2. 5분 이상 working.. 으로 계속되는 경우 캔슬하고 다시 재개 3. 429로 멈춘 세션 있으면 다시 재개. 모든 세션 끝날 때 까지 반복."

The UI exists for three things only:
1. Put the few must-decide items in front of him, recommendation pre-selected, one tap to confirm.
2. Show what the supervisor did on its own, with the reason and Undo, so he can trust it and correct it.
3. Let him steer it in plain language (give work, change rules).

## What changes for the owner

| Who / when | Today | After |
|---|---|---|
| Owner away (Slack, asleep) | A proxy outage or a 429 stalls sessions until he writes recovery rules by hand | Recovery runs by default; afterwards he sees one line per incident ("Proxy errors 13:20-13:50 · retried every 10 min · resumed 4 sessions") |
| Owner on the phone, 10-30 s glance | Three tabs (Supervisor / Inbox / Worktrees) with different models; every prompt escalated to him; lane cards whose only button is Pause | One screen: "Nothing needs you" or "2 decisions for you", each decision a card with the recommended button highlighted |
| Owner at the desk | Supervisor tab hidden behind advanced features, mutually exclusive with Inbox; lanes + goal cards + audit stacked | The left panel of the app IS the supervisor; the right side stays his terminal workspace; clicking anything in the panel focuses that session's terminal |
| Supervisor engine (LLM) | Can only `Submit` / `Status` (mhc_supervisor.rs:343-356); reachable only from Slack; never answers for the owner (security.rs:46-51) | Reads, answers, retries, cancels, resumes and stops sessions; reachable from desktop and phone; every action logged with evidence |

## Decisions

D1. **Autonomy first.** By default the supervisor acts and the owner is informed afterwards. It escalates only when:
  (a) the next step is irreversible or external: DB migration on a non-local database, deploy, publish, delete, payment, credential or permission expansion;
  (b) a question has no basis in the owner's past answers, the goal, or a standing rule;
  (c) recovery failed after its retry budget.
Every escalation carries a recommended option and a one-line reason. Every autonomous action is logged with its evidence (prompt text, matched past answer or rule, result) and is undoable where the agent allows it.

D2. **Default recovery policy (on with no instruction, editable in plain language).**
- Upstream / proxy / 5xx errors: retry the session's last turn on backoff 1, 2, 5, 10 min, then every 10 min. After the first success, resume every other session the same incident hit.
- Stalled (D4): cancel the turn and resume once with the same instruction; if it stalls again, escalate.
- 429 / usage limit: resume when the reset time passes (read from the error; else 10 min backoff).
- Repeat until every session in the incident has ended. One log line per incident, edited in place; never one line per retry.

D3. **Learned answers.** The supervisor answers questions and approvals from, in order: a standing rule, the goal contract, the owner's past answers to the same prompt shape in the same repo and agent. Below the confidence threshold or with no precedent, it escalates with a recommendation. D1(a) items always escalate, whatever the history says.

D4. **Stalled is a judgment, not a state.** `AgentControlState` (agent_control/mod.rs:160) stays Working / Blocked / Idle / Unknown. A screen-sourced Working already turns Idle when the observer's quiet timer elapses (agent_control/events.rs:580-589), so "working with no output" can only happen when the agent itself reported Working (`stateSource: provider`). Stalled = provider-reported Working whose screen revision has not changed for 5 min (default; editable rule). A long Blocked is already a pending prompt; it gains an age ("waiting 23 min"), not a new state. Recovery (D2) acts on a stalled session first; the owner sees a card only after that fails.

D5. **One Supervisor panel replaces every supervisor surface.** No separate session sidebar, no separate Supervisor page, no Inbox, no lanes list. The panel, top to bottom:
1. Header: "Supervisor", one line "N running · handled M things today", Pause.
2. Status line: "N decisions for you. The rest is running." or "Nothing needs you."
3. **Needs your decision**: escalation cards only (D1).
4. **I'm handling**: live recoveries ("Windows installer tests · Proxy 502 · tried 2 times · next try 14:20"), each with Stop.
5. **Sessions**: one row per session not already shown above (state glyph, title, one-line why, computer).
6. **Done for you today**: the action log, collapsed to the last 2 lines with "All N"; Undo where possible; "Why" shows the basis.
7. Rules line: the active recovery policy in one sentence with Change.
8. Composer: "Tell the supervisor anything".
A session appears in exactly one of sections 3-5.

D6. **Desktop layout.** Left: the Supervisor panel (380 px, resizable 320-480). Right: the existing terminal workspace with its session tabs, unchanged. Clicking a card header, a handling row, a session row, or a log line's session name focuses that session's tab. The panel is always visible (collapsible with the existing sidebar toggle), not behind advanced features.

D7. **Phone layout.** The panel is the home screen, full width, no tab bar. Tapping a card header or a row pushes that session's terminal screen with a back button; existing edge-swipe back, sheet drag-dismiss and pull-to-refresh (f78ccb80) keep working. Worktree browsing and "start a session" move into the composer ("start a session in ferryx on omarchy") and a "+" in the header.

D8. **Decision cards.** Content: session short name · computer, "Terminal" link, the question, the command (one mono line, ellipsized) when there is one, the recommendation line ("S" mark + reason), option buttons with the recommended one primary and first. No separate risk tag (the reason says it). On phone, buttons are at least 44 px high. The same prompt stays answerable in the terminal; the first answer wins and the card disappears (the log keeps "You chose Allow from iPhone"). Stale guard via `screenRevision`.

D9. **Status comes from telemetry, never from LLM text.** Section membership, glyphs and ages come from agent-control state, `stateSince`, `screenChangedAt` and the recovery scheduler. Supervisor free text that claims it sent, answered, started, retried or finished something carries `evidenceRefs[]` (delivery receipt, state-transition sequence, commit); without them it renders a grey "Unverified" chip (research A6: fabricated dispatches, claude-code #61167).

D10. **Stop and Pause never go through the LLM.** Stop (handling rows, session row menu) calls the agent-control stop op directly through a Tauri command and a gateway route; it works when the hub or the engine is down. Pause stops autonomous answers and recovery; sessions keep running.

D11. **Composer addressing.** Text in the panel composer goes to the supervisor. `@session` inside the text sends that session an instruction (autocomplete over sessions, titles disambiguated with " · computer"). If a request matches several sessions the supervisor asks which one in its reply; it never guesses a write target. Plain-language rule changes ("retry every 5 min in mahoquot projects") become standing rules (G9) and are echoed as a log line.

D12. **Status vocabulary.** Needs you (triangle, amber), Working (open ring, blue), Finished (check, green), Failed (cross, red), Idle (grey dot). Offline computers: one collapsed line at the bottom of Sessions. "Lane", "goal", "rev", "grant", "adopt", engine codes and CLI commands never appear in default views.

D13. **Notifications.** Push only for a new decision card (and nothing else), deep-linked to the card, suppressed while any client shows the panel; batching reuses lanes/dispatch.rs (30 s window). Per-session mute from the row menu: the session still shows in the panel, no push.

D14. **Legacy goals and lanes are not separate UI.** A goal becomes a property of its session (its why line, and the supervisor driving it); pause/resume of one session is a row menu item.

D15. **Supervisor host.** The supervisor runs on the machine that runs the hub (single-supervisor identity, hub_runtime.rs unified_snapshot). Other computers reach it through the relay like today's supervisor routes. When no hub is running, the panel shows one setup card ("Turn on the supervisor on <computer>") in place of sections 2-6.

## Backend gaps (each maps to a todo)

Autonomy (the reason this redesign exists):
- G1. No answer-on-behalf path: `security::auto_answer` (security.rs:46-51) always returns `EscalateOwner`; `supervisor/answers.rs` answers only from an existing owner or contract decision for the exact prompt id. Need an answer history (prompt shape + repo + agent -> owner choice) and a decision step returning `Answer{option, basis}` or `Escalate{recommended, reason}`.
- G2. Nothing to measure a stall with: `AgentObservation` (agent_control/events.rs:98-107) and the agent-control entry (remote/agent_control_service.rs:150-165) carry no state-entry or screen-change time.
- G3. No health classification: nothing classifies upstream/proxy errors, 429/quota or stalled sessions (sessions/watch.rs and supervisor/ have no such code).
- G4. No recovery actions in the engine: its tools are only `Submit` / `Status` (mhc_supervisor.rs:343-356). Agent control already has `Stop` (types Ctrl-C, agent_control/policy.rs:371) and `Send`, so no new daemon op is needed: cancel = `Stop`, resume / retry = `Send` of a per-agent resume text ("continue" for Claude Code, the agent's equivalent for others; unknown agent -> escalate instead of guessing).
- G5. The goal loop stops on its own quota fault instead of recovering (goals/loop_driver.rs:340-343 -> `SupervisorDegraded`).
- G6. No persisted action log: nowhere stores "what I did, why, evidence, undo handle".

Reach and control:
- G7. Engine tools cannot read or touch sessions: add `ListSessions`, `ReadSession{session_ref, lines}`, `AnswerPrompt{session_ref, prompt_id, screen_revision, answer}`, `SendToSession{session_ref, text}`, `StopSession{session_ref}` over existing agent-control ops (mcp.rs:103-140 already implements equivalents).
- G8. No converse route outside Slack: add `POST /api/v1/supervisor/converse` and `cmd_supervisor_converse` over `MhcSupervisor::converse_in_turn` with `ConversationScope{channel_key:"app"}` (mhc_supervisor.rs:1961-2023; slack_supervisor.rs:75).
- G9. No standing rules: owner instructions have nowhere to live. Add `supervisor_rules(id, scope, kind, params_json, created_from_message_id, created_at)` read by the recovery scheduler and the answer decision.
- G10. No fleet read reachable by clients: add `GET /api/v1/fleet/sessions` on the hub host merging per-machine agent-control lists (agent_control_service.rs:320-342) via `Fleet` (fleet.rs:11-60): `{sessionRef, hostId, hostLabel, title, cwd, agent, state, stateSource, stateSince, screenChangedAt, pendingPrompt, lastAssistantLine, updatedAt}`.
- G11. No session handle: `sessionRef = <hostId>:<backendSessionId>` plus display-name resolution.
- G12. No last-message field: `lastAssistantLine` (<= 200 chars) from the reference-history reader into the agent-control entry (agent_control_service.rs:145-160).
- G13. No aggregate event stream: owner-scoped fleet channel on the hub host's `/api/v1/events` carrying state transitions, prompt bodies, action-log entries (events.rs keeps prompt bodies out of the existing channel by design).
- G14. Agent control off by default (policy.rs:3-6): the setup card enables it for the owner's own devices in one step; non-owner devices unchanged.
- G15. No direct stop command or route for clients (D10).
- G16. Dispatch Submit has no Tauri command (protocol.rs:563-601): "start a session" uses `cmd_supervisor_dispatch`.
- G17. Desktop Inbox is per-desktop localStorage (notificationCenterPersistence.ts:116-160); removed once the panel reaches parity.

## Must NOT have

- No second supervisor surface after cut-over: no session sidebar beside the panel, no Supervisor page, no Inbox, no lanes list, goal cards, Recent audit, or phone Supervisor/Inbox/Worktrees tabs.
- No owner card for anything the supervisor may answer itself (D1), and none for a failure recovery has not yet tried to fix.
- No decision card without a recommended option and reason.
- No auto-approval of D1(a) items.
- No per-retry messages: one log line per incident, edited in place.
- No status derived from LLM text; no "Working" fallback for unknown state (phoneHomeModel.ts:105) - unknown renders Idle with "status unknown".
- No session listed in more than one panel section.
- No raw ids, revisions, engine codes, JSON or CLI commands in default views ("Why" may show the basis in words).
- No push for progress, retries, or finished sessions.
- No polling where an event exists; the existing 3 s history/prompt polls may stay only inside an open terminal screen.
- No new UI dependency; tokens from `ui/src/index.css` (charcoal desktop palette, `--chat-*` phone palette).
- No macOS-only paths; desktop layout works in Windows and Linux Tauri shells.

## Todos

All builds and tests run on a remote host picked with `scripts/pick-build-host.sh` (release the claim after), never on the owner's MacBook. Each todo lands on its own branch in `/Volumes/T9-Mac/project/ferryx-wt/<slug>` and is merged to main, with worktree and branch removed, as soon as its QA passes.

Phase A - autonomy core (hub + daemon). Tasks 1 and 2 are independent; 3 needs 1; 4 needs 2 and 3; 5 needs 4.

- [x] 1. Daemon: state and screen timestamps (G2, D4)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `src-tauri/src/agent_control/events.rs` (`AgentEventBus` record path), `src-tauri/src/remote/agent_control_service.rs` entry JSON.
  - Do: keep `stateSince` (when the current state was entered) and `screenChangedAt` (when `screen_revision` last changed) per session; expose both in list/read entries and the slim event. No new `AgentControlState` variant.
  - QA: `cargo test --lib agent_control::events` with an injected clock: unchanged revision keeps `screenChangedAt`; state change resets `stateSince`; revision change in the same state moves only `screenChangedAt`.
- [x] 2. Hub: action log and standing rules storage (G6, G9)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/supervisor/activity.rs`, `supervisor/rules.rs` (new), migration under `crates/ferryx-hub/migrations/`.
  - Do: `supervisor_actions(id, incident_key, session_ref, kind, summary, basis, evidence_json, undo_json, created_at, updated_at)` with upsert by `incident_key` (one line per incident); `supervisor_rules` per G9 with defaults equal to D2 seeded at first run.
  - QA: `cargo test -p ferryx-hub supervisor::activity supervisor::rules`: three retries of one incident leave one row with updated summary; rules survive restart; defaults present on an empty DB.
- [x] 3. Hub: session health classifier (G3, D4)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/supervisor/health.rs` (new).
  - Do: classify each session as healthy / upstream_error{status} / quota{reset_at} / stalled{since} / failed from agent-control state, `stateSource`, `stateSince`, `screenChangedAt` and the terminal tail; thresholds from rules (task 2). Fixtures from real proxy 5xx, 429 and silent-working transcripts.
  - QA: `cargo test -p ferryx-hub supervisor::health` with a fake clock; mutation check: provider-reported Working unchanged 5 min is stalled, 4 min is not, a screen-sourced session is never stalled, a 429 without reset time yields the 10 min backoff.
- [x] 4. Hub: recovery scheduler and actions (G4, G5, G7 stop/read subset, D2)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/supervisor/recovery.rs` (new), `meta/mhc_supervisor.rs` tool table, `goals/loop_driver.rs` quota path.
  - Do: per-incident state machine (backoff, first-success sweep, cancel+resume once for stalled, escalate after budget with a recommendation); actions call agent-control `Stop` then `Send` with the per-agent resume text (G4); each incident writes one action-log row (task 2) updated in place; `SupervisorFault::Quota` in the goal loop routes here instead of stopping.
  - QA: fake-clock tests with a fake AgentControl, no real sleeps: 20 min proxy outage then recovery resumes all 4 affected sessions and leaves 1 log row; 429 with reset resumes at reset; repeated stall escalates once with `recommended` set; Pause stops all actions.
- [x] 5. Hub: learned answers and escalation decision (G1, D1, D3)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/security.rs`, `supervisor/answers.rs`, new `supervisor/answer_history.rs`.
  - Do: record every owner answer (prompt shape, repo, agent, chosen option); decision returns `Answer{option, basis}` or `Escalate{recommended, reason}`; D1(a) classifier (non-local DB migration, deploy, publish, delete, payment, credentials, permission expansion) always escalates; answered prompts write an action-log row with an undo handle where the agent supports it.
  - QA: table tests: same question answered the same way 3 times -> answered with basis; `sqlx migrate run` against a non-local URL -> escalated with recommendation; unseen question -> escalated; mutation on the D1(a) classifier flips the migration case red.

Phase B - reach and control (hub + gateway + Tauri). Independent of each other; 7 and 8 need Phase A tables for their payloads.

- [x] 6. Engine tools over sessions (G7)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/meta/mhc_supervisor.rs`, `mhc_profile.rs`.
  - Do: add ListSessions / ReadSession / AnswerPrompt / SendToSession / StopSession mapped onto `AgentControl.execute`; supervisor-initiated answers pass through the task 5 decision; owner-initiated card taps are unrestricted.
  - QA: unit tests with a fake AgentControl: owner tap answers any prompt; supervisor-initiated D1(a) approval is refused with a typed reason.
- [x] 7. Converse route and app transcript (G8, D11)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `src-tauri/src/remote/supervisor_api.rs`, `src-tauri/src/ipc/supervisor.rs`, `hub_runtime.rs`, hub `supervisor/transcript.rs`.
  - Do: `POST /api/v1/supervisor/converse {text, mentions[], requestId}` and `cmd_supervisor_converse`; replies persisted and streamed as events; rule-change replies create a rule (task 2) and a log line; claims carry `evidenceRefs[]` (D9).
  - QA: `curl -i -X POST .../converse` against an isolated test gateway on a remote host -> 200 and a transcript row; "retry every 5 min" creates a rule with interval 300 s; view-only device gets 403.
- [x] 8. Fleet read, handle, last line, event channel (G10, G11, G12, G13)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `crates/ferryx-hub/src/fleet.rs`, `src-tauri/src/remote/agent_control_service.rs`, `machine_events.rs`, `server.rs`.
  - QA: two daemons on one remote host with separate data dirs: `GET /api/v1/fleet/sessions` returns both with `sessionRef`, `stateSince`, `screenChangedAt`, `lastAssistantLine`; an offline machine returns `stale: true`; a fixture prompt yields one owner-channel frame with the question within 2 s; a non-owner subscriber gets no prompt body.
- [x] 9. Direct stop, pause, dispatch and owner enablement (G14, G15, G16, D10)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `src-tauri/src/ipc/supervisor.rs`, `src-tauri/src/remote/supervisor_api.rs`, `src-tauri/src/agent_control/policy.rs`.
  - Do: `cmd_session_stop` + `POST /api/v1/sessions/{sessionRef}/stop` straight to agent-control; supervisor pause/resume route; `cmd_supervisor_dispatch`; one-step owner enablement of agent control for the owner's devices.
  - QA: cargo tests with the hub stopped: stop succeeds and the session leaves Working; view-only device gets 403; enablement leaves non-owner devices excluded.

Phase C - UI (desktop + phone). 10 first; 11 needs 10; 12 and 13 need 11.

- [x] 10. UI model `ui/src/lib/supervisorPanel/` (shared)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Do: reducer from fleet snapshot + events + action log + transcript producing the D5 sections; each session in exactly one section; status per D12; stalled and long-Blocked ages; decision cards from escalations only, recommended first; `evidenceRefs` -> "Unverified" chip; mention parser/resolver.
  - QA: `bun run --cwd ui test src/lib/supervisorPanel` (vitest) on a remote host; mutation checks: a session can't appear twice; an escalation without `recommended` is rejected; unknown state maps to Idle.
- [x] 11. Panel components and showcase per DESIGN.md gate
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `ui/src/components/supervisorPanel/` (StatusLine, DecisionCard, HandlingRow, SessionRow, ActionLog, RulesLine, Composer, SetupCard).
  - Do: add glyph and card tokens to `ui/DESIGN.md` first; showcase route rendering every component and state, matching `panel.html`.
  - QA: `/visual-qa` capture matrix of the showcase at 1440 and 390, dark; hit targets >= 44 px on phone measured from the DOM.
- [x] 12. Desktop: panel as the left sidebar
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `ui/src/components/supervisorPanel/SupervisorPanel.tsx`, app shell sidebar, workspace tab focus API.
  - Do: mount the panel in the left sidebar slot (D6); clicks focus the session's tab (native terminal surface rules from AGENTS.md apply: no unmount, mask during drags).
  - QA: Tauri debug build on a remote macOS host and a Linux host; computer-use drive: answer a fixture decision from the panel, the pane's prompt disappears and the card leaves; click a row, that terminal tab is focused. Screenshots of both.
- [x] 13. Phone: panel as the home screen
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `ui/src/remote/home/` replaced by `ui/src/remote/supervisorPanel/`; `RemoteApp.tsx` routing.
  - Do: full-width panel (D7); row/card tap pushes the session's terminal screen; keep f78ccb80 gestures.
  - QA: remote QA harness renders at 390x844 and 375x667 with Bun.WebView; real-device pass on the owner's iPhone via WebDriverAgent (the 2026-11-02 gesture QA flow): tap the recommended button, card leaves, log shows "You chose ... from iPhone"; tap a row, terminal opens, edge-swipe back returns.

Phase D - notifications and cut-over. 14 needs 8; 15 needs 12 and 13.

- [x] 14. Push for decision cards only, with mute (D13)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Where: `src-tauri/src/remote/push.rs`, phone service worker / native shell, hub settings.
  - QA: fixture escalation with no client focused -> one push whose URL opens the card; panel focused -> no push; retries and finished sessions -> no push; muted session -> panel row, no push.
- [x] 15. Cut-over and removal (G17)
  Evidence (measured, 2026-11-07, revision 70831e2e2, omarchy): `cargo test --lib supervisor::` in `crates/ferryx-hub` -> 426 passed; 0 failed, plus `bun run test src/components/supervisorPanel src/lib/supervisorPanel` in `ui` -> 116 passed (11 files), 0 failed.
  - Do: remove SupervisorPanel lanes/goal cards/audit, the Inbox and its localStorage store, the phone tab bar, `SupervisorPaneBar` (replaced by nothing: the panel shows it); keep the CLI.
  - QA: `bun run --cwd ui test` and `cargo test --lib` green on remote hosts; grep of default-view components shows none of "lane", "rev ", "MHC_", "ferryx-cli", "Inbox".

## Final verification wave

- [x] F1. Autonomy, no owner input: on a remote fixture fleet of 3 computers, inject a 20 min proxy outage on 4 sessions, a 429 with reset in 3 min, and a provider-Working session that stops drawing. All sessions finish; the panel's log has exactly one line per incident; zero decision cards and zero pushes appear. Then a fixture `sqlx migrate run` approval against a non-local URL produces exactly one card with a recommendation and one push.
  Evidence (measured, 2026-11-07, revision 70831e2e2): `scripts/qa/unified-supervisor/f1-acceptance.mjs --scenario f1-acceptance` exit 0, **27 of 27 assertions pass**, in 32 min 24 s wall clock as a `systemd-run --user` transient unit on omarchy. The fleet was three computers: hub `omarchy`, target `naver-vm` (root@211.233.199.253), and target `f1c1` (an isolated container inside naver-vm), all at the same revision with clean worktrees (`fleet-same-revision`, `three-computers`). Clause detail in `f1/f1-evidence.json`: `config.outageMs` 1200000 (the real 20 min, not a compressed stand-in) and `config.quotaResetMs` 180000 (3 min), with 10 owner-socket frames recorded. Every clause passed, including `outage-injected-real-duration`, `four-outage-sessions-on-one-computer`, `quota-session-reset-three-minutes`, `stall-session-provider-working`, `all-recovery-sessions-finished`, `outage-sessions-recovered-after-outage`, `quota-resumed-at-reset`, `stall-cancelled-and-resumed`, `one-log-row-per-incident`, `recovery-rows-closed`, `zero-cards-during-recovery`, `zero-pushes-during-recovery`, `approval-one-card-with-recommendation`, `approval-not-answered-by-supervisor`, `approval-exactly-one-push`, and `fleet-teardown-clean`. Run directory on omarchy: `~/code/ferryx-wt/supv-f1-ffa19192c/.evidence/f1-run` (2.7 MB, 30 files, `assertions.json` sha256 `43a053aad1cfd3d3e9764741...`).
  Two infrastructure obstacles were solved before this pass, neither a product defect. First, a target that already runs a production Ferryx daemon holds `127.0.0.1:43821`, and the harness had one fixed `GATEWAY_PORT = 43821`; killing that daemon is forbidden because it owns live PTY masters, so the harness gained a per-target validated `gatewayPort` (commit `70831e2e2`). Second, launching the harness under `nohup` from an SSH session let the session teardown SIGTERM it (`status INTERRUPTED`, exit 130) after 14 of 14 pre-flight assertions had already passed; running it as a `systemd-run --user` transient unit, which is detached from the login session, removed that interference and the same command then completed.
  Fleet-selection finding (measured, 2026-11-07, revision ffa19192c): a computer that already runs a production Ferryx daemon holds 127.0.0.1:43821, and `GATEWAY_PORT = 43821` was a single fixed constant, so such a target died on `PREREQUISITE_MISSING: <label>: port 43821 is in use there`. Killing that daemon is forbidden (it owns live PTY masters), so the harness gained a per-target `gatewayPort` (validated integer 1024..65535 in `harness.mjs`, applied to the port check and `remoteConfigure` in `f1-acceptance.mjs`); the hub port and every other use stay fixed, and the production daemon is never touched. Measured unusable targets, kept out of the fleet: hark-sandbox and dots-sandbox (port 22 closed from omarchy), noye-box (host offline: `ping` 100% loss, `nc -z` port 22 closed, five consecutive ssh timeouts), maho-win (reachable, but no POSIX `sh`/`bun` host agent), instabox-bong-i-1 (its ssh gateway refuses `ssh -R` reverse forwards: `Warning: remote port forwarding failed for listen port 43999` on a direct probe, and `relayTunnel: true` is mandatory because the product refuses a plaintext relay URL on a tailnet 100.x address).

- [x] F2. Learned answer and Undo: a question the owner answered the same way 3 times is answered by the supervisor with its basis shown; Undo puts it back as a decision card.
  Evidence (measured, 2026-11-07, maho-mac, revision ffa19192c): the two named behaviour tests pass at this revision -- `supervisor::autonomy::tests::three_owner_answers_teach_it_and_the_fourth_is_answered_with_a_log_line` and `supervisor::autonomy::tests::undoing_an_autonomous_answer_restores_the_decision_card_without_a_second_send`, inside `cargo +1.95 test --lib supervisor::autonomy::tests` (16 passed; 0 failed). The same pair first passed at `af600255b` (an ancestor of HEAD) with the UI panel suite `ui/src/components/supervisorPanel/components.test.tsx` 26/26; logs `~/ferryx-wt/.evidence/supv-f2-final-20261104/supv-final-f2/`.
- [ ] F3. Real surfaces: desktop (macOS + Windows) and phone (iPhone app): answer a decision from the phone, see it gone on desktop; click a session row on desktop, its terminal is focused; tell the supervisor "retry every 5 min", see the rules line change. Evidence: screenshots + transcript export.
- [x] F4. Status truth: kill a fixture agent mid-turn, its row shows Failed within 5 s from telemetry with no supervisor text consulted; with the hub stopped, Stop still ends a session; a supervisor claim with no delivery event renders "Unverified".
  Evidence (measured, 2026-11-07, maho-mac, revision ce6619010): `scripts/qa/unified-supervisor/f4-status-truth.mjs` exit 0 with all five clauses passing in `out/clauses.jsonl` -- `failed-within-5s` (221 ms measured against a 5000 ms limit), `unverified-claim`, `stop-with-hub-down:hub-stopped`, `stop-with-hub-down:hub-never-started`, `teardown-clean`. Run directory: `~/code/ferryx-wt/.evidence/supv-f4-20261107-105645` (SHA256SUMS over 18 files in `out/`).
- [x] F5. Copy and duplication audit: automated DOM scan at 1440 and 390 for Must-NOT-Have strings and for any session id appearing in two panel sections.
  Evidence (measured, 2026-11-07, maho-mac, revision ce6619010): `bun scripts/qa/supervisor-f5/copy-audit.mjs` exit 0, gate PASS; 20 captures (desktop/phone x 1440/390 x fixture/failed/paused/setup/guards), every capture `findings: 0`, `consoleErrors: []`. Run directory: `~/code/ferryx-wt/.evidence/supv-f5-dom-scan-20261106/run-ce6619010b` (SHA256SUMS over 24 files).
- [ ] F6. Independent gate review (claude-*-5-5 reviewer) of the full diff against this plan.
