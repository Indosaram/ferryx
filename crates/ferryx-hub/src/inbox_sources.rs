//! Inbox source parsers. These are the pure readers from omo-gateway: Kakao
//! (PirateTalk), iMessage, zele mail, and GitHub notification/PR diffs.
//! A fetch that cannot be parsed fails the batch. Nothing here invents a timestamp.
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::now_rfc3339;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceItem {
    pub source: &'static str,
    pub source_ref: String,
    pub snippet: String,
    pub received_at: DateTime<Utc>,
    pub thread_ref: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
    pub link: Option<String>,
}

pub async fn commit_source_items(pool: &SqlitePool, items: &[SourceItem]) -> Result<u64> {
    let mut copied = 0u64;
    for item in items {
        if item.snippet.trim().is_empty() {
            continue;
        }
        let id = format!("{}:{}", item.source, item.source_ref);
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO inbox_items(id, source, source_ref, thread_ref, author, title, snippet, received_at, link, raw_hash, collected_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(item.source)
        .bind(&item.source_ref)
        .bind(&item.thread_ref)
        .bind(&item.author)
        .bind(&item.title)
        .bind(&item.snippet)
        .bind(item.received_at.to_rfc3339())
        .bind(&item.link)
        .bind(&item.source_ref)
        .bind(now_rfc3339())
        .execute(pool)
        .await?;
        copied += inserted.rows_affected();
    }
    Ok(copied)
}

fn optional_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

fn array<'a>(value: &'a Value, context: &str) -> Result<Vec<&'a Value>> {
    match value {
        Value::Array(items) => Ok(items.iter().collect()),
        Value::Object(map) => map
            .get("items")
            .or_else(|| map.get("messages"))
            .and_then(Value::as_array)
            .map(|items| items.iter().collect())
            .ok_or_else(|| HubError::message(format!("{context} response has no array"))),
        _ => Err(HubError::message(format!(
            "{context} response must contain JSON objects"
        ))),
    }
}

/// PirateTalk page. `time` is Unix seconds. A repeated id fails the page.
/// Empty text is skipped after the paging bounds move.
pub fn parse_kakao_messages(
    room_id: &str,
    room_title: Option<&str>,
    room_type: &str,
    page: &Value,
    floor: &str,
) -> Result<(Vec<SourceItem>, Option<String>, Option<String>)> {
    let messages = array(page, "PirateTalk message page")?;
    let mut items = Vec::new();
    let mut page_oldest: Option<String> = None;
    let mut page_newest: Option<String> = None;
    let mut seen = HashSet::new();
    for message in messages {
        let message_id = message
            .get("id")
            .or_else(|| message.get("logId"))
            .and_then(Value::as_str)
            .ok_or_else(|| HubError::message("PirateTalk message missing id"))?;
        if !seen.insert(message_id.to_owned()) {
            return Err(HubError::message("PirateTalk paging repeated a message id"));
        }
        if !floor.is_empty() && message_id <= floor {
            continue;
        }
        page_oldest = Some(page_oldest.map_or_else(
            || message_id.to_owned(),
            |old| old.min(message_id.to_owned()),
        ));
        page_newest = Some(page_newest.map_or_else(
            || message_id.to_owned(),
            |new| new.max(message_id.to_owned()),
        ));
        let Some(text) = optional_string(message, &["text", "message", "content", "msg"]) else {
            continue;
        };
        let received = if let Some(time) = message.get("time") {
            let seconds = time.as_i64().ok_or_else(|| {
                HubError::message("PirateTalk message time must be integral Unix seconds")
            })?;
            DateTime::<Utc>::from_timestamp(seconds, 0)
                .ok_or_else(|| HubError::message("PirateTalk message time is out of range"))?
        } else {
            let timestamp =
                optional_string(message, &["createdAt", "created_at", "timestamp", "date"])
                    .ok_or_else(|| HubError::message("PirateTalk message missing timestamp"))?;
            DateTime::parse_from_rfc3339(&timestamp)
                .map(|parsed| parsed.with_timezone(&Utc))
                .map_err(|error| {
                    HubError::message(format!("invalid PirateTalk message timestamp: {error}"))
                })?
        };
        items.push(SourceItem {
            source: "kakao",
            source_ref: message_id.to_owned(),
            snippet: text,
            received_at: received,
            thread_ref: Some(room_id.to_owned()),
            author: optional_string(message, &["senderName", "authorName", "userName"]),
            title: room_title.map(str::to_owned),
            link: None,
        });
        let _ = room_type;
    }
    Ok((items, page_oldest, page_newest))
}

pub fn parse_kakao_cursor(cursor: Option<&str>) -> HashMap<String, String> {
    cursor
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

/// iMessage history. Messages from the owner and empty text are skipped.
/// The cursor is the newest RFC3339 timestamp seen.
pub fn parse_imessage(
    chats: &Value,
    histories: &[(String, Value)],
    cursor: Option<&str>,
) -> Result<(Vec<SourceItem>, String)> {
    let rows = objects(chats, "imsg chats")?;
    let since = cursor.and_then(|value| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc))
    });
    let mut newest: Option<DateTime<Utc>> = since.map(|value| value.with_timezone(&Utc));
    let mut items = Vec::new();
    for chat in rows {
        let chat_id = identifier(
            chat.get("id")
                .ok_or_else(|| HubError::message("imsg chat missing id"))?,
            "imsg chat id",
        )?;
        let last = chat
            .get("last_message_at")
            .and_then(Value::as_str)
            .ok_or_else(|| HubError::message("imsg chat missing last_message_at"))?;
        let chat_time = DateTime::parse_from_rfc3339(last)
            .map_err(|error| HubError::message(format!("invalid imsg last_message_at: {error}")))?
            .with_timezone(&Utc);
        if since.is_some_and(|cursor| chat_time <= cursor) {
            continue;
        }
        let history = histories
            .iter()
            .find(|(id, _)| id == &chat_id)
            .map(|(_, value)| value)
            .ok_or_else(|| HubError::message(format!("imsg history missing for chat {chat_id}")))?;
        for message in objects(history, "imsg history")? {
            if message
                .get("is_from_me")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let timestamp = message
                .get("created_at")
                .or_else(|| message.get("date"))
                .or_else(|| message.get("timestamp"))
                .and_then(Value::as_str)
                .ok_or_else(|| HubError::message("imsg message missing date"))?;
            let received = DateTime::parse_from_rfc3339(timestamp)
                .map_err(|error| HubError::message(format!("invalid imsg message date: {error}")))?
                .with_timezone(&Utc);
            if since.is_some_and(|cursor| received <= cursor) {
                continue;
            }
            let id = identifier(
                message
                    .get("id")
                    .or_else(|| message.get("rowid"))
                    .ok_or_else(|| HubError::message("imsg message missing id/rowid"))?,
                "imsg message id",
            )?;
            newest = Some(newest.map_or(received, |old| old.max(received)));
            let Some(text) = optional_string(message, &["text", "text_content", "body"]) else {
                continue;
            };
            items.push(SourceItem {
                source: "imessage",
                source_ref: id,
                snippet: text,
                received_at: received,
                thread_ref: Some(chat_id.clone()),
                author: optional_string(message, &["sender", "handle", "from"]).or_else(|| {
                    chat.get("identifier")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
                title: optional_string(chat, &["name", "display_name"]),
                link: None,
            });
        }
    }
    let next = newest
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| cursor.unwrap_or("").to_owned());
    Ok((items, next))
}

fn objects<'a>(value: &'a Value, context: &str) -> Result<Vec<&'a Value>> {
    match value {
        Value::Array(values) => Ok(values.iter().collect()),
        Value::Object(_) => Ok(vec![value]),
        Value::Null => Ok(Vec::new()),
        _ => Err(HubError::message(format!(
            "{context} response must contain JSON objects"
        ))),
    }
}

fn identifier(value: &Value, field: &str) -> Result<String> {
    match value {
        Value::String(text) if !text.is_empty() => Ok(text.clone()),
        Value::Number(number) => Ok(number.to_string()),
        _ => Err(HubError::message(format!(
            "{field} must be a non-empty string or number"
        ))),
    }
}

/// zele `mail list`. One unreadable date fails the whole batch so the cursor does not move.
pub fn parse_mail_at(
    value: &Value,
    cursor: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(Vec<SourceItem>, String)> {
    let root_items = value
        .get("items")
        .ok_or_else(|| HubError::message("zele mail list response missing items"))?;
    let rows = array(root_items, "zele mail list items")?;
    let mut items = Vec::new();
    let mut newest_ts: Option<i64> = cursor.and_then(|value| value.parse().ok());
    let mut unreadable = Vec::new();
    for row in rows {
        let id = row
            .get("thread_id")
            .and_then(Value::as_str)
            .or_else(|| row.get("id").and_then(Value::as_str))
            .ok_or_else(|| HubError::message("mail thread missing thread_id"))?;
        let snippet = optional_string(row, &["snippet", "summary", "preview"]).unwrap_or_default();
        let received_at = match mail_date(row, now) {
            Some(at) => at,
            None => {
                let raw = row
                    .get("internal_date")
                    .or_else(|| row.get("date"))
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "<missing>".into());
                unreadable.push((id.to_owned(), raw));
                continue;
            }
        };
        newest_ts = Some(newest_ts.map_or(received_at.timestamp(), |cur| {
            cur.max(received_at.timestamp())
        }));
        items.push(SourceItem {
            source: "mail",
            source_ref: id.to_owned(),
            snippet,
            received_at,
            thread_ref: Some(id.to_owned()),
            author: optional_string(row, &["from", "sender"]),
            title: optional_string(row, &["subject", "title"]),
            link: Some(format!("https://mail.google.com/mail/u/0/#all/{id}")),
        });
    }
    if !unreadable.is_empty() {
        let shown: Vec<String> = unreadable
            .iter()
            .take(3)
            .map(|(thread, raw)| format!("thread {thread}: {raw}"))
            .collect();
        return Err(HubError::message(format!(
            "zele mail list returned an unreadable date for {} thread(s) [{}]; nothing was committed and the cursor did not advance",
            unreadable.len(),
            shown.join(", ")
        )));
    }
    let next = newest_ts
        .map(|ts| ts.to_string())
        .unwrap_or_else(|| cursor.unwrap_or("").to_owned());
    Ok((items, next))
}

fn mail_date(row: &Value, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Some(value) = row
        .get("internal_date")
        .or_else(|| row.get("date"))
        .and_then(Value::as_str)
    {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
            return Some(parsed.with_timezone(&Utc));
        }
        if let Some(parsed) = display_date(value, now) {
            return Some(parsed);
        }
        if let Ok(millis) = value.parse::<i64>() {
            return millis_date(millis);
        }
    }
    row.get("internal_date")
        .or_else(|| row.get("date"))
        .and_then(Value::as_i64)
        .and_then(millis_date)
}

fn millis_date(millis: i64) -> Option<DateTime<Utc>> {
    let millis = if millis > 10_000_000_000 {
        millis
    } else {
        millis.saturating_mul(1000)
    };
    DateTime::from_timestamp_millis(millis)
}

fn display_date(value: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("just now") {
        return Some(now - Duration::minutes(1));
    }
    if let Some(amount) = value.strip_suffix(" ago") {
        let unit = amount.chars().last()?;
        let n: i64 = amount[..amount.len() - unit.len_utf8()].parse().ok()?;
        if n < 0 {
            return None;
        }
        let count = n.checked_add(1)?;
        let span = match unit {
            'm' => Duration::try_minutes(count)?,
            'h' => Duration::try_hours(count)?,
            'd' => Duration::try_days(count)?,
            _ => return None,
        };
        return now.checked_sub_signed(span);
    }
    let today = now.date_naive();
    let parse_day = |text: &str| {
        ["%b %-d, %Y", "%b %d, %Y", "%b %e, %Y"]
            .iter()
            .find_map(|format| NaiveDate::parse_from_str(text, format).ok())
    };
    let day = match parse_day(value) {
        Some(day) => day,
        None => {
            let this_year = parse_day(&format!("{value}, {}", today.year()))?;
            if this_year > today {
                this_year.with_year(today.year() - 1)?
            } else {
                this_year
            }
        }
    };
    let start = chrono::Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some(start.with_timezone(&Utc))
}

pub const GITHUB_NOTIFY_REASONS: &[&str] = &[
    "review_requested",
    "mention",
    "team_mention",
    "comment",
    "assign",
    "author",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubPoll {
    pub items: Vec<SourceItem>,
    pub cursor: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Snapshot {
    #[serde(default)]
    prs: BTreeMap<String, PrState>,
    #[serde(default)]
    notes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PrState {
    state: String,
    ci: Option<String>,
    oid: Option<String>,
    updated_at: String,
}

#[derive(Clone, Copy)]
enum PrChange {
    CiFailed,
    CiRecovered,
    Merged,
    Closed,
}

impl PrChange {
    fn tag(self) -> &'static str {
        match self {
            Self::CiFailed => "ci_failed",
            Self::CiRecovered => "ci_recovered",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CiFailed => "CI failed",
            Self::CiRecovered => "CI recovered",
            Self::Merged => "merged",
            Self::Closed => "closed without merge",
        }
    }
}

/// First poll records a baseline and emits nothing. Later polls emit CI, merge, close, and notifications.
pub fn github_diff(cursor: Option<&str>, notifications: &Value, prs: &Value) -> Result<GithubPoll> {
    let previous: Option<Snapshot> = cursor
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| serde_json::from_str(value).ok());
    let baseline = previous.is_none();
    let previous = previous.unwrap_or_default();
    let mut next = Snapshot::default();
    let mut items = Vec::new();
    let nodes = prs
        .pointer("/data/search/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            let detail = prs
                .pointer("/errors/0/message")
                .and_then(Value::as_str)
                .unwrap_or("missing data.search.nodes");
            HubError::message(format!("github source PR query failed: {detail}"))
        })?;
    for node in nodes {
        let Some(url) = node.get("url").and_then(Value::as_str) else {
            continue;
        };
        let title = text(node, "title");
        let repo = node
            .pointer("/repository/nameWithOwner")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let commit = node.pointer("/commits/nodes/0/commit");
        let current = PrState {
            state: text(node, "state"),
            ci: commit
                .and_then(|commit| commit.pointer("/statusCheckRollup/state"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            oid: commit
                .and_then(|commit| commit.get("oid"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            updated_at: text(node, "updatedAt"),
        };
        if !baseline {
            if let Some(prev) = previous.prs.get(url) {
                for change in pr_changes(prev, &current) {
                    items.push(pr_item(url, &title, &repo, node, &current, change));
                }
            }
        }
        next.prs.insert(url.to_owned(), current);
    }
    let threads = notifications
        .as_array()
        .ok_or_else(|| HubError::message("github notifications response must be a JSON array"))?;
    for thread in threads {
        let (Some(id), Some(updated)) = (
            thread.get("id").and_then(Value::as_str),
            thread.get("updated_at").and_then(Value::as_str),
        ) else {
            continue;
        };
        next.notes.insert(id.to_owned(), updated.to_owned());
        let reason = thread
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if baseline
            || !GITHUB_NOTIFY_REASONS.contains(&reason)
            || previous.notes.get(id).map(String::as_str) == Some(updated)
        {
            continue;
        }
        items.push(notification_item(thread, reason, updated));
    }
    let cursor = serde_json::to_string(&next)
        .map_err(|error| HubError::message(format!("github snapshot: {error}")))?;
    Ok(GithubPoll { items, cursor })
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn failing(ci: Option<&str>) -> bool {
    matches!(ci, Some("FAILURE" | "ERROR"))
}

fn pr_changes(prev: &PrState, cur: &PrState) -> Vec<PrChange> {
    if prev.state == "OPEN" && cur.state == "MERGED" {
        return vec![PrChange::Merged];
    }
    if prev.state == "OPEN" && cur.state == "CLOSED" {
        return vec![PrChange::Closed];
    }
    if cur.state != "OPEN" {
        return Vec::new();
    }
    let now_failing = failing(cur.ci.as_deref());
    let was_failing = failing(prev.ci.as_deref());
    if now_failing && !(was_failing && prev.oid == cur.oid) {
        vec![PrChange::CiFailed]
    } else if was_failing && cur.ci.as_deref() == Some("SUCCESS") {
        vec![PrChange::CiRecovered]
    } else {
        Vec::new()
    }
}

fn received_at(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn pr_item(
    url: &str,
    title: &str,
    repo: &str,
    node: &Value,
    current: &PrState,
    change: PrChange,
) -> SourceItem {
    let number = node
        .get("number")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let mut tag = change.tag().to_owned();
    if matches!(change, PrChange::CiFailed | PrChange::CiRecovered) {
        if let Some(oid) = current.oid.as_deref() {
            tag.push('@');
            tag.extend(oid.chars().take(12));
        }
    }
    SourceItem {
        source: "github",
        source_ref: format!("gh:{url}:{}:{tag}", current.updated_at),
        snippet: format!("[{}] {repo}#{number}: {title}", change.label()),
        received_at: received_at(&current.updated_at),
        thread_ref: Some(url.to_owned()),
        author: Some(repo.to_owned()),
        title: Some(title.to_owned()),
        link: Some(url.to_owned()),
    }
}

fn notification_item(thread: &Value, reason: &str, updated: &str) -> SourceItem {
    let repo = thread
        .pointer("/repository/full_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let title = thread
        .pointer("/subject/title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let link = thread
        .pointer("/subject/url")
        .and_then(Value::as_str)
        .and_then(api_to_html)
        .unwrap_or_else(|| format!("https://github.com/{repo}"));
    SourceItem {
        source: "github",
        source_ref: format!("gh:{link}:{updated}"),
        snippet: format!("[{}] {repo}: {title}", reason.replace('_', " ")),
        received_at: received_at(updated),
        thread_ref: Some(link.clone()),
        author: Some(repo.to_owned()),
        title: Some(title.to_owned()),
        link: Some(link),
    }
}

pub fn api_to_html(api: &str) -> Option<String> {
    let path = api.strip_prefix("https://api.github.com/repos/")?;
    let path = path
        .replacen("/pulls/", "/pull/", 1)
        .replacen("/commits/", "/commit/", 1);
    Some(format!("https://github.com/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kakao_uses_unix_seconds_and_rejects_a_repeated_id() {
        let page = json!([{
            "id": "m2",
            "time": 1_700_000_000,
            "text": "hello",
            "senderName": "Ada"
        }, {
            "id": "m1",
            "text": ""
        }]);
        let (items, oldest, newest) =
            parse_kakao_messages("room", Some("Room"), "DirectChat", &page, "").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snippet, "hello");
        assert_eq!(items[0].received_at.timestamp(), 1_700_000_000);
        assert_eq!(oldest.as_deref(), Some("m1"));
        assert_eq!(newest.as_deref(), Some("m2"));
        let repeated =
            json!([{"id": "m2", "time": 1, "text": "a"}, {"id": "m2", "time": 2, "text": "b"}]);
        assert!(parse_kakao_messages("room", None, "DirectChat", &repeated, "").is_err());
    }

    #[test]
    fn imessage_skips_self_and_empty_text_and_advances_an_rfc3339_cursor() {
        let chats = json!([{"id": 7, "last_message_at": "2026-10-21T03:00:00Z", "name": "Ada"}]);
        let history = json!([
            {"id": 1, "is_from_me": true, "created_at": "2026-10-21T02:00:00Z", "text": "mine"},
            {"rowid": 2, "created_at": "2026-10-21T03:00:00Z", "text": ""},
            {"id": "3", "created_at": "2026-10-21T02:30:00Z", "text": "hi", "sender": "+1"}
        ]);
        let (items, cursor) = parse_imessage(&chats, &[("7".into(), history)], None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].snippet, "hi");
        assert_eq!(items[0].source, "imessage");
        assert!(cursor.starts_with("2026-10-21T03:00:00"));
    }

    #[test]
    fn mail_fails_the_batch_when_a_date_cannot_be_read() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let good = json!({"items": [{"thread_id": "t1", "snippet": "hello", "date": "just now", "subject": "Hi", "from": "a@b.c"}]});
        let (items, cursor) = parse_mail_at(&good, None, now).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].link.as_ref().unwrap().contains("t1"));
        assert!(!cursor.is_empty());
        let bad = json!({"items": [{"thread_id": "t2", "snippet": "x", "date": "not-a-date"}]});
        let error = parse_mail_at(&bad, Some("1"), now).unwrap_err().to_string();
        assert!(error.contains("unreadable date"));
        assert!(error.contains("cursor did not advance"));
    }

    #[test]
    fn github_baseline_then_merge_and_review_request_emit_once() {
        let prs = json!({"data": {"search": {"nodes": [{
            "url": "https://github.com/o/r/pull/7", "title": "t", "number": 7,
            "updatedAt": "2026-10-15T11:57:11Z", "state": "OPEN",
            "repository": {"nameWithOwner": "o/r"},
            "commits": {"nodes": [{"commit": {"oid": "abc", "statusCheckRollup": {"state": "SUCCESS"}}}]}
        }]}}});
        let base = github_diff(None, &json!([]), &prs).unwrap();
        assert!(base.items.is_empty());
        let mut merged = prs.clone();
        merged["data"]["search"]["nodes"][0]["state"] = json!("MERGED");
        let once = github_diff(Some(&base.cursor), &json!([]), &merged).unwrap();
        assert_eq!(once.items.len(), 1);
        assert!(once.items[0].source_ref.ends_with(":merged"));
        let note = json!([{
            "id": "n1",
            "updated_at": "2026-10-16T00:00:00Z",
            "reason": "review_requested",
            "repository": {"full_name": "o/r"},
            "subject": {"title": "please look", "url": "https://api.github.com/repos/o/r/pulls/7"}
        }]);
        let with_note = github_diff(Some(&once.cursor), &note, &merged).unwrap();
        assert_eq!(with_note.items.len(), 1);
        assert!(with_note.items[0]
            .link
            .as_ref()
            .unwrap()
            .contains("/pull/7"));
        assert_eq!(
            api_to_html("https://api.github.com/repos/o/r/pulls/7").as_deref(),
            Some("https://github.com/o/r/pull/7")
        );
    }

    #[tokio::test]
    async fn parsed_items_commit_into_the_inbox() {
        let pool = crate::storage::memory().await.unwrap();
        let page = json!([{"id": "m9", "time": 1_700_000_100, "text": "ship it"}]);
        let (items, _, _) = parse_kakao_messages("room", None, "DirectChat", &page, "").unwrap();
        assert_eq!(commit_source_items(&pool, &items).await.unwrap(), 1);
        assert_eq!(commit_source_items(&pool, &items).await.unwrap(), 0);
        let source: String = sqlx::query_scalar("SELECT source FROM inbox_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(source, "kakao");
    }
}
