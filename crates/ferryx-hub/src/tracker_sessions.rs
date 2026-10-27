//! omo-gateway `tracker/sessions.rs` on ferryx's agent control.
//!
//! omo probed its own agent sessions over ssh; ferryx lists the agent panes of this machine
//! (`local`) and of every paired host the fleet has listed, through `AgentControl`. The board
//! (`GET /sessions`) keeps omo's JSON shape, send / interrupt / answer act on the pane, and
//! every tracker request mirrors the live sessions into issues of the Sessions team (SES):
//! one issue per session, its state following the session (question, blocked, working, idle),
//! sessions gone from a host that answered move to Ended, and a deleted issue stays deleted.
use crate::storage::WriteTx;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};

/// Set to "0" to skip mirroring live sessions into tracker issues (the board still works).
pub const SYNC_ENV: &str = "OMON_TRACKER_SESSION_SYNC";
pub const PREVIEW_CHARS: usize = 400;
const TOMBSTONE_PREFIX: &str = "session_tombstone:";
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// A tracker request waits this long for a sync, then answers from the database.
const SYNC_WAIT: Duration = Duration::from_secs(3);

/// (name, type, color, position, session_state), as omo's `seed::SESSION_STATES`.
pub const SESSION_STATES: [(&str, &str, &str, f64, &str); 5] = [
    ("Needs answer", "started", "#eb5757", 1.0, "question"),
    ("Blocked", "started", "#fc7840", 2.0, "blocked"),
    ("Working", "started", "#f2c94c", 3.0, "working"),
    ("Idle", "unstarted", "#e2e2e2", 4.0, "idle"),
    ("Ended", "completed", "#95a2b3", 5.0, "ended"),
];

static CONTROL: OnceLock<Arc<dyn AgentControl>> = OnceLock::new();

/// Gives the tracker the hub's agent control. Without it the board is empty, as omo's was
/// with no hosts configured.
pub fn install(control: Arc<dyn AgentControl>) {
    let _ = CONTROL.set(control);
}

fn control() -> Option<&'static Arc<dyn AgentControl>> {
    CONTROL.get()
}

/// One live agent pane, read from an `agents.list` entry.
#[derive(Debug, Clone, Default, PartialEq)]
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

fn live_map() -> &'static RwLock<HashMap<String, LiveSession>> {
    static LIVE: OnceLock<RwLock<HashMap<String, LiveSession>>> = OnceLock::new();
    LIVE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn session_ref(machine: &str, session_id: &str) -> String {
    format!("{machine}:{session_id}")
}

fn remember(snapshots: &[Snapshot]) {
    let mut live = live_map()
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    for (machine, snapshot) in snapshots {
        let Ok(sessions) = snapshot else { continue };
        let prefix = format!("{machine}:");
        live.retain(|key, _| !key.starts_with(&prefix));
        for session in sessions {
            live.insert(session_ref(machine, &session.session_id), session.clone());
        }
    }
}

fn live_session(reference: &str) -> Option<LiveSession> {
    live_map()
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(reference)
        .cloned()
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
                // A stale (hung) host is not an answer: its issues stay as they are.
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

/// `GET /sessions`: collects, mirrors into SES issues, and returns the board.
pub async fn board(pool: &SqlitePool) -> Value {
    let Some(control) = control() else {
        return shape(&[], Utc::now());
    };
    let snapshots = collect(pool, control.as_ref()).await;
    remember(&snapshots);
    if sync_enabled() {
        if let Err(error) = sync_from_snapshots(pool, &snapshots, Utc::now()).await {
            tracing::warn!(%error, "tracker session sync failed");
        }
    }
    shape(&snapshots, Utc::now())
}

fn sync_enabled() -> bool {
    crate::hub_env::var(SYNC_ENV).ok().as_deref() != Some("0")
}

/// omo's `sync_issues`: never fails the caller and never holds a request past SYNC_WAIT.
pub async fn sync_issues(pool: &SqlitePool) {
    if !sync_enabled() {
        return;
    }
    let Some(control) = control() else { return };
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())));
    let Ok(guard) = lock.clone().try_lock_owned() else {
        return;
    };
    let pool = pool.clone();
    let control = Arc::clone(control);
    let task = tokio::spawn(async move {
        let _guard = guard;
        let snapshots = collect(&pool, control.as_ref()).await;
        remember(&snapshots);
        if let Err(error) = sync_from_snapshots(&pool, &snapshots, Utc::now()).await {
            tracing::warn!(%error, "tracker session sync failed");
        }
    });
    if tokio::time::timeout(SYNC_WAIT, task).await.is_err() {
        tracing::debug!("tracker session sync still running; serving current issues");
    }
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

fn require_control() -> Result<&'static Arc<dyn AgentControl>> {
    control().ok_or_else(|| HubError::message("agent control is not available"))
}

pub async fn send(pool: &SqlitePool, machine: &str, session_id: &str, text: &str) -> Result<Value> {
    send_on(require_control()?.as_ref(), pool, machine, session_id, text).await
}

pub async fn interrupt(pool: &SqlitePool, machine: &str, session_id: &str) -> Result<Value> {
    interrupt_on(require_control()?.as_ref(), pool, machine, session_id).await
}

pub async fn answer(
    pool: &SqlitePool,
    machine: &str,
    session_id: &str,
    text: &str,
    request: Option<&str>,
) -> Result<Value> {
    answer_on(
        require_control()?.as_ref(),
        pool,
        machine,
        session_id,
        text,
        request,
    )
    .await
}

async fn send_on(
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

async fn interrupt_on(
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
async fn answer_on(
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

/// The `session` object omo attaches to an SES issue.
pub fn session_summary(reference: &str, state: Option<&str>) -> Option<Value> {
    let (machine, session_id) = reference.split_once(':')?;
    let live = live_session(reference);
    let state = state
        .map(str::to_owned)
        .or_else(|| live.as_ref().map(|l| l.state.to_owned()))
        .unwrap_or_else(|| "ended".to_owned());
    Some(json!({
        "machine": machine,
        "session_id": session_id,
        "state": state,
        "streaming": live.as_ref().is_some_and(|l| l.streaming),
        "message_count": 0,
        "pending_messages": 0,
        "cwd": live.as_ref().and_then(|l| l.cwd.clone()),
        "updated_at": null,
        "questions": live.as_ref().and_then(|l| l.question.clone()).into_iter().collect::<Vec<_>>(),
        "last_user": "",
        "last_assistant": live.map(|l| l.last_assistant).unwrap_or_default(),
    }))
}

/// omo's `manual_state_move`: sync owns SES states. Moving a streaming session to Ended
/// interrupts it; every other move is refused.
pub async fn manual_state_move(pool: &SqlitePool, reference: &str, state_id: &str) -> Result<()> {
    let target: Option<Option<String>> =
        sqlx::query_scalar("SELECT session_state FROM tracker_states WHERE id = ?")
            .bind(state_id)
            .fetch_optional(pool)
            .await?;
    let streaming = live_session(reference).is_some_and(|live| live.streaming);
    if target.flatten().as_deref() == Some("ended") && streaming {
        let (machine, session_id) = reference
            .split_once(':')
            .ok_or_else(|| HubError::message("invalid session reference"))?;
        interrupt(pool, machine, session_id).await?;
        sync_issues(pool).await;
        return Ok(());
    }
    Err(HubError::message(
        "session issues follow the live session state; only Ended on a streaming session (interrupt) is allowed",
    ))
}

/// Keeps a deleted session issue from being re-created while its session lives.
pub async fn tombstone(pool: &SqlitePool, reference: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO tracker_meta (key, value) VALUES (?, '1') ON CONFLICT(key) DO NOTHING",
    )
    .bind(format!("{TOMBSTONE_PREFIX}{reference}"))
    .execute(pool)
    .await?;
    Ok(())
}

const SESSIONS_TEAM_SQL: &str = "SELECT t.id FROM tracker_teams t WHERE t.system = 1
     AND EXISTS (SELECT 1 FROM tracker_states s WHERE s.team_id = t.id AND s.session_state IS NOT NULL)
     ORDER BY t.created_at LIMIT 1";

fn now_utc() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// omo's `seed::ensure_sessions_team`: the Sessions system team (key SES, or the first free
/// SES1..SES99) and its states; a user team holding a key is never adopted.
pub async fn ensure_sessions_team(pool: &SqlitePool) -> Result<String> {
    let mut tx = pool.begin_write().await?;
    let existing: Option<String> = sqlx::query_scalar(SESSIONS_TEAM_SQL)
        .fetch_optional(&mut *tx)
        .await?;
    let team_id = match existing {
        Some(id) => id,
        None => {
            let mut key = None;
            for number in 0..100 {
                let candidate = if number == 0 {
                    "SES".to_owned()
                } else {
                    format!("SES{number}")
                };
                let taken: Option<i64> =
                    sqlx::query_scalar("SELECT 1 FROM tracker_teams WHERE key = ? LIMIT 1")
                        .bind(&candidate)
                        .fetch_optional(&mut *tx)
                        .await?;
                if taken.is_none() {
                    key = Some(candidate);
                    break;
                }
            }
            let key = key.ok_or_else(|| {
                HubError::message("every Sessions team key (SES, SES1..SES99) is already used")
            })?;
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO tracker_teams (id, key, name, icon, color, issue_seq, system, cycles_enabled, cycle_weeks, triage_enabled, created_at, sort_order)
                 VALUES (?, ?, 'Sessions', 'terminal', '#5e6ad2', 0, 1, 0, 2, 0, ?, 0.5)",
            )
            .bind(&id)
            .bind(key)
            .bind(now_utc())
            .execute(&mut *tx)
            .await?;
            id
        }
    };
    for (name, kind, color, position, session_state) in SESSION_STATES {
        let present: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM tracker_states WHERE team_id = ? AND session_state = ? LIMIT 1",
        )
        .bind(&team_id)
        .bind(session_state)
        .fetch_optional(&mut *tx)
        .await?;
        if present.is_some() {
            continue;
        }
        sqlx::query(
            "INSERT INTO tracker_states (id, team_id, name, type, color, position, lane_status, description, session_state)
             VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&team_id)
        .bind(name)
        .bind(kind)
        .bind(color)
        .bind(position)
        .bind(session_state)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(team_id)
}

struct SyncContext {
    team_id: String,
    team_key: String,
    states: HashMap<String, (String, String)>,
    by_state_id: HashMap<String, String>,
    me: Option<String>,
}

struct Existing {
    id: String,
    identifier: String,
    title: String,
    description: String,
    state_id: String,
    assignee_id: Option<String>,
    started_at: Option<String>,
}

fn avatar_color(seed: &str) -> String {
    let hash = Sha256::digest(seed.as_bytes());
    format!(
        "#{:02x}{:02x}{:02x}",
        40 + hash[0] % 180,
        40 + hash[1] % 180,
        40 + hash[2] % 180
    )
}

async fn ensure_machine_user(pool: &SqlitePool, machine: &str) -> Result<String> {
    let model = format!("ferryx@{machine}");
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM tracker_users WHERE kind = 'agent' AND model = ? LIMIT 1",
    )
    .bind(&model)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO tracker_users (id, name, display_name, email, avatar_color, kind, model, is_me, created_at)
         VALUES (?, ?, ?, NULL, ?, 'agent', ?, 0, ?)",
    )
    .bind(&id)
    .bind(machine)
    .bind(machine)
    .bind(avatar_color(&model))
    .bind(&model)
    .bind(now_utc())
    .execute(pool)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn record_state(
    pool: &SqlitePool,
    ctx: &SyncContext,
    issue_id: &str,
    identifier: &str,
    title: &str,
    actor: Option<&str>,
    from: Option<&str>,
    to: &str,
    now: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO tracker_activity (issue_id, actor_id, kind, from_value, to_value, created_at)
         VALUES (?, NULL, 'session_state', ?, ?, ?)",
    )
    .bind(issue_id)
    .bind(from)
    .bind(to)
    .bind(now)
    .execute(pool)
    .await?;
    let body = match to {
        "question" => format!("{identifier} needs an answer: {title}"),
        "blocked" => format!("{identifier} is blocked: {title}"),
        _ => return Ok(()),
    };
    if let Some(me) = ctx.me.as_deref() {
        sqlx::query(
            "INSERT INTO tracker_notifications (id, user_id, issue_id, project_id, kind, actor_id, body, created_at)
             VALUES (?, ?, ?, NULL, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(me)
        .bind(issue_id)
        .bind(format!("session_{to}"))
        .bind(actor)
        .bind(body)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn move_to(
    pool: &SqlitePool,
    ctx: &SyncContext,
    issue: &Existing,
    target: &str,
    now: &str,
) -> Result<bool> {
    let Some((state_id, state_type)) = ctx.states.get(target) else {
        return Ok(false);
    };
    if *state_id == issue.state_id {
        return Ok(false);
    }
    let (started_at, completed_at) = match state_type.as_str() {
        "started" => (
            issue.started_at.clone().or_else(|| Some(now.to_owned())),
            None,
        ),
        "completed" => (issue.started_at.clone(), Some(now.to_owned())),
        _ => (issue.started_at.clone(), None),
    };
    sqlx::query(
        "UPDATE tracker_issues SET state_id = ?, started_at = ?, completed_at = ?, canceled_at = NULL, updated_at = ? WHERE id = ?",
    )
    .bind(state_id)
    .bind(&started_at)
    .bind(&completed_at)
    .bind(now)
    .bind(&issue.id)
    .execute(pool)
    .await?;
    record_state(
        pool,
        ctx,
        &issue.id,
        &issue.identifier,
        &issue.title,
        issue.assignee_id.as_deref(),
        ctx.by_state_id.get(&issue.state_id).map(String::as_str),
        target,
        now,
    )
    .await?;
    Ok(true)
}

async fn load_existing(pool: &SqlitePool, reference: &str) -> Result<Option<Existing>> {
    let row = sqlx::query(
        "SELECT id, identifier, title, description, state_id, assignee_id, started_at FROM tracker_issues WHERE session_ref = ?",
    )
    .bind(reference)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| Existing {
        id: row.get("id"),
        identifier: row.get("identifier"),
        title: row.get("title"),
        description: row
            .get::<Option<String>, _>("description")
            .unwrap_or_default(),
        state_id: row.get("state_id"),
        assignee_id: row.get("assignee_id"),
        started_at: row.get("started_at"),
    }))
}

/// Issue title: the agent name, then the cwd basename, then the short session id.
pub fn session_title(name: Option<&str>, cwd: Option<&str>, session_id: &str) -> String {
    let name = name.map(str::trim).unwrap_or_default();
    let base = cwd
        .and_then(|cwd| cwd.trim_end_matches('/').rsplit('/').next())
        .filter(|base| !base.is_empty());
    match (name.is_empty() || name.starts_with('/'), base) {
        (false, Some(base)) => format!("{name} · {base}"),
        (false, None) => name.to_owned(),
        (true, Some(base)) => base.to_owned(),
        (true, None) => session_id.chars().take(8).collect(),
    }
}

fn session_description(machine: &str, live: &LiveSession) -> String {
    let mut header = format!("**Machine** {machine}");
    if let Some(cwd) = live.cwd.as_deref() {
        header.push_str(&format!(" · **Directory** `{cwd}`"));
    }
    header.push_str(&format!(" · **Session** `{}`", live.session_id));
    let last = if live.last_assistant.trim().is_empty() {
        "_None_".to_owned()
    } else {
        live.last_assistant.clone()
    };
    format!("{header}\n\n### Last status\n{last}")
}

async fn is_tombstoned(pool: &SqlitePool, reference: &str) -> Result<bool> {
    let found: Option<i64> = sqlx::query_scalar("SELECT 1 FROM tracker_meta WHERE key = ?")
        .bind(format!("{TOMBSTONE_PREFIX}{reference}"))
        .fetch_optional(pool)
        .await?;
    Ok(found.is_some())
}

#[allow(clippy::too_many_arguments)]
async fn upsert(
    pool: &SqlitePool,
    ctx: &SyncContext,
    reference: &str,
    machine: &str,
    title: &str,
    description: &str,
    target: &str,
    now: &str,
) -> Result<()> {
    if let Some(issue) = load_existing(pool, reference).await? {
        let mut changed = move_to(pool, ctx, &issue, target, now).await?;
        if issue.title != title || issue.description != description {
            sqlx::query("UPDATE tracker_issues SET title = ?, description = ? WHERE id = ?")
                .bind(title)
                .bind(description)
                .bind(&issue.id)
                .execute(pool)
                .await?;
            changed = true;
        }
        if changed {
            sqlx::query("UPDATE tracker_issues SET updated_at = ? WHERE id = ?")
                .bind(now)
                .bind(&issue.id)
                .execute(pool)
                .await?;
        }
        return Ok(());
    }
    if is_tombstoned(pool, reference).await? {
        return Ok(());
    }
    let Some((state_id, state_type)) = ctx.states.get(target) else {
        return Ok(());
    };
    let agent = ensure_machine_user(pool, machine).await?;
    let id = Uuid::new_v4().to_string();
    let started_at = matches!(state_type.as_str(), "started" | "completed").then(|| now.to_owned());
    let completed_at = (state_type == "completed").then(|| now.to_owned());
    let mut tx = pool.begin_write().await?;
    let number: i64 = sqlx::query_scalar(
        "UPDATE tracker_teams SET issue_seq = issue_seq + 1 WHERE id = ? RETURNING issue_seq",
    )
    .bind(&ctx.team_id)
    .fetch_one(&mut *tx)
    .await?;
    let identifier = format!("{}-{number}", ctx.team_key);
    sqlx::query(
        "INSERT INTO tracker_issues (id, team_id, number, identifier, title, description, state_id, priority,
            estimate, assignee_id, creator_id, project_id, milestone_id, cycle_id, parent_id, due_date,
            sort_order, lane_key, session_ref, created_at, updated_at, started_at, completed_at, canceled_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, NULL, ?, ?, NULL, NULL, NULL, NULL, NULL, 0.0, NULL, ?, ?, ?, ?, ?, NULL)",
    )
    .bind(&id)
    .bind(&ctx.team_id)
    .bind(number)
    .bind(&identifier)
    .bind(title)
    .bind(description)
    .bind(state_id)
    .bind(&agent)
    .bind(&agent)
    .bind(reference)
    .bind(now)
    .bind(now)
    .bind(&started_at)
    .bind(&completed_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    for user in std::iter::once(agent.as_str()).chain(ctx.me.as_deref()) {
        sqlx::query(
            "INSERT INTO tracker_subscribers (issue_id, user_id) VALUES (?, ?) ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .bind(user)
        .execute(pool)
        .await?;
    }
    sqlx::query(
        "INSERT INTO tracker_activity (issue_id, actor_id, kind, from_value, to_value, created_at)
         VALUES (?, ?, 'created', NULL, NULL, ?)",
    )
    .bind(&id)
    .bind(&agent)
    .bind(now)
    .execute(pool)
    .await?;
    record_state(
        pool,
        ctx,
        &id,
        &identifier,
        title,
        Some(agent.as_str()),
        None,
        target,
        now,
    )
    .await
}

/// omo's `sync_from_overview`: hosts that failed keep their issues; sessions gone from a host
/// that answered move to Ended; issues are never deleted here.
pub async fn sync_from_snapshots(
    pool: &SqlitePool,
    snapshots: &[Snapshot],
    now: DateTime<Utc>,
) -> Result<()> {
    let team_id = ensure_sessions_team(pool).await?;
    let now = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut states = HashMap::new();
    let mut by_state_id = HashMap::new();
    for row in sqlx::query(
        "SELECT id, type, session_state FROM tracker_states WHERE team_id = ? AND session_state IS NOT NULL",
    )
    .bind(&team_id)
    .fetch_all(pool)
    .await?
    {
        let id: String = row.get("id");
        let session_state: String = row.get("session_state");
        by_state_id.insert(id.clone(), session_state.clone());
        states.insert(session_state, (id, row.get::<String, _>("type")));
    }
    let me: Option<String> =
        sqlx::query_scalar("SELECT id FROM tracker_users WHERE is_me = 1 LIMIT 1")
            .fetch_optional(pool)
            .await?;
    let team_key: String = sqlx::query_scalar("SELECT key FROM tracker_teams WHERE id = ?")
        .bind(&team_id)
        .fetch_one(pool)
        .await?;
    let ctx = SyncContext {
        team_id,
        team_key,
        states,
        by_state_id,
        me,
    };
    for (machine, snapshot) in snapshots {
        let Ok(sessions) = snapshot else { continue };
        let mut present = HashSet::new();
        for live in sessions {
            let reference = session_ref(machine, &live.session_id);
            let title = session_title(live.name.as_deref(), live.cwd.as_deref(), &live.session_id);
            let description = session_description(machine, live);
            upsert(
                pool,
                &ctx,
                &reference,
                machine,
                &title,
                &description,
                live.state,
                &now,
            )
            .await?;
            present.insert(reference);
        }
        let prefix = format!("{TOMBSTONE_PREFIX}{machine}:");
        let tombstones: Vec<String> =
            sqlx::query_scalar("SELECT key FROM tracker_meta WHERE substr(key, 1, ?) = ?")
                .bind(prefix.chars().count() as i64)
                .bind(&prefix)
                .fetch_all(pool)
                .await?;
        for key in tombstones {
            if !present.contains(&key[TOMBSTONE_PREFIX.len()..]) {
                sqlx::query("DELETE FROM tracker_meta WHERE key = ?")
                    .bind(&key)
                    .execute(pool)
                    .await?;
            }
        }
        let tracked: Vec<String> = sqlx::query_scalar(
            "SELECT session_ref FROM tracker_issues WHERE session_ref IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;
        for reference in tracked {
            let same_machine = reference
                .split_once(':')
                .is_some_and(|(m, _)| m == machine.as_str());
            if !same_machine || present.contains(&reference) {
                continue;
            }
            if let Some(issue) = load_existing(pool, &reference).await? {
                move_to(pool, &ctx, &issue, "ended", &now).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
