//! Supervisor logic for streaming the Windows secure desktop (logon screen,
//! lock screen, UAC consent). Kept target-independent so it is unit-tested on
//! every platform.

use std::path::PathBuf;
use std::time::Duration;

/// A desktop of `WinSta0`. `Winlogon` is the secure desktop: a worker bound to
/// `Default` loses DXGI access the moment Windows switches to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopKind {
    Winlogon,
    Default,
    Screensaver,
    Other,
}

impl DesktopKind {
    pub fn startup_desktop(self) -> &'static str {
        match self {
            Self::Winlogon => "WinSta0\\Winlogon",
            Self::Screensaver => "WinSta0\\Screen-saver",
            Self::Default | Self::Other => "WinSta0\\Default",
        }
    }
}

/// Classifies a `GetUserObjectInformationW(UOI_NAME)` desktop name.
/// Windows desktop names are case-insensitive.
pub fn desktop_kind(name: &str) -> DesktopKind {
    if name.eq_ignore_ascii_case("winlogon") {
        DesktopKind::Winlogon
    } else if name.eq_ignore_ascii_case("default") {
        DesktopKind::Default
    } else if name.eq_ignore_ascii_case("screen-saver") || name.eq_ignore_ascii_case("screensaver")
    {
        DesktopKind::Screensaver
    } else {
        DesktopKind::Other
    }
}

/// Maps the worker-published input-desktop hint to the console desktop.
/// The hint is the ONLY reliable session-1 desktop source: `OpenInputDesktop`
/// from the service's session 0 describes session 0, not the console, and can
/// block for the whole pre-logon window. When no live worker has published a
/// hint yet - the pre-logon state - the console sits on the secure desktop, so
/// `Winlogon` is the safe assumption; the worker corrects it within 500 ms and
/// the supervisor respawns onto the real desktop.
pub fn console_desktop_from_hint(hint: Option<&str>) -> DesktopKind {
    match hint {
        Some(name) if !name.trim().is_empty() => desktop_kind(name),
        _ => DesktopKind::Winlogon,
    }
}

/// Decides whether a session worker whose server accepts TCP but never
/// completes a TLS handshake should recycle itself so the supervisor respawns a
/// fresh worker. A wedged worker from desktop churn accepts connections
/// indefinitely while no handshake ever succeeds; a healthy idle worker simply
/// has no accepts at all, so it never recycles.
pub fn worker_should_recycle(
    accepted: u32,
    successful_handshakes: u32,
    uptime: Duration,
    min_uptime: Duration,
) -> bool {
    uptime >= min_uptime && accepted >= 3 && successful_handshakes == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleState {
    /// `None` when `WTSGetActiveConsoleSessionId` reports `0xFFFF_FFFF`, which
    /// happens while no session is attached to the console.
    pub session_id: Option<u32>,
    pub desktop: DesktopKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerState {
    pub session_id: u32,
    pub desktop: DesktopKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorAction {
    Idle,
    Spawn {
        session_id: u32,
        desktop: DesktopKind,
    },
    Respawn {
        session_id: u32,
        desktop: DesktopKind,
    },
    StopWorker,
}

pub fn decide_action(console: ConsoleState, worker: Option<WorkerState>) -> SupervisorAction {
    let Some(session_id) = console.session_id else {
        return SupervisorAction::StopWorker;
    };
    let desktop = console.desktop;
    match worker {
        None => SupervisorAction::Spawn {
            session_id,
            desktop,
        },
        Some(worker) if worker.session_id == session_id && worker.desktop == desktop => {
            SupervisorAction::Idle
        }
        Some(_) => SupervisorAction::Respawn {
            session_id,
            desktop,
        },
    }
}

/// Machine-wide pairing store: `%ProgramData%\MahoRD\host-authorizations.json`.
///
/// A LocalSystem process resolves the per-user data directory to
/// `C:\Windows\System32\config\systemprofile`, so service mode cannot use
/// `PairingStore::default_path` without forcing paired clients back to PIN.
pub fn service_store_path(program_data: &str) -> PathBuf {
    PathBuf::from(program_data)
        .join("MahoRD")
        .join("host-authorizations.json")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureFailure {
    AccessLost,
    /// What a switch to the secure desktop looks like to DXGI.
    AccessDenied,
    RefreshFailure,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureRecovery {
    Reacquire(Duration),
    Abort,
}

/// Paths the service must re-secure at startup.
///
/// Tightening the directory does not rewrite children that already inherited
/// the permissive `%ProgramData%` ACL, so a credential file created before
/// hardening stays readable by `BUILTIN\Users`. Both the directory and the store
/// file are therefore re-applied every time.
pub fn service_store_hardening_targets(program_data: &str) -> Vec<PathBuf> {
    let directory = PathBuf::from(program_data).join("MahoRD");
    let store = service_store_path(program_data);
    vec![directory, store]
}

/// SDDL for the machine-wide MahoRD directory.
///
/// `host-authorizations.json` holds the pre-shared keys that let a paired client
/// drive this machine, and anything created under `%ProgramData%` inherits an
/// ACL granting `BUILTIN\Users` read. `P` cuts that inheritance so the
/// permissive default cannot flow back in, leaving only SYSTEM and
/// Administrators, both inheriting to the files inside.
pub fn service_store_security_descriptor() -> &'static str {
    "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"
}

/// Whether a running process is a session worker left by a previous service
/// instance, and so safe to terminate at startup.
///
/// Matching on the image name alone would also kill a host a developer is
/// running by hand, or a second install; only a process carrying the
/// `--session-worker` flag is ours to clear.
pub fn is_reapable_worker(image_name: &str, command_line: &str) -> bool {
    image_name.eq_ignore_ascii_case("ferryx-rd-host.exe")
        && command_line
            .split_whitespace()
            .any(|argument| argument.eq_ignore_ascii_case("--session-worker"))
}

/// Whether a starting service should terminate workers left by a previous
/// instance.
///
/// After the SCM restarts a crashed service, the old worker keeps running and
/// keeps the listening ports, but the new supervisor holds no handle to it and
/// can never respawn it onto a secure desktop. Clearing them at startup is what
/// lets the host recover its own capability rather than merely its process.
pub fn reap_orphans_on_startup() -> bool {
    true
}

/// SCM failure policy for the host service.
///
/// The SCM takes no action by default, so a crash leaves the host `STOPPED`
/// until somebody logs in and starts it by hand — which defeats a service whose
/// whole point is being reachable before anyone logs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFailureActions {
    /// Delay before each of the first three restart attempts.
    pub restart_delays_ms: Vec<u32>,
    /// Quiet period after which the failure count resets, so an old fault does
    /// not exhaust the restart budget.
    pub reset_period_secs: u32,
}

pub fn service_failure_actions() -> ServiceFailureActions {
    ServiceFailureActions {
        restart_delays_ms: vec![1_000, 5_000, 15_000],
        reset_period_secs: 86_400,
    }
}

/// Whether a failed acquire should tear the media session down.
///
/// Secure-desktop switches surface as `AccessLost`, `AccessDenied` and
/// `RefreshFailure`, and those must never abort however long they persist:
/// aborting there is what kills streaming at the logon screen. Only an
/// unexplained failure gives up, and only after it has repeated enough times to
/// rule out a transient.
pub fn should_abort_capture(failure: CaptureFailure, consecutive: u32) -> bool {
    matches!(failure, CaptureFailure::Other) && consecutive >= OTHER_FAILURE_ABORT_THRESHOLD
}

/// Recovery policy for the native capture loop.
pub fn capture_recovery(failure: CaptureFailure, consecutive: u32) -> CaptureRecovery {
    if should_abort_capture(failure, consecutive) {
        CaptureRecovery::Abort
    } else {
        CaptureRecovery::Reacquire(reacquire_backoff(consecutive))
    }
}

const OTHER_FAILURE_ABORT_THRESHOLD: u32 = 30;
const REACQUIRE_FLOOR_MILLIS: u64 = 33;
const REACQUIRE_CAP_MILLIS: u64 = 1000;

/// Polling interval while a session worker waits for a capturable output.
pub const WORKER_OUTPUT_POLL: Duration = Duration::from_millis(500);

/// Whether a worker that cannot yet describe an output should keep waiting.
///
/// A worker spawned onto the secure desktop starts before DXGI can enumerate an
/// output. Exiting there makes the supervisor respawn it in a tight loop, so the
/// worker waits instead, bounded so a genuinely displayless host still gives up.
pub fn keep_waiting_for_output(elapsed: Duration) -> bool {
    elapsed < WORKER_OUTPUT_STARTUP_LIMIT
}

/// How long a session worker waits for an output before giving up.
pub const WORKER_OUTPUT_STARTUP_LIMIT: Duration = Duration::from_secs(120);

/// Exponential backoff floored at one frame interval and capped so the loop
/// stays responsive when the desktop switches back.
pub fn reacquire_backoff(attempt: u32) -> Duration {
    let millis = REACQUIRE_FLOOR_MILLIS
        .checked_shl(attempt)
        .unwrap_or(REACQUIRE_CAP_MILLIS)
        .min(REACQUIRE_CAP_MILLIS);
    Duration::from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_desktop_names_case_insensitively() {
        for (name, expected) in [
            ("Winlogon", DesktopKind::Winlogon),
            ("wINLoGON", DesktopKind::Winlogon),
            ("Default", DesktopKind::Default),
            ("dEFAULT", DesktopKind::Default),
            ("Screen-saver", DesktopKind::Screensaver),
            ("sCREEN-SAVER", DesktopKind::Screensaver),
            ("Screensaver", DesktopKind::Screensaver),
            ("sCREENSAVER", DesktopKind::Screensaver),
            ("", DesktopKind::Other),
            ("Custom", DesktopKind::Other),
            (" Winlogon", DesktopKind::Other),
            ("Default-extra", DesktopKind::Other),
        ] {
            assert_eq!(desktop_kind(name), expected, "name: {name:?}");
        }
    }

    #[test]
    fn startup_desktops_use_winsta0_and_default_fallback() {
        for (desktop, expected) in [
            (DesktopKind::Winlogon, "WinSta0\\Winlogon"),
            (DesktopKind::Default, "WinSta0\\Default"),
            (DesktopKind::Screensaver, "WinSta0\\Screen-saver"),
            (DesktopKind::Other, "WinSta0\\Default"),
        ] {
            assert_eq!(desktop.startup_desktop(), expected);
        }
    }

    #[test]
    fn no_console_stops_worker_regardless_of_worker_presence() {
        let console = ConsoleState {
            session_id: None,
            desktop: DesktopKind::Winlogon,
        };
        for worker in [
            None,
            Some(WorkerState {
                session_id: 7,
                desktop: DesktopKind::Default,
            }),
        ] {
            assert_eq!(decide_action(console, worker), SupervisorAction::StopWorker);
        }
    }

    #[test]
    fn console_without_worker_spawns_on_its_desktop() {
        for desktop in [
            DesktopKind::Winlogon,
            DesktopKind::Default,
            DesktopKind::Screensaver,
            DesktopKind::Other,
        ] {
            assert_eq!(
                decide_action(
                    ConsoleState {
                        session_id: Some(7),
                        desktop,
                    },
                    None,
                ),
                SupervisorAction::Spawn {
                    session_id: 7,
                    desktop,
                },
            );
        }
    }

    #[test]
    fn matching_worker_is_idle() {
        assert_eq!(
            decide_action(
                ConsoleState {
                    session_id: Some(7),
                    desktop: DesktopKind::Winlogon,
                },
                Some(WorkerState {
                    session_id: 7,
                    desktop: DesktopKind::Winlogon,
                }),
            ),
            SupervisorAction::Idle,
        );
    }

    #[test]
    fn session_or_desktop_change_respawns_on_console_desktop() {
        let console = ConsoleState {
            session_id: Some(7),
            desktop: DesktopKind::Winlogon,
        };
        for worker in [
            WorkerState {
                session_id: 8,
                desktop: DesktopKind::Winlogon,
            },
            WorkerState {
                session_id: 7,
                desktop: DesktopKind::Default,
            },
            WorkerState {
                session_id: 8,
                desktop: DesktopKind::Default,
            },
        ] {
            assert_eq!(
                decide_action(console, Some(worker)),
                SupervisorAction::Respawn {
                    session_id: 7,
                    desktop: DesktopKind::Winlogon,
                },
            );
        }
    }

    #[test]
    fn service_store_joins_machine_wide_path_components() {
        for program_data in ["/var/program-data", "relative-data", ""] {
            let expected = PathBuf::from(program_data)
                .join("MahoRD")
                .join("host-authorizations.json");
            assert_eq!(service_store_path(program_data), expected);
        }
    }

    #[test]
    fn secure_desktop_failures_never_abort() {
        for failure in [
            CaptureFailure::AccessLost,
            CaptureFailure::AccessDenied,
            CaptureFailure::RefreshFailure,
        ] {
            for (consecutive, millis) in [
                (0, 33),
                (1, 66),
                (29, 1000),
                (30, 1000),
                (10_000, 1000),
                (u32::MAX, 1000),
            ] {
                assert_eq!(
                    capture_recovery(failure, consecutive),
                    CaptureRecovery::Reacquire(Duration::from_millis(millis)),
                    "failure: {failure:?}, consecutive: {consecutive}",
                );
            }
        }
    }

    #[test]
    fn other_capture_failures_abort_at_thirty() {
        for consecutive in 0..30 {
            assert_eq!(
                capture_recovery(CaptureFailure::Other, consecutive),
                CaptureRecovery::Reacquire(reacquire_backoff(consecutive)),
            );
        }
        for consecutive in [30, 31, 10_000, u32::MAX] {
            assert_eq!(
                capture_recovery(CaptureFailure::Other, consecutive),
                CaptureRecovery::Abort,
            );
        }
    }

    #[test]
    fn backoff_doubles_from_one_frame_and_caps_at_one_second() {
        for (attempt, millis) in [33, 66, 132, 264, 528, 1000, 1000].into_iter().enumerate() {
            assert_eq!(
                reacquire_backoff(attempt as u32),
                Duration::from_millis(millis),
            );
        }
    }

    #[test]
    fn hardening_targets_the_store_file_not_only_its_directory() {
        // Given: a credential file created before the directory was hardened.
        // Windows keeps the ACEs it already inherited, so tightening the parent
        // leaves the file itself readable by BUILTIN\Users.
        let targets = service_store_hardening_targets("C:\\ProgramData");

        // Then: both the directory and the file are re-secured.
        assert_eq!(targets.len(), 2, "{targets:?}");
        assert!(
            targets.iter().any(|path| path.ends_with("MahoRD")),
            "{targets:?}"
        );
        assert!(
            targets
                .iter()
                .any(|path| path.ends_with("host-authorizations.json")),
            "{targets:?}"
        );
    }

    #[test]
    fn machine_wide_store_is_not_readable_by_every_local_user() {
        // Given: host-authorizations.json holds the pre-shared keys that let a
        // paired client take over this machine. Under %ProgramData% it inherits
        // an ACL granting BUILTIN\Users read, so any local account could copy
        // the credentials.
        let descriptor = service_store_security_descriptor();

        // Then: only SYSTEM and Administrators may reach it, and inheritance is
        // cut so the permissive ProgramData default cannot flow back in.
        assert!(
            descriptor.contains("P"),
            "inheritance must be disabled: {descriptor}"
        );
        assert!(descriptor.contains("(A;OICI;FA;;;SY)"), "{descriptor}");
        assert!(descriptor.contains("(A;OICI;FA;;;BA)"), "{descriptor}");
        for unwanted in [";;;BU)", ";;;WD)", ";;;AU)"] {
            assert!(
                !descriptor.contains(unwanted),
                "{unwanted} must not appear in {descriptor}"
            );
        }
    }

    #[test]
    fn only_session_workers_are_reaped_not_every_host_process() {
        // Given: the processes a starting service may see. Reaping must clear
        // workers orphaned by the previous instance without killing a host a
        // developer or another install is running.
        assert!(
            is_reapable_worker("ferryx-rd-host.exe", "--session-worker"),
            "an orphaned session worker must be reaped"
        );
        assert!(
            is_reapable_worker(
                "FERRYX-RD-HOST.EXE",
                "\"C:\\erd\\ferryx-rd-host.exe\"  --session-worker "
            ),
            "matching is case- and spacing-insensitive"
        );

        // Then: everything else is left alone.
        for (name, command) in [
            ("ferryx-rd-host.exe", "--service-run"),
            ("ferryx-rd-host.exe", ""),
            ("ferryx-rd-host.exe", "--pin generate"),
            ("ferryx-rd-client.exe", "--session-worker"),
            ("notepad.exe", "--session-worker"),
        ] {
            assert!(
                !is_reapable_worker(name, command),
                "must not reap {name} {command:?}"
            );
        }
    }

    #[test]
    fn a_restarted_service_adopts_or_reaps_the_previous_worker() {
        // Given: the SCM restarted the service after a crash. The worker from
        // the dead instance is still running and still holding the listening
        // ports, but the new supervisor has no handle to it, so it can never be
        // respawned onto a secure desktop.
        let action = decide_action(
            ConsoleState {
                session_id: Some(1),
                desktop: DesktopKind::Default,
            },
            None,
        );

        // Then: the supervisor spawns, which would collide with the orphan on
        // the listening port unless startup reaps it first.
        assert_eq!(
            action,
            SupervisorAction::Spawn {
                session_id: 1,
                desktop: DesktopKind::Default,
            },
        );
        assert!(
            reap_orphans_on_startup(),
            "a restarted service must clear the previous instance's workers"
        );
    }

    #[test]
    fn service_recovery_restarts_after_a_crash() {
        // Given: the SCM's default failure policy takes no action, so a worker
        // or service crash leaves the host STOPPED until someone logs in and
        // starts it by hand — which defeats a host meant to be reachable
        // before anyone logs in.
        let actions = service_failure_actions();

        // Then: every one of the first three failures restarts the service,
        // and the counter resets after a quiet period so a fault long ago does
        // not exhaust the budget.
        assert_eq!(actions.restart_delays_ms.len(), 3, "{actions:?}");
        assert!(
            actions.restart_delays_ms.iter().all(|delay| *delay > 0),
            "a zero delay hot-loops the SCM: {actions:?}"
        );
        assert!(
            actions.reset_period_secs > 0,
            "reset period 0 never clears the failure count: {actions:?}"
        );
    }

    #[test]
    fn only_unexplained_failures_abort_the_capture_loop() {
        // Given: the failures a secure-desktop switch produces. They must never
        // abort, however long they persist, or streaming dies at the logon
        // screen.
        for failure in [
            CaptureFailure::AccessLost,
            CaptureFailure::AccessDenied,
            CaptureFailure::RefreshFailure,
        ] {
            for consecutive in [0, 29, 30, 10_000, u32::MAX] {
                assert!(
                    !should_abort_capture(failure, consecutive),
                    "{failure:?} aborted at {consecutive}"
                );
            }
        }

        // Given: an unexplained failure, which is tolerated briefly and then
        // gives up rather than spinning forever.
        for consecutive in 0..30 {
            assert!(!should_abort_capture(CaptureFailure::Other, consecutive));
        }
        for consecutive in [30, 31, u32::MAX] {
            assert!(should_abort_capture(CaptureFailure::Other, consecutive));
        }
    }

    #[test]
    fn backoff_is_capped_without_overflow_for_large_attempts() {
        for attempt in [31, 32, 63, 64, 10_000, u32::MAX] {
            assert_eq!(reacquire_backoff(attempt), Duration::from_millis(1000));
        }
    }

    #[test]
    fn worker_waits_for_a_capturable_output_then_gives_up() {
        for elapsed in [
            Duration::ZERO,
            Duration::from_secs(1),
            Duration::from_secs(119),
        ] {
            // Then: it keeps waiting rather than exiting into a respawn loop.
            assert!(
                keep_waiting_for_output(elapsed),
                "gave up after {elapsed:?}"
            );
        }
        for elapsed in [
            WORKER_OUTPUT_STARTUP_LIMIT,
            Duration::from_secs(121),
            Duration::from_secs(10_000),
        ] {
            // Then: a genuinely displayless host still terminates.
            assert!(
                !keep_waiting_for_output(elapsed),
                "waited forever at {elapsed:?}"
            );
        }
        assert!(WORKER_OUTPUT_POLL < WORKER_OUTPUT_STARTUP_LIMIT);
    }

    #[test]
    fn console_desktop_from_hint_assumes_winlogon_when_no_worker_published() {
        // RED-first contract: at the pre-logon screen no worker lives to publish
        // a hint, so the supervisor must assume the secure desktop instead of
        // asking session 0 what IT sees.
        use super::console_desktop_from_hint;

        assert_eq!(console_desktop_from_hint(None), DesktopKind::Winlogon);
        assert_eq!(console_desktop_from_hint(Some("")), DesktopKind::Winlogon);
        assert_eq!(
            console_desktop_from_hint(Some("   ")),
            DesktopKind::Winlogon
        );
    }

    #[test]
    fn console_desktop_from_hint_trusts_published_names() {
        use super::console_desktop_from_hint;

        assert_eq!(
            console_desktop_from_hint(Some("Default")),
            DesktopKind::Default
        );
        assert_eq!(
            console_desktop_from_hint(Some("Winlogon")),
            DesktopKind::Winlogon
        );
        assert_eq!(
            console_desktop_from_hint(Some("Screen-saver")),
            DesktopKind::Screensaver
        );
        assert_eq!(
            console_desktop_from_hint(Some("Garbage\\Nope")),
            DesktopKind::Other
        );
    }

    #[test]
    fn worker_recycles_when_accepts_continue_without_handshakes() {
        // The churn-wedge signature: TCP accepts pile up (\u003e= 3) while not a
        // single TLS handshake has ever completed. After the startup grace the
        // worker must retire so the supervisor respawns a clean one.
        use super::worker_should_recycle;

        assert!(worker_should_recycle(
            3,
            0,
            Duration::from_secs(60),
            Duration::from_secs(30)
        ));
        assert!(worker_should_recycle(
            9,
            0,
            Duration::from_secs(600),
            Duration::from_secs(30)
        ));
    }

    #[test]
    fn worker_recycle_spares_healthy_and_fresh_workers() {
        use super::worker_should_recycle;

        // A handshake has ever completed: healthy, keep it.
        assert!(!worker_should_recycle(
            9,
            1,
            Duration::from_secs(600),
            Duration::from_secs(30)
        ));
        // No connections at all: idle host, not wedged, keep it.
        assert!(!worker_should_recycle(
            0,
            0,
            Duration::from_secs(600),
            Duration::from_secs(30)
        ));
        // Within the startup grace: keep it.
        assert!(!worker_should_recycle(
            3,
            0,
            Duration::from_secs(5),
            Duration::from_secs(30)
        ));
    }
}
