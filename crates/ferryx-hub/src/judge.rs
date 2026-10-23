//! typesafe-judge, then the omo fallback. `TYPESAFE_*` comes from hub config, never `~/.zshenv`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{NaiveDate, Utc};
use sqlx::SqlitePool;

use serde::{Deserialize, Serialize};

use crate::error::{HubError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    DoneDiff,
    OwnerIntent,
    EventWake,
    KibitzerWake,
    InboxClass,
    /// omo-gateway cron remediation: the root cause of a failed cron job.
    CronCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub selected: String,
    pub is_fallback: bool,
}

impl Choice {
    pub fn new(selected: impl Into<String>) -> Self {
        Self {
            selected: selected.into(),
            is_fallback: false,
        }
    }

    pub fn fallback(selected: impl Into<String>) -> Self {
        Self {
            selected: selected.into(),
            is_fallback: true,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.selected
    }
}

/// Replaces a home directory and obvious token shapes before text leaves the machine.
pub fn scrub_for_external(input: &str) -> String {
    let mut scrubbed = input.to_owned();
    if let Ok(home) = crate::hub_env::var("HOME") {
        if !home.is_empty() {
            scrubbed = scrubbed.replace(&home, "~");
        }
    }
    if let Ok(pattern) = regex::Regex::new(r"(?i)\b(sk-|ghp_|xox[baprs]-)[A-Za-z0-9_\-]{8,}") {
        scrubbed = pattern.replace_all(&scrubbed, "[redacted]").into_owned();
    }
    scrubbed.chars().take(4000).collect()
}

pub fn typesafe_env(config: &BTreeMap<String, String>) -> Vec<(String, String)> {
    const KEYS: [&str; 3] = ["TYPESAFE_API_KEY", "TYPESAFE_URL", "TYPESAFE_MODEL"];
    KEYS.iter()
        .filter_map(|key| {
            config
                .get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect()
}

pub fn typesafe_from_env() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for key in ["TYPESAFE_API_KEY", "TYPESAFE_URL", "TYPESAFE_MODEL"] {
        if let Ok(value) = crate::hub_env::var(key) {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                map.insert(key.to_owned(), value);
            }
        }
    }
    map
}

pub fn default_judge_bin() -> PathBuf {
    let on_path = std::env::split_paths(crate::child_env::child_path())
        .any(|dir| dir.join("typesafe-judge").is_file());
    if !on_path {
        if let Some(home) = crate::hub_env::var_os("HOME") {
            let cargo = Path::new(&home).join(".cargo/bin/typesafe-judge");
            if cargo.is_file() {
                return cargo;
            }
        }
    }
    PathBuf::from("typesafe-judge")
}

/// omo's judge circuit breaker: this many auth/rate-limit failures in a row open it.
pub const BREAKER_FAILURES: u32 = 5;
/// How long an open breaker skips typesafe-judge.
pub const BREAKER_OPEN: Duration = Duration::from_secs(30 * 60);

/// omo-gateway `JevState`'s breaker half. Calls go to one judge account, so the hub keeps
/// one breaker for every `Judge`.
#[derive(Debug, Default)]
pub struct Breaker {
    consecutive_failures: u32,
    open_until: Option<Instant>,
    last_alert_day: Option<NaiveDate>,
}

impl Breaker {
    /// The time the breaker stays open until, closing (and resetting) it once that passed.
    pub fn open_until(&mut self, now: Instant) -> Option<Instant> {
        match self.open_until {
            Some(until) if now < until => Some(until),
            Some(_) => {
                self.open_until = None;
                self.consecutive_failures = 0;
                None
            }
            None => None,
        }
    }

    /// Records a failed call; returns the owner alert when this failure opened the breaker
    /// (at most one alert per KST day, as omo).
    pub fn failure(
        &mut self,
        trigger: bool,
        reason: &str,
        now: Instant,
        today: NaiveDate,
    ) -> Option<String> {
        // A non-trigger failure neither opens the breaker nor resets the streak.
        if !trigger {
            return None;
        }
        self.consecutive_failures += 1;
        if self.consecutive_failures < BREAKER_FAILURES || self.open_until.is_some() {
            return None;
        }
        self.open_until = Some(now + BREAKER_OPEN);
        if self.last_alert_day == Some(today) {
            return None;
        }
        self.last_alert_day = Some(today);
        Some(format!(
            "판정기(typesafe-judge) 차단: {BREAKER_FAILURES}회 연속 인증/요청한도 실패로 30분간 사용 중지. 마지막 오류: {}",
            scrub_for_external(reason).chars().take(200).collect::<String>()
        ))
    }

    pub fn success(&mut self) {
        self.consecutive_failures = 0;
    }
}

/// omo's `is_breaker_trigger`: HTTP 401/429 as a whole token, or an auth/rate-limit message.
pub fn is_breaker_trigger(text: &str) -> bool {
    static TRIGGER: OnceLock<regex::Regex> = OnceLock::new();
    TRIGGER
        .get_or_init(|| {
            regex::Regex::new(r"(?i)\b(?:401|429)\b|unauthorized|rate[ _-]?limit")
                .expect("valid regex")
        })
        .is_match(text)
}

fn breaker() -> &'static Mutex<Breaker> {
    static BREAKER: OnceLock<Mutex<Breaker>> = OnceLock::new();
    BREAKER.get_or_init(|| Mutex::new(Breaker::default()))
}

static ALERTS: OnceLock<(SqlitePool, String)> = OnceLock::new();

/// Lets an opening breaker reach the owner's DM (omo's `OwnerAlerter`).
pub fn install_alerts(pool: SqlitePool, owner_session: String) {
    let _ = ALERTS.set((pool, owner_session));
}

fn kst_today() -> NaiveDate {
    (Utc::now() + chrono::Duration::hours(9)).date_naive()
}

#[derive(Clone)]
pub struct Judge {
    typesafe: BTreeMap<String, String>,
    /// When set, a missing binary uses this choice instead of spawning a model.
    offline_fallback: Option<String>,
    /// Skips the binary. Tests use this so a machine with typesafe-judge installed
    /// still returns the scripted choice.
    forced: Option<String>,
}

impl Judge {
    pub fn from_config(typesafe: BTreeMap<String, String>) -> Self {
        Self {
            typesafe,
            offline_fallback: None,
            forced: None,
        }
    }

    pub fn forced(selected: impl Into<String>) -> Self {
        Self {
            typesafe: BTreeMap::new(),
            offline_fallback: None,
            forced: Some(selected.into()),
        }
    }

    /// Offline fallback used when typesafe-judge is not installed. Tests and a hub
    /// with no judge binary still settle a claim whose signals already passed.
    pub fn installed() -> bool {
        default_judge_bin().is_file()
    }

    pub fn fallback_yes() -> Self {
        Self {
            typesafe: BTreeMap::new(),
            offline_fallback: Some("yes".into()),
            forced: None,
        }
    }

    pub async fn choice(
        &self,
        _purpose: Purpose,
        state: &str,
        instructions: &str,
        options: &[&str],
    ) -> Result<Choice> {
        if let Some(selected) = &self.forced {
            if options.contains(&selected.as_str()) {
                return Ok(Choice::new(selected.clone()));
            }
            return Err(HubError::message(format!(
                "forced judge choice {selected} is not an option"
            )));
        }
        let open = breaker()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .open_until(Instant::now());
        let result = match open {
            Some(until) => Err(HubError::message(format!(
                "judge circuit breaker open for {}s more",
                until.saturating_duration_since(Instant::now()).as_secs()
            ))),
            None => {
                let result = self.run_typesafe(state, instructions, options).await;
                self.record(&result).await;
                result
            }
        };
        match result {
            Ok(choice) => Ok(choice),
            Err(error) => {
                if let Some(selected) = &self.offline_fallback {
                    if options.contains(&selected.as_str()) {
                        return Ok(Choice::fallback(selected.clone()));
                    }
                }
                Err(error)
            }
        }
    }

    async fn record(&self, result: &Result<Choice>) {
        let alert = {
            let mut breaker = breaker()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            match result {
                Ok(_) => {
                    breaker.success();
                    None
                }
                Err(error) => {
                    let reason = error.to_string();
                    breaker.failure(
                        is_breaker_trigger(&reason),
                        &reason,
                        Instant::now(),
                        kst_today(),
                    )
                }
            }
        };
        if let Some(message) = alert {
            // Without a pool the opening would be invisible; always log it.
            tracing::warn!(alert = %message, "judge circuit breaker opened");
            if let Some((pool, owner)) = ALERTS.get() {
                if let Err(error) = crate::owner_alert::notify_owner(pool, owner, &message).await {
                    tracing::warn!(%error, "judge breaker alert was not sent");
                }
            }
        }
    }

    async fn run_typesafe(
        &self,
        state: &str,
        instructions: &str,
        options: &[&str],
    ) -> Result<Choice> {
        let bin = default_judge_bin();
        let mut command = crate::child_env::command(&bin);
        // typesafe-judge refuses to decide without a state ("No state provided. Supply
        // --state, --file, or pipe content through stdin."), and `output()` gives the child no
        // stdin, so every call used to fail with empty stdout ("typesafe-judge declined: ").
        let state = scrub_for_external(state);
        let state = if state.trim().is_empty() {
            "(no state)".to_owned()
        } else {
            state
        };
        command
            .arg("choice")
            .arg("--state")
            .arg(state)
            .arg("-i")
            .arg(scrub_for_external(instructions))
            .arg("-q");
        for option in options {
            command.arg("-o").arg(*option);
        }
        for (key, value) in typesafe_env(&self.typesafe) {
            command.env(key, value);
        }
        let output = tokio::time::timeout(Duration::from_secs(5), command.output())
            .await
            .map_err(|_| HubError::message("typesafe-judge timed out"))?
            .map_err(|error| HubError::message(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if output.status.success() && options.contains(&stdout.as_str()) {
            Ok(Choice::new(stdout))
        } else {
            let stderr: String = String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(200)
                .collect();
            Err(HubError::message(format!(
                "typesafe-judge declined: {stdout} ({}; {stderr})",
                output.status
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesafe_env_comes_from_config_and_does_not_read_a_shell_file() {
        let mut config = BTreeMap::new();
        config.insert("TYPESAFE_API_KEY".into(), "from-config".into());
        config.insert("UNRELATED".into(), "no".into());
        let env = typesafe_env(&config);
        assert_eq!(env, vec![("TYPESAFE_API_KEY".into(), "from-config".into())]);
        let scrubbed = scrub_for_external("token sk-abcdefghij123456");
        assert!(!scrubbed.contains("sk-abcdefghij123456"));
    }

    #[test]
    fn five_auth_or_rate_limit_failures_open_the_breaker_for_thirty_minutes() {
        let mut breaker = Breaker::default();
        let start = Instant::now();
        let day = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        assert!(is_breaker_trigger("HTTP 429 Too Many Requests"));
        assert!(is_breaker_trigger("Unauthorized"));
        assert!(!is_breaker_trigger("request id 4291 on port 14010"));
        for _ in 0..4 {
            assert_eq!(breaker.failure(true, "429", start, day), None);
        }
        // Other failures neither open it nor reset the streak.
        assert_eq!(breaker.failure(false, "timeout", start, day), None);
        assert_eq!(breaker.open_until(start), None);
        let alert = breaker
            .failure(true, "401 Unauthorized", start, day)
            .unwrap();
        assert!(alert.contains("30분"));
        assert_eq!(breaker.open_until(start), Some(start + BREAKER_OPEN));
        // It closes after 30 minutes; reopening the same KST day does not alert again.
        let later = start + BREAKER_OPEN + Duration::from_secs(1);
        assert_eq!(breaker.open_until(later), None);
        for _ in 0..5 {
            assert_eq!(breaker.failure(true, "429", later, day), None);
        }
        assert!(breaker.open_until(later).is_some());
        // A success resets the streak.
        let mut fresh = Breaker::default();
        for _ in 0..4 {
            fresh.failure(true, "429", start, day);
        }
        fresh.success();
        assert_eq!(fresh.failure(true, "429", start, day), None);
    }

    #[tokio::test]
    async fn the_offline_fallback_is_marked_as_fallback() {
        let judge = Judge::fallback_yes();
        let choice = judge
            .choice(Purpose::DoneDiff, "evidence", "done?", &["yes", "no"])
            .await
            .unwrap();
        assert!(choice.is_fallback);
        assert_eq!(choice.as_str(), "yes");
    }
}
