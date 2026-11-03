//! Pure session health classification (plan task 3, G3, D4).
//!
//! No IO and no clock: the caller passes the session, the terminal tail, the standing rules
//! and `now_ms`. Only the last [`TAIL_LINES`] lines of the tail are inspected, so an error
//! the agent already scrolled past never keeps a session unhealthy.
//!
//! Precedence: quota (429 / rate or usage limit), then upstream 5xx, then stalled, then
//! failed. A stall is a judgment, not a state (D4): provider-reported `working` whose screen
//! has not changed for the `stallRestart` threshold. A screen-sourced `working` is never
//! stalled, because the observer already turns it idle when the screen goes quiet.
//!
//! A quota message without a reset time yields `Quota { reset_at_ms: None }`; the recovery
//! scheduler applies the `quotaResume` fallback, not the classifier. A wall-clock reset such as
//! "resets at 14:05" carries no zone in the tail and is read as the next such time in UTC.
use std::sync::OnceLock;

use regex::Regex;

use super::panel_contract::{FleetSession, HealthClassifier, HealthKind, RuleKind, StandingRule};

/// How many trailing lines of the terminal tail are inspected.
const TAIL_LINES: usize = 40;
/// `stallRestart` threshold when no rule sets one (D2 default, seconds).
const DEFAULT_STALL_SECS: i64 = 300;
const DAY_MS: i64 = 86_400_000;

const QUOTA_MARKERS: [&str; 6] = [
    "rate limit",
    "rate_limit",
    "ratelimit",
    "usage limit",
    "quota",
    "too many requests",
];

const UPSTREAM_MARKERS: [&str; 7] = [
    "bad gateway",
    "service unavailable",
    "gateway timeout",
    "gateway time-out",
    "internal server error",
    "overloaded",
    "connection error",
];

const CRASH_MARKERS: [&str; 8] = [
    "panicked at",
    "segmentation fault",
    "core dumped",
    "fatal error",
    "traceback (most recent call last)",
    "unhandled exception",
    "unhandledrejection",
    "abort trap",
];

/// Classifies sessions from their state, screen timestamps and terminal tail.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionHealthClassifier;

impl HealthClassifier for SessionHealthClassifier {
    fn classify(
        &self,
        session: &FleetSession,
        tail: &str,
        rules: &[StandingRule],
        now_ms: i64,
    ) -> HealthKind {
        let raw: Vec<&str> = tail.lines().rev().take(TAIL_LINES).collect();
        let lines: Vec<String> = raw.iter().map(|line| line.to_ascii_lowercase()).collect();
        if let Some(line) = lines.iter().find(|line| is_quota(line)) {
            return HealthKind::Quota {
                reset_at_ms: reset_at_ms(&lines, line, now_ms),
            };
        }
        if let Some(status) = lines.iter().find_map(|line| upstream_error(line)) {
            return HealthKind::UpstreamError { status };
        }
        if session.state == "working" && session.state_source == "provider" {
            if let Some(changed_at) = session.screen_changed_at_ms {
                let threshold_ms = stall_threshold_secs(rules).saturating_mul(1000);
                if now_ms.saturating_sub(changed_at) >= threshold_ms {
                    return HealthKind::Stalled { since_ms: changed_at };
                }
            }
        }
        if session.state == "unknown" {
            if let Some(at) = lines
                .iter()
                .position(|line| CRASH_MARKERS.iter().any(|marker| line.contains(marker)))
            {
                return HealthKind::Failed {
                    reason: raw[at].trim().chars().take(200).collect(),
                };
            }
        }
        HealthKind::Healthy
    }
}

fn stall_threshold_secs(rules: &[StandingRule]) -> i64 {
    rules
        .iter()
        .find(|rule| rule.kind == RuleKind::StallRestart)
        .and_then(|rule| rule.params.get("thresholdSecs"))
        .and_then(serde_json::Value::as_i64)
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_STALL_SECS)
}

fn cached(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("health regex is valid"))
}

fn is_quota(line: &str) -> bool {
    static HTTP_429: OnceLock<Regex> = OnceLock::new();
    QUOTA_MARKERS.iter().any(|marker| line.contains(marker))
        || cached(&HTTP_429, r"\b429\b").is_match(line)
}

/// `Some(status)` when the line is a proxy/upstream failure; the inner option is the 5xx code
/// when the line names one.
fn upstream_error(line: &str) -> Option<Option<u16>> {
    static API_ERROR: OnceLock<Regex> = OnceLock::new();
    static STATUS: OnceLock<Regex> = OnceLock::new();
    static CODED: OnceLock<Regex> = OnceLock::new();
    static UPSTREAM: OnceLock<Regex> = OnceLock::new();
    let status_in = |text: &str| {
        cached(&STATUS, r"\b(5\d\d)\b")
            .captures(text)
            .and_then(|caps| caps[1].parse::<u16>().ok())
    };
    if let Some(caps) = cached(&API_ERROR, r"api error:?\s*(5\d\d)\b").captures(line) {
        return Some(caps[1].parse::<u16>().ok());
    }
    let marked = UPSTREAM_MARKERS.iter().any(|marker| line.contains(marker))
        || cached(
            &UPSTREAM,
            r"no healthy upstream|upstream (?:connect |request )?(?:error|timeout|failure|unavailable)",
        )
        .is_match(line);
    if marked {
        return Some(status_in(line));
    }
    cached(&CODED, r"(?:http|status|error|code)[ :=]*(5\d\d)\b")
        .captures(line)
        .map(|caps| caps[1].parse::<u16>().ok())
}

/// Reset instant of the quota message: looked up on the quota line first, then the other
/// recent lines (newest first), so "429" on one line and "try again in 3 minutes" on the next
/// still pair up.
fn reset_at_ms(lines: &[String], quota_line: &str, now_ms: i64) -> Option<i64> {
    std::iter::once(quota_line)
        .chain(lines.iter().map(String::as_str))
        .find_map(|line| parse_reset(line, now_ms))
}

/// Reset instant named anywhere in the last [`TAIL_LINES`] lines of `text` (newest line
/// first), or `None`. The same readings as the classifier's quota path.
pub fn reset_at_from_text(text: &str, now_ms: i64) -> Option<i64> {
    text.lines()
        .rev()
        .take(TAIL_LINES)
        .map(str::to_ascii_lowercase)
        .find_map(|line| parse_reset(&line, now_ms))
}

fn parse_reset(line: &str, now_ms: i64) -> Option<i64> {
    static RETRY_AFTER: OnceLock<Regex> = OnceLock::new();
    static RELATIVE: OnceLock<Regex> = OnceLock::new();
    static CLOCK: OnceLock<Regex> = OnceLock::new();
    if let Some(caps) = cached(&RETRY_AFTER, r"retry-after:?\s*(\d+)").captures(line) {
        let secs: i64 = caps[1].parse().ok()?;
        return Some(now_ms.saturating_add(secs.saturating_mul(1000)));
    }
    if let Some(caps) = cached(
        &RELATIVE,
        r"(?:try again|retry|resets?|available again)\s+(?:again\s+)?in\s+(?:about\s+)?(\d+)\s*(seconds?|secs?|minutes?|mins?|hours?|hrs?|s|m|h)\b",
    )
    .captures(line)
    {
        let amount: i64 = caps[1].parse().ok()?;
        let unit_ms: i64 = match caps[2].chars().next()? {
            's' => 1_000,
            'm' => 60_000,
            _ => 3_600_000,
        };
        return Some(now_ms.saturating_add(amount.saturating_mul(unit_ms)));
    }
    let caps = cached(
        &CLOCK,
        r"resets?(?:\s+at)?\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\b",
    )
    .captures(line)?;
    if caps.get(2).is_none() && caps.get(3).is_none() {
        return None;
    }
    let mut hour: i64 = caps[1].parse().ok()?;
    let minute: i64 = caps.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
    if let Some(meridiem) = caps.get(3) {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour %= 12;
        if meridiem.as_str() == "pm" {
            hour += 12;
        }
    }
    if hour > 23 || minute > 59 {
        return None;
    }
    let time_of_day = (hour * 60 + minute) * 60_000;
    let candidate = now_ms.div_euclid(DAY_MS) * DAY_MS + time_of_day;
    Some(if candidate > now_ms {
        candidate
    } else {
        candidate + DAY_MS
    })
}

#[cfg(test)]
mod tests {
    use super::super::panel_contract::{default_rules, SessionRef};
    use super::*;

    /// 2023-11-14 22:13:20 UTC.
    const NOW: i64 = 1_700_000_000_000;
    /// 2023-11-14 00:00:00 UTC.
    const DAY_START: i64 = 1_699_920_000_000;

    fn session(state: &str, source: &str, screen_changed_ago_secs: Option<i64>) -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("mac", "s1"),
            host_id: "mac".into(),
            host_label: "This Mac".into(),
            title: "t".into(),
            cwd: "/work".into(),
            agent: "claude".into(),
            state: state.into(),
            state_source: source.into(),
            state_since_ms: None,
            screen_changed_at_ms: screen_changed_ago_secs.map(|secs| NOW - secs * 1000),
            pending_prompt: None,
            last_assistant_line: None,
            updated_at_ms: NOW,
            stale: false,
        }
    }

    fn classify(session: &FleetSession, tail: &str) -> HealthKind {
        SessionHealthClassifier.classify(session, tail, &default_rules(), NOW)
    }

    #[test]
    fn stall_needs_provider_working_and_the_threshold() {
        let cases = [
            ("working", "provider", 300, true),
            ("working", "provider", 301, true),
            ("working", "provider", 240, false),
            ("working", "screen", 3600, false),
            ("idle", "provider", 3600, false),
            ("blocked", "provider", 3600, false),
        ];
        for (state, source, ago, stalled) in cases {
            let got = classify(&session(state, source, Some(ago)), "thinking...\n");
            let want = if stalled {
                HealthKind::Stalled {
                    since_ms: NOW - ago * 1000,
                }
            } else {
                HealthKind::Healthy
            };
            assert_eq!(got, want, "{state}/{source}/{ago}s");
        }
        assert_eq!(
            classify(&session("working", "provider", None), ""),
            HealthKind::Healthy
        );
    }

    #[test]
    fn stall_threshold_comes_from_the_rule() {
        let mut rules = default_rules();
        let rule = rules
            .iter_mut()
            .find(|rule| rule.kind == RuleKind::StallRestart)
            .unwrap();
        rule.params = serde_json::json!({ "thresholdSecs": 60, "maxRestarts": 1 });
        let working = session("working", "provider", Some(90));
        assert_eq!(
            SessionHealthClassifier.classify(&working, "", &rules, NOW),
            HealthKind::Stalled {
                since_ms: NOW - 90_000
            }
        );
        assert_eq!(
            SessionHealthClassifier.classify(&working, "", &[], NOW),
            HealthKind::Healthy
        );
    }

    #[test]
    fn quota_reads_the_reset_time_when_present() {
        let idle = session("idle", "provider", None);
        let cases = [
            (
                "API Error: 429 rate limit exceeded, try again in 3 minutes",
                Some(NOW + 180_000),
            ),
            ("429 Too Many Requests\nretry-after: 120", Some(NOW + 120_000)),
            ("Claude usage limit reached. Retry in 30 seconds", Some(NOW + 30_000)),
            ("rate limit hit, try again in 2 hours", Some(NOW + 7_200_000)),
            (
                "Usage limit reached, resets at 14:05",
                Some(DAY_START + DAY_MS + 50_700_000),
            ),
            (
                "Usage limit reached, resets at 11pm",
                Some(DAY_START + 23 * 3_600_000),
            ),
            ("API Error: 429 rate limit exceeded", None),
            ("You exceeded your current quota", None),
        ];
        for (tail, reset_at_ms) in cases {
            assert_eq!(
                classify(&idle, tail),
                HealthKind::Quota { reset_at_ms },
                "{tail}"
            );
        }
    }

    #[test]
    fn reset_at_from_text_reads_the_newest_reset_line() {
        assert_eq!(
            reset_at_from_text("Quota hit. Try again in 3 minutes\n", NOW),
            Some(NOW + 180_000)
        );
        assert_eq!(
            reset_at_from_text("retry-after: 120\nTry again in 1 hour", NOW),
            Some(NOW + 3_600_000)
        );
        assert_eq!(reset_at_from_text("quota exceeded\nno time given", NOW), None);
        assert_eq!(reset_at_from_text("", NOW), None);
    }

    #[test]
    fn upstream_errors_carry_the_status() {
        let idle = session("idle", "provider", None);
        let cases = [
            ("502 Bad Gateway", Some(502)),
            ("API Error: 503 {\"error\":\"overloaded\"}", Some(503)),
            ("<html>503 Service Unavailable</html>", Some(503)),
            ("HTTP 504 gateway timeout from proxy", Some(504)),
            ("Overloaded, please retry", None),
            ("API Error: Connection error.", None),
            ("upstream connect error or disconnect/reset before headers", None),
            ("request failed with status 500", Some(500)),
        ];
        for (tail, status) in cases {
            assert_eq!(
                classify(&idle, tail),
                HealthKind::UpstreamError { status },
                "{tail}"
            );
        }
    }

    #[test]
    fn quota_wins_over_upstream_and_only_the_recent_tail_counts() {
        let idle = session("idle", "provider", None);
        assert_eq!(
            classify(&idle, "502 Bad Gateway\n429 rate limit"),
            HealthKind::Quota { reset_at_ms: None }
        );
        let mut old_error = String::from("502 Bad Gateway\n");
        old_error.push_str(&"working on it\n".repeat(40));
        assert_eq!(classify(&idle, &old_error), HealthKind::Healthy);
        assert_eq!(classify(&idle, "ran 500 tests, all ok"), HealthKind::Healthy);
        assert_eq!(classify(&idle, "git push upstream main"), HealthKind::Healthy);
    }

    #[test]
    fn failed_only_for_unknown_state_with_a_crash_marker() {
        let crash = "thread 'main' panicked at src/lib.rs:1:1";
        assert_eq!(
            classify(&session("unknown", "screen", None), crash),
            HealthKind::Failed {
                reason: crash.into()
            }
        );
        assert_eq!(
            classify(&session("unknown", "screen", None), "$ "),
            HealthKind::Healthy
        );
        assert_eq!(
            classify(&session("idle", "screen", None), crash),
            HealthKind::Healthy
        );
    }
}
