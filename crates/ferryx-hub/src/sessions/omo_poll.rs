//! omo sessions on every machine, polled the way omo-gateway's session watcher did.
//!
//! The daemon's agent observer only sees Ferryx panes on this machine. omo sessions also run in
//! plain terminals and on other hosts (`OMON_SESSION_HOSTS`), and omo reports a pending
//! `ask_user_question` over its RPC socket, not on the screen. Every interval the hub reads
//! every session on this machine and on every configured host ([`super::omo_probe`], the
//! native port of `omo-gateway sessions overview`) and feeds each one to [`Hub::ingest`], so a
//! question, a terminal-only question, or a watched session's finished turn becomes the same
//! owner notice a Ferryx pane produces.
//!
//! `FERRYX_HUB_OMO_SESSIONS=off` turns it off; `FERRYX_HUB_OMO_POLL_SECS` sets the interval
//! (default 60, minimum 10).
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::SqlitePool;

use super::omo_probe::{self, MachineSnapshot};
use super::watch::{compact, Observation};
use crate::error::{HubError, Result};
use crate::hub::Hub;

pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(60);
const MIN_INTERVAL: Duration = Duration::from_secs(10);
/// Every host is read concurrently with its own bound; this caps the whole poll.
const CALL_TIMEOUT: Duration = Duration::from_secs(45);

pub fn enabled() -> bool {
    !matches!(
        crate::hub_env::var("FERRYX_HUB_OMO_SESSIONS")
            .ok()
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("0" | "off" | "false" | "no")
    )
}

pub fn interval() -> Duration {
    crate::hub_env::var("FERRYX_HUB_OMO_POLL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .filter(|interval| *interval >= MIN_INTERVAL)
        .unwrap_or(DEFAULT_INTERVAL)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachinePoll {
    pub machine: String,
    pub sessions: std::result::Result<Vec<Value>, String>,
}

/// The `snapshots` of an overview: `[[machine, {"Ok": {"sessions": [...]}} | {"Err": ...}], ...]`.
pub fn parse_overview(overview: &Value) -> Result<Vec<MachinePoll>> {
    let snapshots = overview
        .get("snapshots")
        .and_then(Value::as_array)
        .ok_or_else(|| HubError::message("omo sessions overview has no snapshots"))?;
    Ok(snapshots
        .iter()
        .filter_map(|entry| {
            let machine = entry.get(0)?.as_str()?.to_owned();
            let result = entry.get(1)?;
            let sessions = match result.get("Ok") {
                Some(snapshot) => Ok(snapshot
                    .get("sessions")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()),
                None => Err(result
                    .get("Err")
                    .map(|error| compact(&error.to_string(), 200))
                    .unwrap_or_else(|| "unknown snapshot error".into())),
            };
            Some(MachinePoll { machine, sessions })
        })
        .collect())
}

fn pending_question(session: &Value) -> Option<String> {
    let pending = session.get("questions")?.as_array()?.first()?;
    let item = pending.get("questions")?.as_array()?.first();
    let text = item
        .and_then(|item| {
            item.get("question")
                .and_then(Value::as_str)
                .or_else(|| item.get("header").and_then(Value::as_str))
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("question");
    let options: Vec<&str> = item
        .and_then(|item| item.get("options"))
        .and_then(Value::as_array)
        .map(|options| options.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(if options.is_empty() {
        text.to_owned()
    } else {
        format!("{text} [{}]", options.join(" / "))
    })
}

/// One omo session as a hub observation. `target` is `<machine>:<omo session id>`, the form the
/// watch list (`session_watches`) has always used.
pub fn observation(
    machine: &str,
    session: &Value,
    watched: &HashSet<String>,
) -> Option<Observation> {
    let session_id = session.get("session_id")?.as_str()?.to_owned();
    let target = format!("{machine}:{session_id}");
    let name = session
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(|name| compact(name, 40))
        .unwrap_or_else(|| super::watch::short_id(&session_id).to_owned());
    let question = pending_question(session);
    let flag = |key: &str| session.get(key).and_then(Value::as_bool).unwrap_or(false);
    Some(Observation {
        watched: watched.contains(&target),
        target,
        machine: machine.to_owned(),
        session_id,
        name,
        message_count: session
            .get("message_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        streaming: flag("streaming") || flag("compacting"),
        blocking_question: flag("blocking_question") && question.is_none(),
        question,
        last_assistant: session
            .get("last_assistant")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        machine_failed: false,
        failed: false,
        approval: None,
    })
}

async fn watched_targets(pool: &SqlitePool) -> Result<HashSet<String>> {
    let targets: Vec<String> = sqlx::query_scalar("SELECT target FROM session_watches")
        .fetch_all(pool)
        .await?;
    Ok(targets.into_iter().collect())
}

/// Apply one overview to the hub. A machine whose snapshot failed is skipped: its held notices
/// stay held rather than being withdrawn by a read that said nothing. Returns the number of
/// sessions observed.
pub async fn apply(hub: &Arc<Hub>, overview: &Value) -> Result<usize> {
    let watched = watched_targets(&hub.pool).await?;
    let mut observed = 0;
    for poll in parse_overview(overview)? {
        match poll.sessions {
            Ok(sessions) => {
                for session in &sessions {
                    if let Some(observation) = observation(&poll.machine, session, &watched) {
                        hub.ingest(&observation).await?;
                        observed += 1;
                    }
                }
            }
            Err(error) => {
                tracing::debug!(machine = %poll.machine, %error, "omo sessions snapshot failed; its notices stay held");
            }
        }
    }
    Ok(observed)
}

/// One overview in the shape `omo-gateway sessions overview --json` printed:
/// `{"snapshots": [[machine, {"Ok": snapshot} | {"Err": message}], ...]}`.
pub fn overview_value(
    snapshots: &[(String, std::result::Result<MachineSnapshot, String>)],
) -> Result<Value> {
    let snapshots = serde_json::to_value(snapshots)
        .map_err(|error| HubError::message(format!("omo sessions overview: {error}")))?;
    Ok(serde_json::json!({ "snapshots": snapshots }))
}

async fn overview() -> Result<Value> {
    let hosts = omo_probe::configured_hosts().await;
    let rpc_dir = omo_probe::default_rpc_dir();
    let snapshots = tokio::time::timeout(
        CALL_TIMEOUT,
        omo_probe::snapshot_all(rpc_dir.as_deref(), &hosts),
    )
    .await
    .map_err(|_| HubError::message("omo sessions overview timed out"))?;
    overview_value(&snapshots)
}

/// Poll forever. Returns at once when the poller is off.
pub async fn run(hub: Arc<Hub>) {
    if !enabled() {
        tracing::info!("omo session poller is off (FERRYX_HUB_OMO_SESSIONS)");
        return;
    }
    let every = interval();
    tracing::info!(interval_secs = every.as_secs(), "omo session poller started");
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut failing = false;
    loop {
        ticker.tick().await;
        let result = match overview().await {
            Ok(value) => apply(&hub, &value).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(observed) => {
                if failing {
                    tracing::info!(observed, "omo session poll recovered");
                }
                failing = false;
                tracing::debug!(observed, "omo session poll");
            }
            Err(error) => {
                // One warning per failure streak; a down host is not a log flood.
                if !failing {
                    tracing::warn!(%error, "omo session poll failed; retrying every interval");
                }
                failing = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use crate::config::HubConfig;
    use serde_json::json;

    fn session(
        id: &str,
        streaming: bool,
        blocking: bool,
        question: Option<&str>,
        count: u64,
    ) -> Value {
        let questions = match question {
            Some(text) => {
                json!([{"request_id": "call_1", "questions": [{"id": "q1", "header": "Pick", "question": text, "options": ["Ship", "Hold"]}]}])
            }
            None => json!([]),
        };
        json!({
            "machine": "local", "session_id": id, "name": format!("job {id}"),
            "streaming": streaming, "compacting": false, "blocking_question": blocking,
            "message_count": count, "last_assistant": "patched the parser", "questions": questions,
        })
    }

    fn overview(local: Vec<Value>, omarchy: std::result::Result<Vec<Value>, &str>) -> Value {
        let remote = match omarchy {
            Ok(sessions) => json!({"Ok": {"machine": "omarchy", "sessions": sessions}}),
            Err(error) => json!({"Err": error}),
        };
        json!({"list": {}, "snapshots": [["local", {"Ok": {"machine": "local", "sessions": local}}], ["omarchy", remote]]})
    }

    async fn hub(pool: SqlitePool) -> Arc<Hub> {
        let config = HubConfig {
            owner_slack_id: Some("U0OWNER".into()),
            ..HubConfig::default()
        };
        Hub::with_policy(
            pool,
            Arc::new(RecordingControl::default()),
            config,
            crate::sessions::watch::NotifyPolicy {
                notify_after: Duration::ZERO,
                quiet_hours: None,
                mode: crate::sessions::watch::NotifyMode::Actionable,
            },
        )
    }

    async fn notices(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT dedupe_key, kind FROM session_watch_notices ORDER BY dedupe_key")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn questions_blocks_and_watched_completions_become_notices_once() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query("INSERT INTO session_watches(target, added_at) VALUES('omarchy:w-1','2026-10-24T00:00:00Z')")
            .execute(&pool)
            .await
            .unwrap();
        let hub = hub(pool.clone()).await;

        let first = overview(
            vec![
                session("q-1", false, false, Some("Deploy now?"), 10),
                session("b-1", false, true, None, 20),
                session("idle-1", false, false, None, 5),
            ],
            Ok(vec![session("w-1", true, false, None, 30)]),
        );
        assert_eq!(apply(&hub, &first).await.unwrap(), 4);
        let kinds: Vec<String> = notices(&pool).await.into_iter().map(|row| row.1).collect();
        assert_eq!(
            kinds,
            vec!["blocked", "question"],
            "first poll: no completion baseline yet"
        );

        let second = overview(
            vec![
                session("q-1", false, false, Some("Deploy now?"), 10),
                session("b-1", false, true, None, 20),
                session("idle-1", false, false, None, 5),
            ],
            Ok(vec![session("w-1", false, false, None, 31)]),
        );
        apply(&hub, &second).await.unwrap();
        let stored = notices(&pool).await;
        assert_eq!(
            stored.len(),
            3,
            "the same question and block are not sent again: {stored:?}"
        );
        assert!(stored
            .iter()
            .any(|(key, kind)| kind == "completion" && key.contains("omarchy:w-1")));
        assert!(stored
            .iter()
            .any(|(key, kind)| kind == "question" && key.contains("local:q-1")));

        let outbox = crate::chat::reply::pending(&pool).await.unwrap();
        assert_eq!(
            outbox.len(),
            2,
            "question and block are posted, the completion is ledger-only: {outbox:?}"
        );
        assert!(outbox
            .iter()
            .any(|row| row.2.contains("Deploy now? [Ship / Hold]")));
    }

    #[tokio::test]
    async fn a_failed_host_neither_errors_nor_hides_the_other_machine() {
        let pool = crate::storage::memory().await.unwrap();
        let hub = hub(pool.clone()).await;
        let polled = overview(
            vec![session("q-2", false, false, Some("Merge?"), 3)],
            Err("ssh: connect timed out"),
        );
        assert_eq!(apply(&hub, &polled).await.unwrap(), 1);
        let stored = notices(&pool).await;
        assert_eq!(stored.len(), 1, "{stored:?}");
        assert_eq!(stored[0].1, "question");
    }

    #[test]
    fn native_snapshots_have_the_shape_parse_overview_reads() {
        let local = MachineSnapshot {
            sessions: vec![omo_probe::LiveSession {
                session_id: "s-1".into(),
                routing_id: "s-1".into(),
                endpoint_kind: "tui".into(),
                name: Some("Release".into()),
                blocking_question: true,
                message_count: 4,
                questions: vec![omo_probe::PendingQuestion {
                    request_id: "call_1".into(),
                    questions: vec![omo_probe::QuestionItem {
                        id: "q1".into(),
                        header: None,
                        question: Some("Ship?".into()),
                        options: vec!["Yes".into(), "No".into()],
                    }],
                }],
                ..Default::default()
            }],
            errors: vec![],
        };
        let value = overview_value(&[
            ("local".into(), Ok(local)),
            ("omarchy".into(), Err("probe timed out".into())),
        ])
        .unwrap();
        let polls = parse_overview(&value).unwrap();
        assert_eq!(polls.len(), 2);
        assert_eq!(polls[1].machine, "omarchy");
        assert_eq!(polls[1].sessions, Err("\"probe timed out\"".into()));
        let sessions = polls[0].sessions.as_ref().unwrap();
        let observed = observation("local", &sessions[0], &HashSet::new()).unwrap();
        assert_eq!(observed.target, "local:s-1");
        assert_eq!(observed.name, "Release");
        assert_eq!(observed.message_count, 4);
        assert_eq!(observed.question.as_deref(), Some("Ship? [Yes / No]"));
        assert!(!observed.blocking_question, "a question outranks the block flag");
    }

    #[test]
    fn malformed_overview_is_an_error_and_unnamed_sessions_get_a_short_id() {
        assert!(parse_overview(&json!({"list": {}})).is_err());
        let unnamed = json!({"session_id": "01a16600-dbb3-7ba3-9c2e-ae2418179155", "name": null});
        let observed = observation("local", &unnamed, &HashSet::new()).unwrap();
        assert_eq!(observed.name, "01a16600-dbb3");
        assert_eq!(
            observed.target,
            "local:01a16600-dbb3-7ba3-9c2e-ae2418179155"
        );
        assert!(observed.question.is_none());
    }
}
