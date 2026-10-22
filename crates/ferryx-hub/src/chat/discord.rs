//! Discord gateway dispatch parsing. The live connection is compiled only with `hub-discord`.
//! The bot token comes from `DISCORD_TOKEN` or `DISCORD_TOKEN_FILE`.
use serde_json::Value;

use super::approval::{parse_omon_approval, ApprovalClick};
use super::router::Inbound;
use crate::config::secret_from_env;
#[cfg(feature = "hub-discord")]
use crate::error::HubError;
use crate::error::Result;

pub const PLATFORM: &str = "discord";

pub fn bot_token() -> Option<String> {
    secret_from_env("DISCORD_BOT_TOKEN").or_else(|| secret_from_env("DISCORD_TOKEN"))
}

pub fn should_auto_thread(enabled: bool, in_thread: bool) -> bool {
    enabled && !in_thread
}

pub fn auto_thread_name(text: &str) -> String {
    let name: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(80).collect();
    if name.is_empty() {
        "thread".into()
    } else {
        name
    }
}

pub fn approval_components(options: &[(&str, &str)]) -> serde_json::Value {
    serde_json::json!({
        "type": 1,
        "components": options.iter().map(|(label, custom_id)| serde_json::json!({
            "type": 2,
            "style": 1,
            "label": label,
            "custom_id": custom_id
        })).collect::<Vec<_>>()
    })
}

pub async fn issue_pairing(
    pool: &sqlx::SqlitePool,
    platform: &str,
    user_id: &str,
    code: &str,
) -> crate::error::Result<()> {
    let now = crate::storage::now_rfc3339();
    sqlx::query(
        "INSERT OR REPLACE INTO pairing_codes(code, platform, user_id, created_at, expires_at) VALUES(?,?,?,?,?)",
    )
    .bind(code)
    .bind(platform)
    .bind(user_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn consume_pairing(
    pool: &sqlx::SqlitePool,
    code: &str,
) -> crate::error::Result<Option<String>> {
    let user: Option<String> = sqlx::query_scalar(
        "SELECT user_id FROM pairing_codes WHERE code = ? AND consumed_at IS NULL",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    if user.is_some() {
        sqlx::query("UPDATE pairing_codes SET consumed_at = ? WHERE code = ?")
            .bind(crate::storage::now_rfc3339())
            .bind(code)
            .execute(pool)
            .await?;
    }
    Ok(user)
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiscordDispatch {
    Inbound(Inbound),
    Approval(ApprovalClick),
    Ignore,
}

pub fn handle_dispatch(frame: &Value) -> Result<DiscordDispatch> {
    if frame.get("op").and_then(Value::as_i64) != Some(0) {
        return Ok(DiscordDispatch::Ignore);
    }
    let kind = frame.get("t").and_then(Value::as_str).unwrap_or("");
    let data = frame.get("d").cloned().unwrap_or(Value::Null);
    match kind {
        "MESSAGE_CREATE" => Ok(message_create(&data)
            .map(DiscordDispatch::Inbound)
            .unwrap_or(DiscordDispatch::Ignore)),
        "INTERACTION_CREATE" => Ok(interaction(&data)?
            .map(DiscordDispatch::Approval)
            .unwrap_or(DiscordDispatch::Ignore)),
        _ => Ok(DiscordDispatch::Ignore),
    }
}

fn message_create(data: &Value) -> Option<Inbound> {
    if data.pointer("/author/bot").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let text = data.get("content").and_then(Value::as_str)?.to_owned();
    if text.trim().is_empty() {
        return None;
    }
    Some(Inbound {
        platform: PLATFORM.into(),
        guild_id: data
            .get("guild_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        channel_id: data.get("channel_id").and_then(Value::as_str)?.to_owned(),
        thread_id: None,
        user_id: data
            .pointer("/author/id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        text,
        parent_chat_id: None,
        event_id: data.get("id").and_then(Value::as_str).map(str::to_owned),
    })
}

fn interaction(data: &Value) -> Result<Option<ApprovalClick>> {
    let custom_id = data
        .pointer("/data/custom_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !custom_id.starts_with("omon:approval:") && !custom_id.starts_with("ferryx:approval:") {
        return Ok(None);
    }
    let session_id = data
        .pointer("/ferryx/sessionId")
        .and_then(Value::as_str)
        .unwrap_or("");
    let revision = data
        .pointer("/ferryx/screenRevision")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(click) = super::approval::parse_approval_value(custom_id) {
        return Ok(Some(click));
    }
    Ok(parse_omon_approval(custom_id, session_id, revision))
}

/// REST body for a channel message. The caller posts it with the bot token.
pub fn channel_message_body(text: &str) -> Value {
    serde_json::json!({ "content": text })
}

/// An inbound message is fenced and sent. An approval click becomes one Answer.
pub async fn apply_dispatch(
    frame: &Value,
    meta_session_id: &str,
    profiles: &super::router::ProfileRouter,
    control: &dyn crate::agent_control::AgentControl,
) -> Result<DiscordDispatch> {
    let parsed = handle_dispatch(frame)?;
    match &parsed {
        DiscordDispatch::Inbound(inbound) => {
            super::router::route_inbound(inbound, profiles, meta_session_id, control).await?;
        }
        DiscordDispatch::Approval(click) => {
            let op =
                super::approval::answer_op(click, &format!("discord-approval-{}", click.prompt_id));
            control.execute(None, op).await?;
        }
        DiscordDispatch::Ignore => {}
    }
    Ok(parsed)
}

/// Posts `text` when `DISCORD_TOKEN` is set. No token reports that nothing was sent.
#[cfg(feature = "hub-discord")]
pub async fn post_if_configured(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    let Some(token) = bot_token() else {
        return Ok(false);
    };
    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "https://discord.com/api/v10/channels/{}/messages",
            session.channel_id
        ))
        .header("Authorization", format!("Bot {token}"))
        .json(&channel_message_body(text))
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(HubError::message(format!(
            "discord channel message failed: {status} {body}"
        )));
    }
    Ok(true)
}

/// Posts one cron or lane delivery into a Discord channel.
#[cfg(feature = "hub-discord")]
pub async fn deliver(channel: &str, text: &str) -> Result<bool> {
    let session =
        super::session::SessionKey::new("discord", Some(channel), channel, None::<String>, "");
    post_if_configured(&session, text).await
}

pub async fn publish(session: &super::session::SessionKey, text: &str) -> Result<bool> {
    #[cfg(feature = "hub-discord")]
    {
        return post_if_configured(session, text).await;
    }
    #[cfg(not(feature = "hub-discord"))]
    {
        let _ = (session, text);
        Ok(false)
    }
}

#[cfg(feature = "hub-discord")]
pub async fn connect_gateway<F, Fut>(mut on_frame: F) -> Result<()>
where
    F: FnMut(Value) -> Fut + Send,
    Fut: std::future::Future<Output = ()> + Send,
{
    let token = bot_token().ok_or_else(|| HubError::config("DISCORD_TOKEN is not set"))?;
    let client = reqwest::Client::new();
    let gateway: Value = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", format!("Bot {token}"))
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let url = gateway
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("discord gateway url missing"))?;
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("{url}/?v=10&encoding=json"))
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    use futures_util::{SinkExt, StreamExt};
    let identify = serde_json::json!({
        "op": 2,
        "d": {"token": token, "intents": 33280, "properties": {"os": "linux", "browser": "ferryx-hub", "device": "ferryx-hub"}}
    });
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            identify.to_string().into(),
        ))
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    while let Some(message) = socket.next().await {
        let message = message.map_err(|error| HubError::message(error.to_string()))?;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
            if let Ok(frame) = serde_json::from_str::<Value>(&text) {
                on_frame(frame).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_message_and_an_approval_interaction_parse() {
        let message = json!({"op":0,"t":"MESSAGE_CREATE","d":{"channel_id":"9","content":"hi","author":{"id":"3","bot":false}}});
        match handle_dispatch(&message).unwrap() {
            DiscordDispatch::Inbound(inbound) => assert_eq!(inbound.text, "hi"),
            other => panic!("expected inbound, got {other:?}"),
        }
        let click = json!({
            "op": 0,
            "t": "INTERACTION_CREATE",
            "d": {
                "data": {"custom_id": "omon:approval:prompt-9:once"},
                "ferryx": {"sessionId": "pane-1", "screenRevision": "rev-1"}
            }
        });
        match handle_dispatch(&click).unwrap() {
            DiscordDispatch::Approval(click) => {
                assert_eq!(click.prompt_id, "prompt-9");
                assert_eq!(click.option_index, 0);
                assert_eq!(click.screen_revision, "rev-1");
            }
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn pairing_is_single_use_and_threads_are_named() {
        assert!(should_auto_thread(true, false));
        assert!(!should_auto_thread(true, true));
        assert_eq!(auto_thread_name("  hello   thread  "), "hello thread");
        let components = approval_components(&[("Yes", "ferryx:approval:1")]);
        assert_eq!(
            components["components"][0]["custom_id"],
            "ferryx:approval:1"
        );
        let pool = crate::storage::memory().await.unwrap();
        issue_pairing(&pool, "discord", "user-1", "123456")
            .await
            .unwrap();
        assert_eq!(
            consume_pairing(&pool, "123456").await.unwrap().as_deref(),
            Some("user-1")
        );
        assert!(consume_pairing(&pool, "123456").await.unwrap().is_none());
    }
}
