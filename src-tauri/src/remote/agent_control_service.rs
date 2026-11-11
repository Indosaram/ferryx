//! Daemon side of the agent-neutral control surface (`crate::agent_control`).
//!
//! A child of `remote::server` on purpose: every operation reuses the reference-chat machinery
//! that already serves the phone client — the reconstructed screen, the per-agent prompt
//! detectors, the screen-validated answer and the ordered per-target input queue — so a
//! supervisor and a person on the phone share ONE queue per pane and cannot interleave keys.
//!
//! The caller is the machine owner: the request arrives on the daemon's owner-only control
//! socket, so there is no device token to re-check between writes. Nothing here launches,
//! signals or kills a process; Stop is the provider's interrupt key.

use super::*;

use crate::agent_control::events::{self, WaitHit};
use crate::agent_control::policy::{self, authorize_agent_control, AuditRecord};
use crate::agent_control::{
    derive_status, AgentControlOp, AgentControlState, AgentStateSource, AgentTier, ProviderReport,
};
use crate::scoped_contracts::{Epoch, ScopeError, ScopeErrorCode, TargetRef};
use futures_util::stream::{self, StreamExt as _};

/// Record that a person typed into `session_id` through the daemon input path.
pub(crate) fn note_human_keystrokes(session_id: &str) {
    let clock = reference_input::SystemReferenceClock;
    let now = clock.now_ms();
    reference_chat_runtime().input.note_raw_input(session_id, now);
    crate::hub_runtime::note_lane_keystrokes(session_id, now);
}

/// Raw keys or a paste chunk are about to reach `session_id` outside the per-pane input queue
/// (a terminal keystroke, a paste, a mirrored or phone terminal). Every raw write path calls
/// this right before its bytes go out, so no supervisor delivery attributes the turn they
/// start to itself, however long ago other panes were typed into.
pub(crate) fn note_raw_write(bus: &events::AgentEventBus, session_id: &str) {
    bus.note_input(session_id, None);
}

fn refuse_if_human_typing(session_id: &str) -> Result<(), ScopeError> {
    let Some(typed_at) = reference_chat_runtime().input.last_raw_input_ms(session_id) else {
        return Ok(());
    };
    let now = reference_input::SystemReferenceClock.now_ms();
    if now.saturating_sub(typed_at) < policy::HUMAN_TYPING_WINDOW_MS {
        return Err(reference_chat_scope(
            ScopeErrorCode::ControlConflict,
            "a person typed in this pane; supervisor input is refused",
        ));
    }
    Ok(())
}

fn mutation_authorize(
    session_id: String,
    extra: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Box<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    Box::new(move || {
        if let Some(extra) = &extra {
            extra()?;
        } else {
            // The owner socket has no device token. The uid check on connect is the authorization.
            owner_authorized()()?;
        }
        refuse_if_human_typing(&session_id)
    })
}

/// How many sessions a list reconstructs at once.
const LIST_CONCURRENCY: usize = 8;

fn owner_authorized() -> Box<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    Box::new(|| Ok(()))
}

fn control_target(state: &RemoteGatewayState, session_id: &str) -> reference_types::ReferenceTargetRef {
    reference_types::ReferenceTargetRef::without_provider_session(TargetRef {
        host_id: reference_files::reference_host_id(),
        owner_id: state.reference_owner_id.clone(),
        epoch: Epoch(state.daemon_epoch.load(std::sync::atomic::Ordering::Acquire)),
        backend_session_id: session_id.to_string(),
    })
}

fn provider_report(state: &RemoteGatewayState, session_id: &str) -> Option<ProviderReport> {
    state.machine_services.as_ref()?.sessions.agent_control_report(session_id)
}

/// The wire error a refusal travels as on the daemon socket.
pub(crate) fn error_response(error: ScopeError) -> crate::daemon::protocol::DaemonResponse {
    crate::daemon::protocol::DaemonResponse::Error {
        message: error.message,
        code: Some(reference_chat_wire_code(error.code).to_string()),
        details: None,
    }
}

/// One session's status, and the screen it was read from.
struct Observed {
    entry: serde_json::Value,
    screen: Option<reference_types::ReferenceScreenSnapshot>,
}

/// One session's status without its last assistant line. Every mutation path (send, answer,
/// prompt reads) uses this: it never opens the history store.
async fn observe(state: &Arc<RemoteGatewayState>, session_id: &str) -> Result<Observed, ScopeError> {
    observe_entry(state, session_id, false).await
}

/// One session's status with `lastAssistantLine`, for the List and Read ops only.
async fn observe_with_line(state: &Arc<RemoteGatewayState>, session_id: &str) -> Result<Observed, ScopeError> {
    observe_entry(state, session_id, true).await
}

async fn observe_entry(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    with_last_line: bool,
) -> Result<Observed, ScopeError> {
    let details = state
        .session_backend
        .describe_session(session_id)
        .await
        .map_err(|message| reference_chat_scope(ScopeErrorCode::NotFound, message))?;
    let target = control_target(state, session_id);
    let screen = reference_chat_snapshot_screen(state, &target).await.ok();
    let provider = provider_report(state, session_id);
    let detection = screen.as_ref().and_then(|snapshot| {
        let input = crate::agent_detect::ScreenInput {
            rows: snapshot.text.lines().map(str::to_string).collect(),
            title: String::new(),
        };
        crate::agent_detect::default_engine().detect(&input, None)
    });
    let agent = crate::agent_detect::resolve_agent_identity(
        provider.as_ref().and_then(|report| report.agent.as_deref()),
        detection.as_ref().map(|found| found.manifest_id.as_str()),
    );
    let readable = screen.as_ref().is_some_and(|snapshot| snapshot.is_answerable());
    let pending = match (&agent, &screen) {
        (Some(agent), Some(snapshot))
            if readable && reference_prompts::ReferencePromptFamily::of_agent(agent).is_some() =>
        {
            let identity = reference_chat_identity(state, &target, Some(agent)).await;
            let session = reference_chat_prompt_session(state, &identity).await;
            reference_prompts::reference_prompt_occurrence(
                session_id,
                reference_prompts::detect_reference_prompt_in_session(agent, &snapshot.text, &session.borrow()),
                &snapshot.revision,
            )
        }
        _ => None,
    };
    let provider_outcome = provider.as_ref().and_then(|report| report.outcome);
    let mut status = derive_status(provider.as_ref(), detection.map(|found| found.state), pending.is_some());
    // The observer's quiet timer is the only thing that can call a quiet screen idle. When it
    // has already judged this exact screen, read and list report the same state events did.
    if status.state == AgentControlState::Unknown && pending.is_none() {
        let revision = screen.as_ref().map(|snapshot| snapshot.revision.as_str());
        if let Some(seen) = state.agent_events.latest(session_id) {
            if revision.is_some() && seen.screen_revision.as_deref() == revision {
                status.state = seen.state;
                status.source = seen.state_source;
            }
        }
    }
    // The bus's timestamps belong to its own latest reading; they only describe this entry's
    // state and screen while that reading still matches them.
    // `stamps` is the latest reading, or the persisted stamps of a previous run before this
    // run's first reading, so a restarted daemon still reports the real times.
    let seen = state.agent_events.stamps(session_id);
    let state_since = seen.as_ref().filter(|seen| seen.state == status.state).and_then(|seen| seen.state_since_ms);
    let screen_revision = screen.as_ref().map(|snapshot| snapshot.revision.clone());
    let screen_changed_at = seen
        .as_ref()
        .filter(|seen| seen.screen_revision == screen_revision)
        .and_then(|seen| seen.screen_changed_at_ms);
    let last_assistant_line = match &agent {
        Some(agent) if with_last_line => {
            let identity = reference_chat_identity(state, &target, Some(agent)).await;
            last_assistant_line(state, session_id, identity).await
        }
        _ => None,
    };
    let mut entry = serde_json::json!({
        "sessionId": session_id,
        "running": details.running,
        "cols": details.cols,
        "rows": details.rows,
        "workspaceId": details.workspace_id,
        "cwd": details.worktree_path.map(|path| path.to_string_lossy().into_owned()),
        "agent": agent,
        "tier": status.tier,
        "state": status.state,
        "stateSource": status.source,
        "stateSince": state_since,
        "detail": provider.as_ref().and_then(|report| report.detail.clone()),
        "pending": pending,
        "screenRevision": screen_revision,
        "screenChangedAt": screen_changed_at,
        "screenReadable": readable,
        // When this entry was read, so a hub that caches a list can show how old it is.
        "observedAt": observed_at_ms(),
    });
    let failed = state.machine_services.as_ref().is_some_and(|services| services.sessions.agent_control_failed(session_id));
    if failed {
        entry["health"] = serde_json::json!({ "type": "failed", "reason": "the agent exited in the middle of a turn" });
    }
    let completed = state.agent_events.completion(session_id).is_some()
        || provider_outcome.is_some();
    if with_last_line {
        // A remote or non-native session has no transcript line; the screen's last line stands
        // in for it, but only in a pane an agent runs in.
        let screen_line = match (&agent, &screen) {
            (Some(_), Some(snapshot)) if last_assistant_line.is_none() => last_screen_line(&snapshot.text),
            _ => None,
        };
        let cause = (agent.is_some() || failed).then(|| stop_cause(status.state, failed, completed)).flatten();
        let reason = session_reason(
            pending.as_ref().map(|prompt| prompt.question.as_str()).filter(|question| !question.trim().is_empty())
                .or(pending.as_ref().map(|prompt| prompt.title.as_str())),
            last_assistant_line.as_deref().or(screen_line.as_deref()),
            cause,
        );
        entry["lastAssistantLine"] = serde_json::json!(last_assistant_line);
        entry["reason"] = serde_json::json!(reason);
    }
    let axes = attention_axes(
        status.state,
        pending.as_ref().map(|prompt| prompt.kind),
        failed,
        completed,
        read_for(session_id),
        ownership_for(session_id),
    );
    entry["attention"] = serde_json::json!(ferryx_hub::supervisor::panel_contract::Attention::of(axes));
    Ok(Observed { entry, screen })
}

/// Why the session stopped, in one line: the pending prompt's question, else the agent's last
/// line, else the stop cause. Whitespace is collapsed and the line cut to
/// [`LAST_ASSISTANT_LINE_MAX_CHARS`]; nothing to say is `None`, never an empty string.
fn session_reason(question: Option<&str>, last_line: Option<&str>, cause: Option<&str>) -> Option<String> {
    [question, last_line, cause].into_iter().flatten().find_map(|text| {
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!line.is_empty()).then(|| line.chars().take(LAST_ASSISTANT_LINE_MAX_CHARS).collect())
    })
}

/// How far in from the left edge a user echo, a reply bullet or the input line sits. Anything
/// further in is a reply's own text (a quote, a list, a tool result), never a new turn.
const TURN_MARGIN: usize = 1;

/// How far in a turn's timing line sits (`✻ Worked for 12s · done 4:04 PM`, `  4:08 PM`).
const TIMING_MARGIN: usize = 2;

/// Leading blanks that mark a right-aligned notice rather than transcript text.
const NOTICE_INDENT: usize = 40;

/// How many non-empty bottom lines can hold the input box; it sits under the transcript, with
/// at most its frame and a footer below it.
const INPUT_BOX_TAIL: usize = 4;

fn indent(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// A line that starts a turn's input: a prompt marker at the left margin.
fn is_prompt_line(line: &str) -> bool {
    indent(line) <= TURN_MARGIN && line.trim_start().starts_with(['❯', '›', '>'])
}

/// A prompt line with something typed after the marker: a user turn in the transcript.
fn is_user_echo(line: &str) -> bool {
    is_prompt_line(line) && line.trim_start().chars().skip(1).any(char::is_alphanumeric)
}

/// The first line of an agent reply: a reply bullet at the left margin.
fn is_reply_bullet(line: &str) -> bool {
    indent(line) <= TURN_MARGIN && line.trim_start().starts_with(['⏺', '●', '•'])
}

/// A turn's timing line: near the left edge and ending with a clock time (`4:03 PM`).
fn is_timing_line(line: &str) -> bool {
    indent(line) <= TIMING_MARGIN && !line.trim().is_empty() && ends_with_clock(line)
}

/// Whether `line` ends with a clock time, with or without AM/PM after any kind of space.
fn ends_with_clock(line: &str) -> bool {
    let line = line.trim_end();
    let line = ["AM", "PM", "am", "pm"]
        .iter()
        .find_map(|suffix| line.strip_suffix(suffix))
        .map_or(line, str::trim_end);
    let Some((hours, minutes)) = line.rsplit_once(':') else { return false };
    let hours: String = hours.chars().rev().take_while(char::is_ascii_digit).collect();
    minutes.len() == 2 && minutes.chars().all(|c| c.is_ascii_digit()) && (1..=2).contains(&hours.len())
}

/// The last line of the agent's reply in the newest finished turn on the screen, or `None`.
///
/// Conservative on purpose: a screen only yields a line when it shows a whole turn, which every
/// agent draws the same way: a user echo (`❯ ...`), then a reply bullet (`⏺`, `●`, `•`) with
/// its continuation lines, then the turn's timing line. The input box at the bottom (a prompt
/// line among the last few) is cut off first, and a user echo after the timing line means a new
/// turn started. Banners, tips, footers, status bars, model or path lines and token counts are
/// never part of that shape, so a screen without it (any agent that does not draw it) yields
/// `None` and the reason falls back to the stop cause.
fn last_screen_line(text: &str) -> Option<String> {
    const TOOL_MARKS: &[char] = &['⎿', '└'];
    let mut lines: Vec<&str> = text.lines().collect();
    let tail: Vec<usize> = (0..lines.len()).filter(|&at| !lines[at].trim().is_empty()).rev().take(INPUT_BOX_TAIL).collect();
    // The input box is a prompt line among the last few with no reply under it; a prompt
    // with a reply bullet below it is a user turn, not the box.
    if let Some(&input) = tail.iter().find(|&&at| is_prompt_line(lines[at])) {
        if !lines[input + 1..].iter().any(|line| is_reply_bullet(line)) {
            lines.truncate(input);
        }
    }
    let timing = lines.iter().rposition(|line| is_timing_line(line))?;
    if lines[timing + 1..].iter().any(|line| is_user_echo(line)) {
        return None;
    }
    let echo = lines[..timing].iter().rposition(|line| is_user_echo(line))?;
    let bullet = echo + 1 + lines[echo + 1..timing].iter().rposition(|line| is_reply_bullet(line))?;
    let mut last = lines[bullet].trim_start().chars().skip(1).collect::<String>();
    for line in &lines[bullet + 1..timing] {
        let text = line.trim_start();
        if text.is_empty() || indent(line) >= NOTICE_INDENT || text.starts_with(TOOL_MARKS) {
            continue;
        }
        if indent(line) <= indent(lines[bullet]) {
            break;
        }
        last = text.to_string();
    }
    let last = last.trim_end_matches(|c: char| c.is_whitespace() || c == '│' || c == '█').trim();
    (last.chars().any(char::is_alphanumeric) && !ends_with_clock(last)).then(|| last.to_string())
}

/// A label for why an agent stopped when it said nothing itself.
fn stop_cause(state: AgentControlState, failed: bool, completed: bool) -> Option<&'static str> {
    if failed {
        return Some("The agent exited in the middle of a turn");
    }
    match state {
        AgentControlState::Blocked => Some("Waiting for input"),
        AgentControlState::Idle | AgentControlState::Unknown if completed => Some("Finished its turn"),
        _ => None,
    }
}

/// Whether the owner has looked at the session's current episode. The daemon's read store
/// replaces this seam; until then every episode is unread.
fn read_for(_session_id: &str) -> ferryx_hub::supervisor::panel_contract::AttentionRead {
    ferryx_hub::supervisor::panel_contract::AttentionRead::Unread
}

/// Who acts on the session next, as this daemon knows it. Spawn lineage replaces this seam
/// with `delegated` for a child pane; the hub alone sets `supervisor`.
fn ownership_for(_session_id: &str) -> ferryx_hub::supervisor::panel_contract::AttentionOwnership {
    ferryx_hub::supervisor::panel_contract::AttentionOwnership::Owner
}

/// The attention axes of one reading. A pending prompt is the demand (an approval or a plan
/// asks for approval, anything else is a question), else an agent that died mid-turn is an
/// error. A turn counts as completed once the agent left working (an observer edge) or
/// reported an outcome.
fn attention_axes(
    state: AgentControlState,
    pending: Option<reference_types::ReferencePromptKind>,
    failed: bool,
    completed: bool,
    read: ferryx_hub::supervisor::panel_contract::AttentionRead,
    ownership: ferryx_hub::supervisor::panel_contract::AttentionOwnership,
) -> ferryx_hub::supervisor::panel_contract::AttentionAxes {
    use ferryx_hub::supervisor::panel_contract::{AttentionActivity, AttentionAxes, AttentionCompletion, AttentionDemand};
    use reference_types::ReferencePromptKind;
    let demand = match pending {
        Some(ReferencePromptKind::Approval | ReferencePromptKind::Plan) => AttentionDemand::Approval,
        Some(ReferencePromptKind::Question | ReferencePromptKind::Menu) => AttentionDemand::Question,
        None if failed => AttentionDemand::Error,
        None => AttentionDemand::None,
    };
    let activity = match state {
        AgentControlState::Working => AttentionActivity::Working,
        AgentControlState::Blocked | AgentControlState::Idle => AttentionActivity::Stopped,
        AgentControlState::Unknown => AttentionActivity::Unknown,
    };
    let completion = if completed { AttentionCompletion::Reported } else { AttentionCompletion::None };
    AttentionAxes { demand, activity, completion, read, ownership }
}

fn observed_at_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
}

/// The longest `lastAssistantLine` a list or read entry carries.
const LAST_ASSISTANT_LINE_MAX_CHARS: usize = 200;

/// How many newest turn boundaries are read to find the last assistant message.
const LAST_ASSISTANT_LINE_TURNS: usize = 4;

/// The last line of the session's newest assistant text, from the same history reader the phone
/// chat uses. `None` when the store is unavailable, remote, scrollback-only or holds no
/// assistant text yet; a missing line never fails a list or read.
async fn last_assistant_line(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    identity: reference_history::ReferenceHistoryIdentity,
) -> Option<String> {
    if reference_chat_session_is_remote(state, session_id).await {
        return None;
    }
    let home = reference_chat_history_home(state)?;
    let siblings = reference_chat_siblings(state, session_id).await;
    let stream = reference_chat_resolve_history(
        home,
        identity.clone(),
        siblings,
        reference_history::resolve_reference_history_stream,
    )
    .await
    .ok()?;
    let page = reference_history::read_reference_history_page(&stream, &identity, LAST_ASSISTANT_LINE_TURNS, None)
        .await
        .ok()?;
    if !page.is_native() {
        return None;
    }
    last_assistant_line_of(&page.turns)
}

/// The last non-empty line of the newest assistant text part, cut to
/// [`LAST_ASSISTANT_LINE_MAX_CHARS`] characters.
fn last_assistant_line_of(turns: &[reference_types::ReferenceTurn]) -> Option<String> {
    turns
        .iter()
        .rev()
        .filter(|turn| turn.role == reference_types::ReferenceTurnRole::Assistant)
        .flat_map(|turn| turn.parts.iter().rev())
        .find_map(|part| match part {
            reference_types::ReferencePart::Text { text, .. } => text
                .lines()
                .rev()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.chars().take(LAST_ASSISTANT_LINE_MAX_CHARS).collect::<String>()),
            _ => None,
        })
}

async fn live(state: &Arc<RemoteGatewayState>, session_id: &str) -> Result<(), ScopeError> {
    reference_chat_ensure_live(state, session_id).await
}

fn outcome_result(outcome: reference_input::ReferenceInputOutcome) -> Result<serde_json::Value, ScopeError> {
    match outcome {
        reference_input::ReferenceInputOutcome::Accepted { receipt } => Ok(serde_json::json!({ "receipt": receipt })),
        reference_input::ReferenceInputOutcome::NotTyped { error } => Err(error),
        reference_input::ReferenceInputOutcome::OutcomeUnknown { message, .. } => {
            Err(reference_chat_scope(ScopeErrorCode::OperationOutcomeUnknown, message))
        }
    }
}

fn joined(result: Result<reference_input::ReferenceInputOutcome, tokio::task::JoinError>) -> Result<serde_json::Value, ScopeError> {
    match result {
        Ok(outcome) => outcome_result(outcome),
        Err(error) => Err(reference_chat_scope(
            ScopeErrorCode::OperationOutcomeUnknown,
            format!("the input transaction did not finish; its outcome is unknown: {error}"),
        )),
    }
}

fn request_id_or_new(request_id: Option<String>) -> String {
    request_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("agent-control-{}", uuid::Uuid::new_v4()))
}

/// The agent the per-agent detectors and answer planners run for.
fn registry_agent(observed: &Observed) -> Option<String> {
    observed.entry.get("agent").and_then(serde_json::Value::as_str).map(str::to_string)
}

fn replay_value(replay: events::AgentReplay) -> serde_json::Value {
    match replay {
        events::AgentReplay::Events {
            events,
            after_sequence,
        } => serde_json::json!({
            "gap": false,
            "events": events.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
        events::AgentReplay::Gap {
            snapshot,
            after_sequence,
        } => serde_json::json!({
            "gap": true,
            "snapshot": snapshot.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
    }
}

/// A wait's reply. `instance` is the observer epoch its sequences belong to: a caller holding a
/// sequence of another epoch (this daemon restarted) cannot order the two.
fn wait_value(hit: WaitHit, instance: &str) -> serde_json::Value {
    let mut value = match hit {
        WaitHit::Observation(observation) => serde_json::json!({
            "gap": false,
            "observation": observation.sequenced(),
            "afterSequence": observation.sequence,
        }),
        WaitHit::Gap {
            snapshot,
            after_sequence,
        } => serde_json::json!({
            "gap": true,
            "snapshot": snapshot.iter().map(|event| event.sequenced()).collect::<Vec<_>>(),
            "afterSequence": after_sequence,
        }),
    };
    value["instance"] = serde_json::json!(instance);
    value
}

/// Run one operation for the machine owner. The owner socket stays authorized.
pub(crate) async fn handle(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
) -> Result<serde_json::Value, ScopeError> {
    handle_as(state, op, "owner", None).await
}

/// The daemon-socket reply of a refused supervisor fence. Nothing ran.
pub(crate) fn fence_response(
    refusal: &crate::agent_control::fence::FenceRefusal,
) -> crate::daemon::protocol::DaemonResponse {
    crate::daemon::protocol::DaemonResponse::Error {
        message: refusal.message(),
        code: Some(refusal.code().to_string()),
        details: None,
    }
}

/// Ratchet a supervisor request's fence before it executes on this daemon. A request without
/// a fence (manual, user, an older hub) is never fenced. A refusal is audited; nothing runs.
async fn admit_fence(
    state: &Arc<RemoteGatewayState>,
    op: &AgentControlOp,
    actor: &str,
    fence: Option<&crate::agent_control::fence::SupervisorFence>,
) -> Result<(), crate::agent_control::fence::FenceRefusal> {
    let Some(fence) = fence else {
        return Ok(());
    };
    let admitted = state.supervisor_fence.admit(fence).await;
    if let Err(refusal) = &admitted {
        record_audit(state, AuditRecord::for_op(actor, op, refusal.code()));
    }
    admitted
}

/// One owner-socket operation that may carry a supervisor fence: a stale fence is refused
/// with `STALE_SUPERVISOR_FENCE` and the operation never runs; a higher one is stored first.
pub(crate) async fn handle_fenced(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    fence: Option<crate::agent_control::fence::SupervisorFence>,
) -> crate::daemon::protocol::DaemonResponse {
    if let Err(refusal) = admit_fence(state, &op, "owner", fence.as_ref()).await {
        return fence_response(&refusal);
    }
    match handle(state, op).await {
        Ok(result) => crate::daemon::protocol::DaemonResponse::AgentControlOk { result },
        Err(error) => error_response(error),
    }
}

/// Run one operation as `actor`, re-checking `authorize` inside every input transaction.
///
/// Every call, including a refusal, is appended to the agent-control audit log with the device id.
pub(crate) async fn handle_as(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    actor: &str,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<serde_json::Value, ScopeError> {
    // An answer the owner types (a card tap, the phone, Slack, the terminal helper) joins the
    // answer history once it went through; the supervisor's own answers never do.
    if let Some(handled) = recovery_card_answer(&op).await {
        return handled;
    }
    let owner_answer = crate::hub_runtime::panel_autonomy::owner_answer_of(&op);
    let result = dispatch(state, op.clone(), authorize).await;
    if result.is_ok() && op.is_input_mutation() {
        crate::hub_runtime::note_agent_control_input(&op);
    }
    if let (Ok(_), Some(answer)) = (&result, owner_answer) {
        tokio::spawn(answer.recorded());
    }
    record_audit(
        state,
        AuditRecord::for_op(
            actor,
            &op,
            match &result {
                Ok(_) => "OK".to_string(),
                Err(error) => reference_chat_wire_code(error.code).to_string(),
            },
        ),
    );
    result
}

/// An owner's tap on a recovery card, answered by the recovery scheduler instead of the pane.
/// `None` for every other op (including answers to real pane prompts).
pub(crate) async fn recovery_card_answer(op: &AgentControlOp) -> Option<Result<serde_json::Value, ScopeError>> {
    let AgentControlOp::Answer { prompt_id, answer, .. } = op else {
        return None;
    };
    let result = crate::hub_runtime::panel_autonomy::answer_recovery_card(prompt_id, answer.option_index).await?;
    Some(result.map(|()| serde_json::json!({ "recovery": "answered" })).map_err(|error| {
        let code = match &error {
            ferryx_hub::supervisor::recovery::CardError::NotOpen => ScopeErrorCode::NotFound,
            ferryx_hub::supervisor::recovery::CardError::BadOption => ScopeErrorCode::InvalidRequest,
            ferryx_hub::supervisor::recovery::CardError::Failed(_) => ScopeErrorCode::ControlConflict,
        };
        reference_chat_scope(code, error.to_string())
    }))
}

/// Desktop event the Supervisor view refetches on: a supervisor typed, or was refused.
pub(crate) const SUPERVISOR_CHANGED_EVENT: &str = "supervisor_changed";

/// Machine-events type a paired phone refetches the supervisor snapshot on.
pub(crate) const SUPERVISOR_CHANGED_MACHINE_EVENT: &str = "supervisorChanged";

/// Tell every Supervisor view to refetch: the desktop and mirror through their desktop event,
/// machine-scope sockets through their own event domain. `payload` names what changed
/// (`{sessionId}` or `{laneKey}`).
pub(crate) fn publish_supervisor_changed(state: &RemoteGatewayState, payload: serde_json::Value) {
    if let Some(services) = &state.machine_services {
        services.workspaces.machine_events.publish(
            SUPERVISOR_CHANGED_MACHINE_EVENT,
            None,
            None,
            payload.clone(),
        );
    }
    state.emit_desktop_event(SUPERVISOR_CHANGED_EVENT, payload);
}

/// Append one audit line; an input line also tells the Supervisor views to refresh.
fn record_audit(state: &Arc<RemoteGatewayState>, record: AuditRecord) {
    let input = record.is_input();
    let session_id = record.session_id.clone();
    state.agent_control_audit.record(record);
    if input {
        publish_supervisor_changed(state, serde_json::json!({ "sessionId": session_id }));
    }
}

async fn dispatch(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<serde_json::Value, ScopeError> {
    match op {
        AgentControlOp::List => {
            let ids = state.session_backend.list_sessions().await;
            let entries: Vec<serde_json::Value> = stream::iter(ids)
                .map(|id| async move {
                    match observe_with_line(state, &id).await {
                        Ok(observed) => observed.entry,
                        // A session that vanished between the list and its read is reported, not dropped.
                        Err(error) => serde_json::json!({
                            "sessionId": id,
                            "running": false,
                            "tier": AgentTier::Screen,
                            "state": AgentControlState::Unknown,
                            "stateSource": AgentStateSource::None,
                            "error": error.message,
                        }),
                    }
                })
                .buffered(LIST_CONCURRENCY)
                .collect()
                .await;
            Ok(serde_json::json!({ "sessions": entries }))
        }
        AgentControlOp::Read { session_id } => {
            live(state, &session_id).await?;
            let observed = observe_with_line(state, &session_id).await?;
            let mut entry = observed.entry;
            entry["screen"] = match observed.screen {
                Some(snapshot) => serde_json::json!({
                    "text": snapshot.text,
                    "revision": snapshot.revision,
                    "gap": snapshot.gap,
                    "truncated": snapshot.truncated,
                }),
                None => serde_json::Value::Null,
            };
            Ok(serde_json::json!({ "session": entry }))
        }
        AgentControlOp::Send { session_id, text, request_id } => {
            send_text(state, session_id, text, request_id, authorize, None).await
        }
        AgentControlOp::Answer { session_id, prompt_id, screen_revision, answer, request_id } => {
            live(state, &session_id).await?;
            refuse_if_human_typing(&session_id)?;
            let observed = observe(state, &session_id).await?;
            let Some(agent) = registry_agent(&observed) else {
                return Err(reference_chat_scope(
                    ScopeErrorCode::Unsupported,
                    "no prompt reader knows the agent in this pane, so a prompt cannot be answered",
                ));
            };
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let authorize = mutation_authorize(session_id.clone(), authorize);
            let state = Arc::clone(state);
            let transaction = tokio::spawn(async move {
                let identity = reference_chat_identity(&state, &target, Some(&agent)).await;
                let prompt_session = reference_chat_prompt_session(&state, &identity).await;
                let payload = reference_types::ReferencePromptAnswerPayload {
                    prompt_id,
                    screen_revision,
                    answer,
                };
                let alive = || true;
                let request = reference_prompts::ReferenceAnswerRequest {
                    target: &target,
                    request_id: &request_id,
                    payload: &payload,
                    session: prompt_session.borrow(),
                    agent: agent.as_str(),
                    authorize: &*authorize,
                    alive: &alive,
                };
                let runtime = reference_chat_runtime();
                let step = reference_prompts::ReferenceQueueStep::new(&runtime.input);
                let screen = ReferenceGatewayScreenReader { backend: Arc::clone(&state.session_backend) };
                let writer = reference_input::ReferenceSessionWriter::new(
                    state.session_backend.as_ref(),
                    &state.agent_events,
                    &request_id,
                );
                let clock = reference_input::SystemReferenceClock;
                runtime.answers.answer(&request, &step, &screen, &writer, &clock).await
            });
            joined(transaction.await)
        }
        AgentControlOp::Stop { session_id, request_id } => {
            live(state, &session_id).await?;
            refuse_if_human_typing(&session_id)?;
            let target = control_target(state, &session_id);
            let request_id = request_id_or_new(request_id);
            let payload = reference_types::ReferenceStopPayload {
                capability: reference_types::ReferenceStopCapability::ProviderInterrupt,
            };
            let authorize = mutation_authorize(session_id.clone(), authorize);
            let alive = || true;
            let stop = reference_input::ReferenceStopRequest {
                target: &target,
                request_id: &request_id,
                payload: &payload,
                authorize: &*authorize,
                alive: &alive,
            };
            let writer = reference_input::ReferenceSessionWriter::new(
                state.session_backend.as_ref(),
                &state.agent_events,
                &request_id,
            );
            outcome_result(reference_chat_runtime().input.stop(&stop, &writer).await)
        }
        AgentControlOp::Deliver { session_id, text, request_id, observe_ms } => {
            if request_id.trim().is_empty() {
                return Err(reference_chat_scope(
                    ScopeErrorCode::InvalidRequest,
                    "a delivery needs its request id",
                ));
            }
            let deadline = crate::agent_control::delivery::observe_deadline(observe_ms);
            let report = crate::agent_control::delivery::deliver(
                state, &session_id, text, request_id, deadline, authorize,
            )
            .await?;
            Ok(serde_json::json!({ "delivery": report }))
        }
        AgentControlOp::Wait {
            session_id,
            until,
            after_revision,
            after,
            timeout_ms,
        } => {
            if !events::until_is_known(&until) {
                return Err(reference_chat_scope(
                    ScopeErrorCode::InvalidRequest,
                    "until must be working, blocked, idle, prompt or any",
                ));
            }
            let timeout_ms = policy::clamp_wait_timeout(timeout_ms)?;
            live(state, &session_id).await?;
            let hit = events::wait_for(
                &state.agent_events,
                &session_id,
                &until,
                after,
                after_revision.as_deref(),
                std::time::Duration::from_millis(timeout_ms),
            )
            .await;
            match hit {
                Some(hit) => Ok(wait_value(hit, state.agent_events.instance())),
                None => Err(reference_chat_scope(
                    ScopeErrorCode::Timeout,
                    "the pane did not reach that state before the timeout",
                )),
            }
        }
        AgentControlOp::Events { after } => Ok(replay_value(state.agent_events.replay(after))),
    }
}

/// Type `text` into `session_id` as one submit: the `Send` operation, and the typing step of a
/// delivery. Its writer marks the pane's input ledger at each write, under the target's step.
///
/// `ready`, given by a delivery, is asked under the step after the blocked-prompt screen read,
/// right before the first byte: when the pane is no longer observed ready the submit is refused
/// untyped (`CONTROL_CONFLICT`), so neither a queue wait nor the screen read can make a
/// delivery type into a pane that became busy, or took other input, meanwhile.
pub(crate) async fn send_text(
    state: &Arc<RemoteGatewayState>,
    session_id: String,
    text: String,
    request_id: Option<String>,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
    ready: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
) -> Result<serde_json::Value, ScopeError> {
    if text.trim().is_empty() || text.contains('\0') {
        return Err(reference_chat_scope(ScopeErrorCode::InvalidRequest, "text must not be empty or contain NUL"));
    }
    live(state, &session_id).await?;
    refuse_if_human_typing(&session_id)?;
    let hub = Arc::clone(state.terminal_service.output_hub());
    // A pane this hub does not hold has a paste mode this host cannot read; a multi-line
    // message would submit its first line on its own, so it is refused untyped.
    if hub.session_sequence_range(&session_id).is_none()
        && reference_input::reference_composer_body(&text).contains('\r')
    {
        return Err(reference_chat_scope(
            ScopeErrorCode::Unsupported,
            "this pane's paste mode cannot be read on this host, so a multi-line message cannot be typed as one turn; nothing was typed",
        ));
    }
    let observed = observe(state, &session_id).await?;
    let agent = registry_agent(&observed);
    let target = control_target(state, &session_id);
    let request_id = request_id_or_new(request_id);
    let authorize = mutation_authorize(session_id.clone(), authorize);
    let state = Arc::clone(state);
    let transaction = tokio::spawn(async move {
        let payload = reference_types::ReferenceSubmitPayload {
            text,
            attachment_ids: Vec::new(),
            origin: reference_types::ReferenceSubmitOrigin::Chat,
        };
        let paste_session = session_id.clone();
        let clock = reference_input::SystemReferenceClock;
        let submit = reference_input::ReferenceSubmitRequest {
            target: &target,
            request_id: &request_id,
            payload: &payload,
            bracketed_paste: &move || hub.is_bracketed_paste_enabled(&paste_session),
            blocked_prompt: None,
            arrived_at_ms: clock.now_ms(),
            last_typed_at_ms: None,
            authorize: &*authorize,
        };
        let probe = || async {
            let seen = reference_chat_blocked_prompt(&state, &target, agent.as_deref()).await;
            if !matches!(seen, reference_input::ReferencePromptProbe::Clear) {
                return seen;
            }
            // Asked after the awaited screen read, so a reading or an input that arrived during
            // it is still seen before the first byte.
            if ready.as_ref().is_some_and(|ready| !ready()) {
                return reference_input::ReferencePromptProbe::Blocked(
                    "the pane is no longer observed ready".to_string(),
                );
            }
            seen
        };
        let writer = reference_input::ReferenceSessionWriter::new(
            state.session_backend.as_ref(),
            &state.agent_events,
            &request_id,
        );
        reference_chat_runtime().input.submit_with_probe(&submit, probe, &writer, &clock).await
    });
    joined(transaction.await)
}

/// The pane's agent for a prompt read or answer: the one the caller names, else the one agent
/// control identifies from the provider report or the screen.
async fn prompt_agent(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    registry_id: Option<String>,
) -> Result<Option<String>, ScopeError> {
    if let Some(named) = registry_id.map(|id| id.trim().to_string()).filter(|id| !id.is_empty()) {
        return Ok(Some(named));
    }
    Ok(registry_agent(&observe(state, session_id).await?))
}

/// The desktop's view of one detected card. The "type your own answer" row is not an option:
/// it is answered with free text. A multi-select card has no view: the shared answer path
/// refuses it, and the phone drops it too, so both Inboxes leave it to the pane.
fn prompt_view(
    prompt: &reference_types::ReferencePrompt,
    screen_revision: String,
) -> Option<crate::daemon::protocol::AgentPromptView> {
    use crate::daemon::protocol::{AgentPromptView, AgentPromptViewKind, AgentPromptViewOption};
    if prompt.multi_select {
        return None;
    }
    let options: Vec<AgentPromptViewOption> = prompt
        .options
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index as u32) != prompt.custom_option_index)
        .map(|(index, option)| AgentPromptViewOption {
            key: index.to_string(),
            label: option.label.clone(),
        })
        .collect();
    let kind = match prompt.kind {
        reference_types::ReferencePromptKind::Approval | reference_types::ReferencePromptKind::Plan => {
            AgentPromptViewKind::Approval
        }
        _ if options.is_empty() => AgentPromptViewKind::FreeText,
        _ => AgentPromptViewKind::Choice,
    };
    let question = if prompt.question.trim().is_empty() { prompt.title.clone() } else { prompt.question.clone() };
    // The remote chat's rule, for an option pick: the desktop asks the same confirm it does.
    let needs_confirmation = prompt.needs_confirmation(&reference_types::ReferencePromptAnswer {
        option_index: Some(0),
        ..Default::default()
    });
    Some(AgentPromptView {
        prompt_id: prompt.id.clone(),
        screen_revision,
        question,
        options,
        kind,
        needs_confirmation,
    })
}

/// `AgentPromptGet`: the card the pane's screen holds now, through the same detectors the
/// reference-chat prompt route runs. A screen that cannot be read whole is refused rather than
/// reported as holding no prompt.
pub(crate) async fn prompt_get(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    registry_id: Option<String>,
) -> Result<Option<crate::daemon::protocol::AgentPromptView>, ScopeError> {
    live(state, session_id).await?;
    let agent = prompt_agent(state, session_id, registry_id).await?;
    let target = control_target(state, session_id);
    let snapshot = reference_chat_snapshot_screen(state, &target).await?;
    if !snapshot.is_answerable() {
        return Err(reference_chat_scope(
            ScopeErrorCode::RequestConflict,
            "the pane's screen could not be reconstructed, so no card may be answered from it",
        ));
    }
    let Some(agent) = agent.filter(|agent| reference_prompts::ReferencePromptFamily::of_agent(agent).is_some())
    else {
        return Ok(None);
    };
    let identity = reference_chat_identity(state, &target, Some(&agent)).await;
    let session = reference_chat_prompt_session_for_screen(state, &identity, &agent, &snapshot.text).await;
    let prompt = reference_prompts::detect_reference_prompt_in_session(&agent, &snapshot.text, &session.borrow());
    let prompt = reference_prompts::reference_prompt_occurrence(session_id, prompt, &snapshot.revision);
    Ok(prompt.and_then(|prompt| prompt_view(&prompt, snapshot.revision)))
}

/// The reference answer an `AgentPromptAnswerInput` names, after the daemon boundary's length
/// cap on free text. Both or neither shape set is passed
/// on as such, so the shared answer path refuses it exactly as it refuses the phone's.
fn reference_answer(
    input: crate::daemon::protocol::AgentPromptAnswerInput,
) -> Result<reference_types::ReferencePromptAnswer, ScopeError> {
    // The same bound an HTTP agent-control body has: longer text is refused before anything is
    // read or typed.
    if input.text.as_ref().is_some_and(|text| text.len() > policy::AGENT_CONTROL_HTTP_BODY_MAX) {
        return Err(reference_chat_scope(
            ScopeErrorCode::InvalidRequest,
            "the answer text is longer than an answer may carry; nothing was typed",
        ));
    }
    let option_index = match input.key {
        Some(key) => Some(key.trim().parse::<u32>().map_err(|_| {
            reference_chat_scope(ScopeErrorCode::InvalidRequest, "the answer key is not an option of this prompt")
        })?),
        None => None,
    };
    Ok(reference_types::ReferencePromptAnswer {
        option_index,
        option_indices: None,
        custom_text: input.text,
    })
}

/// `AgentPromptAnswer`: the person at the desktop answers the card they were shown. The answer
/// runs through the shared answer path inside the pane's one input queue, so a changed screen,
/// a replaced card or a replayed answer is refused with nothing typed, as on the phone.
pub(crate) async fn prompt_answer(
    state: &Arc<RemoteGatewayState>,
    session_id: String,
    prompt_id: String,
    screen_revision: String,
    answer: crate::daemon::protocol::AgentPromptAnswerInput,
    registry_id: Option<String>,
) -> Result<crate::scoped_contracts::DeliveryReceipt, ScopeError> {
    let owner_answer =
        crate::hub_runtime::panel_autonomy::owner_prompt_answer_of(&session_id, &prompt_id, answer.key.as_deref());
    let answer = reference_answer(answer)?;
    live(state, &session_id).await?;
    let Some(agent) = prompt_agent(state, &session_id, registry_id).await? else {
        return Err(reference_chat_scope(
            ScopeErrorCode::Unsupported,
            "no prompt reader knows the agent in this pane, so a prompt cannot be answered",
        ));
    };
    let target = control_target(state, &session_id);
    let request_id = format!("agent-prompt-{}", uuid::Uuid::new_v4());
    let state = Arc::clone(state);
    // Owned by its own task, like every answer: a caller that disconnects must not leave a menu
    // moved but unconfirmed.
    let transaction = tokio::spawn(async move {
        let identity = reference_chat_identity(&state, &target, Some(&agent)).await;
        let prompt_session = reference_chat_prompt_session(&state, &identity).await;
        let payload = reference_types::ReferencePromptAnswerPayload { prompt_id, screen_revision, answer };
        // The owner socket's uid check on connect is the authorization.
        let authorize = owner_authorized();
        let alive = || true;
        let request = reference_prompts::ReferenceAnswerRequest {
            target: &target,
            request_id: &request_id,
            payload: &payload,
            session: prompt_session.borrow(),
            agent: agent.as_str(),
            authorize: &*authorize,
            alive: &alive,
        };
        let runtime = reference_chat_runtime();
        let step = reference_prompts::ReferenceQueueStep::new(&runtime.input);
        let screen = ReferenceGatewayScreenReader { backend: Arc::clone(&state.session_backend) };
        let writer = reference_input::ReferenceSessionWriter::new(
            state.session_backend.as_ref(),
            &state.agent_events,
            &request_id,
        );
        let clock = reference_input::SystemReferenceClock;
        runtime.answers.answer(&request, &step, &screen, &writer, &clock).await
    });
    match transaction.await {
        Ok(reference_input::ReferenceInputOutcome::Accepted { receipt }) => {
            if let Some(answer) = owner_answer {
                tokio::spawn(answer.recorded());
            }
            Ok(receipt)
        }
        Ok(reference_input::ReferenceInputOutcome::NotTyped { error }) => Err(error),
        Ok(reference_input::ReferenceInputOutcome::OutcomeUnknown { message, .. }) => {
            Err(reference_chat_scope(ScopeErrorCode::OperationOutcomeUnknown, message))
        }
        Err(error) => Err(reference_chat_scope(
            ScopeErrorCode::OperationOutcomeUnknown,
            format!("the input transaction did not finish; its outcome is unknown: {error}"),
        )),
    }
}

struct ForwardFailure {
    code: String,
    message: String,
}

/// Overwrite a forwarded receipt's host id. List, read, wait, and events have no receipt.
fn stamp_remote_receipt_host(value: &mut serde_json::Value, host_id: &str) {
    if let Some(host) = value.pointer_mut("/receipt/target/hostId") {
        *host = serde_json::Value::String(host_id.to_string());
    }
}

fn forward_code(code: &str) -> String {
    match code {
        "PAIRED_HOST_CAPABILITY_UNAVAILABLE" => "UNSUPPORTED".into(),
        "SESSION_NOT_FOUND" | "SESSION_EXPIRED" => "NOT_FOUND".into(),
        other => other.to_string(),
    }
}

/// Forward one operation to a paired host. Executes once. An ambiguous transport error is
/// `OPERATION_OUTCOME_UNKNOWN` and is not issued again, even when the caller omitted a request id.
pub(crate) async fn forward_daemon(
    state: &Arc<RemoteGatewayState>,
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: String,
    op: AgentControlOp,
    fence: Option<crate::agent_control::fence::SupervisorFence>,
) -> crate::daemon::protocol::DaemonResponse {
    let failure = match forward_once(hosts, &host_id, op.clone(), fence).await {
        Ok(result) => {
            record_audit(state, AuditRecord::for_op("owner", &op, "OK"));
            return crate::daemon::protocol::DaemonResponse::AgentControlOk { result };
        }
        Err(failure) => failure,
    };
    record_audit(state, AuditRecord::for_op("owner", &op, failure.code.clone()));
    crate::daemon::protocol::DaemonResponse::Error {
        message: failure.message,
        code: Some(failure.code),
        details: None,
    }
}

async fn forward_once(
    hosts: &crate::paired_host::service::PairedHostService,
    host_id: &str,
    op: AgentControlOp,
    fence: Option<crate::agent_control::fence::SupervisorFence>,
) -> Result<serde_json::Value, ForwardFailure> {
    // A delivery to a host without causal receipts keeps its own code, so the supervisor holds
    // it as a missing capability instead of an untyped refusal.
    let delivery = matches!(op, AgentControlOp::Deliver { .. });
    let known = hosts.list().await.map_err(|error| ForwardFailure {
        // Nothing was sent yet, so this never claims the op may have been applied.
        code: "INVENTORY_INCOMPLETE".into(),
        message: format!("the paired-host inventory could not be read: {} ({})", error.code, error.message),
    })?;
    let Some(host) = known.into_iter().find(|host| host.host_id == host_id) else {
        return Err(ForwardFailure {
            code: "NOT_FOUND".into(),
            message: format!("no paired host is named {host_id}"),
        });
    };
    let request = crate::paired_host::client::OperationRequest {
        host_id: host.host_id,
        generation: host.generation,
        operation: crate::paired_host::client::Operation::AgentControl { op, fence },
    };
    // One round trip. MachineClient does not mint a new request id, and this function does not
    // call it again when the answer is ambiguous.
    match crate::paired_host::client::MachineClient::new()
        .execute(hosts, request)
        .await
    {
        Ok(response) => match response.result {
            crate::paired_host::client::OperationResult::AgentControl(mut value) => {
                // The remote daemon stamps its own reference host, which defaults to "local".
                // The receipt the caller sees names the paired host this forward was sent to.
                stamp_remote_receipt_host(&mut value, host_id);
                Ok(value)
            }
            _ => Err(ForwardFailure {
                code: "INTERNAL".into(),
                message: "the paired host answered a different operation".into(),
            }),
        },
        Err(error) if error.ambiguous || error.code == "OPERATION_OUTCOME_UNKNOWN" => {
            Err(ForwardFailure {
                code: "OPERATION_OUTCOME_UNKNOWN".into(),
                message: "the paired host may have applied the operation; it was not sent again"
                    .into(),
            })
        }
        Err(error) => {
            let code = error
                .machine_error
                .as_ref()
                .map(|machine| machine.code.as_str())
                .unwrap_or(error.code.as_str());
            Err(ForwardFailure {
                code: if delivery && code == "PAIRED_HOST_CAPABILITY_UNAVAILABLE" {
                    code.to_string()
                } else {
                    forward_code(code)
                },
                message: error
                    .machine_error
                    .as_ref()
                    .map(|machine| machine.message.clone())
                    .unwrap_or_else(|| {
                        format!("the paired host did not accept agent control ({})", error.code)
                    }),
            })
        }
    }
}

fn control_json(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(value),
    )
        .into_response()
}

fn control_finish(result: Result<serde_json::Value, ScopeError>) -> Response {
    match result {
        Ok(value) => control_json(value),
        Err(error) => reference_chat_refusal(&error),
    }
}

/// The machine error a refused supervisor fence answers with on the paired route.
fn fence_refusal(refusal: &crate::agent_control::fence::FenceRefusal) -> Response {
    let status = match refusal {
        crate::agent_control::fence::FenceRefusal::Invalid(_) => StatusCode::BAD_REQUEST,
        crate::agent_control::fence::FenceRefusal::NotPersisted(_)
        | crate::agent_control::fence::FenceRefusal::Unreadable(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::CONFLICT,
    };
    machine_error_with_details(status, refusal.code(), &refusal.message(), serde_json::Map::new())
}

/// A paired send, answer or stop: a supervisor fence in the body is ratcheted before the
/// operation runs; a body without one is a person's or an older hub's and is never fenced.
async fn control_fenced(
    state: &Arc<RemoteGatewayState>,
    op: AgentControlOp,
    actor: &str,
    authorize: Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>,
    fence: Option<u64>,
    account: Option<String>,
) -> Response {
    let fence = match crate::agent_control::fence::SupervisorFence::from_wire(fence, account) {
        Ok(fence) => fence,
        Err(refusal) => return fence_refusal(&refusal),
    };
    if let Err(refusal) = admit_fence(state, &op, actor, fence.as_ref()).await {
        return fence_refusal(&refusal);
    }
    control_finish(handle_as(state, op, actor, Some(authorize)).await)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct FenceBody {
    account: String,
    fence: u64,
}

/// `POST /api/v1/supervisor/fence`: a new lease holder raises this machine's fence for its
/// account before its first dispatch, so the previous holder's lower fence is refused here
/// from now on. A lower fence than the stored one is refused `STALE_SUPERVISOR_FENCE`.
pub(crate) async fn http_supervisor_fence(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<FenceBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_machine_peer(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return *response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the fence body is not readable",
            )
        }
    };
    let fence = match crate::agent_control::fence::SupervisorFence::from_wire(
        Some(body.fence),
        Some(body.account),
    ) {
        Ok(Some(fence)) => fence,
        Ok(None) => {
            return reference_chat_error(ScopeErrorCode::InvalidRequest, "the fence is missing")
        }
        Err(refusal) => return fence_refusal(&refusal),
    };
    match state.supervisor_fence.admit_push(&fence).await {
        Ok(()) => {
            tracing::info!(device = %device.id, fence = fence.fence, "a supervisor lease holder raised this machine's fence");
            control_json(serde_json::json!({
                "account": fence.account,
                "fence": state.supervisor_fence.max_for(&fence.account).await,
            }))
        }
        Err(refusal) => fence_refusal(&refusal),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn http_device(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<DeviceInfo, Response> {
    let token = extract_token(headers)
        .ok_or_else(|| reference_chat_error(ScopeErrorCode::Unauthorized, "missing auth token"))?;
    let device = state.auth_manager.validate_token(&token).map_err(|_| {
        reference_chat_error(ScopeErrorCode::Unauthorized, "invalid or revoked token")
    })?;
    authorize_agent_control(
        &device,
        mutation,
        state.agent_control_policy.permits(&device.id),
    )
    .map_err(|error| reference_chat_refusal(&error))?;
    Ok(device)
}

/// [`http_device`] for the same-account machine-to-machine supervisor routes only (checkpoint,
/// release-to, observe, forwarded supervisor requests and fence pushes). A device the account
/// service vouched is an enrolled machine of this account ([`DeviceInfo::account_machine_id`],
/// set from the sealed grant offer) is not governed by the per-machine agent-control switch
/// or its allowlist; scope, permission and an explicit exclusion still apply. Every other
/// device, a phone or the web app included, follows [`http_device`] exactly. The refusal is
/// boxed: a `Response` is large, and callers return it as `*response`.
pub(crate) fn http_machine_peer(
    state: &RemoteGatewayState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<DeviceInfo, Box<Response>> {
    let token = extract_token(headers).ok_or_else(|| {
        Box::new(reference_chat_error(ScopeErrorCode::Unauthorized, "missing auth token"))
    })?;
    let device = state.auth_manager.validate_token(&token).map_err(|_| {
        Box::new(reference_chat_error(ScopeErrorCode::Unauthorized, "invalid or revoked token"))
    })?;
    let allowed = if device.account_machine_id.is_some() {
        !state.agent_control_policy.excludes(&device.id)
    } else {
        state.agent_control_policy.permits(&device.id)
    };
    // A grant without the account's same-machine mark refused only by the switch (or its
    // allowlist) is marked with a fixed detail a standby keys its grant repair on, so neither
    // the supervisor's own FORBIDDEN outcomes nor a permission refusal triggers it. An explicit
    // exclusion is never marked: a fresh grant would be a new, unexcluded device.
    let repairable = device.account_machine_id.is_none()
        && !state.agent_control_policy.excludes(&device.id)
        && authorize_agent_control(&device, mutation, true).is_ok();
    authorize_agent_control(&device, mutation, allowed).map_err(|error| {
        if repairable && !allowed {
            let mut details = serde_json::Map::new();
            details.insert(
                crate::agent_control::policy::REFUSAL_DETAIL.into(),
                crate::agent_control::policy::AGENT_CONTROL_REFUSAL.into(),
            );
            Box::new(machine_error_with_details(StatusCode::FORBIDDEN, "FORBIDDEN", &error.message, details))
        } else {
            Box::new(reference_chat_refusal(&error))
        }
    })?;
    Ok(device)
}

fn http_reauthorize(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
) -> Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
    let token = extract_token(headers);
    let auth = Arc::clone(&state.auth_manager);
    let policy = Arc::clone(&state.agent_control_policy);
    Arc::new(move || {
        let token = token.clone().ok_or_else(|| {
            reference_chat_scope(ScopeErrorCode::Unauthorized, "the caller's token is gone")
        })?;
        let device = auth.validate_token(&token).map_err(|_| {
            reference_chat_scope(
                ScopeErrorCode::Unauthorized,
                "the caller's device is no longer authorized",
            )
        })?;
        authorize_agent_control(&device, true, policy.permits(&device.id))
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsQuery {
    #[serde(default)]
    after: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct WaitQuery {
    until: String,
    #[serde(default)]
    after: u64,
    timeout_ms: u64,
    #[serde(default)]
    revision: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SendBody {
    text: String,
    #[serde(default)]
    request_id: Option<String>,
    /// Present on a delivery: send, then wait up to this long for this request's turn start.
    #[serde(default)]
    observe_ms: Option<u64>,
    /// A supervisor's lease fence and its account; absent on a person's send.
    #[serde(default)]
    supervisor_fence: Option<u64>,
    #[serde(default)]
    supervisor_account: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnswerBody {
    prompt_id: String,
    screen_revision: String,
    answer: crate::remote::reference_chat::types::ReferencePromptAnswer,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    supervisor_fence: Option<u64>,
    #[serde(default)]
    supervisor_account: Option<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopBody {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    supervisor_fence: Option<u64>,
    #[serde(default)]
    supervisor_account: Option<String>,
}

fn query_error(rejection: QueryRejection) -> Response {
    reference_chat_error(
        ScopeErrorCode::InvalidRequest,
        format!("the query is not readable: {rejection}"),
    )
}

pub(crate) async fn http_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    control_finish(handle(&state, AgentControlOp::List).await)
}

pub(crate) async fn http_session(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    control_finish(handle(&state, AgentControlOp::Read { session_id }).await)
}

pub(crate) async fn http_events(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return query_error(rejection),
    };
    control_finish(handle(&state, AgentControlOp::Events { after: query.after }).await)
}

pub(crate) async fn http_wait(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    query: Result<Query<WaitQuery>, QueryRejection>,
) -> Response {
    if let Err(response) = http_device(&state, &headers, false) {
        return response;
    }
    let query = match query {
        Ok(Query(query)) => query,
        Err(rejection) => return query_error(rejection),
    };
    control_finish(
        handle(
            &state,
            AgentControlOp::Wait {
                session_id,
                until: query.until,
                after_revision: query.revision,
                after: query.after,
                timeout_ms: query.timeout_ms,
            },
        )
        .await,
    )
}

pub(crate) async fn http_send(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<SendBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the send body is not readable",
            )
        }
    };
    let authorize = http_reauthorize(&state, &headers);
    let op = match (body.observe_ms, body.request_id) {
        (Some(observe_ms), Some(request_id)) => AgentControlOp::Deliver {
            session_id,
            text: body.text,
            request_id,
            observe_ms,
        },
        (Some(_), None) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "a delivery needs its request id",
            )
        }
        (None, request_id) => AgentControlOp::Send {
            session_id,
            text: body.text,
            request_id,
        },
    };
    control_fenced(
        &state,
        op,
        &device.id,
        authorize,
        body.supervisor_fence,
        body.supervisor_account,
    )
    .await
}

pub(crate) async fn http_answer(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<AnswerBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the answer body is not readable",
            )
        }
    };
    let authorize = http_reauthorize(&state, &headers);
    control_fenced(
        &state,
        AgentControlOp::Answer {
            session_id,
            prompt_id: body.prompt_id,
            screen_revision: body.screen_revision,
            answer: body.answer,
            request_id: body.request_id,
        },
        &device.id,
        authorize,
        body.supervisor_fence,
        body.supervisor_account,
    )
    .await
}

pub(crate) async fn http_stop(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    body: Result<Json<StopBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    stop_session(&state, &headers, &device, None, session_id, body).await
}

/// The agent-control `Stop` for one pane, as one authorized device. `host_id` is the paired
/// computer that owns the pane, or `None` for this one.
async fn stop_session(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    device: &DeviceInfo,
    host_id: Option<String>,
    session_id: String,
    body: Result<Json<StopBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => StopBody::default(),
    };
    let op = AgentControlOp::Stop {
        session_id,
        request_id: body.request_id,
    };
    control_ref_op(state, headers, device, host_id, op, body.supervisor_fence, body.supervisor_account).await
}

/// One op for one authorized device: in-process for a pane on this computer, else forwarded to
/// the paired computer that owns it through the same paired-host forward the daemon's
/// `AgentControl { host_id: Some(..) }` request uses. A supervisor fence in the body is
/// ratcheted here before a local op runs, and carried to the paired computer, which ratchets it
/// before its op runs; a body without one is a person's or an older hub's and is never fenced.
async fn control_ref_op(
    state: &Arc<RemoteGatewayState>,
    headers: &HeaderMap,
    device: &DeviceInfo,
    host_id: Option<String>,
    op: AgentControlOp,
    fence: Option<u64>,
    account: Option<String>,
) -> Response {
    let authorize = http_reauthorize(state, headers);
    match host_id {
        None => control_fenced(state, op, &device.id, authorize, fence, account).await,
        Some(host_id) => {
            let fence = match crate::agent_control::fence::SupervisorFence::from_wire(fence, account) {
                Ok(fence) => fence,
                Err(refusal) => return fence_refusal(&refusal),
            };
            control_finish(forward_as(state, host_id, op, &device.id, authorize, fence).await)
        }
    }
}

/// Forward one input op for a paired computer's pane, in-process, as one authorized device.
///
/// The device is re-authorized right before the forward and the call is audited once under its
/// id (the daemon's own owner-socket forward is not on this path, so there is no second line or
/// second `supervisor_changed`). The forward executes once: a lost answer is
/// `OPERATION_OUTCOME_UNKNOWN` and is never sent again. The request id is minted here when the
/// caller sent none. An owner answer joins this computer's answer history, looked up under the
/// paired host's session refs only.
async fn forward_as(
    state: &Arc<RemoteGatewayState>,
    host_id: String,
    mut op: AgentControlOp,
    actor: &str,
    authorize: Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>,
    fence: Option<crate::agent_control::fence::SupervisorFence>,
) -> Result<serde_json::Value, ScopeError> {
    match &mut op {
        AgentControlOp::Stop { request_id, .. } | AgentControlOp::Answer { request_id, .. } => {
            *request_id = Some(request_id_or_new(request_id.take()));
        }
        _ => {}
    }
    let owner_answer = crate::hub_runtime::panel_autonomy::forwarded_owner_answer_of(&host_id, &op);
    forward_with(state, host_id, op, actor, authorize, fence, owner_answer).await
}

/// The forward itself, with the owner answer to record already claimed (so it can be tested
/// against an autonomy service of its own instead of the process-wide one). The answer is
/// recorded only when the paired host accepted the op.
async fn forward_with(
    state: &Arc<RemoteGatewayState>,
    host_id: String,
    op: AgentControlOp,
    actor: &str,
    authorize: Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>,
    fence: Option<crate::agent_control::fence::SupervisorFence>,
    owner_answer: Option<crate::hub_runtime::panel_autonomy::PendingOwnerAnswer>,
) -> Result<serde_json::Value, ScopeError> {
    let result = match authorize() {
        Err(error) => Err(error),
        Ok(()) => match state.paired_hosts() {
            None => Err(reference_chat_scope(
                ScopeErrorCode::InventoryIncomplete,
                "this computer has no paired-host service",
            )),
            Some(hosts) => forward_once(&hosts, &host_id, op.clone(), fence)
                .await
                .map_err(|failure| reference_chat_scope(forward_scope_code(&failure.code), failure.message)),
        },
    };
    if let (Ok(_), Some(answer)) = (&result, owner_answer) {
        tokio::spawn(answer.recorded());
    }
    record_audit(
        state,
        AuditRecord::for_op(
            actor,
            &op,
            match &result {
                Ok(_) => "OK".to_string(),
                Err(error) => reference_chat_wire_code(error.code).to_string(),
            },
        ),
    );
    result
}

/// The phone-facing code for a failed forward. The paired computer's own refusals keep their
/// typed code (stale screen, provider owned, not found...). Its credential or reachability
/// failures are this computer's unavailable service, never a 401/403 about the phone's token.
/// Anything unrecognised after the request left may have been applied, so it is outcome-unknown.
fn forward_scope_code(code: &str) -> ScopeErrorCode {
    match code {
        "INVALID_REQUEST" => ScopeErrorCode::InvalidRequest,
        "NOT_FOUND" => ScopeErrorCode::NotFound,
        "TARGET_EXPIRED" => ScopeErrorCode::TargetExpired,
        "CONTROL_CONFLICT" => ScopeErrorCode::ControlConflict,
        "REQUEST_CONFLICT" => ScopeErrorCode::RequestConflict,
        "PROVIDER_OWNED" => ScopeErrorCode::ProviderOwned,
        "UNSUPPORTED" | "PAIRED_HOST_CAPABILITY_UNAVAILABLE" => ScopeErrorCode::Unsupported,
        "PAYLOAD_TOO_LARGE" => ScopeErrorCode::PayloadTooLarge,
        "CAPTURE_UNSUPPORTED" => ScopeErrorCode::CaptureUnsupported,
        "INVENTORY_INCOMPLETE"
        | "UNAUTHORIZED"
        | "FORBIDDEN"
        | "MACHINE_ACCESS_REQUIRED"
        | "REPAIR_REQUIRED"
        | "HOST_UNAVAILABLE"
        | "PERMISSION_DENIED"
        | "RATE_LIMITED"
        | "MACHINE_SERVICE_UNAVAILABLE"
        | "PAIRED_HOST_STALE_GENERATION"
        | "PAIRED_HOST_WRONG_MACHINE"
        | "PAIRED_HOST_REDIRECT_REJECTED" => ScopeErrorCode::InventoryIncomplete,
        _ => ScopeErrorCode::OperationOutcomeUnknown,
    }
}

/// A `<hostId>:<backendSessionId>` handle split for agent control: the paired host that owns the
/// pane (`None` when it is on this computer) and the backend session id. A handle for another
/// computer is never typed into a local pane that happens to share the backend id; it is
/// forwarded to that computer. `local_host_id` is this machine's configured id; the literal
/// `local` always names this machine.
pub(crate) fn agent_target(
    session_ref: &str,
    local_host_id: &str,
) -> Result<(Option<String>, String), ScopeError> {
    let parsed = ferryx_hub::supervisor::panel_contract::SessionRef::parse(session_ref)
        .map_err(|error| reference_chat_scope(ScopeErrorCode::InvalidRequest, error.to_string()))?;
    let host = parsed.host_id();
    let host_id = (host != ferryx_hub::agent_control::LOCAL_HOST_ID && host != local_host_id)
        .then(|| host.to_owned());
    Ok((host_id, parsed.backend_session_id().to_owned()))
}

/// `POST /api/v1/sessions/{sessionRef}/stop`: the provider's interrupt key, straight to agent
/// control. The hub and the engine are not on this path, so it works while either is down.
pub(crate) async fn http_session_ref_stop(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_ref): AxumPath<String>,
    body: Result<Json<StopBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let (host_id, session_id) = match agent_target(&session_ref, &reference_files::reference_host_id()) {
        Ok(target) => target,
        Err(error) => return reference_chat_refusal(&error),
    };
    stop_session(&state, &headers, &device, host_id, session_id, body).await
}

/// `POST /api/v1/supervisor/decisions/{sessionRef}/answer` body: the owner's tap on one option.
/// `screenRevision` is the stale guard; the screen is re-read before any key is typed.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct DecisionAnswerBody {
    prompt_id: String,
    screen_revision: String,
    option_index: u32,
    #[serde(default)]
    request_id: Option<String>,
}

/// The agent-control answer an owner's decision tap becomes: exactly one option, no gate.
pub(crate) fn decision_answer_op(session_id: String, body: DecisionAnswerBody) -> AgentControlOp {
    AgentControlOp::Answer {
        session_id,
        prompt_id: body.prompt_id,
        screen_revision: body.screen_revision,
        answer: reference_types::ReferencePromptAnswer {
            option_index: Some(body.option_index),
            ..Default::default()
        },
        request_id: body.request_id,
    }
}

pub(crate) async fn http_decision_answer(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    AxumPath(session_ref): AxumPath<String>,
    body: Result<Json<DecisionAnswerBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let device = match http_device(&state, &headers, true) {
        Ok(device) => device,
        Err(response) => return response,
    };
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => {
            return reference_chat_error(
                ScopeErrorCode::InvalidRequest,
                "the body must be {promptId, screenRevision, optionIndex}",
            )
        }
    };
    // A recovery card belongs to the scheduler, whichever computer its session is on, so it
    // is answered before the session ref is required to be local.
    if body.prompt_id.starts_with(ferryx_hub::supervisor::recovery::CARD_PREFIX) {
        let op = decision_answer_op(session_ref.clone(), body);
        let result = recovery_card_answer(&op).await.unwrap_or_else(|| {
            Err(reference_chat_scope(ScopeErrorCode::NotFound, "that recovery card is no longer open"))
        });
        record_audit(
            &state,
            AuditRecord::for_op(
                &device.id,
                &op,
                match &result {
                    Ok(_) => "OK".to_string(),
                    Err(error) => reference_chat_wire_code(error.code).to_string(),
                },
            ),
        );
        return control_finish(result);
    }
    let (host_id, session_id) = match agent_target(&session_ref, &reference_files::reference_host_id()) {
        Ok(target) => target,
        Err(error) => return reference_chat_refusal(&error),
    };
    control_ref_op(&state, &headers, &device, host_id, decision_answer_op(session_id, body), None, None).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::backend::RemoteSessionBackend as _;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;

    #[test]
    fn a_multi_select_card_has_no_desktop_view_and_a_single_choice_card_does() {
        let multi = reference_prompts::detect_reference_prompt(
            "claude",
            include_str!("reference_chat/fixtures/claude/prompt/question-tabs-multiselect.screen"),
        )
        .expect("the multi-select fixture is a card");
        assert!(multi.multi_select);
        assert!(prompt_view(&multi, "rev".into()).is_none());

        let single = reference_prompts::detect_reference_prompt(
            "claude",
            include_str!("reference_chat/fixtures/claude/prompt/question.screen"),
        )
        .expect("the question fixture is a card");
        assert!(!single.multi_select);
        assert!(prompt_view(&single, "rev".into()).is_some());
    }
    #[test]
    fn a_session_ref_names_this_computer_or_the_paired_host_that_owns_the_pane() {
        assert_eq!(agent_target("local:pane-1", "studio").unwrap(), (None, "pane-1".into()));
        assert_eq!(
            agent_target("studio:pane-1", "studio").unwrap(),
            (None, "pane-1".into()),
            "this machine's configured host id names it too"
        );
        assert_eq!(
            agent_target("omarchy:pane:1", "studio").unwrap(),
            (Some("omarchy".into()), "pane:1".into()),
            "another computer's pane is forwarded to that computer, never typed into a local pane"
        );
        for bad in ["nohost", ":pane-1", "studio:", ""] {
            assert_eq!(
                agent_target(bad, "studio").unwrap_err().code,
                ScopeErrorCode::InvalidRequest,
                "{bad}"
            );
        }
    }

    #[test]
    fn a_decision_tap_is_exactly_one_option_with_the_stale_guard() {
        let body: DecisionAnswerBody = serde_json::from_value(serde_json::json!({
            "promptId": "p1", "screenRevision": "rev-3", "optionIndex": 1
        }))
        .unwrap();
        let AgentControlOp::Answer { session_id, prompt_id, screen_revision, answer, request_id } =
            decision_answer_op("pane-1".into(), body)
        else {
            panic!("a decision tap is an answer");
        };
        assert_eq!((session_id.as_str(), prompt_id.as_str(), screen_revision.as_str()), ("pane-1", "p1", "rev-3"));
        assert_eq!(answer.option_index, Some(1));
        assert!(answer.is_single_choice());
        assert_eq!(request_id, None);
        for bad in [
            serde_json::json!({"promptId": "p", "optionIndex": 0}),
            serde_json::json!({"promptId": "p", "screenRevision": "r"}),
            serde_json::json!({"promptId": "p", "screenRevision": "r", "optionIndex": 0, "customText": "x"}),
        ] {
            assert!(serde_json::from_value::<DecisionAnswerBody>(bad.clone()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_forward_failure_never_reaches_the_phone_as_its_own_401_and_unknown_is_outcome_unknown() {
        for code in ["UNAUTHORIZED", "FORBIDDEN", "MACHINE_ACCESS_REQUIRED", "REPAIR_REQUIRED", "HOST_UNAVAILABLE"] {
            assert_eq!(forward_scope_code(code), ScopeErrorCode::InventoryIncomplete, "{code}");
        }
        assert_eq!(forward_scope_code("REQUEST_CONFLICT"), ScopeErrorCode::RequestConflict);
        assert_eq!(forward_scope_code("PROVIDER_OWNED"), ScopeErrorCode::ProviderOwned);
        assert_eq!(forward_scope_code("NOT_FOUND"), ScopeErrorCode::NotFound);
        assert_eq!(forward_scope_code("OPERATION_OUTCOME_UNKNOWN"), ScopeErrorCode::OperationOutcomeUnknown);
        assert_eq!(forward_scope_code("PAIRED_HOST_INVALID_RESPONSE"), ScopeErrorCode::OperationOutcomeUnknown);
    }

    #[tokio::test]
    async fn a_forward_is_re_authorized_audited_once_under_the_device_and_mints_an_id_even_when_refused() {
        let (_backend, state) = pane("agent-control-forward-auth", CLAUDE_APPROVAL);
        let refuse: Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync> =
            Arc::new(|| Err(reference_chat_scope(ScopeErrorCode::Unauthorized, "the caller's device is no longer authorized")));
        let before = state.agent_control_audit.recent().len();

        let error = forward_as(
            &state,
            "omarchy".into(),
            AgentControlOp::Stop { session_id: "pane-9".into(), request_id: None },
            "device-1",
            refuse,
            None,
        )
        .await
        .expect_err("the second authorization check refuses");

        assert_eq!(error.code, ScopeErrorCode::Unauthorized);
        let audit = state.agent_control_audit.recent();
        assert_eq!(audit.len(), before + 1, "exactly one audit line: {audit:?}");
        let line = audit.last().unwrap();
        assert_eq!((line.device_id.as_str(), line.op.as_str(), line.code.as_str()), ("device-1", "stop", "UNAUTHORIZED"));
        assert!(line.request_id.is_some(), "a stop with no request id gets one before it is forwarded");
    }

    /// A fake paired host that advertises agent control and serves `extra`, paired through the
    /// real service. The relay and the host are one axum server on loopback.
    async fn fake_paired_host(
        extra: axum::Router,
    ) -> (
        tempfile::TempDir,
        crate::paired_host::service::PairedHostService,
        crate::paired_host::inventory::HostView,
        tokio::task::JoinHandle<()>,
    ) {
        use axum::routing::{get, post};
        let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = axum::Router::new()
            .route(
                "/api/v1/pair/exchange",
                post(|| async {
                    axum::Json(serde_json::json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))
                }),
            )
            .route(
                "/host/a/api/v1/capabilities",
                get(|| async {
                    axum::Json(serde_json::json!({"apiVersion":1,"machineId":"a","daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":["directoryBrowseV1","machineWorkspaceV1","agentControlRemoteV1"],"limits":{"directoryEntries":1000,"terminalSessions":64}}))
                }),
            )
            .merge(extra);
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let service = crate::paired_host::service::PairedHostService::open_test_loopback(root.path().join("data"));
        let host = service
            .pair(crate::paired_host::service::PairRequest {
                relay_origin: origin,
                pin: crate::paired_host::service::Secret("fixture".into()),
                display_label: "fixture".into(),
            })
            .await
            .unwrap();
        (root, service, host, task)
    }

    async fn close_fake_host(root: tempfile::TempDir, task: tokio::task::JoinHandle<()>) {
        task.abort();
        let _ = task.await;
        crate::ipc::run_blocking(move || {
            root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    }

    fn allow() -> Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync> {
        Arc::new(|| Ok(()))
    }

    /// The paired host received the stop, then its reply was unreadable. The forward is not
    /// sent again, is outcome-unknown (409), and is audited once under the device.
    #[tokio::test]
    async fn a_stop_whose_reply_is_lost_after_delivery_is_outcome_unknown_and_never_resent() {
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&hits);
        let host = axum::Router::new().route(
            "/host/a/api/v1/agent-control/sessions/s-1/stop",
            axum::routing::post(move || {
                let counted = Arc::clone(&counted);
                async move {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    (axum::http::StatusCode::OK, "not json")
                }
            }),
        );
        let (root, service, view, task) = fake_paired_host(host).await;
        let (_backend, state) = pane("agent-control-lost-reply", CLAUDE_APPROVAL);
        state.set_paired_hosts(service);

        let error = forward_as(
            &state,
            view.host_id.clone(),
            AgentControlOp::Stop { session_id: "s-1".into(), request_id: None },
            "device-1",
            allow(),
            None,
        )
        .await
        .expect_err("an unreadable reply after delivery is not a success");

        assert_eq!(error.code, ScopeErrorCode::OperationOutcomeUnknown, "{}", error.message);
        assert_eq!(reference_chat_status(error.code), StatusCode::CONFLICT);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "the host saw exactly one stop");
        let audit = state.agent_control_audit.recent();
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(
            (audit[0].device_id.as_str(), audit[0].op.as_str(), audit[0].code.as_str()),
            ("device-1", "stop", crate::remote::reference_chat::types::REFERENCE_OUTCOME_UNKNOWN_CODE)
        );
        close_fake_host(root, task).await;
    }

    struct SilentControl;

    #[async_trait::async_trait]
    impl ferryx_hub::supervisor::panel_contract::SessionControl for SilentControl {
        async fn stop(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn send(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef, _: &str) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn answer(
            &self,
            _: &ferryx_hub::supervisor::panel_contract::SessionRef,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> ferryx_hub::error::Result<Option<String>> {
            Ok(None)
        }
        async fn read_tail(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef, _: usize) -> ferryx_hub::error::Result<String> {
            Ok(String::new())
        }
    }

    /// Sends the summary of every action-log write to the test, in order.
    struct ActionTap(tokio::sync::mpsc::UnboundedSender<String>);

    impl ferryx_hub::supervisor::autonomy::PanelPublisher for ActionTap {
        fn decision(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef, _: Option<&ferryx_hub::supervisor::panel_contract::Escalation>) {}
        fn action(&self, action: &ferryx_hub::supervisor::panel_contract::ActionEntry) {
            let _ = self.0.send(action.summary.clone());
        }
        fn rules(&self, _: &[ferryx_hub::supervisor::panel_contract::StandingRule]) {}
    }

    fn omarchy_blocked() -> ferryx_hub::supervisor::panel_contract::FleetSession {
        use ferryx_hub::supervisor::panel_contract::{FleetSession, PendingPrompt, SessionRef};
        FleetSession {
            session_ref: SessionRef::new("omarchy", "s-1"),
            host_id: "omarchy".into(),
            host_label: "omarchy".into(),
            title: "ferryx".into(),
            cwd: "/nonexistent/forward-answer-test".into(),
            agent: "claude".into(),
            state: "blocked".into(),
            state_source: "screen".into(),
            state_since_ms: None,
            screen_changed_at_ms: None,
            pending_prompt: Some(PendingPrompt {
                prompt_id: "p1".into(),
                kind: "question".into(),
                question: "Which package manager should I use?".into(),
                command: None,
                options: vec!["npm".into(), "pnpm".into()],
                screen_revision: Some("rev-1".into()),
            }),
            last_assistant_line: None,
            updated_at_ms: 1,
            stale: false,
            health: None,
            attention: None,
            reason: None,
        }
    }

    fn forwarded_tap() -> AgentControlOp {
        AgentControlOp::Answer {
            session_id: "s-1".into(),
            prompt_id: "p1".into(),
            screen_revision: "rev-1".into(),
            answer: ReferencePromptAnswer { option_index: Some(1), ..Default::default() },
            request_id: Some("tap-1".into()),
        }
    }

    /// A forwarded owner tap joins the answer history exactly when the paired host accepted it:
    /// recorded once on success, left unsettled on a stale screen and on a lost reply.
    #[tokio::test]
    async fn a_forwarded_owner_tap_is_recorded_only_when_the_paired_host_accepted_it() {
        use axum::routing::post;
        let answers = axum::Router::new().route(
            "/host/a/api/v1/agent-control/sessions/s-1/answer",
            post(|| async {
                axum::Json(serde_json::json!({"receipt": {"stage": "accepted", "target": {"hostId": "local", "backendSessionId": "s-1"}}}))
            }),
        );
        let conflict = axum::Router::new().route(
            "/host/a/api/v1/agent-control/sessions/s-2/answer",
            post(|| async {
                (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({"error": {"code": "REQUEST_CONFLICT", "message": "stale", "retryable": false, "requestId": "3941b9de-b16d-4d9a-ae0a-118f90fd91f4", "details": {}}})),
                )
            }),
        );
        let lost = axum::Router::new().route(
            "/host/a/api/v1/agent-control/sessions/s-3/answer",
            post(|| async { (axum::http::StatusCode::OK, "not json") }),
        );
        let (root, service, view, task) = fake_paired_host(answers.merge(conflict).merge(lost)).await;
        let (_backend, state) = pane("agent-control-forward-history", CLAUDE_APPROVAL);
        state.set_paired_hosts(service);

        let pool = ferryx_hub::storage::memory().await.unwrap();
        let (tap, mut actions) = tokio::sync::mpsc::unbounded_channel();
        let autonomy = ferryx_hub::supervisor::autonomy::Autonomy::open(pool, Arc::new(SilentControl), Arc::new(ActionTap(tap)))
            .await
            .unwrap();
        let claim = |op: &AgentControlOp| crate::hub_runtime::panel_autonomy::owner_answer_with(&autonomy, Some(&view.host_id), op);
        // The prompt is tracked under the paired host's own session ref.
        let track = |session: &str| {
            let mut tracked = omarchy_blocked();
            tracked.session_ref = ferryx_hub::supervisor::panel_contract::SessionRef::new(&view.host_id, session);
            tracked.host_id = view.host_id.clone();
            tracked
        };
        autonomy.observe(&track("s-1")).await;

        let op = forwarded_tap();
        let pending = claim(&op).expect("the paired host's prompt is tracked");
        let accepted = forward_with(&state, view.host_id.clone(), op.clone(), "device-1", allow(), None, Some(pending))
            .await
            .expect("the paired host accepted the tap");
        assert_eq!(accepted["receipt"]["stage"], "accepted", "{accepted}");
        let mut seen = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Some(summary) = actions.recv().await {
                seen.push(summary);
                if seen.iter().any(|summary| summary == "You chose pnpm") {
                    break;
                }
            }
        })
        .await
        .expect("the owner's choice was recorded");
        assert_eq!(seen.iter().filter(|summary| *summary == "You chose pnpm").count(), 1, "{seen:?}");
        assert!(claim(&op).is_none(), "the prompt is settled: learned once");
        assert_eq!(state.agent_control_audit.recent().len(), 1);

        // A tap the host refuses as stale, and one whose reply is lost, record nothing and leave
        // the prompt claimable. Each prompt gets its own session id on the fixture host.
        for (session, expected) in [("s-2", ScopeErrorCode::RequestConflict), ("s-3", ScopeErrorCode::OperationOutcomeUnknown)] {
            autonomy.observe(&track(session)).await;
            let mut refused = forwarded_tap();
            if let AgentControlOp::Answer { session_id, .. } = &mut refused {
                *session_id = session.into();
            }
            let pending = claim(&refused).expect("tracked and unsettled");
            let error = forward_with(&state, view.host_id.clone(), refused.clone(), "device-1", allow(), None, Some(pending))
                .await
                .expect_err("not accepted");
            assert_eq!(error.code, expected, "{session}: {}", error.message);
            assert!(claim(&refused).is_some(), "{session}: nothing was recorded, the prompt is still open");
        }
        while let Ok(summary) = actions.try_recv() {
            assert_ne!(summary, "You chose pnpm", "a refused or lost tap recorded an owner choice");
        }
        close_fake_host(root, task).await;
    }

    #[tokio::test]
    async fn a_stop_by_session_ref_interrupts_the_pane_with_no_hub_open() {
        let session_id = "agent-control-stop-by-ref";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        // No hub is open in this test, so a stop that needed one could not succeed.
        assert!(!crate::hub_runtime::supervisor_available());
        let (host_id, backend_id) = agent_target(&format!("local:{session_id}"), "local").unwrap();
        assert_eq!(host_id, None);

        let result = handle_as(
            &state,
            AgentControlOp::Stop { session_id: backend_id, request_id: None },
            "device-1",
            None,
        )
        .await
        .expect("the stop is accepted");

        assert_eq!(result["receipt"]["stage"], "accepted", "{result}");
        assert_eq!(backend.writes.lock().expect("writes").clone(), vec![b"\x1b".to_vec()]);
        assert!(backend.signals.lock().expect("signals").is_empty(), "stop never signals the process");
        let audit = state.agent_control_audit.latest(1);
        assert_eq!((audit[0].op.as_str(), audit[0].device_id.as_str(), audit[0].code.as_str()), ("stop", "device-1", "OK"));
    }

    #[tokio::test]
    async fn a_decision_tap_on_a_stale_screen_is_a_typed_conflict_and_types_nothing() {
        let session_id = "agent-control-decision-stale";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let (prompt_id, revision) = pending_prompt(&state, session_id).await;
        let tap = |revision: &str| {
            let body = DecisionAnswerBody {
                prompt_id: prompt_id.clone(),
                screen_revision: revision.to_string(),
                option_index: 0,
                request_id: None,
            };
            decision_answer_op(session_id.to_string(), body)
        };

        let stale = handle_as(&state, tap("not-the-current-revision"), "device-1", None)
            .await
            .expect_err("a stale revision is refused");
        assert_eq!(stale.code, ScopeErrorCode::RequestConflict, "{}", stale.message);
        assert!(backend.writes.lock().expect("writes").is_empty(), "nothing typed on a stale tap");
        assert_eq!(state.agent_control_audit.latest(1)[0].code, "REQUEST_CONFLICT");

        handle_as(&state, tap(&revision), "device-1", None).await.expect("the current revision is typed");
        assert!(!backend.writes.lock().expect("writes").is_empty());
        assert_eq!(state.agent_control_audit.latest(1)[0].op, "answer");
    }

    #[test]
    fn a_remote_receipt_carries_the_paired_host_id_and_other_results_do_not() {
        let mut receipt = serde_json::json!({
            "receipt": {
                "requestId": "r1",
                "target": {
                    "hostId": "local",
                    "ownerId": "owner",
                    "epoch": "1",
                    "backendSessionId": "pane-1"
                },
                "stage": "accepted"
            }
        });
        stamp_remote_receipt_host(&mut receipt, "https://relay.example/host/other");
        assert_eq!(
            receipt["receipt"]["target"]["hostId"],
            "https://relay.example/host/other"
        );
        assert_eq!(receipt["receipt"]["target"]["backendSessionId"], "pane-1");

        let mut listed = serde_json::json!({"sessions": [{"sessionId": "pane-1"}]});
        stamp_remote_receipt_host(&mut listed, "https://relay.example/host/other");
        assert!(listed.get("receipt").is_none());
        assert_eq!(listed["sessions"][0]["sessionId"], "pane-1");
    }

    fn turn(role: reference_types::ReferenceTurnRole, parts: Vec<reference_types::ReferencePart>) -> reference_types::ReferenceTurn {
        reference_types::ReferenceTurn {
            role,
            started_at: None,
            ended_at: None,
            source: None,
            parts,
            abandoned: None,
            id: None,
            meta: None,
        }
    }

    fn text(text: &str) -> reference_types::ReferencePart {
        reference_types::ReferencePart::Text { text: text.to_string(), phase: None, id: None }
    }

    #[test]
    fn the_last_assistant_line_is_the_newest_assistant_text_cut_to_the_cap() {
        use reference_types::ReferenceTurnRole::{Assistant, User};
        assert_eq!(last_assistant_line_of(&[]), None);
        let turns = vec![
            turn(Assistant, vec![text("older answer")]),
            turn(Assistant, vec![text("First line\n\nSecond line  \n\n")]),
            turn(User, vec![text("a later user message")]),
        ];
        assert_eq!(last_assistant_line_of(&turns).as_deref(), Some("Second line"));
        let long = vec![turn(Assistant, vec![text(&"x".repeat(500))])];
        assert_eq!(last_assistant_line_of(&long).map(|line| line.chars().count()), Some(LAST_ASSISTANT_LINE_MAX_CHARS));
        let no_text = vec![turn(Assistant, vec![reference_types::ReferencePart::Thinking { text: "hmm".into(), id: None }])];
        assert_eq!(last_assistant_line_of(&no_text), None);
    }

    #[tokio::test]
    async fn a_list_entry_carries_the_last_assistant_line_key_null_when_unavailable() {
        let session_id = "agent-control-last-line";
        let (_backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let listed = handle(&state, AgentControlOp::List).await.expect("list");
        let entry = &listed["sessions"][0];
        assert!(entry.get("lastAssistantLine").is_some(), "{listed}");
        assert!(entry["lastAssistantLine"].is_null(), "a pane with no transcript has no line: {listed}");
        let read = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        assert!(read["session"].get("lastAssistantLine").is_some(), "{read}");
    }

    fn reading(session_id: &str, state: AgentControlState, source: AgentStateSource) -> events::ObservationDraft {
        events::ObservationDraft {
            session_id: session_id.to_string(),
            state,
            state_source: source,
            tier: AgentTier::Native,
            prompt_id: None,
            kind: None,
            screen_revision: None,
            workspace_id: None,
        }
    }

    #[tokio::test]
    async fn a_provider_working_to_idle_edge_with_no_outcome_is_unread() {
        let session_id = "agent-control-attention-edge";
        let (_backend, state) = pane(session_id, "$ ");
        let attention = |read: &serde_json::Value| read["session"]["attention"].clone();
        let before = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        assert_eq!(attention(&before)["axes"]["completion"], "none", "{before}");
        assert_ne!(attention(&before)["group"], "unread", "{before}");

        state.agent_events.publish(reading(session_id, AgentControlState::Working, AgentStateSource::Provider));
        state.agent_events.publish(reading(session_id, AgentControlState::Idle, AgentStateSource::Provider));
        let after = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        let attention = attention(&after);
        assert_eq!(attention["axes"]["completion"], "reported", "{after}");
        assert_eq!(attention["axes"]["read"], "unread", "{after}");
        assert_eq!(attention["axes"]["ownership"], "owner", "{after}");
        assert_eq!(attention["group"], "unread", "{after}");

        state.agent_events.publish(reading(session_id, AgentControlState::Working, AgentStateSource::Provider));
        let again = state.agent_events.completion(session_id);
        assert_eq!(again, None, "a new turn ends the completion episode");
    }

    #[tokio::test]
    async fn attention_of_a_pane_with_an_open_approval_needs_you() {
        let session_id = "agent-control-attention-approval";
        let (_backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let listed = handle(&state, AgentControlOp::List).await.expect("list");
        let attention = &listed["sessions"][0]["attention"];
        assert_eq!(attention["axes"]["demand"], "approval", "{listed}");
        assert_eq!(attention["axes"]["activity"], "stopped", "{listed}");
        assert_eq!(attention["group"], "needsYou", "{listed}");
    }

    #[test]
    fn attention_axes_map_prompt_kinds_failure_and_state() {
        use ferryx_hub::supervisor::panel_contract::{
            AttentionActivity, AttentionCompletion, AttentionDemand, AttentionOwnership, AttentionRead,
        };
        use reference_types::ReferencePromptKind as K;
        let axes = |state, pending, failed, completed| {
            attention_axes(state, pending, failed, completed, AttentionRead::Unread, AttentionOwnership::Owner)
        };
        assert_eq!(axes(AgentControlState::Blocked, Some(K::Question), false, false).demand, AttentionDemand::Question);
        assert_eq!(axes(AgentControlState::Blocked, Some(K::Menu), false, false).demand, AttentionDemand::Question);
        assert_eq!(axes(AgentControlState::Blocked, Some(K::Approval), true, false).demand, AttentionDemand::Approval);
        assert_eq!(axes(AgentControlState::Blocked, Some(K::Plan), false, false).demand, AttentionDemand::Approval);
        assert_eq!(axes(AgentControlState::Idle, None, true, false).demand, AttentionDemand::Error);
        assert_eq!(axes(AgentControlState::Idle, None, false, false).demand, AttentionDemand::None);
        assert_eq!(axes(AgentControlState::Working, None, false, false).activity, AttentionActivity::Working);
        assert_eq!(axes(AgentControlState::Unknown, None, false, false).activity, AttentionActivity::Unknown);
        assert_eq!(axes(AgentControlState::Idle, None, false, true).completion, AttentionCompletion::Reported);
    }

    struct InertControl;

    #[async_trait::async_trait]
    impl ferryx_hub::supervisor::panel_contract::SessionControl for InertControl {
        async fn stop(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn send(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef, _: &str) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn answer(
            &self,
            _: &ferryx_hub::supervisor::panel_contract::SessionRef,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> ferryx_hub::error::Result<Option<String>> {
            Err(ferryx_hub::error::HubError::message("this test never answers"))
        }
        async fn read_tail(&self, _: &ferryx_hub::supervisor::panel_contract::SessionRef, _: usize) -> ferryx_hub::error::Result<String> {
            Ok(String::new())
        }
    }

    /// Autonomy over an in-memory hub whose owner channel refreshes this computer's panes.
    async fn refreshing_autonomy(state: &Arc<RemoteGatewayState>) -> Arc<ferryx_hub::supervisor::autonomy::Autonomy> {
        use crate::hub_runtime::panel_autonomy::{OwnerChannel, SessionRefresh};
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let slot: Arc<std::sync::OnceLock<Arc<ferryx_hub::supervisor::autonomy::Autonomy>>> = Arc::default();
        let sources = {
            let slot = Arc::clone(&slot);
            Arc::new(move || {
                slot.get().map_or_else(Default::default, |autonomy| ferryx_hub::fleet::PanelSources {
                    log: Some(autonomy.action_log()),
                    rules: Some(autonomy.rule_store()),
                    escalations: Some(autonomy.escalation_source()),
                })
            })
        };
        let channel = OwnerChannel { sessions: Some(SessionRefresh { state: Arc::downgrade(state), sources }) };
        let autonomy = ferryx_hub::supervisor::autonomy::Autonomy::open(pool, Arc::new(InertControl), Arc::new(channel))
            .await
            .unwrap();
        let _ = slot.set(Arc::clone(&autonomy));
        autonomy
    }

    /// The next `panel.session` frame for `session_ref` on the process-wide owner channel.
    async fn next_panel_session(
        frames: &mut tokio::sync::broadcast::Receiver<serde_json::Value>,
        session_ref: &str,
    ) -> serde_json::Value {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                match frames.recv().await {
                    Ok(frame) if frame["type"] == "panel.session" && frame["sessionRef"] == session_ref => {
                        return frame["session"].clone()
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => panic!("the owner channel closed"),
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no panel.session for {session_ref}"))
    }

    fn local_ref(session_id: &str) -> ferryx_hub::supervisor::panel_contract::SessionRef {
        ferryx_hub::supervisor::panel_contract::SessionRef::new(&reference_files::reference_host_id(), session_id)
    }

    #[tokio::test]
    async fn a_decision_opened_or_resolved_emits_an_overlaid_panel_session() {
        let session_id = format!("attention-decision-{}", uuid::Uuid::new_v4().simple());
        let (_backend, state) = pane(&session_id, CLAUDE_APPROVAL);
        let autonomy = refreshing_autonomy(&state).await;
        let session_ref = local_ref(&session_id);
        let mut frames = crate::hub_runtime::subscribe_panel_events();
        let sequence = state.agent_events.head();

        let mut session = crate::hub_runtime::local_fleet_session(&state, &session_id).await.expect("the pane");
        session.session_ref = session_ref.clone();
        autonomy.observe(&session).await;
        assert_eq!(autonomy.escalation_source().pending().await.unwrap().len(), 1, "a decision opened");
        let opened = next_panel_session(&mut frames, session_ref.as_str()).await;
        assert_eq!(opened["attention"]["axes"]["ownership"], "owner", "{opened}");
        assert_eq!(opened["attention"]["group"], "needsYou", "{opened}");

        autonomy.clear(&session_ref);
        let resolved = next_panel_session(&mut frames, session_ref.as_str()).await;
        assert_eq!(resolved["attention"]["group"], "needsYou", "{resolved}");
        assert_eq!(resolved["state"], opened["state"], "the agent state did not move");
        assert_eq!(state.agent_events.head(), sequence, "no agent reading was published");
    }

    #[tokio::test]
    async fn a_handling_entry_ending_emits_an_overlaid_panel_session() {
        use ferryx_hub::supervisor::panel_contract::{ActionEntry, ActionKind};
        let session_id = format!("attention-handling-{}", uuid::Uuid::new_v4().simple());
        let (_backend, state) = pane(&session_id, CLAUDE_APPROVAL);
        let autonomy = refreshing_autonomy(&state).await;
        let session_ref = local_ref(&session_id);
        let mut frames = crate::hub_runtime::subscribe_panel_events();
        let sequence = state.agent_events.head();
        let entry = |live| ActionEntry {
            id: 0,
            incident_key: format!("recovery:{session_id}"),
            session_ref: Some(session_ref.clone()),
            kind: ActionKind::Retried,
            summary: "Retrying".into(),
            basis: "default rule".into(),
            evidence: Vec::new(),
            undo: None,
            live,
            created_at_ms: 1,
            updated_at_ms: observed_at_ms(),
        };

        autonomy.action_log().upsert_incident(entry(true)).await.unwrap();
        let handled = next_panel_session(&mut frames, session_ref.as_str()).await;
        assert_eq!(handled["attention"]["axes"]["ownership"], "supervisor", "{handled}");
        assert_eq!(handled["attention"]["group"], "seen", "{handled}");

        autonomy.action_log().upsert_incident(entry(false)).await.unwrap();
        let ended = next_panel_session(&mut frames, session_ref.as_str()).await;
        assert_eq!(ended["attention"]["axes"]["ownership"], "owner", "{ended}");
        assert_eq!(ended["attention"]["group"], "needsYou", "{ended}");
        assert_eq!(ended["state"], handled["state"], "the agent state did not move");
        assert_eq!(state.agent_events.head(), sequence, "no agent reading was published");
    }

    #[test]
    fn reason_prompt_question_wins_over_last_line_and_cause() {
        assert_eq!(
            session_reason(Some("Run the migration?"), Some("I wrote the plan"), Some("Waiting for input")).as_deref(),
            Some("Run the migration?")
        );
    }

    #[test]
    fn reason_falls_back_to_the_last_line_then_the_stop_cause() {
        assert_eq!(session_reason(None, Some("  All tests pass\n "), Some("Finished its turn")).as_deref(), Some("All tests pass"));
        assert_eq!(session_reason(Some("  "), None, Some("Finished its turn")).as_deref(), Some("Finished its turn"));
        assert_eq!(stop_cause(AgentControlState::Blocked, false, false), Some("Waiting for input"));
        assert_eq!(stop_cause(AgentControlState::Idle, false, true), Some("Finished its turn"));
        assert_eq!(stop_cause(AgentControlState::Working, true, false), Some("The agent exited in the middle of a turn"));
        assert_eq!(stop_cause(AgentControlState::Working, false, false), None);
        assert_eq!(session_reason(None, None, None), None, "nothing to say is null, never empty");
    }

    #[test]
    fn reason_is_one_line_capped_at_200_chars() {
        let long = format!("{}\nsecond line", "é".repeat(300));
        let reason = session_reason(Some(&long), None, None).expect("a reason");
        assert_eq!(reason.chars().count(), LAST_ASSISTANT_LINE_MAX_CHARS);
        assert!(!reason.contains('\n'));
    }

    #[test]
    fn reason_screen_line_skips_rules_glyphs_and_empty_input_boxes() {
        let screen = "❯ build it\n⏺ Building the relay\n  Done.\n✻ Worked for 3s · done 4:04 PM\n────────\n> \n────────\n  ⏸ manual mode on\n";
        assert_eq!(last_screen_line(screen).as_deref(), Some("Done."));
        assert_eq!(last_screen_line("────\n\n"), None);
    }

    /// Every captured agent screen, with the line it must yield. Reviewed by hand against each
    /// fixture: a `Some` is the last line of the agent's reply in the newest whole turn, and
    /// every other screen (banners, tips, footers, status bars, model or path lines, token
    /// counts, menus, spinners) is `None`, so its reason falls back to the stop cause.
    fn golden_screens() -> Vec<(&'static str, &'static str, Option<&'static str>)> {
        macro_rules! golden {
            ($name:literal, $line:expr) => {
                ($name, include_str!(concat!("../agent_detect/fixtures/screens/", $name, ".screen")), $line)
            };
        }
        vec![
            golden!("agy/idle", None),
            golden!("agy/lc-idle", None),
            golden!("agy/lc-working", None),
            golden!("agy/resume", None),
            golden!("agy/trust", None),
            golden!("agy/working", None),
            golden!("aider/approval-add-file", None),
            golden!("aider/approval-gitignore-confirm", None),
            golden!("aider/approval-run-shell", None),
            golden!("aider/idle-after-turn", None),
            golden!("aider/idle", None),
            golden!("aider/login-wall", None),
            golden!("aider/working-commit-message", None),
            golden!("aider/working-waiting-spinner", None),
            golden!("claude/after-turn-idle", Some("plain ls.")),
            golden!("claude/approval", None),
            golden!("claude/done", Some("The serena MCP server failed to connect (timed out after 30s). It doesn't affect this ls command.")),
            golden!("claude/idle", None),
            golden!("claude/lc-working", None),
            golden!("claude/local1-idle", None),
            golden!("claude/local1-working", None),
            golden!("claude/resumed", Some("contains the current date: Thu Oct 22 16:04:12 KST 2026.")),
            golden!("claude/working", Some("contains the current date: Thu Oct 22 16:04:12 KST 2026.")),
            golden!("cline/after-turn-idle", None),
            golden!("cline/approval", None),
            golden!("cline/idle-after-turn", None),
            golden!("cline/idle", None),
            golden!("cline/lc-working", None),
            golden!("cline/resumed", None),
            golden!("cline/working", None),
            golden!("codex/after-turn-idle", Some("b.py")),
            golden!("codex/approval", None),
            golden!("codex/done", Some("Command completed successfully.")),
            golden!("codex/idle", None),
            golden!("codex/lc-working", None),
            golden!("codex/local1-working", None),
            golden!("codex/resumed", Some("Command completed successfully.")),
            golden!("codex/working", None),
            golden!("copilot/after-turn", None),
            golden!("copilot/approval", None),
            golden!("copilot/idle", None),
            golden!("copilot/local3-idle", None),
            golden!("copilot/local3-working", None),
            golden!("copilot/resumed", None),
            golden!("copilot/working-modelfail", None),
            golden!("copilot/working", None),
            golden!("crush/after-allow", None),
            golden!("crush/approval", None),
            golden!("crush/idle-after-turn", None),
            golden!("crush/idle", None),
            golden!("crush/resumed-session", None),
            golden!("crush/working", None),
            golden!("cursor-agent/live-approval", None),
            golden!("cursor-agent/live-idle", None),
            golden!("cursor-agent/live-running", None),
            golden!("cursor-agent/live-trust", None),
            golden!("cursor-agent/live-working", None),
            golden!("cursor-agent/live2-approval", None),
            golden!("cursor-agent/live2-trust", None),
            golden!("cursor-agent/login-required", None),
            golden!("droid/login-device-auth", None),
            golden!("droid/login-wall", None),
            golden!("gjc/after-answer", None),
            golden!("gjc/approval-ask", None),
            golden!("gjc/idle-after-turn", None),
            golden!("gjc/idle", None),
            golden!("gjc/working", None),
            golden!("grok/after-approve", None),
            golden!("grok/approval", None),
            golden!("grok/idle-after-turn", None),
            golden!("grok/idle", None),
            golden!("grok/login-wall", None),
            golden!("grok/resumed", None),
            golden!("grok/trust-folder", None),
            golden!("grok/working", None),
            golden!("kimi/idle", None),
            golden!("kimi/resume", None),
            golden!("kimi/working", None),
            golden!("omp/approval", None),
            golden!("omp/idle-after-cancel-retry", None),
            golden!("omp/idle-after-interrupt", None),
            golden!("omp/idle-after-turn", None),
            golden!("omp/idle-no-model", None),
            golden!("omp/idle-resumed", None),
            golden!("omp/idle", None),
            golden!("omp/question-multi", None),
            golden!("omp/question-single", None),
            golden!("omp/working-2", None),
            golden!("omp/working", None),
            golden!("opencode/approval", None),
            golden!("opencode/done", None),
            golden!("opencode/idle", None),
            golden!("opencode/lc-approval", None),
            golden!("opencode/lc-working", None),
            golden!("opencode/local1-working", None),
            golden!("opencode/resumed", None),
            golden!("opencode/working", None),
            golden!("pi/approval", None),
            golden!("pi/done", None),
            golden!("pi/idle", None),
            golden!("pi/lc-idle-no-model", None),
            golden!("pi/local1-idle", None),
            golden!("pi/resumed", None),
            golden!("pi/working", None),
        ]
    }

    #[test]
    fn reason_screen_line_matches_the_reviewed_line_of_every_agent_fixture() {
        let golden = golden_screens();
        let on_disk = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/agent_detect/fixtures/screens"))
            .expect("the fixture directory")
            .flat_map(|agent| std::fs::read_dir(agent.expect("an agent dir").path()).expect("its screens"))
            .filter(|file| file.as_ref().is_ok_and(|file| file.path().extension().is_some_and(|ext| ext == "screen")))
            .count();
        assert_eq!(golden.len(), on_disk, "every fixture screen has a reviewed row");
        const CHROME: &[&str] = &[
            "for shortcuts", "mode on", "esc to", "ctrl+", "shift+tab", "Tip:", "warnings", "tok/s", "context:",
            "(1x)", "128K", "Type your message", "/model", "/private/", "Changelog", "Working (", "Thinking",
        ];
        let mut wrong = Vec::new();
        for (name, screen, expected) in &golden {
            let line = last_screen_line(screen);
            if line.as_deref() != *expected {
                wrong.push(format!("{name}: got {line:?}, want {expected:?}"));
            }
            if let Some(line) = &line {
                assert!(screen.lines().any(|row| row.contains(line.as_str())), "{name}: {line:?} is on the screen");
                for chrome in CHROME {
                    assert!(!line.contains(chrome), "{name}: chrome {chrome:?} was taken: {line:?}");
                }
            }
        }
        assert!(wrong.is_empty(), "{} of {} fixtures differ:\n{}", wrong.len(), golden.len(), wrong.join("\n"));
        assert!(golden.iter().any(|(_, _, line)| line.is_some()), "the table has transcript lines");
    }

    #[test]
    fn reason_screen_line_is_the_transcript_line_above_the_input_box_not_the_footer() {
        let screens = golden_screens();
        for (name, expected) in [
            ("claude/done", "The serena MCP server failed to connect (timed out after 30s). It doesn't affect this ls command."),
            ("claude/after-turn-idle", "plain ls."),
            ("codex/done", "Command completed successfully."),
            ("codex/after-turn-idle", "b.py"),
        ] {
            let screen = screens.iter().find(|(fixture, _, _)| *fixture == name).map(|(_, screen, _)| *screen).expect(name);
            assert_eq!(last_screen_line(screen).as_deref(), Some(expected), "{name}");
        }
    }

    #[test]
    fn reason_screen_line_ignores_quotes_and_needs_the_newest_turn() {
        let quoted = "❯ check the doc\n⏺ It says:\n  > keep the relay\n  So the relay stays.\n✻ Worked for 3s · done 4:04 PM\n";
        assert_eq!(last_screen_line(quoted).as_deref(), Some("So the relay stays."), "an indented > is a quote, not the input");
        let newer = "❯ first\n⏺ First reply.\n✻ Worked for 3s · done 4:04 PM\n❯ second\n⏺ Thinking about it\n";
        assert_eq!(last_screen_line(newer), None, "a newer turn without its timing line has no finished reply");
        assert_eq!(last_screen_line("⏺ A reply with no user turn above it\n✻ done 4:04 PM\n"), None);
    }

    #[test]
    fn reason_with_nothing_above_the_input_box_falls_to_the_stop_cause() {
        let screen = "\n\n────────\n❯ \n────────\n  ⏸ manual mode on · ? for shortcuts\n";
        assert_eq!(last_screen_line(screen), None);
        let codex = "  4:08 PM\n\n› Ask Codex to do anything\n\n  GPT-5.6-Luna medium · /tmp/x\n";
        assert_eq!(last_screen_line(codex), None, "a timing line alone is not the agent's line");
        let cause = stop_cause(AgentControlState::Idle, false, true);
        assert_eq!(session_reason(None, last_screen_line(screen).as_deref(), cause).as_deref(), Some("Finished its turn"));
    }

    #[tokio::test]
    async fn reason_of_a_pane_with_a_question_is_the_question() {
        let session_id = "agent-control-reason-prompt";
        let (_backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let read = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        let session = &read["session"];
        let pending = &session["pending"];
        let question = pending["question"].as_str().filter(|q| !q.trim().is_empty()).or(pending["title"].as_str()).expect("a prompt");
        assert_eq!(session["reason"].as_str(), session_reason(Some(question), None, None).as_deref(), "{read}");
        assert!(session["reason"].as_str().is_some_and(|reason| !reason.is_empty()), "{read}");
    }

    #[tokio::test]
    async fn reason_of_a_session_without_a_transcript_uses_the_screen_line() {
        let session_id = "agent-control-reason-screen";
        let (_backend, state) = pane(session_id, CLAUDE_TURN);
        let read = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        let session = &read["session"];
        assert!(session["agent"].is_string(), "an agent is detected: {read}");
        assert!(session["pending"].is_null(), "{read}");
        assert!(session["lastAssistantLine"].is_null(), "{read}");
        assert_eq!(session["reason"], "It has README.md and src.", "{read}");
    }

    #[tokio::test]
    async fn reason_of_a_plain_shell_pane_is_null_not_empty() {
        let session_id = "agent-control-reason-shell";
        let (_backend, state) = pane(session_id, "$ ls\nREADME.md\n$ ");
        let listed = handle(&state, AgentControlOp::List).await.expect("list");
        let entry = &listed["sessions"][0];
        assert!(entry["agent"].is_null(), "{listed}");
        assert!(entry.get("reason").is_some_and(serde_json::Value::is_null), "{listed}");
        let fleet = ferryx_hub::fleet::merge_sessions(
            &std::collections::HashMap::new(),
            &[ferryx_hub::fleet::InventoryEntry { host_id: "h".into(), sessions: vec![entry.clone()], error: None, stale: false }],
            1,
        );
        assert_eq!(fleet[0].reason, None);
    }

    #[tokio::test]
    async fn a_mutation_path_observation_never_reads_the_history_store() {
        let session_id = "agent-control-no-line-on-mutation";
        let (_backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let plain = observe(&state, session_id).await.expect("observe");
        assert!(plain.entry.get("lastAssistantLine").is_none(), "{}", plain.entry);
        let with_line = observe_with_line(&state, session_id).await.expect("observe with line");
        assert!(with_line.entry.get("lastAssistantLine").is_some(), "{}", with_line.entry);
        for entry in [&plain.entry, &with_line.entry] {
            assert!(entry["observedAt"].as_i64().is_some_and(|at| at > 0), "{entry}");
        }
    }

    const CLAUDE_APPROVAL: &str = include_str!("reference_chat/fixtures/claude/prompt/approval-command.screen");
    /// A finished Claude turn that fits the 100x30 test pane whole: user echo, reply, timing line,
    /// input box and footer.
    const CLAUDE_TURN: &str = "❯ list it\n\n⏺ Listed the directory.\n  It has README.md and src.\n\n✻ Baked for 4s · done 4:03 PM\n\n\n────────────────────────────────────────\n❯\n────────────────────────────────────────\n  ⏸ manual mode on · ? for shortcuts · ← for agents\n";

    /// One pane showing a fixed screen; every write and every signal it is handed is recorded.
    struct FixedPane {
        session_id: String,
        screen: Vec<u8>,
        writes: std::sync::Mutex<Vec<Vec<u8>>>,
        signals: std::sync::Mutex<Vec<crate::terminal::TerminalSignal>>,
        fail_writes: std::sync::atomic::AtomicBool,
        write_attempts: std::sync::atomic::AtomicUsize,
    }

    impl crate::remote::backend::RemoteSessionBackend for FixedPane {
        fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
            Box::pin(async move { vec![self.session_id.clone()] })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> futures_util::future::BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            Box::pin(async move {
                if session_id != self.session_id {
                    return Err("unknown session".to_string());
                }
                Ok(RemoteSessionDetails {
                    session_id: self.session_id.clone(),
                    workspace_id: None,
                    worktree_label: None,
                    worktree_path: None,
                    running: true,
                    cols: 100,
                    rows: 30,
                })
            })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_sequence: Option<u64>,
        ) -> futures_util::future::BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>> {
            Box::pin(async move {
                let (sender, receiver) = tokio::sync::broadcast::channel(1);
                std::mem::forget(sender);
                Ok(crate::terminal::SessionAttachment {
                    snapshot: crate::terminal::AttachmentSnapshot {
                        session_id: self.session_id.clone(),
                        history_start_sequence: Some(1),
                        history_end_sequence: Some(1),
                        history: self.screen.clone(),
                        history_segments: vec![crate::terminal::HistorySegment {
                            cols: Some(100),
                            rows: Some(30),
                            bytes: self.screen.clone(),
                        }],
                        gap: None,
                        history_incomplete: false,
                    },
                    receiver,
                })
            })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            data: &'a [u8],
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            self.write_attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                return Box::pin(async { Err("the write failed".to_string()) });
            }
            self.writes.lock().expect("writes").push(data.to_vec());
            Box::pin(async { Ok(()) })
        }
        fn resize<'a>(&'a self, _session_id: &'a str, _cols: u16, _rows: u16) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Err("this pane never resizes".to_string()) })
        }
        fn signal<'a>(
            &'a self,
            _session_id: &'a str,
            signal: crate::terminal::TerminalSignal,
        ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
            self.signals.lock().expect("signals").push(signal);
            Box::pin(async { Ok(()) })
        }
    }

    fn pane(session_id: &str, screen: &str) -> (Arc<FixedPane>, Arc<RemoteGatewayState>) {
        let backend = Arc::new(FixedPane {
            session_id: session_id.to_string(),
            screen: screen.trim_end().replace('\n', "\r\n").into_bytes(),
            writes: std::sync::Mutex::new(Vec::new()),
            signals: std::sync::Mutex::new(Vec::new()),
            fail_writes: std::sync::atomic::AtomicBool::new(false),
            write_attempts: std::sync::atomic::AtomicUsize::new(0),
        });
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            Arc::clone(&backend) as Arc<dyn crate::remote::backend::RemoteSessionBackend>,
            crate::worktree::registry::WorkspaceRegistry::new(),
        ));
        (backend, state)
    }

    /// The pending prompt a read reports, and the revision it was read from.
    async fn pending_prompt(state: &Arc<RemoteGatewayState>, session_id: &str) -> (String, String) {
        let read = handle(state, AgentControlOp::Read { session_id: session_id.to_string() }).await.expect("read");
        let session = &read["session"];
        assert_eq!(session["agent"], "claude", "{read}");
        assert_eq!(session["state"], "blocked", "{read}");
        assert_eq!(session["stateSource"], "screen", "{read}");
        let prompt_id = session["pending"]["id"].as_str().expect("a pending prompt").to_string();
        let revision = session["screenRevision"].as_str().expect("a screen revision").to_string();
        (prompt_id, revision)
    }

    fn answer(session_id: &str, prompt_id: &str, revision: &str, request_id: Option<&str>) -> AgentControlOp {
        AgentControlOp::Answer {
            session_id: session_id.to_string(),
            prompt_id: prompt_id.to_string(),
            screen_revision: revision.to_string(),
            answer: ReferencePromptAnswer { option_index: Some(0), ..Default::default() },
            request_id: request_id.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn an_answer_to_the_prompt_on_the_screen_is_typed() {
        let session_id = "agent-control-answer-ok";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let (prompt_id, revision) = pending_prompt(&state, session_id).await;

        let result = handle(&state, answer(session_id, &prompt_id, &revision, None)).await.expect("the answer is accepted");

        assert_eq!(result["receipt"]["stage"], "accepted", "{result}");
        assert!(!backend.writes.lock().expect("writes").is_empty(), "an accepted answer typed its keys");
        let replay = handle(&state, answer(session_id, &prompt_id, &revision, Some("agent-control-answer-replay")))
            .await
            .expect_err("a card is answered once");
        assert_eq!(replay.code, ScopeErrorCode::RequestConflict, "{}", replay.message);
    }

    #[tokio::test]
    async fn an_answer_for_a_screen_that_moved_on_types_nothing() {
        let session_id = "agent-control-answer-stale";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);
        let (prompt_id, _) = pending_prompt(&state, session_id).await;

        let error = handle(&state, answer(session_id, &prompt_id, "not-the-current-revision", None))
            .await
            .expect_err("a stale revision is refused");

        assert_eq!(error.code, ScopeErrorCode::RequestConflict, "{}", error.message);
        assert!(backend.writes.lock().expect("writes").is_empty(), "a refused answer typed nothing");
    }

    #[tokio::test]
    async fn a_pane_no_prompt_reader_knows_cannot_be_answered() {
        let session_id = "agent-control-answer-shell";
        let (backend, state) = pane(session_id, "$ ");

        let error = handle(&state, answer(session_id, "any", "any", None)).await.expect_err("no reader knows a shell");

        assert_eq!(error.code, ScopeErrorCode::Unsupported, "{}", error.message);
        assert!(backend.writes.lock().expect("writes").is_empty(), "nothing was typed");
    }

    #[tokio::test]
    async fn stop_presses_the_interrupt_key_and_never_signals() {
        let session_id = "agent-control-stop";
        let (backend, state) = pane(session_id, CLAUDE_APPROVAL);

        handle(&state, AgentControlOp::Stop { session_id: session_id.to_string(), request_id: None })
            .await
            .expect("stop is accepted");

        assert_eq!(backend.writes.lock().expect("writes").clone(), vec![b"\x1b".to_vec()]);
        assert!(backend.signals.lock().expect("signals").is_empty(), "stop must never signal the process");
        assert!(backend.list_sessions().await.contains(&session_id.to_string()));
    }

    #[tokio::test]
    async fn a_person_who_just_typed_blocks_supervisor_input() {
        let session_id = "agent-control-human-typing";
        let (backend, state) = pane(session_id, "$ ");
        reference_chat_runtime()
            .input
            .note_raw_input(session_id, reference_input::SystemReferenceClock.now_ms());

        let error = handle(
            &state,
            AgentControlOp::Send {
                session_id: session_id.to_string(),
                text: "echo hi".into(),
                request_id: Some("human-1".into()),
            },
        )
        .await
        .expect_err("a recent keystroke is a conflict");

        assert_eq!(
            error.code,
            ScopeErrorCode::ControlConflict,
            "{}",
            error.message
        );
        assert_eq!(
            backend
                .write_attempts
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(backend.writes.lock().expect("writes").is_empty());
    }

    #[tokio::test]
    async fn an_outcome_unknown_send_is_not_retried() {
        let session_id = "agent-control-outcome-unknown";
        let (backend, state) = pane(session_id, "$ ");
        backend
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let op = AgentControlOp::Send {
            session_id: session_id.to_string(),
            text: "echo hi".into(),
            request_id: Some("unknown-1".into()),
        };

        let first = handle(&state, op.clone())
            .await
            .expect_err("the write failed");
        assert_eq!(
            first.code,
            ScopeErrorCode::OperationOutcomeUnknown,
            "{}",
            first.message
        );
        let attempts = backend
            .write_attempts
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(attempts, 1);

        let second = handle(&state, op)
            .await
            .expect_err("the same id is not typed again");
        assert_eq!(
            second.code,
            ScopeErrorCode::OperationOutcomeUnknown,
            "{}",
            second.message
        );
        assert_eq!(
            backend
                .write_attempts
                .load(std::sync::atomic::Ordering::SeqCst),
            attempts,
            "outcome-unknown is held and never replayed"
        );
    }

    #[tokio::test]
    async fn a_revoked_authorizer_types_nothing() {
        let session_id = "agent-control-revoked-midway";
        let (backend, state) = pane(session_id, "$ ");
        let authorize = Arc::new(|| {
            Err(reference_chat_scope(
                ScopeErrorCode::Unauthorized,
                "the caller's device is no longer authorized",
            ))
        });

        let error = handle_as(
            &state,
            AgentControlOp::Send {
                session_id: session_id.to_string(),
                text: "echo hi".into(),
                request_id: Some("revoked-1".into()),
            },
            "dev-revoked",
            Some(authorize),
        )
        .await
        .expect_err("a revoke before the first byte types nothing");

        assert_eq!(
            error.code,
            ScopeErrorCode::Unauthorized,
            "{}",
            error.message
        );
        assert!(backend.writes.lock().expect("writes").is_empty());
        let audit = state.agent_control_audit.recent();
        let record = audit.last().expect("the refusal is audited");
        assert_eq!(record.device_id, "dev-revoked");
        assert_eq!(record.op, "send");
        assert_eq!(record.code, "UNAUTHORIZED");
    }

    /// A device holding a machine Control grant is refused until the owner turns agent control on.
    #[tokio::test]
    async fn a_disabled_policy_refuses_injection() {
        let session_id = "agent-control-disabled-policy";
        let (backend, state) = pane(session_id, "$ ");
        let pin = state
            .auth_manager
            .create_scoped_pairing_code(
                crate::remote::auth::DevicePermission::Control,
                crate::remote::auth::DeviceAccessScope::Machine,
            )
            .expect("machine control pin");
        let (token, _device) = state.auth_manager.exchange_pairing_code(&pin, "hub").expect("exchange");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("header"),
        );
        let send = |request_id: &str| {
            Ok(Json(SendBody {
                text: "echo hi".into(),
                request_id: Some(request_id.into()),
                observe_ms: None,
                supervisor_fence: None,
                supervisor_account: None,
            }))
        };

        assert!(!state.agent_control_policy.enabled(), "agent control is off on a fresh state");
        let refused = http_send(
            State(Arc::clone(&state)),
            headers.clone(),
            AxumPath(session_id.to_string()),
            send("disabled-1"),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(refused.into_body(), 64 * 1024).await.expect("body"),
        )
        .expect("json");
        assert_eq!(body["error"]["code"], "FORBIDDEN", "{body}");
        assert_eq!(backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(backend.writes.lock().expect("writes").is_empty());

        state.set_agent_control(true, Vec::new()).expect("enable in memory");
        let accepted = http_send(
            State(Arc::clone(&state)),
            headers,
            AxumPath(session_id.to_string()),
            send("enabled-1"),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::OK);
        assert!(!backend.writes.lock().expect("writes").is_empty(), "the enabled send typed its keys");
    }

    fn supervisor(account: &str, fence: u64) -> Option<crate::agent_control::fence::SupervisorFence> {
        Some(crate::agent_control::fence::SupervisorFence {
            account: account.into(),
            fence,
        })
    }

    fn send_op(session_id: &str, request_id: &str) -> AgentControlOp {
        AgentControlOp::Send {
            session_id: session_id.to_string(),
            text: "echo hi".into(),
            request_id: Some(request_id.into()),
        }
    }

    fn error_code(response: &crate::daemon::protocol::DaemonResponse) -> Option<&str> {
        match response {
            crate::daemon::protocol::DaemonResponse::Error { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    #[tokio::test]
    async fn a_stale_supervisor_fence_is_refused_and_leaves_the_pane_untouched() {
        let session_id = "agent-control-fence-stale";
        let (backend, state) = pane(session_id, "$ ");

        let newer = handle_fenced(&state, send_op(session_id, "fence-2"), supervisor("user-1", 2)).await;
        assert!(
            matches!(newer, crate::daemon::protocol::DaemonResponse::AgentControlOk { .. }),
            "{newer:?}"
        );
        let typed = backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst);
        assert!(typed > 0, "the current holder's send typed");

        let stale = handle_fenced(&state, send_op(session_id, "fence-1"), supervisor("user-1", 1)).await;
        assert_eq!(error_code(&stale), Some("STALE_SUPERVISOR_FENCE"), "{stale:?}");
        assert_eq!(
            backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst),
            typed,
            "a stale supervisor typed nothing"
        );
        assert_eq!(state.supervisor_fence.max_for("user-1").await, 2);
        let audit = state.agent_control_audit.recent();
        assert_eq!(audit.last().expect("the refusal is audited").code, "STALE_SUPERVISOR_FENCE");
    }

    #[tokio::test]
    async fn an_equal_fence_passes_and_a_request_without_one_is_never_fenced() {
        let session_id = "agent-control-fence-manual";
        let (backend, state) = pane(session_id, "$ ");
        state.supervisor_fence.admit(&supervisor("user-1", 9).unwrap()).await.unwrap();

        let same = handle_fenced(&state, send_op(session_id, "fence-9"), supervisor("user-1", 9)).await;
        assert!(
            matches!(same, crate::daemon::protocol::DaemonResponse::AgentControlOk { .. }),
            "the holder's own fence is not stale: {same:?}"
        );
        let after_holder = backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst);
        // A person's or an older hub's request carries no fence: accepted after the max rose.
        let manual = handle_fenced(&state, send_op(session_id, "manual-1"), None).await;
        assert!(
            matches!(manual, crate::daemon::protocol::DaemonResponse::AgentControlOk { .. }),
            "{manual:?}"
        );
        assert!(backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst) > after_holder);
    }

    #[tokio::test]
    async fn a_paired_send_with_a_stale_fence_is_refused_before_typing() {
        let session_id = "agent-control-fence-paired";
        let (backend, state) = pane(session_id, "$ ");
        state.set_agent_control(true, Vec::new()).expect("enable in memory");
        let pin = state
            .auth_manager
            .create_scoped_pairing_code(
                crate::remote::auth::DevicePermission::Control,
                crate::remote::auth::DeviceAccessScope::Machine,
            )
            .expect("machine control pin");
        let (token, _device) = state.auth_manager.exchange_pairing_code(&pin, "hub").expect("exchange");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {token}").parse().expect("header"),
        );
        let pushed = http_supervisor_fence(
            State(Arc::clone(&state)),
            headers.clone(),
            Ok(Json(FenceBody {
                account: "user-1".into(),
                fence: 5,
            })),
        )
        .await;
        assert_eq!(pushed.status(), StatusCode::OK);

        let refused = http_send(
            State(Arc::clone(&state)),
            headers.clone(),
            AxumPath(session_id.to_string()),
            Ok(Json(SendBody {
                text: "echo hi".into(),
                request_id: Some("paired-stale".into()),
                observe_ms: None,
                supervisor_fence: Some(4),
                supervisor_account: Some("user-1".into()),
            })),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::CONFLICT);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(refused.into_body(), 64 * 1024).await.expect("body"),
        )
        .expect("json");
        assert_eq!(body["error"]["code"], "STALE_SUPERVISOR_FENCE", "{body}");
        assert_eq!(backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);

        // A lower fence push is refused too: the old holder cannot lower the maximum.
        let lowered = http_supervisor_fence(
            State(Arc::clone(&state)),
            headers.clone(),
            Ok(Json(FenceBody {
                account: "user-1".into(),
                fence: 3,
            })),
        )
        .await;
        assert_eq!(lowered.status(), StatusCode::CONFLICT);

        let manual = http_send(
            State(Arc::clone(&state)),
            headers,
            AxumPath(session_id.to_string()),
            Ok(Json(SendBody {
                text: "echo hi".into(),
                request_id: Some("paired-manual".into()),
                observe_ms: None,
                supervisor_fence: None,
                supervisor_account: None,
            })),
        )
        .await;
        assert_eq!(manual.status(), StatusCode::OK, "a send without a fence is never fenced");
    }

    #[tokio::test]
    async fn events_and_wait_replay_a_gap_and_a_matching_reading() {
        let session_id = "agent-control-gap";
        let (_backend, state) = pane(session_id, "$ ");
        for index in 0..=events::AGENT_EVENT_RETAIN {
            state.agent_events.publish(events::ObservationDraft {
                session_id: session_id.to_string(),
                state: if index % 2 == 0 {
                    AgentControlState::Working
                } else {
                    AgentControlState::Idle
                },
                state_source: AgentStateSource::Screen,
                tier: AgentTier::Screen,
                prompt_id: None,
                kind: None,
                screen_revision: Some(format!("rev-{index}")),
                workspace_id: None,
            });
        }

        let gapped = handle(&state, AgentControlOp::Events { after: 0 })
            .await
            .expect("a fallen cursor is still a successful replay");
        assert_eq!(gapped["gap"], true, "{gapped}");
        assert!(!gapped["snapshot"].as_array().unwrap().is_empty());

        let waited = handle(
            &state,
            AgentControlOp::Wait {
                session_id: session_id.to_string(),
                until: "any".into(),
                after_revision: None,
                after: 0,
                timeout_ms: 1_000,
            },
        )
        .await
        .expect("wait reports the gap instead of blocking");
        assert_eq!(waited["gap"], true, "{waited}");

        let latest = state.agent_events.latest(session_id).unwrap();
        let tail = handle(
            &state,
            AgentControlOp::Events {
                after: latest.sequence.saturating_sub(1),
            },
        )
        .await
        .expect("a cursor inside the tail replays");
        assert_eq!(tail["gap"], false, "{tail}");
        assert!(!tail["events"].as_array().unwrap().is_empty());
    }

    const CODEX_APPROVAL: &str = include_str!("reference_chat/fixtures/codex/approval-command.txt");

    /// A Slack approval click answers the card that is on the pane. The click carries the
    /// prompt id and the screen revision the owner saw, and the answer goes through the
    /// same handler as every other agent-control answer.
    #[tokio::test]
    async fn a_slack_approval_answers_a_claude_card_and_a_codex_card() {
        for (session_id, screen) in [
            ("slack-approval-claude", CLAUDE_APPROVAL),
            ("slack-approval-codex", CODEX_APPROVAL),
        ] {
            let (backend, state) = pane(session_id, screen);
            let read = handle(&state, AgentControlOp::Read { session_id: session_id.to_string() })
                .await
                .expect("read");
            let prompt_id = read["session"]["pending"]["id"].as_str().unwrap_or_else(|| panic!("pending prompt: {read}")).to_string();
            let revision = read["session"]["screenRevision"].as_str().unwrap_or_else(|| panic!("revision: {read}")).to_string();
            let payload = serde_json::json!({
                "type": "block_actions",
                "user": {"id": "UOWNER"},
                "actions": [{
                    "value": format!("ferryx:approval:{session_id}:{prompt_id}:{revision}:0:once")
                }]
            });
            let op = ferryx_hub::chat::approval::slack_approval_op(&payload, "UOWNER", "local")
                .expect("owner click")
                .expect("an approval op");
            let mapped = crate::hub_runtime::to_agent_control_op(&op).expect("daemon op");
            let result = handle(&state, mapped).await.expect("the slack approval is typed");
            assert_eq!(result["receipt"]["stage"], "accepted", "{result}");
            assert!(
                !backend.writes.lock().expect("writes").is_empty(),
                "{session_id} typed the approval"
            );
        }
    }

    const CODEX_MENU: &str = include_str!("reference_chat/fixtures/codex/menu.txt");

    async fn desktop_prompt(
        state: &Arc<RemoteGatewayState>,
        session_id: &str,
    ) -> crate::daemon::protocol::AgentPromptView {
        prompt_get(state, session_id, Some("codex".into()))
            .await
            .expect("the screen is readable")
            .expect("the menu is a prompt")
    }

    fn pick(key: &str) -> crate::daemon::protocol::AgentPromptAnswerInput {
        crate::daemon::protocol::AgentPromptAnswerInput { key: Some(key.into()), text: None }
    }

    #[tokio::test]
    async fn agent_prompt_get_reads_a_three_option_menu_as_a_choice() {
        let session_id = "agent-prompt-get-menu";
        let (backend, state) = pane(session_id, CODEX_MENU);

        let prompt = desktop_prompt(&state, session_id).await;

        assert_eq!(prompt.kind, crate::daemon::protocol::AgentPromptViewKind::Choice);
        let keys: Vec<&str> = prompt.options.iter().map(|option| option.key.as_str()).collect();
        assert_eq!(keys, vec!["0", "1", "2"]);
        assert!(!prompt.prompt_id.is_empty() && !prompt.screen_revision.is_empty());
        let wire = serde_json::to_value(&prompt).expect("wire");
        assert_eq!(wire["kind"], "choice", "{wire}");
        // The update menu is a Menu card: picking an option is confirmed first, as on the phone.
        assert!(prompt.needs_confirmation);
        assert_eq!(wire["needsConfirmation"], true, "{wire}");
        assert_eq!(wire["options"].as_array().map(Vec::len), Some(3), "{wire}");
        assert!(wire["promptId"].is_string() && wire["screenRevision"].is_string(), "{wire}");
        assert!(backend.writes.lock().expect("writes").is_empty(), "a read types nothing");

        let (_shell, idle) = pane("agent-prompt-get-shell", "$ ");
        assert_eq!(prompt_get(&idle, "agent-prompt-get-shell", None).await.expect("readable"), None);
    }

    #[tokio::test]
    async fn agent_prompt_answer_with_a_stale_revision_is_refused_and_types_nothing() {
        let session_id = "agent-prompt-answer-stale";
        let (backend, state) = pane(session_id, CODEX_MENU);
        let prompt = desktop_prompt(&state, session_id).await;

        let error = prompt_answer(
            &state,
            session_id.into(),
            prompt.prompt_id,
            "not-the-current-revision".into(),
            pick("2"),
            Some("codex".into()),
        )
        .await
        .expect_err("a stale revision is refused");

        assert_eq!(error.code, ScopeErrorCode::RequestConflict, "{}", error.message);
        match error_response(error) {
            crate::daemon::protocol::DaemonResponse::Error { code, .. } => {
                assert_eq!(code.as_deref(), Some("REQUEST_CONFLICT"));
            }
            other => panic!("a refusal is an error response: {other:?}"),
        }
        assert_eq!(backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(backend.writes.lock().expect("writes").is_empty(), "a refused answer typed nothing");
    }

    #[tokio::test]
    async fn agent_prompt_answer_with_the_current_revision_writes_the_option_key() {
        let session_id = "agent-prompt-answer-current";
        let (backend, state) = pane(session_id, CODEX_MENU);
        let prompt = desktop_prompt(&state, session_id).await;

        let receipt = prompt_answer(
            &state,
            session_id.into(),
            prompt.prompt_id,
            prompt.screen_revision,
            pick("2"),
            Some("codex".into()),
        )
        .await
        .expect("the answer is typed");

        assert_eq!(receipt.target.backend_session_id, session_id);
        // The cursor starts on row 1, so the third option is two rows down, then Enter.
        assert_eq!(
            backend.writes.lock().expect("writes").clone(),
            vec![b"\x1b[B".to_vec(), b"\x1b[B".to_vec(), b"\r".to_vec()]
        );
    }

    const CLAUDE_QUESTION: &str = include_str!("reference_chat/fixtures/claude/prompt/question.screen");

    /// Answer the claude question card, which takes typed text on its "Type something." row,
    /// with `text`. Each call is its own pane, so the single-use ledger never decides it.
    async fn answer_claude_question_with(
        session_id: &str,
        text: String,
    ) -> (Arc<FixedPane>, Result<crate::scoped_contracts::DeliveryReceipt, ScopeError>) {
        let (backend, state) = pane(session_id, CLAUDE_QUESTION);
        let prompt = prompt_get(&state, session_id, Some("claude".into()))
            .await
            .expect("the screen is readable")
            .expect("the question is a prompt");
        let result = prompt_answer(
            &state,
            session_id.into(),
            prompt.prompt_id,
            prompt.screen_revision,
            crate::daemon::protocol::AgentPromptAnswerInput { key: None, text: Some(text) },
            Some("claude".into()),
        )
        .await;
        (backend, result)
    }

    fn assert_refused_untyped(
        backend: &FixedPane,
        result: Result<crate::scoped_contracts::DeliveryReceipt, ScopeError>,
    ) {
        let error = result.expect_err("text over the cap is refused");
        assert_eq!(error.code, ScopeErrorCode::InvalidRequest, "{}", error.message);
        assert_eq!(backend.write_attempts.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(backend.writes.lock().expect("writes").is_empty(), "a refused answer typed nothing");
    }

    #[tokio::test]
    async fn agent_prompt_answer_text_at_the_cap_is_typed() {
        let text = "a".repeat(policy::AGENT_CONTROL_HTTP_BODY_MAX);
        let (backend, result) = answer_claude_question_with("agent-prompt-answer-at-cap", text.clone()).await;

        result.expect("text of exactly the cap is accepted");
        let writes = backend.writes.lock().expect("writes").clone();
        assert!(writes.iter().any(|write| write == text.as_bytes()), "the whole text was typed");
    }

    #[tokio::test]
    async fn agent_prompt_answer_text_over_the_cap_is_refused_and_types_nothing() {
        let text = "a".repeat(policy::AGENT_CONTROL_HTTP_BODY_MAX + 1);
        let (backend, result) = answer_claude_question_with("agent-prompt-answer-over-cap", text).await;

        assert_refused_untyped(&backend, result);
    }

    #[tokio::test]
    async fn agent_prompt_answer_multibyte_text_over_the_byte_cap_is_refused() {
        // Three bytes a character: over the cap in bytes, well under it in characters.
        let text = "\u{AC00}".repeat(policy::AGENT_CONTROL_HTTP_BODY_MAX / 3 + 1);
        assert!(text.len() > policy::AGENT_CONTROL_HTTP_BODY_MAX);
        assert!(text.chars().count() < policy::AGENT_CONTROL_HTTP_BODY_MAX);
        let (backend, result) = answer_claude_question_with("agent-prompt-answer-multibyte", text).await;

        assert_refused_untyped(&backend, result);
    }

    #[test]
    fn agent_prompt_requests_travel_in_camel_case() {
        let request = crate::daemon::protocol::DaemonRequest::AgentPromptAnswer {
            session_id: "s".into(),
            prompt_id: "p".into(),
            screen_revision: "r".into(),
            answer: pick("1"),
            registry_id: None,
        };
        let wire = serde_json::to_value(&request).expect("wire");
        assert_eq!(wire["type"], "agentPromptAnswer", "{wire}");
        assert_eq!(wire["screenRevision"], "r", "{wire}");
        assert_eq!(wire["answer"]["key"], "1", "{wire}");
        let get: crate::daemon::protocol::DaemonRequest =
            serde_json::from_value(serde_json::json!({"type": "agentPromptGet", "sessionId": "s"}))
                .expect("a get without a registry id parses");
        assert!(matches!(get, crate::daemon::protocol::DaemonRequest::AgentPromptGet { registry_id: None, .. }));
        let view = crate::daemon::protocol::AgentPromptView {
            prompt_id: "p".into(),
            screen_revision: "r".into(),
            question: "q".into(),
            options: Vec::new(),
            kind: crate::daemon::protocol::AgentPromptViewKind::FreeText,
            needs_confirmation: false,
        };
        let wire = serde_json::to_value(&view).expect("wire");
        assert_eq!(wire["needsConfirmation"], false, "{wire}");
        assert_eq!(wire["kind"], "freeText", "{wire}");
    }
}
