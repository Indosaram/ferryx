//! Profile routing ported from omo-gateway, plus the platform dispatcher.
//! Slack and Discord ids that are not decimal use the session key as a lane lead.
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize};

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::fence::fence_untrusted;
use super::session::{SessionContext, SessionKey};
use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};

fn deserialize_optional_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumericOrString {
        U64(u64),
        I64(i64),
        String(String),
    }
    let opt: Option<NumericOrString> = Option::deserialize(deserializer)?;
    match opt {
        Some(NumericOrString::U64(value)) => Ok(Some(value)),
        Some(NumericOrString::I64(value)) => u64::try_from(value).map(Some).map_err(|_| {
            serde::de::Error::custom(format!("negative id {value} is not a valid Discord id"))
        }),
        Some(NumericOrString::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                trimmed
                    .parse::<u64>()
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        None => Ok(None),
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRoute {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(
        default,
        alias = "guild_id",
        deserialize_with = "deserialize_optional_u64"
    )]
    pub guild: Option<u64>,
    #[serde(
        default,
        alias = "channel_id",
        alias = "chat_id",
        deserialize_with = "deserialize_optional_u64"
    )]
    pub channel: Option<u64>,
    #[serde(
        default,
        alias = "thread_id",
        deserialize_with = "deserialize_optional_u64"
    )]
    pub thread: Option<u64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default, alias = "toolsets")]
    pub enabled_toolsets: Option<Vec<String>>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub allowed_users: Option<Vec<u64>>,
    /// Pane that should receive this route. Empty means the meta-agent pane.
    #[serde(default)]
    pub pane_session_id: Option<String>,
}

impl ProfileRoute {
    pub fn specificity(&self) -> u32 {
        let mut score = 0;
        if self.guild.is_some() {
            score += 2;
        }
        if self.channel.is_some() {
            score += 4;
        }
        if self.thread.is_some() {
            score += 8;
        }
        score
    }

    pub fn matches(&self, guild_id: Option<u64>, channel_id: u64, thread_id: Option<u64>) -> bool {
        if !self.enabled {
            return false;
        }
        if let Some(required) = self.thread {
            if thread_id != Some(required) {
                return false;
            }
        }
        if let Some(required) = self.channel {
            if channel_id != required {
                return false;
            }
        }
        if let Some(required) = self.guild {
            if guild_id != Some(required) {
                return false;
            }
        }
        true
    }

    pub fn apply_to_session(&self, session: &mut SessionContext) {
        if session.state.active_model.is_none() {
            if let Some(model) = &self.model {
                session.state.active_model = Some(model.clone());
            }
        }
        if session.state.system_prompt.is_none() {
            if let Some(prompt) = &self.system_prompt {
                session.state.system_prompt = Some(prompt.clone());
            }
        }
        if let Some(toolsets) = &self.enabled_toolsets {
            if session.state.enabled_toolsets.is_none() {
                session.state.enabled_toolsets = Some(toolsets.clone());
            }
        }
    }
}

impl fmt::Display for ProfileRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ProfileRoute(name={:?}, guild={:?}, channel={:?}, thread={:?}, model={:?})",
            self.name, self.guild, self.channel, self.thread, self.model
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileRouter {
    routes: Vec<ProfileRoute>,
}

impl ProfileRouter {
    pub fn new(mut routes: Vec<ProfileRoute>) -> Self {
        routes.sort_by_key(|route| std::cmp::Reverse(route.specificity()));
        Self { routes }
    }

    pub fn from_json(json: &str) -> Self {
        let routes = serde_json::from_str::<Vec<ProfileRoute>>(json).unwrap_or_default();
        Self::new(routes)
    }

    pub fn match_route(
        &self,
        guild_id: Option<u64>,
        channel_id: u64,
        thread_id: Option<u64>,
    ) -> Option<&ProfileRoute> {
        self.match_route_with_parent(guild_id, channel_id, thread_id, None)
    }

    pub fn match_route_with_parent(
        &self,
        guild_id: Option<u64>,
        channel_id: u64,
        thread_id: Option<u64>,
        parent_chat_id: Option<u64>,
    ) -> Option<&ProfileRoute> {
        if let Some(tid) = thread_id {
            if let Some(route) = self.routes.iter().find(|route| {
                route.thread == Some(tid)
                    && (route.channel.is_none()
                        || route.channel == Some(channel_id)
                        || parent_chat_id.is_some_and(|parent| route.channel == Some(parent)))
                    && (route.guild.is_none() || route.guild == guild_id)
                    && route.enabled
            }) {
                return Some(route);
            }
            if let Some(route) = self.routes.iter().find(|route| {
                route.channel == Some(tid)
                    && route.thread.is_none()
                    && (route.guild.is_none() || route.guild == guild_id)
                    && route.enabled
            }) {
                return Some(route);
            }
        }
        if let Some(route) = self
            .routes
            .iter()
            .find(|route| route.matches(guild_id, channel_id, thread_id))
        {
            return Some(route);
        }
        if thread_id.is_some() {
            if let Some(route) = self.routes.iter().find(|route| {
                route.channel == Some(channel_id)
                    && route.thread.is_none()
                    && (route.guild.is_none() || route.guild == guild_id)
                    && route.enabled
            }) {
                return Some(route);
            }
        }
        if let Some(parent_id) = parent_chat_id {
            if parent_id != channel_id {
                if let Some(route) = self.routes.iter().find(|route| {
                    route.channel == Some(parent_id)
                        && route.thread.is_none()
                        && (route.guild.is_none() || route.guild == guild_id)
                        && route.enabled
                }) {
                    return Some(route);
                }
            }
        }
        if let Some(gid) = guild_id {
            if let Some(route) = self.routes.iter().find(|route| {
                route.guild == Some(gid)
                    && route.channel.is_none()
                    && route.thread.is_none()
                    && route.enabled
            }) {
                return Some(route);
            }
        }
        None
    }

    pub fn match_session(&self, session_key: &SessionKey) -> Option<&ProfileRoute> {
        let guild_id = session_key
            .guild_id
            .as_deref()
            .and_then(|value| value.parse().ok());
        let channel_id = session_key.channel_id.parse().ok()?;
        let thread_id = session_key
            .thread_id
            .as_deref()
            .and_then(|value| value.parse().ok());
        self.match_route(guild_id, channel_id, thread_id)
    }
}

#[async_trait]
pub trait Outbound: Send + Sync {
    async fn send(&self, session: &SessionKey, text: &str) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct PlatformRouter {
    routes: HashMap<String, Arc<dyn Outbound>>,
    fallback: Option<Arc<dyn Outbound>>,
}

impl PlatformRouter {
    pub fn new(fallback: Arc<dyn Outbound>) -> Self {
        Self {
            routes: HashMap::new(),
            fallback: Some(fallback),
        }
    }

    pub fn for_transports(
        discord: Option<Arc<dyn Outbound>>,
        slack: Option<Arc<dyn Outbound>>,
    ) -> Option<Self> {
        let mut router = Self::new(discord.clone().or_else(|| slack.clone())?);
        if let Some(discord) = discord {
            router = router.with_platform("discord", discord);
        }
        if let Some(slack) = slack {
            router = router.with_platform("slack", slack);
        }
        Some(router)
    }

    pub fn with_platform(mut self, platform: &str, dispatcher: Arc<dyn Outbound>) -> Self {
        self.routes
            .insert(platform.to_ascii_lowercase(), dispatcher);
        self
    }

    pub async fn send(&self, session: &SessionKey, text: &str) -> Result<()> {
        let dispatcher = self
            .routes
            .get(&session.platform.to_ascii_lowercase())
            .or(self.fallback.as_ref())
            .ok_or_else(|| {
                HubError::config(format!(
                    "no outbound dispatcher for platform {}",
                    session.platform
                ))
            })?;
        dispatcher.send(session, text).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inbound {
    pub platform: String,
    pub guild_id: Option<String>,
    pub channel_id: String,
    pub thread_id: Option<String>,
    pub user_id: String,
    pub text: String,
    pub parent_chat_id: Option<String>,
    /// Platform id of this one message. A retry of the same message keeps the same agent-control request id.
    pub event_id: Option<String>,
}

/// Pane a typed message on this inbound would reach. A voice utterance uses the same pane.
pub fn inbound_pane(inbound: &Inbound, profiles: &ProfileRouter, meta_session_id: &str) -> String {
    inbound_target(inbound, profiles, meta_session_id).1
}

fn inbound_target(
    inbound: &Inbound,
    profiles: &ProfileRouter,
    meta_session_id: &str,
) -> (SessionKey, String) {
    let key = SessionKey::new(
        inbound.platform.clone(),
        inbound.guild_id.clone(),
        inbound.channel_id.clone(),
        inbound.thread_id.clone(),
        inbound.user_id.clone(),
    );
    let route = profiles.match_session(&key).or_else(|| {
        let parent = inbound
            .parent_chat_id
            .as_deref()
            .and_then(|value| value.parse().ok());
        let guild = inbound
            .guild_id
            .as_deref()
            .and_then(|value| value.parse().ok());
        let channel = inbound.channel_id.parse().ok()?;
        let thread = inbound
            .thread_id
            .as_deref()
            .and_then(|value| value.parse().ok());
        profiles.match_route_with_parent(guild, channel, thread, parent)
    });
    let session_id = route
        .and_then(|route| route.pane_session_id.clone())
        .unwrap_or_else(|| meta_session_id.to_owned());
    (key, session_id)
}

/// Routes an inbound message to the meta-agent pane, or to a lane pane the profile names.
/// The body is fenced untrusted text. This never reads the pane.
pub async fn route_inbound(
    inbound: &Inbound,
    profiles: &ProfileRouter,
    meta_session_id: &str,
    control: &dyn AgentControl,
) -> Result<AgentOp> {
    let (key, session_id) = inbound_target(inbound, profiles, meta_session_id);
    let origin = format!(
        "{} channel={} user={}",
        inbound.platform, inbound.channel_id, inbound.user_id
    );
    let op = AgentOp::Send {
        session_id,
        text: fence_untrusted(&origin, &inbound.text),
        request_id: format!("chat-{}-{}", key.storage_key(), message_token(inbound)),
    };
    control.execute(None, op.clone()).await?;
    Ok(op)
}

/// Remembers that this pane's next completion belongs to the chat that started it.
pub async fn remember_chat_origin(
    pool: &SqlitePool,
    pane_session_id: &str,
    origin: &SessionKey,
) -> Result<()> {
    if pane_session_id.is_empty() {
        return Ok(());
    }
    sqlx::query("INSERT OR REPLACE INTO hub_meta(key, value) VALUES(?, ?)")
        .bind(format!("chat_origin:{pane_session_id}"))
        .bind(origin.storage_key())
        .execute(pool)
        .await?;
    Ok(())
}

/// Same memory, using the pane `route_inbound` just targeted.
pub async fn remember_inbound_origin(
    pool: &SqlitePool,
    inbound: &Inbound,
    profiles: &ProfileRouter,
    meta_session_id: &str,
) -> Result<()> {
    let (key, session_id) = inbound_target(inbound, profiles, meta_session_id);
    remember_chat_origin(pool, &session_id, &key).await
}

/// Reads and clears the chat that started this pane, if one was stored.
pub async fn take_chat_origin(
    pool: &SqlitePool,
    pane_session_id: &str,
) -> Result<Option<SessionKey>> {
    if pane_session_id.is_empty() {
        return Ok(None);
    }
    let meta_key = format!("chat_origin:{pane_session_id}");
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = ?")
        .bind(&meta_key)
        .fetch_optional(pool)
        .await?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    sqlx::query("DELETE FROM hub_meta WHERE key = ?")
        .bind(&meta_key)
        .execute(pool)
        .await?;
    Ok(SessionKey::from_storage_key(&stored).ok())
}

/// Slack `ts` or Discord message id when the platform sent one. Otherwise a hash of the text,
/// so two different messages are not treated as a replay of the first.
fn message_token(inbound: &Inbound) -> String {
    if let Some(id) = inbound
        .event_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return id.to_owned();
    }
    Sha256::digest(inbound.text.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;

    #[test]
    fn a_thread_route_beats_a_channel_route_and_a_negative_id_is_rejected() {
        let router = ProfileRouter::from_json(
            r#"[{"channel":"10","model":"channel"},{"thread":300,"channel":10,"model":"thread"}]"#,
        );
        let matched = router.match_route(None, 10, Some(300)).unwrap();
        assert_eq!(matched.model.as_deref(), Some("thread"));
        let channel = router.match_route(None, 10, None).unwrap();
        assert_eq!(channel.model.as_deref(), Some("channel"));
        let error = serde_json::from_str::<ProfileRoute>(r#"{"channel":-1}"#);
        assert!(error.is_err());
    }

    #[tokio::test]
    async fn inbound_slack_text_is_fenced_and_sent_without_a_screen_read() {
        let control = RecordingControl::default();
        let profiles = ProfileRouter::new(vec![]);
        let inbound = Inbound {
            platform: "slack".into(),
            guild_id: None,
            channel_id: "C1".into(),
            thread_id: None,
            user_id: "U1".into(),
            text: "status?".into(),
            parent_chat_id: None,
            event_id: Some("1.0".into()),
        };
        let op = route_inbound(&inbound, &profiles, "meta-pane", &control)
            .await
            .unwrap();
        match op {
            AgentOp::Send {
                session_id, text, ..
            } => {
                assert_eq!(session_id, "meta-pane");
                assert!(text.contains("origin: slack channel=C1 user=U1"));
                assert!(text.contains("status?"));
            }
            other => panic!("expected a send, got {other:?}"),
        }
        assert!(control
            .ops()
            .iter()
            .all(|(_, op)| !matches!(op, AgentOp::Read { .. })));
    }
}
