//! Session-host durability capability (design rev3 section 3).
//!
//! Classified once at daemon start from measured facts: FERRYX_SESSION_HOST, the job the daemon
//! runs in, and the versioned-copy check. There is no MSIX branch; packaged installs go through
//! the same measurements. The classifier and spawn policy are pure; only [probe_job] touches the
//! OS and exists on Windows alone.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SESSION_HOST_ENV: &str = "FERRYX_SESSION_HOST";
/// JOB_OBJECT_LIMIT_BREAKAWAY_OK.
pub const JOB_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;
/// JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK.
pub const JOB_LIMIT_SILENT_BREAKAWAY_OK: u32 = 0x0000_1000;
/// CREATE_BREAKAWAY_FROM_JOB process creation flag.
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DurableMode {
    NoJob,
    Breakaway,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnavailableReason {
    JobNoBreakaway,
    EnvOff,
    CopyFailed,
}

impl UnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JobNoBreakaway => "job-no-breakaway",
            Self::EnvOff => "env-off",
            Self::CopyFailed => "copy-failed",
        }
    }
}

/// Exposed as DaemonDescribe.sessionHostCapability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum HostCapability {
    Durable { mode: DurableMode },
    Unavailable { reason: UnavailableReason },
}

impl HostCapability {
    /// Extra CreateProcessW flags for the host process, or None when hosts are unavailable.
    pub const fn host_creation_flags(self) -> Option<u32> {
        match self {
            Self::Durable {
                mode: DurableMode::NoJob,
            } => Some(0),
            Self::Durable {
                mode: DurableMode::Breakaway,
            } => Some(CREATE_BREAKAWAY_FROM_JOB),
            Self::Unavailable { .. } => None,
        }
    }
}

/// Parsed FERRYX_SESSION_HOST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostMode {
    Auto,
    Off,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("FERRYX_SESSION_HOST={0:?} is not one of off, required, auto")]
pub struct HostModeError(pub String);

impl HostMode {
    /// Unset or empty is Auto. Any other unrecognized value is an error so a typo such as
    /// "requried" is reported instead of silently running in Auto.
    pub fn parse(value: Option<&str>) -> Result<Self, HostModeError> {
        match value.map(str::trim) {
            None | Some("") => Ok(Self::Auto),
            Some(v) if v.eq_ignore_ascii_case("auto") => Ok(Self::Auto),
            Some(v) if v.eq_ignore_ascii_case("off") => Ok(Self::Off),
            Some(v) if v.eq_ignore_ascii_case("required") => Ok(Self::Required),
            Some(other) => Err(HostModeError(other.to_string())),
        }
    }

    pub fn from_env() -> Result<Self, HostModeError> {
        let value = std::env::var(SESSION_HOST_ENV).ok();
        Self::parse(value.as_deref())
    }
}

/// The daemon process's job membership, as measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobInfo {
    NotInJob,
    /// LimitFlags of the job's extended limit information.
    InJob {
        limit_flags: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JobProbeError {
    #[error("IsProcessInJob failed (os error {0})")]
    IsProcessInJob(i32),
    #[error("QueryInformationJobObject failed (os error {0})")]
    QueryInformation(i32),
}

/// Outcome of the versioned-copy step (temp copy, re-hash, rename, re-hash at startup, one
/// quarantine-and-recopy retry). Only its success matters here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyCheck {
    Verified,
    Failed,
}

/// The pure classifier. Off wins before any measurement; then the job decides; then the copy.
pub fn classify(mode: HostMode, job: JobInfo, copy: CopyCheck) -> HostCapability {
    if mode == HostMode::Off {
        return HostCapability::Unavailable {
            reason: UnavailableReason::EnvOff,
        };
    }
    let durable = match job {
        JobInfo::NotInJob => DurableMode::NoJob,
        JobInfo::InJob { limit_flags }
            if limit_flags & (JOB_LIMIT_BREAKAWAY_OK | JOB_LIMIT_SILENT_BREAKAWAY_OK) != 0 =>
        {
            DurableMode::Breakaway
        }
        JobInfo::InJob { .. } => {
            return HostCapability::Unavailable {
                reason: UnavailableReason::JobNoBreakaway,
            };
        }
    };
    match copy {
        CopyCheck::Verified => HostCapability::Durable { mode: durable },
        CopyCheck::Failed => HostCapability::Unavailable {
            reason: UnavailableReason::CopyFailed,
        },
    }
}

/// Classifier over a raw probe result. A failed probe cannot prove that a host would escape
/// the daemon's job, so it classifies as job-no-breakaway; the caller logs the probe error.
pub fn classify_probe(
    mode: HostMode,
    job: Result<JobInfo, JobProbeError>,
    copy: CopyCheck,
) -> HostCapability {
    match job {
        Ok(job) => classify(mode, job, copy),
        Err(_) if mode == HostMode::Off => classify(mode, JobInfo::NotInJob, copy),
        Err(_) => HostCapability::Unavailable {
            reason: UnavailableReason::JobNoBreakaway,
        },
    }
}

/// Where a new local session is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnRoute {
    /// Spawn a session host with these extra creation flags. A failure here is
    /// SESSION_HOST_SPAWN_FAILED, never a fallback to an in-daemon session.
    Hosted { creation_flags: u32 },
    /// In-daemon legacy ConPTY session, marked durability: legacy so upgrades refuse.
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("SESSION_HOST_UNAVAILABLE: {}", .reason.as_str())]
pub struct SessionHostUnavailable {
    pub reason: UnavailableReason,
}

/// Spawn policy, decided once per capability rather than per spawn.
pub fn spawn_route(
    capability: HostCapability,
    mode: HostMode,
) -> Result<SpawnRoute, SessionHostUnavailable> {
    match (capability, mode) {
        (HostCapability::Durable { .. }, _) => Ok(SpawnRoute::Hosted {
            creation_flags: capability.host_creation_flags().unwrap_or(0),
        }),
        (HostCapability::Unavailable { reason }, HostMode::Required) => {
            Err(SessionHostUnavailable { reason })
        }
        (HostCapability::Unavailable { .. }, HostMode::Auto | HostMode::Off) => {
            Ok(SpawnRoute::Legacy)
        }
    }
}

/// Measures the current process's job. Queries the innermost job only; a denied breakaway
/// from an outer job surfaces later as SESSION_HOST_SPAWN_FAILED.
#[cfg(windows)]
pub fn probe_job() -> Result<JobInfo, JobProbeError> {
    use windows_sys::Win32::System::JobObjects::{
        IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    fn last_os_error() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    let mut in_job = 0;
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no close; a null job handle
    // asks about any job; in_job is a valid out pointer for the duration of the call.
    let ok = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) };
    if ok == 0 {
        return Err(JobProbeError::IsProcessInJob(last_os_error()));
    }
    if in_job == 0 {
        return Ok(JobInfo::NotInJob);
    }
    // SAFETY: JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a plain C struct; all-zero is valid.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(|_| JobProbeError::QueryInformation(0))?;
    // SAFETY: a null job handle means the job of the calling process; the buffer pointer and
    // size describe info exactly; the return-length pointer may be null.
    let ok = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of_mut!(info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(JobProbeError::QueryInformation(last_os_error()));
    }
    Ok(JobInfo::InJob {
        limit_flags: info.BasicLimitInformation.LimitFlags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_BREAKAWAY: JobInfo = JobInfo::InJob {
        limit_flags: 0x2000,
    };

    #[test]
    fn not_in_job_is_durable_no_job() {
        let capability = classify(HostMode::Auto, JobInfo::NotInJob, CopyCheck::Verified);
        assert_eq!(
            capability,
            HostCapability::Durable {
                mode: DurableMode::NoJob
            }
        );
        assert_eq!(capability.host_creation_flags(), Some(0));
    }

    #[test]
    fn job_with_either_breakaway_flag_is_durable_breakaway() {
        for flags in [
            JOB_LIMIT_BREAKAWAY_OK,
            JOB_LIMIT_SILENT_BREAKAWAY_OK,
            JOB_LIMIT_BREAKAWAY_OK | 0x2000,
        ] {
            let capability = classify(
                HostMode::Required,
                JobInfo::InJob { limit_flags: flags },
                CopyCheck::Verified,
            );
            assert_eq!(
                capability,
                HostCapability::Durable {
                    mode: DurableMode::Breakaway
                },
                "{flags:#x}"
            );
            assert_eq!(
                capability.host_creation_flags(),
                Some(CREATE_BREAKAWAY_FROM_JOB)
            );
        }
    }

    #[test]
    fn job_without_breakaway_is_unavailable() {
        let capability = classify(HostMode::Auto, NO_BREAKAWAY, CopyCheck::Verified);
        assert_eq!(
            capability,
            HostCapability::Unavailable {
                reason: UnavailableReason::JobNoBreakaway
            }
        );
        assert_eq!(capability.host_creation_flags(), None);
    }

    #[test]
    fn env_off_wins_over_every_measurement() {
        for job in [JobInfo::NotInJob, NO_BREAKAWAY] {
            for copy in [CopyCheck::Verified, CopyCheck::Failed] {
                assert_eq!(
                    classify(HostMode::Off, job, copy),
                    HostCapability::Unavailable {
                        reason: UnavailableReason::EnvOff
                    }
                );
            }
        }
        let failed_probe = classify_probe(
            HostMode::Off,
            Err(JobProbeError::IsProcessInJob(5)),
            CopyCheck::Verified,
        );
        assert_eq!(
            failed_probe,
            HostCapability::Unavailable {
                reason: UnavailableReason::EnvOff
            }
        );
    }

    #[test]
    fn copy_failure_is_unavailable_only_when_the_job_allows_hosts() {
        assert_eq!(
            classify(HostMode::Auto, JobInfo::NotInJob, CopyCheck::Failed),
            HostCapability::Unavailable {
                reason: UnavailableReason::CopyFailed
            }
        );
        assert_eq!(
            classify(HostMode::Auto, NO_BREAKAWAY, CopyCheck::Failed),
            HostCapability::Unavailable {
                reason: UnavailableReason::JobNoBreakaway
            }
        );
    }

    #[test]
    fn failed_job_probe_is_never_durable() {
        for error in [
            JobProbeError::IsProcessInJob(5),
            JobProbeError::QueryInformation(6),
        ] {
            assert_eq!(
                classify_probe(HostMode::Auto, Err(error), CopyCheck::Verified),
                HostCapability::Unavailable {
                    reason: UnavailableReason::JobNoBreakaway
                }
            );
        }
    }

    #[test]
    fn spawn_route_never_falls_back_when_durable_and_fails_when_required() {
        let durable = HostCapability::Durable {
            mode: DurableMode::Breakaway,
        };
        assert_eq!(
            spawn_route(durable, HostMode::Auto),
            Ok(SpawnRoute::Hosted {
                creation_flags: CREATE_BREAKAWAY_FROM_JOB
            })
        );
        let unavailable = HostCapability::Unavailable {
            reason: UnavailableReason::CopyFailed,
        };
        assert_eq!(
            spawn_route(unavailable, HostMode::Auto),
            Ok(SpawnRoute::Legacy)
        );
        assert_eq!(
            spawn_route(unavailable, HostMode::Off),
            Ok(SpawnRoute::Legacy)
        );
        let refused = spawn_route(unavailable, HostMode::Required).unwrap_err();
        assert_eq!(refused.reason, UnavailableReason::CopyFailed);
        assert_eq!(refused.to_string(), "SESSION_HOST_UNAVAILABLE: copy-failed");
    }

    #[test]
    fn host_mode_parse_accepts_known_values_and_rejects_typos() {
        assert_eq!(HostMode::parse(None), Ok(HostMode::Auto));
        assert_eq!(HostMode::parse(Some(" ")), Ok(HostMode::Auto));
        assert_eq!(HostMode::parse(Some("OFF")), Ok(HostMode::Off));
        assert_eq!(HostMode::parse(Some("required")), Ok(HostMode::Required));
        assert_eq!(
            HostMode::parse(Some("requried")),
            Err(HostModeError("requried".into()))
        );
    }

    #[test]
    fn capability_wire_shape_uses_kebab_case_reasons() {
        let unavailable = serde_json::to_value(HostCapability::Unavailable {
            reason: UnavailableReason::JobNoBreakaway,
        })
        .unwrap();
        assert_eq!(
            unavailable,
            serde_json::json!({"state": "unavailable", "reason": "job-no-breakaway"})
        );
        let durable = serde_json::to_value(HostCapability::Durable {
            mode: DurableMode::NoJob,
        })
        .unwrap();
        assert_eq!(
            durable,
            serde_json::json!({"state": "durable", "mode": "no-job"})
        );
        for reason in [
            UnavailableReason::JobNoBreakaway,
            UnavailableReason::EnvOff,
            UnavailableReason::CopyFailed,
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), reason.as_str());
        }
    }

    #[cfg(windows)]
    #[test]
    fn local_flag_constants_match_windows_sys() {
        use windows_sys::Win32::System::JobObjects::{
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        };
        use windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB as SYS_CREATE_BREAKAWAY_FROM_JOB;
        assert_eq!(JOB_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_BREAKAWAY_OK);
        assert_eq!(
            JOB_LIMIT_SILENT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK
        );
        assert_eq!(CREATE_BREAKAWAY_FROM_JOB, SYS_CREATE_BREAKAWAY_FROM_JOB);
    }

    #[cfg(windows)]
    #[test]
    fn probe_job_classifies_the_test_process() {
        let job = probe_job().expect("job probe succeeds for the current process");
        let capability = classify(HostMode::Auto, job, CopyCheck::Verified);
        assert!(matches!(
            capability,
            HostCapability::Durable { .. }
                | HostCapability::Unavailable {
                    reason: UnavailableReason::JobNoBreakaway
                }
        ));
    }
}
