# F4 status truth: source findings (2026-11-03)

Base: d552f528be8fbdad35fe9d557acd82103e1bb6ff, branch qa/supv-f4-status-truth-20261103.
Plan row: `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md:193` (F4).

Method: source reading only. No build or test was run for this record. Every existing test named
below is UNVERIFIED until a remote gate runs it. This record makes no acceptance claim.

Verdict: F4 is not satisfiable on this checkout. Clause 1 has no production path; clauses 2 and 3
are partially covered. Production repairs belong to a separate implementation lane.

## Clause 1: a killed fixture agent's row shows Failed within 5 s, from telemetry

Status: GAP (product). Acceptance evidence cannot exist until it is repaired.

Path today when an agent process dies mid-turn:

1. `src-tauri/src/daemon/server.rs:5226-5230`: the foreground observer (250 ms tick at
   `server.rs:5066`, full listing on a marker change or every 2 s, `server.rs:2086`) sees the shell
   own the pane again and takes `AgentProcessEdge::Released` -> `agent_states.release_foreground(&id)`.
2. `src-tauri/src/daemon/agent_state.rs:223-244`: `release_foreground` publishes `state: "idle"`,
   `origin: ProcessReleased`, `outcome: None`. The previous `working` state is read into `previous`
   and only logged; "left mid-turn" is not kept.
3. `crates/ferryx-hub/src/fleet.rs:190-192`: the fleet entry maps that to `state: "idle"`.
4. `ui/src/lib/supervisorPanel/model.ts:343-357`: a row is `failed` only when
   `PanelViewOptions.health` holds a fresh `failed` report; otherwise `glyphFor("idle")` -> Idle.

Missing links:

- No producer of `failed` for the panel. `HealthKind::Failed` is built only at
  `crates/ferryx-hub/src/supervisor/health.rs:87-96`, and only for `state == "unknown"` with a crash
  marker in the terminal tail. A released pane is `idle`, so a killed agent is never Failed. The
  classifier runs only inside the recovery scheduler (`src-tauri/src/hub_runtime/panel_autonomy.rs:66`);
  its results are not published to any panel source.
- No health on the wire. `PanelSnapshot` (`crates/ferryx-hub/src/supervisor/panel_contract.rs:265`,
  mirrored in `ui/src/lib/supervisorPanel/contract.ts`) and the `panel.session` frame
  (`src-tauri/src/remote/machine_events.rs:38-54`) carry no health.
- No consumer passes health. `PanelSurface` is rendered without `health` at
  `ui/src/components/supervisorPanel/SupervisorPanelHost.tsx:111`,
  `ui/src/remote/supervisorPanel/WidePanel.tsx:19` and
  `ui/src/remote/supervisorPanel/PhoneSupervisorHome.tsx:79`. Only the showcase passes it
  (`ui/src/components/supervisorPanel/SupervisorPanelShowcase.tsx:89`).
- The desktop panel gets no live frames. `ui/src/lib/tauri.ts:600-603` listens for
  `supervisor_panel_event`; no Rust source emits that name (a search of `src-tauri/src` finds none).
  The desktop host reloads only after its own writes (`SupervisorPanelHost.tsx:82-91`); its 30 s tick
  (`SupervisorPanelHost.tsx:74-77`) moves ages and does not reload. The phone does receive
  `panel.session` frames (`machine_events.rs:523`, `ui/src/remote/supervisorPanel/usePhonePanel.ts`),
  but they carry no health.

Existing tests that do not cover the clause: `ui/src/lib/supervisorPanel/model.test.ts:460-487` and
`ui/src/components/supervisorPanel/components.test.tsx:217` inject a hand-built `failed` health into
the model and surface. They prove the renderer, not a telemetry path.

Smallest repair seam (not applied here):

1. Daemon: in `agent_state.rs:223` `release_foreground`, keep "left mid-turn"
   (`previous.state == "working"`) as `outcome: Some(AgentOutcome::Error)` (existing field,
   `src-tauri/src/daemon/protocol.rs:1938`) and expose it on the agent-control entry
   (`src-tauri/src/remote/agent_control_service.rs:183-200`).
2. Hub: map it to `failed` health in `fleet.rs:166-213` / `health.rs` and carry it to clients
   (snapshot and `panel.session` frame); a contract change to `panel_contract.rs` and `contract.ts`
   together.
3. Desktop channel: relay the panel frames onto `remote_event_tx` as `supervisor_panel_event`, next
   to the `agentPromptChanged` relay (`server.rs:4091-4115`); `src-tauri/src/lib.rs:1311` already
   re-emits remote events by name.
4. UI: pass health from the three hosts above into `PanelSurface`.

Acceptance test to land with the repair (deterministic, no sleeps): a unix daemon test in the style
of `protocol_release_tests` (`server.rs:9901`): real PTY, a fixture agent script named `omo` blocking
in `read`, a canonical `working` report, `spawn_foreground_observer()`, subscribe to
`remote_event_tx` before sending SIGKILL to the agent pid, then a 5 s `tokio::time::timeout` on the
first panel frame whose session is `failed`. It must fail on this base (the release yields `idle`).
Plus a `SupervisorPanelHost` test that feeds that frame through `subscribe` and finds
`data-state="failed"` on the row. Neither path reads supervisor text.

## Clause 2: with the hub stopped, Stop still ends a session

Literal QA wording (plan `SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md:159`, task 9): "cargo tests
with the hub stopped: stop succeeds and the session leaves Working". The assertion is that the session
leaves Working, not that the PTY exits.

Status: route covered; "leaves Working" not asserted.

- Production: `POST /api/v1/sessions/{sessionRef}/stop` -> handler at
  `src-tauri/src/remote/agent_control_service.rs:1287` -> agent-control `Stop`
  (`agent_control_service.rs:534`, provider interrupt key). No hub call on the path.
- Existing tests (unrun here):
  - `src-tauri/src/remote/agent_control_service.rs:1433`
    `a_stop_by_session_ref_interrupts_the_pane_with_no_hub_open`: asserts `!supervisor_available()`,
    receipt `accepted`, exactly `\x1b` typed, audit `stop/OK`.
  - `src-tauri/src/remote/supervisor_api_tests.rs:103`
    `a_stop_routes_by_session_ref_and_stays_on_this_computer`: the HTTP route reaches agent control
    (typed NOT_FOUND) with no hub.
- Gap: no test observes the session leave Working after the hub-down stop.

## Clause 3: a supervisor claim with no delivery event renders "Unverified"

Status: covered for the action log; not for converse replies.

- Production: `ui/src/lib/supervisorPanel/model.ts:261-270` (`unverified` = claim kind with empty
  `evidence`), rendered at `ui/src/components/supervisorPanel/ActionLog.tsx:18-19`.
- Existing tests (unrun here): `ui/src/components/supervisorPanel/components.test.tsx:275`
  "marks Unverified only on claim kinds without evidence"; hub side
  `crates/ferryx-hub/src/supervisor/tests/transcript.rs:202`
  `converse_stores_the_owner_text_and_an_unverified_reply_on_channel_app` (engine reply stored with
  `evidence: []`, `crates/ferryx-hub/src/supervisor/transcript.rs:571-581`).
- Gaps: no UI renders the converse transcript, so an unattested supervisor reply never shows the
  chip. The rule keys on any evidence, not on a delivery event: an entry carrying any evidence ref
  (for example `kind: "prompt"`) counts as verified.

## Unverified commands (to run on a remote host, not run here)

- `cargo test --lib remote::agent_control_service::tests::a_stop_by_session_ref_interrupts_the_pane_with_no_hub_open` (in `src-tauri`)
- `cargo test --lib remote::supervisor_api_tests::a_stop_routes_by_session_ref_and_stays_on_this_computer` (in `src-tauri`)
- `cargo test -p ferryx-hub supervisor::tests::transcript::converse_stores_the_owner_text_and_an_unverified_reply_on_channel_app`
- `bun run --cwd ui test src/components/supervisorPanel/components.test.tsx`
- `bun run --cwd ui test src/lib/supervisorPanel/model.test.ts`

## Remaining literal F4 gaps

1. Clause 1 entirely: producer, wire, desktop channel and consumers.
2. Clause 2: no assertion that the session leaves Working after a hub-down stop.
3. Clause 3: converse replies have no Unverified rendering; verification keys on any evidence, not a
   delivery event.
