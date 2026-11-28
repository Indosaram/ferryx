use super::*;
use crate::meta::omo_client::rpc_tests::Script;
use std::time::Duration;

const INTERRUPTED: &str = include_str!("fixtures/interrupted-run.jsonl");

struct Fixture {
    script: Script,
    pool: SqlitePool,
    turn: SessionTurn,
    config: SlackTurnConfig,
}

impl Fixture {
    async fn new(lifecycle: &str) -> Self {
        let mut script = Script::new();
        script.config.ack_window = Duration::from_millis(100);
        let pool = crate::storage::memory().await.unwrap();
        let turn = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "D1", Some("1.0"), "U1"),
            reply_channel: None,
            reply_thread: None,
            status_ts: None,
            text: "normal".into(),
            background: false,
        };
        std::fs::write(script.session(), lifecycle).unwrap();
        let mut state = SessionState::default();
        state.metadata.insert("omo_thread_id".into(), json!(script.session()));
        state.metadata.insert("turn_rules".into(), json!(TURN_RULES_MARKER));
        save_state(&pool, &turn.key, &state).await.unwrap();
        let config = SlackTurnConfig {
            omo: script.config.clone(),
            allowed_users: vec!["U1".into()],
            free_channels: Vec::new(),
            default_model: None,
            allowed_channels: Vec::new(),
            ignored_channels: Vec::new(),
            thread_require_mention: false,
            auto_thread: true,
            allow_bots: AllowBots::None,
            allow_all_users: false,
        };
        Self { script, pool, turn, config }
    }

    async fn run(&self) -> Option<String> {
        tokio::time::timeout(Duration::from_secs(10), run_session_turn(&self.config, &self.pool, self.turn.clone()))
            .await.unwrap()
    }
}

#[tokio::test]
async fn an_interrupted_legacy_session_is_not_resumed() {
    // Given: the redacted lifecycle record from the measured stuck thread.
    let fixture = Fixture::new(INTERRUPTED).await;
    // When: the owner replies.
    let reply = fixture.run().await;
    // Then: a fresh engine answers, without deleting the old session file.
    assert_eq!(reply.as_deref(), Some("final"));
    assert!(!fixture.script.argv().iter().any(|arg| arg == "--session"));
    assert!(std::path::Path::new(&fixture.script.session()).exists());
}

#[tokio::test]
async fn a_running_unowned_legacy_session_is_not_resumed() {
    // Given: the last run has no settle or background/child custodian.
    let fixture = Fixture::new(r#"{"type":"custom","customType":"session-lifecycle","data":{"state":"running","runActive":true,"waitingOn":{"children":0,"background":[],"human":[]}}}"#).await;
    // When: the owner replies.
    let reply = fixture.run().await;
    // Then: the unowned run is not resumed.
    assert_eq!(reply.as_deref(), Some("final"));
    assert!(!fixture.script.argv().iter().any(|arg| arg == "--session"));
}

#[tokio::test]
async fn a_settled_legacy_session_is_resumed() {
    // Given: a completed session still associated with this Slack thread.
    let fixture = Fixture::new(r#"{"type":"custom","customType":"session-lifecycle","data":{"state":"done","runActive":false}}"#).await;
    // When: the owner replies.
    let reply = fixture.run().await;
    // Then: the settled session is resumed, preserving its engine thread id.
    assert_eq!(reply.as_deref(), Some("final"));
    assert!(fixture.script.argv().windows(2).any(|args| args == ["--session", &fixture.script.session()]));
}

#[tokio::test]
async fn a_session_waiting_on_background_is_still_resumed() {
    // Given: a session with real background custody.
    let fixture = Fixture::new(r#"{"type":"custom","customType":"session-lifecycle","data":{"state":"waiting_background","runActive":false,"waitingOn":{"children":0,"background":[{"source":"senpi-task","activeCount":1}],"human":[]}}}"#).await;
    // When: the owner replies.
    let reply = fixture.run().await;
    // Then: the healthy session is resumed.
    assert_eq!(reply.as_deref(), Some("final"));
    assert!(fixture.script.argv().windows(2).any(|args| args == ["--session", &fixture.script.session()]));
}

#[tokio::test]
async fn a_session_still_settling_on_its_held_engine_keeps_its_thread() {
    // Given: the last turn answered, but its held engine still records the run active.
    let mut fixture = Fixture::new(r#"{"type":"custom","customType":"session-lifecycle","data":{"state":"done","runActive":false}}"#).await;
    fixture.turn.text = "lingering".into();
    assert_eq!(fixture.run().await.as_deref(), Some("final"));
    let mut state = load_state(&fixture.pool, &fixture.turn.key).await.unwrap();
    // When: the owner's next message checks the thread before the engine settles.
    let recap = legacy::recover(&fixture.pool, &fixture.turn.key, &mut state).await.unwrap();
    // Then: the owned session is kept for resume.
    assert_eq!(recap, None);
    assert_eq!(state.metadata.get("omo_thread_id"), Some(&json!(fixture.script.session())));
    fixture.script.release_background().await;
}

#[tokio::test]
async fn an_ack_timeout_drops_the_stored_session() {
    // Given: a settled session whose next prompt is never acknowledged.
    let mut fixture = Fixture::new(r#"{"type":"custom","customType":"session-lifecycle","data":{"state":"done","runActive":false}}"#).await;
    fixture.turn.text = "ack-timeout".into();
    // When: the acknowledgement deadline expires (the behavior under test).
    let reply = fixture.run().await;
    // Then: the next owner message cannot resume the ambiguous session.
    assert_eq!(reply, None);
    let state = load_state(&fixture.pool, &fixture.turn.key).await.unwrap();
    assert!(!state.metadata.contains_key("omo_thread_id"));
}
