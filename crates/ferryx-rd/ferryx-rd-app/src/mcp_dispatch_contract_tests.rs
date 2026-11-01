use super::*;
use ferryx_rd_proto::InputEventType;
use std::sync::Mutex;

struct RecordedBackend {
    accepted: Mutex<Vec<InputEventType>>,
    fail_on: Option<InputEventType>,
}

impl AgentServerBackend for RecordedBackend {
    fn send_input_event(&self, event: ferryx_rd_proto::InputEvent) -> Result<(), String> {
        if self.fail_on == Some(event.event_type) {
            return Err("fixture transmission failure".into());
        }
        self.accepted.lock().unwrap().push(event.event_type);
        Ok(())
    }
    fn get_screen_info(&self) -> ScreenInfo {
        ScreenInfo {
            width: 64,
            height: 64,
            scale: 1.0,
            logical_width: Some(64),
            logical_height: Some(64),
            monitors: vec![],
            connected_host: "fixture".into(),
        }
    }
    fn get_latest_frame_nv12(&self) -> Option<(u32, u32, Arc<Vec<u8>>)> {
        Some((64, 64, Arc::new(vec![128; 64 * 64 * 3 / 2])))
    }
}

#[test]
fn successful_down_then_failed_up_is_followed_by_reset() {
    let backend = Arc::new(RecordedBackend {
        accepted: Mutex::new(Vec::new()),
        fail_on: Some(InputEventType::LeftMouseUp),
    });
    let response = handle_mcp_message(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"remote_mouse_click","arguments":{"x":1,"y":2}}}"#,
        backend.clone(), &mut InputStateTracker::default(), &mut (0.0, 0.0),
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["result"]["isError"],
        true
    );
    let events = backend.accepted.lock().unwrap();
    let down = events
        .iter()
        .position(|event| *event == InputEventType::LeftMouseDown)
        .unwrap();
    let reset = events
        .iter()
        .position(|event| *event == InputEventType::Reset)
        .unwrap();
    assert!(down < reset);
}

#[test]
fn every_advertised_tool_has_a_successful_dispatch_contract() {
    for (name, arguments, expected) in [
        (
            "remote_mouse_click",
            json!({"x":1,"y":2}),
            Some(InputEventType::LeftMouseDown),
        ),
        (
            "remote_mouse_move",
            json!({"x":1,"y":2}),
            Some(InputEventType::MouseMove),
        ),
        (
            "remote_mouse_drag",
            json!({"start_x":1,"start_y":2,"end_x":3,"end_y":4}),
            Some(InputEventType::LeftMouseUp),
        ),
        (
            "remote_mouse_scroll",
            json!({"dx":0,"dy":1}),
            Some(InputEventType::ScrollWheel),
        ),
        (
            "remote_key_press",
            json!({"key":"Enter"}),
            Some(InputEventType::KeyDown),
        ),
        (
            "remote_hotkey",
            json!({"keys":["Ctrl","A"]}),
            Some(InputEventType::KeyDown),
        ),
        (
            "remote_type_text",
            json!({"text":"a"}),
            Some(InputEventType::KeyDown),
        ),
        ("remote_release_all", json!({}), Some(InputEventType::Reset)),
        ("remote_get_screen_info", json!({}), None),
        ("remote_take_screenshot", json!({"format":"png"}), None),
    ] {
        let backend = Arc::new(RecordedBackend {
            accepted: Mutex::new(Vec::new()),
            fail_on: None,
        });
        let response = handle_mcp_message(
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}}).to_string(),
            backend.clone(), &mut InputStateTracker::default(), &mut (0.0, 0.0),
        ).unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_eq!(response["result"]["isError"], false, "{name}");
        let events = backend.accepted.lock().unwrap();
        match expected {
            Some(event) => assert!(events.contains(&event), "{name}: {events:?}"),
            None => assert!(events.is_empty()),
        }
    }
}
