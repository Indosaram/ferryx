use super::*;
use crate::agent_control::RecordingControl;

fn entry(id: &str, state: &str, pending: Option<Value>) -> Value {
    json!({
        "sessionId": id,
        "agent": "claude",
        "cwd": format!("/work/{id}"),
        "state": state,
        "pending": pending,
        "detail": "editing files",
    })
}

fn live(entries: &[Value]) -> Vec<LiveSession> {
    entries.iter().filter_map(LiveSession::from_entry).collect()
}

#[test]
fn list_entries_map_to_omo_states_and_the_board_keeps_omos_shape() {
    let sessions = live(&[
        entry("a", "idle", None),
        entry("b", "working", None),
        entry(
            "c",
            "blocked",
            Some(json!({"id": "p1", "question": "Proceed?"})),
        ),
        entry("d", "unknown", None),
    ]);
    assert_eq!(
        sessions.iter().map(|s| s.state).collect::<Vec<_>>(),
        vec!["idle", "working", "question", "idle"]
    );
    let board = shape(
        &[
            ("local".into(), Ok(sessions)),
            ("mini".into(), Err("host mini timed out".into())),
        ],
        Utc::now(),
    );
    assert_eq!(board["sessions"][0]["session_id"], "c");
    assert_eq!(board["sessions"][0]["questions"][0]["id"], "p1");
    assert_eq!(board["sessions"][1]["state"], "working");
    assert_eq!(board["counts"]["total"], 4);
    assert_eq!(board["counts"]["idle"], 2);
    assert_eq!(board["machines"][1]["ok"], false);
}

#[tokio::test]
async fn board_actions_reach_the_pane_through_agent_control() {
    let pool = crate::storage::memory().await.unwrap();
    let control = Arc::new(RecordingControl::default());
    *control.result.lock().unwrap() = json!({
        "session": {"sessionId": "a", "pending": {"id": "prompt-1"}, "screenRevision": "r9"}
    });
    let c = control.as_ref();
    send_on(c, &pool, "local", "a", " hello ").await.unwrap();
    interrupt_on(c, &pool, "local", "a").await.unwrap();
    answer_on(c, &pool, "local", "a", "2", None).await.unwrap();
    answer_on(c, &pool, "local", "a", "use main", None)
        .await
        .unwrap();
    assert!(send_on(c, &pool, "local", "a", "  ").await.is_err());
    assert!(send_on(c, &pool, "nowhere", "a", "hi").await.is_err());
    let ops = control.ops();
    assert!(
        matches!(&ops[0], (None, AgentOp::Send { session_id, text, .. }) if session_id == "a" && text == "hello")
    );
    assert!(matches!(&ops[1], (None, AgentOp::Stop { .. })));
    assert!(matches!(&ops[2], (None, AgentOp::Read { .. })));
    assert!(
        matches!(&ops[3], (None, AgentOp::Answer { prompt_id, screen_revision, answer, .. })
        if prompt_id == "prompt-1" && screen_revision == "r9" && answer == &json!({"optionIndex": 1}))
    );
    assert!(
        matches!(&ops[5], (None, AgentOp::Answer { answer, .. }) if answer == &json!({"customText": "use main"}))
    );

    // A paired host the fleet has listed is a valid target.
    sqlx::query("INSERT INTO fleet_inventory_cache (host_id, sessions, updated_at) VALUES ('mini', '[]', '2020')")
        .execute(&pool)
        .await
        .unwrap();
    send_on(c, &pool, "mini", "b", "hi").await.unwrap();
    assert!(
        matches!(control.ops().last(), Some((Some(host), AgentOp::Send { .. })) if host == "mini")
    );
}
