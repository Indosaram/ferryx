//! Owner report rules: durable owner objections to a kind of report (docs/design/hub-owner-reporting.md section 3).
//!
//! The default is to send: with no rule nothing changes. A `mute` rule decides an alert before
//! the judge with no model call; `digest` and `send` override the judge's class afterwards. A
//! newer rule closes every active overlapping rule of the same source (`superseded_by`,
//! `closed_at`); rows are never deleted.
use sqlx::SqlitePool;

use crate::error::{HubError, Result};
use crate::storage::{WriteTx, now_rfc3339};

/// Rendered example alerts are cut to this many characters.
const EXAMPLE_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Never send.
    Mute,
    /// Daily digest only.
    Digest,
    /// Always send immediately.
    Send,
}

impl Effect {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "mute" => Ok(Self::Mute),
            "digest" => Ok(Self::Digest),
            "send" => Ok(Self::Send),
            other => Err(HubError::message(format!(
                "unknown owner report rule effect {other}"
            ))),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mute => "mute",
            Self::Digest => "digest",
            Self::Send => "send",
        }
    }

    /// The inbox triage option this effect stands for.
    pub const fn triage_option(self) -> &'static str {
        match self {
            Self::Mute => "ignore",
            Self::Digest => "fyi",
            Self::Send => "owner_must_act",
        }
    }
}

/// What a rule applies to. `None` fields are wildcards; `source` is always set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub source: String,
    pub sender_key: Option<String>,
    pub topic_key: Option<String>,
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub scope: Scope,
    pub effect: Effect,
    pub owner_words: String,
    pub origin_alert_ref: Option<String>,
    pub mhc_receipt: Option<String>,
    pub created_at: String,
    pub superseded_by: Option<String>,
    pub closed_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct RuleRow {
    id: String,
    source: String,
    sender_key: Option<String>,
    topic_key: Option<String>,
    pattern: Option<String>,
    effect: String,
    owner_words: String,
    origin_alert_ref: Option<String>,
    mhc_receipt: Option<String>,
    created_at: String,
    superseded_by: Option<String>,
    closed_at: Option<String>,
}

impl TryFrom<RuleRow> for Rule {
    type Error = HubError;

    fn try_from(row: RuleRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            scope: Scope {
                source: row.source,
                sender_key: row.sender_key,
                topic_key: row.topic_key,
                pattern: row.pattern,
            },
            effect: Effect::parse(&row.effect)?,
            owner_words: row.owner_words,
            origin_alert_ref: row.origin_alert_ref,
            mhc_receipt: row.mhc_receipt,
            created_at: row.created_at,
            superseded_by: row.superseded_by,
            closed_at: row.closed_at,
        })
    }
}

const RULE_COLUMNS: &str = "id, source, sender_key, topic_key, pattern, effect, owner_words, \
     origin_alert_ref, mhc_receipt, created_at, superseded_by, closed_at";

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// A sender address, domain or room id, lowercased; `Name <a@b>` keeps only the address.
pub fn normalize_sender(sender: &str) -> Option<String> {
    let trimmed = sender.trim();
    let inner = match (trimmed.rfind('<'), trimmed.rfind('>')) {
        (Some(open), Some(close)) if open < close => &trimmed[open + 1..close],
        _ => trimmed,
    };
    non_empty(Some(inner.to_lowercase().as_str()))
}

fn is_id_token(token: &str) -> bool {
    let compact: String = token.chars().filter(|c| *c != '-').collect();
    compact.len() >= 8
        && compact.chars().all(|c| c.is_ascii_hexdigit())
        && compact.chars().any(|c| c.is_ascii_digit())
}

fn is_url_token(token: &str) -> bool {
    token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.")
}

/// A subject stem: lowercase, with urls, hex ids and digits removed and whitespace collapsed.
pub fn subject_stem(subject: &str) -> Option<String> {
    let lower = subject.to_lowercase();
    let words: Vec<String> = lower
        .split_whitespace()
        .filter(|token| !is_url_token(token) && !is_id_token(token))
        .map(|token| token.chars().filter(|c| !c.is_ascii_digit()).collect())
        .collect();
    let joined = words.join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = collapsed.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, ':' | '-' | ',' | '.' | '#' | '(' | ')' | '[' | ']')
    });
    non_empty(Some(stem))
}

/// The rule scope of one alert. The scope always comes from an alert, never from a guess.
pub fn scope_from_alert(
    source: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    topic_key: Option<&str>,
) -> Scope {
    Scope {
        source: source.trim().to_owned(),
        sender_key: sender.and_then(normalize_sender),
        topic_key: non_empty(topic_key),
        pattern: subject.and_then(subject_stem),
    }
}

/// A newer scope supersedes an older rule only when it covers all of that rule's items.
fn covers(newer: &Scope, older: &Scope) -> bool {
    newer.source == older.source
        && newer.sender_key.as_ref().is_none_or(|key| older.sender_key.as_ref() == Some(key))
        && newer.topic_key.as_ref().is_none_or(|key| older.topic_key.as_ref() == Some(key))
        && newer.pattern.as_ref().is_none_or(|pattern| {
            older.pattern.as_ref().is_some_and(|old| old.contains(pattern))
        })
}

/// Owner text that is a one-time exception (`once:`, `이번만:` and the other task-only
/// prefixes). Such an objection affects only the alert it was attached to.
pub fn is_task_only(text: &str) -> bool {
    let (learnable, once) = crate::supervisor::memory::split_task_only(text);
    learnable.is_empty() && !once.is_empty()
}

/// Stores a rule and closes active rules fully covered by it. The mhc receipt stays empty.
pub async fn record(
    pool: &SqlitePool,
    scope: &Scope,
    effect: Effect,
    owner_words: &str,
    origin_alert_ref: Option<&str>,
) -> Result<Rule> {
    record_with_receipt(pool, scope, effect, owner_words, origin_alert_ref, None).await
}

/// [`record`] with the mhc receipt of the owner text the rule came from.
pub async fn record_with_receipt(
    pool: &SqlitePool,
    scope: &Scope,
    effect: Effect,
    owner_words: &str,
    origin_alert_ref: Option<&str>,
    mhc_receipt: Option<&str>,
) -> Result<Rule> {
    let id = format!("orr_{}", uuid::Uuid::new_v4().simple());
    let now = now_rfc3339();
    let mut tx = pool.begin_write().await?;
    let active: Vec<RuleRow> = sqlx::query_as(&format!(
        "SELECT {RULE_COLUMNS} FROM owner_report_rules WHERE source = ? AND closed_at IS NULL"
    ))
    .bind(&scope.source)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO owner_report_rules(id, source, sender_key, topic_key, pattern, effect,
             owner_words, origin_alert_ref, mhc_receipt, created_at)
         VALUES(?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&scope.source)
    .bind(&scope.sender_key)
    .bind(&scope.topic_key)
    .bind(&scope.pattern)
    .bind(effect.as_str())
    .bind(owner_words)
    .bind(origin_alert_ref)
    .bind(mhc_receipt)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    for row in active {
        let older = Rule::try_from(row)?;
        if covers(scope, &older.scope) {
            sqlx::query(
                "UPDATE owner_report_rules SET superseded_by = ?, closed_at = ?
                 WHERE id = ? AND closed_at IS NULL",
            )
            .bind(&id)
            .bind(&now)
            .bind(&older.id)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(Rule {
        id,
        scope: scope.clone(),
        effect,
        owner_words: owner_words.to_owned(),
        origin_alert_ref: origin_alert_ref.map(str::to_owned),
        mhc_receipt: mhc_receipt.map(str::to_owned),
        created_at: now,
        superseded_by: None,
        closed_at: None,
    })
}

/// An owner objection: a task-only one (`once:`, `이번만:`) stores no rule and returns `None`;
/// anything else is [`record_with_receipt`].
pub async fn object(
    pool: &SqlitePool,
    scope: &Scope,
    effect: Effect,
    owner_words: &str,
    origin_alert_ref: Option<&str>,
    mhc_receipt: Option<&str>,
) -> Result<Option<Rule>> {
    if is_task_only(owner_words) {
        return Ok(None);
    }
    record_with_receipt(
        pool,
        scope,
        effect,
        owner_words,
        origin_alert_ref,
        mhc_receipt,
    )
    .await
    .map(Some)
}

/// Active rules, newest first.
pub async fn active_rules(pool: &SqlitePool) -> Result<Vec<Rule>> {
    let rows: Vec<RuleRow> = sqlx::query_as(&format!(
        "SELECT {RULE_COLUMNS} FROM owner_report_rules WHERE closed_at IS NULL
         ORDER BY julianday(created_at) DESC, rowid DESC"
    ))
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Rule::try_from).collect()
}

/// The rules a newer rule closed, newest first.
pub async fn superseded(pool: &SqlitePool, id: &str) -> Result<Vec<Rule>> {
    let rows: Vec<RuleRow> = sqlx::query_as(&format!(
        "SELECT {RULE_COLUMNS} FROM owner_report_rules WHERE superseded_by = ?
         ORDER BY julianday(created_at) DESC, rowid DESC"
    ))
    .bind(id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(Rule::try_from).collect()
}

/// Whether `rule` applies to an alert: same source, and every set field of the rule matches
/// (sender and topic equal, the subject stem contains the pattern).
pub fn applies(
    rule: &Rule,
    source: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    topic_key: Option<&str>,
) -> bool {
    let alert = scope_from_alert(source, sender, subject, topic_key);
    let scope = &rule.scope;
    scope.source == alert.source
        && scope
            .sender_key
            .as_ref()
            .is_none_or(|key| alert.sender_key.as_ref() == Some(key))
        && scope
            .topic_key
            .as_ref()
            .is_none_or(|key| alert.topic_key.as_ref() == Some(key))
        && scope.pattern.as_ref().is_none_or(|pattern| {
            alert
                .pattern
                .as_deref()
                .is_some_and(|stem| stem.contains(pattern.as_str()))
        })
}

/// The first rule of `rules` (newest first) that applies to the alert.
pub fn find_match<'a>(
    rules: &'a [Rule],
    source: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    topic_key: Option<&str>,
) -> Option<&'a Rule> {
    rules
        .iter()
        .find(|rule| applies(rule, source, sender, subject, topic_key))
}

/// The newest active rule that applies to the alert.
pub async fn matching_rule(
    pool: &SqlitePool,
    source: &str,
    sender: Option<&str>,
    subject: Option<&str>,
    topic_key: Option<&str>,
) -> Result<Option<Rule>> {
    let rules = active_rules(pool).await?;
    Ok(find_match(&rules, source, sender, subject, topic_key).cloned())
}

/// Closes an active rule by hand (`/omo-rules`). Returns whether a rule was closed.
pub async fn close(pool: &SqlitePool, id: &str) -> Result<bool> {
    let closed = sqlx::query(
        "UPDATE owner_report_rules SET closed_at = ? WHERE id = ? AND closed_at IS NULL",
    )
    .bind(now_rfc3339())
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(closed > 0)
}

/// The owner-facing name of an alert source in the owner's language.
fn source_name(source: &str) -> &str {
    let locale = crate::locale::current();
    match source {
        "mail" => locale.pick("메일", "mail"),
        "kakao" => locale.pick("카톡", "KakaoTalk"),
        "imessage" => "iMessage",
        "cron" => locale.pick("크론", "cron"),
        "github" => "GitHub",
        "ci" => "CI",
        "report" => locale.pick("보고", "report"),
        other => other,
    }
}

/// The rule's scope in the owner's language, e.g. `gitlab@mg.gitlab.com의 'gitlab.com sign-in
/// from new location' 메일 알림` or `mail alerts from gitlab@mg.gitlab.com matching 'gitlab.com
/// sign-in from new location'`.
pub fn describe_ko(rule: &Rule) -> String {
    let scope = &rule.scope;
    if !crate::locale::current().is_korean() {
        let mut text = format!("{} alerts", source_name(&scope.source));
        if let Some(sender) = &scope.sender_key {
            text.push_str(&format!(" from {sender}"));
        }
        if let Some(pattern) = &scope.pattern {
            text.push_str(&format!(" matching '{pattern}'"));
        }
        if let Some(topic) = &scope.topic_key {
            text.push_str(&format!(" [{topic}]"));
        }
        return text;
    }
    let mut parts = Vec::new();
    if let Some(sender) = &scope.sender_key {
        parts.push(format!("{sender}의"));
    }
    if let Some(pattern) = &scope.pattern {
        parts.push(format!("'{pattern}'"));
    }
    if let Some(topic) = &scope.topic_key {
        parts.push(format!("[{topic}]"));
    }
    parts.push(format!("{} 알림", source_name(&scope.source)));
    parts.join(" ")
}

/// The one confirmation, in the owner's language, sent after a rule is stored. A closed rule
/// with a different effect is named in one more line.
pub fn confirmation_ko(rule: &Rule, superseded: Option<&Rule>) -> String {
    let description = describe_ko(rule);
    if !crate::locale::current().is_korean() {
        let mut text = match rule.effect {
            Effect::Mute => format!(
                "I won't send {description} any more. To get them again, say 'send these again'."
            ),
            Effect::Digest => format!(
                "From now on {description} go only into the daily digest. To get them right away, say 'send these again'."
            ),
            Effect::Send => format!("From now on I'll send {description} right away."),
        };
        if let Some(older) = superseded.filter(|older| older.effect != rule.effect) {
            text.push_str(&format!("\nThis replaces the earlier rule for {}.", describe_ko(older)));
        }
        return text;
    }
    let mut text = match rule.effect {
        Effect::Mute => format!(
            "앞으로 {description}은 보내지 않습니다. 다시 받으시려면 '이건 다시 보내'라고 해 주세요."
        ),
        Effect::Digest => format!(
            "앞으로 {description}은 하루 요약에만 넣습니다. 바로 받으시려면 '이건 다시 보내'라고 해 주세요."
        ),
        Effect::Send => format!("앞으로 {description}은 바로 보내 드립니다."),
    };
    if let Some(older) = superseded.filter(|older| older.effect != rule.effect) {
        text.push_str(&format!(
            "\n이전 규칙 '{}'은 이 규칙으로 대체됐습니다.",
            describe_ko(older)
        ));
    }
    text
}

/// One line per rule for the judge instructions. Holds alert-derived text: fence it.
pub fn rule_line(rule: &Rule) -> String {
    let scope = &rule.scope;
    let mut line = format!(
        "- rule {}: {} => {} source={}",
        rule.id,
        rule.effect.as_str(),
        rule.effect.triage_option(),
        scope.source
    );
    if let Some(sender) = &scope.sender_key {
        line.push_str(&format!(" sender={sender}"));
    }
    if let Some(topic) = &scope.topic_key {
        line.push_str(&format!(" topic={topic}"));
    }
    if let Some(pattern) = &scope.pattern {
        line.push_str(&format!(" subject~\"{pattern}\""));
    }
    line
}

/// An alert the owner labelled with a rule, and the effect he chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelledExample {
    pub effect: Effect,
    pub alert: String,
}

/// The origin alerts of the most recently closed rules, at most `limit`, newest first. The
/// alert text is the inbox item's title and snippet, else the owner's words.
pub async fn labelled_examples(pool: &SqlitePool, limit: i64) -> Result<Vec<LabelledExample>> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT r.effect, r.owner_words, i.title, i.snippet
           FROM owner_report_rules r LEFT JOIN inbox_items i ON i.id = r.origin_alert_ref
          WHERE r.closed_at IS NOT NULL AND r.origin_alert_ref IS NOT NULL
          ORDER BY julianday(r.closed_at) DESC, r.rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(effect, owner_words, title, snippet)| -> Result<LabelledExample> {
            let alert = match (title, snippet) {
                (Some(title), Some(snippet)) => format!("{title}: {snippet}"),
                (Some(text), None) | (None, Some(text)) => text,
                (None, None) => owner_words,
            };
            let alert: String = alert
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(EXAMPLE_MAX_CHARS)
                .collect();
            Ok(LabelledExample {
                effect: Effect::parse(&effect)?,
                alert,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{Class, classify_pending};
    use chrono::{DateTime, TimeDelta, Utc};

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

    async fn decision(pool: &SqlitePool, id: &str) -> (String, String) {
        sqlx::query_as("SELECT class, reason FROM inbox_decisions WHERE item_id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn triage(pool: &SqlitePool, judge: &crate::judge::Judge) -> usize {
        classify_pending(pool, judge, Utc::now(), "meta", |_id, _class| async move {
            Ok(None)
        })
        .await
        .unwrap()
    }

    const GITLAB: &str = "GitLab <gitlab@mg.gitlab.com>";
    const GITLAB_SIGN_IN: &str = "gitlab.com sign-in from new location";

    #[test]
    fn a_subject_stem_drops_digits_ids_and_urls() {
        assert_eq!(
            subject_stem("Your code 483920 for https://x.y/z ref 3f9a0c1d22 ").as_deref(),
            Some("your code for ref")
        );
        assert_eq!(
            normalize_sender("GitLab <GitLab@MG.gitlab.com>").as_deref(),
            Some("gitlab@mg.gitlab.com")
        );
    }

    #[tokio::test]
    async fn a_mute_rule_skips_the_judge() {
        let pool = crate::storage::memory().await.unwrap();
        let scope = scope_from_alert("mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None);
        let rule = record(&pool, &scope, Effect::Mute, "이건 이제 그만 보내", None)
            .await
            .unwrap();
        mail(
            &pool,
            "m1",
            GITLAB,
            GITLAB_SIGN_IN,
            Utc::now() - TimeDelta::minutes(1),
        )
        .await;
        // A judge that errors when asked: the mute rule must decide without it.
        let judge = crate::judge::Judge::forced("not_an_option");
        assert_eq!(triage(&pool, &judge).await, 1);
        let (class, reason) = decision(&pool, "m1").await;
        assert_eq!(class, Class::Ignore.as_str());
        assert_eq!(reason, format!("muted:{}", rule.id));
    }

    #[tokio::test]
    async fn send_kind_supersedes_mute() {
        let pool = crate::storage::memory().await.unwrap();
        let scope = scope_from_alert("mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None);
        let mute = record(&pool, &scope, Effect::Mute, "그만 보내", Some("m1"))
            .await
            .unwrap();
        let send = record(&pool, &scope, Effect::Send, "이건 다시 보내", Some("m2"))
            .await
            .unwrap();
        let active = active_rules(&pool).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, send.id);
        let closed = superseded(&pool, &send.id).await.unwrap();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id, mute.id);
        assert!(closed[0].closed_at.is_some());
        let found = matching_rule(&pool, "mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.effect, Effect::Send);
        let text = confirmation_ko(&send, Some(&closed[0]));
        assert!(text.starts_with("앞으로 gitlab@mg.gitlab.com의 'gitlab.com sign-in from new location' 메일 알림은 바로"));
        assert!(text.contains("이전 규칙"));
        let muted = confirmation_ko(&mute, None);
        assert!(muted.contains("보내지 않습니다. 다시 받으시려면 '이건 다시 보내'라고 해 주세요."));
        assert!(!muted.contains("이전 규칙"));
        crate::locale::with(crate::locale::OwnerLocale::english_utc(), || {
            let text = confirmation_ko(&send, Some(&closed[0]));
            assert!(text.starts_with("From now on I'll send mail alerts from gitlab@mg.gitlab.com matching 'gitlab.com sign-in from new location' right away."), "{text}");
            assert!(text.contains("This replaces the earlier rule for mail alerts"));
            let muted = confirmation_ko(&mute, None);
            assert!(muted.starts_with("I won't send mail alerts"), "{muted}");
            assert!(!muted.chars().any(crate::locale::is_hangul));
        });
        assert!(close(&pool, &send.id).await.unwrap());
        assert!(!close(&pool, &send.id).await.unwrap());
        assert!(active_rules(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn distinct_subject_rules_survive_and_latest_matching_scope_wins() {
        use crate::report_gate::{queue, Decision, Report, Salience};
        let pool = crate::storage::memory().await.unwrap();
        let login = scope_from_alert("mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None);
        let expiry = scope_from_alert("mail", Some(GITLAB), Some("Token expires"), None);
        record(&pool, &login, Effect::Mute, "stop login", None).await.unwrap();
        record(&pool, &expiry, Effect::Mute, "stop expiry", None).await.unwrap();
        assert_eq!(active_rules(&pool).await.unwrap().len(), 2);
        for subject in [GITLAB_SIGN_IN, "Token expires"] {
            assert_eq!(matching_rule(&pool, "mail", Some(GITLAB), Some(subject), None)
                .await.unwrap().unwrap().effect, Effect::Mute);
        }
        let all_mail = Scope { source: "mail".into(), sender_key: None, topic_key: None, pattern: None };
        record(&pool, &all_mail, Effect::Mute, "stop mail", None).await.unwrap();
        let exception = record(&pool, &login, Effect::Send, "send login", None).await.unwrap();
        assert_eq!(active_rules(&pool).await.unwrap().len(), 2);
        let report = Report::new("first", "mail", "inbox:first", Salience::Action, "확인이 필요한 메일입니다.")
            .with_sender(GITLAB).with_quote(GITLAB_SIGN_IN);
        assert_eq!(queue(&pool, "owner", &report, false).await.unwrap().0, Decision::Sent);
        assert_eq!(matching_rule(&pool, "mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None)
            .await.unwrap().unwrap().id, exception.id);
        let other = Report::new("other", "mail", "inbox:other", Salience::Action, "확인이 필요한 메일입니다.")
            .with_sender(GITLAB).with_quote("Token expires");
        assert_eq!(queue(&pool, "owner", &other, false).await.unwrap().0, Decision::Muted);
        let newest = record(&pool, &all_mail, Effect::Mute, "stop all again", None).await.unwrap();
        assert_eq!(active_rules(&pool).await.unwrap().len(), 1);
        let later = Report::new("later", "mail", "inbox:later", Salience::Action, "확인이 필요한 메일입니다.")
            .with_sender(GITLAB).with_quote(GITLAB_SIGN_IN);
        assert_eq!(queue(&pool, "owner", &later, false).await.unwrap().0, Decision::Muted);
        assert_eq!(matching_rule(&pool, "mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None)
            .await.unwrap().unwrap().id, newest.id);
    }

    #[tokio::test]
    async fn once_objection_stores_no_rule() {
        let pool = crate::storage::memory().await.unwrap();
        assert!(is_task_only("이번만: 그만 보내"));
        assert!(is_task_only("once: stop sending this"));
        assert!(!is_task_only("이건 이제 그만 보내"));
        let scope = scope_from_alert("mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None);
        let stored = object(&pool, &scope, Effect::Mute, "이번만: 그만 보내", Some("m1"), None)
            .await
            .unwrap();
        assert!(stored.is_none());
        assert!(active_rules(&pool).await.unwrap().is_empty());
        let stored = object(
            &pool,
            &scope,
            Effect::Mute,
            "그만 보내",
            Some("m1"),
            Some("mhc_receipt_1"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(stored.mhc_receipt.as_deref(), Some("mhc_receipt_1"));
    }

    #[tokio::test]
    async fn incident_2026_10_30() {
        let pool = crate::storage::memory().await.unwrap();
        let judge = crate::judge::Judge::forced("owner_must_act");
        let at = Utc::now() - TimeDelta::minutes(10);
        let google = (
            "Google <no-reply@accounts.google.com>",
            "Security alert",
        );
        let gitlab_sign_in = (GITLAB, GITLAB_SIGN_IN);
        let gitlab_tokens = (
            GITLAB,
            "Your personal access tokens will expire in 30 days or less",
        );
        let namecheap = (
            "Namecheap <renewals@namecheap.com>",
            "indo, add funds before your auto-renewal fails",
        );
        let ferryx = ("Ferryx <login@mahobrowser.com>", "Sign in to Ferryx");
        let first = [
            ("r1", google),
            ("r2", gitlab_sign_in),
            ("r3", gitlab_tokens),
            ("r4", namecheap),
            ("r5", ferryx),
        ];
        for (id, (sender, subject)) in first {
            mail(&pool, id, sender, subject, at).await;
        }
        // No rules: every mail is delivered as the judge classified it.
        assert_eq!(triage(&pool, &judge).await, 5);
        for (id, _) in first {
            assert_eq!(
                decision(&pool, id).await,
                (String::from("owner"), String::from("owner_must_act"))
            );
        }

        // The owner mutes the GitLab sign-in kind on the delivered r2 alert.
        let scope = scope_from_alert("mail", Some(GITLAB), Some(GITLAB_SIGN_IN), None);
        let mute = record(&pool, &scope, Effect::Mute, "이건 이제 그만 보내", Some("r2"))
            .await
            .unwrap();
        let at = Utc::now() - TimeDelta::minutes(5);
        mail(&pool, "n1", GITLAB, GITLAB_SIGN_IN, at).await;
        mail(&pool, "n2", google.0, google.1, at).await;
        mail(&pool, "n3", namecheap.0, namecheap.1, at).await;
        assert_eq!(triage(&pool, &judge).await, 3);
        assert_eq!(
            decision(&pool, "n1").await,
            (String::from("ignore"), format!("muted:{}", mute.id))
        );
        assert_eq!(decision(&pool, "n2").await.0, "owner");
        assert_eq!(decision(&pool, "n3").await.0, "owner");

        // "이건 다시 보내" restores GitLab.
        let send = record(&pool, &scope, Effect::Send, "이건 다시 보내", Some("n1"))
            .await
            .unwrap();
        mail(&pool, "s1", GITLAB, GITLAB_SIGN_IN, Utc::now() - TimeDelta::minutes(1)).await;
        assert_eq!(triage(&pool, &judge).await, 1);
        let (class, reason) = decision(&pool, "s1").await;
        assert_eq!(class, "owner");
        assert!(reason.ends_with(&format!("send:{}", send.id)), "{reason}");
        assert_eq!(
            superseded(&pool, &send.id).await.unwrap()[0].id,
            mute.id
        );
    }
}
