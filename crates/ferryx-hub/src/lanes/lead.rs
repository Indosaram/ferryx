//! omo-gateway's lead: the owner's Slack DM session (`runtime_config::default_lead`, set at boot
//! from `slack::home::owner_dm_key`). Lane events, the action loop and owner alerts address it,
//! and routine events go to the home channel session (`slack::home::report_key`).
use std::sync::OnceLock;

use sqlx::SqlitePool;

use crate::chat::session::SessionKey;

static DEFAULT_LEAD: OnceLock<String> = OnceLock::new();

pub fn set_default_lead(lead: String) -> bool {
    DEFAULT_LEAD.set(lead).is_ok()
}

/// The boot-resolved lead, then `OMON_LANE_DEFAULT_LEAD`, then `fallback`.
pub fn default_lead_or(fallback: &str) -> String {
    if let Some(lead) = DEFAULT_LEAD.get() {
        return lead.clone();
    }
    crate::hub_env::var("OMON_LANE_DEFAULT_LEAD")
        .ok()
        .map(|lead| lead.trim().to_owned())
        .filter(|lead| !lead.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// The owner's Slack DM session key: `OMON_LANE_DEFAULT_LEAD`, else the DM session omo-gateway
/// (or this hub) already keeps for the owner, else `opened` (channel, bot user) from
/// `conversations.open`, which is persisted like omo's `persist_session`.
pub async fn owner_dm_lead(
    pool: &SqlitePool,
    owner: &str,
    opened: Option<(String, String)>,
) -> Option<String> {
    if let Some(lead) = crate::hub_env::var("OMON_LANE_DEFAULT_LEAD")
        .ok()
        .map(|lead| lead.trim().to_owned())
        .filter(|lead| !lead.is_empty())
    {
        return Some(lead);
    }
    if let Some((channel, bot)) = opened {
        let key = SessionKey::new("slack", None::<String>, channel, None::<String>, owner)
            .with_bot_id(bot)
            .storage_key();
        let _ = sqlx::query(
            "INSERT INTO sessions (session_key, platform, guild_id, channel_id, thread_id, user_id, state_json, updated_at)
             SELECT ?, 'slack', NULL, ?, NULL, ?, '{}', ?
             WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE session_key = ?)",
        )
        .bind(&key)
        .bind(SessionKey::from_storage_key(&key).ok()?.channel_id)
        .bind(owner)
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false))
        .bind(&key)
        .execute(pool)
        .await;
        return Some(key);
    }
    known_owner_dm(pool, owner).await
}

/// Resolves the lead once per process like omo's boot (`set_default_lead`): the owner DM
/// session, else `fallback`. Every caller after the first gets the same key.
pub async fn resolve(pool: &SqlitePool, owner: &str, fallback: &str) -> String {
    static RESOLVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _once = RESOLVING.lock().await;
    if let Some(lead) = DEFAULT_LEAD.get() {
        return lead.clone();
    }
    #[cfg_attr(not(feature = "hub-slack"), allow(unused_mut))]
    let mut lead = owner_dm_lead(pool, owner, None).await;
    #[cfg(feature = "hub-slack")]
    if lead.is_none() {
        let opened = crate::chat::slack::open_owner_dm(owner).await;
        lead = owner_dm_lead(pool, owner, opened).await;
    }
    match lead {
        Some(lead) => {
            tracing::info!(lead = %lead, "Slack owner DM is the lead");
            set_default_lead(lead.clone());
            lead
        }
        None => {
            tracing::warn!(
                lead = fallback,
                "the owner DM is unknown; lane events keep the configured lead"
            );
            fallback.to_owned()
        }
    }
}

/// The newest stored Slack DM session of the owner.
pub async fn known_owner_dm(pool: &SqlitePool, owner: &str) -> Option<String> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT session_key FROM sessions
         WHERE platform = 'slack' AND guild_id IS NULL AND (thread_id IS NULL OR thread_id = '')
           AND user_id = ? AND channel_id LIKE 'D%'
         ORDER BY updated_at DESC",
    )
    .bind(owner)
    .fetch_all(pool)
    .await
    .ok()?;
    rows.into_iter()
        .find(|raw| SessionKey::from_storage_key(raw).is_ok_and(|key| key.bot_id.is_some()))
}

/// omo's `report_key`: the first home channel's session in the team the bot talks in, carrying
/// the lead's bot id. `None` when the lead is not a Slack DM or no home channel is set.
pub async fn report_target(pool: &SqlitePool, lead: &SessionKey) -> Option<SessionKey> {
    if lead.platform != "slack" || lead.guild_id.is_some() {
        return None;
    }
    let home = crate::hub_env::var("SLACK_HOME_CHANNEL")
        .ok()
        .as_deref()
        .and_then(crate::config::normalize_slack_home)?;
    let team: Option<String> = sqlx::query_scalar(
        "SELECT guild_id FROM sessions WHERE platform = 'slack' AND guild_id IS NOT NULL AND guild_id <> ''
         ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let team = team.filter(|team| !team.is_empty())?;
    let mut key = SessionKey::new("slack", Some(team), home, None::<String>, "");
    if let Some(bot) = lead.bot_id.as_deref() {
        key = key.with_bot_id(bot);
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_lead_is_the_owner_dm_omo_keeps() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = SessionKey::new("slack", None::<String>, "D0DM", None::<String>, "U0OWNER")
            .with_bot_id("U0BOT")
            .storage_key();
        sqlx::query("INSERT INTO sessions(session_key, platform, guild_id, channel_id, thread_id, user_id, state_json, updated_at) VALUES(?, 'slack', NULL, 'D0DM', NULL, 'U0OWNER', '{}', '2026-10-22T00:00:00+00:00')")
            .bind(&dm)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            owner_dm_lead(&pool, "U0OWNER", None).await.as_deref(),
            Some(dm.as_str())
        );
        assert_eq!(owner_dm_lead(&pool, "U0OTHER", None).await, None);
        let opened = owner_dm_lead(&pool, "U0OTHER", Some(("D0NEW".into(), "U0BOT".into())))
            .await
            .unwrap();
        let key = SessionKey::from_storage_key(&opened).unwrap();
        assert_eq!(key.channel_id, "D0NEW");
        assert_eq!(key.bot_id.as_deref(), Some("U0BOT"));
        assert_eq!(
            known_owner_dm(&pool, "U0OTHER").await.as_deref(),
            Some(opened.as_str())
        );
    }
}
