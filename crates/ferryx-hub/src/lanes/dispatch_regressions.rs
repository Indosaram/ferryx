use super::*;

struct Fixture {
    pool: SqlitePool,
    lead: String,
    now: chrono::DateTime<chrono::Utc>,
}

impl Fixture {
    async fn new() -> Self {
        Self {
            pool: crate::storage::memory().await.unwrap(),
            lead: SessionKey::new("slack", None::<String>, "DQA", None::<String>, "UOWNER")
                .with_bot_id("UBOT")
                .storage_key(),
            now: chrono::DateTime::parse_from_rfc3339("2026-11-27T01:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        }
    }

    async fn event(&self, subject: &str, state: &str, body: &str) {
        sqlx::query("INSERT INTO lane_events(lead_session_key,kind,body,created_at,subject_id,state_id) VALUES(?,'action_loop',?,?,?,?)")
            .bind(&self.lead).bind(body)
            .bind((self.now - chrono::Duration::minutes(1)).to_rfc3339())
            .bind(subject).bind(state).execute(&self.pool).await.unwrap();
    }

    async fn tick(&self) -> usize {
        dispatch_tick(&self.pool, &crate::judge::Judge::forced_noul(1.0), self.now, |_, _| async { true })
            .await.unwrap()
    }

    async fn replay_fixture(&self) -> usize {
        let fixture: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../tests/fixtures/lane_events_20261127.json")).unwrap();
        let mut pr_turns = 0;
        let mut now = chrono::DateTime::parse_from_rfc3339(fixture[0]["created_at"].as_str().unwrap()).unwrap().with_timezone(&chrono::Utc) + chrono::Duration::seconds(31);
        for item in fixture.iter().take(8) {
            let body = item["body"].as_str().unwrap();
            let at = chrono::DateTime::parse_from_rfc3339(item["created_at"].as_str().unwrap()).unwrap().with_timezone(&chrono::Utc);
            sqlx::query("INSERT INTO lane_events(lead_session_key,kind,body,created_at,subject_id,state_id) VALUES(?,?,?,?,?,?)")
                .bind(&self.lead).bind("action_loop").bind(body).bind(at.to_rfc3339())
                .bind("action_loop:github:Indosaram/ferryx#50").bind("fixture-head-a")
                .execute(&self.pool).await.unwrap();
            now = now.max(at + chrono::Duration::seconds(31));
            let admitted = dispatch_tick(&self.pool, &crate::judge::Judge::forced_noul(1.0), now, |_, _| async { true }).await.unwrap();
            pr_turns += admitted;
            now += chrono::Duration::minutes(1);
        }
        let urgent = fixture[8]["body"].as_str().unwrap();
        let urgent_at = chrono::DateTime::parse_from_rfc3339(fixture[8]["created_at"].as_str().unwrap()).unwrap().with_timezone(&chrono::Utc);
        sqlx::query("INSERT INTO lane_events(lead_session_key,kind,body,created_at,subject_id,state_id) VALUES(?,'inbox_urgent',?,?,?,?)")
            .bind(&self.lead).bind(urgent).bind(urgent_at.to_rfc3339()).bind("inbox_urgent:fixture").bind("due-fixture")
            .execute(&self.pool).await.unwrap();
        let urgent_turns = dispatch_tick(&self.pool, &crate::judge::Judge::forced_noul(1.0), now.max(urgent_at), |_, _| async { true }).await.unwrap();
        assert_eq!(pr_turns, 1, "PR deliveries collapse to one lead turn");
        assert_eq!(urgent_turns, 1, "urgent inbox still gets a turn");
        pr_turns + urgent_turns
    }
}

#[tokio::test]
async fn the_same_pr_notice_with_new_wording_does_not_wake_the_lead_again() {
    // Given: an admitted revision of a PR.
    let fixture = Fixture::new().await;
    fixture.event("action_loop:github:Indosaram/ferryx#50", "head-a", "first wording").await;
    assert_eq!(fixture.tick().await, 1);
    fixture.event("action_loop:github:Indosaram/ferryx#50", "head-a", "entirely different wording").await;
    // When: the producer reports the same fact after a full day.
    let turns = fixture.tick().await;
    // Then: wording cannot buy another turn.
    assert_eq!(turns, 0);
}

#[tokio::test]
async fn a_new_pr_head_wakes_once() {
    // Given: an admitted old head, followed by a new head with unchanged prose.
    let mut fixture = Fixture::new().await;
    fixture.event("action_loop:github:Indosaram/ferryx#50", "head-a", "PR notice").await;
    assert_eq!(fixture.tick().await, 1);
    fixture.now += chrono::Duration::seconds(31);
    fixture.event("action_loop:github:Indosaram/ferryx#50", "head-b", "PR notice").await;
    // When: the new state is dispatched.
    let turns = fixture.tick().await;
    // Then: the new head wakes even though the notice text did not change.
    assert_eq!(turns, 1);
}

#[tokio::test]
async fn redacted_fixture_replay_collapses_pr_notice_without_dropping_urgent_inbox() {
    // Given: the committed, redacted event replay for the incident window.
    let fixture = Fixture::new().await;
    // When: each fixture delivery is replayed through dispatch.
    let turns = fixture.replay_fixture().await;
    // Then: one PR turn and one urgent inbox turn are admitted.
    assert_eq!(turns, 2);
}

#[tokio::test]
async fn quiet_events_ride_on_the_next_waking_turn() {
    // Given: a quiet bot-handled event that has waited longer than the batch window.
    let fixture = Fixture::new().await;
    fixture.event("action_loop:inbox:bot-1", "reply-1", "bot handled").await;
    sqlx::query("UPDATE lane_events SET wake_policy = 'quiet'").execute(&fixture.pool).await.unwrap();
    assert_eq!(fixture.tick().await, 0);
    sqlx::query("INSERT INTO lane_events(lead_session_key,kind,body,created_at) VALUES(?,'inbox_urgent','inbox:urgent-1',?)")
        .bind(&fixture.lead).bind(fixture.now.to_rfc3339()).execute(&fixture.pool).await.unwrap();
    let mut heard = String::new();
    // When: an urgent event arrives at the same instant, without aging the window.
    let turns = dispatch_tick(&fixture.pool, &crate::judge::Judge::forced_noul(0.0), fixture.now, |_, text| {
        heard = text;
        async { true }
    }).await.unwrap();
    // Then: one immediate turn contains both the wake and the quiet block.
    assert_eq!(turns, 1);
    assert!(heard.contains("[QUIET_EVENTS]"));
    let reasons: Vec<String> = sqlx::query_scalar("SELECT delivery_reason FROM lane_events ORDER BY id")
        .fetch_all(&fixture.pool).await.unwrap();
    assert_eq!(reasons, ["lead_turn", "lead_turn"]);
}
