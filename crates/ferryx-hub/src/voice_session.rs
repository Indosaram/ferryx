//! A Discord voice utterance continues the text channel's pane.
//!
//! omo binds the voice channel to the text [`SessionKey`] and routes the transcript
//! through that session (`MultiplexerVoiceModel`). The spoken text is the assistant
//! rows that turn stored, not a second model call. Here the text session is the pane
//! `route_inbound` would send a typed message to. The reply is that pane's new
//! assistant lines, or the new screen suffix once the pane is idle.

use crate::storage::WriteTx;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agent_control::{AgentControl, AgentOp};
use crate::chat::router::{inbound_pane, route_inbound, Inbound, ProfileRouter};
use crate::chat::session::SessionKey;
use crate::error::{HubError, Result};
use crate::sessions::external::{screen_diff, snapshot_from_read, PaneSnapshot};
use crate::storage::now_rfc3339;

const VOICE_TURN_TIMEOUT_MS: u64 = 120_000;

pub struct VoiceTurnContext {
    pub pool: SqlitePool,
    pub profiles: ProfileRouter,
    pub meta_session_id: String,
    pub control: Arc<dyn AgentControl>,
}

struct Installed {
    context: StdMutex<Option<Arc<VoiceTurnContext>>>,
}

fn installed() -> &'static Installed {
    static SLOT: OnceLock<Installed> = OnceLock::new();
    SLOT.get_or_init(|| Installed {
        context: StdMutex::new(None),
    })
}

fn bindings() -> &'static StdMutex<HashMap<u64, SessionKey>> {
    static BOUND: OnceLock<StdMutex<HashMap<u64, SessionKey>>> = OnceLock::new();
    BOUND.get_or_init(|| StdMutex::new(HashMap::new()))
}

pub fn install(context: Arc<VoiceTurnContext>) {
    if let Ok(mut slot) = installed().context.lock() {
        *slot = Some(context);
    }
}

fn context() -> Option<Arc<VoiceTurnContext>> {
    installed()
        .context
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

/// Guild text channel a typed Discord message uses. The sender is not part of the key.
pub fn discord_text_session(guild_id: u64, channel_id: &str) -> SessionKey {
    SessionKey::new(
        "discord",
        Some(guild_id.to_string()),
        channel_id,
        None::<String>,
        "",
    )
}

pub fn bind(voice_channel: u64, session: SessionKey) {
    if let Ok(mut map) = bindings().lock() {
        map.insert(voice_channel, session);
    }
}

pub fn bound(voice_channel: u64) -> Option<SessionKey> {
    bindings()
        .lock()
        .ok()
        .and_then(|map| map.get(&voice_channel).cloned())
}

/// Routes one transcript and returns the assistant text that turn produced.
pub async fn respond_bound(
    voice_channel: u64,
    speaker: Option<u64>,
    transcript: &str,
) -> Result<String> {
    let ctx =
        context().ok_or_else(|| HubError::message("voice turns have no session multiplexer"))?;
    respond_with_speaker(&ctx, voice_channel, speaker, transcript).await
}

pub async fn respond_with_speaker(
    ctx: &VoiceTurnContext,
    voice_channel: u64,
    speaker: Option<u64>,
    transcript: &str,
) -> Result<String> {
    let session = bound(voice_channel).ok_or_else(|| {
        HubError::message(format!(
            "no text session is bound to voice channel {voice_channel}"
        ))
    })?;
    let message_id = Uuid::new_v4();
    let inbound = voice_inbound(&session, speaker, transcript, message_id);
    let session_id = inbound_pane(&inbound, &ctx.profiles, &ctx.meta_session_id);
    let before = read_pane(ctx.control.as_ref(), &session_id).await;
    let cursor = event_cursor(ctx.control.as_ref()).await;
    ensure_session(&ctx.pool, &session).await?;
    insert_message(
        &ctx.pool,
        &session,
        &message_id.to_string(),
        "user",
        transcript,
    )
    .await?;
    route_inbound(
        &inbound,
        &ctx.profiles,
        &ctx.meta_session_id,
        ctx.control.as_ref(),
    )
    .await?;
    if let Err(error) = crate::chat::router::remember_inbound_origin(
        &ctx.pool,
        &inbound,
        &ctx.profiles,
        &ctx.meta_session_id,
    )
    .await
    {
        tracing::warn!(%error, "voice chat origin was not stored");
    }
    let before = before?;
    let revision = (!before.revision.is_empty()).then(|| before.revision.clone());
    ctx.control
        .execute(
            None,
            AgentOp::Wait {
                session_id: session_id.clone(),
                until: "idle".into(),
                after_revision: revision,
                after: cursor,
                timeout_ms: VOICE_TURN_TIMEOUT_MS,
            },
        )
        .await?;
    let after = read_pane(ctx.control.as_ref(), &session_id).await?;
    let parts = assistant_parts(&before, &after);
    if parts.is_empty() {
        return Err(HubError::message("voice turn produced no assistant reply"));
    }
    for part in &parts {
        insert_message(
            &ctx.pool,
            &session,
            &Uuid::new_v4().to_string(),
            "assistant",
            part,
        )
        .await?;
    }
    assistant_reply_after(&ctx.pool, &session, message_id).await
}

pub fn voice_inbound(
    session: &SessionKey,
    speaker: Option<u64>,
    transcript: &str,
    message_id: Uuid,
) -> Inbound {
    Inbound {
        platform: session.platform.clone(),
        guild_id: session.guild_id.clone(),
        channel_id: session.channel_id.clone(),
        thread_id: session.thread_id.clone(),
        user_id: speaker.map(|id| id.to_string()).unwrap_or_default(),
        text: transcript.to_owned(),
        parent_chat_id: None,
        event_id: Some(format!("voice:{message_id}")),
    }
}

/// Assistant rows written after this utterance, oldest first.
pub async fn assistant_reply_after(
    pool: &SqlitePool,
    session: &SessionKey,
    inbound_id: Uuid,
) -> Result<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT content FROM messages
         WHERE session_key = ? AND role = 'assistant'
           AND sequence > COALESCE((SELECT sequence FROM messages WHERE id = ?), 0)
         ORDER BY sequence ASC",
    )
    .bind(session.storage_key())
    .bind(inbound_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| row.0)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// New assistant lines when the pane published history. Otherwise the screen suffix.
pub fn assistant_parts(before: &PaneSnapshot, after: &PaneSnapshot) -> Vec<String> {
    if !before.history.is_empty()
        && after.history.len() > before.history.len()
        && after.history.starts_with(&before.history[..])
    {
        return nonempty(&after.history[before.history.len()..]);
    }
    if before.revision == after.revision && before.screen == after.screen {
        return Vec::new();
    }
    let text = if before.screen.is_empty() {
        after.screen.trim().to_owned()
    } else {
        screen_diff(&before.screen, &after.screen).trim().to_owned()
    };
    if text.is_empty() {
        Vec::new()
    } else {
        vec![text]
    }
}

fn nonempty(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

async fn read_pane(control: &dyn AgentControl, session_id: &str) -> Result<PaneSnapshot> {
    let value = control
        .execute(
            None,
            AgentOp::Read {
                session_id: session_id.to_owned(),
            },
        )
        .await?;
    Ok(snapshot_of(&value))
}

fn snapshot_of(value: &Value) -> PaneSnapshot {
    let mut snapshot = snapshot_from_read(value);
    if snapshot.revision.is_empty() {
        if let Some(revision) = value
            .pointer("/session/screenRevision")
            .and_then(Value::as_str)
        {
            snapshot.revision = revision.to_owned();
        }
    }
    snapshot
}

async fn event_cursor(control: &dyn AgentControl) -> u64 {
    control
        .execute(None, AgentOp::Events { after: 0 })
        .await
        .ok()
        .and_then(|value| value.get("afterSequence").and_then(Value::as_u64))
        .unwrap_or(0)
}

async fn ensure_session(pool: &SqlitePool, key: &SessionKey) -> Result<()> {
    sqlx::query(
        "INSERT INTO sessions (session_key, platform, guild_id, channel_id, thread_id, user_id, state_json)
         VALUES (?, ?, ?, ?, ?, ?, '{}') ON CONFLICT(session_key) DO NOTHING",
    )
    .bind(key.storage_key())
    .bind(&key.platform)
    .bind(&key.guild_id)
    .bind(&key.channel_id)
    .bind(&key.thread_id)
    .bind(&key.user_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_message(
    pool: &SqlitePool,
    session: &SessionKey,
    id: &str,
    role: &str,
    content: &str,
) -> Result<()> {
    let mut tx = pool.begin_write().await?;
    let sequence: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO messages (id, session_key, role, content, sequence, created_at) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(session.storage_key())
    .bind(role)
    .bind(content)
    .bind(sequence)
    .bind(now_rfc3339())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::agent_control::AgentOp;

    #[test]
    fn a_changed_screen_suffix_is_the_assistant_reply() {
        let before = PaneSnapshot {
            revision: "r1".into(),
            screen: "hello".into(),
            history: vec![],
        };
        let after = PaneSnapshot {
            revision: "r2".into(),
            screen: "hello\nSpoken reply".into(),
            history: vec![],
        };
        assert_eq!(
            assistant_parts(&before, &after),
            vec!["Spoken reply".to_owned()]
        );
        let same = PaneSnapshot {
            revision: "r1".into(),
            screen: "hello".into(),
            history: vec![],
        };
        assert!(assistant_parts(&before, &same).is_empty());
    }

    #[tokio::test]
    async fn assistant_rows_after_the_voice_turn_are_joined_in_order() {
        let pool = crate::storage::memory().await.unwrap();
        let session = discord_text_session(7, "42");
        ensure_session(&pool, &session).await.unwrap();
        let prior = Uuid::new_v4();
        let user = Uuid::new_v4();
        for (id, role, content) in [(prior, "assistant", "before"), (user, "user", "heard")] {
            insert_message(&pool, &session, &id.to_string(), role, content)
                .await
                .unwrap();
        }
        for content in ["first", "second"] {
            insert_message(
                &pool,
                &session,
                &Uuid::new_v4().to_string(),
                "assistant",
                content,
            )
            .await
            .unwrap();
        }
        let reply = assistant_reply_after(&pool, &session, user).await.unwrap();
        assert_eq!(reply, "first\n\nsecond");
    }

    #[tokio::test]
    async fn an_unbound_voice_channel_is_refused() {
        let pool = crate::storage::memory().await.unwrap();
        let ctx = VoiceTurnContext {
            pool,
            profiles: ProfileRouter::new(vec![]),
            meta_session_id: "meta".into(),
            control: Arc::new(ScriptedControl::default()),
        };
        let error = respond_with_speaker(&ctx, u64::MAX, None, "[voice] hi")
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            format!("no text session is bound to voice channel {}", u64::MAX)
        );
    }

    #[tokio::test]
    async fn a_voice_turn_is_sent_to_the_text_pane_and_speaks_its_reply() {
        let pool = crate::storage::memory().await.unwrap();
        let control = Arc::new(ScriptedControl::default());
        let profiles =
            ProfileRouter::from_json(r#"[{"channel":42,"pane_session_id":"lane-pane"}]"#);
        let ctx = VoiceTurnContext {
            pool: pool.clone(),
            profiles,
            meta_session_id: "meta".into(),
            control: control.clone(),
        };
        let voice_channel = 900_000_000 + u64::from(std::process::id());
        let session = discord_text_session(7, "42");
        insert_message(
            &pool,
            &session,
            &Uuid::new_v4().to_string(),
            "assistant",
            "before",
        )
        .await
        .unwrap();
        bind(voice_channel, session);
        let reply = respond_with_speaker(&ctx, voice_channel, Some(99), "[voice] hi")
            .await
            .unwrap();
        assert_eq!(reply, "first\n\nsecond");
        assert!(!reply.contains("before"));
        let ops = control.ops.lock().expect("ops").clone();
        let send = ops.iter().find_map(|op| match op {
            AgentOp::Send {
                session_id,
                text,
                request_id,
            } => Some((session_id.clone(), text.clone(), request_id.clone())),
            _ => None,
        });
        let (session_id, text, request_id) = send.expect("a send");
        assert_eq!(session_id, "lane-pane");
        assert!(text.contains("[voice] hi"));
        assert!(text.contains("user=99"));
        assert!(request_id.contains("voice:"));
        assert!(ops.iter().any(|op| matches!(
            op,
            AgentOp::Wait {
                until,
                after,
                after_revision,
                ..
            } if until == "idle" && *after == 5 && after_revision.as_deref() == Some("rev-1")
        )));
        assert!(ops.iter().all(|op| !matches!(
            op,
            AgentOp::Send { session_id, .. } if session_id == "meta"
        )));
    }

    #[derive(Default)]
    struct ScriptedControl {
        ops: StdMutex<Vec<AgentOp>>,
        reads: AtomicUsize,
    }

    #[async_trait]
    impl AgentControl for ScriptedControl {
        async fn execute(&self, _host_id: Option<&str>, op: AgentOp) -> Result<Value> {
            let read = matches!(op, AgentOp::Read { .. });
            self.ops.lock().expect("ops").push(op);
            if read {
                let nth = self.reads.fetch_add(1, Ordering::SeqCst);
                if nth == 0 {
                    return Ok(json!({
                        "session": {
                            "screenRevision": "rev-1",
                            "screen": { "text": "earlier line", "revision": "rev-1" },
                            "history": ["earlier"]
                        }
                    }));
                }
                return Ok(json!({
                    "session": {
                        "screenRevision": "rev-2",
                        "screen": { "text": "earlier line\nfirst\nsecond", "revision": "rev-2" },
                        "history": ["earlier", "first", "second"]
                    }
                }));
            }
            Ok(json!({ "ok": true, "afterSequence": 5 }))
        }
    }
}
