//! `ferryx-cli agent-hook <agent> <event>` reports agent state, so a headless install whose hook
//! entries name ferryx-cli reaches the daemon the same way the desktop binary does.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

#[test]
fn ferryx_cli_agent_hook_sends_one_report_and_exits_zero() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback listener");
    let port = listener.local_addr().unwrap().port();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ferryx-cli"))
        .args(["agent-hook", "claude", "Stop"])
        .env("FERRYX_SESSION_ID", "pane-cli-1")
        .env("FERRYX_AGENT_STATE_PORT", port.to_string())
        .env("FERRYX_AGENT_STATE_TOKEN", "cli-token")
        .env_remove("FERRYX_AGENT_STATE_SOCKET")
        .env_remove("GROK_HOOK_EVENT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ferryx-cli");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"Stop","session_id":"abc"}"#)
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(output.stdout.is_empty(), "the reporter prints nothing");

    // The reporter has exited, so its connection and every byte it wrote are already queued in
    // the listener's backlog: a non-blocking accept either finds them or the report never came.
    listener.set_nonblocking(true).unwrap();
    let (mut stream, _) = listener.accept().expect("the reporter connected before it exited");
    stream.set_nonblocking(false).unwrap();
    let mut received = String::new();
    stream.read_to_string(&mut received).unwrap();

    assert!(received.ends_with('\n'), "one framed line: {received:?}");
    assert_eq!(received.matches('\n').count(), 1, "exactly one report: {received:?}");
    let report: serde_json::Value = serde_json::from_str(received.trim_end()).unwrap();
    assert_eq!(report["type"], "agentState");
    assert_eq!(report["sessionId"], "pane-cli-1");
    assert_eq!(report["state"], "idle");
    assert_eq!(report["agent"], "claude");
    assert_eq!(report["token"], "cli-token");

    // Nothing else connects after the one report.
    assert!(listener.accept().is_err(), "a second connection arrived");
}

#[test]
fn ferryx_cli_agent_hook_without_arguments_prints_nothing_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_ferryx-cli"))
        .arg("agent-hook")
        .stdin(Stdio::null())
        .output()
        .expect("run ferryx-cli");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty() && output.stderr.is_empty(), "{output:?}");
}
