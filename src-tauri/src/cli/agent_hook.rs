//! `ferryx agent-hook <agent> <event>`: the reporter hook-style agents run on each lifecycle hook.
//!
//! The agent pipes its hook payload (JSON) to stdin. The reporter maps the event to the activity
//! state the bundled omo extension reports and sends the same newline-delimited `agentState` line
//! (`resources/agent-extensions/ferryx-agent-state.ts`) to the daemon's ingress: the unix socket in
//! `FERRYX_AGENT_STATE_SOCKET`, or loopback TCP on `FERRYX_AGENT_STATE_PORT` with `FERRYX_AGENT_STATE_TOKEN`.
//!
//! Every failure is silent and exits 0: a hook that errors surfaces in the agent's own UI, and a
//! pane Ferryx did not spawn (no `FERRYX_SESSION_ID`) has nothing to report to.

use serde_json::{Map, Value};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::time::Duration;

/// The CLI word that selects this mode: `ferryx agent-hook <agent> <event>`.
pub const AGENT_HOOK_SUBCOMMAND: &str = "agent-hook";

/// Upper bound on the payload read from stdin; a Stop payload carries the last assistant message.
const MAX_PAYLOAD_BYTES: u64 = 16 * 1024 * 1024;
/// Upper bound on the blocked text shown on an inbox row.
const MAX_DETAIL_CHARS: usize = 500;
/// The extension gives up on a stalled receiver after one second; so does the reporter.
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

/// The hook-style agents this bridge reports for, each with the hook events Ferryx installs.
///
/// Event names are the ones measured in `.omo-parity/facts/`: claude and codex in `local-1.md`,
/// grok in `remote-grok.md` section 5, cline in `local-3.md`, antigravity in `local-2.md`,
/// crush in `remote-crush.md` section 4a and cursor in `cursor-hooks-live-2026-10-23.md`.
/// Claude's `Notification` is left out on purpose: its
/// `permission_prompt` copy arrives about five seconds after `PermissionRequest`, so an approval
/// given in between would be re-blocked by the late copy. Codex never fired `Notification` or
/// `SessionEnd`.
pub const HOOK_AGENTS: [(&str, &[&str]); 7] = [
    (
        "claude",
        &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
            "SessionEnd",
        ],
    ),
    (
        "codex",
        &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
        ],
    ),
    // Grok has no permission event: the approval screen is announced by `Notification` with
    // `notificationType == "permission_prompt"`, which `payload_reports` keys on.
    (
        "grok",
        &[
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Notification",
            "Stop",
            "StopFailure",
            "StopCancelled",
            "SessionEnd",
        ],
    ),
    // Cline fires `TaskStart` per turn at prompt submit, and `TaskResume` in its place on the
    // first turn after `--id`. Its `PreToolUse` runs before the approval prompt, so cline has no
    // blocked edge either (local-3.md); that stays with screen inference.
    (
        "cline",
        &["TaskStart", "TaskResume", "PreToolUse", "PostToolUse", "TaskComplete", "TaskError"],
    ),
    // Only these three were observed firing in the live agy probe (local-2.md). `PostInvocation`
    // is a real event name but never fired, and the tool events need a tool run, so neither is
    // installed until one is measured.
    ("antigravity", &["SessionStart", "PreInvocation", "Stop"]),
    // Crush's only hook event (`internal/hooks/hooks.go:14-16`). It is the working edge and the
    // provider-session capture; idle and blocked stay with screen inference.
    ("crush", &["PreToolUse"]),
    // cursor-agent fired these, in this order, in a logged-in turn (cursor-hooks-live-2026-10-23.md).
    // `afterAgentThought` fires between them and carries no state change, and `sessionEnd` never
    // fired. There is no approval event: the "Run this command?" card sits between `preToolUse`
    // and `beforeShellExecution`, so blocked stays with the screen rule
    // `run_command_card_blocked` (manifests/cursor.toml), as for cline.
    (
        "cursor",
        &[
            "sessionStart",
            "beforeSubmitPrompt",
            "preToolUse",
            "postToolUse",
            "beforeShellExecution",
            "afterShellExecution",
            "stop",
        ],
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookState {
    Working,
    Blocked,
    Idle,
}

impl HookState {
    pub fn as_str(self) -> &'static str {
        match self {
            HookState::Working => "working",
            HookState::Blocked => "blocked",
            HookState::Idle => "idle",
        }
    }
}

/// The state one hook event reports, or `None` for an event this bridge does not report.
///
/// This is the event's candidate state. A few events only report for certain payloads, which
/// [`payload_reports`] decides once stdin has been read; `run` consults this table first so a hook
/// outside it costs nothing.
pub fn state_for(agent: &str, event: &str) -> Option<HookState> {
    match (agent, event) {
        ("claude", "SessionStart" | "Stop" | "SessionEnd") | ("codex", "SessionStart" | "Stop") => {
            Some(HookState::Idle)
        }
        ("claude" | "codex", "UserPromptSubmit" | "PreToolUse" | "PostToolUse") => {
            Some(HookState::Working)
        }
        ("claude" | "codex", "PermissionRequest") => Some(HookState::Blocked),
        ("grok", "SessionStart" | "Stop" | "StopFailure" | "StopCancelled" | "SessionEnd") => {
            Some(HookState::Idle)
        }
        ("grok", "UserPromptSubmit" | "PreToolUse" | "PostToolUse") => Some(HookState::Working),
        ("grok", "Notification") => Some(HookState::Blocked),
        ("cline", "TaskStart" | "TaskResume" | "PreToolUse" | "PostToolUse") => {
            Some(HookState::Working)
        }
        ("cline", "TaskComplete" | "TaskError") => Some(HookState::Idle),
        ("antigravity", "SessionStart" | "Stop") => Some(HookState::Idle),
        ("antigravity", "PreInvocation") => Some(HookState::Working),
        ("crush", "PreToolUse") => Some(HookState::Working),
        ("cursor", "sessionStart" | "stop") => Some(HookState::Idle),
        (
            "cursor",
            "beforeSubmitPrompt" | "preToolUse" | "beforeShellExecution" | "afterShellExecution"
            | "postToolUse",
        ) => Some(HookState::Working),
        _ => None,
    }
}

/// Grok and agy spell the same field two ways, so every lookup accepts both spellings.
fn field<'a>(payload: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    payload.get(camel).or_else(|| payload.get(snake)).filter(|value| !value.is_null())
}

fn text<'a>(payload: &'a Value, camel: &str, snake: &str) -> Option<&'a str> {
    field(payload, camel, snake).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty())
}

/// Whether an event whose candidate state is [`state_for`] actually reports for this payload.
///
/// Three measured events carry their meaning in a field rather than in their name, so the table
/// alone would mislabel them. An unparsable payload reports `Value::Null` here, which fails every
/// test below: a state this bridge cannot confirm is better left to screen inference than guessed.
pub fn payload_reports(agent: &str, event: &str, payload: &Value) -> bool {
    match (agent, event) {
        // One grok hook file serves the root session and its subagents alike, and a subagent's
        // turn says nothing about what the pane is waiting on (remote-grok.md section 5).
        ("grok", _) if field(payload, "subagentType", "subagent_type").is_some() => false,
        // `permission_prompt` is grok's approval screen. `idle_prompt` and `task_complete` are
        // settled states the Stop events already report, about a minute sooner.
        ("grok", "Notification") => {
            text(payload, "notificationType", "notification_type") == Some("permission_prompt")
        }
        // agy raises Stop at the end of every invocation, including ones another invocation
        // follows. Only `fullyIdle` means the pane is back with the user (local-2.md).
        ("antigravity", "Stop") => payload.get("fullyIdle").and_then(Value::as_bool) == Some(true),
        _ => true,
    }
}

/// The provider session the payload names, in the shape the daemon's resume plan accepts.
///
/// Every agent here resumes by id under the `session_id` key (`terminal/shell.rs`), but they do
/// not agree on where the id sits: claude, codex, crush and grok name it at the top level
/// (`session_id`, and `sessionId` too for grok), and so does cursor, whose `session_id` equals its
/// `conversation_id`, the chat id `cursor-agent --resume` takes. Cline nests it under
/// `sessionContext` because its
/// top-level `taskId` is per-turn, and agy calls it `conversationId`.
pub fn provider_session(agent: &str, payload: &Value) -> Option<Value> {
    let (id, transcript) = match agent {
        "cline" => (
            payload
                .get("sessionContext")
                .and_then(|context| text(context, "rootSessionId", "root_session_id")),
            None,
        ),
        "antigravity" => (
            text(payload, "conversationId", "conversation_id"),
            text(payload, "transcriptPath", "transcript_path"),
        ),
        // cursor's `transcript_path` (`~/.cursor/projects/<slug>/agent-transcripts/<id>/<id>.jsonl`)
        // is left out. The machine-session admission refuses any report that carries a path
        // (`daemon/session_metadata_provider.rs` `admit_agent_report`), so sending it would cost a
        // machine pane its identity. Remote history would also read that file with the generic
        // conversation reader (`remote/server.rs`), and cursor's format there is unmeasured.
        "cursor" => (text(payload, "sessionId", "session_id"), None),
        _ => (
            text(payload, "sessionId", "session_id"),
            text(payload, "transcriptPath", "transcript_path"),
        ),
    };
    let mut session = Map::new();
    session.insert("key".into(), Value::from("session_id"));
    session.insert("id".into(), Value::from(id?));
    if let Some(path) = transcript {
        session.insert("transcriptPath".into(), Value::from(path));
    }
    Some(Value::Object(session))
}

/// What a blocked pane is waiting on: the tool and the agent's own description of the request.
///
/// Grok announces its approval screen with a `Notification` that names no tool, so the
/// notification's own `message` is the fallback.
pub fn blocked_detail(payload: &Value) -> Option<String> {
    let tool = text(payload, "toolName", "tool_name");
    let input = field(payload, "toolInput", "tool_input");
    let what = ["description", "command"]
        .iter()
        .find_map(|key| input.and_then(|input| text(input, key, key)));
    let detail = match (tool, what) {
        (Some(tool), Some(what)) => format!("{tool}: {what}"),
        (Some(tool), None) => format!("Permission required: {tool}"),
        (None, Some(what)) => what.to_string(),
        (None, None) => text(payload, "message", "message")?.to_string(),
    };
    Some(detail.chars().take(MAX_DETAIL_CHARS).collect())
}

/// Where the report goes. TCP wins when both halves are present, as in the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Tcp { port: u16, token: String },
    Socket(String),
}

pub fn endpoint(env: &impl Fn(&str) -> Option<String>) -> Option<Endpoint> {
    let port = env("FERRYX_AGENT_STATE_PORT")
        .and_then(|port| port.trim().parse::<u16>().ok())
        .filter(|port| *port > 0);
    let token = env("FERRYX_AGENT_STATE_TOKEN").filter(|token| !token.is_empty());
    if let (Some(port), Some(token)) = (port, token) {
        return Some(Endpoint::Tcp { port, token });
    }
    env("FERRYX_AGENT_STATE_SOCKET")
        .filter(|path| !path.is_empty())
        .map(Endpoint::Socket)
}

fn pane_session_id(env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    env("FERRYX_SESSION_ID").filter(|id| !id.is_empty())
}

/// Grok also runs the hooks in `~/.claude/settings.json` (facts/remote-grok.md section 5), so
/// the claude entry fires inside grok panes too. Grok sets `GROK_HOOK_EVENT` for every hook it
/// runs. Without this check a grok pane would report as claude, with grok's session id.
fn runs_under_another_agent(agent: &str, env: &impl Fn(&str) -> Option<String>) -> bool {
    agent == "claude" && env("GROK_HOOK_EVENT").is_some()
}

/// The endpoint and the exact line to send for one hook invocation, or `None` when nothing is
/// reported: not a Ferryx pane, no endpoint, or an event outside the agent's table.
pub fn prepare(
    agent: &str,
    event: &str,
    stdin: &[u8],
    env: &impl Fn(&str) -> Option<String>,
) -> Option<(Endpoint, String)> {
    let session_id = pane_session_id(env)?;
    if runs_under_another_agent(agent, env) {
        return None;
    }
    let endpoint = endpoint(env)?;
    let state = state_for(agent, event)?;
    // An unreadable payload still reports the state; only the provider identity is lost.
    let payload = serde_json::from_slice::<Value>(stdin).unwrap_or(Value::Null);
    if !payload_reports(agent, event, &payload) {
        return None;
    }
    let mut report = Map::new();
    report.insert("type".into(), Value::from("agentState"));
    report.insert("sessionId".into(), Value::from(session_id));
    report.insert("state".into(), Value::from(state.as_str()));
    report.insert("agent".into(), Value::from(agent));
    if let Some(session) = provider_session(agent, &payload) {
        report.insert("providerSession".into(), session);
    }
    if state == HookState::Blocked {
        if let Some(detail) = blocked_detail(&payload) {
            report.insert("detail".into(), Value::from(detail));
        }
    }
    if let Endpoint::Tcp { token, .. } = &endpoint {
        report.insert("token".into(), Value::from(token.as_str()));
    }
    let mut line = serde_json::to_string(&Value::Object(report)).ok()?;
    line.push('\n');
    Some((endpoint, line))
}

pub fn send(endpoint: &Endpoint, line: &str) -> std::io::Result<()> {
    match endpoint {
        Endpoint::Tcp { port, .. } => {
            let address = std::net::SocketAddr::from(([127, 0, 0, 1], *port));
            let mut stream = std::net::TcpStream::connect_timeout(&address, SEND_TIMEOUT)?;
            stream.set_write_timeout(Some(SEND_TIMEOUT))?;
            stream.write_all(line.as_bytes())
        }
        #[cfg(unix)]
        Endpoint::Socket(path) => {
            let mut stream = std::os::unix::net::UnixStream::connect(path)?;
            stream.set_write_timeout(Some(SEND_TIMEOUT))?;
            stream.write_all(line.as_bytes())
        }
        #[cfg(not(unix))]
        Endpoint::Socket(_) => Err(std::io::Error::from(std::io::ErrorKind::Unsupported)),
    }
}

/// Runs the reporter for the arguments after `agent-hook`. Always returns 0.
pub fn run(args: impl IntoIterator<Item = OsString>) -> i32 {
    let args: Vec<String> = args.into_iter().filter_map(|arg| arg.into_string().ok()).collect();
    let [agent, event] = args.as_slice() else {
        return 0;
    };
    let env = |key: &str| std::env::var(key).ok();
    if pane_session_id(&env).is_none() || endpoint(&env).is_none() || state_for(agent, event).is_none()
    {
        return 0;
    }
    let mut stdin = Vec::new();
    let _ = std::io::stdin()
        .lock()
        .take(MAX_PAYLOAD_BYTES)
        .read_to_end(&mut stdin);
    if let Some((endpoint, line)) = prepare(agent, event, &stdin, &env) {
        let _ = send(&endpoint, &line);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::protocol::AgentStateReport;

    fn fixture(name: &str) -> &'static str {
        match name {
            "claude-SessionStart" => include_str!("fixtures/agent_hook/claude-SessionStart.json"),
            "claude-UserPromptSubmit" => {
                include_str!("fixtures/agent_hook/claude-UserPromptSubmit.json")
            }
            "claude-PreToolUse" => include_str!("fixtures/agent_hook/claude-PreToolUse.json"),
            "claude-PostToolUse" => include_str!("fixtures/agent_hook/claude-PostToolUse.json"),
            "claude-PermissionRequest" => {
                include_str!("fixtures/agent_hook/claude-PermissionRequest.json")
            }
            "claude-Notification" => include_str!("fixtures/agent_hook/claude-Notification.json"),
            "claude-Stop" => include_str!("fixtures/agent_hook/claude-Stop.json"),
            "claude-SessionEnd" => include_str!("fixtures/agent_hook/claude-SessionEnd.json"),
            "claude-SubagentStop" => include_str!("fixtures/agent_hook/claude-SubagentStop.json"),
            "codex-SessionStart" => include_str!("fixtures/agent_hook/codex-SessionStart.json"),
            "codex-UserPromptSubmit" => {
                include_str!("fixtures/agent_hook/codex-UserPromptSubmit.json")
            }
            "codex-PreToolUse" => include_str!("fixtures/agent_hook/codex-PreToolUse.json"),
            "codex-PostToolUse" => include_str!("fixtures/agent_hook/codex-PostToolUse.json"),
            "codex-PermissionRequest" => {
                include_str!("fixtures/agent_hook/codex-PermissionRequest.json")
            }
            "codex-Stop" => include_str!("fixtures/agent_hook/codex-Stop.json"),
            "grok-SessionStart" => include_str!("fixtures/agent_hook/grok-SessionStart.json"),
            "grok-UserPromptSubmit" => {
                include_str!("fixtures/agent_hook/grok-UserPromptSubmit.json")
            }
            "grok-PreToolUse" => include_str!("fixtures/agent_hook/grok-PreToolUse.json"),
            "grok-PostToolUse" => include_str!("fixtures/agent_hook/grok-PostToolUse.json"),
            "grok-Notification" => include_str!("fixtures/agent_hook/grok-Notification.json"),
            "grok-Stop" => include_str!("fixtures/agent_hook/grok-Stop.json"),
            "grok-StopCancelled" => include_str!("fixtures/agent_hook/grok-StopCancelled.json"),
            "grok-SessionEnd" => include_str!("fixtures/agent_hook/grok-SessionEnd.json"),
            "cline-TaskStart" => include_str!("fixtures/agent_hook/cline-TaskStart.json"),
            "cline-TaskResume" => include_str!("fixtures/agent_hook/cline-TaskResume.json"),
            "cline-PreToolUse" => include_str!("fixtures/agent_hook/cline-PreToolUse.json"),
            "cline-PostToolUse" => include_str!("fixtures/agent_hook/cline-PostToolUse.json"),
            "cline-TaskComplete" => include_str!("fixtures/agent_hook/cline-TaskComplete.json"),
            "cline-TaskError" => include_str!("fixtures/agent_hook/cline-TaskError.json"),
            "antigravity-SessionStart" => {
                include_str!("fixtures/agent_hook/antigravity-SessionStart.json")
            }
            "antigravity-PreInvocation" => {
                include_str!("fixtures/agent_hook/antigravity-PreInvocation.json")
            }
            "antigravity-Stop" => include_str!("fixtures/agent_hook/antigravity-Stop.json"),
            "crush-PreToolUse" => include_str!("fixtures/agent_hook/crush-PreToolUse.json"),
            "cursor-sessionStart" => include_str!("fixtures/agent_hook/cursor-sessionStart.json"),
            "cursor-beforeSubmitPrompt" => {
                include_str!("fixtures/agent_hook/cursor-beforeSubmitPrompt.json")
            }
            "cursor-afterAgentThought" => {
                include_str!("fixtures/agent_hook/cursor-afterAgentThought.json")
            }
            "cursor-preToolUse" => include_str!("fixtures/agent_hook/cursor-preToolUse.json"),
            "cursor-beforeShellExecution" => {
                include_str!("fixtures/agent_hook/cursor-beforeShellExecution.json")
            }
            "cursor-afterShellExecution" => {
                include_str!("fixtures/agent_hook/cursor-afterShellExecution.json")
            }
            "cursor-postToolUse" => include_str!("fixtures/agent_hook/cursor-postToolUse.json"),
            "cursor-stop" => include_str!("fixtures/agent_hook/cursor-stop.json"),
            other => panic!("no fixture {other}"),
        }
    }

    const CLAUDE_ID: &str = "d49fdc31-682a-40b4-811a-eea8a17f6cf9";
    const CLAUDE_TRANSCRIPT: &str = "/Users/indo/.claude/projects/-private-tmp-parity-claude-59r7/d49fdc31-682a-40b4-811a-eea8a17f6cf9.jsonl";
    const CODEX_ID: &str = "01a1626f-f0db-75d0-b73c-2372a21d4cae";
    const CODEX_TRANSCRIPT: &str = "/Users/indo/.codex/sessions/2026/10/22/rollout-2026-10-22T16-07-46-01a1626f-f0db-75d0-b73c-2372a21d4cae.jsonl";
    const GROK_ID: &str = "01a16274-a688-7e73-b6fc-434bc7781be4";
    /// The Ctrl+C capture came from an earlier grok session than the rest.
    const GROK_CANCELLED_ID: &str = "01a16272-de6c-7ea0-8fe6-f9cc7a5bcf24";
    const GROK_TRANSCRIPT_DIR: &str =
        "/home/indo/.grok/sessions/%2Fhome%2Findo%2Fagent-parity-facts%2Fgrok%2Fproj";
    const CLINE_ID: &str = "1792653340197_ys4yu";
    const AGY_ID: &str = "76a07aec-16fc-4700-9d83-0726bad9de7f";
    const AGY_TRANSCRIPT: &str = "/Users/indo/.gemini/antigravity-cli/brain/76a07aec-16fc-4700-9d83-0726bad9de7f/.system_generated/logs/transcript_full.jsonl";
    const CRUSH_ID: &str = "3df112d9-e56f-4f69-b8bb-c58b5d580e89";
    const CURSOR_ID: &str = "16b85941-4c42-45ff-b3d5-6e52e48d9e14";

    /// The field a fixture carries its own event name on, and the value it must hold, so a fixture
    /// can never be filed under the wrong event. Agy payloads name no event at all.
    fn event_marker(agent: &str, event: &str) -> Option<(&'static str, String)> {
        match agent {
            "claude" | "codex" | "grok" | "cursor" => Some(("hook_event_name", event.to_string())),
            "crush" => Some(("event", event.to_string())),
            // Cline's payload names the event only through the internal alias it maps to
            // (local-3.md `M2`).
            "cline" => {
                let alias = match event {
                    "TaskStart" => "agent_start",
                    "TaskResume" => "agent_resume",
                    "PreToolUse" => "tool_call",
                    "PostToolUse" => "tool_result",
                    "TaskComplete" => "agent_end",
                    "TaskError" => "agent_error",
                    other => panic!("no cline alias for {other}"),
                };
                Some(("hookName", alias.to_string()))
            }
            "antigravity" => None,
            other => panic!("no event marker for {other}"),
        }
    }

    /// The provider session one fixture must yield. Keyed by fixture rather than by agent because
    /// grok's first `SessionStart` carries no transcript yet and its Ctrl+C capture is an earlier
    /// session, and because cline and crush name no transcript at all.
    fn expected_provider_session(agent: &str, name: &str) -> Value {
        let grok_transcript = |id: &str| format!("{GROK_TRANSCRIPT_DIR}/{id}/updates.jsonl");
        let (id, transcript): (&str, Option<String>) = match (agent, name) {
            ("claude", _) => (CLAUDE_ID, Some(CLAUDE_TRANSCRIPT.to_string())),
            ("codex", _) => (CODEX_ID, Some(CODEX_TRANSCRIPT.to_string())),
            ("grok", "grok-SessionStart") => (GROK_ID, None),
            ("grok", "grok-StopCancelled") => {
                (GROK_CANCELLED_ID, Some(grok_transcript(GROK_CANCELLED_ID)))
            }
            ("grok", _) => (GROK_ID, Some(grok_transcript(GROK_ID))),
            ("cline", _) => (CLINE_ID, None),
            ("antigravity", _) => (AGY_ID, Some(AGY_TRANSCRIPT.to_string())),
            ("crush", _) => (CRUSH_ID, None),
            // cursor names a transcript from the first tool on, but it is never forwarded.
            ("cursor", _) => (CURSOR_ID, None),
            (other, _) => panic!("no provider session for {other}"),
        };
        match transcript {
            Some(path) => {
                serde_json::json!({"key": "session_id", "id": id, "transcriptPath": path})
            }
            None => serde_json::json!({"key": "session_id", "id": id}),
        }
    }

    fn socket_env(key: &str) -> Option<String> {
        match key {
            "FERRYX_SESSION_ID" => Some("pane-1".into()),
            "FERRYX_AGENT_STATE_SOCKET" => Some("/tmp/agent-state.sock".into()),
            _ => None,
        }
    }

    fn parse_line(line: &str) -> Value {
        assert!(line.ends_with('\n'), "the wire is newline-delimited: {line:?}");
        assert_eq!(line.matches('\n').count(), 1, "one report is one line: {line:?}");
        serde_json::from_str(line.trim_end()).unwrap()
    }

    /// Every measured fixture, with the state its event must report.
    const EXPECTED: [(&str, &str, &str, Option<&str>); 41] = [
        ("claude", "SessionStart", "claude-SessionStart", Some("idle")),
        ("claude", "UserPromptSubmit", "claude-UserPromptSubmit", Some("working")),
        ("claude", "PreToolUse", "claude-PreToolUse", Some("working")),
        ("claude", "PostToolUse", "claude-PostToolUse", Some("working")),
        ("claude", "PermissionRequest", "claude-PermissionRequest", Some("blocked")),
        ("claude", "Notification", "claude-Notification", None),
        ("claude", "Stop", "claude-Stop", Some("idle")),
        ("claude", "SessionEnd", "claude-SessionEnd", Some("idle")),
        ("claude", "SubagentStop", "claude-SubagentStop", None),
        ("codex", "SessionStart", "codex-SessionStart", Some("idle")),
        ("codex", "UserPromptSubmit", "codex-UserPromptSubmit", Some("working")),
        ("codex", "PreToolUse", "codex-PreToolUse", Some("working")),
        ("codex", "PostToolUse", "codex-PostToolUse", Some("working")),
        ("codex", "PermissionRequest", "codex-PermissionRequest", Some("blocked")),
        ("codex", "Stop", "codex-Stop", Some("idle")),
        ("grok", "SessionStart", "grok-SessionStart", Some("idle")),
        ("grok", "UserPromptSubmit", "grok-UserPromptSubmit", Some("working")),
        ("grok", "PreToolUse", "grok-PreToolUse", Some("working")),
        ("grok", "PostToolUse", "grok-PostToolUse", Some("working")),
        ("grok", "Notification", "grok-Notification", Some("blocked")),
        ("grok", "Stop", "grok-Stop", Some("idle")),
        ("grok", "StopCancelled", "grok-StopCancelled", Some("idle")),
        ("grok", "SessionEnd", "grok-SessionEnd", Some("idle")),
        ("cline", "TaskStart", "cline-TaskStart", Some("working")),
        ("cline", "TaskResume", "cline-TaskResume", Some("working")),
        ("cline", "PreToolUse", "cline-PreToolUse", Some("working")),
        ("cline", "PostToolUse", "cline-PostToolUse", Some("working")),
        ("cline", "TaskComplete", "cline-TaskComplete", Some("idle")),
        ("cline", "TaskError", "cline-TaskError", Some("idle")),
        ("antigravity", "SessionStart", "antigravity-SessionStart", Some("idle")),
        ("antigravity", "PreInvocation", "antigravity-PreInvocation", Some("working")),
        ("antigravity", "Stop", "antigravity-Stop", Some("idle")),
        ("crush", "PreToolUse", "crush-PreToolUse", Some("working")),
        ("cursor", "sessionStart", "cursor-sessionStart", Some("idle")),
        ("cursor", "beforeSubmitPrompt", "cursor-beforeSubmitPrompt", Some("working")),
        ("cursor", "afterAgentThought", "cursor-afterAgentThought", None),
        ("cursor", "preToolUse", "cursor-preToolUse", Some("working")),
        ("cursor", "beforeShellExecution", "cursor-beforeShellExecution", Some("working")),
        ("cursor", "afterShellExecution", "cursor-afterShellExecution", Some("working")),
        ("cursor", "postToolUse", "cursor-postToolUse", Some("working")),
        ("cursor", "stop", "cursor-stop", Some("idle")),
    ];

    #[test]
    fn measured_hook_payloads_map_to_the_extension_states() {
        for (agent, event, name, expected) in EXPECTED {
            let payload: Value = serde_json::from_str(fixture(name)).unwrap();
            if let Some((key, value)) = event_marker(agent, event) {
                assert_eq!(
                    payload[key], value,
                    "fixture {name} must be the payload of the event it is named after"
                );
            }
            let prepared = prepare(agent, event, fixture(name).as_bytes(), &socket_env);
            match expected {
                None => assert!(prepared.is_none(), "{name} must not report"),
                Some(state) => {
                    let (endpoint, line) = prepared.unwrap_or_else(|| panic!("{name} must report"));
                    assert_eq!(endpoint, Endpoint::Socket("/tmp/agent-state.sock".into()));
                    let report = parse_line(&line);
                    assert_eq!(report["type"], "agentState");
                    assert_eq!(report["sessionId"], "pane-1");
                    assert_eq!(report["state"], state, "{name}");
                    assert_eq!(report["agent"], agent);
                    assert!(report.get("token").is_none(), "the socket path carries no token");
                }
            }
        }
    }

    #[test]
    fn installed_events_are_exactly_the_reported_ones() {
        for (agent, events) in HOOK_AGENTS {
            for event in events {
                assert!(state_for(agent, event).is_some(), "{agent} {event} is installed but unmapped");
            }
            // Every agent the installer writes an entry for must also have fixtures proving the
            // mapping, so removing an agent's support fails a measured assertion too.
            assert!(
                EXPECTED.iter().any(|(name, _, _, state)| *name == agent && state.is_some()),
                "{agent} is installed but no measured fixture asserts its states"
            );
        }
        assert_eq!(state_for("claude", "Notification"), None);
        assert_eq!(state_for("claude", "SubagentStop"), None);
        assert_eq!(state_for("codex", "SessionEnd"), None);
        // Events the facts name but that this bridge does not install for that agent.
        assert_eq!(state_for("grok", "SubagentStop"), None);
        assert_eq!(state_for("grok", "PreCompact"), None);
        assert_eq!(state_for("cline", "SessionShutdown"), None, "it never fired (local-3.md)");
        assert_eq!(state_for("cline", "UserPromptSubmit"), None, "it never fired (local-3.md)");
        assert_eq!(state_for("antigravity", "PostInvocation"), None, "it never fired (local-2.md)");
        assert_eq!(state_for("crush", "Stop"), None, "crush has no Stop event");
        assert_eq!(state_for("cursor", "sessionEnd"), None, "it never fired (cursor-hooks-live)");
        assert_eq!(state_for("cursor", "afterAgentThought"), None, "no state change");
        // cursor has no approval event, so nothing it installs may report blocked.
        assert!(HOOK_AGENTS
            .iter()
            .find(|(agent, _)| *agent == "cursor")
            .expect("cursor is installed")
            .1
            .iter()
            .all(|event| state_for("cursor", event) != Some(HookState::Blocked)));
        // Agents recorded as N/A in the F8 matrix must stay out of the table entirely.
        for agent in ["copilot", "kimi", "droid", "cursor-agent", "aider", "gjc", "gemini"] {
            for event in ["SessionStart", "Stop", "PreToolUse", "sessionEnd", "stop", "idle"] {
                assert_eq!(state_for(agent, event), None, "{agent} has no hook bridge");
            }
        }
        assert_eq!(state_for("omo", "Stop"), None, "only table agents report");
    }

    #[test]
    fn provider_session_comes_from_the_payload_fields() {
        for (agent, _, name, expected) in EXPECTED {
            if expected.is_none() {
                continue;
            }
            let payload: Value = serde_json::from_str(fixture(name)).unwrap();
            assert_eq!(
                provider_session(agent, &payload),
                Some(expected_provider_session(agent, name)),
                "{name}"
            );
        }
        assert_eq!(provider_session("claude", &serde_json::json!({"session_id": "  "})), None);
        assert_eq!(
            provider_session("claude", &serde_json::json!({"session_id": "abc"})),
            Some(serde_json::json!({"key": "session_id", "id": "abc"}))
        );
        // Each agent reads only its own field, so a payload cannot be mined for another's id.
        let foreign = serde_json::json!({
            "session_id": "top", "conversationId": "agy", "sessionContext": {"rootSessionId": "cl"}
        });
        assert_eq!(provider_session("grok", &foreign).unwrap()["id"], "top");
        assert_eq!(provider_session("crush", &foreign).unwrap()["id"], "top");
        assert_eq!(provider_session("cursor", &foreign).unwrap()["id"], "top");
        // cursor's transcript_path is null until the first tool runs and a path after it; the
        // id reports either way and the path never does.
        let fresh: Value = serde_json::from_str(fixture("cursor-sessionStart")).unwrap();
        assert!(fresh["transcript_path"].is_null());
        let after_tool: Value = serde_json::from_str(fixture("cursor-postToolUse")).unwrap();
        assert!(after_tool["transcript_path"].as_str().is_some_and(|path| path.ends_with(".jsonl")));
        for payload in [&fresh, &after_tool] {
            assert_eq!(
                provider_session("cursor", payload),
                Some(serde_json::json!({"key": "session_id", "id": CURSOR_ID}))
            );
        }
        assert_eq!(fresh["session_id"], fresh["conversation_id"], "the id --resume takes");
        assert_eq!(provider_session("antigravity", &foreign).unwrap()["id"], "agy");
        assert_eq!(provider_session("cline", &foreign).unwrap()["id"], "cl");
        assert_eq!(provider_session("cline", &serde_json::json!({"session_id": "top"})), None);
        // Grok sends the id under both spellings; either alone is enough.
        assert_eq!(
            provider_session("grok", &serde_json::json!({"sessionId": "camel"})),
            Some(serde_json::json!({"key": "session_id", "id": "camel"}))
        );
    }

    #[test]
    fn payload_dependent_events_report_only_what_the_payload_confirms() {
        // Grok's approval screen is a Notification, but so are its settle and completion notices.
        let notification = fixture("grok-Notification");
        assert_eq!(
            parse_line(&prepare("grok", "Notification", notification.as_bytes(), &socket_env).unwrap().1)
                ["state"],
            "blocked"
        );
        let mut idle_prompt: Value = serde_json::from_str(notification).unwrap();
        for key in ["notificationType", "notification_type"] {
            idle_prompt[key] = Value::from("idle_prompt");
        }
        assert!(
            prepare("grok", "Notification", idle_prompt.to_string().as_bytes(), &socket_env)
                .is_none(),
            "only permission_prompt means the pane is waiting on the user"
        );
        // A subagent's turn is not the pane's state, whatever the event.
        let mut subagent: Value = serde_json::from_str(fixture("grok-Stop")).unwrap();
        subagent["subagentType"] = Value::from("explore");
        assert!(prepare("grok", "Stop", subagent.to_string().as_bytes(), &socket_env).is_none());
        // agy raises Stop between invocations too; only fullyIdle hands the pane back.
        let stop = fixture("antigravity-Stop");
        assert!(prepare("antigravity", "Stop", stop.as_bytes(), &socket_env).is_some());
        let mut partial: Value = serde_json::from_str(stop).unwrap();
        partial["fullyIdle"] = Value::Bool(false);
        assert!(
            prepare("antigravity", "Stop", partial.to_string().as_bytes(), &socket_env).is_none()
        );
        // An unparsable payload cannot confirm any of the three, so none of them report.
        for (agent, event) in [("grok", "Notification"), ("antigravity", "Stop")] {
            assert!(prepare(agent, event, b"not json", &socket_env).is_none(), "{agent} {event}");
        }
    }

    #[test]
    fn reports_decode_as_daemon_reports_and_keep_a_resumable_provider_session() {
        // The daemon keeps providerSession only when the agent's resume plan accepts it
        // (daemon/server.rs parse_agent_state_report), so test the exact predicate it applies.
        for (agent, event, name, expected) in EXPECTED {
            if expected.is_none() {
                continue;
            }
            let (_, line) = prepare(agent, event, fixture(name).as_bytes(), &socket_env).unwrap();
            let report: AgentStateReport = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(report.session_id, "pane-1");
            assert_eq!(report.agent.as_deref(), Some(agent));
            let provider = report.provider_session.expect("provider session decodes");
            let plan = crate::terminal::shell::resolve_agent_resume_plan(agent, &provider)
                .unwrap_or_else(|error| panic!("{name}: resume plan refused: {error:?}"));
            let id = expected_provider_session(agent, name)["id"].as_str().unwrap().to_string();
            assert!(plan.args.contains(&id), "{name}: {plan:?}");
            let program = match agent {
                "antigravity" => "agy",
                "cursor" => "cursor-agent",
                other => other,
            };
            assert_eq!(plan.program, program, "{name}: the resume arm must launch {program}");
        }
    }

    #[test]
    fn blocked_reports_carry_the_permission_text() {
        let (_, line) = prepare(
            "claude",
            "PermissionRequest",
            fixture("claude-PermissionRequest").as_bytes(),
            &socket_env,
        )
        .unwrap();
        assert_eq!(
            parse_line(&line)["detail"],
            "Bash: Create created.txt and write current date into it"
        );
        let (_, line) = prepare(
            "codex",
            "PermissionRequest",
            fixture("codex-PermissionRequest").as_bytes(),
            &socket_env,
        )
        .unwrap();
        assert_eq!(
            parse_line(&line)["detail"],
            "Bash: May I create and write the requested created.txt file in the workspace?"
        );
        // Only blocked reports carry a detail.
        let (_, line) =
            prepare("codex", "PreToolUse", fixture("codex-PreToolUse").as_bytes(), &socket_env)
                .unwrap();
        assert!(parse_line(&line).get("detail").is_none());
        // A request without a description falls back to the command, and the text is bounded.
        let long = "x".repeat(2 * MAX_DETAIL_CHARS);
        let detail = blocked_detail(&serde_json::json!({"tool_input": {"command": long}})).unwrap();
        assert_eq!(detail.chars().count(), MAX_DETAIL_CHARS);
        assert_eq!(blocked_detail(&serde_json::json!({})), None);
    }

    #[test]
    fn a_grok_approval_report_carries_the_notification_text() {
        // Grok's approval Notification names no tool, so its own message is the detail.
        let (_, line) =
            prepare("grok", "Notification", fixture("grok-Notification").as_bytes(), &socket_env)
                .unwrap();
        let report = parse_line(&line);
        assert_eq!(report["state"], "blocked");
        assert_eq!(report["detail"], "Tool permission requested");
        assert_eq!(report["providerSession"]["id"], GROK_ID);
        // Grok's tool events use the camelCase spelling of the same fields.
        let grok_tool: Value = serde_json::from_str(fixture("grok-PreToolUse")).unwrap();
        let detail = blocked_detail(&grok_tool).expect("a camelCase tool payload still reads");
        assert!(detail.starts_with(grok_tool["toolName"].as_str().unwrap()), "{detail}");
    }

    #[test]
    fn tcp_endpoint_wins_and_carries_the_token() {
        let env = |key: &str| match key {
            "FERRYX_SESSION_ID" => Some("pane-2".to_string()),
            "FERRYX_AGENT_STATE_SOCKET" => Some("/tmp/agent-state.sock".to_string()),
            "FERRYX_AGENT_STATE_PORT" => Some("4123".to_string()),
            "FERRYX_AGENT_STATE_TOKEN" => Some("secret".to_string()),
            _ => None,
        };
        let (reached, line) =
            prepare("codex", "Stop", fixture("codex-Stop").as_bytes(), &env).unwrap();
        assert_eq!(reached, Endpoint::Tcp { port: 4123, token: "secret".into() });
        assert_eq!(parse_line(&line)["token"], "secret");
        // A port without a token is not a usable TCP endpoint, as in the extension.
        let half = |key: &str| match key {
            "FERRYX_AGENT_STATE_PORT" => Some("4123".to_string()),
            _ => None,
        };
        assert_eq!(endpoint(&half), None);
    }

    #[test]
    fn nothing_is_reported_outside_a_ferryx_pane() {
        let no_session = |key: &str| match key {
            "FERRYX_AGENT_STATE_SOCKET" => Some("/tmp/agent-state.sock".to_string()),
            _ => None,
        };
        assert!(prepare("claude", "Stop", fixture("claude-Stop").as_bytes(), &no_session).is_none());
        let no_endpoint = |key: &str| match key {
            "FERRYX_SESSION_ID" => Some("pane-1".to_string()),
            _ => None,
        };
        assert!(prepare("claude", "Stop", fixture("claude-Stop").as_bytes(), &no_endpoint).is_none());
        assert!(prepare("unknown", "Stop", b"{}", &socket_env).is_none());
    }

    #[test]
    fn an_unreadable_payload_still_reports_the_state() {
        let (_, line) = prepare("claude", "UserPromptSubmit", b"not json", &socket_env).unwrap();
        let report = parse_line(&line);
        assert_eq!(report["state"], "working");
        assert!(report.get("providerSession").is_none());
    }

    #[test]
    fn a_claude_hook_run_by_grok_is_not_reported_as_claude() {
        let grok = |key: &str| match key {
            "GROK_HOOK_EVENT" => Some("stop".to_string()),
            other => socket_env(other),
        };
        assert!(prepare("claude", "Stop", fixture("claude-Stop").as_bytes(), &grok).is_none());
        assert!(prepare("codex", "Stop", fixture("codex-Stop").as_bytes(), &grok).is_some());
    }

    #[test]
    fn run_exits_zero_on_bad_arguments() {
        assert_eq!(run(Vec::<OsString>::new()), 0);
        assert_eq!(run(vec![OsString::from("claude")]), 0);
        assert_eq!(run(vec![OsString::from("nobody"), OsString::from("Stop")]), 0);
    }

    #[test]
    fn send_delivers_one_line_over_tcp() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let line = "{\"type\":\"agentState\"}\n";
        send(&Endpoint::Tcp { port, token: "t".into() }, line).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut received = String::new();
        stream.read_to_string(&mut received).unwrap();
        assert_eq!(received, line);
    }

    #[cfg(unix)]
    #[test]
    fn send_delivers_one_line_over_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let line = "{\"type\":\"agentState\"}\n";
        send(&Endpoint::Socket(path.to_string_lossy().into_owned()), line).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut received = String::new();
        stream.read_to_string(&mut received).unwrap();
        assert_eq!(received, line);
    }

    #[test]
    fn a_missing_receiver_is_an_error_the_reporter_swallows() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(send(&Endpoint::Tcp { port, token: "t".into() }, "x\n").is_err());
    }
}
