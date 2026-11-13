//! The agent sessions engine: the agent panes of this machine (`local`) and of every paired
//! host the fleet has listed, read and driven through `AgentControl`. `shape` is the board
//! JSON omo-gateway served; send / interrupt / answer act on a pane. `/api/agent/sessions`
//! serves both.
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};

pub const PREVIEW_CHARS: usize = 400;
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

static CONTROL: OnceLock<Arc<dyn AgentControl>> = OnceLock::new();

/// Gives the sessions engine the hub's agent control. Without it the board is empty, as omo's
/// was with no hosts configured.
pub fn install(control: Arc<dyn AgentControl>) {
    let _ = CONTROL.set(control);
}

/// One live agent pane, read from an `agents.list` entry.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct LiveSession {
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub state: &'static str,
    pub streaming: bool,
    pub question: Option<Value>,
    pub last_assistant: String,
}

impl LiveSession {
    pub fn from_entry(entry: &Value) -> Option<Self> {
        let session_id = entry.get("sessionId")?.as_str()?.to_owned();
        let pending = entry
            .get("pending")
            .filter(|value| !value.is_null())
            .cloned();
        let raw = entry
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let state = if pending.is_some() {
            "question"
        } else {
            match raw {
                "working" => "working",
                "blocked" => "blocked",
                _ => "idle",
            }
        };
        Some(Self {
            session_id,
            name: entry
                .get("agent")
                .and_then(Value::as_str)
                .map(str::to_owned),
            cwd: entry.get("cwd").and_then(Value::as_str).map(str::to_owned),
            state,
            streaming: raw == "working",
            question: pending,
            last_assistant: entry
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
    }
}

/// A host's answer: its sessions, or why it could not be listed.
pub type Snapshot = (String, std::result::Result<Vec<LiveSession>, String>);

fn last_snapshots() -> &'static RwLock<Option<Vec<Snapshot>>> {
    static LAST: OnceLock<RwLock<Option<Vec<Snapshot>>>> = OnceLock::new();
    LAST.get_or_init(|| RwLock::new(None))
}

/// The last collected snapshots, or a fresh `collect` on `control` when `refresh` is set or
/// nothing was collected yet.
pub async fn snapshots_on(
    control: &dyn AgentControl,
    pool: &SqlitePool,
    refresh: bool,
) -> Vec<Snapshot> {
    if !refresh {
        let cached = last_snapshots()
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        if let Some(cached) = cached {
            return cached;
        }
    }
    let snapshots = collect(pool, control).await;
    remember(&snapshots);
    snapshots
}

/// The agent control [`install`] gave the hub, if any.
pub fn installed() -> Option<Arc<dyn AgentControl>> {
    CONTROL.get().cloned()
}

fn remember(snapshots: &[Snapshot]) {
    *last_snapshots()
        .write()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(snapshots.to_vec());
}

/// `local` plus every paired host the fleet has listed.
async fn hosts(pool: &SqlitePool) -> Vec<String> {
    let mut hosts = vec!["local".to_owned()];
    let known: Vec<String> =
        sqlx::query_scalar("SELECT host_id FROM fleet_inventory_cache ORDER BY host_id")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    for host in known {
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
}

pub async fn collect(pool: &SqlitePool, control: &dyn AgentControl) -> Vec<Snapshot> {
    let hosts = hosts(pool).await;
    let entries = crate::fleet::list_persisted(control, pool, &hosts, LIST_TIMEOUT).await;
    entries
        .into_iter()
        .map(|entry| {
            let snapshot = match entry.error {
                // A stale (hung) host is not an answer: it reports why it was not listed.
                Some(error) => Err(error),
                None => Ok(entry
                    .sessions
                    .iter()
                    .filter_map(LiveSession::from_entry)
                    .collect()),
            };
            (entry.host_id, snapshot)
        })
        .collect()
}

fn state_rank(state: &str) -> u8 {
    match state {
        "question" => 0,
        "blocked" => 1,
        "working" => 2,
        _ => 3,
    }
}

fn preview(text: &str) -> String {
    text.chars().take(PREVIEW_CHARS).collect()
}

/// The board JSON, as omo's `shape`.
pub fn shape(snapshots: &[Snapshot], now: DateTime<Utc>) -> Value {
    let mut machines = Vec::new();
    let mut rows = Vec::new();
    for (machine, snapshot) in snapshots {
        match snapshot {
            Err(error) => machines.push(json!({
                "machine": machine, "ok": false, "error": error, "session_count": 0,
            })),
            Ok(sessions) => {
                machines.push(json!({
                    "machine": machine, "ok": true, "error": null, "session_count": sessions.len(),
                }));
                for live in sessions {
                    rows.push((
                        state_rank(live.state),
                        json!({
                            "machine": machine,
                            "session_id": live.session_id,
                            "name": live.name,
                            "cwd": live.cwd,
                            "state": live.state,
                            "streaming": live.streaming,
                            "compacting": false,
                            "message_count": 0,
                            "pending_messages": 0,
                            "last_user": "",
                            "last_assistant": preview(&live.last_assistant),
                            "questions": live.question.iter().cloned().collect::<Vec<_>>(),
                            "updated_at": null,
                        }),
                    ));
                }
            }
        }
    }
    rows.sort_by_key(|(rank, _)| *rank);
    let count = |name: &str| rows.iter().filter(|(_, row)| row["state"] == name).count();
    json!({
        "collected_at": now.to_rfc3339(),
        "machines": machines,
        "counts": {
            "working": count("working"),
            "question": count("question"),
            "blocked": count("blocked"),
            "idle": count("idle"),
            "total": rows.len(),
        },
        "sessions": rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>(),
    })
}

/// Which session a board action targets; a host the hub does not know is refused.
async fn target(pool: &SqlitePool, machine: &str) -> Result<Option<String>> {
    if machine == "local" {
        return Ok(None);
    }
    if hosts(pool).await.iter().any(|host| host == machine) {
        Ok(Some(machine.to_owned()))
    } else {
        Err(HubError::message(format!(
            "machine {machine} was not found"
        )))
    }
}

pub(crate) async fn send_on(
    control: &dyn AgentControl,
    pool: &SqlitePool,
    machine: &str,
    session_id: &str,
    text: &str,
) -> Result<Value> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HubError::message("text is required"));
    }
    let host = target(pool, machine).await?;
    control
        .execute(
            host.as_deref(),
            AgentOp::Send {
                session_id: session_id.to_owned(),
                text: text.to_owned(),
                request_id: Uuid::new_v4().to_string(),
            },
        )
        .await
}

pub(crate) async fn interrupt_on(
    control: &dyn AgentControl,
    pool: &SqlitePool,
    machine: &str,
    session_id: &str,
) -> Result<Value> {
    let host = target(pool, machine).await?;
    control
        .execute(
            host.as_deref(),
            AgentOp::Stop {
                session_id: session_id.to_owned(),
                request_id: Uuid::new_v4().to_string(),
            },
        )
        .await
}

/// Answers the pane's pending question: a number picks that option (1-based), anything else
/// is typed as the answer, as omo's `answer` took free text.
pub(crate) async fn answer_on(
    control: &dyn AgentControl,
    pool: &SqlitePool,
    machine: &str,
    session_id: &str,
    text: &str,
    request: Option<&str>,
) -> Result<Value> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HubError::message("text is required"));
    }
    let host = target(pool, machine).await?;
    let read = control
        .execute(
            host.as_deref(),
            AgentOp::Read {
                session_id: session_id.to_owned(),
            },
        )
        .await?;
    let session = read.get("session").unwrap_or(&read);
    let pending = session
        .get("pending")
        .filter(|value| !value.is_null())
        .ok_or_else(|| HubError::message("the session has no pending question"))?;
    let prompt_id = request
        .map(str::to_owned)
        .or_else(|| pending.get("id").and_then(Value::as_str).map(str::to_owned))
        .ok_or_else(|| HubError::message("the pending question has no id"))?;
    let revision = session
        .get("screenRevision")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let answer = match text.parse::<u64>() {
        Ok(number) if number >= 1 => json!({"optionIndex": number - 1}),
        _ => json!({"customText": text}),
    };
    control
        .execute(
            host.as_deref(),
            AgentOp::Answer {
                session_id: session_id.to_owned(),
                prompt_id,
                screen_revision: revision,
                answer,
                request_id: Uuid::new_v4().to_string(),
            },
        )
        .await
}

#[cfg(test)]
mod tests;
