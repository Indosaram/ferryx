//! OSC 7501 ingest for the daemon's screen mirror.
//!
//! The mirror is the only engine that opts into program status, and only for sessions this daemon
//! answers for (local PTYs and SSH bridge sessions; a paired proxy never does, the peer daemon
//! answers there). Reports land in the mirror's [`ProgramStatusRecords`]; the support query reply
//! is captured by the mirror and handed back to the output pump, which writes it to the session's
//! input outside every hub lock. Summary changes go to the agent state hub through
//! [`ProgramStatusPublisher`], also after the locks are released. Because of that, two changes can
//! reach the publisher in either order; each carries a [`ProgramStatusChange::revision`] taken
//! under the mirror's lock so the publisher keeps only the newest.

use std::sync::atomic::{AtomicU64, Ordering};

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
    /// `Some` when the session's summary changed.
    pub summary: Option<ProgramStatusChange>,
    /// The revision a `working`, `blocked` or `idle` record applied by this feed was stamped
    /// with, whether or not it changed the summary.
    pub transient_applied: Option<u64>,
}

/// One change of a session's program status summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusChange {
    /// Increases with every change of every mirror in this process, so a later change of a
    /// session (even from a mirror that replaced the first one) always carries a larger value.
    pub revision: u64,
    /// `None` means no record is left.
    pub summary: Option<ProgramStatusSummary>,
}

/// Source of [`ProgramStatusChange::revision`]. Taken while the mirror's lock is held, so the
/// order of one session's revisions is the order its changes happened in.
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

/// Receives a session's program status summary whenever it changes. Installed once at daemon
/// startup; called without any output hub or screen lock held, so calls for one session may
/// arrive out of order: the implementation must drop a change older than one it already applied.
pub(crate) trait ProgramStatusPublisher: Send + Sync {
    fn publish(&self, session_id: &str, change: &ProgramStatusChange);

    /// A feed applied a transient record under `revision`, delivered before that feed's summary
    /// change, if any. A record that did not change the summary (a repeated report, or one
    /// hidden behind the root record) still has a program that must be seen gone before an exit
    /// may release it, so its revision must reach whoever decides that.
    fn transient_applied(&self, _session_id: &str, _revision: u64) {}
}

impl ProgramStatusPublisher for crate::daemon::agent_state::AgentStateHub {
    fn publish(&self, session_id: &str, change: &ProgramStatusChange) {
        self.publish_protocol(session_id, change.revision, change.summary.as_ref());
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

    use super::{ProgramStatusChange, ProgramStatusPublisher};

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
        let summary = mirror
            .take_program_status()
            .summary
            .and_then(|change| change.summary)
            .expect("live report");
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
        // No record is left: the program claims nothing, so screen inference is re-armed.
        assert_eq!(state.origin, AgentStateOrigin::ProcessObserved);
        assert!(!state.origin.claims_activity());
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

        // What the daemon's acknowledgeAgentOutcome handler does; the hub publishes itself.
        assert_eq!(hub.acknowledge_program_outcome("s").expect("opted in"), None);
        assert_eq!(states.current("s").expect("published").outcome, None);
    }

    #[test]
    fn program_status_release_prunes_working() {
        let (hub, states) = answering_hub();
        replies(&hub, WORKING_REPORT);
        // What the foreground release edge does; the hub publishes itself.
        assert_eq!(hub.program_exited("s", u64::MAX).expect("opted in"), None);
        let state = states.current("s").expect("published");
        assert_eq!(state.state, "idle");
        assert!(!state.origin.claims_activity());
    }

    /// Finding 3: two summary changes are numbered under the mirror's lock but published after
    /// it is released, so they can reach the publisher reversed. The older one must not win:
    /// the mirror deduplicates against its newest summary and would never re-report it.
    #[test]
    fn program_status_changes_published_out_of_order_keep_the_newest() {
        let mut mirror = ScreenMirror::new(80, 24).unwrap();
        assert!(mirror.enable_program_status());
        mirror.feed(WORKING_REPORT);
        let working = mirror.take_program_status().summary.expect("working change");
        mirror.feed(PROMPT_START);
        let cleared = mirror.take_program_status().summary.expect("cleared change");
        assert!(cleared.revision > working.revision);
        assert_eq!(cleared.summary, None);

        let states = AgentStateHub::default();
        // The pump that took `working` is preempted; the one that took `cleared` publishes first.
        states.publish("s", &cleared);
        assert!(states.current("s").is_none(), "nothing the protocol installed to release");
        // The stale working change arriving last is dropped instead of pinning `working`.
        states.publish("s", &working);
        assert!(states.current("s").is_none());

        // In order, the same two changes report working, then release it.
        let ordered = AgentStateHub::default();
        ordered.publish("s", &working);
        assert_eq!(ordered.current("s").expect("working").state, "working");
        ordered.publish("s", &cleared);
        let released = ordered.current("s").expect("released");
        assert_eq!(released.state, "idle");
        assert!(!released.origin.claims_activity());
    }

    /// Acknowledge and exit go through the hub's publisher with a revision taken under the
    /// mirror lock, the same path as live feeds, so they are ordered with them.
    #[test]
    fn program_status_lifecycle_updates_are_revisioned_through_the_publisher() {
        #[derive(Default)]
        struct Recorder(parking_lot::Mutex<Vec<ProgramStatusChange>>);
        impl ProgramStatusPublisher for Recorder {
            fn publish(&self, _session_id: &str, change: &ProgramStatusChange) {
                self.0.lock().push(change.clone());
            }
        }
        let hub = TerminalOutputHub::new(64 * 1024);
        let recorder = Arc::new(Recorder::default());
        let publisher: Arc<dyn ProgramStatusPublisher> = recorder.clone();
        hub.set_program_status_publisher(publisher);
        hub.register_session("s");
        assert!(hub.enable_program_status("s"));
        replies(&hub, DONE_REPORT);
        assert!(
            hub.program_exited("s", u64::MAX).expect("opted in").is_some(),
            "done survives exit"
        );
        assert_eq!(hub.acknowledge_program_outcome("s").expect("opted in"), None);
        assert_eq!(hub.acknowledge_program_outcome("s").expect("opted in"), None);
        let changes = recorder.0.lock().clone();
        // The exit and the second acknowledgement changed nothing and publish nothing.
        assert_eq!(changes.len(), 2);
        assert!(changes[0].revision < changes[1].revision);
        assert_eq!(changes[0].summary.as_ref().map(|s| s.state), Some(ProgramState::Done));
        assert_eq!(changes[1].summary, None);
    }

    /// The exit decision is bound to the newest revision read before its process listing was
    /// requested. A report parsed after that, even one that did not change the summary and so
    /// never reached the reporters, may come from a program the listing never saw: the mirror
    /// release drops only the records applied by the bound, under the mirror lock.
    #[test]
    fn program_status_exit_keeps_a_report_applied_after_its_bound() {
        #[derive(Default)]
        struct Recorder(parking_lot::Mutex<Vec<ProgramStatusChange>>);
        impl ProgramStatusPublisher for Recorder {
            fn publish(&self, _session_id: &str, change: &ProgramStatusChange) {
                self.0.lock().push(change.clone());
            }
        }
        let hub = TerminalOutputHub::new(64 * 1024);
        let recorder = Arc::new(Recorder::default());
        let publisher: Arc<dyn ProgramStatusPublisher> = recorder.clone();
        hub.set_program_status_publisher(publisher);
        hub.register_session("s");
        assert!(hub.enable_program_status("s"));
        replies(&hub, WORKING_REPORT);
        let bound = recorder.0.lock().last().expect("working published").revision;
        // The root record still wins the summary, so this report publishes nothing.
        replies(&hub, b"\x1b]7501;state=working:id=late:app=make\x1b\\");
        assert_eq!(recorder.0.lock().len(), 1);

        let left = hub.program_exited("s", bound).expect("opted in").expect("late kept");
        assert_eq!(left.state, ProgramState::Working);
        assert_eq!(left.app.as_deref(), Some("make"));
        // Evidence gathered after the late report releases it too.
        let newest = recorder.0.lock().last().expect("blocked published").revision;
        assert!(newest > bound);
        assert_eq!(hub.program_exited("s", newest).expect("opted in"), None);
    }

    /// A repeated report changes no summary, so it is never published, yet it re-stamps its
    /// record. When it lands while an exit listing is in flight, that exit keeps the record; the
    /// reporter entry must stay pending under the repeat's revision so the next listing that
    /// proves its program gone releases it. A report applied after that listing was requested
    /// is still never released by it.
    #[test]
    fn program_status_exit_keeps_a_repeated_report_pending_until_a_later_listing() {
        use crate::terminal::foreground::{Foreground, ProtocolReporters};

        /// The daemon's relay, minus the agent state hub: ConPTY, so there is no group.
        struct Relay(Arc<ProtocolReporters>);
        impl ProgramStatusPublisher for Relay {
            fn publish(&self, session_id: &str, change: &ProgramStatusChange) {
                let transient = change.summary.as_ref().is_some_and(|summary| {
                    matches!(
                        summary.state,
                        ProgramState::Working | ProgramState::Blocked | ProgramState::Idle
                    )
                });
                self.0.reported(session_id, change.revision, transient, None);
            }
            fn transient_applied(&self, session_id: &str, revision: u64) {
                self.0.transient_applied(session_id, revision, None);
            }
        }
        let hub = TerminalOutputHub::new(64 * 1024);
        let reporters = Arc::new(ProtocolReporters::default());
        let publisher: Arc<dyn ProgramStatusPublisher> = Arc::new(Relay(Arc::clone(&reporters)));
        hub.set_program_status_publisher(publisher);
        hub.register_session("s");
        assert!(hub.enable_program_status("s"));
        // The bound an observer tick reads before it requests its listing.
        let requested = || reporters.newest("s").max(hub.program_status_revision("s"));
        // A tick whose listing shows the shell alone and no Released edge: as in `server.rs`,
        // records are dropped only when the pending report is released.
        let lone_shell = |requested: u64| {
            let released = reporters.foreground_returned(
                "s",
                Some(Foreground::Shell),
                None,
                Some(requested),
                || None,
            )?;
            hub.program_exited("s", requested.max(released))
        };

        replies(&hub, WORKING_REPORT);
        let bound = requested();
        // The agent's listing is in flight; it repeats its report, changing no summary.
        replies(&hub, WORKING_REPORT);
        // That listing ends in the agent's Released edge, which drops what its bound covers.
        let shell = Some(Foreground::Shell);
        let released = reporters.foreground_returned("s", shell, None, Some(bound), || None);
        let kept = hub
            .program_exited("s", bound.max(released.unwrap_or(0)))
            .expect("opted in")
            .expect("the repeat was applied after the listing was requested");
        assert_eq!(kept.state, ProgramState::Working);

        // The next listing is in flight when a newer repeat lands: it must not release it.
        let bound = requested();
        replies(&hub, WORKING_REPORT);
        assert_eq!(lone_shell(bound), None, "a report newer than the listing stays pending");

        // A listing requested after the newest report proves its program gone and releases it.
        assert_eq!(lone_shell(requested()), Some(None));
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
