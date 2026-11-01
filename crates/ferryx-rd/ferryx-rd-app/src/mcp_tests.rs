use super::*;
use crate::agent_input::ScreenInfo;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestBackend {
    sent: AtomicUsize,
}

#[tokio::test]
async fn eof_releases_before_completion() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    run_mcp_io(backend.clone(), &b""[..], tokio::io::sink())
        .await
        .unwrap();
    assert_eq!(backend.sent.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn writer_error_releases() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    let (writer, peer) = tokio::io::duplex(8);
    drop(peer);
    let result = run_mcp_io(
        backend.clone(),
        &b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n"[..],
        writer,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(backend.sent.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_arguments_send_nothing() {
    for args in [
        json!({}),
        json!({"x":"bad","y":2}),
        json!({"x":1,"y":2,"button":"bad"}),
        json!({"x":1,"y":2,"count":4294967297u64}),
    ] {
        let backend = Arc::new(TestBackend {
            sent: AtomicUsize::new(0),
        });
        let request = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":args}});
        let response: Value = serde_json::from_str(
            &handle_mcp_message(
                &request.to_string(),
                backend.clone(),
                &mut InputStateTracker::default(),
                &mut (0.0, 0.0),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(backend.sent.load(Ordering::SeqCst), 0, "{request}");
        assert_eq!(response["error"]["code"], -32602);
    }
}

#[test]
fn invalid_envelope_never_dispatches() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    let response = handle_mcp_message(r#"{"id":1,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":1,"y":2}}}"#, backend.clone(), &mut InputStateTracker::default(), &mut (0.0,0.0)).unwrap();
    assert_eq!(backend.sent.load(Ordering::SeqCst), 0);
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["error"]["code"],
        -32600
    );
}

#[test]
fn protocol_errors_notifications_and_tool_names() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    for (input, code) in [
        ("{", -32700),
        ("[]", -32600),
        (
            r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{}}"#,
            -32602,
        ),
        (
            r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"remote_hotkey","arguments":{"keys":["Control",42]}}}"#,
            -32602,
        ),
    ] {
        let result = handle_mcp_message(
            input,
            backend.clone(),
            &mut InputStateTracker::default(),
            &mut (0.0, 0.0),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["error"]["code"],
            code
        );
    }
    for method in ["ping", "tools/list", "notifications/initialized"] {
        assert!(handle_mcp_message(
            &json!({"jsonrpc":"2.0","method":method}).to_string(),
            backend.clone(),
            &mut InputStateTracker::default(),
            &mut (0.0, 0.0)
        )
        .is_none());
    }
    let ping = handle_mcp_message(
        r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#,
        backend.clone(),
        &mut InputStateTracker::default(),
        &mut (0.0, 0.0),
    )
    .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&ping).unwrap()["id"], "a");
    let tools = list_mcp_tools();
    let names: Vec<_> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "remote_mouse_click",
            "remote_mouse_move",
            "remote_mouse_drag",
            "remote_mouse_scroll",
            "remote_key_press",
            "remote_hotkey",
            "remote_type_text",
            "remote_release_all",
            "remote_get_screen_info",
            "remote_take_screenshot",
            "remote_wait_for_screen_change"
        ]
    );
    assert_eq!(backend.sent.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancelled_reader_releases_and_returns_cleanup_failure() {
    let backend = Arc::new(FailingBackend(Default::default()));
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let (reader, _peer) = tokio::io::duplex(8);
    stop.send(true).unwrap();
    let result = run_mcp_io_until(backend.clone(), (reader, tokio::io::sink()), receiver).await;
    assert!(result.is_err());
    assert_eq!(
        *backend.0.lock().unwrap(),
        [ferryx_rd_proto::InputEventType::Reset]
    );
}

#[tokio::test]
async fn read_error_releases() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    assert!(
        run_mcp_io(backend.clone(), &b"\xff\n"[..], tokio::io::sink())
            .await
            .is_err()
    );
    assert_eq!(backend.sent.load(Ordering::SeqCst), 1);
}

struct FailingBackend(std::sync::Mutex<Vec<ferryx_rd_proto::InputEventType>>);
impl AgentServerBackend for FailingBackend {
    fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
        self.0.lock().unwrap().push(event.event_type);
        Err("scripted send failure".into())
    }
    fn get_screen_info(&self) -> ScreenInfo {
        TestBackend {
            sent: AtomicUsize::new(0),
        }
        .get_screen_info()
    }
    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        None
    }
}

#[test]
fn release_failure_is_error() {
    let backend = Arc::new(FailingBackend(Default::default()));
    let response = handle_mcp_message(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"remote_release_all"}}"#,
        backend,
        &mut InputStateTracker::default(),
        &mut (0.0, 0.0),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["result"]["isError"],
        true
    );
}

#[test]
fn partial_dispatch_resets() {
    let backend = Arc::new(FailingBackend(Default::default()));
    handle_mcp_message(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":1,"y":2}}}"#,
        backend.clone(),
        &mut InputStateTracker::default(),
        &mut (0.0, 0.0),
    );
    assert!(backend
        .0
        .lock()
        .unwrap()
        .contains(&ferryx_rd_proto::InputEventType::Reset));
}

impl AgentServerBackend for TestBackend {
    fn send_input_event(&self, _event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
        self.sent.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn get_screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 1920,
            height: 1080,
            scale: 1.0,
            logical_width: Some(1920),
            logical_height: Some(1080),
            monitors: vec![],
            connected_host: "test-mcp".to_string(),
        }
    }

    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        let w = 64u32;
        let h = 64u32;
        let len = (w * h * 3 / 2) as usize;
        Some((w, h, Arc::new(vec![128u8; len])))
    }
}

#[test]
fn handles_mcp_protocol_handshake_and_tool_calls() {
    let backend = Arc::new(TestBackend {
        sent: AtomicUsize::new(0),
    });
    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.0, 0.0);

    let init_req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let init_resp =
        handle_mcp_message(init_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let init: Value = serde_json::from_str(&init_resp).unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init["result"]["serverInfo"]["name"], "mahord-mcp");

    let list_req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let list_resp =
        handle_mcp_message(list_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let listed: Value = serde_json::from_str(&list_resp).unwrap();
    assert_eq!(listed["result"]["tools"], list_mcp_tools());

    let click_req = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":100,"y":200}}}"#;
    let click_resp =
        handle_mcp_message(click_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let clicked: Value = serde_json::from_str(&click_resp).unwrap();
    assert_eq!(clicked["id"], 3);
    assert_eq!(clicked["result"]["isError"], false);
    assert_eq!(backend.sent.load(Ordering::SeqCst), 3);

    let info_req = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"remote_get_screen_info","arguments":{}}}"#;
    let info_resp =
        handle_mcp_message(info_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let info: Value = serde_json::from_str(&info_resp).unwrap();
    let screen: ScreenInfo =
        serde_json::from_str(info["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(screen.width, 1920);
    assert_eq!(screen.connected_host, "test-mcp");

    let shot_req = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"remote_take_screenshot","arguments":{"format":"png"}}}"#;
    let shot_resp =
        handle_mcp_message(shot_req, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let shot: Value = serde_json::from_str(&shot_resp).unwrap();
    assert_eq!(shot["result"]["content"][0]["mimeType"], "image/png");
    use base64::Engine;
    let bytes = base64::prelude::BASE64_STANDARD
        .decode(shot["result"]["content"][0]["data"].as_str().unwrap())
        .unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
}

#[path = "mcp_dispatch_contract_tests.rs"]
mod dispatch_contract;
