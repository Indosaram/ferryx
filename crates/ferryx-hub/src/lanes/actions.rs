//! Lane opening and owner lane actions for an external tracker (`/api/agent/lanes`): a
//! queued lane with its brief written to the brief dir, and send / object / close / reopen.
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

#[cfg(test)]
#[path = "actions/contract_tests.rs"]
mod contract_tests;

/// The brief file text an external tracker shows for a lane: the first 20 KiB, lossy UTF-8;
/// empty when there is no file or it cannot be read.
pub async fn read_brief_content(brief_path: &str) -> String {
    if brief_path.is_empty() {
        return String::new();
    }
    match tokio::fs::read(brief_path).await {
        Ok(bytes) => {
            let limit = bytes.len().min(20 * 1024);
            String::from_utf8_lossy(&bytes[..limit]).into_owned()
        }
        Err(_) => String::new(),
    }
}

/// The lane side of a delegation: a queued lane keyed `<key_base>-<4 hex>`, its brief written to
/// the tracker brief dir (not linked to the lane, as before), and a valid Slack thread ref bound.
pub(crate) async fn open_lane(
    pool: &SqlitePool,
    key_base: &str,
    title: &str,
    brief: &str,
    cwd: &str,
    done: &str,
    thread: Option<&str>,
) -> Result<String> {
    let request = LaneRequest { key_base, title, brief, cwd, done, thread };
    open_lane_with_policy(pool, request, false).await
}

pub(crate) struct LaneRequest<'a> {
    pub key_base: &'a str,
    pub title: &'a str,
    pub brief: &'a str,
    pub cwd: &'a str,
    pub done: &'a str,
    pub thread: Option<&'a str>,
}

pub(crate) async fn open_lane_with_policy(
    pool: &SqlitePool,
    request: LaneRequest<'_>,
    cron_remediation: bool,
) -> Result<String> {
    let LaneRequest { key_base, title, brief, cwd, done, thread } = request;
    let parsed = crate::lanes::signals::parse_signals(done)
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    if !cron_remediation && parsed.iter().all(|signal| matches!(signal, crate::lanes::signals::DoneSignal::File { .. })) {
        return Err(crate::error::HubError::message("needs a verifiable done check"));
    }
    let brief_text = brief.trim();
    if brief_text.is_empty() || !brief_text.to_ascii_lowercase().contains("source:") || !brief_text.to_ascii_lowercase().contains("owner request:") {
        return Err(crate::error::HubError::message("brief must contain the owner's original request and source message reference"));
    }
    let thread = thread
        .filter(|value| crate::lanes::threads::parse_ref(value).is_some())
        .ok_or_else(|| crate::error::HubError::message("lane requires a valid thread binding"))?;
    for signal in &parsed {
        if !cron_remediation {
            if let crate::lanes::signals::DoneSignal::Command { .. } = signal {
                let results = crate::lanes::signals::check_signals(std::slice::from_ref(signal), std::path::Path::new(cwd)).await;
                if results.first().is_some_and(|result| result.passed) {
                    return Err(crate::error::HubError::message(format!("command done check already passes at open: {}", results[0].detail)));
                }
            }
        }
    }
    let stored_signals = serde_json::to_string(&parsed)
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    let lane_key = sanitize_lane_key(&format!(
        "{}-{:04x}",
        key_base,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.subsec_nanos() & 0xffff)
            .unwrap_or(1)
    ));
    let brief_dir = crate::hub_env::var("OMON_TRACKER_BRIEF_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("ferryx-tracker-briefs"));
    tokio::fs::create_dir_all(&brief_dir)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    let brief_path = brief_dir.join(format!("{lane_key}.md"));
    tokio::fs::write(&brief_path, brief)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    crate::lanes::state::insert_lane(pool, &lane_key, title, "tracker", cwd, &stored_signals)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    crate::lanes::worktree::create(pool, &lane_key, cwd).await?;
    sqlx::query("UPDATE lanes SET thread_channel_id = ? WHERE key = ?")
        .bind(thread)
        .bind(&lane_key)
        .execute(pool)
        .await?;
    let kickoff = format!("Starting: {title}. Done checks: {done}");
    sqlx::query("INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)")
        .bind(&lane_key)
        .bind("tracker")
        .bind("kickoff")
        .bind(kickoff)
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
    Ok(lane_key)
}

pub async fn migrate_legacy_file_only_lanes(pool: &SqlitePool) -> Result<u64> {
    let lanes: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT key, status, lead_session_key, done_signals FROM lanes
         WHERE status NOT IN ('verified_done','closed','failed') AND key NOT LIKE 'cron-fix-%'",
    )
    .fetch_all(pool)
    .await?;
    let mut blocked = 0;
    for (key, status, lead, stored) in lanes {
        let parsed = serde_json::from_str::<Vec<crate::lanes::signals::DoneSignal>>(&stored)
            .or_else(|_| crate::lanes::signals::parse_signals(&stored));
        if !parsed.is_ok_and(|signals| !signals.is_empty() && signals.iter().all(|signal| matches!(signal, crate::lanes::signals::DoneSignal::File { .. }))) {
            continue;
        }
        let transitioned = if status == "blocked" {
            true
        } else {
            crate::lanes::state::transition_unless_supervised(
                pool, &key, crate::lanes::state::LaneStatus::Blocked,
                Some("needs a verifiable done check"),
            ).await.map_err(|error| crate::error::HubError::message(error.to_string()))?
        };
        if transitioned {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lane_events WHERE lane_key = ? AND kind = 'legacy_file_only_done_check')")
                .bind(&key).fetch_one(pool).await?;
            if !exists {
                crate::lanes::dispatch::enqueue(pool, &key, &lead, "legacy_file_only_done_check", "Lane blocked: needs a verifiable done check").await?;
            }
            blocked += 1;
        }
    }
    Ok(blocked)
}

fn sanitize_lane_key(key: &str) -> String {
    let mut out = String::new();
    for ch in key.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
            out.push(ch);
        } else if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        out.push_str("lane-1");
    }
    if out.len() > 48 {
        out.truncate(48);
    }
    out
}

/// True for the actions that act on the lane: send, object, close and reopen.
pub(crate) fn is_lane_action(action: &str) -> bool {
    matches!(action, "send" | "object" | "close" | "reopen")
}

/// The lane side of a lane action ([`is_lane_action`]): the transition, refused for a lane the
/// supervisor owns, then a `lane_events` row. The error is the conflict message.
pub(crate) async fn apply_lane_action(
    pool: &SqlitePool,
    lane_key: &str,
    action: &str,
    note: &str,
    force: bool,
) -> std::result::Result<(), String> {
    let reason = match action {
        "close" if force => Some(if note.starts_with("forced:") {
            note.to_owned()
        } else if note.is_empty() {
            "forced: closed from tracker".into()
        } else {
            format!("forced: {note}")
        }),
        "reopen" => Some("note: reopened".into()),
        _ => Some(note.to_owned()),
    };
    let target = match action {
        "send" | "reopen" => crate::lanes::state::LaneStatus::Working,
        "object" => crate::lanes::state::LaneStatus::Blocked,
        "close" => crate::lanes::state::LaneStatus::Closed,
        _ => crate::lanes::state::LaneStatus::Working,
    };
    // A lane the supervisor owns changes only through the supervisor; the UPDATE itself
    // refuses it, also when it was adopted a moment ago.
    match crate::lanes::state::transition_unless_supervised(
        pool,
        lane_key,
        target,
        reason.as_deref(),
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            return Err(format!(
                "{}: lane {lane_key} belongs to a supervisor goal; act on it through the supervisor",
                crate::supervisor::SUPERVISOR_OWNED
            ))
        }
        Err(error) => return Err(error.to_string()),
    }
    // A reopened lane starts a new run: a fresh quiet clock, evidence set and ceilings.
    if action == "reopen" {
        let now = chrono::Utc::now().timestamp_millis();
        let ceilings = crate::lanes::oversight::Ceilings::default();
        if let Err(error) = crate::lanes::oversight::start_run(pool, lane_key, ceilings, now).await {
            return Err(error.to_string());
        }
    }
    let kind = if action == "object" {
        "blocked"
    } else {
        "lane_action"
    };
    let _ = sqlx::query(
        "INSERT INTO lane_events(lane_key, lead_session_key, kind, body, created_at) VALUES(?,?,?,?,?)",
    )
    .bind(lane_key)
    .bind("tracker")
    .bind(kind)
    .bind(note)
    .bind(now_rfc3339())
    .execute(pool)
    .await;
    Ok(())
}
