use super::*;
use crate::agent_control::{AgentOp, RecordingControl};
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

async fn call(app: &Router, method: &str, uri: &str, body: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("http://127.0.0.1{uri}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 256 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn lane(pool: &SqlitePool, key: &str, status: &str, brief_path: &str) {
    sqlx::query(
        "INSERT INTO lanes(key, title, lead_session_key, cwd, brief_path, session_key, status, done_signals, last_activity_at, created_at)
         VALUES(?, ?, 'lead', '/work', ?, 'lane', ?, '[]', '2020-01-01T00:00:00Z', ?)",
    )
    .bind(key)
    .bind(format!("Lane {key}"))
    .bind(brief_path)
    .bind(status)
    .bind(format!("2020-01-01T00:00:0{}Z", key.len() % 10))
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn lanes_carry_status_pause_brief_and_notifying_transitions() {
    let pool = crate::storage::memory().await.unwrap();
    let brief = std::env::temp_dir().join(format!("agent-api-brief-{}.md", std::process::id()));
    std::fs::write(&brief, "# Fix it\n\nbody").unwrap();
    lane(&pool, "a", "working", brief.to_str().unwrap()).await;
    lane(&pool, "bb", "working", "").await;
    crate::lanes::state::transition(
        &pool,
        "a",
        crate::lanes::state::LaneStatus::Blocked,
        Some("stuck"),
    )
    .await
    .unwrap();
    crate::lanes::pause::pause(&pool, "bb", "session:lead")
        .await
        .unwrap();
    let app = router_with(pool.clone(), None);
    let (status, body) = call(&app, "GET", "/lanes", "").await;
    assert_eq!(status, StatusCode::OK);
    let lanes = body["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0]["key"], "a");
    assert_eq!(lanes[0]["status"], "blocked");
    assert_eq!(lanes[0]["paused"], false);
    assert_eq!(lanes[0]["brief"], "# Fix it\n\nbody");
    assert_eq!(lanes[0]["title"], "Lane a");
    assert!(lanes[0]["model"].is_null());
    let transitions = lanes[0]["transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0]["from_status"], "working");
    assert_eq!(transitions[0]["to_status"], "blocked");
    assert_eq!(transitions[0]["reason"], "stuck");
    assert!(transitions[0]["at"].is_string());
    // A pause keeps the status, so it is not a transition.
    assert_eq!(lanes[1]["paused"], true);
    assert_eq!(lanes[1]["brief"], "");
    assert_eq!(lanes[1]["transitions"], json!([]));
    // The mirror tables are untouched.
    let issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(issues, 0);
    let _ = std::fs::remove_file(&brief);
}

#[tokio::test]
async fn creating_a_lane_opens_it_queued_with_its_brief_and_thread() {
    let pool = crate::storage::memory().await.unwrap();
    let app = router_with(pool.clone(), None);
    let (status, _) = call(&app, "POST", "/lanes", r#"{"title":"x","cwd":"/w"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = call(
        &app,
        "POST",
        "/lanes",
        &json!({
            "title": "Import the ledger",
            "brief": "# Ledger\n\nimport it",
            "cwd": "/work",
            "done": "tests pass",
            "thread": "slack:C123:1700000000.000100",
            "key_base": "AGT-7",
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let lane = &body["lane"];
    let key = lane["key"].as_str().unwrap();
    assert!(key.starts_with("agt-7-"), "{key}");
    assert_eq!(lane["status"], "queued");
    assert_eq!(lane["title"], "Import the ledger");
    assert_eq!(lane["cwd"], "/work");
    assert_eq!(lane["paused"], false);
    let (done, thread): (String, Option<String>) =
        sqlx::query_as("SELECT done_signals, thread_channel_id FROM lanes WHERE key = ?")
            .bind(key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(done, "tests pass");
    assert_eq!(thread.as_deref(), Some("slack:C123:1700000000.000100"));
    let (_, body) = call(
        &app,
        "POST",
        "/lanes",
        r#"{"title":"No thread","cwd":"/w","done":"d","thread":"not-a-ref"}"#,
    )
    .await;
    assert!(body["lane"]["key"].as_str().unwrap().starts_with("lane-"));
    let thread: Option<String> =
        sqlx::query_scalar("SELECT thread_channel_id FROM lanes WHERE title = 'No thread'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(thread, None);
}

#[tokio::test]
async fn lane_actions_drive_the_lane_and_record_an_event() {
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "a", "working", "").await;
    let app = router_with(pool.clone(), None);
    let (status, _) = call(&app, "POST", "/lanes/a/action", r#"{"action":"pause"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, "POST", "/lanes/zz/action", r#"{"action":"send"}"#).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(
        &app,
        "POST",
        "/lanes/a/action",
        r#"{"action":"object","note":"wrong file"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lane"]["status"], "blocked");
    // Close needs a verified lane unless forced.
    let (status, body) = call(&app, "POST", "/lanes/a/action", r#"{"action":"close"}"#).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].is_string());
    let (status, body) = call(
        &app,
        "POST",
        "/lanes/a/action",
        r#"{"action":"send","text":"go on"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["lane"]["status"], "working");
    let (status, body) = call(
        &app,
        "POST",
        "/lanes/a/action",
        r#"{"action":"close","force":true,"note":"shipped"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["lane"]["status"], "closed");
    let (status, body) = call(&app, "POST", "/lanes/a/action", r#"{"action":"reopen"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["lane"]["status"], "working");
    let events: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, body FROM lane_events WHERE lane_key = 'a' ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        events,
        vec![
            ("blocked".into(), "wrong file".into()),
            ("lane_action".into(), "go on".into()),
            ("lane_action".into(), "shipped".into()),
            ("lane_action".into(), "reopen".into()),
        ]
    );
    let reason: String = sqlx::query_scalar(
        "SELECT reason FROM lane_history WHERE lane_key = 'a' AND to_status = 'closed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason, "forced: shipped");
}

async fn insert_item(pool: &SqlitePool, id: &str, author: &str, class: &str, at: &str) {
    sqlx::query(
        "INSERT INTO inbox_items (id, source, source_ref, thread_ref, author, title, snippet, received_at, link, raw_hash, collected_at)
         VALUES (?, 'kakao', ?, 't', ?, NULL, ?, ?, NULL, 'h', ?)",
    )
    .bind(id).bind(id).bind(author).bind(format!("make {id}")).bind(at).bind(at)
    .execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO inbox_decisions (item_id, class, reason, due_at, proposed_action, status, decided_at) VALUES (?, ?, 'r', '2026-10-22T00:00:00+00:00', 'draft', 'open', ?)",
    )
    .bind(id).bind(class).bind(at)
    .execute(pool).await.unwrap();
}

#[tokio::test]
async fn requests_list_bot_items_of_each_requester_with_the_locale() {
    let pool = crate::storage::memory().await.unwrap();
    insert_item(&pool, "b", "여신님😍", "bot", "2026-10-21T11:00:00+00:00").await;
    insert_item(&pool, "a", "여신님😍", "bot", "2026-10-21T10:00:00+00:00").await;
    insert_item(&pool, "c", "여신님😍", "owner", "2026-10-21T10:00:00+00:00").await;
    insert_item(&pool, "d", "다른사람", "bot", "2026-10-21T10:00:00+00:00").await;
    let value = requests_value(&pool, &["여신님".to_owned()]).await.unwrap();
    assert_eq!(value["requesters"], json!([{"name": "여신님", "label": "여신님"}]));
    assert_eq!(value["locale"]["korean"], true);
    assert_eq!(value["locale"]["timezone"], "Asia/Seoul");
    let items = value["items"].as_array().unwrap();
    assert_eq!(
        items.iter().map(|i| i["item_id"].as_str().unwrap()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert_eq!(items[0]["requester"], "여신님");
    assert_eq!(items[0]["author"], "여신님😍");
    assert_eq!(items[0]["source"], "kakao");
    assert_eq!(items[0]["snippet"], "make a");
    assert_eq!(items[0]["class"], "bot");
    assert_eq!(items[0]["status"], "open");
    assert_eq!(items[0]["reason"], "r");
    assert_eq!(items[0]["due_at"], "2026-10-22T00:00:00+00:00");
    assert_eq!(items[0]["proposed_action"], "draft");
    assert_eq!(items[0]["received_at"], "2026-10-21T10:00:00+00:00");
    // No requester is built in, so the route lists none unless TRACKER_REQUESTERS is set.
    let empty = requests_value(&pool, &[]).await.unwrap();
    assert_eq!(empty["items"], json!([]));
    let app = router_with(pool, None);
    let (status, body) = call(&app, "GET", "/requests", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].is_array() && body["requesters"].is_array());
}

#[tokio::test]
async fn locale_reports_the_owner_language_and_zone() {
    let app = router_with(crate::storage::memory().await.unwrap(), None);
    let (status, body) = call(&app, "GET", "/locale", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"language": "ko", "korean": true, "timezone": "Asia/Seoul"})
    );
    let english = crate::locale::scope(crate::locale::OwnerLocale::english_utc(), async {
        locale_value()
    })
    .await;
    assert_eq!(
        english,
        json!({"language": "en", "korean": false, "timezone": "UTC"})
    );
}

#[tokio::test]
async fn sessions_list_snapshots_and_the_board_and_actions_reach_the_pane() {
    let pool = crate::storage::memory().await.unwrap();
    // Without agent control the board is empty and actions are refused.
    let bare = router_with(pool.clone(), None);
    let (status, body) = call(&bare, "GET", "/sessions?refresh=1", "").await;
    assert_eq!(status, StatusCode::OK);
    if crate::sessions::live::installed().is_none() {
        assert_eq!(body["snapshots"], json!([]));
        assert_eq!(body["board"]["counts"]["total"], 0);
        let (status, _) = call(&bare, "POST", "/sessions/local/a/send", r#"{"text":"hi"}"#).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let control = Arc::new(RecordingControl::default());
    *control.result.lock().unwrap() = json!({
        "sessions": [{"sessionId": "a", "agent": "claude", "cwd": "/work/a", "state": "working", "detail": "editing"}],
        "session": {"sessionId": "a", "pending": {"id": "prompt-1"}, "screenRevision": "r9"},
    });
    let app = router_with(pool.clone(), Some(control.clone() as Arc<dyn AgentControl>));
    let (status, body) = call(&app, "GET", "/sessions?refresh=1", "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["snapshots"],
        json!([{
            "machine": "local", "ok": true, "error": null,
            "sessions": [{
                "session_id": "a", "name": "claude", "cwd": "/work/a", "state": "working",
                "streaming": true, "question": null, "last_assistant": "editing",
            }],
        }])
    );
    assert_eq!(body["board"]["counts"]["working"], 1);
    assert_eq!(body["board"]["sessions"][0]["session_id"], "a");
    assert_eq!(body["board"]["machines"][0]["machine"], "local");
    let lists = |ops: &[(Option<String>, AgentOp)]| {
        ops.iter().filter(|(_, op)| matches!(op, AgentOp::List)).count()
    };
    assert_eq!(lists(&control.ops()), 1);
    // Without refresh the last snapshot is served; nothing is mirrored into tracker issues.
    let (_, cached) = call(&app, "GET", "/sessions", "").await;
    assert_eq!(cached["snapshots"][0]["machine"], "local");
    let issues: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracker_issues")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(issues, 0);

    let (status, body) = call(&app, "POST", "/sessions/local/a/send", r#"{"text":" hello "}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert!(body["result"].is_object());
    let (status, _) = call(&app, "POST", "/sessions/local/a/send", r#"{"text":"  "}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, "POST", "/sessions/nowhere/a/send", r#"{"text":"hi"}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, "POST", "/sessions/local/a/interrupt", "").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, "POST", "/sessions/local/a/answer", r#"{"text":"2"}"#).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &app,
        "POST",
        "/sessions/local/a/answer",
        r#"{"text":"use main","request":"prompt-7"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ops: Vec<AgentOp> = control
        .ops()
        .into_iter()
        .filter(|(_, op)| !matches!(op, AgentOp::List))
        .map(|(_, op)| op)
        .collect();
    assert!(matches!(&ops[0], AgentOp::Send { session_id, text, .. } if session_id == "a" && text == "hello"));
    assert!(matches!(&ops[1], AgentOp::Stop { session_id, .. } if session_id == "a"));
    assert!(matches!(&ops[2], AgentOp::Read { .. }));
    assert!(matches!(&ops[3], AgentOp::Answer { prompt_id, screen_revision, answer, .. }
        if prompt_id == "prompt-1" && screen_revision == "r9" && answer == &json!({"optionIndex": 1})));
    assert!(matches!(&ops[5], AgentOp::Answer { prompt_id, answer, .. }
        if prompt_id == "prompt-7" && answer == &json!({"customText": "use main"})));
}

#[tokio::test]
async fn the_agent_routes_sit_behind_the_hub_host_guard() {
    let pool = crate::storage::memory().await.unwrap();
    let app = crate::http::router(pool, std::env::temp_dir());
    let get = |host: &'static str| {
        Request::builder()
            .uri("/api/agent/locale")
            .header("host", host)
            .body(Body::empty())
            .unwrap()
    };
    let ok = app.clone().oneshot(get("127.0.0.1:9119")).await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let foreign = app.clone().oneshot(get("evil.example")).await.unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let cross = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/lanes")
                .header("host", "127.0.0.1:9119")
                .header("origin", "https://evil.example")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn lane_detail_serves_the_panel_row_history_transcript_and_events() {
    let pool = crate::storage::memory().await.unwrap();
    lane(&pool, "a", "working", "").await;
    sqlx::query("UPDATE lanes SET claim_evidence = 'ev', turns_run = 3, thread_channel_id = 'slack:C1:1.2' WHERE key = 'a'")
        .execute(&pool)
        .await
        .unwrap();
    crate::lanes::state::transition(&pool, "a", crate::lanes::state::LaneStatus::Blocked, Some("stuck"))
        .await
        .unwrap();
    crate::lanes::state::transition(&pool, "a", crate::lanes::state::LaneStatus::Working, Some("unstuck"))
        .await
        .unwrap();
    for n in 0..205 {
        sqlx::query("INSERT INTO lane_transcript(lane_key, at, role, text) VALUES('a', '2020', 'assistant', ?)")
            .bind(format!("t{n}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES('a', 'lead', 'note', ?, '2020')")
            .bind(format!("e{n}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    let app = router_with(pool.clone(), None);
    let (status, _) = call(&app, "GET", "/lanes/zz", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(&app, "GET", "/lanes/a", "").await;
    assert_eq!(status, StatusCode::OK);
    let lane = &body["lane"];
    assert_eq!(lane["key"], "a");
    assert_eq!(lane["status"], "working");
    assert_eq!(lane["done_signals"], "[]");
    assert_eq!(lane["claim_evidence"], "ev");
    assert_eq!(lane["turns_run"], 3);
    assert_eq!(lane["thread_channel_id"], "slack:C1:1.2");
    assert_eq!(lane["lead_session_key"], "lead");
    for column in ["model", "objection", "last_turn_outcome", "last_report"] {
        assert!(lane[column].is_null(), "{column}");
    }
    let history = body["history"].as_array().unwrap();
    assert_eq!(
        history.iter().map(|h| h["to_status"].as_str().unwrap()).collect::<Vec<_>>(),
        vec!["blocked", "working"]
    );
    assert_eq!(history[1]["reason"], "unstuck");
    let transcript = body["transcript"].as_array().unwrap();
    assert_eq!(transcript.len(), 200);
    assert_eq!(transcript[0]["text"], "t5");
    assert_eq!(transcript[199]["text"], "t204");
    assert_eq!(transcript[0]["role"], "assistant");
    assert!(transcript[0]["id"].as_i64().unwrap() < transcript[1]["id"].as_i64().unwrap());
    let events = body["events"].as_array().unwrap();
    assert_eq!(events.len(), 200);
    assert_eq!(events[0]["body"], "e5");
    assert_eq!(events[199]["body"], "e204");
    let mut keys: Vec<String> = events[0].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, vec!["body", "created_at", "id", "kind"]);
}

#[tokio::test]
async fn a_supervisor_owned_lane_is_refused_before_the_transition_is_judged() {
    use crate::goals::contract::RequirementSpec;
    use crate::supervisor::admission::{
        submit_work, ActorContext, OperationClass, OwnerChoices, SubmitOutcome, SubmitWork,
    };
    const OWNER: &str = "qa-owner";
    let pool = crate::storage::memory().await.unwrap();
    crate::supervisor::provision_owner(&pool, OWNER, "qa", "qa:qa", "host-a", "/work/qa/.git")
        .await
        .unwrap();
    let actor = ActorContext {
        principal: OWNER.into(),
        source_adapter: crate::supervisor::ChannelProvider::Cli,
        source_message_id: "m-1".into(),
        source_body: "Create result.txt containing READY. Run the verifier.".into(),
        channel_binding_id: None,
        choices: OwnerChoices::default(),
    };
    let work = SubmitWork {
        project_id: None,
        host_id: None,
        kind: None,
        title: "Write result.txt".into(),
        instruction: "Create result.txt containing READY.".into(),
        requirements: vec![RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        cwd: None,
        operations: OperationClass::BASELINE.to_vec(),
        turn_limit: None,
    };
    let key = match submit_work(&pool, &actor, &work).await.unwrap() {
        SubmitOutcome::Admitted(work) => work.receipt.lane_key,
        other => panic!("expected an admitted goal, got {other:?}"),
    };
    let status_of = || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM lanes WHERE key = ?")
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let before = status_of().await;
    let app = router_with(pool.clone(), None);
    // An unforced close of a non-verified lane is also illegal; ownership is reported first.
    for action in ["close", "send", "object", "reopen"] {
        let (status, body) = call(
            &app,
            "POST",
            &format!("/lanes/{key}/action"),
            &json!({"action": action}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}");
        assert!(
            body["error"].as_str().unwrap().starts_with("SUPERVISOR_OWNED:"),
            "{action}: {body}"
        );
    }
    assert_eq!(status_of().await, before);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lane_events WHERE lane_key = ?")
        .bind(&key)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
}
