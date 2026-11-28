//! Adversarial tests recovered from the independent falsification verifier
//! (`lanes/chat-g1-falsify-20261125/falsify.rs`) and kept as a permanent regression module for
//! lane G1. Every test states a behaviour the lane claims, so a regression names the claim that
//! broke.
//!
//! This is a `#[cfg(test)]` child of `store` on purpose: `SessionInner`, `SessionHandle` and
//! `validate_cwd_in` are private to `store`, and only a descendant module can build a session
//! fixture without spawning a real `codex`. The Codex JSON-RPC tests use a transport this file
//! owns, so the lane's own harness cannot hide a defect in it.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::sync::mpsc::{self, error::TryRecvError};
use tokio::sync::watch;

use super::*;
use crate::structured_chat::codex::{
    classify_frame, notification_events, turn_error_message, AppServerReader, AppServerTransport,
    AppServerWriter, CodexProvider, DeltaBuffers, InboundFrame,
};
use crate::structured_chat::types::ToolCallStatus;

// ---------------------------------------------------------------------------
// A transport of our own: scripted inbound lines, a log of everything written.
// ---------------------------------------------------------------------------

enum WireStep {
    /// Unsolicited inbound line.
    Line(String),
    /// Answers the next outbound *request* (a frame with both `id` and `method`).
    Reply(String),
    /// The child's stdout fails with this error kind on the next read.
    Fail(io::ErrorKind),
}

type InboundItem = Result<String, io::Error>;

struct Wire {
    script: StdMutex<VecDeque<WireStep>>,
    inbound: StdMutex<Option<mpsc::UnboundedSender<InboundItem>>>,
    written: mpsc::UnboundedSender<String>,
}

impl Wire {
    fn deliver(&self, item: InboundItem) {
        if let Some(sink) = self.inbound.lock().expect("sink").as_ref() {
            let _ = sink.send(item);
        }
    }

    /// Pops the next step that needs no request: a line or a stdout failure.
    fn pop_step(&self) -> Option<InboundItem> {
        let mut script = self.script.lock().expect("script");
        let unsolicited = matches!(
            script.front(),
            Some(WireStep::Line(_)) | Some(WireStep::Fail(_))
        );
        if !unsolicited {
            return None;
        }
        match script.pop_front().expect("front is unsolicited") {
            WireStep::Line(line) => Some(Ok(line)),
            WireStep::Fail(kind) => Some(Err(io::Error::new(kind, "scripted stdout failure"))),
            WireStep::Reply(_) => None,
        }
    }

    /// Pops the reply in front, if the next step is one.
    fn pop_reply(&self) -> Option<String> {
        let mut script = self.script.lock().expect("script");
        if !matches!(script.front(), Some(WireStep::Reply(_))) {
            return None;
        }
        match script.pop_front().expect("front is a reply") {
            WireStep::Reply(reply) => Some(reply),
            _ => None,
        }
    }

    /// Pushes every unsolicited step that sits in front of the next scripted reply.
    fn flush_unsolicited(&self) {
        while let Some(item) = self.pop_step() {
            self.deliver(item);
        }
    }
}

/// The transport value handed to `CodexProvider::new`. It owns the inbound receiver, which the
/// shared [`Wire`] cannot hold before the split.
struct WireTransport {
    wire: Wire,
    reader: StdMutex<Option<mpsc::UnboundedReceiver<InboundItem>>>,
}

impl AppServerTransport for WireTransport {
    fn split(self: Box<Self>) -> (Box<dyn AppServerReader>, Box<dyn AppServerWriter>) {
        let WireTransport { wire, reader } = *self;
        let reader = reader.into_inner().expect("reader lock").expect("reader taken once");
        let shared = Arc::new(wire);
        (
            Box::new(WireReader {
                rx: StdMutex::new(Some(reader)),
                wire: Arc::clone(&shared),
            }),
            Box::new(WireWriter { wire: shared }) as Box<dyn AppServerWriter>,
        )
    }
}

struct WireReader {
    rx: StdMutex<Option<mpsc::UnboundedReceiver<InboundItem>>>,
    wire: Arc<Wire>,
}

#[async_trait::async_trait]
impl AppServerReader for WireReader {
    async fn read_line(&mut self) -> io::Result<Option<String>> {
        let Some(mut rx) = self.rx.lock().expect("rx").take() else {
            return Ok(None);
        };
        let item = match rx.try_recv() {
            Ok(item) => Some(item),
            Err(TryRecvError::Disconnected) => None,
            // Nothing queued: the script may still have an unsolicited step for us, and only then
            // does this wait on the channel at all.
            Err(TryRecvError::Empty) => match self.wire.pop_step() {
                Some(item) => Some(item),
                None => rx.recv().await,
            },
        };
        *self.rx.lock().expect("rx") = Some(rx);
        match item {
            Some(Ok(line)) => Ok(Some(line)),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }
}

struct WireWriter {
    wire: Arc<Wire>,
}

#[async_trait::async_trait]
impl AppServerWriter for WireWriter {
    async fn send_line(&mut self, line: String) -> io::Result<()> {
        let _ = self.wire.written.send(line.clone());
        let is_request = serde_json::from_str::<serde_json::Value>(&line)
            .map(|frame| frame.get("id").is_some() && frame.get("method").is_some())
            .unwrap_or(false);
        // Steps that sit in front of the reply go out first, then the reply, then anything the
        // script places behind it - the order a real child writes them in.
        self.wire.flush_unsolicited();
        if is_request {
            if let Some(reply) = self.wire.pop_reply() {
                self.wire.deliver(Ok(reply));
            }
        }
        self.wire.flush_unsolicited();
        Ok(())
    }
}

struct FakeChild {
    provider: CodexProvider,
    written: mpsc::UnboundedReceiver<String>,
    events: mpsc::UnboundedReceiver<ProviderEvent>,
}

impl FakeChild {
    fn new(script: Vec<WireStep>) -> Self {
        let (written, written_rx) = mpsc::unbounded_channel();
        let (inbound, inbound_rx) = mpsc::unbounded_channel();
        let transport: Box<dyn AppServerTransport> = Box::new(WireTransport {
            wire: Wire {
                script: StdMutex::new(script.into()),
                inbound: StdMutex::new(Some(inbound)),
                written: written.clone(),
            },
            reader: StdMutex::new(Some(inbound_rx)),
        });
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        Self {
            provider: CodexProvider::new(transport, PathBuf::from("/tmp"), events_tx),
            written: written_rx,
            events: events_rx,
        }
    }

    /// Starts the provider against a scripted handshake and thread start.
    async fn started(script: Vec<WireStep>) -> Self {
        let child = Self::new(script);
        child.provider.start(None).await.expect("scripted start");
        child
    }

    fn outbound(&mut self) -> Vec<serde_json::Value> {
        let mut frames = Vec::new();
        while let Ok(line) = self.written.try_recv() {
            frames.push(serde_json::from_str(&line).expect("outbound frame is JSON"));
        }
        frames
    }

    /// The next event, or a panic after `within`: a bounded wait, never a sleep.
    async fn next_event(&mut self, within: Duration) -> ProviderEvent {
        tokio::time::timeout(within, self.events.recv())
            .await
            .expect("an event arrived in time")
            .expect("event channel open")
    }

    /// The outbound frame with `id`, waiting for the reader task when it has not written it yet.
    /// The reader answers a server request on its own task, so a frame can land after the call
    /// that caused it returns.
    async fn frame_for(&mut self, id: i64, within: Duration) -> serde_json::Value {
        loop {
            let line = tokio::time::timeout(within, self.written.recv())
                .await
                .expect("a frame arrived in time")
                .expect("writer channel open");
            let frame: serde_json::Value =
                serde_json::from_str(&line).expect("outbound frame is JSON");
            if frame["id"] == id {
                return frame;
            }
        }
    }

    /// The next reported approval request: its route id and its part kind.
    async fn next_approval(&mut self, within: Duration) -> (String, String) {
        loop {
            if let ProviderEvent::Part(PartDraft {
                payload: PartKind::ApprovalRequest { request_id, kind, .. },
                ..
            }) = self.next_event(within).await
            {
                return (request_id, kind);
            }
        }
    }
}

fn reply(id: i64, result: serde_json::Value) -> WireStep {
    WireStep::Reply(serde_json::json!({"id": id, "result": result}).to_string())
}

fn unsolicited(value: serde_json::Value) -> WireStep {
    WireStep::Line(value.to_string())
}

fn settings() -> SessionSettings {
    SessionSettings {
        model: Some("gpt-5.1-codex".into()),
        effort: Some("medium".into()),
        permission_mode: Some(PermissionMode::FullAccess),
        ..SessionSettings::default()
    }
}

/// The handshake + thread start every provider test needs.
fn handshake() -> Vec<WireStep> {
    vec![
        reply(1, serde_json::json!({})),
        reply(2, serde_json::json!({"thread": {"id": "th-1"}})),
    ]
}

// ---------------------------------------------------------------------------
// Codex JSON-RPC layer
// ---------------------------------------------------------------------------

/// Claim: "codex app-server JSON-RPC handling ... unknown methods". An unknown server *request*
/// must be refused with an error frame and the connection must stay usable.
#[tokio::test]
async fn unknown_server_request_method_is_refused_and_the_connection_survives() {
    let mut script = handshake();
    script.push(unsolicited(serde_json::json!({
        "id": 700, "method": "thread/unknown", "params": {}
    })));
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    let mut child = FakeChild::started(script).await;
    let turn = child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("the connection survived the unknown request");
    assert_eq!(turn.turn_id, "turn-1");
    let refusal = child.frame_for(700, Duration::from_secs(5)).await;
    assert!(refusal.get("method").is_none(), "the refusal is a response frame");
    assert_eq!(refusal["error"]["code"], -32601);
}

/// Claim: "notifications/requests out of order". A response nobody awaits is dropped, not fatal.
#[tokio::test]
async fn unknown_response_id_is_dropped_and_the_connection_survives() {
    let mut script = handshake();
    script.push(unsolicited(
        serde_json::json!({"id": 4242, "result": {"stale": true}}),
    ));
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    let mut child = FakeChild::started(script).await;
    let turn = child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("the connection survived the stale response");
    assert_eq!(turn.turn_id, "turn-1");
    let _ = child.outbound();
}

/// Claim: "partial lines, invalid UTF-8". One undecodable line is skipped like malformed JSON and
/// the session survives: `read_line` decodes lossily, and a decode error is not EOF.
#[tokio::test]
async fn a_non_utf8_line_is_skipped_instead_of_killing_the_session() {
    let script = vec![
        reply(1, serde_json::json!({})),
        WireStep::Fail(io::ErrorKind::InvalidData),
        reply(2, serde_json::json!({"thread": {"id": "th-1"}})),
    ];
    let child = FakeChild::new(script);
    match tokio::time::timeout(Duration::from_secs(5), child.provider.start(None)).await {
        Ok(Ok(thread)) => assert_eq!(thread, "th-1"),
        Ok(Err(error)) => panic!("one undecodable line failed the handshake: {error:?}"),
        Err(_) => panic!("one undecodable line stranded the handshake: the session died"),
    }
}

/// Claim: "huge outputs". A 4 MiB delta must survive the reader and the merge buffer intact.
#[tokio::test]
async fn a_four_megabyte_delta_is_not_truncated() {
    let delta = "x".repeat(4 * 1024 * 1024);
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    script.push(unsolicited(serde_json::json!({
        "method": "item/agentMessage/delta",
        "params": {"turnId": "turn-1", "itemId": "m1", "delta": delta},
    })));
    script.push(unsolicited(serde_json::json!({
        "method": "item/completed",
        "params": {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "m1"}},
    })));
    let mut child = FakeChild::started(script).await;
    child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("turn started");
    match child.next_event(Duration::from_secs(20)).await {
        ProviderEvent::Part(PartDraft {
            payload: PartKind::AssistantMessage { text },
            ..
        }) => assert_eq!(text.len(), 4 * 1024 * 1024, "delta was truncated"),
        other => panic!("expected the merged assistant message, got {other:?}"),
    }
}

/// Claim: "approvals round trip (approve/decline/twice/unknown id)". A second answer and an
/// unknown id are both `NotFound`; the first answer carries the protocol's decision string.
#[tokio::test]
async fn approval_answered_twice_and_unknown_id_are_not_found() {
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    script.push(unsolicited(serde_json::json!({
        "id": 901, "method": "item/fileChange/requestApproval",
        "params": {"threadId": "th-1", "turnId": "turn-1", "reason": "edit src"},
    })));
    let mut child = FakeChild::started(script).await;
    child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("turn started");
    let (request_id, kind) = child.next_approval(Duration::from_secs(5)).await;
    assert_eq!(kind, "fileChange");
    child
        .provider
        .answer_approval(&request_id, ApprovalDecision::Decline)
        .await
        .expect("first answer");
    let frames = child.outbound();
    let answer = frames
        .iter()
        .find(|frame| frame["id"] == 901 && frame.get("method").is_none())
        .expect("the answer frame");
    assert_eq!(answer["result"]["decision"], "decline");
    assert_eq!(
        child
            .provider
            .answer_approval(&request_id, ApprovalDecision::Accept)
            .await
            .unwrap_err(),
        StructuredChatError::NotFound
    );
    assert_eq!(
        child
            .provider
            .answer_approval("no-such-request", ApprovalDecision::Accept)
            .await
            .unwrap_err(),
        StructuredChatError::NotFound
    );
}

/// Claim: "approvals round trip (... after turn end)". An approval the turn already abandoned is
/// refused: `turn/completed` purges the turn's pending requests, so no response frame is written
/// for an id the server has already resolved.
#[tokio::test]
async fn approval_answered_after_the_turn_ended_is_refused() {
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    script.push(unsolicited(serde_json::json!({
        "id": 900, "method": "item/commandExecution/requestApproval",
        "params": {"threadId": "th-1", "turnId": "turn-1", "command": "rm -rf /"},
    })));
    script.push(unsolicited(serde_json::json!({
        "method": "turn/completed",
        "params": {"turn": {"id": "turn-1", "status": "interrupted"}},
    })));
    let mut child = FakeChild::started(script).await;
    child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("turn started");
    let (request_id, _) = child.next_approval(Duration::from_secs(5)).await;
    loop {
        if let ProviderEvent::TurnCompleted { .. } = child.next_event(Duration::from_secs(5)).await {
            break;
        }
    }
    assert_eq!(
        child
            .provider
            .answer_approval(&request_id, ApprovalDecision::Accept)
            .await
            .unwrap_err(),
        StructuredChatError::NotFound,
        "answering {request_id} after turn/completed was accepted: the pending map never expires"
    );
}

/// Claim (module doc): "Parts come from `item/started`, `item/completed`, ...". `item/started`
/// reports the in-flight item, so a command an interrupt kills before `item/completed` still
/// reaches history.
#[test]
fn item_started_reports_the_in_progress_item() {
    let mut buffers = DeltaBuffers::new();
    let events = notification_events(
        "item/started",
        &serde_json::json!({"turnId": "t1", "item": {"type": "commandExecution", "id": "c1",
            "command": "sleep 600", "status": "inProgress"}}),
        &mut buffers,
    );
    assert_eq!(
        events.len(),
        1,
        "item/started surfaced nothing: an interrupted command is invisible in history"
    );
    assert!(matches!(
        &events[0],
        ProviderEvent::Part(PartDraft {
            payload: PartKind::ToolCall { id, name, status, exit_code, .. },
            ..
        }) if id.as_str() == "c1" && name.as_str() == "shell" && *status == ToolCallStatus::InProgress
            && exit_code.is_none()
    ));
}

/// Claim: "notifications ... out of order". `turn/completed` before `turn/started` must not be
/// dropped and must not panic.
#[tokio::test]
async fn turn_completed_before_turn_started_is_reported_not_dropped() {
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    script.push(unsolicited(serde_json::json!({
        "method": "turn/completed",
        "params": {"turn": {"id": "turn-1", "status": "completed"}},
    })));
    script.push(unsolicited(serde_json::json!({
        "method": "turn/started", "params": {"turn": {"id": "turn-1"}},
    })));
    let mut child = FakeChild::started(script).await;
    child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("turn started");
    match child.next_event(Duration::from_secs(5)).await {
        ProviderEvent::TurnCompleted { turn_id, error } => {
            assert_eq!(turn_id, "turn-1");
            assert!(error.is_none());
        }
        other => panic!("expected TurnCompleted first, got {other:?}"),
    }
}

/// Claim: "child exit mid-turn -> turn failed with error part", provider half. When the child's
/// stdout dies mid-turn the reader fails the open turn before reporting `Ended`.
#[tokio::test]
async fn child_exit_mid_turn_reports_a_failed_turn() {
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-1"}})));
    script.push(unsolicited(serde_json::json!({
        "method": "turn/started", "params": {"turn": {"id": "turn-1"}},
    })));
    script.push(WireStep::Fail(io::ErrorKind::BrokenPipe));
    let mut child = FakeChild::started(script).await;
    child
        .provider
        .turn("hi".into(), Vec::new(), &settings())
        .await
        .expect("turn started");
    let mut completed = None;
    let mut ended = false;
    while completed.is_none() && !ended {
        match child.next_event(Duration::from_secs(5)).await {
            ProviderEvent::TurnCompleted { turn_id, error } => completed = Some((turn_id, error)),
            ProviderEvent::Ended { .. } => ended = true,
            _ => {}
        }
    }
    match completed {
        Some((turn_id, error)) => {
            assert_eq!(turn_id, "turn-1");
            assert!(
                error.is_some(),
                "the turn was reported complete with no error after the child died"
            );
        }
        None => panic!("the dead turn produced no TurnCompleted: only Ended"),
    }
}

/// Claim: "turn/completed error object (`TurnError`) surfaces its real message". `TurnError` is an
/// object, so reading it as a string flattens every failure to a generic line.
#[test]
fn turn_error_object_surfaces_its_message() {
    assert_eq!(
        turn_error_message(&serde_json::json!({
            "error": {"message": "model overloaded", "codexErrorInfo": "serverOverloaded"},
            "status": "failed",
        }))
        .as_deref(),
        Some("model overloaded")
    );
    // `additionalDetails` is the second place codex puts the human sentence.
    assert_eq!(
        turn_error_message(&serde_json::json!({
            "error": {"message": "", "additionalDetails": "try again later"},
        }))
        .as_deref(),
        Some("try again later")
    );
    assert_eq!(turn_error_message(&serde_json::json!({"status": "completed"})), None);
    assert_eq!(
        turn_error_message(&serde_json::json!({"error": {"message": ""}, "status": "failed"}))
            .as_deref(),
        Some("turn failed")
    );
}

/// Claim: "`/resume` keeps the resumed turn id". `thread/queue/start` answers with the turn it
/// resumed; without reporting it the resumed turn never enters `turn_ids` and cannot be steered
/// or interrupted.
#[tokio::test]
async fn resume_reports_the_turn_it_continued() {
    let mut script = handshake();
    script.push(reply(3, serde_json::json!({"turn": {"id": "turn-9"}})));
    let mut child = FakeChild::started(script).await;
    child.provider.resume().await.expect("resume");
    match child.next_event(Duration::from_secs(5)).await {
        ProviderEvent::TurnStarted { turn_id } => assert_eq!(turn_id, "turn-9"),
        other => panic!("resume reported no started turn, got {other:?}"),
    }
}

/// Claim: "resume 409 is typed, not substring-based". An empty queue is a JSON-RPC
/// invalid-request code; the message text is irrelevant, and no pinned codex source contains the
/// prose the old check matched on.
#[tokio::test]
async fn resume_conflict_reads_the_code_not_the_message() {
    let mut script = handshake();
    script.push(WireStep::Reply(
        serde_json::json!({
            "id": 3,
            "error": {"code": -32600, "message": "no queued submission for this thread"},
        })
        .to_string(),
    ));
    let mut child = FakeChild::started(script).await;
    assert_eq!(
        child.provider.resume().await.unwrap_err(),
        StructuredChatError::Conflict(
            "RESUME_NOTHING_TO_CONTINUE: no stopped run is queued for this thread".into()
        )
    );
    let frames = child.outbound();
    assert_eq!(frames[3]["method"], "thread/queue/start");
    assert_eq!(frames[3]["params"]["threadId"], "th-1");
}

/// The same claim from the other side: a provider failure that is not a state conflict stays a
/// provider failure, so a real outage is never reported as "nothing to continue".
#[tokio::test]
async fn resume_provider_failure_stays_a_provider_failure() {
    let mut script = handshake();
    script.push(WireStep::Reply(
        serde_json::json!({
            "id": 3,
            "error": {"code": -32603, "message": "queue is empty"},
        })
        .to_string(),
    ));
    let child = FakeChild::started(script).await;
    let error = child.provider.resume().await.unwrap_err();
    assert_eq!(error.code(), "PROVIDER_FAILED", "a -32603 is not a state conflict");
}

/// Cheap guard on the classifier the reader depends on: an error body is a response, not a
/// request, and a line with a BOM is malformed.
#[test]
fn frame_classifier_keeps_error_responses_out_of_the_request_branch() {
    assert!(matches!(
        classify_frame("{\"id\":7,\"error\":{\"code\":-32600,\"message\":\"queue is empty\"}}"),
        InboundFrame::Response { result: Err(_), .. }
    ));
    assert!(matches!(classify_frame("\u{feff}{}"), InboundFrame::Malformed));
}

// ---------------------------------------------------------------------------
// Store layer: a scripted provider this file owns
// ---------------------------------------------------------------------------

struct ProbeProvider {
    events: mpsc::UnboundedSender<ProviderEvent>,
    calls: Arc<StdMutex<Vec<String>>>,
    seen: Arc<StdMutex<Option<SessionSettings>>>,
    gate: StdMutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    turns: AtomicU32,
    complete: bool,
    on_turn: Vec<ProviderEvent>,
}

impl ProbeProvider {
    fn new(calls: Arc<StdMutex<Vec<String>>>) -> (Self, mpsc::UnboundedReceiver<ProviderEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        (
            Self {
                events,
                calls,
                seen: Arc::new(StdMutex::new(None)),
                gate: StdMutex::new(None),
                turns: AtomicU32::new(0),
                complete: true,
                on_turn: Vec::new(),
            },
            rx,
        )
    }

    fn emit(&self, event: ProviderEvent) {
        let _ = self.events.send(event);
    }
}

#[async_trait::async_trait]
impl StructuredProvider for ProbeProvider {
    fn provider_id(&self) -> &'static str {
        "probe"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: true,
            rollback: true,
            resume: true,
            revert_files: false,
            history_paging: true,
            permission_modes: PermissionMode::all().to_vec(),
            models: Vec::new(),
            slash_commands: Vec::new(),
            ..Capabilities::default()
        }
    }

    async fn start(&self, resume_thread: Option<String>) -> Result<String, StructuredChatError> {
        Ok(resume_thread.unwrap_or_else(|| "th-probe".into()))
    }

    async fn turn(
        &self,
        text: String,
        _attachments: Vec<String>,
        settings: &SessionSettings,
    ) -> Result<crate::structured_chat::TurnRef, StructuredChatError> {
        *self.seen.lock().expect("seen") = Some(settings.clone());
        let gate = self.gate.lock().expect("gate").take();
        if let Some(gate) = gate {
            let _ = gate.await;
        }
        let n = self.turns.fetch_add(1, Ordering::SeqCst) + 1;
        let turn_id = format!("t{n}");
        self.calls
            .lock()
            .expect("calls")
            .push(format!("turn:{text}"));
        for event in &self.on_turn {
            self.emit(event.clone());
        }
        if self.complete {
            self.emit(ProviderEvent::TurnCompleted {
                turn_id: turn_id.clone(),
                error: None,
            });
        }
        Ok(crate::structured_chat::TurnRef { turn_id })
    }

    async fn steer(&self, expected_turn: &str, text: String) -> Result<(), StructuredChatError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("steer:{expected_turn}:{text}"));
        Ok(())
    }

    async fn interrupt(&self, turn_id: &str) -> Result<(), StructuredChatError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("interrupt:{turn_id}"));
        Ok(())
    }

    async fn rollback(&self, before_turn_id: &str) -> Result<String, StructuredChatError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("rollback:{before_turn_id}"));
        // The probe thread is rewound in place, so the id it speaks for is unchanged.
        Ok("th-probe".into())
    }

    async fn answer_approval(
        &self,
        request_id: &str,
        _decision: ApprovalDecision,
    ) -> Result<(), StructuredChatError> {
        self.calls
            .lock()
            .expect("calls")
            .push(format!("approval:{request_id}"));
        Ok(())
    }

    async fn clear(&self) -> Result<String, StructuredChatError> {
        self.calls.lock().expect("calls").push("clear".into());
        Ok("th-probe-2".into())
    }

    async fn compact(&self) -> Result<(), StructuredChatError> {
        Ok(())
    }

    async fn resume(&self) -> Result<(), StructuredChatError> {
        self.calls.lock().expect("calls").push("resume".into());
        Ok(())
    }

    async fn close(&self) {}

    fn event_sink(&self) -> mpsc::UnboundedSender<ProviderEvent> {
        self.events.clone()
    }
}

fn store_in(temp: &Path) -> Arc<StructuredChatStore> {
    Arc::new(StructuredChatStore {
        sessions: RwLock::new(HashMap::new()),
        journal_dir: temp.to_path_buf(),
    })
}

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("ferryx-falsify-{}", uuid::Uuid::new_v4()))
}

async fn session_with(
    store: &StructuredChatStore,
    temp: &Path,
    session_id: &str,
    provider: ProbeProvider,
    events_rx: mpsc::UnboundedReceiver<ProviderEvent>,
) -> Arc<SessionHandle> {
    let journal_path = temp.join(format!("{session_id}.jsonl"));
    let handle = Arc::new(SessionHandle {
        session_id: session_id.to_owned(),
        provider_id: "probe".into(),
        cwd: temp.to_path_buf(),
        inner: tokio::sync::Mutex::new(SessionInner {
            provider: Some(Arc::new(provider)),
            thread_id: Some("th-probe".into()),
            settings: SessionSettings {
                model: None,
                effort: None,
                permission_mode: Some(PermissionMode::FullAccess),
                ..SessionSettings::default()
            },
            state: SessionState::Running,
            parts: Vec::new(),
            published_seq: 0,
            parts_loaded: true,
            next_seq: 1,
            turn_ids: Vec::new(),
            turn_snapshots: HashMap::new(),
            thread_first_turn: 0,
            turn_starting: false,
            active_turn: None,
            last_completed_turn: None,
            events_tx: watch::channel(0).0,
            journal_path: journal_path.clone(),
        }),
        journal: tokio::sync::Mutex::new(SessionJournal::new(journal_path)),
    });
    spawn_event_pump(Arc::clone(&handle), events_rx);
    store
        .sessions
        .write()
        .await
        .insert(session_id.to_owned(), Arc::clone(&handle));
    handle
}

/// Bounded spin: yields until `ready` holds. Never a sleep, and never unbounded.
async fn until_async<F, Fut>(within: Duration, mut ready: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + within;
    while !ready().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition never held within {within:?}"
        );
        tokio::task::yield_now().await;
    }
}

async fn parts_of(store: &StructuredChatStore, id: &str) -> Vec<StructuredPart> {
    store
        .history(id, &HistoryQuery::default())
        .await
        .expect("history")
        .parts
}

/// Waits until the session row reports no in-flight turn. A provider that finishes a turn in the
/// same call that starts it can leave `active_turn` set until its `TurnCompleted` is pumped, so a
/// rollback test must observe the idle state instead of assuming it.
async fn until_idle(store: &StructuredChatStore, id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let idle = store
            .list_sessions()
            .await
            .into_iter()
            .find(|row| row.session_id == id)
            .is_some_and(|row| row.active_turn_id.is_none());
        if idle {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session {id} never went idle"
        );
        tokio::task::yield_now().await;
    }
}

async fn state_of(store: &StructuredChatStore, id: &str) -> Option<SessionState> {
    store
        .list_sessions()
        .await
        .into_iter()
        .find(|row| row.session_id == id)
        .map(|row| row.state)
}

fn turn(text: &str) -> TurnBody {
    TurnBody {
        text: text.into(),
        attachments: None,
    }
}

/// Claim: "child exit mid-turn -> turn failed with error part", store half. The provider's `Ended`
/// event fails the turn still in flight, so history records why it stopped instead of ending on a
/// user message.
#[tokio::test]
async fn child_exit_mid_turn_leaves_a_failed_turn_part() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (mut provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    provider.complete = false;
    provider.on_turn = vec![
        ProviderEvent::Part(PartDraft {
            turn_id: Some("t1".into()),
            payload: PartKind::AssistantMessage {
                text: "partial".into(),
            },
        }),
        ProviderEvent::Ended {
            reason: "app-server closed its stdout".into(),
        },
    ];
    session_with(&store, &temp, "s-exit", provider, rx).await;
    store.start_turn("s-exit", &turn("go")).await.expect("turn");

    // The pump handles events in order, so `ended` means the `Ended` event behind the assistant
    // part has been handled too.
    let store_ref = Arc::clone(&store);
    until_async(Duration::from_secs(5), || {
        let store = Arc::clone(&store_ref);
        async move { state_of(&store, "s-exit").await == Some(SessionState::Ended) }
    })
    .await;
    let parts = parts_of(&store, "s-exit").await;
    assert!(
        parts
            .iter()
            .any(|part| matches!(part.payload, PartKind::Error { .. })),
        "the dead turn left no Error part: history is {:?}",
        parts.iter().map(|p| &p.payload).collect::<Vec<_>>()
    );
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "path/cwd jail for session cwd". A session may only run under a trusted root: the
/// filesystem root and system directories are refused, and so is anything outside the roots.
#[test]
fn cwd_jail_refuses_root_and_system_directories() {
    let home = PathBuf::from("/home/tester");
    for candidate in ["/", "/etc", "/usr"] {
        assert!(
            validate_cwd_in(candidate, std::slice::from_ref(&home)).is_err(),
            "{candidate} is accepted as a session cwd"
        );
    }
    // A path outside every allowed root is refused even when it exists.
    let outside = scratch_dir();
    std::fs::create_dir_all(&outside).expect("scratch");
    assert!(
        validate_cwd_in(outside.to_str().expect("utf8"), &[home]).is_err(),
        "a directory outside every allowed root is accepted"
    );
    let _ = std::fs::remove_dir_all(&outside);
}

/// Claim: "rollback (... rollback while running)". A rollback while a turn is in flight is
/// refused: it would revert the thread the provider is still writing to.
#[tokio::test]
async fn rollback_is_refused_while_a_turn_is_running() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (mut provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    provider.complete = false;
    session_with(&store, &temp, "s-run", provider, rx).await;
    store.start_turn("s-run", &turn("long")).await.expect("turn");
    let refused = store.rollback("s-run", 1).await;
    assert!(
        matches!(refused, Err(StructuredChatError::Conflict(_))),
        "rollback ran mid-turn (provider saw {:?}) instead of a typed 409",
        calls.lock().expect("calls")
    );
    assert!(
        calls.lock().expect("calls").iter().all(|call| !call.starts_with("rollback:")),
        "the provider was asked to revert a running thread"
    );
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "rollback (seq strictly increasing, marker part)" plus `/clear` as a divider. `/clear`
/// keeps the previous thread's turns, so a later rollback takes back only the current thread and
/// the pre-clear transcript survives.
#[tokio::test]
async fn rollback_after_clear_keeps_the_previous_thread() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    session_with(&store, &temp, "s-clear", provider, rx).await;
    store
        .start_turn("s-clear", &turn("first thread"))
        .await
        .expect("first turn");
    store.clear("s-clear").await.expect("clear");
    store
        .start_turn("s-clear", &turn("second thread"))
        .await
        .expect("second turn");
    until_idle(&store, "s-clear").await;
    store.rollback("s-clear", 1).await.expect("rollback");
    let parts = parts_of(&store, "s-clear").await;
    assert!(
        parts.iter().any(|part| matches!(
            &part.payload,
            PartKind::UserMessage { text } if text == "first thread"
        )),
        "rollback after /clear deleted the previous thread: history is {:?}",
        parts.iter().map(|p| &p.payload).collect::<Vec<_>>()
    );
    assert!(
        !parts.iter().any(|part| matches!(
            &part.payload,
            PartKind::UserMessage { text } if text == "second thread"
        )),
        "the rollback kept the turn it was asked to take back: history is {:?}",
        parts.iter().map(|p| &p.payload).collect::<Vec<_>>()
    );
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "rollback beyond history". `turns` past the current thread's turn count and `turns == 0`
/// are typed invalid requests, not a panic or a silent no-op.
#[tokio::test]
async fn rollback_beyond_history_is_a_typed_invalid_request() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    session_with(&store, &temp, "s-over", provider, rx).await;
    store
        .start_turn("s-over", &turn("only"))
        .await
        .expect("turn");
    until_idle(&store, "s-over").await;
    assert!(matches!(
        store.rollback("s-over", 5).await,
        Err(StructuredChatError::InvalidRequest(_))
    ));
    assert!(matches!(
        store.rollback("s-over", 0).await,
        Err(StructuredChatError::InvalidRequest(_))
    ));
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "history paging (cursor/limit, hasOlder)". Walking `before` with a small `limit` covers
/// the journal exactly once, oldest to newest, and `hasOlder` is false only on the oldest page.
#[tokio::test]
async fn history_pages_cover_every_part_exactly_once() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    let handle = session_with(&store, &temp, "s-page", provider, rx).await;
    {
        let mut inner = handle.inner.lock().await;
        for seq in 1..=25u64 {
            inner.parts.push(StructuredPart {
                seq,
                turn_id: None,
                ts_ms: seq,
                payload: PartKind::UserMessage {
                    text: format!("p{seq}"),
                },
            });
        }
        inner.next_seq = 26;
        inner.published_seq = 25;
    }

    let mut seen: Vec<u64> = Vec::new();
    let mut before = None;
    for page_number in 0..10 {
        let page = store
            .history(
                "s-page",
                &HistoryQuery {
                    after: None,
                    before,
                    limit: Some(10),
                },
            )
            .await
            .expect("page");
        assert!(page.parts.len() <= 10, "page exceeded its limit");
        let mut seqs: Vec<u64> = page.parts.iter().map(|part| part.seq).collect();
        seqs.extend(seen);
        seen = seqs;
        if !page.has_older {
            break;
        }
        assert!(page_number < 9, "paging never terminated");
        before = page.next_before;
        assert!(before.is_some(), "hasOlder with no cursor to walk back");
    }
    assert_eq!(seen, (1..=25u64).collect::<Vec<_>>());
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "session isolation across concurrent sessions". Two sessions keep separate history,
/// separate sequence numbering, and separate rows.
#[tokio::test]
async fn concurrent_sessions_do_not_share_history_or_seq() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    for (id, text) in [("s-a", "alpha"), ("s-b", "beta")] {
        let calls = Arc::new(StdMutex::new(Vec::new()));
        let (mut provider, rx) = ProbeProvider::new(calls);
        // No provider parts: the user message is then the only part, so the assertions read the
        // store's own numbering instead of racing the event pump.
        provider.complete = false;
        session_with(&store, &temp, id, provider, rx).await;
        store.start_turn(id, &turn(text)).await.expect("turn");
    }
    let a = parts_of(&store, "s-a").await;
    let b = parts_of(&store, "s-b").await;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].seq, 1);
    assert_eq!(b[0].seq, 1);
    assert!(matches!(&a[0].payload, PartKind::UserMessage { text } if text == "alpha"));
    assert!(matches!(&b[0].payload, PartKind::UserMessage { text } if text == "beta"));
    assert_eq!(store.list_sessions().await.len(), 2);
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "GET history ... GET events (long-poll)". A read of a session must not wait for that
/// session's provider round trip: `start_turn` reserves the turn under the lock and then releases
/// it, so history keeps serving while the provider is still starting the turn.
#[tokio::test]
async fn history_is_not_blocked_by_an_in_flight_turn_start() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    let seen = Arc::clone(&provider.seen);
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel();
    *provider.gate.lock().expect("gate") = Some(gate_rx);
    session_with(&store, &temp, "s-block", provider, rx).await;

    let starter = {
        let store = Arc::clone(&store);
        tokio::spawn(async move { store.start_turn("s-block", &turn("slow")).await })
    };
    // `seen` is written inside `turn()`, which runs after the turn is reserved, so observing it
    // proves the provider round trip is in flight.
    let seen_ref = Arc::clone(&seen);
    until_async(Duration::from_secs(5), || {
        let seen = Arc::clone(&seen_ref);
        async move { seen.lock().expect("seen").is_some() }
    })
    .await;
    let read = tokio::time::timeout(
        Duration::from_secs(2),
        store.history("s-block", &HistoryQuery::default()),
    )
    .await;
    let _ = gate_tx.send(());
    let _ = starter.await;
    assert!(
        read.is_ok(),
        "history blocked behind the provider round trip of start_turn"
    );
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// Claim: "settings (model/effort/permission) propagation". The settings a client writes reach
/// the provider's next turn verbatim.
#[tokio::test]
async fn settings_reach_the_next_turn() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    let seen = Arc::clone(&provider.seen);
    session_with(&store, &temp, "s-set", provider, rx).await;
    store
        .update_settings(
            "s-set",
            &SessionSettings {
                model: Some("gpt-6-sol".into()),
                effort: Some("high".into()),
                permission_mode: Some(PermissionMode::Supervised),
                ..SessionSettings::default()
            },
        )
        .await
        .expect("settings");
    store.start_turn("s-set", &turn("go")).await.expect("turn");
    let applied = seen.lock().expect("seen").clone().expect("settings seen");
    assert_eq!(applied.model.as_deref(), Some("gpt-6-sol"));
    assert_eq!(applied.effort.as_deref(), Some("high"));
    assert_eq!(applied.permission_mode, Some(PermissionMode::Supervised));
    let _ = tokio::fs::remove_dir_all(&temp).await;
}

/// The contract addition: `SessionSummary` carries the in-flight turn, so the composer enables
/// Stop from the session row instead of guessing from history. Absent while idle, so a session
/// row keeps its five-field shape.
#[tokio::test]
async fn session_summary_carries_the_in_flight_turn() {
    let temp = scratch_dir();
    tokio::fs::create_dir_all(&temp).await.expect("scratch");
    let store = store_in(&temp);
    let calls = Arc::new(StdMutex::new(Vec::new()));
    let (mut provider, rx) = ProbeProvider::new(Arc::clone(&calls));
    provider.complete = false;
    let handle = session_with(&store, &temp, "s-active", provider, rx).await;

    let idle = serde_json::to_value(store.list_sessions().await.remove(0)).expect("summary");
    assert!(
        idle.get("activeTurnId").is_none(),
        "an idle session reported an in-flight turn: {idle}"
    );
    assert_eq!(idle.as_object().expect("object").len(), 5);

    store
        .start_turn("s-active", &turn("go"))
        .await
        .expect("turn");
    let active = serde_json::to_value(store.list_sessions().await.remove(0)).expect("summary");
    assert_eq!(active["activeTurnId"], "t1");
    assert_eq!(active["state"], "running");

    // The turn completes: the field goes away again.
    handle.inner.lock().await.active_turn = None;
    let done = serde_json::to_value(store.list_sessions().await.remove(0)).expect("summary");
    assert!(done.get("activeTurnId").is_none(), "a finished turn stayed in flight: {done}");
    let _ = tokio::fs::remove_dir_all(&temp).await;
}
