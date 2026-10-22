use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionKeyParseError {
    #[error("invalid storage key: input is empty")]
    EmptyInput,
    #[error("invalid storage key: malformed component at byte offset {0}: {1}")]
    MalformedComponent(usize, String),
    #[error("invalid storage key: expected '|' separator at byte offset {0}")]
    ExpectedSeparator(usize),
    #[error("invalid storage key: unexpected trailing data at byte offset {0}")]
    TrailingData(usize),
    #[error("invalid storage key: missing required field '{0}'")]
    MissingRequiredField(&'static str),
}

/// Stable routing identity for a conversation across supported platforms.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct SessionKey {
    pub platform: String,
    pub guild_id: Option<String>,
    pub channel_id: String,
    pub thread_id: Option<String>,
    pub user_id: String,
    /// Platform bot/application identity that received the conversation.
    ///
    /// This is optional for backward compatibility and for non-bot transports,
    /// but Discord ingress always populates it so replies can use the same bot.
    #[serde(default)]
    pub bot_id: Option<String>,
}

impl SessionKey {
    pub fn new(
        platform: impl Into<String>,
        guild_id: Option<impl Into<String>>,
        channel_id: impl Into<String>,
        thread_id: Option<impl Into<String>>,
        user_id: impl Into<String>,
    ) -> Self {
        let guild_id = guild_id.map(Into::into);
        let user_id = if guild_id.is_some() {
            String::new()
        } else {
            user_id.into()
        };
        Self {
            platform: platform.into(),
            guild_id,
            channel_id: channel_id.into(),
            thread_id: thread_id.map(Into::into),
            user_id,
            bot_id: None,
        }
    }

    pub fn with_bot_id(mut self, bot_id: impl Into<String>) -> Self {
        self.bot_id = Some(bot_id.into());
        self
    }

    /// Returns the canonical, collision-resistant storage key.
    ///
    /// Each component is length-prefixed so embedded separators cannot make
    /// distinct session identities produce the same key. The optional bot ID is
    /// appended only when present, preserving storage keys created before
    /// identity-aware routing was introduced.
    ///
    /// Guild channel and thread lanes are permanent per-(bot, channel/thread) and
    /// deliberately exclude the sender identity, ensuring all participants in the
    /// channel share a single conversation lane.
    pub fn storage_key(&self) -> String {
        fn component(value: Option<&str>) -> String {
            match value {
                Some(value) => format!("{}:{value}", value.len()),
                None => "-".to_owned(),
            }
        }

        let user_comp = if self.guild_id.is_some() {
            Some("")
        } else {
            Some(self.user_id.as_str())
        };

        let mut components = vec![
            component(Some(&self.platform)),
            component(self.guild_id.as_deref()),
            component(Some(&self.channel_id)),
            component(self.thread_id.as_deref()),
            component(user_comp),
        ];
        if let Some(bot_id) = self.bot_id.as_deref() {
            components.push(component(Some(bot_id)));
        }
        components.join("|")
    }

    /// Strict inverse parser that reconstructs the full `SessionKey` from a canonical storage key,
    /// preserving the exact `bot_id` and all routing dimensions.
    pub fn from_storage_key(raw: &str) -> Result<Self, SessionKeyParseError> {
        if raw.is_empty() {
            return Err(SessionKeyParseError::EmptyInput);
        }

        let mut rem = raw;
        let mut offset = 0;

        let parse_component =
            |rem: &mut &str, offset: &mut usize| -> Result<Option<String>, SessionKeyParseError> {
                if rem.starts_with('-') {
                    *rem = &rem[1..];
                    *offset += 1;
                    return Ok(None);
                }

                let colon_idx = rem.find(':').ok_or_else(|| {
                    SessionKeyParseError::MalformedComponent(
                        *offset,
                        "missing ':' length separator".to_owned(),
                    )
                })?;

                let len_str = &rem[..colon_idx];
                let len: usize = len_str.parse().map_err(|_| {
                    SessionKeyParseError::MalformedComponent(
                        *offset,
                        format!("invalid length prefix '{len_str}'"),
                    )
                })?;

                let after_colon = &rem[colon_idx + 1..];
                let prefix_len = colon_idx + 1;

                if after_colon.len() < len {
                    return Err(SessionKeyParseError::MalformedComponent(
                        *offset,
                        format!("expected {len} bytes, found {}", after_colon.len()),
                    ));
                }

                if !after_colon.is_char_boundary(len) {
                    return Err(SessionKeyParseError::MalformedComponent(
                        *offset,
                        "byte length does not align with UTF-8 character boundary".to_owned(),
                    ));
                }

                let val = &after_colon[..len];
                *rem = &after_colon[len..];
                *offset += prefix_len + len;

                Ok(Some(val.to_owned()))
            };

        // Component 0: platform (required)
        let platform = parse_component(&mut rem, &mut offset)?
            .ok_or(SessionKeyParseError::MissingRequiredField("platform"))?;

        if !rem.starts_with('|') {
            return Err(SessionKeyParseError::ExpectedSeparator(offset));
        }
        rem = &rem[1..];
        offset += 1;

        // Component 1: guild_id (optional)
        let guild_id = parse_component(&mut rem, &mut offset)?;

        if !rem.starts_with('|') {
            return Err(SessionKeyParseError::ExpectedSeparator(offset));
        }
        rem = &rem[1..];
        offset += 1;

        // Component 2: channel_id (required)
        let channel_id = parse_component(&mut rem, &mut offset)?
            .ok_or(SessionKeyParseError::MissingRequiredField("channel_id"))?;

        if !rem.starts_with('|') {
            return Err(SessionKeyParseError::ExpectedSeparator(offset));
        }
        rem = &rem[1..];
        offset += 1;

        // Component 3: thread_id (optional)
        let thread_id = parse_component(&mut rem, &mut offset)?;

        if !rem.starts_with('|') {
            return Err(SessionKeyParseError::ExpectedSeparator(offset));
        }
        rem = &rem[1..];
        offset += 1;

        // Component 4: user_id (required for DM/guild-less, excluded for guild sessions)
        let user_id = parse_component(&mut rem, &mut offset)?;
        let user_id = match user_id {
            Some(uid) => {
                if guild_id.is_some() {
                    String::new()
                } else {
                    uid
                }
            }
            None if guild_id.is_some() => String::new(),
            None => return Err(SessionKeyParseError::MissingRequiredField("user_id")),
        };

        // Component 5: optional bot_id
        let bot_id = if rem.is_empty() {
            None
        } else if rem.starts_with('|') {
            rem = &rem[1..];
            offset += 1;
            let bot = parse_component(&mut rem, &mut offset)?;
            if !rem.is_empty() {
                return Err(SessionKeyParseError::TrailingData(offset));
            }
            bot
        } else {
            return Err(SessionKeyParseError::ExpectedSeparator(offset));
        };

        Ok(Self {
            platform,
            guild_id,
            channel_id,
            thread_id,
            user_id,
            bot_id,
        })
    }
}

impl FromStr for SessionKey {
    type Err = SessionKeyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_storage_key(s)
    }
}

impl TryFrom<&str> for SessionKey {
    type Error = SessionKeyParseError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::from_storage_key(s)
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.storage_key())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    #[serde(default)]
    pub active_model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub enabled_toolsets: Option<Vec<String>>,
    #[serde(default)]
    pub yolo: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionContext {
    pub key: SessionKey,
    #[serde(default)]
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SessionContext {
    pub fn new(key: SessionKey) -> Self {
        let now = Utc::now();
        Self {
            key,
            state: SessionState::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::SessionKey;

    fn discord_key(thread_id: Option<&str>) -> SessionKey {
        SessionKey::new("discord", Some("guild-1"), "channel-2", thread_id, "user-3")
    }

    #[test]
    fn derives_stable_key_from_every_routing_dimension() {
        let key = discord_key(Some("thread-4"));

        assert_eq!(
            key.storage_key(),
            "7:discord|7:guild-1|9:channel-2|8:thread-4|0:"
        );
        assert_eq!(key.to_string(), key.storage_key());
    }

    #[test]
    fn bot_identity_partitions_discord_sessions_without_breaking_legacy_keys() {
        let legacy = discord_key(Some("thread-4"));
        let bot_a = legacy.clone().with_bot_id("42");
        let bot_b = legacy.clone().with_bot_id("84");

        assert_eq!(
            legacy.storage_key(),
            "7:discord|7:guild-1|9:channel-2|8:thread-4|0:"
        );
        assert_eq!(
            bot_a.storage_key(),
            "7:discord|7:guild-1|9:channel-2|8:thread-4|0:|2:42"
        );
        assert_ne!(bot_a.storage_key(), bot_b.storage_key());
    }

    #[test]
    fn dm_session_key_includes_user_and_guild_session_key_excludes_user() {
        let dm_key = SessionKey::new(
            "discord",
            None::<String>,
            "dm-chan",
            None::<String>,
            "user-123",
        );
        assert_eq!(dm_key.user_id, "user-123");
        assert_eq!(dm_key.storage_key(), "7:discord|-|7:dm-chan|-|8:user-123");

        let guild_alice = SessionKey::new(
            "discord",
            Some("guild-1"),
            "chan-2",
            None::<String>,
            "alice",
        );
        let guild_bob =
            SessionKey::new("discord", Some("guild-1"), "chan-2", None::<String>, "bob");
        assert_eq!(guild_alice, guild_bob);
        assert_eq!(guild_alice.storage_key(), guild_bob.storage_key());
        assert_eq!(
            guild_alice.storage_key(),
            "7:discord|7:guild-1|6:chan-2|-|0:"
        );
    }

    #[test]
    fn differentiates_absent_values_and_separator_like_content() {
        let absent_thread = discord_key(None);
        let empty_thread = discord_key(Some(""));
        let first = SessionKey::new("a|1:b", None::<String>, "c", None::<String>, "d");
        let second = SessionKey::new("a", Some("1:b"), "c", None::<String>, "d");

        assert_ne!(absent_thread, empty_thread);
        assert_ne!(absent_thread.storage_key(), empty_thread.storage_key());
        assert_ne!(first.storage_key(), second.storage_key());
    }

    #[test]
    fn supports_hash_based_session_lookup_and_serde_round_trip() {
        let key = discord_key(Some("thread-4")).with_bot_id("42");
        let mut sessions = HashSet::new();
        sessions.insert(key.clone());

        assert!(sessions.contains(&key));
        let json = serde_json::to_string(&key).expect("session key should serialize");
        let decoded: SessionKey =
            serde_json::from_str(&json).expect("session key should deserialize");
        assert_eq!(decoded, key);
    }

    #[test]
    fn deserializes_pre_identity_session_keys() {
        let json = r#"{"platform":"discord","guild_id":null,"channel_id":"7","thread_id":null,"user_id":"10"}"#;
        let decoded: SessionKey = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.bot_id, None);
    }

    #[test]
    fn storage_key_strict_inverse_round_trip() {
        let cases = vec![
            discord_key(Some("thread-4")),
            discord_key(Some("thread-4")).with_bot_id("42"),
            discord_key(None).with_bot_id("84"),
            SessionKey::new("a|1:b", None::<String>, "c", None::<String>, "d"),
            SessionKey::new(
                "complex:plat",
                Some("guild:1|2"),
                "chan|3:4",
                Some("thr"),
                "user-9",
            )
            .with_bot_id("bot:app|42"),
            SessionKey::new("utf8", Some("가나다"), "channel", None::<String>, "user")
                .with_bot_id("🤖"),
        ];

        for key in cases {
            let encoded = key.storage_key();
            let parsed =
                SessionKey::from_storage_key(&encoded).expect("must parse canonical storage key");
            assert_eq!(
                parsed, key,
                "roundtrip must preserve all dimensions identically"
            );
            let from_str: SessionKey = encoded.parse().expect("FromStr must succeed");
            assert_eq!(from_str, key);
        }
    }

    #[test]
    fn storage_key_parser_rejects_malformed_inputs() {
        assert!(SessionKey::from_storage_key("").is_err());
        assert!(SessionKey::from_storage_key("not-a-key").is_err());
        assert!(SessionKey::from_storage_key("-|-|1:c|-|1:d").is_err()); // platform is None
        assert!(SessionKey::from_storage_key("7:discord|-|9:channel-2|-|6:user-3|extra").is_err());
        assert!(
            SessionKey::from_storage_key("7:discord|-|9:channel-2|-|6:user-3|2:42|trailing")
                .is_err()
        );
        assert!(SessionKey::from_storage_key("99:short|-|1:c|-|1:d").is_err()); // length out of range
    }
}
