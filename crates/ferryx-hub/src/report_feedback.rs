//! Owner report feedback capture (docs/design/hub-owner-reporting.md section 3, "Capture" and
//! "Confirmation and undo").
//!
//! The owner objects to a kind of alert by replying to the delivered alert ("이거 그만 보내",
//! "이건 다시 보내") or by reacting to it with `:no_bell:`. One judge call
//! ([`Purpose::ReportFeedback`]) says whether owner text is such feedback. The objected alert
//! is found through [`delivered_alerts`](remember_delivered): the Slack message each alert was
//! posted as. Its scope becomes an [`owner_report_rules`](crate::report_rules) row, and the bot
//! confirms once in the owner's language. A `once:` / `이번만:` objection only dismisses that
//! alert. Feedback that points at no alert lists the recent alerts and asks once; it stores
//! nothing.
use chrono::{DateTime, Duration, Utc};
use sqlx::SqlitePool;

use crate::chat::fence::fence_untrusted;
use crate::error::Result;
use crate::judge::{Judge, Purpose};
use crate::report_rules::{self, Effect};
use crate::storage::now_rfc3339;

/// The Slack reaction that mutes the kind of alert it is put on.
pub const MUTE_REACTION: &str = "no_bell";
/// Recent alerts listed when feedback points at no alert.
const ASK_MAX_ALERTS: i64 = 5;
const ASK_WINDOW_HOURS: i64 = 24;

pub const FEEDBACK_OPTIONS: &[&str] = &["mute_kind", "send_kind", "not_feedback"];
pub const FEEDBACK_INSTRUCTIONS: &str = "The owner wrote the fenced message, in any language, to their assistant bot. Decide whether it is feedback about the alerts and notifications the bot sends them. mute_kind = they do not want this kind of alert any more, e.g. \"stop sending these\", \"I don't need this\", \"don't send useless alerts\", \"그만 보내\", \"이건 이제 보내지 마\", \"필요없어\"; send_kind = they want a kind of alert sent again or always, e.g. \"send these again\", \"always tell me about this\", \"이건 다시 보내\", \"이런 건 꼭 알려 줘\"; not_feedback = anything else: a question, a task, an answer to the alert's content, or an objection to the bot's other work rather than to an alert. When unsure, not_feedback.";

/// What owner text says about alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    MuteKind,
    SendKind,
    NotFeedback,
}

impl Feedback {
    /// A judge answer. The capped and conservative answer of [`Purpose::ReportFeedback`]
    /// (`none`) and anything unknown are not feedback.
    pub fn parse(value: &str) -> Self {
        match value {
            "mute_kind" => Self::MuteKind,
            "send_kind" => Self::SendKind,
            _ => Self::NotFeedback,
        }
    }

    fn effect(self) -> Option<Effect> {
        match self {
            Self::MuteKind => Some(Effect::Mute),
            Self::SendKind => Some(Effect::Send),
            Self::NotFeedback => None,
        }
    }
}

/// One alert as it was delivered to the owner. `sender` and `subject` are what triage matches
/// a rule against (`inbox_items.author`, and `title` or else `snippet`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertRef {
    pub source: String,
    pub sender: Option<String>,
    pub subject: Option<String>,
    pub topic_key: Option<String>,
    /// The inbox item id (or other report id) of the alert.
    pub alert_ref: String,
}

impl AlertRef {
    fn scope(&self) -> report_rules::Scope {
        let mut scope = report_rules::scope_from_alert(
            &self.source,
            self.sender.as_deref(),
            self.subject.as_deref(),
            self.topic_key.as_deref(),
        );
        if let Some(topic) = scope.topic_key.as_deref() {
            if topic.starts_with("inbox:") {
                // Inbox topics identify individual messages, so match their kind instead.
                if scope.sender_key.is_some() || scope.pattern.is_some() {
                    scope.topic_key = None;
                }
            } else {
                // A report topic already identifies the subject of the objection. Its
                // next state may have entirely different wording without undoing a mute.
                scope.pattern = None;
            }
        }
        scope
    }

    fn line(&self) -> String {
        use crate::owner_alert::{one_line, source_label};
        let unknown = crate::locale::text("알 수 없음", "unknown");
        let who = one_line(self.sender.as_deref().unwrap_or(&unknown), 60);
        let what = one_line(self.subject.as_deref().unwrap_or(""), 90);
        format!("[{}] {who} — {what}", source_label(&self.source))
    }
}

/// One judge call: is this owner text feedback about alerts? `alert` is the alert it replies
/// to, shown as context. A capped, fallback or failed judge answers [`Feedback::NotFeedback`],
/// so a judge outage never turns an owner message into a rule.
pub async fn classify(judge: &Judge, text: &str, alert: Option<&AlertRef>) -> Result<Feedback> {
    let mut state = fence_untrusted("owner slack message", text);
    if let Some(alert) = alert {
        state.push_str("\n\nThe message replies to this delivered alert:\n");
        state.push_str(&fence_untrusted("delivered alert", &alert.line()));
    }
    let choice = match judge
        .choice(
            Purpose::ReportFeedback,
            &crate::judge::scrub_for_external(&state),
            FEEDBACK_INSTRUCTIONS,
            FEEDBACK_OPTIONS,
        )
        .await
    {
        Ok(choice) => choice,
        Err(error) => {
            // The judge's conservative ReportFeedback answer is `none`, which is not an option,
            // so an unavailable judge is an error here; not feedback is the conservative reading.
            tracing::warn!(%error, "report feedback was not judged; treated as not feedback");
            return Ok(Feedback::NotFeedback);
        }
    };
    if choice.is_capped || choice.is_fallback {
        return Ok(Feedback::NotFeedback);
    }
    Ok(Feedback::parse(choice.as_str()))
}

/// Remembers that `alerts` were delivered as the Slack message `slack_ts` in `slack_channel`.
/// Idempotent per (channel, ts, alert). Returns how many rows were new.
pub async fn remember_delivered(
    pool: &SqlitePool,
    slack_channel: &str,
    slack_ts: &str,
    alerts: &[AlertRef],
) -> Result<u64> {
    let now = now_rfc3339();
    let mut stored = 0;
    for alert in alerts {
        stored += sqlx::query(
            "INSERT OR IGNORE INTO delivered_alerts(slack_channel, slack_ts, source, sender, subject,
                 topic_key, alert_ref, created_at)
             VALUES(?,?,?,?,?,?,?,?)",
        )
        .bind(slack_channel)
        .bind(slack_ts)
        .bind(&alert.source)
        .bind(&alert.sender)
        .bind(&alert.subject)
        .bind(&alert.topic_key)
        .bind(&alert.alert_ref)
        .bind(&now)
        .execute(pool)
        .await?
        .rows_affected();
    }
    Ok(stored)
}

/// The alert of one inbox item, with the sender and subject inbox triage matches rules on.
pub async fn alert_from_inbox_item(pool: &SqlitePool, item_id: &str) -> Result<Option<AlertRef>> {
    let row: Option<(String, Option<String>, Option<String>, String)> =
        sqlx::query_as("SELECT source, author, title, snippet FROM inbox_items WHERE id = ?")
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(source, author, title, snippet)| AlertRef {
        source,
        sender: author,
        subject: Some(title.filter(|title| !title.trim().is_empty()).unwrap_or(snippet)),
        topic_key: None,
        alert_ref: item_id.to_owned(),
    }))
}

/// [`remember_delivered`] for owner alerts that are inbox items (`owner_alert::sweep`).
/// Unknown item ids are skipped.
pub async fn remember_delivered_items(
    pool: &SqlitePool,
    slack_channel: &str,
    slack_ts: &str,
    item_ids: &[&str],
) -> Result<u64> {
    let mut alerts = Vec::with_capacity(item_ids.len());
    for id in item_ids {
        if let Some(alert) = alert_from_inbox_item(pool, id).await? {
            alerts.push(alert);
        }
    }
    remember_delivered(pool, slack_channel, slack_ts, &alerts).await
}

/// `delivered_alerts.slack_channel` of an alert whose outbox row is not posted yet; its
/// `slack_ts` is the outbox row id.
const OUTBOX_CHANNEL: &str = "outbox";

/// Remembers the alerts an outbox row carries before it is posted, so the post (now or by the
/// outbox retry) can name them with [`posted_from_outbox`].
pub async fn queue_for_outbox(pool: &SqlitePool, outbox_id: i64, alerts: &[AlertRef]) -> Result<u64> {
    remember_delivered(pool, OUTBOX_CHANNEL, &outbox_id.to_string(), alerts).await
}

/// The outbox row `outbox_id` was posted as `posted`: its queued alerts are now remembered by
/// the Slack message. Returns how many alerts were bound; 0 when the row carried no alert or
/// the platform returned no ts.
pub async fn posted_from_outbox(
    pool: &SqlitePool,
    outbox_id: i64,
    posted: &crate::chat::reply::PostedMessage,
) -> Result<u64> {
    let Some(ts) = posted.ts.as_deref() else {
        return Ok(0);
    };
    let queued = alerts_for_thread(pool, OUTBOX_CHANNEL, &outbox_id.to_string()).await?;
    if queued.is_empty() {
        return Ok(0);
    }
    let stored = remember_delivered(pool, &posted.channel, ts, &queued).await?;
    sqlx::query("DELETE FROM delivered_alerts WHERE slack_channel = ? AND slack_ts = ?")
        .bind(OUTBOX_CHANNEL)
        .bind(outbox_id.to_string())
        .execute(pool)
        .await?;
    Ok(stored)
}

type AlertRow = (String, Option<String>, Option<String>, Option<String>, String);

fn alert_from_row((source, sender, subject, topic_key, alert_ref): AlertRow) -> AlertRef {
    AlertRef {
        source,
        sender,
        subject,
        topic_key,
        alert_ref,
    }
}

/// Every alert delivered as the Slack message `thread_ts` in `channel`.
pub async fn alerts_for_thread(
    pool: &SqlitePool,
    channel: &str,
    thread_ts: &str,
) -> Result<Vec<AlertRef>> {
    let rows: Vec<AlertRow> = sqlx::query_as(
        "SELECT source, sender, subject, topic_key, alert_ref FROM delivered_alerts
         WHERE slack_channel = ? AND slack_ts = ? ORDER BY id",
    )
    .bind(channel)
    .bind(thread_ts)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

/// The one alert a Slack thread or reply points at. A message that carried several alerts
/// names none of them.
pub async fn alert_for_thread(
    pool: &SqlitePool,
    channel: &str,
    thread_ts: &str,
) -> Result<Option<AlertRef>> {
    let mut alerts = alerts_for_thread(pool, channel, thread_ts).await?;
    Ok(if alerts.len() == 1 { alerts.pop() } else { None })
}

/// Alerts delivered since `since`, newest first, one per (source, sender, subject).
pub async fn recent_alerts(
    pool: &SqlitePool,
    since: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<AlertRef>> {
    let rows: Vec<AlertRow> = sqlx::query_as(
        "SELECT source, sender, subject, topic_key, alert_ref FROM delivered_alerts
         WHERE id IN (
             SELECT MAX(id) FROM delivered_alerts
             WHERE julianday(created_at) >= julianday(?) AND slack_channel != ?
             GROUP BY source, sender, subject)
         ORDER BY id DESC LIMIT ?",
    )
    .bind(since.to_rfc3339())
    .bind(OUTBOX_CHANNEL)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(alert_from_row).collect())
}

/// Project of the report-domain lessons (docs/design/hub-owner-reporting.md section 8).
pub const LESSON_PROJECT: &str = "owner-reporting";

/// The lesson subject of a delivered alert: `report:<topic_key>`; an inbox item's topic is
/// `inbox:<item id>`, as the report gate keys it.
pub fn lesson_subject(alert: &AlertRef) -> String {
    match alert.topic_key.as_deref().filter(|key| !key.trim().is_empty()) {
        Some(topic) => format!("report:{topic}"),
        None => format!("report:inbox:{}", alert.alert_ref),
    }
}

/// A mute is the owner's objection to a delivered report: the lessons injected into it hurt,
/// and the judge is asked once whether the objection teaches a reporting lesson (evidence
/// `rule:<rule_id>`, which resolves while the rule exists). A lesson failure never blocks the
/// rule or its confirmation.
async fn learn_from_mute(
    pool: &SqlitePool,
    judge: &Judge,
    alert: &AlertRef,
    rule: &report_rules::Rule,
) {
    use crate::supervisor::lessons;
    if let Err(error) = lessons::settle(pool, &lesson_subject(alert), lessons::Outcome::Hurt).await {
        tracing::warn!(%error, "report lessons were not settled hurt");
    }
    let evidence = format!("rule:{}", rule.id);
    let resolves = |reference: &str| reference == evidence;
    if let Err(error) = lessons::reflect(
        pool,
        judge,
        lessons::LessonDomain::Report,
        LESSON_PROJECT,
        &if crate::locale::current().is_korean() {
            format!("대표가 {}을 원하지 않았습니다.", report_rules::describe_ko(rule))
        } else {
            format!("The owner did not want {}.", report_rules::describe_ko(rule))
        },
        &evidence,
        resolves,
    )
    .await
    {
        tracing::warn!(%error, "report lesson was not reflected");
    }
}

/// Stores the rule for `alert` and returns the confirmation in the owner's language. Task-only
/// text never reaches here.
async fn apply_rule(
    pool: &SqlitePool,
    judge: &Judge,
    alert: &AlertRef,
    effect: Effect,
    owner_words: &str,
) -> Result<Option<String>> {
    let rule = report_rules::record(
        pool,
        &alert.scope(),
        effect,
        owner_words,
        Some(&alert.alert_ref),
    )
    .await?;
    if effect == Effect::Mute {
        learn_from_mute(pool, judge, alert, &rule).await;
    }
    let closed = report_rules::superseded(pool, &rule.id).await?;
    Ok(Some(report_rules::confirmation_ko(&rule, closed.first())))
}

/// The Korean reply to a one-time objection; [`once_reply`] picks the owner's language.
pub const ONCE_REPLY: &str = "이번 알림만 넘깁니다.";

/// The reply to a one-time objection in the owner's language.
pub fn once_reply() -> String {
    crate::locale::text(ONCE_REPLY, "I'll skip just this alert.")
}

/// A one-time objection: the objected alerts leave the open inbox, and no rule is stored.
async fn dismiss_once(pool: &SqlitePool, alerts: &[AlertRef]) -> Result<()> {
    for alert in alerts {
        sqlx::query(
            "UPDATE inbox_decisions SET status = 'dismissed' WHERE item_id = ? AND status = 'open'",
        )
        .bind(&alert.alert_ref)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// The one question asked when feedback points at no single alert. Stores nothing.
fn ask_which(feedback: Feedback, candidates: &[AlertRef]) -> String {
    if !crate::locale::current().is_korean() {
        return ask_which_english(feedback, candidates);
    }
    let (question, words) = match feedback {
        Feedback::SendKind => ("어떤 알림을 다시 보내 드릴까요?", "이건 다시 보내"),
        Feedback::MuteKind | Feedback::NotFeedback => ("어떤 알림을 그만 보낼까요?", "이거 그만 보내"),
    };
    if candidates.is_empty() {
        return format!(
            "{question} 최근 24시간 동안 보낸 알림이 없습니다. 해당 알림이 오면 그 메시지에 '{words}'라고 답장해 주세요."
        );
    }
    let mut text = format!("{question} 최근에 보낸 알림입니다.");
    for (n, alert) in candidates.iter().enumerate() {
        text.push_str(&format!("\n{}. {}", n + 1, alert.line()));
    }
    // The reaction only mutes.
    let how = match feedback {
        Feedback::SendKind => "답장해 주세요.".to_owned(),
        Feedback::MuteKind | Feedback::NotFeedback => {
            format!("답장하시거나 :{MUTE_REACTION}: 반응을 달아 주세요.")
        }
    };
    text.push_str(&format!("\n해당 알림 메시지에 '{words}'라고 {how}"));
    text
}

/// [`ask_which`] for an owner who does not read Korean.
fn ask_which_english(feedback: Feedback, candidates: &[AlertRef]) -> String {
    let (question, words) = match feedback {
        Feedback::SendKind => ("Which alerts should I send again?", "send these again"),
        Feedback::MuteKind | Feedback::NotFeedback => ("Which alerts should I stop sending?", "stop sending these"),
    };
    if candidates.is_empty() {
        return format!(
            "{question} I haven't sent any alerts in the last 24 hours. When one arrives, reply '{words}' to that message."
        );
    }
    let mut text = format!("{question} These are the alerts I sent recently:");
    for (n, alert) in candidates.iter().enumerate() {
        text.push_str(&format!("\n{}. {}", n + 1, alert.line()));
    }
    // The reaction only mutes.
    let how = match feedback {
        Feedback::SendKind => String::new(),
        Feedback::MuteKind | Feedback::NotFeedback => format!(", or react with :{MUTE_REACTION}:"),
    };
    text.push_str(&format!("\nReply '{words}' to the alert's message{how}."));
    text
}

/// The owner's message, before any agent turn. Returns the reply, in the owner's language, to
/// post in the same thread when the message is alert feedback (the bot handles it fully; no
/// agent turn), or `None` when it is not feedback.
///
/// `thread_ts` is the Slack thread the message was written in, which is the delivered alert's
/// own ts when the owner replied to it.
pub async fn handle_owner_message(
    pool: &SqlitePool,
    judge: &Judge,
    channel: &str,
    thread_ts: Option<&str>,
    text: &str,
) -> Result<Option<String>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let alerts = match thread_ts {
        Some(ts) => alerts_for_thread(pool, channel, ts).await?,
        None => Vec::new(),
    };
    let task_only = report_rules::is_task_only(text);
    let judged = if task_only {
        crate::supervisor::memory::split_task_only(text).1.join("\n")
    } else {
        text.to_owned()
    };
    let single = (alerts.len() == 1).then(|| alerts[0].clone());
    let feedback = classify(judge, &judged, single.as_ref()).await?;
    let Some(effect) = feedback.effect() else {
        return Ok(None);
    };
    if task_only {
        // A one-time objection never becomes a rule; a one-time "send" changes nothing.
        if effect != Effect::Mute || alerts.is_empty() {
            return Ok(None);
        }
        dismiss_once(pool, &alerts).await?;
        return Ok(Some(once_reply()));
    }
    match single {
        Some(alert) => apply_rule(pool, judge, &alert, effect, text).await,
        None => {
            let candidates: Vec<AlertRef> = if alerts.len() > 1 {
                alerts
                    .into_iter()
                    .take(ASK_MAX_ALERTS as usize)
                    .collect()
            } else {
                recent_alerts(
                    pool,
                    Utc::now() - Duration::hours(ASK_WINDOW_HOURS),
                    ASK_MAX_ALERTS,
                )
                .await?
            };
            Ok(Some(ask_which(feedback, &candidates)))
        }
    }
}

/// An owner reaction on a Slack message. `:no_bell:` on a delivered alert mutes that kind of
/// alert with no feedback judge call (`judge` only reflects the report lesson). Returns the confirmation, in the owner's language, to post in the alert's thread,
/// or `None` when the reaction is another one or the message is not one alert.
pub async fn handle_reaction(
    pool: &SqlitePool,
    judge: &Judge,
    channel: &str,
    message_ts: &str,
    reaction: &str,
) -> Result<Option<String>> {
    // Slack sends skin-tone variants as `name::skin-tone-N`.
    if reaction.split("::").next() != Some(MUTE_REACTION) {
        return Ok(None);
    }
    let Some(alert) = alert_for_thread(pool, channel, message_ts).await? else {
        return Ok(None);
    };
    apply_rule(pool, judge, &alert, Effect::Mute, &format!(":{MUTE_REACTION}:")).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{Class, classify_pending};

    const DM: &str = "D0OWNER";
    const GITLAB: &str = "GitLab <gitlab@mg.gitlab.com>";
    const GITLAB_SIGN_IN: &str = "gitlab.com sign-in from new location";
    const GOOGLE: &str = "Google <no-reply@accounts.google.com>";

    async fn mail(pool: &SqlitePool, id: &str, sender: &str, subject: &str, at: DateTime<Utc>) {
        sqlx::query(
            "INSERT INTO inbox_items(id, source, source_ref, author, title, snippet, received_at, raw_hash, collected_at)
             VALUES(?, 'mail', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(id)
        .bind(sender)
        .bind(subject)
        .bind(subject)
        .bind(at.to_rfc3339())
        .bind(id)
        .bind(at.to_rfc3339())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn decision(pool: &SqlitePool, id: &str) -> (String, String, String) {
        sqlx::query_as("SELECT class, reason, status FROM inbox_decisions WHERE item_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn triage(pool: &SqlitePool, judge: &Judge) -> usize {
        classify_pending(pool, judge, Utc::now(), "meta", |_id, _class| async move {
            Ok(None)
        })
        .await
        .unwrap()
    }

    /// A GitLab sign-in alert delivered to the owner DM as Slack message `100.1`.
    async fn delivered_gitlab(pool: &SqlitePool) {
        mail(
            pool,
            "r2",
            GITLAB,
            GITLAB_SIGN_IN,
            Utc::now() - Duration::minutes(30),
        )
        .await;
        assert_eq!(
            remember_delivered_items(pool, DM, "100.1", &["r2"])
                .await
                .unwrap(),
            1
        );
    }

    async fn rule_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM owner_report_rules")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn owner_reply_to_alert_stores_a_mute_rule_and_confirms_in_korean() {
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        let alert = alert_for_thread(&pool, DM, "100.1").await.unwrap().unwrap();
        assert_eq!(alert.alert_ref, "r2");
        let judge = Judge::forced("mute_kind");
        let reply = handle_owner_message(&pool, &judge, DM, Some("100.1"), "이거 그만 보내")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reply,
            "앞으로 gitlab@mg.gitlab.com의 'gitlab.com sign-in from new location' 메일 알림은 보내지 않습니다. 다시 받으시려면 '이건 다시 보내'라고 해 주세요."
        );
        let rules = report_rules::active_rules(&pool).await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].effect, Effect::Mute);
        assert_eq!(rules[0].owner_words, "이거 그만 보내");
        assert_eq!(rules[0].origin_alert_ref.as_deref(), Some("r2"));
        assert_eq!(
            rules[0].scope.sender_key.as_deref(),
            Some("gitlab@mg.gitlab.com")
        );
    }

    #[tokio::test]
    async fn topic_feedback_survives_changed_wording_without_muting_other_topics() {
        use crate::report_gate::{queue, Decision, Report, Salience};
        let pool = crate::storage::memory().await.unwrap();
        let first = Report::new("first", "producer", "task:alpha", Salience::Result,
            "요청한 초안이 준비됐습니다.").with_state("draft");
        let (_, id) = queue(&pool, "owner", &first, false).await.unwrap();
        posted_from_outbox(&pool, id.unwrap(), &crate::chat::reply::PostedMessage {
            channel: DM.into(), ts: Some("100.1".into()),
        }).await.unwrap();
        handle_owner_message(&pool, &Judge::forced("mute_kind"), DM, Some("100.1"), "이거 그만 보내")
            .await.unwrap().unwrap();

        let changed = Report::new("changed", "producer", "task:alpha", Salience::Result,
            "검토를 마친 문서가 승인됐습니다.").with_state("reviewed");
        assert_eq!(queue(&pool, "owner", &changed, false).await.unwrap().0, Decision::Muted);
        let other = Report::new("other", "producer", "task:beta", Salience::Result,
            &changed.body).with_state("reviewed");
        assert_eq!(queue(&pool, "owner", &other, false).await.unwrap().0, Decision::Sent);

        handle_owner_message(&pool, &Judge::forced("send_kind"), DM, Some("100.1"), "이건 다시 보내")
            .await.unwrap().unwrap();
        assert_eq!(queue(&pool, "owner", &changed, false).await.unwrap().0, Decision::Sent);
    }

    #[tokio::test]
    async fn send_kind_reply_restores() {
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        let mute = handle_owner_message(
            &pool,
            &Judge::forced("mute_kind"),
            DM,
            Some("100.1"),
            "그만 보내",
        )
        .await
        .unwrap();
        assert!(mute.is_some());
        let reply = handle_owner_message(
            &pool,
            &Judge::forced("send_kind"),
            DM,
            Some("100.1"),
            "이건 다시 보내",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(reply.contains("바로 보내 드립니다"), "{reply}");
        assert!(reply.contains("이전 규칙"), "{reply}");
        let rules = report_rules::active_rules(&pool).await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].effect, Effect::Send);
        assert_eq!(rule_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn not_feedback_passes_through() {
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        let judge = Judge::forced("not_feedback");
        let reply = handle_owner_message(&pool, &judge, DM, Some("100.1"), "이거 누가 로그인한 거야?")
            .await
            .unwrap();
        assert!(reply.is_none());
        // A judge that cannot answer is not feedback either.
        let broken = Judge::forced("not_an_option");
        let reply = handle_owner_message(&pool, &broken, DM, None, "그만 보내")
            .await
            .unwrap();
        assert!(reply.is_none());
        assert_eq!(Feedback::parse("none"), Feedback::NotFeedback);
        assert_eq!(rule_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn once_reply_stores_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        crate::inbox::decide(&pool, "r2", Class::Owner, "owner_must_act", None)
            .await
            .unwrap();
        let judge = Judge::forced("mute_kind");
        let reply = handle_owner_message(&pool, &judge, DM, Some("100.1"), "이번만: 그만 보내")
            .await
            .unwrap();
        assert_eq!(reply.as_deref(), Some(ONCE_REPLY));
        assert_eq!(rule_count(&pool).await, 0);
        assert_eq!(decision(&pool, "r2").await.2, "dismissed");
    }

    #[tokio::test]
    async fn feedback_without_alert_asks_once_and_stores_nothing() {
        let pool = crate::storage::memory().await.unwrap();
        let judge = Judge::forced("mute_kind");
        let empty = handle_owner_message(&pool, &judge, DM, None, "쓸데없는 메일 알림 하지마")
            .await
            .unwrap()
            .unwrap();
        assert!(empty.starts_with("어떤 알림을 그만 보낼까요? 최근 24시간 동안 보낸 알림이 없습니다."));
        delivered_gitlab(&pool).await;
        mail(&pool, "g1", GOOGLE, "Security alert", Utc::now()).await;
        remember_delivered_items(&pool, DM, "101.1", &["g1"])
            .await
            .unwrap();
        // A reply in a thread that is not an alert is the same as no thread.
        let reply = handle_owner_message(&pool, &judge, DM, Some("999.9"), "쓸데없는 메일 알림 하지마")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reply,
            "어떤 알림을 그만 보낼까요? 최근에 보낸 알림입니다.\n\
             1. [메일] Google <no-reply@accounts.google.com> — Security alert\n\
             2. [메일] GitLab <gitlab@mg.gitlab.com> — gitlab.com sign-in from new location\n\
             해당 알림 메시지에 '이거 그만 보내'라고 답장하시거나 :no_bell: 반응을 달아 주세요."
        );
        assert_eq!(rule_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn an_english_owner_gets_feedback_replies_in_english() {
        use crate::locale::{scope, OwnerLocale};
        scope(OwnerLocale::english_utc(), async {
            let pool = crate::storage::memory().await.unwrap();
            let judge = Judge::forced("mute_kind");
            let empty = handle_owner_message(&pool, &judge, DM, None, "stop sending useless mail alerts")
                .await.unwrap().unwrap();
            assert_eq!(empty, "Which alerts should I stop sending? I haven't sent any alerts in the last 24 hours. \
                When one arrives, reply 'stop sending these' to that message.");
            delivered_gitlab(&pool).await;
            let once = handle_owner_message(&pool, &judge, DM, Some("100.1"), "once: stop sending these")
                .await.unwrap();
            assert_eq!(once.as_deref(), Some("I'll skip just this alert."));
            let confirmed = handle_owner_message(&pool, &judge, DM, Some("100.1"), "stop sending these")
                .await.unwrap().unwrap();
            assert!(confirmed.starts_with("I won't send mail alerts from gitlab@mg.gitlab.com"), "{confirmed}");
            assert_eq!(rule_count(&pool).await, 1);
        }).await;
    }

    #[tokio::test]
    async fn a_no_bell_reaction_mutes_the_alert_kind() {
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        assert!(handle_reaction(&pool, &Judge::forced("no_lesson"), DM, "100.1", "eyes")
            .await
            .unwrap()
            .is_none());
        assert!(handle_reaction(&pool, &Judge::forced("no_lesson"), DM, "555.5", MUTE_REACTION)
            .await
            .unwrap()
            .is_none());
        let reply = handle_reaction(&pool, &Judge::forced("no_lesson"), DM, "100.1", "no_bell::skin-tone-2")
            .await
            .unwrap()
            .unwrap();
        assert!(reply.contains("보내지 않습니다"), "{reply}");
        let rules = report_rules::active_rules(&pool).await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].owner_words, ":no_bell:");
    }

    #[tokio::test]
    async fn lessons_report_hurt_on_mute() {
        use crate::supervisor::lessons::{self, LessonDomain, LessonDraft};
        let pool = crate::storage::memory().await.unwrap();
        delivered_gitlab(&pool).await;
        let lesson = lessons::record(
            &pool,
            LessonDraft {
                domain: LessonDomain::Report,
                project: LESSON_PROJECT,
                rule: "Security notices from GitLab are owner items.",
                rationale: "seed",
                keywords: "",
                evidence_ref: "rule:seed",
            },
            |_| true,
        )
        .await
        .unwrap()
        .unwrap();
        // Triage injected the lesson into the alert's report.
        lessons::note_injected(&pool, "report:inbox:r2", std::slice::from_ref(&lesson.id))
            .await
            .unwrap();
        // A :no_bell: mute needs no feedback judge, so the judge here is the reflection's.
        let reply = handle_reaction(&pool, &Judge::forced("lesson"), DM, "100.1", MUTE_REACTION)
            .await
            .unwrap()
            .unwrap();
        assert!(reply.contains("보내지 않습니다"), "{reply}");
        let hurt = lessons::get(&pool, &lesson.id).await.unwrap().unwrap().hurt;
        assert_eq!(hurt, 1);
        let outcome: String = sqlx::query_scalar(
            "SELECT outcome FROM lesson_injections WHERE subject_ref = 'report:inbox:r2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(outcome, "hurt");
        let rule = report_rules::active_rules(&pool).await.unwrap().remove(0);
        let reflected: Vec<(String, String)> = sqlx::query_as(
            "SELECT project, evidence_ref FROM supervisor_lessons WHERE domain = 'report' AND id != ?",
        )
        .bind(&lesson.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            reflected,
            vec![(LESSON_PROJECT.to_owned(), format!("rule:{}", rule.id))]
        );
    }

    #[tokio::test]
    async fn real_outbox_feedback_applies_to_new_items_and_can_be_restored() {
        use crate::owner_alert::{alert_report, PendingAlert};
        use crate::report_gate::{queue, Decision};
        for source in ["mail", "kakao", "imessage"] {
            let pool = crate::storage::memory().await.unwrap();
            let alert = PendingAlert {
                item_id: "first".into(), source: source.into(),
                author: Some("Service <Notify@Example.com>".into()),
                title: Some("Your code 483920 for sign-in".into()),
                snippet: "Your code 483920 for sign-in".into(), link: None,
            };
            let report = alert_report(&alert);
            let (decision, id) = queue(&pool, "owner", &report, false).await.unwrap();
            assert_eq!(decision, Decision::Sent);
            posted_from_outbox(&pool, id.unwrap(), &crate::chat::reply::PostedMessage {
                channel: DM.into(), ts: Some("100.1".into()),
            }).await.unwrap();
            handle_owner_message(&pool, &Judge::forced("mute_kind"), DM, Some("100.1"), "이거 그만 보내")
                .await.unwrap().unwrap();
            let rules = report_rules::active_rules(&pool).await.unwrap();
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].scope.topic_key, None);
            let next = PendingAlert { item_id: "second".into(),
                title: Some("Your code 791362 for sign-in".into()), ..alert.clone() };
            let next_report = alert_report(&next);
            assert!(report_rules::matching_rule(&pool, source, next.author.as_deref(),
                next.title.as_deref(), Some(&next_report.topic_key)).await.unwrap().is_some());
            assert_eq!(queue(&pool, "owner", &next_report, false).await.unwrap().0, Decision::Muted);
            let other = PendingAlert { item_id: "other".into(),
                title: Some("Payment approval required".into()), ..alert.clone() };
            assert_eq!(queue(&pool, "owner", &alert_report(&other), false).await.unwrap().0, Decision::Sent);
            handle_owner_message(&pool, &Judge::forced("send_kind"), DM, Some("100.1"), "이건 다시 보내")
                .await.unwrap().unwrap();
            assert_eq!(queue(&pool, "owner", &next_report, false).await.unwrap().0, Decision::Sent);
        }
    }

    #[tokio::test]
    async fn incident_2026_10_30_feedback_loop() {
        let pool = crate::storage::memory().await.unwrap();
        // The GitLab sign-in alert was delivered to the owner as Slack message 100.1.
        delivered_gitlab(&pool).await;
        // The owner replies in its thread.
        let reply = handle_owner_message(
            &pool,
            &Judge::forced("mute_kind"),
            DM,
            Some("100.1"),
            "이거 그만 보내",
        )
        .await
        .unwrap()
        .unwrap();
        assert!(reply.starts_with("앞으로 gitlab@mg.gitlab.com"), "{reply}");
        let rule = report_rules::active_rules(&pool).await.unwrap().remove(0);
        // r2 itself was never triaged in this test; settle it so only new mail is pending.
        crate::inbox::decide(&pool, "r2", Class::Owner, "owner_must_act", None)
            .await
            .unwrap();

        // The next GitLab new-location mail: a judge that errors when asked proves the rule
        // decided it with no model call.
        mail(
            &pool,
            "n1",
            "GitLab <gitlab@mg.gitlab.com>",
            "gitlab.com sign-in from new location",
            Utc::now() - Duration::minutes(5),
        )
        .await;
        assert_eq!(triage(&pool, &Judge::forced("not_an_option")).await, 1);
        let (class, reason, _) = decision(&pool, "n1").await;
        assert_eq!(class, Class::Ignore.as_str());
        assert_eq!(reason, format!("muted:{}", rule.id));

        // The Google security alert is still delivered as the judge says.
        mail(
            &pool,
            "n2",
            GOOGLE,
            "Security alert",
            Utc::now() - Duration::minutes(4),
        )
        .await;
        assert_eq!(triage(&pool, &Judge::forced("owner_must_act")).await, 1);
        let (class, reason, _) = decision(&pool, "n2").await;
        assert_eq!(class, Class::Owner.as_str());
        assert_eq!(reason, "owner_must_act");
    }
}
