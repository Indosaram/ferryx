//! The owner's language and time zone. Every owner-facing text, day boundary and
//! language-sensitive gate rule reads them from here, so the hub serves any owner.
//!
//! `FERRYX_HUB_OWNER_LANGUAGE` (a BCP 47 tag such as `ko`, `en-US`) and
//! `FERRYX_HUB_OWNER_TIMEZONE` (an IANA name such as `Asia/Seoul`) come from the daemon
//! environment or the hub env file. Unset, the language follows `LANG` and the zone follows
//! `TZ` or the system zone, then English and UTC. Built-in texts exist in Korean and English;
//! any other language gets the English text, and model-written text is asked for in the
//! owner's language by name.

use std::cell::RefCell;
use std::sync::OnceLock;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerLocale {
    /// Lowercase primary language subtag: `ko`, `en`, `ja`.
    pub language: String,
    pub timezone: Tz,
}

impl OwnerLocale {
    pub fn new(language: &str, timezone: Tz) -> Self {
        Self { language: primary_subtag(language).unwrap_or_else(|| "en".into()), timezone }
    }

    pub fn korean() -> Self {
        Self::new("ko", chrono_tz::Asia::Seoul)
    }

    pub fn english_utc() -> Self {
        Self::new("en", chrono_tz::UTC)
    }

    pub fn is_korean(&self) -> bool {
        self.language == "ko"
    }

    /// The language's English name for model instructions ("Reply in Korean").
    pub fn language_name(&self) -> String {
        match self.language.as_str() {
            "ko" => "Korean".into(),
            "en" => "English".into(),
            "ja" => "Japanese".into(),
            "zh" => "Chinese".into(),
            "es" => "Spanish".into(),
            "fr" => "French".into(),
            "de" => "German".into(),
            "pt" => "Portuguese".into(),
            "vi" => "Vietnamese".into(),
            other => format!("the language with BCP 47 tag '{other}'"),
        }
    }

    /// The Korean text for a Korean owner, the English text for everyone else.
    pub fn pick<'a>(&self, korean: &'a str, english: &'a str) -> &'a str {
        if self.is_korean() { korean } else { english }
    }

    /// The owner's calendar date of `at`.
    pub fn local_date(&self, at: DateTime<Utc>) -> NaiveDate {
        at.with_timezone(&self.timezone).date_naive()
    }

    /// The UTC instant the owner's `date` begins.
    pub fn day_start(&self, date: NaiveDate) -> DateTime<Utc> {
        let midnight = date.and_hms_opt(0, 0, 0).expect("midnight exists");
        self.timezone
            .from_local_datetime(&midnight)
            .earliest()
            .map(|at| at.with_timezone(&Utc))
            .unwrap_or_else(|| midnight.and_utc())
    }

    /// The owner's local hour (0-23) at `at`.
    pub fn local_hour(&self, at: DateTime<Utc>) -> u32 {
        use chrono::Timelike;
        at.with_timezone(&self.timezone).hour()
    }

    /// `at` in the owner's zone with `format` (chrono strftime), followed by the zone
    /// abbreviation, e.g. `2026-11-01 09:30 KST`.
    pub fn format(&self, at: DateTime<Utc>, format: &str) -> String {
        let local = at.with_timezone(&self.timezone);
        format!("{} {}", local.format(format), local.format("%Z"))
    }

    /// True when a letter belongs to a script the owner's language is written in.
    pub fn is_owner_letter(&self, c: char) -> bool {
        match self.language.as_str() {
            "ko" => is_hangul(c),
            "ja" => is_kana(c) || is_han(c),
            "zh" => is_han(c),
            "ru" | "uk" | "bg" | "sr" => is_cyrillic(c),
            _ => c.is_ascii_alphabetic() || is_latin_extended(c),
        }
    }

    /// True when prose (letters only) is mostly not in the owner's script: under 30% of its
    /// letters. Text with no letters is not foreign.
    pub fn is_foreign_prose(&self, text: &str) -> bool {
        let (letters, own) = text.chars().filter(|c| c.is_alphabetic()).fold((0usize, 0usize), |(l, o), c| {
            (l + 1, o + usize::from(self.is_owner_letter(c)))
        });
        letters > 0 && own * 10 < letters * 3
    }
}

fn primary_subtag(tag: &str) -> Option<String> {
    let primary = tag.trim().split(['-', '_', '.', '@']).next()?.trim().to_ascii_lowercase();
    (primary.len() >= 2 && primary.len() <= 3 && primary.chars().all(|c| c.is_ascii_alphabetic()) && primary != "c")
        .then_some(primary)
}

pub fn is_hangul(c: char) -> bool {
    matches!(c, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}')
}

fn is_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}')
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

fn is_latin_extended(c: char) -> bool {
    matches!(c, '\u{00C0}'..='\u{024F}')
}

fn timezone_from(name: &str) -> Option<Tz> {
    let name = name.trim().trim_start_matches(':');
    name.parse::<Tz>().ok().or_else(|| {
        // A TZ path such as /usr/share/zoneinfo/Asia/Seoul.
        name.split_once("zoneinfo/").and_then(|(_, zone)| zone.parse::<Tz>().ok())
    })
}

fn system_timezone() -> Option<Tz> {
    if let Ok(tz) = std::env::var("TZ") {
        if let Some(zone) = timezone_from(&tz) {
            return Some(zone);
        }
    }
    let link = std::fs::read_link("/etc/localtime").ok()?;
    timezone_from(&link.to_string_lossy())
}

/// The configured locale: explicit settings, then the system's, then English/UTC.
pub fn from_settings(language: Option<&str>, timezone: Option<&str>, system_language: Option<&str>, system_zone: Option<Tz>) -> OwnerLocale {
    let language = language
        .and_then(primary_subtag)
        .or_else(|| system_language.and_then(primary_subtag))
        .unwrap_or_else(|| "en".into());
    let zone = timezone.and_then(timezone_from).or(system_zone).unwrap_or(chrono_tz::UTC);
    OwnerLocale { language, timezone: zone }
}

fn configured() -> &'static OwnerLocale {
    static LOCALE: OnceLock<OwnerLocale> = OnceLock::new();
    LOCALE.get_or_init(|| {
        let locale = from_settings(
            crate::hub_env::var("FERRYX_HUB_OWNER_LANGUAGE").ok().as_deref(),
            crate::hub_env::var("FERRYX_HUB_OWNER_TIMEZONE").ok().as_deref(),
            std::env::var("LC_ALL").ok().or_else(|| std::env::var("LANG").ok()).as_deref(),
            system_timezone(),
        );
        tracing::info!(language = %locale.language, timezone = %locale.timezone, "owner locale");
        locale
    })
}

thread_local! {
    static OVERRIDE: RefCell<Option<OwnerLocale>> = const { RefCell::new(None) };
}

/// The owner's locale. Unit tests default to Korean/Asia/Seoul (the fixtures' owner) and may
/// run a block under another locale with [`with`].
pub fn current() -> OwnerLocale {
    if let Some(locale) = OVERRIDE.with(|cell| cell.borrow().clone()) {
        return locale;
    }
    if cfg!(test) {
        return OwnerLocale::korean();
    }
    configured().clone()
}

/// Runs `f` with `locale` as the owner's locale on this thread.
pub fn with<T>(locale: OwnerLocale, f: impl FnOnce() -> T) -> T {
    let previous = OVERRIDE.with(|cell| cell.replace(Some(locale)));
    let result = f();
    OVERRIDE.with(|cell| *cell.borrow_mut() = previous);
    result
}

/// Async form of [`with`]: the override holds across awaits of `future` on a
/// current-thread runtime (`#[tokio::test]`).
pub async fn scope<F: std::future::Future>(locale: OwnerLocale, future: F) -> F::Output {
    let previous = OVERRIDE.with(|cell| cell.replace(Some(locale)));
    let result = future.await;
    OVERRIDE.with(|cell| *cell.borrow_mut() = previous);
    result
}

/// Shorthand for `current().pick(korean, english)`, owned.
pub fn text(korean: &str, english: &str) -> String {
    current().pick(korean, english).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_win_then_the_system_then_english_utc() {
        let seoul = from_settings(Some("ko-KR"), Some("Asia/Seoul"), Some("en_US.UTF-8"), None);
        assert_eq!((seoul.language.as_str(), seoul.timezone), ("ko", chrono_tz::Asia::Seoul));
        let system = from_settings(None, None, Some("ja_JP.UTF-8"), Some(chrono_tz::Asia::Tokyo));
        assert_eq!((system.language.as_str(), system.timezone), ("ja", chrono_tz::Asia::Tokyo));
        let bare = from_settings(None, Some("not/a-zone"), Some("C"), None);
        assert_eq!((bare.language.as_str(), bare.timezone), ("en", chrono_tz::UTC));
        assert_eq!(from_settings(None, Some("/usr/share/zoneinfo/Europe/Paris"), None, None).timezone, chrono_tz::Europe::Paris);
    }

    #[test]
    fn days_and_scripts_follow_the_owner() {
        let at = DateTime::parse_from_rfc3339("2026-10-31T16:30:00Z").unwrap().with_timezone(&Utc);
        let seoul = OwnerLocale::korean();
        let utc = OwnerLocale::english_utc();
        assert_eq!(seoul.local_date(at).to_string(), "2026-11-01");
        assert_eq!(utc.local_date(at).to_string(), "2026-10-31");
        assert_eq!(seoul.day_start(seoul.local_date(at)).to_rfc3339(), "2026-10-31T15:00:00+00:00");
        assert_eq!(seoul.format(at, "%m-%d %H:%M"), "11-01 01:30 KST");
        assert!(seoul.is_foreign_prose("I'll wait for the login attempt result."));
        assert!(!seoul.is_foreign_prose("메일 제목은 \"Sign in to Ferryx\"입니다."));
        assert!(!utc.is_foreign_prose("I'll wait for the login attempt result."));
        assert!(utc.is_foreign_prose("로그인 결과를 기다리겠습니다."));
        assert!(!utc.is_foreign_prose("12:40 ✅"));
    }

    #[tokio::test]
    async fn tests_default_to_the_korean_owner_and_can_switch() {
        assert!(current().is_korean());
        let english = scope(OwnerLocale::english_utc(), async { text("완료", "Done") }).await;
        assert_eq!(english, "Done");
        assert_eq!(text("완료", "Done"), "완료");
    }
}
