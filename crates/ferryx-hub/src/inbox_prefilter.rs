//! omo-gateway's inbox prefilter (`src/inbox/prefilter.rs`): noise that never reaches triage.
//! Rules are read from omo's `~/.omon/inbox-filters.json` (never written here); a missing file
//! uses omo's defaults. Drops are counted per KST day in `inbox_filter_stats` like omo.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::inbox_sources::SourceItem;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct PrefilterRules {
    pub owner_kakao_name: String,
    /// KakaoTalk sender names that are the owner himself; his own messages never need his action.
    pub owner_kakao_authors: Vec<String>,
    pub kakao_keywords: Vec<String>,
    pub excluded_kakao_rooms: HashSet<String>,
}

impl Default for PrefilterRules {
    fn default() -> Self {
        Self {
            owner_kakao_name: String::new(),
            owner_kakao_authors: vec!["윤인도".to_owned()],
            kakao_keywords: Vec::new(),
            excluded_kakao_rooms: ["18491246151592761".to_owned()].into_iter().collect(),
        }
    }
}

pub fn rules_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".omon/inbox-filters.json"))
}

impl PrefilterRules {
    /// omo's rules file, or omo's defaults when it is missing or unreadable.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::warn!(%error, "inbox filter rules are invalid; using omo defaults");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn from_home() -> Self {
        Self::load(rules_path().as_deref())
    }

    /// omo's `drop_reason` for the sources ferryx collects raw (mail, KakaoTalk, iMessage).
    /// Slack and GitHub rows are already selected by their sources.
    pub fn drop_reason(&self, item: &SourceItem) -> Option<&'static str> {
        if item.source_ref.trim().is_empty() || item.snippet.trim().is_empty() {
            return Some("empty");
        }
        let author = item.author.as_deref().unwrap_or_default();
        let title = item.title.as_deref().unwrap_or_default();
        let text = format!("{author} {title} {}", item.snippet).to_lowercase();
        match item.source {
            "mail" => {
                let has = |category: &str| item.mail_categories.iter().any(|c| c == category);
                if has("promotions")
                    || contains_any(&text, &["category:promotions", "[promotions]"])
                {
                    return Some("mail_promotions");
                }
                if has("social") || contains_any(&text, &["category:social", "[social]"]) {
                    return Some("mail_social");
                }
                if contains_any(
                    &text,
                    &[
                        "인증 코드",
                        "인증코드",
                        "인증번호",
                        "보안 코드",
                        "verification code",
                        "security code",
                        "one-time code",
                        "login code",
                    ],
                ) {
                    return Some("mail_otp");
                }
                // Security, billing and account notices come from no-reply senders in Gmail's
                // Updates tab; the owner must see them (2026-10-23: such mail never reached him).
                if is_account_critical(&text) {
                    return None;
                }
                if has("updates") || contains_any(&text, &["category:updates", "[updates]"]) {
                    return Some("mail_updates");
                }
                let sender = author.to_lowercase();
                if contains_any(
                    &sender,
                    &["no-reply", "noreply", "donotreply", "do-not-reply"],
                ) && !contains_any(&sender, &["github.com", "discord.com", "linkedin.com"])
                {
                    return Some("mail_no_reply");
                }
                None
            }
            "kakao" => {
                let room = item.thread_ref.as_deref().unwrap_or(&item.source_ref);
                if self.excluded_kakao_rooms.contains(room) {
                    return Some("kakao_excluded_room");
                }
                if self.owner_kakao_authors.iter().any(|name| name == author) {
                    return Some("kakao_owner_author");
                }
                let room_type = item.room_type.as_deref().unwrap_or_default();
                if matches!(room_type, "OM" | "OD" | "PlusChat") {
                    let matches_owner = !self.owner_kakao_name.is_empty()
                        && text.contains(&self.owner_kakao_name.to_lowercase());
                    if !matches_owner
                        && !self
                            .kakao_keywords
                            .iter()
                            .any(|keyword| text.contains(&keyword.to_lowercase()))
                    {
                        return Some("kakao_noise_room_no_keyword");
                    }
                } else if !matches!(room_type, "DirectChat" | "MultiChat") {
                    return Some("kakao_unsupported_room_type");
                }
                None
            }
            "imessage" => {
                if author.len() >= 2
                    && author.len() <= 6
                    && author.chars().all(|ch| ch.is_ascii_digit())
                {
                    return Some("imessage_shortcode");
                }
                let body = item.snippet.trim_start();
                let body = body.strip_prefix("[Web발신]").unwrap_or(body).trim_start();
                if body.starts_with("(광고)") {
                    return Some("imessage_ad");
                }
                if contains_any(
                    &text,
                    &[
                        "인증 코드",
                        "인증코드",
                        "보안 코드",
                        "verification code",
                        "security code",
                    ],
                ) {
                    return Some("imessage_otp");
                }
                None
            }
            _ => None,
        }
    }

    /// omo's `direct_class`: known notification mail is FYI without a judge call.
    pub fn direct_class(&self, item: &SourceItem) -> Option<&'static str> {
        if item.source != "mail" {
            return None;
        }
        let author = item.author.as_deref().unwrap_or_default().to_lowercase();
        let text = format!(
            "{} {}",
            item.title.as_deref().unwrap_or_default(),
            item.snippet
        )
        .to_lowercase();
        let known = ["github.com", "discord.com", "linkedin.com"]
            .iter()
            .any(|sender| author.contains(sender));
        let escalated = contains_any(
            &text,
            &["failed", "error", "review requested", "mentioned you"],
        );
        (known && !escalated).then_some("fyi")
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

/// Security, billing and account notices: sign-in alerts, password and 2FA changes, failed
/// payments, suspended or expiring accounts. `text` is the lowercased sender, subject and snippet.
fn is_account_critical(text: &str) -> bool {
    contains_any(
        text,
        &[
            "security alert",
            "보안 알림",
            "new sign-in",
            "new login",
            "suspicious",
            "unusual activity",
            "새로운 로그인",
            "로그인 알림",
            "password was changed",
            "password changed",
            "reset your password",
            "비밀번호",
            "two-factor",
            "2-step",
            "2단계",
            "payment failed",
            "payment declined",
            "card declined",
            "결제 실패",
            "결제가 실패",
            "invoice",
            "청구서",
            "billing",
            "past due",
            "overdue",
            "미납",
            "account suspended",
            "account has been suspended",
            "account will be",
            "계정이 정지",
            "계정 정지",
            "subscription expir",
            "subscription has been cancel",
            "domain expir",
            "will expire",
            "만료",
            "action required",
        ],
    )
}

/// omo's `record_drop`, per KST day. The table is omo's; it is created when missing.
pub async fn record_drops(pool: &SqlitePool, drops: &[(&'static str, &'static str)]) {
    if drops.is_empty() {
        return;
    }
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS inbox_filter_stats (
            day TEXT NOT NULL,
            source TEXT NOT NULL,
            rule TEXT NOT NULL,
            count INTEGER NOT NULL CHECK(count >= 0),
            PRIMARY KEY(day, source, rule)
        )",
    )
    .execute(pool)
    .await;
    let day = Utc::now()
        .with_timezone(&chrono_tz::Asia::Seoul)
        .date_naive()
        .to_string();
    for (source, rule) in drops {
        if let Err(error) = sqlx::query(
            "INSERT INTO inbox_filter_stats(day, source, rule, count) VALUES(?, ?, ?, 1)
             ON CONFLICT(day, source, rule) DO UPDATE SET count = count + 1",
        )
        .bind(&day)
        .bind(source)
        .bind(rule)
        .execute(pool)
        .await
        {
            tracing::debug!(%error, "inbox filter stat was not recorded");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: &'static str, author: &str, snippet: &str) -> SourceItem {
        SourceItem {
            source,
            source_ref: "r1".into(),
            snippet: snippet.into(),
            received_at: Utc::now(),
            thread_ref: Some("room".into()),
            author: Some(author.into()),
            title: None,
            link: None,
            room_type: None,
            mail_categories: Vec::new(),
        }
    }

    #[test]
    fn kakao_noise_rooms_and_the_owner_are_dropped_like_omo() {
        let rules = PrefilterRules::default();
        let mut open_chat = item("kakao", "someone", "공지");
        open_chat.room_type = Some("OM".into());
        assert_eq!(
            rules.drop_reason(&open_chat),
            Some("kakao_noise_room_no_keyword")
        );
        let mut direct = item("kakao", "friend", "hi");
        direct.room_type = Some("DirectChat".into());
        assert_eq!(rules.drop_reason(&direct), None);
        let mut own = direct.clone();
        own.author = Some("윤인도".into());
        assert_eq!(rules.drop_reason(&own), Some("kakao_owner_author"));
        let mut excluded = direct.clone();
        excluded.thread_ref = Some("18491246151592761".into());
        assert_eq!(rules.drop_reason(&excluded), Some("kakao_excluded_room"));
        let mut unknown = direct.clone();
        unknown.room_type = None;
        assert_eq!(
            rules.drop_reason(&unknown),
            Some("kakao_unsupported_room_type")
        );
        let keyed = PrefilterRules {
            kakao_keywords: vec!["ferryx".into()],
            ..Default::default()
        };
        let mut mention = open_chat.clone();
        mention.snippet = "Ferryx 질문".into();
        assert_eq!(keyed.drop_reason(&mention), None);
    }

    #[test]
    fn mail_and_imessage_noise_is_dropped_like_omo() {
        let rules = PrefilterRules::default();
        assert_eq!(
            rules.drop_reason(&item(
                "mail",
                "Slack <no-reply@slack.com>",
                "Your security code is 1"
            )),
            Some("mail_otp")
        );
        assert_eq!(
            rules.drop_reason(&item("mail", "Survey <noreply@x.example>", "answer")),
            Some("mail_no_reply")
        );
        let mut promo = item("mail", "Shop <shop@x.example>", "sale");
        promo.mail_categories = vec!["promotions".into()];
        assert_eq!(rules.drop_reason(&promo), Some("mail_promotions"));
        let mut update = item("mail", "Shop <shop@x.example>", "your order shipped");
        update.mail_categories = vec!["updates".into()];
        assert_eq!(rules.drop_reason(&update), Some("mail_updates"));
        let mut alert = item("mail", "Google <no-reply@accounts.google.com>", "Security alert: new sign-in on Mac");
        alert.mail_categories = vec!["updates".into()];
        assert_eq!(rules.drop_reason(&alert), None, "security notices reach the owner");
        assert_eq!(
            rules.drop_reason(&item("mail", "Cloudflare <noreply@notify.cloudflare.com>", "Payment failed for your invoice")),
            None,
            "billing notices reach the owner"
        );
        assert_eq!(
            rules.drop_reason(&item("mail", "Person <p@x.example>", "Re: quote")),
            None
        );
        assert_eq!(
            rules.drop_reason(&item("imessage", "1588", "hi")),
            Some("imessage_shortcode")
        );
        assert_eq!(
            rules.drop_reason(&item("imessage", "+8210", "[Web발신] (광고) sale")),
            Some("imessage_ad")
        );
        assert_eq!(rules.drop_reason(&item("imessage", "+8210", "점심?")), None);
        assert_eq!(rules.drop_reason(&item("slack", "U1", "anything")), None);
        assert_eq!(rules.drop_reason(&item("mail", "a", "  ")), Some("empty"));
    }

    #[test]
    fn omo_rules_files_load_and_missing_files_use_defaults() {
        let dir = std::env::temp_dir().join(format!("fx-prefilter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("inbox-filters.json");
        std::fs::write(
            &path,
            r#"{"owner_kakao_name":"","kakao_keywords":["x"],"excluded_kakao_rooms":["1"],"owner_discord_id":"9","bot_discord_id":null}"#,
        )
        .unwrap();
        let rules = PrefilterRules::load(Some(&path));
        assert_eq!(rules.kakao_keywords, vec!["x".to_owned()]);
        assert!(rules.excluded_kakao_rooms.contains("1"));
        assert_eq!(rules.owner_kakao_authors, vec!["윤인도".to_owned()]);
        let missing = PrefilterRules::load(Some(&dir.join("missing.json")));
        assert!(missing.excluded_kakao_rooms.contains("18491246151592761"));
        assert!(
            !dir.join("missing.json").exists(),
            "the rules file is never written"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
