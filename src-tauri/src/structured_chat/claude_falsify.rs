//! Independent falsification tests for lane G2's Claude provider (`super::claude`).
//!
//! These are written by a verifier, not the lane author, and live only in the scratch tree.
//! Each test states the behaviour the CLI/SDK and t3code (`ClaudeAdapterV2.ts`) actually
//! require, so a failing test names a defect: the provider is under test unchanged.
//!
//! Where a claim could be settled against the installed CLI (`claude` 2.1.309) the measured
//! wire fact is quoted in the test that rests on it.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{mpsc, Notify};

use super::claude::{ChildClaude, ClaudeProvider, ClaudeTransport, Launcher};
use super::types::{PartKind, PermissionMode, SessionSettings, ToolCallStatus};
use super::{PartDraft, ProviderEvent, StructuredProvider};

// ---------------------------------------------------------------------------
// Scripted in-process child
// ---------------------------------------------------------------------------

/// One scripted child: a queue of inbound lines, plus everything the provider wrote.
struct FakeClaude {
    inbound: Mutex<VecDeque<String>>,
    closed: AtomicBool,
    wake: Notify,
    written: mpsc::UnboundedSender<String>,
}

impl FakeClaude {
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
}

#[async_trait::async_trait]
impl ClaudeTransport for FakeClaude {
    async fn send_line(&self, line: String) -> io::Result<()> {
        // Answer host→CLI control requests the way a cooperative CLI does, so no test waits on
        // the provider's 120 s control timeout.
        if let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(&line) {
            if frame.get("type").and_then(Value::as_str) == Some("control_request") {
                let reply = json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "success",
                        "request_id": frame.get("request_id").cloned().unwrap_or(Value::Null),
                        "response": {},
                    },
                });
                self.inbound.lock().unwrap().push_back(reply.to_string());
                self.wake.notify_one();
            }
        }
        let _ = self.written.send(line);
        Ok(())
    }

    async fn read_line(&self) -> io::Result<Option<String>> {
        loop {
            let wake = self.wake.notified();
            {
                let mut inbound = self.inbound.lock().unwrap();
                if let Some(line) = inbound.pop_front() {
                    return Ok(Some(line));
                }
                if self.closed.load(Ordering::SeqCst) {
                    return Ok(None);
                }
            }
            wake.await;
        }
    }
}

struct Link {
    child: Arc<FakeClaude>,
    written: mpsc::UnboundedReceiver<String>,
}

struct Harness {
    provider: ClaudeProvider<FakeClaude>,
    events: mpsc::UnboundedReceiver<ProviderEvent>,
    links: Arc<Mutex<Vec<Link>>>,
    argvs: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Harness {
    fn new(settings: SessionSettings) -> Self {
        let links: Arc<Mutex<Vec<Link>>> = Arc::new(Mutex::new(Vec::new()));
        let argvs: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded_links = Arc::clone(&links);
        let recorded_argvs = Arc::clone(&argvs);
        let launch: Launcher<FakeClaude> = Arc::new(move |argv, _cwd| {
            let (written_tx, written_rx) = mpsc::unbounded_channel();
            recorded_argvs.lock().unwrap().push(argv.to_vec());
            let child = Arc::new(FakeClaude {
                inbound: Mutex::new(VecDeque::new()),
                closed: AtomicBool::new(false),
                wake: Notify::new(),
                written: written_tx,
            });
            recorded_links.lock().unwrap().push(Link {
                child: Arc::clone(&child),
                written: written_rx,
            });
            Ok(child)
        });
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        let provider =
            ClaudeProvider::with_launcher(PathBuf::from("/tmp"), events_tx, &settings, launch);
        Self {
            provider,
            events: events_rx,
            links,
            argvs,
        }
    }

    fn script(&self, lines: Vec<String>) {
        let links = self.links.lock().unwrap();
        let link = links.last().expect("a child was spawned");
        {
            let mut inbound = link.child.inbound.lock().unwrap();
            for line in lines {
                inbound.push_back(line);
            }
        }
        link.child.wake.notify_one();
    }

    /// Ends the newest child's stdout, as a crash or a killed process does.
    fn close_child(&self) {
        let links = self.links.lock().unwrap();
        links.last().expect("a child was spawned").child.close();
    }

    fn argvs(&self) -> Vec<Vec<String>> {
        self.argvs.lock().unwrap().clone()
    }

    fn last_argv(&self) -> Vec<String> {
        self.argvs().pop().expect("a child was spawned")
    }

    /// Everything the newest child was sent, parsed as JSON.
    fn outbound(&self) -> Vec<Value> {
        let mut links = self.links.lock().unwrap();
        let link = links.last_mut().expect("a child was spawned");
        let mut frames = Vec::new();
        while let Ok(line) = link.written.try_recv() {
            frames.push(serde_json::from_str(&line).expect("outbound frame is JSON"));
        }
        frames
    }

    /// Drains events until the child reports `Ended` or `budget` elapses. Every assertion below
    /// is on the resulting set, never on when a member arrived, so the budget cannot decide the
    /// outcome: a missing event is missing for the whole budget.
    async fn collect(&mut self, budget: Duration) -> Vec<ProviderEvent> {
        let deadline = tokio::time::Instant::now() + budget;
        let mut out = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                break;
            }
            match tokio::time::timeout(left, self.events.recv()).await {
                Ok(Some(event)) => {
                    let stop = matches!(event, ProviderEvent::Ended { .. });
                    out.push(event);
                    if stop {
                        break;
                    }
                }
                _ => break,
            }
        }
        out
    }
}

fn settings(mode: PermissionMode) -> SessionSettings {
    SessionSettings {
        model: None,
        effort: None,
        permission_mode: Some(mode),
        ..SessionSettings::default()
    }
}

fn arg_value(argv: &[String], flag: &str) -> Option<String> {
    argv.iter()
        .position(|arg| arg == flag)
        .and_then(|index| argv.get(index + 1))
        .cloned()
}

fn has_flag(argv: &[String], flag: &str) -> bool {
    argv.iter().any(|arg| arg == flag)
}

fn assistant_frame(message_id: &str, text: &str, uuid: &str) -> String {
    json!({
        "type": "assistant",
        "message": {"id": message_id, "content": [{"type": "text", "text": text}]},
        "parent_tool_use_id": null,
        "uuid": uuid,
    })
    .to_string()
}

fn success_result_frame() -> String {
    json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "ok",
        "usage": {"input_tokens": 3, "output_tokens": 2},
    })
    .to_string()
}

/// A segment result that echoes the prompt uuid it answered, as the CLI does. t3code holds a
/// completed result while a steer is pending and the result echoes a prompt
/// (`ClaudeAdapterV2.ts:12638-12649`, `claudeEchoedPromptUuids` at `:5026-5033`) instead of
/// finishing the turn there.
fn segment_result_frame(echoed_prompt_uuid: &str) -> String {
    json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "ok",
        "usage": {"input_tokens": 3, "output_tokens": 2},
        "user_message_uuids": [echoed_prompt_uuid],
    })
    .to_string()
}

fn assistant_parts(events: &[ProviderEvent]) -> Vec<(Option<String>, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Part(PartDraft {
                turn_id,
                payload: PartKind::AssistantMessage { text },
            }) => Some((turn_id.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

fn completed_turns(events: &[ProviderEvent]) -> Vec<(String, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::TurnCompleted { turn_id, error } => {
                Some((turn_id.clone(), error.clone()))
            }
            _ => None,
        })
        .collect()
}

const BUDGET: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// DEFECT 1 — a live switch to full access is refused by the CLI
// ---------------------------------------------------------------------------

/// Measured on `claude` 2.1.309: a process launched without
/// `--allow-dangerously-skip-permissions` answers
/// `set_permission_mode {"mode":"bypassPermissions"}` with
/// `{"subtype":"error","error":"Cannot set permission mode to bypassPermissions because the
/// session was not launched with --dangerously-skip-permissions","error_code":
/// "bypass_not_launched"}`, and accepts the same request once that flag was on the launch argv.
///
/// t3code therefore keys process reuse on the permission mode
/// (`claudeRuntimeQueryPolicyKey` includes `permissionMode`, `ClaudeAdapterV2.ts:3918-3926`):
/// a mode change is a relaunch, not a `set_permission_mode`. `claude::turn` sends the control
/// request instead (`claude.rs:1305-1319`) while `launch_argv` adds the skip flag only when the
/// *launch* mode was `FullAccess` (`claude.rs:305-309`), so a session opened supervised and
/// escalated to full access cannot run a turn at all.
#[tokio::test]
async fn full_access_after_a_narrower_launch_requires_a_relaunch() {
    let harness = Harness::new(settings(PermissionMode::Supervised));
    harness.provider.start(None).await.expect("start");
    assert!(
        !has_flag(&harness.last_argv(), "--allow-dangerously-skip-permissions"),
        "precondition: a supervised launch carries no skip flag"
    );

    harness
        .provider
        .turn("escalate".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("a full-access turn on a supervised session");

    let argvs = harness.argvs();
    assert!(
        argvs
            .iter()
            .any(|argv| has_flag(argv, "--allow-dangerously-skip-permissions")),
        "no CLI process was launched with --allow-dangerously-skip-permissions, so the live \
         set_permission_mode bypassPermissions this provider sent is the one the CLI refuses \
         with bypass_not_launched; launched argvs: {argvs:?}"
    );
}

// ---------------------------------------------------------------------------
// DEFECT 2 — a steered turn ends at the first segment result
// ---------------------------------------------------------------------------

/// The SDK reports one `result` per steered segment, and a result echoes the prompt it answered
/// (`user_message_uuids`). t3code holds such a result while a steer is pending and absorbs its
/// usage into `steeredSegmentUsage` (`ClaudeAdapterV2.ts:12638-12649`, `:9780-9790`, `:12527`),
/// then bills every segment of the one turn.
///
/// `claude::run_reader` treats *every* `result` as the end of the turn
/// (`claude.rs:1042-1046,1064-1076`): it emits `TurnCompleted` and clears the tracker, so the
/// segment after a steer is published with no turn id and the store sees the turn finish
/// mid-flight.
#[tokio::test]
async fn a_steer_result_does_not_end_the_turn_early() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    let turn = harness
        .provider
        .turn("first".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    harness.provider.steer(&turn.turn_id, "second".into()).await.expect("steer");

    // Segment one, then the segment the steer produced: two results, one turn, each echoing the
    // prompt it answered.
    harness.script(vec![
        assistant_frame("m1", "one", "u1"),
        segment_result_frame("prompt-1"),
        assistant_frame("m2", "two", "u2"),
        segment_result_frame("prompt-2"),
    ]);
    let events = harness.collect(BUDGET).await;

    let completions = completed_turns(&events);
    assert_eq!(
        completions.len(),
        1,
        "a steered turn produced {} TurnCompleted events; the set is {completions:?}",
        completions.len()
    );
    let texts = assistant_parts(&events);
    let second = texts
        .iter()
        .find(|(_, text)| text == "two")
        .unwrap_or_else(|| panic!("the steered segment's text is missing: {texts:?}"));
    assert_eq!(
        second.0.as_deref(),
        Some(turn.turn_id.as_str()),
        "the steered segment's part lost its turn id, so the store cannot attribute it"
    );
}

// ---------------------------------------------------------------------------
// DEFECT 3 — a file change never leaves `inProgress`
// ---------------------------------------------------------------------------

/// `claude::message_parts` emits a `FileChange` with `InProgress` for an Edit/Write/MultiEdit
/// `tool_use` (`claude.rs:645-666`) and, on the matching `tool_result`, only a `ToolCall`
/// (`claude.rs:501-536`). Parts are append-only, so the file change stays `inProgress` in the
/// history forever; t3code moves the `file_change` item to its terminal status when the result
/// lands (`ClaudeAdapterV2.ts:4755-4787` is the approval path, the tool result path in
/// `ClaudeAdapterV2.ts:11455+` completes the item).
#[tokio::test]
async fn a_file_change_completes_when_its_tool_result_arrives() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("edit".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");

    let tool_use = json!({
        "type": "assistant",
        "message": {
            "id": "m1",
            "content": [{
                "type": "tool_use", "id": "tu1", "name": "Edit",
                "input": {"file_path": "/tmp/x.rs", "old_string": "a", "new_string": "b"},
            }],
        },
        "parent_tool_use_id": null,
        "uuid": "u1",
    })
    .to_string();
    let tool_result = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{
                "type": "tool_result", "tool_use_id": "tu1",
                "content": [{"type": "text", "text": "edited"}],
            }],
        },
        "parent_tool_use_id": null,
        "uuid": "u2",
    })
    .to_string();
    harness.script(vec![tool_use, tool_result, success_result_frame()]);
    let events = harness.collect(BUDGET).await;

    let statuses: Vec<(String, ToolCallStatus)> = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Part(PartDraft {
                payload: PartKind::FileChange { id, status, .. },
                ..
            }) => Some((id.clone(), status.clone())),
            _ => None,
        })
        .collect();
    assert!(
        statuses
            .iter()
            .any(|(id, status)| id == "tu1" && *status != ToolCallStatus::InProgress),
        "the file change for tu1 never reached a terminal status; emitted: {statuses:?}"
    );
}

// ---------------------------------------------------------------------------
// DEFECT 4 — one non-UTF-8 byte kills the session
// ---------------------------------------------------------------------------

/// The reader takes `String` lines from `ChildClaude::read_line` (`claude.rs:237-245`), whose
/// `BufReader::read_line` fails with `InvalidData` on the first invalid byte; `run_reader`
/// treats any read error as end of stream and reports `Ended` (`claude.rs:963-968`). The SDK
/// decodes with a `StringDecoder` and keeps going, so a resumed transcript containing a byte
/// the host cannot decode ends a whole Ferryx session.
///
/// The child stays alive after its frames (`sleep 30`), so `!ended` measures the bad
/// byte and nothing else: a correct reader skips that one line and keeps reading, while the
/// `InvalidData` defect ends the stream at the 0xFF and the assistant frame behind it is never
/// seen. Without that the child's own exit would end the session either way and the assertion
/// would pass for the wrong reason. `sleep` rather than a redirect back to `/dev/null`: a
/// redirect closes the shell's copy of the write end and ends the stream itself.
#[cfg(unix)]
#[tokio::test]
async fn a_line_with_invalid_utf8_does_not_end_the_session() {
    // A real child, `/bin/sh`: it waits for the turn line, then writes an init frame, one line
    // holding the single byte 0xFF (no UTF-8 decoder accepts it), and a normal assistant frame.
    let script = concat!(
        "read -r _; ",
        r#"printf '%s\n' '{"type":"system","subtype":"init","session_id":"s"}'; "#,
        r#"printf '\377\n'; "#,
        r#"printf '%s\n' '{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"after the bad byte"}]},"parent_tool_use_id":null,"uuid":"u1"}'; "#,
        r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"output_tokens":1}}'; "#,
        // Stay alive holding the write end of the pipe open: `!ended` must measure the bad byte,
        // not this script running out of lines. `sleep` keeps fd 1 untouched — a redirect would
        // close the only copy of the write end and end the stream itself.
        "sleep 30",
    );
    let launch: Launcher<ChildClaude> = Arc::new(move |_argv, cwd| {
        let argv = vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()];
        ChildClaude::spawn(&argv, cwd).map(Arc::new)
    });
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let provider = ClaudeProvider::with_launcher(
        std::env::temp_dir(),
        events_tx,
        &settings(PermissionMode::FullAccess),
        launch,
    );
    provider.start(None).await.expect("start");
    provider
        .turn("hi".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");

    let deadline = tokio::time::Instant::now() + BUDGET;
    let mut saw_text = false;
    let mut ended = false;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, events_rx.recv()).await {
            Ok(Some(ProviderEvent::Part(PartDraft {
                payload: PartKind::AssistantMessage { text },
                ..
            }))) => saw_text = saw_text || text == "after the bad byte",
            Ok(Some(ProviderEvent::Ended { .. })) => {
                ended = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    assert!(
        saw_text && !ended,
        "a single 0xFF byte ended the session (saw_text={saw_text}, ended={ended}) instead of \
         being skipped like any other undecodable line"
    );
}

// ---------------------------------------------------------------------------
// Witness — the first-turn rollback's new session id never reaches the store
// ---------------------------------------------------------------------------

/// `claude::rollback` rewinds past the first turn by opening a *fresh* `--session-id`
/// (`claude.rs`, the `_ => self.open(None)` arm), which is right, and t3code returns the reset
/// native thread id in the provider thread it hands back (`ClaudeAdapterV2.ts:15435`
/// `nativeThreadId: resetNativeThreadId`).
///
/// The defect this pins was that `StructuredProvider::rollback` returned `()`, so the store kept
/// the journal's old `thread_id` and a later gateway restart resumed the discarded conversation
/// through `ensure_running`. The assertion is that the provider hands back the id the fresh
/// process speaks for: that is exactly the value the store now writes to the meta record (see
/// `store::tests::rollback_persists_the_thread_id_the_provider_reopened`).
#[tokio::test]
async fn rollback_of_the_first_turn_opens_a_session_id_the_store_cannot_learn() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    let thread_id = harness.provider.start(None).await.expect("start");
    assert_eq!(
        arg_value(&harness.last_argv(), "--session-id").as_deref(),
        Some(thread_id.as_str())
    );

    let turn = harness
        .provider
        .turn("one".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    harness.script(vec![assistant_frame("m1", "one", "u1"), success_result_frame()]);
    let events = harness.collect(BUDGET).await;
    assert_eq!(completed_turns(&events).len(), 1);

    let reopened_id = harness
        .provider
        .rollback(&turn.turn_id)
        .await
        .expect("rollback");
    let reopened = harness.last_argv();
    let fresh = arg_value(&reopened, "--session-id").expect("a fresh session id");
    assert_ne!(
        fresh, thread_id,
        "rewinding past the first turn must open a new session"
    );
    assert_eq!(
        reopened_id, fresh,
        "rollback must hand back the id the fresh process speaks for, or the store cannot learn it"
    );

    harness
        .provider
        .turn("two".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn after rollback");
    let frame = harness.outbound().into_iter().next().expect("a user frame");
    assert_eq!(
        frame["session_id"].as_str(),
        Some(fresh.as_str()),
        "the live process speaks for {fresh}, which is the id rollback returned"
    );
    assert_ne!(fresh, thread_id);
}

// ---------------------------------------------------------------------------
// DEFECT 5 — a second snapshot of one assistant message is dropped whole
// ---------------------------------------------------------------------------

/// The CLI streams one assistant message as several `assistant` frames that all carry the same
/// `message.id` and a growing content list: the text block, then the `tool_use` block appended.
/// t3code keys per-block identity on `message.message.id` plus the block index
/// (`snapshotBlockIndex`, `ClaudeAdapterV2.ts:5405-5410`, ids `${message.id}:text:${index}`) and
/// emits only blocks it has not emitted yet.
///
/// `claude::message_parts` keeps a `seen_messages` set of `message.id`
/// (`claude.rs:437`, `:589-595`) and returns early for every later frame, so the tool call that
/// only appears in the second snapshot is lost. Its `tool_result` then lands as a completed call
/// with no matching start, and the file change it should have produced never exists.
#[tokio::test]
async fn a_tool_call_arriving_in_a_later_snapshot_of_the_same_message_is_kept() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("edit".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");

    // Frame one: the message's text. Frame two: the same message, now with the tool call.
    let first = json!({
        "type": "assistant",
        "message": {"id": "msg_1", "content": [{"type": "text", "text": "let me edit"}]},
        "parent_tool_use_id": null,
        "uuid": "u1",
    })
    .to_string();
    let second = json!({
        "type": "assistant",
        "message": {
            "id": "msg_1",
            "content": [
                {"type": "text", "text": "let me edit"},
                {"type": "tool_use", "id": "tu1", "name": "Edit",
                 "input": {"file_path": "/tmp/x.rs", "old_string": "a", "new_string": "b"}},
            ],
        },
        "parent_tool_use_id": null,
        "uuid": "u2",
    })
    .to_string();
    harness.script(vec![first, second, success_result_frame()]);
    let events = harness.collect(BUDGET).await;

    let calls: Vec<(String, String, ToolCallStatus)> = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { id, name, status, .. },
                ..
            }) => Some((id.clone(), name.clone(), status.clone())),
            _ => None,
        })
        .collect();
    assert!(
        calls
            .iter()
            .any(|(id, _, status)| id == "tu1" && *status == ToolCallStatus::InProgress),
        "the tool call in the second snapshot of msg_1 was dropped, so the history has a result \
         with no start; tool calls emitted: {calls:?}"
    );
    assert!(
        assistant_parts(&events)
            .iter()
            .filter(|(_, text)| text == "let me edit")
            .count()
            == 1,
        "the already-emitted text block must not be repeated: {:?}",
        assistant_parts(&events)
    );
}

// ---------------------------------------------------------------------------
// DEFECT 6 — a denial is reported as a second, invented call
// ---------------------------------------------------------------------------

/// `result.permission_denials` names the `tool_use_id` the CLI refused, and t3code never reads
/// the field (zero hits in `t3src/`): the refusal reaches the transcript through the ordinary
/// approval/`tool_result` path, so the call that was actually shown to the user is the one that
/// ends up declined.
///
/// `claude::run_reader` invents `denied-{index}-{tool_name}` parts (`claude.rs:1052-1062`) and
/// leaves the real call `InProgress`, so the history grows a duplicate call the model never made
/// while the real one never finishes.
#[tokio::test]
async fn a_denied_call_finishes_the_call_it_names() {
    let mut harness = Harness::new(settings(PermissionMode::Supervised));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("run".into(), Vec::new(), &settings(PermissionMode::Supervised))
        .await
        .expect("turn");

    let tool_use = json!({
        "type": "assistant",
        "message": {"id": "m1", "content": [
            {"type": "tool_use", "id": "tu9", "name": "Bash", "input": {"command": "ls"}},
        ]},
        "parent_tool_use_id": null,
        "uuid": "u1",
    })
    .to_string();
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "ok",
        "usage": {"input_tokens": 1, "output_tokens": 1},
        "permission_denials": [
            {"tool_name": "Bash", "tool_use_id": "tu9", "tool_input": {"command": "ls"}},
        ],
    })
    .to_string();
    harness.script(vec![tool_use, result]);
    let events = harness.collect(BUDGET).await;

    let calls: Vec<(String, String, ToolCallStatus)> = events
        .iter()
        .filter_map(|event| match event {
            ProviderEvent::Part(PartDraft {
                payload: PartKind::ToolCall { id, name, status, .. },
                ..
            }) => Some((id.clone(), name.clone(), status.clone())),
            _ => None,
        })
        .collect();
    let invented: Vec<&(String, String, ToolCallStatus)> = calls
        .iter()
        .filter(|(id, _, _)| id != "tu9")
        .collect();
    assert!(
        invented.is_empty(),
        "the denial invented a call the model never made: {invented:?}"
    );
    assert!(
        calls
            .iter()
            .any(|(id, _, status)| id == "tu9" && *status != ToolCallStatus::InProgress),
        "the denied call tu9 is still InProgress, so the user sees a call that never ends: {calls:?}"
    );
}

// ---------------------------------------------------------------------------
// DEFECT 7 — a child that dies mid-turn is reported as a clean end
// ---------------------------------------------------------------------------

/// The lane's own brief requires a session that loses its CLI to surface an error part and a
/// failed turn; t3code maps `process_killed_by_signal` / `process_exited_nonzero` to a failed
/// turn with `providerProcessExitGuidance(stderrTail)` as the message
/// (`ClaudeAdapterV2.ts:1956`, `:241`).
///
/// `claude::run_reader` reports only `Ended` when the read side closes
/// (`claude.rs:1078-1084`), and `store::pump` turns that into `SessionState::Ended` with no
/// part and no `TurnCompleted` (`store.rs:713-719`), so a crashed turn looks like a finished
/// one to every reader of the journal.
#[tokio::test]
async fn a_child_dying_mid_turn_fails_the_turn() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("die".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    harness.script(vec![assistant_frame("m1", "half an answer", "u1")]);
    harness.close_child();
    let events = harness.collect(BUDGET).await;

    let failures = completed_turns(&events)
        .into_iter()
        .filter(|(_, error)| error.is_some())
        .count();
    let error_parts = events
        .iter()
        .filter(|event| matches!(event, ProviderEvent::Part(PartDraft { payload: PartKind::Error { .. }, .. })))
        .count();
    assert!(
        failures + error_parts > 0,
        "the child died mid-turn and the stream said nothing: completions {:?}, error parts {}",
        completed_turns(&events),
        error_parts
    );
}

// ---------------------------------------------------------------------------
// Claims that survive the same treatment
// ---------------------------------------------------------------------------

/// `ClaudeTurnTokenUsage.ts`: input is uncached input plus cache reads plus cache creations,
/// output is `output_tokens`, and the window comes from `modelUsage[*].contextWindow`.
#[tokio::test]
async fn usage_sums_the_billed_fields_and_reports_the_window() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("count".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "result": "ok",
        "usage": {
            "input_tokens": 5,
            "cache_read_input_tokens": 7,
            "cache_creation_input_tokens": 11,
            "output_tokens": 13,
        },
        "modelUsage": {"claude-sonnet-4-6": {"contextWindow": 200000}},
    })
    .to_string();
    harness.script(vec![result]);
    let events = harness.collect(BUDGET).await;

    let usage = events
        .iter()
        .find(|event| {
            matches!(
                event,
                ProviderEvent::Part(PartDraft {
                    payload: PartKind::Usage { .. },
                    ..
                })
            )
        })
        .expect("a usage part");
    let ProviderEvent::Part(PartDraft {
        payload:
            PartKind::Usage {
                total_tokens,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                context_window,
            },
        ..
    }) = usage
    else {
        unreachable!()
    };
    assert_eq!((*input_tokens, *cached_input_tokens, *output_tokens), (23, 7, 13));
    assert_eq!(*total_tokens, 36);
    assert_eq!(*context_window, Some(200_000));
}

/// One bad line must not take the connection down (`classify_frame`), including a JSON array, an
/// unknown event type and a truncated object.
#[tokio::test]
async fn malformed_and_unknown_frames_do_not_kill_the_connection() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("junk".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    harness.script(vec![
        "[1,2,3]".to_string(),
        json!({"type": "command_lifecycle", "command_uuid": "c", "state": "cancelled"})
            .to_string(),
        "{\"type\":\"assistant\",\"message\":{\"id\"".to_string(),
        json!({"type": "system", "subtype": "status", "status": "requesting"}).to_string(),
        assistant_frame("m1", "survived", "u1"),
        success_result_frame(),
    ]);
    let events = harness.collect(BUDGET).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Ended { .. })),
        "a malformed line ended the connection"
    );
    assert_eq!(assistant_parts(&events).len(), 1);
    assert_eq!(completed_turns(&events).len(), 1);
}

/// A single very long line is one frame, not a truncated part: the CLI emits whole assistant
/// messages on one line and `-p` transcripts routinely exceed a megabyte.
#[tokio::test]
async fn a_huge_single_line_assistant_frame_survives_intact() {
    let mut harness = Harness::new(settings(PermissionMode::FullAccess));
    harness.provider.start(None).await.expect("start");
    harness
        .provider
        .turn("big".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    let big = "x".repeat(1024 * 1024);
    harness.script(vec![
        assistant_frame("m1", &big, "u1"),
        success_result_frame(),
    ]);
    let events = harness.collect(BUDGET).await;
    let parts = assistant_parts(&events);
    assert_eq!(parts.len(), 1, "one frame must be one part: {parts:?}");
    assert_eq!(parts[0].1.len(), big.len());
}

/// Two providers in one gateway share nothing: separate children, separate trackers, separate
/// parts. The store keeps every session's events in its own channel; this pins the provider half.
#[tokio::test]
async fn two_providers_keep_their_streams_separate() {
    let mut first = Harness::new(settings(PermissionMode::FullAccess));
    let mut second = Harness::new(settings(PermissionMode::FullAccess));
    first.provider.start(None).await.expect("start");
    second.provider.start(None).await.expect("start");
    let turn = first
        .provider
        .turn("a".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");
    let other = second
        .provider
        .turn("b".into(), Vec::new(), &settings(PermissionMode::FullAccess))
        .await
        .expect("turn");

    first.script(vec![assistant_frame("m1", "from first", "u1"), success_result_frame()]);
    second.script(vec![assistant_frame("m2", "from second", "u2"), success_result_frame()]);
    let first_events = first.collect(BUDGET).await;
    let second_events = second.collect(BUDGET).await;

    assert_eq!(
        assistant_parts(&first_events),
        vec![(Some(turn.turn_id.clone()), "from first".to_owned())]
    );
    assert_eq!(
        assistant_parts(&second_events),
        vec![(Some(other.turn_id.clone()), "from second".to_owned())],
        "the second provider must attribute its part to its own turn, not the first's"
    );
    assert_ne!(turn.turn_id, other.turn_id);
}
