//! One always-on hub. The daemon opens it after it is ready. Unit tests that build a
//! daemon server do not open it.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use sqlx::SqlitePool;

use crate::agent_control::AgentControl;
use crate::chat::session::SessionKey;
use crate::config::HubConfig;
use crate::error::Result;
use crate::import::import_once;
use crate::sessions::watch::ObserveOutcome;
use crate::sessions::watch::{owner_post, NotifyPolicy, Observation, Watcher};

pub struct Hub {
    pub pool: SqlitePool,
    pub control: Arc<dyn AgentControl>,
    pub watcher: Arc<Watcher>,
    pub config: HubConfig,
    /// Latest observation per target, re-read when a held notice comes due.
    latest: std::sync::Mutex<HashMap<String, Observation>>,
    /// Earliest armed wake per target. One timer per target, never a poll.
    wakes: std::sync::Mutex<HashMap<String, DateTime<Utc>>>,
}

impl Hub {
    pub async fn open(mut config: HubConfig, control: Arc<dyn AgentControl>) -> Result<Arc<Self>> {
        let path = config
            .database_path
            .clone()
            .unwrap_or_else(default_database_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pool = crate::storage::connect(&path).await?;
        if config.import_invoked {
            if let Some(source) = &config.import_from {
                import_once(&pool, source).await?;
            }
        }
        // The file and SLACK_HOME_CHANNEL already won in HubConfig::load. The import
        // fills the channel only when neither of those set one.
        if crate::config::normalize_slack_home(config.slack_home_channel.as_deref().unwrap_or(""))
            .is_none()
        {
            if let Some(channel) = crate::import::imported_slack_home(&pool).await? {
                config.slack_home_channel = Some(channel);
            }
        } else if let Some(channel) =
            crate::config::normalize_slack_home(config.slack_home_channel.as_deref().unwrap_or(""))
        {
            config.slack_home_channel = Some(channel);
        }
        let mut policy = NotifyPolicy::from_env();
        policy.mode = crate::sessions::watch::effective_notify_mode(&pool).await?;
        Ok(Self::with_policy(pool, control, config, policy))
    }

    pub fn with_policy(
        pool: SqlitePool,
        control: Arc<dyn AgentControl>,
        config: HubConfig,
        policy: NotifyPolicy,
    ) -> Arc<Self> {
        let watcher = Arc::new(Watcher::new(pool.clone(), policy));
        Arc::new(Self {
            pool,
            control,
            watcher,
            config,
            latest: std::sync::Mutex::new(HashMap::new()),
            wakes: std::sync::Mutex::new(HashMap::new()),
        })
    }

    /// Where owner notices, approval cards, and auto-answer escalations go.
    /// A configured Slack home channel receives them. With no home channel they go to
    /// the owner DM (the owner id as the channel, which is how Slack accepts a DM post).
    pub fn owner_session(&self) -> SessionKey {
        let owner = self
            .config
            .owner_slack_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| "owner".into());
        let channel = self
            .config
            .slack_home_channel
            .as_deref()
            .and_then(crate::config::normalize_slack_home)
            .unwrap_or_else(|| owner.clone());
        SessionKey::new("slack", None::<String>, channel, None::<String>, owner)
    }

    /// Apply one observer event. A notice that is not yet due arms a timer for its release
    /// time; when it fires, the latest observation of that target is applied again, so a
    /// question that is still on screen is sent and one that went away is not.
    pub async fn ingest(self: &Arc<Self>, observation: &Observation) -> Result<ObserveOutcome> {
        self.latest
            .lock()
            .expect("hub latest lock")
            .insert(observation.target.clone(), observation.clone());
        let outcome = self.apply(observation, Utc::now()).await?;
        if let Some(at) = outcome.next_wake {
            self.arm(observation.target.clone(), at);
        }
        Ok(outcome)
    }

    async fn apply(&self, observation: &Observation, now: DateTime<Utc>) -> Result<ObserveOutcome> {
        let outcome = self.watcher.on_observation(observation, now).await?;
        for notice in &outcome.due {
            if self.watcher.deliver(notice, now).await? {
                let level = if matches!(
                    notice.kind.as_str(),
                    "failure" | "error" | "crash" | "failed" | "remediation"
                ) {
                    "error"
                } else {
                    "info"
                };
                if let Err(error) = crate::http::record_log(&self.pool, level, &notice.text).await {
                    tracing::warn!(%error, key = %notice.dedupe_key, "notice was not written to the activity log");
                }
                if owner_post(self.watcher.mode(), &notice.kind) {
                    let session = self.owner_session();
                    if let Err(error) = crate::chat::reply::chat_reply_card(
                        &self.pool,
                        &session,
                        &notice.text,
                        notice.approval.as_ref(),
                    )
                    .await
                    {
                        tracing::warn!(%error, key = %notice.dedupe_key, "owner notice is stored but was not posted");
                    }
                }
                if matches!(notice.kind.as_str(), "completion" | "failure") {
                    self.reply_to_origin(observation, &notice.text).await;
                }
            }
        }
        Ok(outcome)
    }

    /// Final result of a task the owner started from Slack or Discord, in that thread.
    /// Actionable mode does this instead of posting the home-channel turn-complete.
    async fn reply_to_origin(&self, observation: &Observation, notice_text: &str) {
        let origin = match crate::chat::router::take_chat_origin(
            &self.pool,
            &observation.session_id,
        )
        .await
        {
            Ok(origin) => origin,
            Err(error) => {
                tracing::warn!(%error, session = %observation.session_id, "chat origin was not read");
                return;
            }
        };
        let Some(origin) = origin else {
            return;
        };
        let text = if observation.last_assistant.trim().is_empty() {
            notice_text
        } else {
            observation.last_assistant.as_str()
        };
        if let Err(error) = crate::chat::reply::chat_reply(&self.pool, &origin, text, None).await {
            tracing::warn!(%error, session = %observation.session_id, "origin result was not posted");
        }
    }

    fn arm(self: &Arc<Self>, target: String, at: DateTime<Utc>) {
        {
            let mut wakes = self.wakes.lock().expect("hub wakes lock");
            if wakes.get(&target).is_some_and(|armed| *armed <= at) {
                return;
            }
            wakes.insert(target.clone(), at);
        }
        let hub = Arc::clone(self);
        tokio::spawn(async move {
            let mut at = at;
            loop {
                let wait = (at - Utc::now()).to_std().unwrap_or_default();
                tokio::time::sleep(wait).await;
                {
                    let mut wakes = hub.wakes.lock().expect("hub wakes lock");
                    match wakes.get(&target) {
                        // An earlier timer replaced this one; that timer owns the target now.
                        Some(armed) if *armed < at => return,
                        _ => {
                            wakes.remove(&target);
                        }
                    }
                }
                let latest = hub
                    .latest
                    .lock()
                    .expect("hub latest lock")
                    .get(&target)
                    .cloned();
                let Some(observation) = latest else { return };
                let next = match hub.apply(&observation, Utc::now()).await {
                    Ok(outcome) => outcome.next_wake,
                    Err(error) => {
                        tracing::warn!(%error, %target, "held notice was not applied");
                        return;
                    }
                };
                let Some(next) = next else { return };
                {
                    let mut wakes = hub.wakes.lock().expect("hub wakes lock");
                    if wakes.get(&target).is_some_and(|armed| *armed <= next) {
                        return;
                    }
                    wakes.insert(target.clone(), next);
                }
                at = next;
            }
        });
    }
}

pub fn default_database_path() -> PathBuf {
    if let Ok(path) = crate::hub_env::var("FERRYX_HUB_DB") {
        return PathBuf::from(path);
    }
    let home = crate::hub_env::var_os("HOME").or_else(|| crate::hub_env::var_os("USERPROFILE"));
    match home {
        Some(home) => PathBuf::from(home)
            .join(".ferryx")
            .join("hub")
            .join("hub.sqlite"),
        None => PathBuf::from("ferryx-hub.sqlite"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use crate::sessions::watch::NotifyMode;

    fn hub(pool: SqlitePool, notify_after: std::time::Duration) -> Arc<Hub> {
        hub_with(pool, notify_after, HubConfig::default())
    }

    fn hub_with(
        pool: SqlitePool,
        notify_after: std::time::Duration,
        config: HubConfig,
    ) -> Arc<Hub> {
        hub_mode(pool, notify_after, config, NotifyMode::Actionable)
    }

    fn hub_mode(
        pool: SqlitePool,
        notify_after: std::time::Duration,
        config: HubConfig,
        mode: NotifyMode,
    ) -> Arc<Hub> {
        Arc::new(Hub {
            pool: pool.clone(),
            control: Arc::new(RecordingControl::default()),
            watcher: Arc::new(Watcher::new(
                pool,
                NotifyPolicy {
                    notify_after,
                    quiet_hours: None,
                    mode,
                },
            )),
            config,
            latest: std::sync::Mutex::new(HashMap::new()),
            wakes: std::sync::Mutex::new(HashMap::new()),
        })
    }

    fn blocked(question: Option<&str>) -> Observation {
        Observation {
            target: "local:pane".into(),
            machine: "local".into(),
            session_id: "pane".into(),
            name: "pane".into(),
            message_count: 7,
            streaming: false,
            blocking_question: question.is_none(),
            question: question.map(str::to_owned),
            last_assistant: String::new(),
            watched: true,
            machine_failed: false,
            failed: false,
            approval: None,
        }
    }

    fn turn(streaming: bool, failed: bool) -> Observation {
        Observation {
            target: "local:pane".into(),
            machine: "local".into(),
            session_id: "pane".into(),
            name: "pane".into(),
            message_count: 3,
            streaming,
            blocking_question: false,
            question: None,
            last_assistant: "shipped the patch".into(),
            watched: true,
            machine_failed: false,
            failed,
            approval: None,
        }
    }

    fn home_config() -> HubConfig {
        HubConfig {
            owner_slack_id: Some("UOWNER".into()),
            slack_home_channel: Some("CHOME".into()),
            ..HubConfig::default()
        }
    }

    async fn notices(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT dedupe_key, kind FROM session_watch_notices ORDER BY dedupe_key")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn owner_notices_use_the_home_channel_and_fall_back_to_the_owner_dm() {
        let pool = crate::storage::memory().await.unwrap();
        let dm = hub_with(
            pool.clone(),
            std::time::Duration::ZERO,
            HubConfig {
                owner_slack_id: Some("UOWNER".into()),
                ..HubConfig::default()
            },
        );
        let fallback = dm.owner_session();
        assert_eq!(fallback.channel_id, "UOWNER");
        assert_eq!(fallback.user_id, "UOWNER");
        let home = hub_with(
            pool,
            std::time::Duration::ZERO,
            HubConfig {
                owner_slack_id: Some("UOWNER".into()),
                slack_home_channel: Some(" #C0C9XG5FP8W , COTHER ".into()),
                ..HubConfig::default()
            },
        );
        let session = home.owner_session();
        assert_eq!(session.channel_id, "C0C9XG5FP8W");
        assert_eq!(session.user_id, "UOWNER");
    }

    #[tokio::test]
    async fn a_due_notice_is_queued_for_the_home_channel() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub_with(
            pool.clone(),
            std::time::Duration::ZERO,
            HubConfig {
                owner_slack_id: Some("UOWNER".into()),
                slack_home_channel: Some("CHOME".into()),
                ..HubConfig::default()
            },
        );
        hub.ingest(&blocked(Some("Proceed?"))).await.unwrap();
        let queued: Vec<(String,)> =
            sqlx::query_as("SELECT session_key FROM chat_outbox ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queued.len(), 1);
        let session = SessionKey::from_storage_key(&queued[0].0).unwrap();
        assert_eq!(session.channel_id, "CHOME");
        assert_ne!(session.channel_id, "UOWNER");
    }

    #[tokio::test]
    async fn a_held_notice_is_sent_by_its_timer_without_another_observation() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub(pool.clone(), std::time::Duration::from_millis(200));
        let outcome = hub.ingest(&blocked(Some("Proceed?"))).await.unwrap();
        assert!(outcome.due.is_empty(), "not due before the delay");
        assert!(outcome.next_wake.is_some());
        assert!(notices(&pool).await.is_empty());

        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let sent = notices(&pool).await;
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(sent[0].1, "question");
        let outbox = crate::chat::reply::pending(&pool).await.unwrap();
        assert_eq!(outbox.len(), 1, "the owner notice is queued for chat");
        assert!(outbox[0].2.contains("Proceed?"));
    }

    #[tokio::test]
    async fn one_prompt_with_a_card_is_a_single_notice() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub(pool.clone(), std::time::Duration::ZERO);
        let mut observation = blocked(Some("Proceed?"));
        observation.approval = Some(crate::sessions::watch::ApprovalCard {
            session_id: "pane".into(),
            prompt_id: "same-prompt".into(),
            screen_revision: "rev-1".into(),
            options: vec!["Yes".into(), "No".into()],
        });
        hub.ingest(&observation).await.unwrap();
        let outbox = crate::chat::reply::pending(&pool).await.unwrap();
        assert_eq!(outbox.len(), 1, "the card is the only notice: {outbox:?}");
        assert!(outbox[0].2.contains("Proceed?"));
        assert!(
            !outbox[0].2.contains("needs the owner"),
            "the plain escalation line is not also queued: {}",
            outbox[0].2
        );
    }

    #[tokio::test]
    async fn a_question_that_went_away_is_not_sent_when_the_timer_fires() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub(pool.clone(), std::time::Duration::from_millis(200));
        hub.ingest(&blocked(Some("Proceed?"))).await.unwrap();
        let mut answered = blocked(None);
        answered.blocking_question = false;
        hub.ingest(&answered).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        assert!(notices(&pool).await.is_empty());
    }

    async fn finish_turn(hub: &Arc<Hub>, failed: bool) {
        hub.ingest(&turn(true, false)).await.unwrap();
        hub.ingest(&turn(false, failed)).await.unwrap();
    }

    #[tokio::test]
    async fn a_turn_complete_is_logged_and_not_posted() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub_with(pool.clone(), std::time::Duration::ZERO, home_config());
        finish_turn(&hub, false).await;
        let outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_outbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(outbox, 0);
        let logged: String = sqlx::query_scalar("SELECT message FROM hub_log")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(logged.contains("turn complete"), "{logged}");
        assert!(logged.contains("shipped the patch"), "{logged}");
        let stored = notices(&pool).await;
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(stored[0].1, "completion");
    }

    #[tokio::test]
    async fn a_failed_turn_and_a_crashed_worker_are_posted() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub_with(pool.clone(), std::time::Duration::ZERO, home_config());
        finish_turn(&hub, true).await;
        let mut crashed = turn(false, false);
        crashed.target = "local:worker".into();
        crashed.session_id = "worker".into();
        crashed.machine_failed = true;
        hub.ingest(&crashed).await.unwrap();
        let outbox = crate::chat::reply::pending(&pool).await.unwrap();
        assert_eq!(outbox.len(), 2, "{outbox:?}");
        assert!(outbox.iter().any(|row| row.2.contains("turn failed")));
        assert!(outbox
            .iter()
            .any(|row| row.2.contains("worker unavailable")));
        for row in &outbox {
            let session = SessionKey::from_storage_key(&row.1).unwrap();
            assert_eq!(session.channel_id, "CHOME");
        }
    }

    #[tokio::test]
    async fn an_explicit_request_result_is_replied_to_its_origin() {
        let pool = crate::storage::memory().await.unwrap();
        let origin = SessionKey::new(
            "slack",
            None::<String>,
            "CORIGIN",
            Some("123.456".to_owned()),
            "UOWNER",
        );
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('chat_origin:pane', ?)")
            .bind(origin.storage_key())
            .execute(&pool)
            .await
            .unwrap();
        let hub = hub_with(pool.clone(), std::time::Duration::ZERO, home_config());
        finish_turn(&hub, false).await;
        let queued: Vec<(String, String)> =
            sqlx::query_as("SELECT session_key, text FROM chat_outbox ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queued.len(), 1, "{queued:?}");
        let session = SessionKey::from_storage_key(&queued[0].0).unwrap();
        assert_eq!(session.channel_id, "CORIGIN");
        assert_eq!(session.thread_id.as_deref(), Some("123.456"));
        assert_eq!(queued[0].1, "shipped the patch");
        let left: Option<String> =
            sqlx::query_scalar("SELECT value FROM hub_meta WHERE key = 'chat_origin:pane'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(left.is_none(), "the origin is consumed by the result");
    }

    #[tokio::test]
    async fn all_mode_posts_the_turn_complete_and_the_origin_result() {
        let pool = crate::storage::memory().await.unwrap();
        let origin = SessionKey::new("slack", None::<String>, "CORIGIN", None::<String>, "UOWNER");
        sqlx::query("INSERT INTO hub_meta(key, value) VALUES('chat_origin:pane', ?)")
            .bind(origin.storage_key())
            .execute(&pool)
            .await
            .unwrap();
        let hub = hub_mode(
            pool.clone(),
            std::time::Duration::ZERO,
            home_config(),
            NotifyMode::All,
        );
        finish_turn(&hub, false).await;
        let queued: Vec<(String, String)> =
            sqlx::query_as("SELECT session_key, text FROM chat_outbox ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queued.len(), 2, "{queued:?}");
        let home = SessionKey::from_storage_key(&queued[0].0).unwrap();
        assert_eq!(home.channel_id, "CHOME");
        assert!(queued[0].1.contains("turn complete"));
        let back = SessionKey::from_storage_key(&queued[1].0).unwrap();
        assert_eq!(back.channel_id, "CORIGIN");
        assert_eq!(queued[1].1, "shipped the patch");
    }
}
