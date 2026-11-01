//! Owner-facing session overview. Grouping and the character cap follow omo-gateway.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::watch::{compact, short_id};

pub const DISCORD_LIMIT: usize = 1900;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionItem {
    pub id: String,
    pub header: Option<String>,
    pub question: Option<String>,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingQuestion {
    pub request_id: String,
    pub questions: Vec<QuestionItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveSession {
    pub machine: String,
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub streaming: bool,
    pub compacting: bool,
    pub blocking_question: bool,
    pub message_count: u64,
    pub last_user: String,
    pub last_assistant: String,
    pub questions: Vec<PendingQuestion>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MachineSnapshot {
    pub sessions: Vec<LiveSession>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedSession {
    pub machine: String,
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: Option<String>,
    pub updated_at: Option<String>,
    pub last_activity: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub machine: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleListing {
    pub machine: String,
    pub stale: bool,
    pub age_seconds: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionList {
    pub sessions: Vec<ListedSession>,
    pub errors: Vec<HostError>,
    pub stale: Vec<StaleListing>,
}

#[derive(Debug, Clone)]
pub struct Overview {
    pub list: SessionList,
    pub snapshots: Vec<(String, Result<MachineSnapshot, String>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Working,
    Question,
    Blocked,
    Idle,
}

pub fn state_of(session: &LiveSession) -> SessionState {
    if !session.questions.is_empty() {
        SessionState::Question
    } else if session.blocking_question {
        SessionState::Blocked
    } else if session.streaming || session.compacting {
        SessionState::Working
    } else {
        SessionState::Idle
    }
}

pub fn home_relative(folder: &str) -> String {
    for prefix in ["/Users/", "/home/"] {
        if let Some(rest) = folder.strip_prefix(prefix) {
            return match rest.split_once('/') {
                Some((_, suffix)) => format!("~/{suffix}"),
                None => "~".into(),
            };
        }
    }
    if folder == "/root" {
        return "~".into();
    }
    folder
        .strip_prefix("/root/")
        .map_or_else(|| folder.to_owned(), |rest| format!("~/{rest}"))
}

/// How long ago `value` was, with the owner-local clock time: `3분 전 (14:02 KST)` or
/// `3 min ago (05:02 UTC)`.
pub fn activity(value: Option<&str>, now: DateTime<Utc>) -> String {
    let locale = crate::locale::current();
    let ko = locale.is_korean();
    let Some(at) = value.and_then(|value| DateTime::parse_from_rfc3339(value).ok()) else {
        return locale.pick("활동 미상", "activity unknown").into();
    };
    let at = at.with_timezone(&Utc);
    let seconds = now.signed_duration_since(at).num_seconds().max(0);
    let relative = match (seconds, ko) {
        (0..=59, true) => format!("{seconds}초 전"),
        (0..=59, false) => format!("{seconds}s ago"),
        (60..=3599, true) => format!("{}분 전", seconds / 60),
        (60..=3599, false) => format!("{} min ago", seconds / 60),
        (3600..=86399, true) => format!("{}시간 전", seconds / 3600),
        (3600..=86399, false) => format!("{} h ago", seconds / 3600),
        (_, true) => format!("{}일 전", seconds / 86400),
        (_, false) => format!("{} d ago", seconds / 86400),
    };
    format!("{relative} ({})", locale.format(at, "%H:%M"))
}

fn collapse_newlines(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn question_pending() -> String {
    crate::locale::text("질문 대기 중", "question pending")
}

pub fn format_question_summary(question: &PendingQuestion) -> String {
    if let Some(first) = question.questions.first() {
        let pending = question_pending();
        let text = first
            .question
            .as_deref()
            .or(first.header.as_deref())
            .unwrap_or(pending.as_str());
        let collapsed = collapse_newlines(text);
        if first.options.is_empty() {
            compact(&collapsed, 90)
        } else {
            let opts = first
                .options
                .iter()
                .map(|opt| format!("[{}]", collapse_newlines(opt)))
                .collect::<Vec<_>>()
                .join(" ");
            compact(&format!("{collapsed} {opts}"), 90)
        }
    } else {
        question_pending()
    }
}

pub fn format_overview(overview: &Overview, now: DateTime<Utc>, max_chars: usize) -> String {
    let locale = crate::locale::current();
    let mut machines = Vec::<String>::new();
    let mut add_machine = |machine: &str| {
        if !machines.iter().any(|seen| seen == machine) {
            machines.push(machine.to_owned());
        }
    };
    for (machine, _) in &overview.snapshots {
        add_machine(machine);
    }
    for session in &overview.list.sessions {
        add_machine(&session.machine);
    }
    for error in &overview.list.errors {
        add_machine(&error.machine);
    }
    for stale in &overview.list.stale {
        add_machine(&stale.machine);
    }
    if machines.is_empty() {
        machines.push("local".into());
    }
    let snapshot_map: BTreeMap<&str, &Result<MachineSnapshot, String>> = overview
        .snapshots
        .iter()
        .map(|(machine, result)| (machine.as_str(), result))
        .collect();
    let mut lines = Vec::new();
    for machine in &machines {
        if let Some(Err(error)) = snapshot_map.get(machine.as_str()) {
            lines.push(format!(
                "⚠️ {}: {}",
                compact(machine, 32),
                compact(error, 70)
            ));
        }
        let snap_sessions = snapshot_map
            .get(machine.as_str())
            .and_then(|result| result.as_ref().ok())
            .map(|snap| &snap.sessions);
        let listing: Vec<_> = overview
            .list
            .sessions
            .iter()
            .filter(|session| session.machine == *machine)
            .collect();
        let stale_suffix = overview
            .list
            .stale
            .iter()
            .find(|item| item.machine == *machine && item.stale)
            .map(|item| {
                format!(
                    " · stale {}{}",
                    item.age_seconds,
                    locale.pick("초 전", "s ago")
                )
            })
            .unwrap_or_default();
        let mut working = Vec::new();
        let mut questions = Vec::new();
        let mut blocked = Vec::new();
        let mut idle = Vec::new();
        let mut matched = BTreeSet::new();
        if let Some(snaps) = snap_sessions {
            for (index, live) in snaps.iter().enumerate() {
                matched.insert(index);
                let matched_listing = listing.iter().find(|listed| {
                    listed.session_id == live.session_id
                        || listed.raw.get("durableSessionId").and_then(Value::as_str)
                            == Some(live.session_id.as_str())
                });
                let activity_str = activity(
                    live.updated_at.as_deref().or_else(|| {
                        matched_listing.and_then(|listed| {
                            listed
                                .updated_at
                                .as_deref()
                                .or(listed.last_activity.as_deref())
                        })
                    }),
                    now,
                );
                let id = short_id(&live.session_id);
                let name = live
                    .name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| matched_listing.and_then(|listed| listed.name.as_deref()))
                    .unwrap_or(&live.session_id);
                let cwd = home_relative(live.cwd.as_deref().unwrap_or("?"));
                match state_of(live) {
                    SessionState::Working => {
                        working.push((
                            format!(
                                "🟢 {} · {} · {} · `{}:{}`",
                                compact(name, 32),
                                compact(&cwd, 36),
                                activity_str,
                                machine,
                                id
                            ),
                            format!(
                                "  ↳ {}",
                                compact(&collapse_newlines(&live.last_assistant), 90)
                            ),
                        ));
                    }
                    SessionState::Question => {
                        let summary = live
                            .questions
                            .first()
                            .map(format_question_summary)
                            .unwrap_or_else(question_pending);
                        questions.push((
                            format!(
                                "❓ {} · {} · {} · `{}:{}`",
                                compact(name, 32),
                                compact(&cwd, 36),
                                activity_str,
                                machine,
                                id
                            ),
                            format!("  ↳ {summary}"),
                        ));
                    }
                    SessionState::Blocked => {
                        blocked.push((
                            format!(
                                "⛔ {} · {} · {} · `{}:{}`",
                                compact(name, 32),
                                compact(&cwd, 36),
                                activity_str,
                                machine,
                                id
                            ),
                            locale
                                .pick(
                                    "  ↳ 터미널에서 답변 필요 (차단형 질문)",
                                    "  ↳ answer needed in the terminal (blocking question)",
                                )
                                .into(),
                        ));
                    }
                    SessionState::Idle => {
                        idle.push(format!("⚪ {} · `{}:{}`", compact(name, 28), machine, id))
                    }
                }
            }
        }
        for listed in &listing {
            let included = snap_sessions.is_some_and(|snaps| {
                snaps.iter().any(|live| {
                    live.session_id == listed.session_id
                        || listed.raw.get("durableSessionId").and_then(Value::as_str)
                            == Some(live.session_id.as_str())
                })
            });
            if !included {
                idle.push(format!(
                    "⚪ {} · `{}:{}`",
                    compact(listed.name.as_deref().unwrap_or(&listed.session_id), 28),
                    machine,
                    short_id(&listed.session_id)
                ));
            }
        }
        let (machine_name, busy, waiting, idle_count) = (
            compact(machine, 32),
            working.len(),
            questions.len() + blocked.len(),
            idle.len(),
        );
        lines.push(if locale.is_korean() {
            format!(
                "🖥️ {machine_name} — 작업 중 {busy} · 답변 대기 {waiting} · 대기 {idle_count}{stale_suffix}"
            )
        } else {
            format!(
                "🖥️ {machine_name} — working {busy} · awaiting answer {waiting} · idle {idle_count}{stale_suffix}"
            )
        });
        for (header, sub) in working.into_iter().chain(questions).chain(blocked) {
            lines.push(header);
            lines.push(sub);
        }
        if idle.len() <= 8 {
            lines.extend(idle);
        } else {
            let overflow = idle.len() - 8;
            lines.extend(idle.into_iter().take(8));
            lines.push(if locale.is_korean() {
                format!("… 외 {overflow}개 대기")
            } else {
                format!("… {overflow} more idle")
            });
        }
    }
    for error in &overview.list.errors {
        if !snapshot_map.contains_key(error.machine.as_str()) {
            lines.push(format!(
                "⚠️ {}: {}",
                compact(&error.machine, 32),
                compact(&error.message, 70)
            ));
        }
    }
    if lines.is_empty() {
        return locale.pick("실행 중인 세션 없음", "no running sessions").into();
    }
    apply_max_chars(lines, max_chars)
}

fn apply_max_chars(lines: Vec<String>, max_chars: usize) -> String {
    let ellipsis = crate::locale::text("… (일부 생략)", "… (truncated)");
    let ellipsis = ellipsis.as_str();
    let full = lines.join("\n");
    if full.chars().count() <= max_chars {
        return full;
    }
    let mut out = Vec::new();
    let mut current = 0usize;
    let ellipsis_chars = ellipsis.chars().count() + 1;
    for line in lines {
        let line_chars = line.chars().count() + usize::from(!out.is_empty());
        if current + line_chars + ellipsis_chars > max_chars {
            break;
        }
        current += line_chars;
        out.push(line);
    }
    out.push(ellipsis.to_owned());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_groups_questions_and_keeps_the_character_cap() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let live = LiveSession {
            machine: "host".into(),
            session_id: "session-abcdefghij".into(),
            name: Some("worker".into()),
            cwd: Some("/home/sook/code".into()),
            streaming: false,
            questions: vec![PendingQuestion {
                request_id: "q".into(),
                questions: vec![QuestionItem {
                    id: "1".into(),
                    question: Some("ship it?".into()),
                    options: vec!["yes".into(), "no".into()],
                    header: None,
                }],
            }],
            updated_at: Some("2026-10-21T02:59:00Z".into()),
            ..LiveSession::default()
        };
        let overview = Overview {
            list: SessionList {
                stale: vec![StaleListing {
                    machine: "host".into(),
                    stale: true,
                    age_seconds: 12,
                }],
                errors: vec![HostError {
                    machine: "down".into(),
                    message: "timed out".into(),
                }],
                ..SessionList::default()
            },
            snapshots: vec![
                (
                    "host".into(),
                    Ok(MachineSnapshot {
                        sessions: vec![live],
                    }),
                ),
                ("down".into(), Err("timed out".into())),
            ],
        };
        let text = format_overview(&overview, now, 400);
        assert!(text.contains("답변 대기 1"), "{text}");
        assert!(text.contains("ship it?"), "{text}");
        assert!(text.contains("[yes]"), "{text}");
        assert!(text.contains("stale 12초 전"), "{text}");
        assert!(text.contains("⚠️ down"), "{text}");
        assert!(text.contains("~/code"), "{text}");
        let capped = format_overview(&overview, now, 40);
        assert!(capped.chars().count() <= 40, "{capped}");
        assert!(capped.contains("… (일부 생략)"));
        let _ = json!({});
    }

    #[test]
    fn activity_labels_carry_the_owner_zone_and_language() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(activity(Some("2026-10-21T02:57:00Z"), now), "3분 전 (11:57 KST)");
        assert_eq!(activity(None, now), "활동 미상");
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            assert_eq!(activity(Some("2026-10-21T02:57:00Z"), now), "3 min ago (02:57 UTC)");
            assert_eq!(activity(Some("2026-10-19T02:57:00Z"), now), "2 d ago (02:57 UTC)");
            assert_eq!(activity(None, now), "activity unknown");
            let empty = Overview { list: SessionList::default(), snapshots: Vec::new() };
            assert_eq!(format_overview(&empty, now, 400), "no running sessions");
            assert_eq!(
                format_question_summary(&PendingQuestion::default()),
                "question pending"
            );
            assert_eq!(
                apply_max_chars(vec!["x".repeat(30), "y".repeat(30)], 50),
                format!("{}\n… (truncated)", "x".repeat(30))
            );
        });
    }
}
