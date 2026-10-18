//! Non-destructive daemon owner admission.
//!
//! Every start site asks classify_owner before it starts a daemon. Only an
//! Absent owner may be started; an Alive or Unknown owner is never reloaded,
//! replaced, or killed.

use serde::{Deserialize, Serialize};

use super::client::DAEMON_READY_TIMEOUT;

/// Classification of the current daemon owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerState {
    Alive { pid: u32, start_time: u64 },
    Unknown { reason: String },
    Absent,
}

/// Combined state of the persistent and the legacy daemon lock.
/// Held if either lock is held, Free if both are free, Error otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockState {
    Held,
    Free,
    Error,
}

/// What the daemon endpoint does when a client connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endpoint {
    /// Any frame came back, including a protocol mismatch or DAEMON_RETIRED.
    Answers,
    /// The connection succeeded but no frame arrived within the probe window.
    Silent,
    /// Connection refused, or no socket / port file exists.
    Missing,
}

/// <runtime_dir>/owner.json, written by the daemon after it holds its locks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OwnerRecord {
    pub pid: u32,
    pub start_time: u64,
    pub generation_key: String,
    pub epoch: u64,
}

/// <runtime_dir>/starting.json, written by a launcher before it issues a start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartingRecord {
    pub launcher_pid: u32,
    pub launcher_start_time: u64,
    pub generation_key: String,
    pub started_at_unix_ms: u64,
}

/// Signals used by classify_owner. Injected so tests need no real daemon.
pub(crate) trait OwnerProbe {
    fn lock_state(&self) -> LockState;
    /// Ok(None) when owner.json does not exist; Err when it cannot be read or parsed.
    fn read_record(&self) -> Result<Option<OwnerRecord>, String>;
    /// Start time of a live process, or None when the process is dead or its
    /// start time cannot be read.
    fn process_start_time(&self, pid: u32) -> Option<u64>;
    fn endpoint(&self) -> Result<Endpoint, String>;
    /// Ok(None) when starting.json does not exist; Err when it cannot be read or parsed.
    fn read_starting(&self) -> Result<Option<StartingRecord>, String>;
    fn now_unix_ms(&self) -> u64;
}

/// A starting.json younger than this means a start is still pending.
pub(crate) fn starting_window_ms() -> u64 {
    (DAEMON_READY_TIMEOUT.as_millis() as u64).saturating_mul(2)
}

fn record_owner_alive(probe: &dyn OwnerProbe, record: &OwnerRecord) -> bool {
    probe.process_start_time(record.pid) == Some(record.start_time)
}

/// Classify the daemon owner. First matching row wins:
///
/// 1. lock Error or any probe error -> Unknown
/// 2. Held + record pid alive with equal start time -> Alive
/// 3. Held otherwise -> Unknown
/// 4. Free + endpoint Answers or Silent -> Unknown
/// 5. Free + Missing + record pid alive with equal start time -> Unknown
/// 5b. Free + Missing + starting.json younger than 2 x DAEMON_READY_TIMEOUT -> Unknown
/// 6. Free + Missing otherwise (no record, dead pid, different start time) -> Absent
pub(crate) fn classify_owner(probe: &dyn OwnerProbe) -> OwnerState {
    // Row 1.
    let lock = probe.lock_state();
    if lock == LockState::Error {
        return OwnerState::Unknown {
            reason: "daemon lock state could not be determined".to_string(),
        };
    }
    let record = match probe.read_record() {
        Ok(record) => record,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("owner record unreadable: {error}"),
            }
        }
    };
    let endpoint = match probe.endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("endpoint probe failed: {error}"),
            }
        }
    };
    let starting = match probe.read_starting() {
        Ok(starting) => starting,
        Err(error) => {
            return OwnerState::Unknown {
                reason: format!("starting record unreadable: {error}"),
            }
        }
    };

    let alive_record = record
        .as_ref()
        .filter(|record| record_owner_alive(probe, record));

    if lock == LockState::Held {
        // Row 2.
        if let Some(record) = alive_record {
            return OwnerState::Alive {
                pid: record.pid,
                start_time: record.start_time,
            };
        }
        // Row 3.
        return OwnerState::Unknown {
            reason: "daemon lock is held but its holder cannot be identified".to_string(),
        };
    }

    // Row 4.
    if endpoint != Endpoint::Missing {
        return OwnerState::Unknown {
            reason: format!("daemon lock is free but the endpoint is {endpoint:?}"),
        };
    }
    // Row 5.
    if let Some(record) = alive_record {
        return OwnerState::Unknown {
            reason: format!(
                "recorded owner pid {} is alive without lock or endpoint",
                record.pid
            ),
        };
    }
    // Row 5b.
    if let Some(starting) = starting {
        let age_ms = probe
            .now_unix_ms()
            .saturating_sub(starting.started_at_unix_ms);
        if age_ms < starting_window_ms() {
            return OwnerState::Unknown {
                reason: format!(
                    "a start by launcher pid {} is pending ({age_ms} ms old)",
                    starting.launcher_pid
                ),
            };
        }
    }
    // Row 6.
    OwnerState::Absent
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const NOW_MS: u64 = 1_000_000_000;

    struct FakeProbe {
        lock: LockState,
        record: Result<Option<OwnerRecord>, String>,
        processes: HashMap<u32, u64>,
        endpoint: Result<Endpoint, String>,
        starting: Result<Option<StartingRecord>, String>,
        now_ms: u64,
    }

    impl FakeProbe {
        fn new(lock: LockState, endpoint: Endpoint) -> Self {
            Self {
                lock,
                record: Ok(None),
                processes: HashMap::new(),
                endpoint: Ok(endpoint),
                starting: Ok(None),
                now_ms: NOW_MS,
            }
        }

        fn with_record(mut self, pid: u32, start_time: u64) -> Self {
            self.record = Ok(Some(OwnerRecord {
                pid,
                start_time,
                generation_key: "gen-a".to_string(),
                epoch: 1,
            }));
            self
        }

        fn with_process(mut self, pid: u32, start_time: u64) -> Self {
            self.processes.insert(pid, start_time);
            self
        }

        fn with_starting_age_ms(mut self, age_ms: u64) -> Self {
            self.starting = Ok(Some(StartingRecord {
                launcher_pid: 4242,
                launcher_start_time: 7,
                generation_key: "gen-b".to_string(),
                started_at_unix_ms: self.now_ms - age_ms,
            }));
            self
        }
    }

    impl OwnerProbe for FakeProbe {
        fn lock_state(&self) -> LockState {
            self.lock
        }
        fn read_record(&self) -> Result<Option<OwnerRecord>, String> {
            self.record.clone()
        }
        fn process_start_time(&self, pid: u32) -> Option<u64> {
            self.processes.get(&pid).copied()
        }
        fn endpoint(&self) -> Result<Endpoint, String> {
            self.endpoint.clone()
        }
        fn read_starting(&self) -> Result<Option<StartingRecord>, String> {
            self.starting.clone()
        }
        fn now_unix_ms(&self) -> u64 {
            self.now_ms
        }
    }

    fn is_unknown(state: &OwnerState) -> bool {
        matches!(state, OwnerState::Unknown { .. })
    }

    #[test]
    fn row1_lock_error_is_unknown_even_with_alive_record() {
        let probe = FakeProbe::new(LockState::Error, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row1_probe_error_is_unknown() {
        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.record = Err("corrupt owner.json".to_string());
        assert!(is_unknown(&classify_owner(&probe)));

        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.endpoint = Err("permission denied".to_string());
        assert!(is_unknown(&classify_owner(&probe)));

        let mut probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        probe.starting = Err("corrupt starting.json".to_string());
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row2_held_with_matching_alive_record_is_alive() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 100);
        assert_eq!(
            classify_owner(&probe),
            OwnerState::Alive {
                pid: 10,
                start_time: 100
            }
        );
    }

    #[test]
    fn row3_held_without_record_is_unknown() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Silent);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row3_held_with_reused_pid_is_unknown() {
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers)
            .with_record(10, 100)
            .with_process(10, 999);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row4_free_lock_with_answering_endpoint_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Answers);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row4_free_lock_with_silent_endpoint_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Silent);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5_free_missing_with_alive_recorded_owner_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_process(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5b_recent_starting_record_is_unknown() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_starting_age_ms(starting_window_ms() - 1);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row5b_is_evaluated_before_row6_even_with_dead_record() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_starting_age_ms(0);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn row6_free_missing_without_record_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn row6_free_missing_with_dead_recorded_pid_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing).with_record(10, 100);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn d3_stale_starting_record_is_ignored_and_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_starting_age_ms(starting_window_ms());
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }

    #[test]
    fn b_lock_held_with_unreadable_holder_pid_is_unknown() {
        // The record names pid 10 but its start time cannot be read.
        let probe = FakeProbe::new(LockState::Held, Endpoint::Answers).with_record(10, 100);
        assert!(is_unknown(&classify_owner(&probe)));
    }

    #[test]
    fn c_alive_pid_with_different_start_time_and_free_lock_is_absent() {
        let probe = FakeProbe::new(LockState::Free, Endpoint::Missing)
            .with_record(10, 100)
            .with_process(10, 555);
        assert_eq!(classify_owner(&probe), OwnerState::Absent);
    }
}
