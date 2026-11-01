#![cfg(feature = "hub-slack")]

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use ferryx_hub::{judge::Judge, lanes::kibitzer::{ensure_schema, ensure_lead, evaluate_at, ModelTurn}};

struct Scripted { text: String }

#[async_trait]
impl ModelTurn for Scripted {
    async fn complete(&self, _prompt: &str) -> ferryx_hub::error::Result<String> {
        Ok(self.text.clone())
    }
}

#[tokio::test]
async fn generated_nudges_suppress_rewording_but_deliver_a_new_owner_blocker() {
    // The fixture owner is Korean in Asia/Seoul: the 11:00-14:00 local wakes are outside quiet
    // hours and the Korean report bodies are in the owner's language, whatever the host locale.
    ferryx_hub::locale::scope(ferryx_hub::locale::OwnerLocale::korean(), nudges_for_the_fixture_owner()).await;
}

async fn nudges_for_the_fixture_owner() {
    use ferryx_hub::chat::{session::SessionKey, slack_turn::{SessionTurn, turn_report}};
    use ferryx_hub::report_gate::{Decision as GateDecision, queue};
    let pool = ferryx_hub::storage::memory().await.unwrap();
    ensure_schema(&pool).await.unwrap();
    ensure_lead(&pool, "meta").await.unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-30T02:00:00Z").unwrap().with_timezone(&Utc);
    for (id, snippet) in [
        ("fixture:sba-certificate", "중소기업확인서를 10월 30일까지 보내 주세요."),
        ("fixture:band-certificate", "중소기업확인서를 11월 3일 14시까지 제출해 주세요."),
    ] {
        ferryx_hub::inbox::ingest(&pool, id, "mail", id, snippet, "2026-10-29T09:00:00Z").await.unwrap();
        sqlx::query("INSERT INTO inbox_decisions(item_id,class,reason,status,decided_at) VALUES(?,'owner','certificate required','open',?)")
            .bind(id).bind(start.to_rfc3339()).execute(&pool).await.unwrap();
    }
    for (index, expected) in [GateDecision::Sent, GateDecision::Duplicate, GateDecision::Sent, GateDecision::Sent].into_iter().enumerate() {
        if index == 2 {
            sqlx::query("INSERT INTO cron_problems(id,job_id,kind,signature,status,created_at) VALUES(1,'collect','login','new-account','needs_owner','2026-10-30T04:00:00Z')")
                .execute(&pool).await.unwrap();
        }
        if index == 3 {
            sqlx::query("UPDATE inbox_decisions SET status='done' WHERE item_id='fixture:sba-certificate'")
                .execute(&pool).await.unwrap();
        }
        sqlx::query("UPDATE kibitzer_state SET generation = generation + 1 WHERE lead = 'meta'")
            .execute(&pool).await.unwrap();
        let model = Scripted {
            text: serde_json::json!({"wake":true,"reason":format!("reason {index}"),"suggested":format!("suggestion {index}"),"target_lane":null}).to_string(),
        };
        assert!(evaluate_at(&pool, "meta", &Judge::forced("wake"), &model,
            start + TimeDelta::hours(i64::try_from(index).unwrap())).await.unwrap());
        let event: String = sqlx::query_scalar("SELECT body FROM lane_events WHERE kind = 'kibitzer' ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool).await.unwrap();
        let turn = SessionTurn {
            key: SessionKey::new("slack", None::<String>, "DTEST", None::<String>, "UOWNER"),
            reply_channel: Some("DTEST".into()), reply_thread: None, status_ts: None,
            text: format!("[LANE_EVENT]\n- kibitzer: {event}"), background: false,
        };
        let report = turn_report(&turn, &format!("확인이 필요한 사항입니다. {index}"));
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, expected);
    }
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox").fetch_one(&pool).await.unwrap();
    assert_eq!(pending, 3);
}
