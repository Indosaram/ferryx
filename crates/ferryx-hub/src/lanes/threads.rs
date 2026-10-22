//! Slack lane thread refs. The lane stores `slack:<channel>:<root_ts>` and a
//! status post carries the working, waiting, or done emoji.
use sqlx::SqlitePool;

use crate::error::Result;
use crate::storage::now_rfc3339;

pub const REF_PREFIX: &str = "slack:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadStatus {
    Working,
    Waiting,
    Done,
}

impl ThreadStatus {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Working => "🔄 ",
            Self::Waiting => "⏸ ",
            Self::Done => "✅ ",
        }
    }

    pub fn reaction(self) -> &'static str {
        match self {
            Self::Working => "arrows_counterclockwise",
            Self::Waiting => "double_vertical_bar",
            Self::Done => "white_check_mark",
        }
    }
}

pub fn thread_ref(channel: &str, thread_ts: &str) -> String {
    format!("{REF_PREFIX}{channel}:{thread_ts}")
}

pub fn parse_ref(thread: &str) -> Option<(&str, &str)> {
    let (channel, ts) = thread.strip_prefix(REF_PREFIX)?.split_once(':')?;
    (is_channel_id(channel)
        && !ts.is_empty()
        && ts.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.'))
    .then_some((channel, ts))
}

fn is_channel_id(id: &str) -> bool {
    id.len() >= 2
        && matches!(id.as_bytes()[0], b'C' | b'G' | b'D')
        && id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

pub const DISCORD_THREAD_NAME_MAX_CHARS: usize = 100;

pub fn strip_status_prefix(title: &str) -> &str {
    let trimmed = title.trim();
    for prefix in ["🔄", "⏸", "✅"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.strip_prefix(' ').unwrap_or(rest).trim();
        }
    }
    trimmed
}

/// Discord thread titles carry the same status emoji and stop at 100 characters.
pub fn discord_thread_name(status: ThreadStatus, title: &str) -> String {
    let clean = strip_status_prefix(title);
    let base = if clean.is_empty() { "lane" } else { clean };
    let full = format!("{}{base}", status.prefix());
    full.chars().take(DISCORD_THREAD_NAME_MAX_CHARS).collect()
}

pub fn thread_status_for_lane(status: crate::lanes::state::LaneStatus) -> ThreadStatus {
    use crate::lanes::state::LaneStatus;
    match status {
        LaneStatus::Working => ThreadStatus::Working,
        LaneStatus::Queued | LaneStatus::Blocked | LaneStatus::NotDone => ThreadStatus::Waiting,
        LaneStatus::DoneClaimed
        | LaneStatus::VerifiedDone
        | LaneStatus::Closed
        | LaneStatus::Failed => ThreadStatus::Done,
    }
}

pub fn starter_text(status: ThreadStatus, key: &str, title: &str) -> String {
    let title = escape(title.trim());
    let key = escape(key.trim());
    if key.is_empty() {
        format!("{}lane: {title}", status.prefix())
    } else {
        format!("{}lane {key}: {title}", status.prefix())
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("<!", "&lt;!")
        .replace("<@", "&lt;@")
}

pub async fn bind_lane_thread(pool: &SqlitePool, lane_key: &str, bot_id: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO lane_thread_bindings(lane_key, bot_id, bound_at) VALUES(?,?,?)
         ON CONFLICT(lane_key) DO UPDATE SET bot_id = excluded.bot_id, bound_at = excluded.bound_at",
    )
    .bind(lane_key)
    .bind(bot_id)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remember_thread_ref(pool: &SqlitePool, lane_key: &str, thread: &str) -> Result<()> {
    sqlx::query("UPDATE lanes SET thread_channel_id = ? WHERE key = ?")
        .bind(thread)
        .bind(lane_key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slack_ref_parses_and_a_status_post_names_the_lane() {
        let thread = thread_ref("C0123", "1700.0001");
        assert_eq!(parse_ref(&thread), Some(("C0123", "1700.0001")));
        assert!(parse_ref("slack:nope:1").is_none());
        assert_eq!(
            starter_text(ThreadStatus::Working, "gate", "Ship <it>"),
            "🔄 lane gate: Ship &lt;it&gt;"
        );
        assert_eq!(ThreadStatus::Done.reaction(), "white_check_mark");
        assert_eq!(ThreadStatus::Waiting.reaction(), "double_vertical_bar");
        assert_eq!(
            discord_thread_name(ThreadStatus::Done, "🔄 Ship it"),
            "✅ Ship it"
        );
        let long = discord_thread_name(ThreadStatus::Working, &"x".repeat(200));
        assert_eq!(long.chars().count(), DISCORD_THREAD_NAME_MAX_CHARS);
        assert!(long.starts_with("🔄 "));
        assert_eq!(
            thread_status_for_lane(crate::lanes::state::LaneStatus::Blocked),
            ThreadStatus::Waiting
        );
    }

    #[tokio::test]
    async fn a_binding_and_a_thread_ref_are_stored() {
        let pool = crate::storage::memory().await.unwrap();
        sqlx::query(
            "INSERT INTO lanes(key, title, lead_session_key, cwd, session_key, status, done_signals, last_activity_at) VALUES('gate','Gate','meta','/tmp','meta','queued','[]','2026-10-21T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let thread = thread_ref("C0123", "1700.0001");
        remember_thread_ref(&pool, "gate", &thread).await.unwrap();
        bind_lane_thread(&pool, "gate", "B1").await.unwrap();
        let stored: String =
            sqlx::query_scalar("SELECT thread_channel_id FROM lanes WHERE key = 'gate'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, thread);
        let bot: String =
            sqlx::query_scalar("SELECT bot_id FROM lane_thread_bindings WHERE lane_key = 'gate'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bot, "B1");
    }
}
