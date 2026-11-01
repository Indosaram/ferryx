//! Bounded adoption of a legacy Slack card the bot already posted. The hub reads the exact
//! message back with its own token, checks the bot posted it, and records it in one write
//! transaction as an already-delivered gated report, so owner rules, feedback and card
//! mutations apply to it. Producer content is never accepted and nothing is posted or updated.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::chat::slack::{OwnMessage, SlackMessage};
use crate::report_gate::{Report, Salience};
use crate::storage::WriteTx;
use crate::{HubError, Result};

const MAX_FIELD: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub channel: String,
    pub ts: String,
    pub source: String,
    pub topic: String,
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        let channel = (2..=32).contains(&self.channel.len())
            && self.channel.starts_with(['C', 'D', 'G'])
            && self.channel.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
        let ts = self.ts.len() <= 32 && self.ts.split_once('.').is_some_and(|(secs, frac)|
            !secs.is_empty() && !frac.is_empty()
                && secs.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit()));
        let named = |value: &str| !value.is_empty() && value == value.trim()
            && value.len() <= MAX_FIELD && !value.chars().any(char::is_control);
        if channel && ts && named(&self.source) && named(&self.topic) {
            Ok(())
        } else {
            Err(HubError::message("adoption requires a Slack channel id, exact message ts, source and topic"))
        }
    }

    fn report_id(&self) -> String {
        format!("adopt:{}:{}", self.channel, self.ts)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Adopted { outbox_id: i64, replay: bool },
    Pending,
    Rejected(&'static str),
}

pub async fn reply(pool: &SqlitePool, request: &Request) -> Value {
    match perform(pool, request).await {
        Ok(Outcome::Adopted { outbox_id, replay }) => json!({"status":"ok","op":"report_adopt",
            "adoption":"adopted","replay":replay,"outbox_id":outbox_id,
            "channel":request.channel,"ts":request.ts}),
        Ok(Outcome::Pending) => json!({"status":"pending","op":"report_adopt","code":"slack_token_missing"}),
        Ok(Outcome::Rejected(code)) => json!({"status":"failed","op":"report_adopt","code":code}),
        Err(error) => json!({"status":"failed","op":"report_adopt","error":error.to_string()}),
    }
}

async fn perform(pool: &SqlitePool, request: &Request) -> Result<Outcome> {
    if request.validate().is_err() {
        return Ok(Outcome::Rejected("invalid"));
    }
    // A recorded adoption answers locally: no Slack call for a replay or a conflict.
    let previous = {
        let mut connection = pool.acquire().await?;
        prior(&mut connection, request).await?
    };
    if let Some(outcome) = previous {
        return Ok(outcome);
    }
    let found = crate::chat::slack::read_own_message(&request.channel, &request.ts).await?;
    adopt(pool, request, found).await
}

/// The adoption already recorded for this exact receipt: the same identity replays, anything
/// else (another topic or source, an ungated row, several rows) is a conflict.
async fn prior(conn: &mut sqlx::SqliteConnection, request: &Request) -> Result<Option<Outcome>> {
    let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT o.id, p.report_json FROM chat_outbox o
         LEFT JOIN report_delivery_parts p ON p.outbox_id = o.id
         WHERE o.platform = 'slack' AND o.posted_channel = ? AND o.posted_ts = ?")
        .bind(&request.channel).bind(&request.ts).fetch_all(&mut *conn).await?;
    let [(id, Some(part))] = rows.as_slice() else {
        return Ok((!rows.is_empty()).then_some(Outcome::Rejected("conflict")));
    };
    let report: Report = serde_json::from_str(part).map_err(|error| HubError::message(error.to_string()))?;
    let same = report.id == request.report_id() && report.source == request.source
        && report.topic_key == request.topic;
    Ok(Some(if same { Outcome::Adopted { outbox_id: *id, replay: true } } else { Outcome::Rejected("conflict") }))
}

async fn adopt(pool: &SqlitePool, request: &Request, found: OwnMessage) -> Result<Outcome> {
    let message = match found {
        OwnMessage::Unconfigured => return Ok(Outcome::Pending),
        OwnMessage::NotFound => return Ok(Outcome::Rejected("not_found")),
        OwnMessage::NotOwn => return Ok(Outcome::Rejected("not_own")),
        OwnMessage::Found(message) => message,
    };
    if message.channel != request.channel || message.ts != request.ts {
        return Ok(Outcome::Rejected("not_found"));
    }
    if message.text.trim().is_empty() && message.blocks.is_none() {
        return Ok(Outcome::Rejected("empty"));
    }
    let report = report_for(request, &message);
    let encoded = serde_json::to_string(&report).map_err(|error| HubError::message(error.to_string()))?;
    let blocks = message.blocks.as_ref().map(serde_json::to_string).transpose()
        .map_err(|error| HubError::message(error.to_string()))?;
    let session = crate::chat::session::SessionKey::new(
        "slack", None::<String>, request.channel.clone(), None::<String>, "").storage_key();
    let now = crate::storage::now_rfc3339();
    let mut tx = pool.begin_write().await?;
    if let Some(outcome) = prior(&mut tx, request).await? {
        tx.rollback().await?;
        return Ok(outcome);
    }
    let id = sqlx::query("INSERT INTO chat_outbox(session_key, platform, text, created_at, delivered_at,
            posted_channel, posted_ts, blocks_json) VALUES(?, 'slack', ?, ?, ?, ?, ?, ?)")
        .bind(&session).bind(&message.text).bind(&now).bind(&now)
        .bind(&message.channel).bind(&message.ts).bind(blocks)
        .execute(&mut *tx).await?.last_insert_rowid();
    sqlx::query("INSERT INTO report_delivery_parts(outbox_id, report_id, report_json) VALUES(?, ?, ?)")
        .bind(id).bind(&report.id).bind(encoded).execute(&mut *tx).await?;
    sqlx::query("INSERT OR IGNORE INTO delivered_alerts(slack_channel, slack_ts, source, sender, subject,
            topic_key, alert_ref, created_at) VALUES(?, ?, ?, NULL, ?, ?, ?, ?)")
        .bind(&message.channel).bind(&message.ts).bind(&report.source).bind(&report.body)
        .bind(&report.topic_key).bind(&report.id).bind(&now).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Outcome::Adopted { outbox_id: id, replay: false })
}

/// The gated report of an adopted card. Its body is the card's own text from Slack.
fn report_for(request: &Request, message: &SlackMessage) -> Report {
    let actions = message.blocks.as_deref()
        .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "actions"));
    let salience = if actions { Salience::Decision } else { Salience::Result };
    Report::new(request.report_id(), &request.source, &request.topic, salience, &message.text)
        .with_state(&format!("adopted:{}:{}", request.channel, request.ts))
        .with_origin(format!("slack:{}:{}", request.channel, request.ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "1700000000.000100";

    fn request(topic: &str) -> Request {
        Request { channel: "DOWNER".into(), ts: TS.into(), source: "autopilot".into(), topic: topic.into() }
    }

    async fn count(pool: &SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}")).fetch_one(pool).await.unwrap()
    }

    #[test]
    fn producer_blocks_and_malformed_identity_are_refused() {
        let supplied = json!({"channel":"DOWNER","ts":TS,"source":"autopilot","topic":"card:1",
            "blocks":[{"type":"actions","elements":[]}]});
        assert!(serde_json::from_value::<Request>(supplied).is_err());
        assert!(request("card:1").validate().is_ok());
        for bad in [
            Request { channel: "downer".into(), ..request("card:1") },
            Request { ts: "1700000000".into(), ..request("card:1") },
            Request { ts: "1700000000.x".into(), ..request("card:1") },
            Request { topic: " card:1".into(), ..request("card:1") },
            Request { source: String::new(), ..request("card:1") },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn missing_token_is_pending_and_unverified_messages_write_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        let req = request("card:1");
        assert_eq!(adopt(&pool, &req, OwnMessage::Unconfigured).await.unwrap(), Outcome::Pending);
        assert_eq!(adopt(&pool, &req, OwnMessage::NotFound).await.unwrap(), Outcome::Rejected("not_found"));
        assert_eq!(adopt(&pool, &req, OwnMessage::NotOwn).await.unwrap(), Outcome::Rejected("not_own"));
        let elsewhere = SlackMessage { channel: "DOTHER".into(), ts: TS.into(), text: "x".into(), blocks: None };
        assert_eq!(adopt(&pool, &req, OwnMessage::Found(elsewhere)).await.unwrap(), Outcome::Rejected("not_found"));
        assert_eq!(reply(&pool, &Request { ts: "bad".into(), ..req }).await["code"], "invalid");
        for table in ["chat_outbox", "report_delivery_parts", "delivered_alerts"] {
            assert_eq!(count(&pool, table).await, 0, "{table}");
        }
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn http_adoption_records_the_bots_exact_card_once_and_never_posts() {
        let original = json!([
            {"type":"section","text":{"type":"mrkdwn","text":"지원 여부를 선택해 주세요."}},
            {"type":"actions","elements":[{"type":"button","text":{"type":"plain_text","text":"Approve"},
                "action_id":"omon:btn:autopilot:apply:42","value":"omon:btn:autopilot:apply:42"}]}]);
        let (base, calls) = crate::chat::slack_web::mock::serve_scripted("UBOT", vec![
            ("auth.test", vec![json!({"ok":true,"user_id":"UBOT","bot_id":"BBOT"})]),
            ("conversations.history", vec![json!({"ok":true,"messages":[{"type":"message","ts":TS,
                "user":"UBOT","bot_id":"BBOT","text":"지원 여부를 선택해 주세요.","blocks":original.clone()}]})]),
        ]).await;
        let pool = crate::storage::memory().await.unwrap();
        let (first, replay, conflict) = crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            (reply(&pool, &request("card:42")).await,
             reply(&pool, &request("card:42")).await,
             reply(&pool, &request("card:other")).await)
        }).await;

        assert_eq!(first["status"], "ok", "{first}");
        assert_eq!(first["replay"], false);
        let id = first["outbox_id"].as_i64().unwrap();
        assert_eq!(replay["outbox_id"], id);
        assert_eq!(replay["replay"], true);
        assert_eq!(conflict["code"], "conflict");
        assert_eq!(count(&pool, "chat_outbox").await, 1);

        let (text, delivered, channel, ts, blocks, platform): (String, Option<String>, Option<String>,
            Option<String>, Option<String>, String) = sqlx::query_as(
            "SELECT text, delivered_at, posted_channel, posted_ts, blocks_json, platform FROM chat_outbox WHERE id=?")
            .bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(text, "지원 여부를 선택해 주세요.");
        assert!(delivered.is_some());
        assert_eq!((channel.as_deref(), ts.as_deref(), platform.as_str()), (Some("DOWNER"), Some(TS), "slack"));
        assert_eq!(serde_json::from_str::<Value>(&blocks.unwrap()).unwrap(), original);
        let parts: Vec<String> = sqlx::query_scalar("SELECT report_json FROM report_delivery_parts WHERE outbox_id=?")
            .bind(id).fetch_all(&pool).await.unwrap();
        assert_eq!(parts.len(), 1);
        let report: Report = serde_json::from_str(&parts[0]).unwrap();
        assert_eq!((report.source.as_str(), report.topic_key.as_str()), ("autopilot", "card:42"));
        let status = crate::control::report_status_reply(&pool, id).await;
        assert_eq!((status["delivery"].as_str(), status["ts"].as_str()), (Some("delivered"), Some(TS)));

        // Owner rules recorded after adoption govern the adopted card.
        let scope = crate::report_rules::scope_from_alert("autopilot", None, None, Some("card:42"));
        crate::report_rules::record(&pool, &scope, crate::report_rules::Effect::Mute, "stop", None).await.unwrap();
        assert!(matches!(crate::report_gate::check_parent_policy(&pool, id).await.unwrap(),
            crate::report_gate::ParentPolicy::Muted { .. }));

        // Exactly one verification read; the replay and the conflict did not call Slack; nothing posted.
        let calls = calls.lock().unwrap();
        let methods: Vec<&str> = calls.iter().map(|(method, _)| method.as_str()).collect();
        assert_eq!(methods, ["auth.test", "conversations.history"]);
        let history = &calls[1].1;
        assert_eq!((history["channel"].as_str(), history["latest"].as_str(), history["oldest"].as_str(),
            history["inclusive"].as_str()), (Some("DOWNER"), Some(TS), Some(TS), Some("true")));
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn http_foreign_neighbouring_or_missing_messages_are_not_adopted() {
        let (base, calls) = crate::chat::slack_web::mock::serve_scripted("UBOT", vec![
            ("auth.test", vec![json!({"ok":true,"user_id":"UBOT","bot_id":"BBOT"})]),
            ("conversations.history", vec![
                json!({"ok":true,"messages":[{"type":"message","ts":TS,"user":"USOMEONE","text":"forged",
                    "blocks":[{"type":"actions","elements":[]}]}]}),
                json!({"ok":true,"messages":[{"type":"message","ts":"1700000000.000099",
                    "user":"UBOT","bot_id":"BBOT","text":"older"}]}),
                json!({"ok":false,"error":"channel_not_found"}),
            ]),
        ]).await;
        let pool = crate::storage::memory().await.unwrap();
        let codes = crate::chat::slack::with_test_api(base, "xoxb-test".into(), async {
            let mut codes = Vec::new();
            for _ in 0..3 {
                codes.push(reply(&pool, &request("card:42")).await["code"].clone());
            }
            codes
        }).await;
        assert_eq!(codes, [json!("not_own"), json!("not_found"), json!("not_found")]);
        for table in ["chat_outbox", "report_delivery_parts", "delivered_alerts"] {
            assert_eq!(count(&pool, table).await, 0, "{table}");
        }
        assert!(calls.lock().unwrap().iter()
            .all(|(method, _)| matches!(method.as_str(), "auth.test" | "conversations.history")));
    }
}
