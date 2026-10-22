//! `ferryx agent-hook <agent> <event>`: the reporter hook-style agents run on each lifecycle hook.
//!
//! The mapping and the wire line live in `ferryx_scope/ssh/agent_hook_core.rs`, which the SSH
//! remote helper compiles too (`ferryx-remote-helper agent-hook`), so a hook on an SSH host reports
//! exactly what this reporter reports. The tests stay here because they decode through daemon-only
//! types.

pub use crate::ssh::helper_runtime::agent_hook_core::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::ffi::OsString;
    use std::io::Read;
    use crate::daemon::protocol::AgentStateReport;
    use crate::ssh::helper_runtime::agent_hook_core::fixtures::{fixture, EXPECTED};

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
