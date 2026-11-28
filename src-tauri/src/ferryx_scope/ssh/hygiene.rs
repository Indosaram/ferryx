//! Reaps login sessions that abnormal SSH disconnects left behind on the host.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::mpsc;
use std::time::Duration;

pub const HYGIENE_INTERVAL_ENV: &str = "FERRYX_HELPER_HYGIENE_SECS";
pub const DEFAULT_INTERVAL_SECS: u64 = 300;
const STARTUP_DELAY: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcFacts {
    pub pid: u32,
    pub cmdline: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    pub id: String,
    pub leader_alive: bool,
    pub class: String,
    pub service: String,
    pub procs: Vec<ProcFacts>,
    pub procs_owned_by_us: bool,
    pub own_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReapDecision {
    Reap,
    Keep(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait SessionHost {
    fn list_sessions(&self) -> Result<Vec<String>, String>;
    fn session_facts(&self, id: &str) -> Result<SessionFacts, String>;
    fn kill_session(&self, id: &str, signal: &str) -> Result<(), String>;
}

pub fn decide(facts: &SessionFacts, self_pid: u32) -> ReapDecision {
    if facts.own_session {
        return ReapDecision::Keep("own session");
    }
    if facts.leader_alive {
        return ReapDecision::Keep("leader alive");
    }
    if facts.class != "user" {
        return ReapDecision::Keep("non-user session class");
    }
    if !facts.procs_owned_by_us {
        return ReapDecision::Keep("foreign user");
    }
    if facts.procs.is_empty() {
        return ReapDecision::Keep("no processes to reap");
    }
    if facts
        .procs
        .iter()
        .any(|p| p.pid == self_pid || protected_cmdline(&p.cmdline))
    {
        return ReapDecision::Keep("protected process");
    }
    ReapDecision::Reap
}

fn protected_cmdline(cmdline: &str) -> bool {
    const MARKERS: [&str; 6] = [
        "ferryx-remote-helper",
        "systemd",
        "sshd",
        "tailscaled",
        "sddm",
        "hyprland",
    ];
    MARKERS.iter().any(|marker| cmdline.contains(marker))
}

pub fn interval_from_env(value: Option<&str>) -> Option<Duration> {
    match value {
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(0) => None,
            Ok(secs) => Some(Duration::from_secs(secs)),
            Err(_) => Some(Duration::from_secs(DEFAULT_INTERVAL_SECS)),
        },
        None => Some(Duration::from_secs(DEFAULT_INTERVAL_SECS)),
    }
}

pub fn run_tick(host: &dyn SessionHost, self_pid: u32, grace: Duration) -> Vec<String> {
    let ids = match host.list_sessions() {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!("hygiene: listing sessions failed: {error}");
            return Vec::new();
        }
    };

    let mut candidates = Vec::new();
    for id in ids {
        let facts = match host.session_facts(&id) {
            Ok(facts) => facts,
            Err(error) => {
                eprintln!("hygiene: session {id} inspection failed: {error}");
                continue;
            }
        };
        if decide(&facts, self_pid) != ReapDecision::Reap {
            continue;
        }
        if let Err(error) = host.kill_session(&id, "SIGTERM") {
            eprintln!("hygiene: session {id} SIGTERM failed: {error}");
            continue;
        }
        if !grace.is_zero() {
            std::thread::sleep(grace);
        }
        let remaining = host.session_facts(&id).map(|f| f.procs).unwrap_or_default();
        if !remaining.is_empty() {
            if let Err(error) = host.kill_session(&id, "SIGKILL") {
                eprintln!("hygiene: session {id} SIGKILL failed: {error}");
            }
        }
        candidates.push(id);
    }

    if candidates.is_empty() {
        return candidates;
    }
    match host.list_sessions() {
        Ok(after) => candidates.retain(|id| !after.iter().any(|live| live == id)),
        Err(error) => eprintln!("hygiene: verify listing failed: {error}"),
    }
    candidates
}

#[cfg(target_os = "linux")]
pub fn start() {
    let interval = match interval_from_env(std::env::var(HYGIENE_INTERVAL_ENV).ok().as_deref()) {
        Some(interval) => interval,
        None => return,
    };
    let _ = std::thread::Builder::new()
        .name("ferryx-hygiene".into())
        .spawn(move || {
            std::thread::sleep(STARTUP_DELAY);
            loop {
                let host = SystemHost::detect();
                let reaped = run_tick(&host, std::process::id(), KILL_GRACE);
                if !reaped.is_empty() {
                    eprintln!(
                        "hygiene: reaped {} leaked login sessions ({})",
                        reaped.len(),
                        reaped.join(", ")
                    );
                }
                std::thread::sleep(interval);
            }
        });
}

#[cfg(not(target_os = "linux"))]
pub fn start() {}

#[cfg(target_os = "linux")]
struct SystemHost {
    own_session: Option<String>,
    uid: u32,
}

#[cfg(target_os = "linux")]
impl SystemHost {
    fn detect() -> Self {
        Self {
            own_session: own_session_from_cgroup(),
            uid: unsafe { libc::getuid() },
        }
    }
}

#[cfg(target_os = "linux")]
fn own_session_from_cgroup() -> Option<String> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let start = text.find("session-")? + "session-".len();
    let tail = &text[start..];
    let end = tail.find(".scope")?;
    let id = tail[..end].trim_matches('/');
    (!id.is_empty()).then(|| id.to_string())
}

#[cfg(target_os = "linux")]
impl SessionHost for SystemHost {
    fn list_sessions(&self) -> Result<Vec<String>, String> {
        let output = run_command(
            "loginctl",
            &["list-sessions".to_string(), "--no-legend".to_string()],
            COMMAND_TIMEOUT,
        )?;
        if !output.success {
            return Err(format!(
                "loginctl list-sessions failed: {}",
                output.stderr.trim()
            ));
        }
        Ok(output
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(str::to_string)
            .collect())
    }

    fn session_facts(&self, id: &str) -> Result<SessionFacts, String> {
        let class = show_session(id, "Class")?;
        let service = show_session(id, "Service")?;
        let leader = show_session(id, "Leader")?;
        let leader_pid: u32 = leader
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        let leader_alive =
            leader_pid != 0 && std::path::Path::new(&format!("/proc/{leader_pid}")).exists();

        let mut procs = Vec::new();
        let mut procs_owned_by_us = true;
        for pid in session_cgroup_pids(id) {
            procs.push(ProcFacts {
                pid,
                cmdline: process_cmdline(pid),
            });
            if !process_owned_by_uid(pid, self.uid) {
                procs_owned_by_us = false;
            }
        }

        Ok(SessionFacts {
            id: id.to_string(),
            leader_alive,
            class,
            service,
            procs,
            procs_owned_by_us,
            own_session: self.own_session.as_deref() == Some(id),
        })
    }

    fn kill_session(&self, id: &str, signal: &str) -> Result<(), String> {
        let output = run_command(
            "loginctl",
            &[
                "kill-session".to_string(),
                id.to_string(),
                format!("--signal={signal}"),
            ],
            COMMAND_TIMEOUT,
        )?;
        if output.success {
            Ok(())
        } else {
            Err(format!(
                "loginctl kill-session {id} {signal} failed: {}",
                output.stderr.trim()
            ))
        }
    }
}

#[cfg(target_os = "linux")]
fn show_session(id: &str, property: &str) -> Result<String, String> {
    let output = run_command(
        "loginctl",
        &[
            "show-session".to_string(),
            id.to_string(),
            "-p".to_string(),
            property.to_string(),
            "--value".to_string(),
        ],
        COMMAND_TIMEOUT,
    )?;
    if !output.success {
        return Err(format!(
            "loginctl show-session {id} {property} failed: {}",
            output.stderr.trim()
        ));
    }
    Ok(output.stdout.trim().to_string())
}

#[cfg(target_os = "linux")]
fn session_cgroup_pids(id: &str) -> Vec<u32> {
    let output = match run_command(
        "systemctl",
        &[
            "show".to_string(),
            format!("session-{id}.scope"),
            "-p".to_string(),
            "ControlGroup".to_string(),
            "--value".to_string(),
        ],
        COMMAND_TIMEOUT,
    ) {
        Ok(output) if output.success => output,
        _ => return Vec::new(),
    };
    let cgroup = output.stdout.trim();
    if cgroup.is_empty() {
        return Vec::new();
    }
    std::fs::read_to_string(format!("/sys/fs/cgroup{cgroup}/cgroup.procs"))
        .map(|text| text.lines().filter_map(|line| line.trim().parse().ok()).collect())
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn process_cmdline(pid: u32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .replace('\0', " ")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn process_owned_by_uid(pid: u32, uid: u32) -> bool {
    let status = match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(status) => status,
        Err(_) => return false,
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|value| value.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        == Some(uid)
}

fn kill_and_reap(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(pgid) = i32::try_from(child.id()) {
        if pgid > 0 {
            // SAFETY: child runs in its own process group (`process_group(0)`),
            // so negating its PID signals only that process group.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_command(program: &str, args: &[String], timeout: Duration) -> Result<CommandOutput, String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("HYGIENE_SPAWN_FAILED: {program}: {e}"))?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let (sender, receiver) = mpsc::channel();
    let io_thread = std::thread::spawn(move || {
        use std::io::Read;
        let stderr_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(ref mut pipe) = stderr {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        });
        let mut out_buf = Vec::new();
        if let Some(ref mut pipe) = stdout {
            let _ = pipe.read_to_end(&mut out_buf);
        }
        let err_buf = stderr_thread.join().unwrap_or_default();
        let _ = sender.send((out_buf, err_buf));
    });

    let deadline = std::time::Instant::now() + timeout;
    match receiver.recv_timeout(timeout) {
        Ok((stdout_bytes, stderr_bytes)) => {
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => {
                        if std::time::Instant::now() >= deadline {
                            kill_and_reap(&mut child);
                            let _ = io_thread.join();
                            return Err(format!("HYGIENE_COMMAND_TIMEOUT: {program}"));
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => {
                        kill_and_reap(&mut child);
                        let _ = io_thread.join();
                        return Err(format!("HYGIENE_COMMAND_FAILED: {program}: {error}"));
                    }
                }
            };
            let _ = io_thread.join();
            Ok(CommandOutput {
                success: status.success(),
                stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
            })
        }
        Err(_) => {
            kill_and_reap(&mut child);
            let _ = io_thread.join();
            Err(format!("HYGIENE_COMMAND_TIMEOUT: {program}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn facts(id: &str) -> SessionFacts {
        SessionFacts {
            id: id.to_string(),
            leader_alive: false,
            class: "user".to_string(),
            service: "tailscaled".to_string(),
            procs: vec![ProcFacts {
                pid: 4242,
                cmdline: "sleep 900".to_string(),
            }],
            procs_owned_by_us: true,
            own_session: false,
        }
    }

    struct FakeHost {
        sessions: RefCell<Vec<SessionFacts>>,
        kills: RefCell<Vec<(String, String)>>,
        reap_on_kill: bool,
    }

    impl FakeHost {
        fn new(sessions: Vec<SessionFacts>, reap_on_kill: bool) -> Self {
            Self {
                sessions: RefCell::new(sessions),
                kills: RefCell::new(Vec::new()),
                reap_on_kill,
            }
        }

        fn kills(&self) -> Vec<(String, String)> {
            self.kills.borrow().clone()
        }
    }

    impl SessionHost for FakeHost {
        fn list_sessions(&self) -> Result<Vec<String>, String> {
            Ok(self
                .sessions
                .borrow()
                .iter()
                .map(|facts| facts.id.clone())
                .collect())
        }

        fn session_facts(&self, id: &str) -> Result<SessionFacts, String> {
            self.sessions
                .borrow()
                .iter()
                .find(|facts| facts.id == id)
                .cloned()
                .ok_or_else(|| format!("missing session {id}"))
        }

        fn kill_session(&self, id: &str, signal: &str) -> Result<(), String> {
            self.kills
                .borrow_mut()
                .push((id.to_string(), signal.to_string()));
            if self.reap_on_kill {
                self.sessions.borrow_mut().retain(|facts| facts.id != id);
            }
            Ok(())
        }
    }

    #[test]
    fn hygiene_decide_reaps_dead_leader_with_leftovers() {
        assert_eq!(decide(&facts("c11"), 99), ReapDecision::Reap);
    }

    #[test]
    fn hygiene_decide_keeps_live_leader() {
        let mut session = facts("c12");
        session.leader_alive = true;
        assert_eq!(
            decide(&session, 99),
            ReapDecision::Keep("leader alive")
        );
    }

    #[test]
    fn hygiene_decide_keeps_protected_helper_process() {
        let mut session = facts("c536");
        session.procs = vec![ProcFacts {
            pid: 176844,
            cmdline: "/home/indo/.ferryx/bin/ferryx-remote-helper daemon --root /home/indo/.ferryx"
                .to_string(),
        }];
        assert_eq!(
            decide(&session, 99),
            ReapDecision::Keep("protected process")
        );
    }

    #[test]
    fn hygiene_decide_keeps_non_user_session_class() {
        let mut session = facts("2");
        session.class = "manager".to_string();
        assert_eq!(
            decide(&session, 99),
            ReapDecision::Keep("non-user session class")
        );
    }

    #[test]
    fn hygiene_decide_keeps_foreign_processes() {
        let mut session = facts("c13");
        session.procs_owned_by_us = false;
        assert_eq!(decide(&session, 99), ReapDecision::Keep("foreign user"));
    }

    #[test]
    fn hygiene_decide_keeps_own_session() {
        let mut session = facts("c14");
        session.own_session = true;
        assert_eq!(decide(&session, 99), ReapDecision::Keep("own session"));
    }

    #[test]
    fn hygiene_decide_keeps_empty_session() {
        let mut session = facts("c15");
        session.procs.clear();
        assert_eq!(
            decide(&session, 99),
            ReapDecision::Keep("no processes to reap")
        );
    }

    #[test]
    fn hygiene_decide_keeps_session_containing_self() {
        let mut session = facts("c16");
        session.procs = vec![ProcFacts {
            pid: 99,
            cmdline: "zsh".to_string(),
        }];
        assert_eq!(
            decide(&session, 99),
            ReapDecision::Keep("protected process")
        );
    }

    #[test]
    fn hygiene_interval_from_env_defaults_zero_and_garbage() {
        assert_eq!(interval_from_env(None), Some(Duration::from_secs(300)));
        assert_eq!(interval_from_env(Some("0")), None);
        assert_eq!(interval_from_env(Some("42")), Some(Duration::from_secs(42)));
        assert_eq!(interval_from_env(Some("junk")), Some(Duration::from_secs(300)));
    }

    #[test]
    fn hygiene_run_tick_reaps_only_leaked_sessions() {
        let mut kept = facts("c12");
        kept.leader_alive = true;
        let host = FakeHost::new(vec![facts("c11"), kept], true);
        let reaped = run_tick(&host, 99, Duration::ZERO);
        assert_eq!(reaped, vec!["c11".to_string()]);
        assert_eq!(host.kills(), vec![("c11".to_string(), "SIGTERM".to_string())]);
    }

    #[test]
    fn hygiene_run_tick_escalates_when_processes_survive() {
        let host = FakeHost::new(vec![facts("c17")], false);
        let reaped = run_tick(&host, 99, Duration::ZERO);
        assert!(reaped.is_empty());
        assert_eq!(
            host.kills(),
            vec![
                ("c17".to_string(), "SIGTERM".to_string()),
                ("c17".to_string(), "SIGKILL".to_string()),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn hygiene_run_command_succeeds_and_reaps_hung_command_on_timeout() {
        let ok = run_command(
            "/bin/sh",
            &["-c".to_string(), "printf ok".to_string()],
            Duration::from_secs(5),
        )
        .expect("fast command succeeds");
        assert!(ok.success);
        assert_eq!(ok.stdout, "ok");

        let started = std::time::Instant::now();
        let err = run_command(
            "/bin/sh",
            &["-c".to_string(), "exec sleep 30".to_string()],
            Duration::from_millis(50),
        )
        .expect_err("hung command must time out");
        assert_eq!(err, "HYGIENE_COMMAND_TIMEOUT: /bin/sh");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timeout must kill and reap promptly instead of waiting for sleep 30"
        );
    }
}
