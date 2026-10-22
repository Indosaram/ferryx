//! The `agent-hook <agent> <event>` reporter hook-style agents run on each lifecycle hook.
//!
//! Shared source: the desktop binary runs it as `ferryx agent-hook` (`cli/agent_hook.rs`) and the
//! SSH remote helper as `ferryx-remote-helper agent-hook` (`process.rs` `run`), so a hook on an SSH
//! host sends exactly the line a local pane sends. Keep it to `serde_json` and `std`: the remote
//! helper crate has no other dependency this file could use.
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
pub const MAX_DETAIL_CHARS: usize = 500;
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
// The remote helper binary reads only `run`; the table serves the desktop installer.
#[allow(dead_code)]
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

/// The measured hook payloads (`cli/fixtures/agent_hook/`) and the state each must report, shared
/// by the desktop crate's tests and the remote helper crate's, which compile this same file.
#[cfg(test)]
pub mod fixtures {
    pub fn fixture(name: &str) -> &'static str {
        match name {
            "claude-SessionStart" => include_str!("../../cli/fixtures/agent_hook/claude-SessionStart.json"),
            "claude-UserPromptSubmit" => {
                include_str!("../../cli/fixtures/agent_hook/claude-UserPromptSubmit.json")
            }
            "claude-PreToolUse" => include_str!("../../cli/fixtures/agent_hook/claude-PreToolUse.json"),
            "claude-PostToolUse" => include_str!("../../cli/fixtures/agent_hook/claude-PostToolUse.json"),
            "claude-PermissionRequest" => {
                include_str!("../../cli/fixtures/agent_hook/claude-PermissionRequest.json")
            }
            "claude-Notification" => include_str!("../../cli/fixtures/agent_hook/claude-Notification.json"),
            "claude-Stop" => include_str!("../../cli/fixtures/agent_hook/claude-Stop.json"),
            "claude-SessionEnd" => include_str!("../../cli/fixtures/agent_hook/claude-SessionEnd.json"),
            "claude-SubagentStop" => include_str!("../../cli/fixtures/agent_hook/claude-SubagentStop.json"),
            "codex-SessionStart" => include_str!("../../cli/fixtures/agent_hook/codex-SessionStart.json"),
            "codex-UserPromptSubmit" => {
                include_str!("../../cli/fixtures/agent_hook/codex-UserPromptSubmit.json")
            }
            "codex-PreToolUse" => include_str!("../../cli/fixtures/agent_hook/codex-PreToolUse.json"),
            "codex-PostToolUse" => include_str!("../../cli/fixtures/agent_hook/codex-PostToolUse.json"),
            "codex-PermissionRequest" => {
                include_str!("../../cli/fixtures/agent_hook/codex-PermissionRequest.json")
            }
            "codex-Stop" => include_str!("../../cli/fixtures/agent_hook/codex-Stop.json"),
            "grok-SessionStart" => include_str!("../../cli/fixtures/agent_hook/grok-SessionStart.json"),
            "grok-UserPromptSubmit" => {
                include_str!("../../cli/fixtures/agent_hook/grok-UserPromptSubmit.json")
            }
            "grok-PreToolUse" => include_str!("../../cli/fixtures/agent_hook/grok-PreToolUse.json"),
            "grok-PostToolUse" => include_str!("../../cli/fixtures/agent_hook/grok-PostToolUse.json"),
            "grok-Notification" => include_str!("../../cli/fixtures/agent_hook/grok-Notification.json"),
            "grok-Stop" => include_str!("../../cli/fixtures/agent_hook/grok-Stop.json"),
            "grok-StopCancelled" => include_str!("../../cli/fixtures/agent_hook/grok-StopCancelled.json"),
            "grok-SessionEnd" => include_str!("../../cli/fixtures/agent_hook/grok-SessionEnd.json"),
            "cline-TaskStart" => include_str!("../../cli/fixtures/agent_hook/cline-TaskStart.json"),
            "cline-TaskResume" => include_str!("../../cli/fixtures/agent_hook/cline-TaskResume.json"),
            "cline-PreToolUse" => include_str!("../../cli/fixtures/agent_hook/cline-PreToolUse.json"),
            "cline-PostToolUse" => include_str!("../../cli/fixtures/agent_hook/cline-PostToolUse.json"),
            "cline-TaskComplete" => include_str!("../../cli/fixtures/agent_hook/cline-TaskComplete.json"),
            "cline-TaskError" => include_str!("../../cli/fixtures/agent_hook/cline-TaskError.json"),
            "antigravity-SessionStart" => {
                include_str!("../../cli/fixtures/agent_hook/antigravity-SessionStart.json")
            }
            "antigravity-PreInvocation" => {
                include_str!("../../cli/fixtures/agent_hook/antigravity-PreInvocation.json")
            }
            "antigravity-Stop" => include_str!("../../cli/fixtures/agent_hook/antigravity-Stop.json"),
            "crush-PreToolUse" => include_str!("../../cli/fixtures/agent_hook/crush-PreToolUse.json"),
            "cursor-sessionStart" => include_str!("../../cli/fixtures/agent_hook/cursor-sessionStart.json"),
            "cursor-beforeSubmitPrompt" => {
                include_str!("../../cli/fixtures/agent_hook/cursor-beforeSubmitPrompt.json")
            }
            "cursor-afterAgentThought" => {
                include_str!("../../cli/fixtures/agent_hook/cursor-afterAgentThought.json")
            }
            "cursor-preToolUse" => include_str!("../../cli/fixtures/agent_hook/cursor-preToolUse.json"),
            "cursor-beforeShellExecution" => {
                include_str!("../../cli/fixtures/agent_hook/cursor-beforeShellExecution.json")
            }
            "cursor-afterShellExecution" => {
                include_str!("../../cli/fixtures/agent_hook/cursor-afterShellExecution.json")
            }
            "cursor-postToolUse" => include_str!("../../cli/fixtures/agent_hook/cursor-postToolUse.json"),
            "cursor-stop" => include_str!("../../cli/fixtures/agent_hook/cursor-stop.json"),
            other => panic!("no fixture {other}"),
        }
    }

    /// Every measured fixture, with the state its event must report. One table for both reporters:
    /// the desktop's tests (`cli/agent_hook.rs`) and the remote helper's read it from here.
    pub const EXPECTED: [(&str, &str, &str, Option<&str>); 41] = [
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
}

#[cfg(test)]
mod shared_tests {
    use super::fixtures::{fixture, EXPECTED};
    use super::*;

    fn socket_env(key: &str) -> Option<String> {
        match key {
            "FERRYX_SESSION_ID" => Some("pane-1".into()),
            "FERRYX_AGENT_STATE_SOCKET" => Some("/tmp/ferryx-agent-pane-1.sock".into()),
            _ => None,
        }
    }

    fn helper_env(key: &str) -> Option<String> {
        match key {
            "FERRYX_SESSION_ID" => Some("pane-1".into()),
            "FERRYX_AGENT_STATE_PORT" => Some("41234".into()),
            "FERRYX_AGENT_STATE_TOKEN" => Some("helper-token".into()),
            _ => None,
        }
    }

    /// Runs in both crates. In the remote helper crate it proves `ferryx-remote-helper agent-hook`
    /// maps every measured payload; in the desktop crate the same assertions cover `ferryx
    /// agent-hook`. Every table agent must keep a reporting row, so dropping one agent's mapping
    /// from either reporter fails here.
    #[test]
    fn every_measured_fixture_reports_the_same_line_on_both_remote_endpoints() {
        for (agent, _) in HOOK_AGENTS {
            assert!(
                EXPECTED.iter().any(|(name, _, _, state)| *name == agent && state.is_some()),
                "{agent} lost its reporting fixtures"
            );
        }
        for (agent, event, name, expected) in EXPECTED {
            let direct = prepare(agent, event, fixture(name).as_bytes(), &socket_env);
            let helper = prepare(agent, event, fixture(name).as_bytes(), &helper_env);
            let Some(state) = expected else {
                assert!(direct.is_none() && helper.is_none(), "{name} must not report");
                continue;
            };
            let (endpoint, line) = direct.unwrap_or_else(|| panic!("{name} must report"));
            assert_eq!(endpoint, Endpoint::Socket("/tmp/ferryx-agent-pane-1.sock".into()));
            let report: Value = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(report["state"], state, "{name}");
            assert_eq!(report["agent"], agent, "{name}");
            assert_eq!(report["sessionId"], "pane-1", "{name}");
            // A helper-spawned pane reaches the helper's TCP ingress (`agent_state.rs`), which
            // requires the token; otherwise the line is the same report.
            let (endpoint, line) = helper.unwrap_or_else(|| panic!("{name} must report over TCP"));
            assert_eq!(endpoint, Endpoint::Tcp { port: 41234, token: "helper-token".into() });
            let mut over_tcp: Value = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(over_tcp["token"], "helper-token");
            over_tcp.as_object_mut().unwrap().remove("token");
            assert_eq!(over_tcp, report, "{name}");
        }
    }

    /// The remote helper answers `agent-hook` without a runtime root; without the dispatch it
    /// would fail with REMOTE_RUNTIME_MISSING (or print its usage) and the agent would show a
    /// failing hook.
    #[test]
    fn the_helper_answers_agent_hook_without_a_runtime_root() {
        for args in [
            vec!["agent-hook", "nobody", "Stop"],
            vec!["agent-hook", "claude"],
            vec!["agent-hook"],
        ] {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert_eq!(super::super::process::run(args.clone()), Ok(()), "{args:?}");
        }
    }
}

