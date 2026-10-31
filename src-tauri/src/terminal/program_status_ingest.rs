//! OSC 7501 ingest for the daemon's screen mirror.
//!
//! The mirror is the only engine that opts into program status, and only for sessions this daemon
//! answers for (local PTYs and SSH bridge sessions; a paired proxy never does, the peer daemon
//! answers there). Reports land in the mirror's [`ProgramStatusRecords`]; the support query reply
//! is captured by the mirror and handed back to the output pump, which writes it to the session's
//! input outside every hub lock. Summary changes go to the agent state hub through
//! [`ProgramStatusPublisher`], also after the locks are released.

use crate::native_terminal::{
    ProgramStatusEvent, ProgramStatusKind, ProgramStatusReport as NativeReport, ProgramStatusState,
};
use crate::terminal::program_status::{
    BlockedKind, ProgramState, ProgramStatusRecords, ProgramStatusReport, ProgramStatusSummary,
};

/// What one live feed of the mirror produced for program status.
#[derive(Debug, Default)]
pub struct ProgramStatusIngest {
    /// OSC 7501 support query replies to write to the session's input, oldest first.
    pub replies: Vec<Vec<u8>>,
    /// `Some` when the session's summary changed; the inner `None` means no record is left.
    pub summary: Option<Option<ProgramStatusSummary>>,
}

/// Receives a session's program status summary whenever it changes. Installed once at daemon
/// startup; called without any output hub or screen lock held.
pub(crate) trait ProgramStatusPublisher: Send + Sync {
    fn publish(&self, session_id: &str, summary: Option<&ProgramStatusSummary>);
}

impl ProgramStatusPublisher for crate::daemon::agent_state::AgentStateHub {
    fn publish(&self, session_id: &str, summary: Option<&ProgramStatusSummary>) {
        self.publish_protocol(session_id, summary);
    }
}

/// Applies one parser event to the record store: a report is applied, a prompt start drops
/// transient records and a full reset clears every record.
pub(crate) fn apply_event(records: &mut ProgramStatusRecords, event: ProgramStatusEvent) {
    match event {
        ProgramStatusEvent::Report(report) => records.apply(owned_report(report)),
        ProgramStatusEvent::PromptStart(_) => records.prompt_started(),
        ProgramStatusEvent::Reset => records.apply(ProgramStatusReport {
            id: String::new(),
            state: ProgramState::Clear,
            kind: None,
            app: None,
            title: None,
            message: None,
            progress: None,
        }),
    }
}

fn owned_report(report: NativeReport) -> ProgramStatusReport {
    ProgramStatusReport {
        id: report.id,
        state: match report.state {
            ProgramStatusState::Idle => ProgramState::Idle,
            ProgramStatusState::Working => ProgramState::Working,
            ProgramStatusState::Done => ProgramState::Done,
            ProgramStatusState::Blocked => ProgramState::Blocked,
            ProgramStatusState::Error => ProgramState::Error,
            ProgramStatusState::Clear => ProgramState::Clear,
        },
        kind: report.kind.map(|kind| match kind {
            ProgramStatusKind::Permission => BlockedKind::Permission,
            ProgramStatusKind::Question => BlockedKind::Question,
            ProgramStatusKind::Auth => BlockedKind::Auth,
        }),
        app: Some(report.app).filter(|app| !app.is_empty()),
        title: Some(report.title).filter(|title| !title.is_empty()),
        message: Some(report.message).filter(|message| !message.is_empty()),
        progress: report.progress.and_then(|value| i8::try_from(value).ok()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::daemon::agent_state::AgentStateHub;
    use crate::daemon::protocol::{AgentOutcome, AgentStateOrigin, DaemonRequest};
    use crate::terminal::output_hub::TerminalOutputHub;
    use crate::terminal::program_status::ProgramState;
    use crate::terminal::screen_mirror::{ScreenMirror, ScreenSeed};

    use super::ProgramStatusPublisher;

    const QUERY: &[u8] = b"\x1b]7501;?\x1b\\";
    const QUERY_REPLY: &[u8] = b"\x1b]7501;?:states=idle,working,done,blocked,error\
:kinds=permission,question,auth\x1b\\";
    /// `msg` is base64 for "Running tests".
    const WORKING_REPORT: &[u8] =
        b"\x1b]7501;state=working:app=claude:msg=UnVubmluZyB0ZXN0cw==\x1b\\";
    const DONE_REPORT: &[u8] = b"\x1b]7501;state=done:app=claude\x1b\\";
    const PROMPT_START: &[u8] = b"\x1b]133;A\x07";

    /// A local session's hub wired to a real agent state hub, as the daemon wires it at startup.
    fn answering_hub() -> (TerminalOutputHub, Arc<AgentStateHub>) {
        let hub = TerminalOutputHub::new(64 * 1024);
        let states = Arc::new(AgentStateHub::default());
        let publisher: Arc<dyn ProgramStatusPublisher> = states.clone();
        hub.set_program_status_publisher(publisher);
        hub.register_session("s");
        assert!(hub.enable_program_status("s"));
        (hub, states)
    }

    fn replies(hub: &TerminalOutputHub, bytes: &[u8]) -> Vec<Vec<u8>> {
        hub.publish_with_replies("s", bytes.to_vec(), None).expect("session").1
    }

    #[test]
    fn program_status_query_gets_exactly_one_reply() {
        let (hub, _states) = answering_hub();
        // A device attributes query in the same chunk stays suppressed: only OSC 7501 passes.
        let mut chunk = b"\x1b[c".to_vec();
        chunk.extend_from_slice(QUERY);
        assert_eq!(replies(&hub, &chunk), vec![QUERY_REPLY.to_vec()]);
        assert!(replies(&hub, b"plain output\r\n").is_empty(), "a reply is handed out once");

        let mut two = QUERY.to_vec();
        two.extend_from_slice(QUERY);
        assert_eq!(replies(&hub, &two).len(), 2, "each query gets its own reply");
    }

    #[test]
    fn program_status_seed_replay_yields_no_reply_and_no_report() {
        let mut replayed = WORKING_REPORT.to_vec();
        replayed.extend_from_slice(QUERY);

        let mut mirror = ScreenMirror::new(80, 24).unwrap();
        assert!(mirror.enable_program_status());
        mirror.feed_replay(&replayed);
        let ingest = mirror.take_program_status();
        assert!(ingest.replies.is_empty());
        assert_eq!(ingest.summary, None);

        // A seeded mirror opts in after its seed, as import does: the seed is never ingested.
        let mut seeded =
            ScreenMirror::from_seed(&ScreenSeed { cols: 80, rows: 24, bytes: replayed }).unwrap();
        assert!(seeded.enable_program_status());
        let ingest = seeded.take_program_status();
        assert!(ingest.replies.is_empty());
        assert_eq!(ingest.summary, None);

        // The replay left the mirror live: the next real report is ingested.
        mirror.feed(WORKING_REPORT);
        let summary = mirror.take_program_status().summary.flatten().expect("live report");
        assert_eq!(summary.state, ProgramState::Working);
    }

    #[test]
    fn program_status_working_report_reaches_summary() {
        let (hub, states) = answering_hub();
        assert!(replies(&hub, WORKING_REPORT).is_empty());
        let state = states.current("s").expect("published");
        assert_eq!(state.state, "working");
        assert_eq!(state.origin, AgentStateOrigin::Protocol);
        assert_eq!(state.agent.as_deref(), Some("claude"));
    }

    #[test]
    fn program_status_prompt_start_drops_working() {
        let (hub, states) = answering_hub();
        replies(&hub, WORKING_REPORT);
        replies(&hub, PROMPT_START);
        let state = states.current("s").expect("published");
        assert_eq!(state.state, "idle");
        assert_eq!(state.origin, AgentStateOrigin::Protocol);
    }

    /// A paired proxy hub (and any session that did not opt in) never answers the query.
    #[test]
    fn program_status_paired_hub_never_replies() {
        let hub = std::sync::Arc::new(TerminalOutputHub::new(64 * 1024));
        let proxy = crate::terminal::paired_daemon::Proxy::new(
            crate::terminal::paired_daemon::Descriptor {
                host_id: "host-key".into(),
                generation: crate::scoped_contracts::Epoch(1),
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id: "m".into(),
                    daemon_epoch: crate::scoped_contracts::Epoch(1),
                    session_id: "s".into(),
                },
                after_sequence: None,
            },
            hub.clone(),
        )
        .unwrap();
        let id = proxy.id().to_string();
        let (_chunk, replies) = hub.publish_with_replies(&id, QUERY.to_vec(), None).unwrap();
        assert!(replies.is_empty());
        assert_eq!(hub.acknowledge_program_outcome(&id), None);
    }

    #[test]
    fn program_status_acknowledge_clears_done() {
        let (hub, states) = answering_hub();
        replies(&hub, DONE_REPORT);
        let done = states.current("s").expect("published");
        assert_eq!((done.state.as_str(), done.outcome), ("idle", Some(AgentOutcome::Done)));

        // What the daemon's acknowledgeAgentOutcome handler does.
        let summary = hub.acknowledge_program_outcome("s").expect("opted in");
        assert_eq!(summary, None);
        states.publish_protocol("s", summary.as_ref());
        assert_eq!(states.current("s").expect("published").outcome, None);
    }

    #[test]
    fn program_status_release_prunes_working() {
        let (hub, states) = answering_hub();
        replies(&hub, WORKING_REPORT);
        // What the foreground release edge does.
        let summary = hub.program_exited("s").expect("opted in");
        assert_eq!(summary, None);
        states.publish_protocol("s", summary.as_ref());
        assert_eq!(states.current("s").expect("published").state, "idle");
    }

    #[test]
    fn program_status_acknowledge_request_wire_shape() {
        let request: DaemonRequest =
            serde_json::from_str(r#"{"type":"acknowledgeAgentOutcome","sessionId":"s"}"#).unwrap();
        assert!(
            matches!(request, DaemonRequest::AcknowledgeAgentOutcome { ref session_id } if session_id == "s")
        );
    }
}
