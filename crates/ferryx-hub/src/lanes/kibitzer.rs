//! Owner-intent nudge gate. Quiet hours, spacing, the daily wake cap, and the
//! strict decision JSON are the gateway's rules. A model turn runs only after
//! `next_due` says the generation is armed; this module does not poll.
use chrono::{DateTime, TimeDelta, Timelike, Utc};
use serde::{Deserialize, Serialize};

const SPACING: TimeDelta = TimeDelta::minutes(30);
const KST: TimeDelta = TimeDelta::hours(9);

#[derive(Debug, Clone, Copy)]
pub struct NudgeClock {
    pub quiet_start: u32,
    pub quiet_end: u32,
}

impl Default for NudgeClock {
    fn default() -> Self {
        Self {
            quiet_start: 0,
            quiet_end: 8,
        }
    }
}

impl NudgeClock {
    pub fn allows(self, now: DateTime<Utc>, last_nudge: Option<DateTime<Utc>>) -> bool {
        let hour = (now + KST).hour();
        let quiet = if self.quiet_start <= self.quiet_end {
            (self.quiet_start..self.quiet_end).contains(&hour)
        } else {
            hour >= self.quiet_start || hour < self.quiet_end
        };
        if quiet {
            return false;
        }
        match last_nudge {
            Some(last) => now.signed_duration_since(last) >= SPACING,
            None => true,
        }
    }
}

/// Records a nudge when the clock allows one. The caller invokes this from an idle
/// agent event, not from a timer.
pub fn take_nudge(clock: NudgeClock, now: DateTime<Utc>, last: &mut Option<DateTime<Utc>>) -> bool {
    if clock.allows(now, *last) {
        *last = Some(now);
        true
    } else {
        false
    }
}

pub fn take_nudge_now(last: &mut Option<DateTime<Utc>>) -> bool {
    take_nudge(NudgeClock::default(), Utc::now(), last)
}

#[derive(Debug, Clone, Copy)]
pub struct ArmState {
    pub generation: i64,
    pub consumed: i64,
    pub admitted: Option<DateTime<Utc>>,
    pub wakes_today: i64,
    pub enabled: bool,
}

/// `None` means unarmed. No timer is installed in that state.
pub fn next_due(now: DateTime<Utc>, state: &ArmState) -> Option<DateTime<Utc>> {
    if !state.enabled || state.generation == state.consumed {
        return None;
    }
    let mut due = now;
    if let Some(last) = state.admitted {
        due = due.max(last + SPACING);
    }
    if state.wakes_today >= 6 {
        due = due.max(day_start(now) + TimeDelta::days(1));
    }
    if (due + KST).hour() < 8 {
        due = day_start(due) + TimeDelta::hours(8);
    }
    Some(due)
}

fn day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now + KST;
    local
        - TimeDelta::seconds(i64::from(local.num_seconds_from_midnight()))
        - TimeDelta::nanoseconds(i64::from(local.nanosecond()))
        - KST
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct TargetLane(pub Option<String>);

impl<'de> Deserialize<'de> for TargetLane {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        serde_json::from_value(value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub wake: bool,
    pub reason: String,
    pub suggested: String,
    pub target_lane: TargetLane,
}

pub fn parse_decision(raw: &str) -> Result<Decision, serde_json::Error> {
    serde_json::from_str(strip_code_fence(raw.trim()))
}

fn strip_code_fence(text: &str) -> &str {
    let Some(inner) = text
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
    else {
        return text;
    };
    let inner = inner.strip_prefix("json").unwrap_or(inner);
    inner.trim()
}

pub async fn ensure_schema(pool: &sqlx::SqlitePool) -> crate::error::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS kibitzer_state (
            lead TEXT PRIMARY KEY,
            generation INTEGER NOT NULL DEFAULT 0,
            consumed INTEGER NOT NULL DEFAULT 0,
            admitted_ms INTEGER
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_hours_and_spacing_both_hold_a_nudge() {
        let clock = NudgeClock::default();
        let quiet = DateTime::parse_from_rfc3339("2026-10-21T16:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!clock.allows(quiet, None));
        let day = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(clock.allows(day, None));
        let recent = day - TimeDelta::minutes(10);
        assert!(!clock.allows(day, Some(recent)));
        assert!(clock.allows(day, Some(day - TimeDelta::minutes(30))));
    }

    #[test]
    fn an_unarmed_generation_has_no_due_time_and_the_seventh_wake_waits() {
        let now = DateTime::parse_from_rfc3339("2026-10-21T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let idle = ArmState {
            generation: 1,
            consumed: 1,
            admitted: None,
            wakes_today: 0,
            enabled: true,
        };
        assert!(next_due(now, &idle).is_none());
        let armed = ArmState {
            consumed: 0,
            wakes_today: 6,
            ..idle
        };
        let due = next_due(now, &armed).unwrap();
        assert!(due > now);
        let parsed = parse_decision("```json{\"wake\":true,\"reason\":\"ask\",\"suggested\":\"ping\",\"target_lane\":null}```").unwrap();
        assert!(parsed.wake);
        assert!(parse_decision("{\"wake\":true,\"reason\":\"x\",\"suggested\":\"y\"}").is_err());
    }
}
