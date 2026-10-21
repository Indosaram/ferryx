//! Owner-intent nudge gate. The gateway's model loop is not ported.
//! This keeps the KST quiet window and the 30 minute spacing between nudges.
use chrono::{DateTime, TimeDelta, Timelike, Utc};

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
}
