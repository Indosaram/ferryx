use std::sync::Arc;

use ferryx_rd_app::{
    agent_input::{
        convert_agent_action_to_events, normalize_agent_coordinates, AgentAction,
        InputStateTracker, MouseButton, ScreenInfo,
    },
    agent_server::{AgentServer, AgentServerBackend},
    mcp_server::handle_mcp_message,
};
use ferryx_rd_proto::{InputEvent, InputEventType, Modifiers, WireCodec};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct MockHostReceiver {
    received_events: Arc<std::sync::Mutex<Vec<InputEvent>>>,
}

impl AgentServerBackend for MockHostReceiver {
    fn send_input_event(&self, event: InputEvent) -> Result<(), String> {
        self.received_events.lock().unwrap().push(event);
        Ok(())
    }

    fn get_screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 3840,
            height: 1600,
            scale: 1.0,
            logical_width: Some(3840),
            logical_height: Some(1600),
            monitors: Vec::new(),
            connected_host: "omarchy-linux".to_string(),
        }
    }

    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        let w = 128u32;
        let h = 128u32;
        let len = (w * h * 3 / 2) as usize;
        Some((w, h, Arc::new(vec![128u8; len])))
    }
}

#[tokio::test]
async fn e2e_agent_actions_encode_and_decode_on_mock_host() {
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend = Arc::new(MockHostReceiver {
        received_events: received.clone(),
    });

    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.0, 0.0);

    let actions = vec![
        AgentAction::MouseMove {
            x: 1920.0,
            y: 800.0,
            normalized: false,
        },
        AgentAction::Click {
            x: 960.0,
            y: 400.0,
            button: MouseButton::Middle,
            count: 1,
            normalized: false,
        },
        AgentAction::Drag {
            start_x: 100.0,
            start_y: 100.0,
            end_x: 500.0,
            end_y: 500.0,
            button: MouseButton::Left,
            steps: 4,
            duration_ms: 100,
            normalized: false,
        },
        AgentAction::Scroll {
            dx: 0.0,
            dy: -120.0,
            x: Some(500.0),
            y: Some(500.0),
            normalized: false,
        },
        AgentAction::Hotkey {
            keys: vec!["Ctrl".into(), "Shift".into(), "t".into()],
        },
        AgentAction::TypeText {
            text: "uname -a\n".into(),
            delay_ms: 0,
            paste_mode: false,
        },
        AgentAction::ReleaseAll,
    ];

    let host_width = 3840.0;
    let host_height = 1600.0;

    for action in &actions {
        let events = convert_agent_action_to_events(
            action,
            &mut tracker,
            &mut current_pos,
            host_width,
            host_height,
        )
        .unwrap();

        for event in events {
            let encoded_bytes = event.encode().unwrap();
            assert_eq!(encoded_bytes.len(), InputEvent::SIZE);

            let decoded_event = InputEvent::decode(&encoded_bytes).unwrap();
            assert_eq!(decoded_event.event_type, event.event_type);
            assert!((decoded_event.x - event.x).abs() < 0.001);
            assert!((decoded_event.y - event.y).abs() < 0.001);
            assert_eq!(decoded_event.key_code, event.key_code);
            assert_eq!(decoded_event.modifiers, event.modifiers);

            backend.send_input_event(decoded_event).unwrap();
        }
    }

    let host_events = received.lock().unwrap();
    assert!(!host_events.is_empty());

    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::MiddleMouseDown));
    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::MiddleMouseUp));

    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::LeftMouseDragged));

    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::ScrollWheel && e.scroll_dy == -120.0));

    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::KeyDown
            && e.modifiers.contains(Modifiers::CONTROL)
            && e.modifiers.contains(Modifiers::SHIFT)));

    assert!(host_events
        .iter()
        .any(|e| e.event_type == InputEventType::Reset));
}

#[tokio::test]
async fn e2e_coordinate_normalization_across_resolutions() {
    let resolutions = [(1920.0, 1080.0), (2560.0, 1440.0), (3840.0, 1600.0)];

    for (w, h) in resolutions {
        let (top_left_x, top_left_y) = normalize_agent_coordinates(0.0, 0.0, false, w, h).unwrap();
        assert_eq!(top_left_x, 0.0);
        assert_eq!(top_left_y, 1.0);

        let (center_x, center_y) =
            normalize_agent_coordinates(w / 2.0, h / 2.0, false, w, h).unwrap();
        assert!((center_x - 0.5).abs() < 0.001);
        assert!((center_y - 0.5).abs() < 0.001);

        let (bottom_right_x, bottom_right_y) =
            normalize_agent_coordinates(w, h, false, w, h).unwrap();
        assert_eq!(bottom_right_x, 1.0);
        assert_eq!(bottom_right_y, 0.0);
    }
}

#[tokio::test]
async fn e2e_agent_http_and_mcp_servers_integration() {
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let backend = Arc::new(MockHostReceiver {
        received_events: received.clone(),
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let (server, addr) = AgentServer::bind("127.0.0.1:0".parse().unwrap(), backend.clone())
        .await
        .unwrap();

    let server_handle = tokio::spawn(async move {
        server.run(shutdown_rx).await.unwrap();
    });

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /api/v1/screen/info HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut buf),
    )
    .await
    .unwrap()
    .unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("3840"));
    assert!(resp.contains("omarchy-linux"));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /api/v1/screen/screenshot?format=png HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buf = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut buf),
    )
    .await
    .unwrap()
    .unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("\"format\":\"png\""));
    assert!(resp.contains("\"base64\""));

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let batch_json = serde_json::to_vec(&vec![
        AgentAction::MouseMove {
            x: 100.0,
            y: 100.0,
            normalized: false,
        },
        AgentAction::MouseDown {
            button: MouseButton::Left,
        },
        AgentAction::MouseUp {
            button: MouseButton::Left,
        },
    ])
    .unwrap();
    let req = format!(
        "POST /api/v1/input/batch HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        batch_json.len()
    );
    stream.write_all(req.as_bytes()).await.unwrap();
    stream.write_all(&batch_json).await.unwrap();

    let mut buf = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut buf),
    )
    .await
    .unwrap()
    .unwrap();
    let resp = String::from_utf8_lossy(&buf);
    assert!(resp.contains("200 OK"));
    assert!(resp.contains("\"events_sent\":3"));

    let mut tracker = InputStateTracker::default();
    let mut current_pos = (0.5, 0.5);
    let mcp_call = r#"{"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":500.0,"y":500.0,"button":"right"}}}"#;
    let mcp_resp =
        handle_mcp_message(mcp_call, backend.clone(), &mut tracker, &mut current_pos).unwrap();
    let mcp_resp: serde_json::Value = serde_json::from_str(&mcp_resp).unwrap();
    assert_eq!(mcp_resp["id"], 100);
    assert!(mcp_resp.get("error").is_none());

    let _ = shutdown_tx.send(true);
    let _ = server_handle.await;
}
