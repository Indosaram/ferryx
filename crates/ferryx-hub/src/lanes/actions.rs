//! Lane opening and owner lane actions for an external tracker (`/api/agent/lanes`): a
//! queued lane with its brief written to the brief dir, and send / object / close / reopen.
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

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
    crate::lanes::state::insert_lane(pool, &lane_key, title, "tracker", cwd, done)
        .await
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    if let Some(thread) = thread.filter(|value| crate::lanes::threads::parse_ref(value).is_some()) {
        sqlx::query("UPDATE lanes SET thread_channel_id = ? WHERE key = ?")
            .bind(thread)
            .bind(&lane_key)
            .execute(pool)
            .await?;
    }
    Ok(lane_key)
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
