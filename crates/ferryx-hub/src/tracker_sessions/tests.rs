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

async fn states(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT i.session_ref, s.session_state FROM tracker_issues i JOIN tracker_states s ON s.id = i.state_id ORDER BY i.number",
    )
    .fetch_all(pool)
    .await
    .unwrap()
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
    assert_eq!(
        session_title(Some("claude"), Some("/work/a/"), "a"),
        "claude · a"
    );
    assert_eq!(session_title(None, None, "0123456789"), "01234567");
}

#[tokio::test]
async fn live_sessions_become_ses_issues_that_follow_state_end_and_stay_deleted() {
    let pool = crate::storage::memory().await.unwrap();
    sqlx::query("INSERT INTO tracker_users(id, name, display_name, avatar_color, kind, is_me, created_at) VALUES('me','Me','Me','#000','human',1,'2020-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    // A user team already owns "SES": the Sessions team takes SES1 and never adopts it.
    sqlx::query("INSERT INTO tracker_teams(id, key, name, icon, color, created_at) VALUES('user','SES','Mine','x','#111','2020-01-01T00:00:00Z')")
        .execute(&pool).await.unwrap();
    let first = vec![(
        "local".to_owned(),
        Ok(live(&[
            entry("a", "working", None),
            entry("b", "idle", None),
        ])),
    )];
    sync_from_snapshots(&pool, &first, Utc::now())
        .await
        .unwrap();
    sync_from_snapshots(&pool, &first, Utc::now())
        .await
        .unwrap();
    assert_eq!(
        states(&pool).await,
        vec![
            ("local:a".into(), "working".into()),
            ("local:b".into(), "idle".into())
        ]
    );
    let identifiers: Vec<String> =
        sqlx::query_scalar("SELECT identifier FROM tracker_issues ORDER BY number")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        identifiers,
        vec!["SES1-1".to_string(), "SES1-2".to_string()]
    );

    // a asks a question (owner notified), b is gone, and an unreachable host changes nothing.
    let second = vec![
        (
            "local".to_owned(),
            Ok(live(&[entry("a", "blocked", Some(json!({"id": "p"})))])),
        ),
        ("mini".to_owned(), Err("down".to_owned())),
    ];
    sync_from_snapshots(&pool, &second, Utc::now())
        .await
        .unwrap();
    assert_eq!(
        states(&pool).await,
        vec![
            ("local:a".into(), "question".into()),
            ("local:b".into(), "ended".into())
        ]
    );
    let notices: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM tracker_notifications WHERE user_id = 'me'")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(notices, vec!["session_question".to_string()]);

    // A deleted issue is not re-created while its session lives.
    tombstone(&pool, "local:a").await.unwrap();
    sqlx::query("DELETE FROM tracker_issues WHERE session_ref = 'local:a'")
        .execute(&pool)
        .await
        .unwrap();
    sync_from_snapshots(&pool, &second, Utc::now())
        .await
        .unwrap();
    assert_eq!(states(&pool).await.len(), 1);
    // Once the session ends the tombstone goes, so a new session with that id is tracked.
    let gone = vec![("local".to_owned(), Ok(Vec::new()))];
    sync_from_snapshots(&pool, &gone, Utc::now()).await.unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracker_meta WHERE key LIKE 'session_tombstone:%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, 0);
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
    // Nothing was installed, so the board is empty and actions are refused, as omo's
    // board was with no hosts configured.
    assert_eq!(board(&pool).await["counts"]["total"], 0);
    assert!(send(&pool, "local", "a", "hi").await.is_err());
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
