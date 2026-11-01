//! Isolated real-Jev feedback replay. No Slack calls or production database access.
//! Usage: feedback_replay <new-database-path>
use ferryx_hub::{judge::Judge, owner_alert::{alert_report, PendingAlert}, report_feedback, report_gate, report_rules, storage};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(std::env::args().nth(1).ok_or("new database path required")?);
    if path.exists() { return Err("replay database must not already exist".into()); }
    let mut pool = storage::connect(&path).await?;
    for source in ["mail", "kakao", "imessage"] {
        let judge = Judge::from_config(ferryx_hub::judge::typesafe_from_env()).with_pool(pool.clone());
        let alert = PendingAlert {
            item_id: format!("{source}:first"), source: source.into(),
            author: Some("Service <notify@example.com>".into()),
            title: Some("New sign-in from location 123".into()),
            snippet: "새 위치에서 로그인했습니다. 확인해 주세요.".into(), link: None,
        };
        let (initial, id) = report_gate::queue(&pool, "owner", &alert_report(&alert), false).await?;
        if initial != report_gate::Decision::Sent { return Err(format!("{source}: default did not send").into()); }
        // Synthetic transport receipt: tests rule linkage, not delivery to a live person.
        report_feedback::posted_from_outbox(&pool, id.ok_or("missing outbox")?,
            &ferryx_hub::chat::reply::PostedMessage { channel: "DTEST".into(), ts: Some(source.into()) }).await?;
        let mute = report_feedback::handle_owner_message(&pool, &judge, "DTEST", Some(source), "이거는 이제 그만 보내. 앞으로 이런 알림은 보내지 마.").await?;
        if mute.is_none() { return Err(format!("{source}: real judge did not recognize mute feedback").into()); }
        let rules = report_rules::active_rules(&pool).await?;
        if !rules.iter().any(|rule| rule.scope.source == source && rule.effect == report_rules::Effect::Mute) {
            return Err(format!("{source}: mute was not persisted").into());
        }
        pool.close().await;
        pool = storage::connect(&path).await?;
        let next = PendingAlert { item_id: format!("{source}:next"), title: Some("New sign-in from location 456".into()), ..alert.clone() };
        let muted = report_gate::queue(&pool, "owner", &alert_report(&next), false).await?.0;
        if muted != report_gate::Decision::Muted { return Err(format!("{source}: new matching item escaped after reopen").into()); }
        let other = PendingAlert { item_id: format!("{source}:payment"), title: Some("Payment approval required".into()), ..alert };
        if report_gate::queue(&pool, "owner", &alert_report(&other), false).await?.0 != report_gate::Decision::Sent {
            return Err(format!("{source}: unrelated action was suppressed").into());
        }
        let judge = Judge::from_config(ferryx_hub::judge::typesafe_from_env()).with_pool(pool.clone());
        let restored = report_feedback::handle_owner_message(&pool, &judge, "DTEST", Some(source), "이건 다시 보내. 앞으로 이런 알림은 꼭 알려 줘.").await?;
        if restored.is_none() { return Err(format!("{source}: real judge did not recognize send-again feedback").into()); }
        if report_gate::queue(&pool, "owner", &alert_report(&next), false).await?.0 != report_gate::Decision::Sent {
            return Err(format!("{source}: new send rule did not supersede mute").into());
        }
        println!("{}", json!({"source":source,"default_send":true,"mute_persisted_after_reopen":true,"unrelated_action_preserved":true,"send_again":true}));
    }
    let calls: Vec<(String, String, bool)> = sqlx::query_as("SELECT purpose, outcome, used_fallback FROM judge_calls WHERE purpose = 'report_feedback'")
        .fetch_all(&pool).await?;
    println!("{}", json!({"feedback_calls":calls}));
    if calls.len() != 6 || calls.iter().any(|(_, _, fallback)| *fallback) {
        return Err("feedback replay requires six successful nonfallback Jev decisions".into());
    }
    pool.close().await;
    Ok(())
}
