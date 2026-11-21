//! Phase 2 acceptance against the **real** PTY keeper binary
//! (`.omo/plans/local-pty-crash-survival-20261116.md`, "Phase 2" and the Phase 3 gate rules).
//!
//! This is the companion of `daemon_crash_supervisor_acceptance.rs`. The two cover different
//! things on purpose:
//!
//! * that file proves the supervisor's *decision* logic against a wire-faithful keeper responder:
//!   the real `KeeperSocketProbe` dials it, so the gate opens only through production validation,
//!   but no keeper process exists.
//! * this file runs the real `ferryx-ptykeeper` process. A separate process derives
//!   `<FERRYX_RUNTIME_DIR>/keeper.sock` from its own environment exactly as it does in production,
//!   answers `HELLO` itself, and reports its own pid, which the kernel's `LOCAL_PEERPID`
//!   corroborates. The pid in the supervisor's gate line is a pid this harness spawned and can kill.
//!
//! Both use the same isolated-runtime fixture, so neither can reach the user's daemon, keeper or
//! `/tmp/ferryx-<uid>`.
//!
//! ## Coverage of the plan's Phase 2 cases
//!
//! | Plan case | Real keeper here | Wire-level companion |
//! |---|---|---|
//! | abnormal exit restarts once | yes | yes |
//! | exit 0 does not restart | yes | yes |
//! | handover does not restart | yes | yes |
//! | 6 rapid failures stop at the limit | yes | yes |
//! | no keeper refuses supervised restart | yes, plus a *dead* socket | yes |
//! | keeper live, gate names it | yes | stand-in pid |
//! | keeper ownership of a PTY | yes, an owned child survives its client | not covered |
//!
//! The handover case needs a successor process, which the supervisor never spawns (it only starts a
//! plain `--daemon`), so this harness starts it over the real handover path exactly as the
//! wire-level companion does. Unlike that companion this file has a live keeper, so the case also
//! proves the handover leaves a keeper-backed runtime with its keeper still serving.
//!
//! ## The build seam this file depends on
//!
//! `crates/ferryx-ptykeeper` is a **separate cargo workspace** with its own `Cargo.lock` and its own
//! `[workspace]` key, and `src-tauri` neither declares a workspace nor depends on it. Cargo
//! therefore never builds a keeper alongside `ferryx-cli`, and `env!("CARGO_BIN_EXE_...")` cannot
//! name one. Resolution order:
//!
//! 1. `FERRYX_PTY_KEEPER_BIN`; set but unusable is a hard failure, never a skip.
//! 2. `<crates/ferryx-ptykeeper/target/{debug,release}/ferryx-ptykeeper`, a run-owned build.
//!
//! ```text
//! scripts/build-remote.sh p2-keeper -- \
//!   'cd crates/ferryx-ptykeeper && cargo build --bin ferryx-ptykeeper && cd ../../src-tauri && \
//!    cargo test --test daemon_crash_supervisor_real_keeper_acceptance'
//! ```
//!
//! A run with no keeper binary still tells the truth: the gate test below reports the absence and
//! asserts it is the typed `PayloadAbsent` condition the daemon itself hits.
//!
//! ## The launchd cases (`real_launchd`, macOS only)
//!
//! The cases above spawn the supervisor from this harness, so they prove the loop but not the
//! ownership the plan actually ships: under launchd the **job** runs the supervisor
//! (`launchd::generate_launchd_plist` names `--daemon-crash-supervisor`), which is what stops an app
//! replacement from starting a second one. `mod real_launchd` bootstraps real jobs - a private
//! keeper job and a private supervisor job - into this user's `gui/<uid>` domain, under labels minted
//! per run, and asserts on what launchd itself reports (`launchctl print`), not only on the plist on
//! disk: a job bootstrapped from an older plist keeps that plist's arguments in memory, so a passing
//! plist check alone proves nothing about the running chain.
//!
//! Those four cases are `#[ignore]`d, because they need a real Darwin session, and they are gated on
//! `FERRYX_REAL_LAUNCHD_SUPERVISOR=1`. Both are deliberate:
//!
//! * `#[ignore]` is what a plain `cargo test` reports - an honest "ignored", never a green run that
//!   silently skipped the acceptance.
//! * When asked to run, a **missing prerequisite is a failure**: [`require_real_launchd`] panics and
//!   names the unmet condition. There is no branch that returns early, so a Darwin host without
//!   `launchctl`, without a reachable `gui/<uid>` domain, or without the keeper binary fails this
//!   file instead of reporting a pass.
//! * Off Darwin the module does not exist at all, so `--ignored` would select nothing and report
//!   success. `real_launchd_cases_fail_rather_than_vanish_off_darwin` occupies that slot and fails
//!   when selected, so the gate command in `.omo/evidence/supervisor/GATE-COMMAND.md` cannot pass
//!   empty on a host that has no launchd.
//!
//! Isolation is the same as everywhere else in this file plus two rules the ownership cases add:
//! every label is proven private with `keeper_launchd::is_private_label` before `launchctl` is
//! touched, so `com.ferryx.daemon`, `com.ferryx.ptykeeper`, `com.ferryx.qa.ptykeeper` and
//! `com.rorca.daemon` can never be bootstrapped, booted out, kicked or read by this harness; and both
//! jobs forward their plist paths, runtime, data, session and keeper directories, because a launchd
//! job inherits no shell environment. Nothing here signals a process it did not start or find through
//! its own runtime's control endpoint: the daemon pid comes from `control-v1 Hello`, every other pid
//! from `launchctl print` or `Run::spawn`.
//!
//! One prerequisite of the gate is not a job's to produce: besides a live keeper, the supervisor
//! requires the durable table to route new sessions to that keeper (`KeeperSocketProbe::
//! owns_new_sessions`), and it reads the table in its own process before it spawns the daemon whose
//! startup pass is the only writer. Every case that expects `supervised_restart=on` therefore calls
//! `Fixture::activate_keeper_ownership` after the keeper answers and before the supervisor is
//! started, through the product's own `keeper_session::activate_ownership`; the cases that expect
//! the gate to refuse (no keeper, dead socket) never call it.
//!
//! ## Signalling discipline
//!
//! Every signal goes through `Run::terminate` or `Run::stop`, which refuse pid 0 and 1, refuse this
//! harness's own pid, and refuse any pid the run never recorded. A test asserts the guard, and no
//! bare `libc::kill` exists outside it. A keeper-owned PTY child is a grandchild of this harness, so
//! waiting for its death asks the kernel for liveness; the deadline keeps that bounded.

#![cfg(unix)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use ferryx_lib::daemon::control::{connect, ControlConnection, ControlReply, ControlRequest};
use ferryx_lib::daemon::crash_supervisor::{
    KeeperSocketProbe, KEEPER_SOCKET_NAME, MAX_RESTARTS_PER_WINDOW, RESTART_LOG_NAME,
    SUPERVISOR_ARG,
};
use ferryx_lib::daemon::keeper_client::KeeperClient;
use ferryx_lib::daemon::keeper_session::{
    activate_ownership, OwnershipStore, SessionOwner, OWNERSHIP_FILE_NAME,
};
use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
#[cfg(target_os = "macos")]
use ferryx_lib::daemon::keeper_launchd::{keeper_service_for, QA_PLIST_LABEL, PLIST_LABEL};
use notify::Watcher as _;
use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_ferryx-cli");
const KEEPER_BIN_ENV: &str = "FERRYX_PTY_KEEPER_BIN";
const KEEPER_BINARY_NAME: &str = "ferryx-ptykeeper";
const LOG_WAIT: Duration = Duration::from_secs(30);
const READY_WAIT: Duration = Duration::from_secs(90);
const CASE_WAIT: Duration = Duration::from_secs(240);
const WAKE_NET: Duration = Duration::from_millis(200);

const GATE_LINE: &str = "event=gate";
const GATE_ON: &str = "supervised_restart=on";
const GATE_OFF: &str = "supervised_restart=off";
const BANNER_LINE: &str = "event=banner";
const STOP_LINE: &str = "event=stop";
const RESTART_LINE: &str = "event=restart";
const ABNORMAL_LINE: &str = "kind=abnormal";

/// Asks for the real-launchd cases. Unset means they stay `#[ignore]`d - which is exactly what a
/// plain `cargo test` reports - and set means every prerequisite must hold: a missing one is a
/// failure that names the missing thing, never a silent skip.
const REAL_LAUNCHD_ASK_ENV: &str = "FERRYX_REAL_LAUNCHD_SUPERVISOR";

fn real_launchd_asked() -> bool {
    std::env::var(REAL_LAUNCHD_ASK_ENV).is_ok_and(|value| !value.trim().is_empty())
}

fn keeper_binary() -> Result<PathBuf, String> {
    let explicit = std::env::var_os(KEEPER_BIN_ENV).map(PathBuf::from);
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap_or(manifest);
    let keeper_workspace = workspace.join("crates").join("ferryx-ptykeeper");
    let run_owned = ["debug", "release"]
        .iter()
        .map(|profile| {
            keeper_workspace
                .join("target")
                .join(profile)
                .join(KEEPER_BINARY_NAME)
        })
        .find(|candidate| candidate.is_file());

    match (explicit, run_owned) {
        (Some(path), _) if path.is_file() => Ok(path),
        (Some(path), _) => Err(format!(
            "{KEEPER_BIN_ENV} names {} which is not a file; a path that cannot be executed is a \
             broken fixture, not a keeper",
            path.display()
        )),
        (None, Some(path)) => Ok(path),
        (None, None) => Err(format!(
            "KEEPER_PAYLOAD_ABSENT: no real {KEEPER_BINARY_NAME} binary. Produce one with \
             `cd crates/ferryx-ptykeeper && cargo build --bin {KEEPER_BINARY_NAME}`, leaving it at \
             {}/target/debug/{KEEPER_BINARY_NAME}, or set {KEEPER_BIN_ENV}. The keeper crate is its \
             own cargo workspace, so a build here never produces it.",
            keeper_workspace.display()
        )),
    }
}

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix(&format!("fx-rk-{label}-"))
            .tempdir_in("/tmp")
            .expect("private case root under /tmp");
        for name in [
            "home",
            "runtime",
            "legacy-run",
            "data",
            "data/logs",
            "sessions",
            "config",
            "cache",
            "tmp",
            "launchd",
            "keeper",
            "logs",
        ] {
            std::fs::create_dir_all(root.path().join(name)).expect("case directory");
        }
        set_mode(&root.path().join("runtime"), 0o700);
        set_mode(&root.path().join("legacy-run"), 0o700);
        let fixture = Self { root };
        fixture.refuse_production_paths();
        fixture
    }

    fn refuse_production_paths(&self) {
        // SAFETY: `getuid` takes no arguments, dereferences no pointers and has no side effects.
        let uid = unsafe { libc::getuid() };
        let production = PathBuf::from(format!("/tmp/ferryx-{uid}"));
        assert!(
            self.runtime().starts_with(self.root.path()),
            "case path {} must stay inside {}",
            self.runtime().display(),
            self.root.path().display()
        );
        assert_ne!(
            self.runtime(),
            production,
            "an isolated case must never run on the production runtime directory"
        );
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn runtime(&self) -> PathBuf {
        self.path("runtime")
    }

    fn restart_log(&self) -> PathBuf {
        self.path("data/logs").join(RESTART_LOG_NAME)
    }

    fn keeper_socket(&self) -> PathBuf {
        self.runtime().join(KEEPER_SOCKET_NAME)
    }

    /// The durable ownership table the supervisor's gate reads, beside the daemon's other state.
    ///
    /// `OwnershipStore::for_runtime` resolves to exactly this path from a child's environment -
    /// `logging::logs_dir().parent()`, i.e. `$FERRYX_DATA_DIR/pty-ownership.json` - so this names
    /// the file the supervisor will actually read, not a copy of it.
    fn ownership_table(&self) -> PathBuf {
        self.path("data").join(OWNERSHIP_FILE_NAME)
    }

    /// Opens the supervisor's gate the way production does, before any supervisor is started.
    ///
    /// The gate asks two independent questions - is a keeper live, and does the durable table route
    /// new sessions to it (`KeeperSocketProbe::owns_new_sessions`) - and the supervisor asks both in
    /// its own process, before it spawns the daemon whose startup pass is the only writer of the
    /// second answer. A supervisor therefore cannot establish it: on a first start there is no
    /// daemon to run that pass, and a refused supervisor exits 0, which launchd's
    /// `KeepAlive { successful_exit: false }` does not relaunch. So the harness enters the state
    /// through the product's own enable path, `keeper_session::activate_ownership` - the function
    /// `KeeperOwnership::activate_new_sessions` calls - bound to this run's keeper and this run's
    /// table.
    ///
    /// The table is named explicitly rather than left to `OwnershipStore::for_runtime`, which would
    /// resolve through *this* process's `FERRYX_DATA_DIR`: the fixture applies its isolation to the
    /// children it spawns, not to the harness itself. `activate_ownership` answers `NotProven` and
    /// writes nothing when no keeper answers, so this cannot manufacture the state for a case that
    /// has none - it times out naming the probe's answer instead.
    async fn activate_keeper_ownership(&self) {
        let table = self.ownership_table();
        let store = OwnershipStore::at(&table);
        let probe = KeeperSocketProbe::new(self.runtime()).with_ownership_table(&table);
        let deadline = Instant::now() + READY_WAIT;
        loop {
            match activate_ownership(&store, &probe) {
                Ok(outcome) if outcome.is_active() => break,
                Ok(outcome) => assert!(
                    Instant::now() < deadline,
                    "new sessions must route to this runtime's keeper within {READY_WAIT:?}; the \
                     probe answered {outcome:?}, which does not activate"
                ),
                Err(error) => assert!(
                    Instant::now() < deadline,
                    "new sessions must route to this runtime's keeper within {READY_WAIT:?}; the \
                     ownership table {} is not writable: {error:?}",
                    table.display()
                ),
            }
            tokio::time::sleep(WAKE_NET).await;
        }
        assert_eq!(
            store
                .load()
                .expect("the table activation just wrote")
                .new_session_owner(),
            SessionOwner::Keeper,
            "activation must leave new sessions routed to this runtime's keeper"
        );
    }

    fn keeper_plist(&self) -> PathBuf {
        self.path("launchd/com.ferryx.qa.ptykeeper.plist")
    }

    fn isolation_env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            ("HOME", self.path("home").into_os_string()),
            ("USERPROFILE", self.path("home").into_os_string()),
            ("ZDOTDIR", self.path("home").into_os_string()),
            ("XDG_CONFIG_HOME", self.path("config").into_os_string()),
            ("XDG_DATA_HOME", self.path("data").into_os_string()),
            ("XDG_CACHE_HOME", self.path("cache").into_os_string()),
            ("XDG_RUNTIME_DIR", self.runtime().into_os_string()),
            ("TMPDIR", self.path("tmp").into_os_string()),
            ("FERRYX_RUNTIME_DIR", self.runtime().into_os_string()),
            (
                "FERRYX_LEGACY_RUNTIME_DIR",
                self.path("legacy-run").into_os_string(),
            ),
            ("FERRYX_DATA_DIR", self.path("data").into_os_string()),
            ("FERRYX_SESSION_DIR", self.path("sessions").into_os_string()),
            ("FERRYX_KEEPER_DIR", self.path("keeper").into_os_string()),
            (
                "FERRYX_PTY_KEEPER_PLIST_PATH",
                self.keeper_plist().into_os_string(),
            ),
            (
                "FERRYX_DAEMON_PLIST_PATH",
                self.path("launchd/com.ferryx.qa.daemon.plist").into_os_string(),
            ),
            ("PATH", OsString::from("/usr/bin:/bin:/usr/sbin:/sbin")),
            ("SHELL", OsString::from("/bin/sh")),
            ("TERM", OsString::from("xterm-256color")),
            ("LANG", OsString::from("C")),
        ]
    }

    fn apply_env(&self, command: &mut Command) {
        command.env_clear().current_dir(self.root.path());
        for (key, value) in self.isolation_env() {
            command.env(key, value);
        }
    }

    fn supervisor_command(&self) -> Command {
        let mut command = Command::new(BIN);
        command.arg(SUPERVISOR_ARG);
        self.apply_env(&mut command);
        command
    }

    fn keeper_command(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        self.apply_env(&mut command);
        command
    }

    fn daemon_socket(&self) -> PathBuf {
        self.path("runtime/daemon.sock")
    }

    fn successor_command(&self, legacy_socket: &Path) -> Command {
        let mut command = Command::new(BIN);
        command.arg("--daemon").arg("--handover-from").arg(legacy_socket);
        self.apply_env(&mut command);
        command
    }
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

fn path_is_a_unix_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt as _;
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_socket())
        .unwrap_or(false)
}

struct Identity {
    role: &'static str,
    pid: u32,
}

struct Run {
    identities: Vec<Identity>,
    sender: tokio::sync::mpsc::UnboundedSender<(u32, ExitStatus)>,
    exited: tokio::sync::mpsc::UnboundedReceiver<(u32, ExitStatus)>,
    statuses: std::collections::HashMap<u32, ExitStatus>,
    waiters: Vec<std::thread::JoinHandle<()>>,
}

impl Run {
    fn new() -> Self {
        let (sender, exited) = tokio::sync::mpsc::unbounded_channel();
        Self {
            identities: Vec::new(),
            sender,
            exited,
            statuses: std::collections::HashMap::new(),
            waiters: Vec::new(),
        }
    }

    fn record(&mut self, role: &'static str, pid: u32) {
        assert!(pid > 1, "refusing to record pid {pid}");
        assert_ne!(
            pid,
            std::process::id(),
            "refusing to record the harness process itself"
        );
        if !self.identities.iter().any(|known| known.pid == pid) {
            self.identities.push(Identity { role, pid });
        }
    }

    fn spawn(&mut self, role: &'static str, mut child: Child) -> u32 {
        let pid = child.id();
        self.record(role, pid);
        let sender = self.sender.clone();
        let waiter = std::thread::Builder::new()
            .name(format!("fx-rk-{role}"))
            .spawn(move || {
                if let Ok(status) = child.wait() {
                    let _ = sender.send((pid, status));
                }
            })
            .expect("process waiter thread");
        self.waiters.push(waiter);
        pid
    }

    fn forget(&mut self, pid: u32) {
        self.identities.retain(|known| known.pid != pid);
    }

    fn role_of(&self, pid: u32) -> Option<&'static str> {
        self.identities
            .iter()
            .find(|known| known.pid == pid)
            .map(|known| known.role)
    }

    fn terminate(&mut self, pid: u32) -> Result<(), String> {
        if pid <= 1 {
            return Err(format!("refusing to signal pid {pid}"));
        }
        if pid == std::process::id() {
            return Err("refusing to signal the harness itself".to_string());
        }
        if self.role_of(pid).is_none() {
            return Err(format!(
                "refusing to signal pid {pid}: this run never created it"
            ));
        }
        // SAFETY: SIGKILL to a pid this run recorded when it spawned that process. A pid that has
        // already exited answers ESRCH, which is an expected outcome here.
        let sent = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        self.forget(pid);
        if sent == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!(
                "kill {pid} failed: {}",
                std::io::Error::last_os_error()
            ))
        }
    }

    fn stop(&mut self, pid: u32) -> Result<(), String> {
        if pid <= 1 || pid == std::process::id() || self.role_of(pid).is_none() {
            return self.terminate(pid);
        }
        // SAFETY: SIGTERM to a pid this run recorded, which is the default disposition the keeper
        // relies on for its own exit. The caller removes any socket the process left behind.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
        Ok(())
    }

    async fn await_exit(&mut self, pid: u32, what: &str) -> ExitStatus {
        if let Some(status) = self.statuses.remove(&pid) {
            self.forget(pid);
            return status;
        }
        let deadline = Instant::now() + CASE_WAIT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                panic!("{what} (pid {pid}) did not exit within {CASE_WAIT:?}");
            }
            let next = tokio::time::timeout(remaining, self.exited.recv()).await;
            match next {
                Ok(Some((exited, status))) if exited == pid => {
                    self.forget(pid);
                    return status;
                }
                Ok(Some((exited, status))) => {
                    self.statuses.insert(exited, status);
                }
                Ok(None) => {
                    panic!("every tracked process exited without reporting {what} (pid {pid})")
                }
                Err(_) => panic!("{what} (pid {pid}) did not exit within {CASE_WAIT:?}"),
            }
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let pids: Vec<u32> = self.identities.iter().map(|known| known.pid).collect();
        for pid in pids {
            let _ = self.terminate(pid);
        }
        for waiter in self.waiters.drain(..) {
            let _ = waiter.join();
        }
    }
}

fn process_is_alive(pid: u32) -> bool {
    // SAFETY: signal 0 is a pure existence probe and sends nothing.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

async fn await_process_gone(pid: u32, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if !process_is_alive(pid) {
            return true;
        }
        tokio::time::sleep(WAKE_NET).await;
    }
    !process_is_alive(pid)
}

struct DirWatch {
    _watcher: notify::RecommendedWatcher,
    rx: tokio::sync::mpsc::UnboundedReceiver<()>,
}

impl DirWatch {
    fn new(dir: &Path) -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok() {
                let _ = tx.send(());
            }
        })
        .expect("a directory watcher");
        watcher
            .watch(dir, notify::RecursiveMode::NonRecursive)
            .expect("watch the case directory");
        Self {
            _watcher: watcher,
            rx,
        }
    }

    async fn settled(&mut self, deadline: Instant) -> bool {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let _ = tokio::time::timeout(remaining.min(WAKE_NET), self.rx.recv()).await;
        Instant::now() < deadline
    }
}

struct RealKeeper {
    pid: u32,
    socket: PathBuf,
}

impl RealKeeper {
    async fn start(case: &mut Case, binary: &Path) -> Self {
        let mut command = case.fixture.keeper_command(binary);
        let stderr = std::fs::File::create(case.fixture.path("logs/keeper.err"))
            .expect("capture the keeper's stderr");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = command.spawn().expect("spawn the real pty keeper");
        let pid = case.run.spawn("keeper", child);
        let socket = case.fixture.keeper_socket();
        let runtime_dir = case.fixture.runtime();

        let deadline = Instant::now() + READY_WAIT;
        let mut last;
        loop {
            match KeeperClient::connect(&runtime_dir) {
                Ok(client) => {
                    drop(client);
                    return Self { pid, socket };
                }
                Err(error) => last = error.to_string(),
            }
            if !case.runtime_events.settled(deadline).await {
                panic!(
                    "the real keeper (pid {pid}) never answered HELLO at {} within {READY_WAIT:?} \
                     ({last}); keeper stderr:\n{}",
                    socket.display(),
                    case.diagnostics()
                );
            }
        }
    }
}

struct Case {
    run: Run,
    logs: DirWatch,
    runtime_events: DirWatch,
    fixture: Fixture,
}

impl Case {
    fn new(label: &str) -> Self {
        let fixture = Fixture::new(label);
        let logs = DirWatch::new(&fixture.path("data/logs"));
        let runtime_events = DirWatch::new(&fixture.runtime());
        Self {
            run: Run::new(),
            logs,
            runtime_events,
            fixture,
        }
    }

    fn spawn_supervisor(&mut self) -> u32 {
        let mut command = self.fixture.supervisor_command();
        let stderr = std::fs::File::create(self.fixture.path("logs/supervisor.err"))
            .expect("capture the supervisor's stderr");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = command.spawn().expect("spawn the crash supervisor");
        self.run.spawn("supervisor", child)
    }

    fn spawn_successor(&mut self, legacy_socket: &Path) -> u32 {
        let mut command = self.fixture.successor_command(legacy_socket);
        let stderr = std::fs::File::create(self.fixture.path("logs/successor.err"))
            .expect("capture the successor's stderr");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let child = command.spawn().expect("spawn the handover successor");
        self.run.spawn("successor", child)
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.fixture.restart_log()).unwrap_or_default()
    }

    fn diagnostics(&self) -> String {
        let mut out = String::new();
        for name in ["logs/supervisor.err", "logs/keeper.err"] {
            let path = self.fixture.path(name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push_str(&format!("--- {name} ---\n{text}\n"));
            }
        }
        if out.is_empty() {
            out.push_str("(no stderr was captured)\n");
        }
        out
    }

    async fn await_daemon_ready(&mut self) -> u32 {
        self.await_daemon_ready_excluding(None).await
    }

    async fn await_daemon_ready_excluding(&mut self, exclude: Option<u32>) -> u32 {
        let deadline = Instant::now() + READY_WAIT;
        let mut last;
        loop {
            match control_pid(&self.fixture.runtime()).await {
                Ok(Some(pid)) if Some(pid) != exclude => {
                    self.run.record("daemon", pid);
                    return pid;
                }
                Ok(Some(pid)) => last = format!("control endpoint still answers with pid {pid}"),
                Ok(None) => last = "no control endpoint yet".to_string(),
                Err(error) => last = format!("control endpoint error: {error}"),
            }
            if !self.runtime_events.settled(deadline).await {
                panic!(
                    "no daemon answered control-v1 within {READY_WAIT:?} ({last}); \
                     restart log:\n{}\n{}",
                    self.log_text(),
                    self.diagnostics()
                );
            }
        }
    }

    async fn await_log(&mut self, want: &str) -> String {
        let path = self.fixture.restart_log();
        let deadline = Instant::now() + LOG_WAIT;
        loop {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains(want) {
                return text;
            }
            if !self.logs.settled(deadline).await {
                panic!(
                    "the restart log never recorded {want:?} within {LOG_WAIT:?}; it holds:\n{text}\n{}",
                    self.diagnostics()
                );
            }
        }
    }
}

async fn control_pid(runtime: &Path) -> Result<Option<u32>, String> {
    match connect(runtime).await.map_err(|e| e.to_string())? {
        ControlConnection::Missing => Ok(None),
        ControlConnection::Connected(mut client) => {
            match client
                .call(&ControlRequest::Hello)
                .await
                .map_err(|e| e.to_string())?
            {
                ControlReply::Hello(hello) => Ok(Some(hello.pid)),
                other => Err(format!("Hello answered {other:?}")),
            }
        }
    }
}

async fn shutdown_daemon(runtime: &Path, expected_pid: u32) -> Result<u32, String> {
    let ControlConnection::Connected(mut client) =
        connect(runtime).await.map_err(|e| e.to_string())?
    else {
        return Err("no control endpoint to shut down".to_string());
    };
    let pid = match client
        .call(&ControlRequest::Hello)
        .await
        .map_err(|e| e.to_string())?
    {
        ControlReply::Hello(hello) => hello.pid,
        other => return Err(format!("Hello answered {other:?}")),
    };
    if pid != expected_pid {
        return Err(format!(
            "refusing to shut down pid {pid}: this case owns pid {expected_pid}"
        ));
    }
    match client
        .call(&ControlRequest::Shutdown {
            confirm_live_sessions: 0,
        })
        .await
        .map_err(|e| e.to_string())?
    {
        ControlReply::Live { live } => Ok(live),
        ControlReply::Failed(failure) => Err(format!("Shutdown refused: {failure:?}")),
        other => Err(format!("Shutdown answered {other:?}")),
    }
}

struct DataClient {
    reader: tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl DataClient {
    async fn connect(socket: &Path, expected_pid: u32) -> Result<Self, String> {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

        let stream = tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| format!("connect {}: {e}", socket.display()))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(read_half);

        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: None,
        };
        let mut frame = serde_json::to_string(&handshake).map_err(|e| e.to_string())?;
        frame.push('\n');
        write_half
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .map_err(|_| "handshake timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("EOF during handshake".to_string());
        }
        match serde_json::from_str::<DaemonResponse>(line.trim()).map_err(|e| e.to_string())? {
            DaemonResponse::HandshakeOk { pid, version, .. }
                if pid == expected_pid && version == DAEMON_PROTOCOL_VERSION => {}
            other => {
                return Err(format!(
                    "expected HandshakeOk for pid {expected_pid}, got {other:?}"
                ))
            }
        }
        Ok(Self {
            reader,
            writer: write_half,
        })
    }

    async fn request(&mut self, request: &DaemonRequest) -> Result<DaemonResponse, String> {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};

        let mut frame = serde_json::to_string(request).map_err(|e| e.to_string())?;
        frame.push('\n');
        self.writer
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        self.writer.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), self.reader.read_line(&mut line))
            .await
            .map_err(|_| "request timed out".to_string())?
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("EOF from the daemon".to_string());
        }
        serde_json::from_str(line.trim()).map_err(|e| e.to_string())
    }
}

fn assert_log(text: &str, needle: &str, why: &str) {
    assert!(text.contains(needle), "{why}\nrestart log:\n{text}");
}

fn assert_no_log(text: &str, needle: &str, why: &str) {
    assert!(!text.contains(needle), "{why}\nrestart log:\n{text}");
}

fn count_log(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[test]
fn the_real_keeper_binary_is_either_usable_or_absent_for_a_named_reason() {
    match keeper_binary() {
        Ok(binary) => {
            assert!(binary.is_file(), "{} must be a file", binary.display());
            assert_eq!(
                binary.file_name().and_then(|name| name.to_str()),
                Some(KEEPER_BINARY_NAME),
                "the resolved path must be the keeper binary itself"
            );
            let version = Command::new(&binary)
                .arg("--version")
                .env_clear()
                .output()
                .expect("run the keeper's --version");
            assert!(
                version.status.success(),
                "--version must succeed; stderr: {}",
                String::from_utf8_lossy(&version.stderr)
            );
            assert!(
                !String::from_utf8_lossy(&version.stdout).trim().is_empty(),
                "--version must report a version"
            );
        }
        Err(reason) => {
            eprintln!("real keeper cases are not exercised here: {reason}");
            assert!(
                reason.starts_with("KEEPER_PAYLOAD_ABSENT"),
                "an absent keeper is a typed condition, got: {reason}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_keeper_opens_the_gate_naming_its_own_pid() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("gate");
    let keeper = RealKeeper::start(&mut case, &binary).await;
    assert!(
        path_is_a_unix_socket(&case.fixture.keeper_socket()),
        "the keeper derives its socket from FERRYX_RUNTIME_DIR, so it must appear there"
    );

    case.fixture.activate_keeper_ownership().await;
    let supervisor = case.spawn_supervisor();
    let text = case.await_log(GATE_LINE).await;
    let gate = text
        .lines()
        .find(|line| line.contains(GATE_LINE))
        .expect("a gate line");
    assert!(
        gate.contains(GATE_ON),
        "a live real keeper must open the gate\nline: {gate}"
    );
    assert!(
        gate.contains(&format!("pid {})", keeper.pid)),
        "the gate must name the keeper process the kernel corroborated, not the daemon's pid or the \
         harness's; expected pid {} in:\n{gate}",
        keeper.pid
    );

    let daemon = case.await_daemon_ready().await;
    shutdown_daemon(&case.fixture.runtime(), daemon)
        .await
        .expect("stop the daemon this case started");
    let _ = case.run.await_exit(supervisor, "the supervisor").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_keeper_restarts_once_after_an_abnormal_exit_then_exit_zero_stops() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("restart");
    let keeper = RealKeeper::start(&mut case, &binary).await;

    case.fixture.activate_keeper_ownership().await;
    let supervisor = case.spawn_supervisor();
    let text = case.await_log(GATE_LINE).await;
    assert_log(
        &text,
        GATE_ON,
        "a live real keeper must authorise supervision",
    );
    assert_log(
        &text,
        &format!("pid {})", keeper.pid),
        "the gate names the real keeper",
    );

    let crashed = case.await_daemon_ready().await;
    case.run.terminate(crashed).expect("crash the first daemon");

    let exit_text = case.await_log(ABNORMAL_LINE).await;
    assert_log(
        &exit_text,
        &format!("event=exit pid={crashed}"),
        "the abnormal exit must be recorded against the pid that died",
    );
    let restart_text = case.await_log(RESTART_LINE).await;
    assert_log(
        &restart_text,
        "decision=restart",
        "an abnormal exit with a live keeper must be restarted",
    );

    let restarted = case.await_daemon_ready_excluding(Some(crashed)).await;
    assert_ne!(restarted, crashed, "the restart must be a new daemon process");

    shutdown_daemon(&case.fixture.runtime(), restarted)
        .await
        .expect("stop the restarted daemon deliberately");
    let stop_text = case.await_log(STOP_LINE).await;
    assert_log(
        &stop_text,
        "reason=intentional:clean_exit",
        "an exit 0 is deliberate and must stop supervision",
    );
    assert_eq!(
        count_log(&stop_text, RESTART_LINE),
        1,
        "a deliberate exit must not trigger a second restart\nrestart log:\n{stop_text}"
    );
    let _ = case.run.await_exit(supervisor, "the supervisor").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_keeper_keeps_owning_the_pty_after_the_client_dies() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("owns");
    let keeper = RealKeeper::start(&mut case, &binary).await;
    let runtime = case.fixture.runtime();

    let mut client = KeeperClient::connect(&runtime).expect("handshake with the real keeper");
    let session = "p2-real-owner";
    let spawn = client
        .spawn(
            session,
            &["/bin/sh".to_string()],
            &[("PS1".to_string(), "$ ".to_string())],
            None,
            80,
            24,
            1,
        )
        .expect("the real keeper spawns a real PTY");
    assert!(spawn.pid > 1, "a real child pid");
    assert!(
        spawn.process_start_time.is_some(),
        "the keeper reports the child's kernel start time on unix, the identity a reused pid cannot \
         forge"
    );
    case.run.record("keeper-pty-child", spawn.pid);

    let owned = client.ownership().expect("authoritative discovery");
    let entry = owned
        .iter()
        .find(|entry| entry.session_id == session)
        .expect("the keeper is the source of truth for the sessions it owns");
    assert_eq!(entry.pid, spawn.pid, "discovery reports the owned child");
    assert_eq!(
        entry.owner, "keeper",
        "the master must be held by the keeper, not the daemon"
    );
    assert_eq!(
        entry.sequence_offset, spawn.sequence_offset,
        "the owner reports the same output head the spawn did"
    );
    assert!(
        process_is_alive(spawn.pid),
        "the keeper's child must be a live process"
    );

    drop(client);

    let mut successor = KeeperClient::connect(&runtime).expect("a second client may connect");
    let reattached = successor.ownership().expect("discovery after the client died");
    let same = reattached
        .iter()
        .find(|entry| entry.session_id == session)
        .expect("the session outlives the client that spawned it");
    assert_eq!(same.pid, spawn.pid, "the child identity must be unchanged");
    assert!(
        process_is_alive(spawn.pid),
        "the PTY and its child survive the death of the client, which is the keeper's whole purpose"
    );

    successor
        .kill(session, spawn.epoch)
        .expect("the keeper kills the child it owns");
    assert!(
        await_process_gone(spawn.pid, READY_WAIT).await,
        "the killed child must be gone"
    );
    case.run.forget(spawn.pid);

    case.run.stop(keeper.pid).expect("ask the real keeper to stop");
    let _ = case.run.await_exit(keeper.pid, "the real keeper").await;
    let _ = std::fs::remove_file(&keeper.socket);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_keeper_socket_refuses_supervised_restart() {
    let mut case = Case::new("dead");
    let socket = case.fixture.keeper_socket();
    {
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind then abandon");
        set_mode(&socket, 0o600);
        drop(listener);
    }
    assert!(
        path_is_a_unix_socket(&socket),
        "the unreachable socket must exist, so the probe refuses because nothing answers rather \
         than reporting a missing socket"
    );
    assert!(
        KeeperClient::connect(&case.fixture.runtime()).is_err(),
        "nothing may answer on an abandoned socket"
    );

    let supervisor = case.spawn_supervisor();
    let text = case.await_log(BANNER_LINE).await;
    assert_log(
        &text,
        "reason=no_live_keeper",
        "an unreachable keeper must refuse supervised restart",
    );
    assert_log(&text, GATE_OFF, "the gate must be recorded as closed");
    assert_no_log(
        &text,
        "decision=restart",
        "a refused gate must never restart the daemon",
    );

    case.await_daemon_ready().await;
    let status = case.run.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "a refused gate still serves this login once and exits cleanly, got {status:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_keeper_killed_mid_run_refuses_the_restart() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("killed");
    let keeper = RealKeeper::start(&mut case, &binary).await;

    case.fixture.activate_keeper_ownership().await;
    let supervisor = case.spawn_supervisor();
    let text = case.await_log(GATE_LINE).await;
    assert_log(&text, GATE_ON, "the gate opens for the live keeper");
    assert_log(
        &text,
        &format!("pid {})", keeper.pid),
        "the gate names the real keeper",
    );
    let daemon = case.await_daemon_ready().await;

    case.run.terminate(keeper.pid).expect("kill the keeper");
    let _ = case.run.await_exit(keeper.pid, "the real keeper").await;
    assert!(
        KeeperClient::connect(&case.fixture.runtime()).is_err(),
        "a killed keeper must not answer any more handshakes"
    );

    case.run.terminate(daemon).expect("crash the daemon");

    let refusal = case.await_log(BANNER_LINE).await;
    assert_log(
        &refusal,
        "reason=no_live_keeper",
        "the gate is re-evaluated after the exit, so a dead keeper refuses the restart",
    );
    assert_log(&refusal, GATE_OFF, "the gate is recorded as closed");
    assert_no_log(
        &refusal,
        "decision=restart",
        "a keeper that died mid-run must not authorise a restart",
    );
    let _ = case.run.await_exit(supervisor, "the supervisor").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_handover_with_a_real_keeper_exits_without_a_restart_and_keeps_the_keeper() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("handover");
    let keeper = RealKeeper::start(&mut case, &binary).await;

    case.fixture.activate_keeper_ownership().await;
    let supervisor = case.spawn_supervisor();
    let gate_text = case.await_log(GATE_LINE).await;
    assert_log(&gate_text, GATE_ON, "the gate opens for the live keeper");
    assert_log(
        &gate_text,
        &format!("pid {})", keeper.pid),
        "the gate names the real keeper",
    );

    let predecessor = case.await_daemon_ready().await;
    let mut client = DataClient::connect(&case.fixture.daemon_socket(), predecessor)
        .await
        .expect("data-protocol handshake with the supervised daemon");
    let legacy_socket = match client.request(&DaemonRequest::PrepareHandover).await {
        Ok(DaemonResponse::PrepareHandoverOk {
            legacy_socket_path,
            active_sessions,
        }) => {
            assert!(
                active_sessions.is_empty(),
                "a zero-session handover is legal and is what this case exercises: \
                 {active_sessions:?}"
            );
            PathBuf::from(legacy_socket_path)
        }
        other => panic!("PrepareHandover failed: {other:?}"),
    };
    assert!(
        legacy_socket.starts_with(case.fixture.runtime()),
        "the legacy socket must live in this case's runtime: {}",
        legacy_socket.display()
    );
    drop(client);

    let successor = case.spawn_successor(&legacy_socket);
    assert_ne!(successor, predecessor);

    let adopted = case.await_daemon_ready_excluding(Some(predecessor)).await;
    assert_eq!(
        adopted, successor,
        "the daemon serving the endpoint after commit must be the successor"
    );
    case.run.forget(predecessor);
    DataClient::connect(&case.fixture.daemon_socket(), successor)
        .await
        .expect("the successor serves the canonical data endpoint with its own pid");

    let text = case.await_log(STOP_LINE).await;
    assert_log(
        &text,
        &format!("pid={predecessor}"),
        "the predecessor's exit is on record: {text}",
    );
    assert_log(
        &text,
        "kind=handover",
        "a committed handover is classified from the decision record the commit wrote, not from the \
         exit status: {text}",
    );
    assert_log(
        &text,
        "reason=intentional:handover",
        "a handover must never be treated as a crash: {text}",
    );
    assert_no_log(
        &text,
        RESTART_LINE,
        "a handover must never be answered by a restart",
    );

    let status = case.run.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "the supervisor exits 0 after the handover, got {status:?}; restart log:\n{text}"
    );
    assert!(
        process_is_alive(successor),
        "the successor must still serve this runtime"
    );

    // The evidence behind the classification: the commit's durable decision record for the pid that
    // retired, in this case's own runtime directory.
    let records: Vec<serde_json::Value> = std::fs::read_dir(case.fixture.runtime())
        .expect("the runtime directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with(&format!("legacy-{predecessor}-"))
                        && name.ends_with(".transaction")
                })
        })
        .filter_map(|path| serde_json::from_slice(&std::fs::read(path).ok()?).ok())
        .collect();
    assert_eq!(
        records.len(),
        1,
        "the commit writes exactly one decision record for the retiring predecessor"
    );
    assert_eq!(
        records[0].get("predecessorPid").and_then(|pid| pid.as_u64()),
        Some(u64::from(predecessor)),
        "the record must name the pid that retired: {records:?}"
    );
    assert_eq!(
        records[0].get("state").and_then(|state| state.as_str()),
        Some("retired"),
        "the record must carry the committed state the supervisor reads: {records:?}"
    );

    let mut after = KeeperClient::connect(&case.fixture.runtime())
        .expect("the keeper must still answer after the handover");
    let owned = after.ownership().expect("keeper discovery after the handover");
    assert!(
        owned.is_empty(),
        "a zero-session handover leaves the keeper owning nothing: {owned:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn six_rapid_failures_with_a_real_keeper_stop_at_the_restart_limit() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let mut case = Case::new("limit");
    let keeper = RealKeeper::start(&mut case, &binary).await;

    case.fixture.activate_keeper_ownership().await;
    let supervisor = case.spawn_supervisor();
    let gate_text = case.await_log(GATE_LINE).await;
    assert_log(&gate_text, GATE_ON, "the gate opens for the live keeper");
    assert_log(
        &gate_text,
        &format!("pid {})", keeper.pid),
        "the gate names the real keeper",
    );

    let mut previous = None;
    let mut crashed = Vec::new();
    for _ in 0..=MAX_RESTARTS_PER_WINDOW {
        let daemon = case.await_daemon_ready_excluding(previous).await;
        case.run.terminate(daemon).expect("crash the daemon");
        crashed.push(daemon);
        previous = Some(daemon);
    }
    assert_eq!(
        crashed.len(),
        MAX_RESTARTS_PER_WINDOW + 1,
        "the initial start plus one start per allowed restart"
    );

    let text = case.await_log(BANNER_LINE).await;
    assert_log(
        &text,
        "reason=restart_limit",
        "the stop reason names the exhausted budget: {text}",
    );
    assert_eq!(
        count_log(&text, RESTART_LINE),
        MAX_RESTARTS_PER_WINDOW,
        "the cap is {MAX_RESTARTS_PER_WINDOW} restarts per window: {text}"
    );
    assert_eq!(
        count_log(&text, ABNORMAL_LINE),
        MAX_RESTARTS_PER_WINDOW + 1,
        "every one of the six deaths is recorded as a crash: {text}"
    );
    for attempt in 1..=MAX_RESTARTS_PER_WINDOW {
        assert_log(
            &text,
            &format!("attempt={attempt}"),
            "each restart is numbered: {text}",
        );
    }
    assert_log(
        &text,
        "decision=stop",
        "the stop itself is a decision on record: {text}",
    );
    assert_log(
        &text,
        &format!("crashed {MAX_RESTARTS_PER_WINDOW} times in 10 minutes"),
        "the banner states the limit it hit: {text}",
    );
    assert_no_log(
        &text,
        GATE_OFF,
        "the keeper stayed live throughout, so the gate never closed: {text}",
    );

    let status = case.run.await_exit(supervisor, "the supervisor").await;
    assert!(
        status.success(),
        "hitting the limit is a normal stop, got {status:?}; restart log:\n{text}"
    );
    assert_eq!(
        control_pid(&case.fixture.runtime()).await,
        Ok(None),
        "no daemon may be serving this runtime after the limit stop"
    );
    assert!(
        process_is_alive(keeper.pid),
        "the keeper is not the process under supervision and must survive the limit stop"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_qa_keeper_job_names_the_isolated_runtime_and_never_the_production_label() {
    let binary = keeper_binary().expect("this case needs the real keeper binary");
    let fixture = Fixture::new("job");
    let env = fixture.isolation_env();
    let lookup = move |key: &str| -> Option<OsString> {
        env.iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.clone())
    };

    let service = keeper_service_for(cfg!(debug_assertions), true, &lookup)
        .expect("an isolated runtime still gets a keeper job");
    assert_eq!(
        service.label, QA_PLIST_LABEL,
        "an isolated runtime must never be given the user's production keeper label"
    );
    assert_ne!(service.label, PLIST_LABEL, "the production label is not usable here");
    assert_eq!(
        service.plist_path,
        fixture.keeper_plist(),
        "the plist path must come from the fixture's override"
    );
    assert!(
        service.plist_path.starts_with(fixture.path("launchd")),
        "the plist must land inside this case"
    );

    let plist = service.render(&binary, false);
    assert!(
        plist.contains(&format!("<string>{QA_PLIST_LABEL}</string>")),
        "the rendered job must carry the QA label:\n{plist}"
    );
    assert!(
        !plist.contains(&format!("<string>{PLIST_LABEL}</string>")),
        "the production label must appear nowhere in a QA job:\n{plist}"
    );
    assert!(
        plist.contains(&*fixture.runtime().to_string_lossy()),
        "the job must forward the isolated runtime so the keeper binds this case's socket:\n{plist}"
    );
    assert!(
        !plist.contains("--socket"),
        "the endpoint must stay derived from the runtime, never passed as an argument:\n{plist}"
    );
    assert!(
        plist.contains(&*binary.to_string_lossy()),
        "the job must run the keeper binary resolved for this run:\n{plist}"
    );
    assert!(
        !fixture.keeper_plist().exists(),
        "rendering a plist is not installing one: no test may write a job"
    );
}

#[test]
fn the_run_guard_refuses_to_signal_any_process_it_did_not_create() {
    let mut run = Run::new();
    for foreign in [0, 1, std::process::id()] {
        let error = run
            .terminate(foreign)
            .expect_err("a pid outside this run must be refused");
        assert!(
            error.contains("refusing to signal"),
            "the refusal must say so, got: {error}"
        );
    }
    let stranger = Command::new("/bin/sh")
        .arg("-c")
        .arg("exit 0")
        .spawn()
        .map(|mut child| {
            let _ = child.wait();
            child.id()
        })
        .expect("a process this run did not record");
    let error = run
        .terminate(stranger)
        .expect_err("an unrecorded pid must be refused even when it once existed");
    assert!(
        error.contains("never created"),
        "the refusal must name the reason, got: {error}"
    );
    assert_eq!(run.role_of(stranger), None);
}

/// The real-`launchd` acceptance: the plan's Phase 2 cases run through the job that actually owns
/// supervision, not through a supervisor this harness spawned itself.
///
/// Every case here is `#[ignore]`d and gated on `FERRYX_REAL_LAUNCHD_SUPERVISOR`. Unasked, the file
/// reports them as ignored - the honest status for "not run" - and asked, every prerequisite must
/// hold: [`require_real_launchd`] panics naming the unmet one, so a Darwin host without a GUI domain
/// or without the keeper binary fails the case instead of passing it.
///
/// Nothing here can reach the user's jobs. Each label is minted from this fixture's own directory
/// name and proven private with `keeper_launchd::is_private_label` before `launchctl` sees it, so
/// `com.ferryx.daemon`, `com.ferryx.ptykeeper`, `com.ferryx.qa.ptykeeper` and `com.rorca.daemon` are
/// never written, bootstrapped, booted out or kicked. Every job forwards this fixture's runtime,
/// data, session, keeper and plist paths, because a LaunchAgent inherits no shell environment.
#[cfg(target_os = "macos")]
mod real_launchd {
    use super::*;
    use ferryx_lib::daemon::crash_supervisor::SUPERVISOR_LOCK_NAME;
    use ferryx_lib::daemon::keeper_launchd::{
        generate_keeper_launchd_plist, is_private_label, keeper_service_with_label, PLIST_PATH_ENV,
    };
    use ferryx_lib::daemon::launchd::{
        generate_launchd_plist, launchctl_job_pid, launchctl_print_program, plist_label,
        plist_program_arguments, supervisor_chain_from_plist, supervisor_chain_from_print,
        with_job_environment,
    };

    /// Label prefix for a private run. `com.ferryx.qa.` is this project's own namespace for jobs that
    /// are not the user's, and the suffix makes each run unmistakeable in `launchctl list`.
    const LABEL_PREFIX: &str = "com.ferryx.qa.supervisor-acceptance";

    /// The program the job runs. A `cargo test` build defaults it to this harness's own CLI, which
    /// exercises the same code but hashes to different bytes than the shipped daemon; the combined
    /// gate binds its Phase 2 evidence to the candidate's daemon digest, so a release run points this
    /// at `Ferryx.app/Contents/MacOS/ferryx`. In production the job's program *is* the daemon binary
    /// - `ProcessHost` re-executes `current_exe()` - which is why one digest covers both.
    const PROGRAM_ENV: &str = "FERRYX_LAUNCHD_PROGRAM";

    /// Where to write the gate's chain evidence (`chain.json` plus the three raw artifacts). Unset
    /// means write nothing: a plain `cargo test` must not create files nobody asked for.
    const EVIDENCE_DIR_ENV: &str = "FERRYX_LAUNCHD_EVIDENCE_DIR";

    /// `combined-release-gate.mjs::LAUNCHD_CHAIN_SCHEMA`. Duplicated on purpose: the test must not
    /// import the gate, and a schema the gate stopped reading would be caught as a failing gate step
    /// rather than as a silently stale constant here.
    const CHAIN_SCHEMA: &str = "ferryx-launchd-supervisor-chain/1";

    /// Cold-start budget for a job: launchd, then the supervisor, then a daemon that hashes its own
    /// executable before it binds control-v1.
    const JOB_WAIT: Duration = Duration::from_secs(120);

    /// How long "launchd never relaunches this" is observed for. A negative claim has no event to
    /// subscribe to, so this is the one place a bounded window is the criterion; it is short because
    /// the positive evidence is the job's own state, and this only catches a late relaunch.
    const NEGATIVE_WINDOW: Duration = Duration::from_secs(5);

    /// How often a job's state is re-read - its `launchctl print`, or a pid's `ps` identity. Both
    /// are subprocesses, so the deadline, not this interval, is the criterion.
    const JOB_POLL: Duration = Duration::from_millis(500);

    /// How long a pid launchd reported is given to describe itself as the job's binary.
    ///
    /// `launchctl print` reports a job's pid as soon as launchd has forked the child, before the
    /// child has exec'd the job's program: a just-forked child still answers to its parent's name,
    /// so one `ps -ocomm` read can describe the pre-exec image and the next the real program.
    /// Darwin offers no exec notification to subscribe to for an arbitrary pid - no kqueue or `poll`
    /// event reports "this pid replaced its image" - so the pid's own `comm` is the only observable,
    /// and a bounded, event-free poll is how to wait for it. The deadline is still the criterion: a
    /// pid whose `comm` never names the expected binary fails.
    const COMM_WAIT: Duration = Duration::from_secs(10);

    fn domain() -> String {
        // SAFETY: `getuid` takes no arguments, dereferences no pointers and has no side effects.
        format!("gui/{}", unsafe { libc::getuid() })
    }

    fn launchctl(args: &[&str]) -> Result<String, String> {
        let output = Command::new("/bin/launchctl")
            .args(args)
            .output()
            .map_err(|error| format!("launchctl {} could not run: {error}", args.join(" ")))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!(
                "launchctl {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    /// One `ps -o <field>= -p <pid>` reading. The identity of a pid is what makes it safe to signal:
    /// a pid that launchd reported and `ps` describes as this run's program is this run's process.
    fn ps_field(pid: u32, field: &str) -> Option<String> {
        let output = Command::new("/bin/ps")
            .arg(format!("-o{field}="))
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!value.is_empty()).then_some(value)
    }

    /// The ask variable is the whole gate: unset means "not run", set means every prerequisite must
    /// hold. There is no branch that returns early, so an unmeetable host fails rather than passes.
    fn require_real_launchd() -> Prerequisites {
        assert!(
            real_launchd_asked(),
            "the real-launchd cases are asked for by setting {REAL_LAUNCHD_ASK_ENV}=1; a run without \
             it must stay `ignored`, never green"
        );
        let keeper = keeper_binary().unwrap_or_else(|reason| {
            panic!("{REAL_LAUNCHD_ASK_ENV} is set but the launchd cases cannot run: {reason}")
        });
        // `/bin/launchctl` and a reachable GUI domain are the prerequisites this harness cannot
        // create: a job can only be bootstrapped into a session that exists.
        launchctl(&["print", &domain()]).unwrap_or_else(|error| {
            panic!(
                "{REAL_LAUNCHD_ASK_ENV} is set but {} is not reachable ({error}); these cases need a \
                 real Darwin GUI session for this uid",
                domain()
            )
        });
        assert!(
            Path::new("/bin/ps").is_file(),
            "{REAL_LAUNCHD_ASK_ENV} is set but /bin/ps is missing, so a pid could not be identified \
             before it is signalled"
        );
        let program = std::env::var_os(PROGRAM_ENV)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from(BIN));
        assert!(
            program.is_file(),
            "{PROGRAM_ENV} names {}, which is not a file; the job it is bootstrapped from would not \
             start",
            program.display()
        );
        Prerequisites { keeper, program }
    }

    /// What a run needs before it can bootstrap anything.
    struct Prerequisites {
        keeper: PathBuf,
        program: PathBuf,
    }

    /// A job this run bootstrapped, and the plist it was bootstrapped from.
    struct Job {
        label: String,
        plist: PathBuf,
    }

    impl Job {
        fn print(&self) -> Result<String, String> {
            launchctl(&["print", &format!("{}/{}", domain(), self.label)])
        }

        fn pid(&self) -> Option<u32> {
            self.print().ok().and_then(|print| launchctl_job_pid(&print))
        }
    }

    /// The fixture plus the jobs it bootstrapped, so teardown cannot be forgotten: every label is
    /// booted out, then every pid this run learned is terminated, then the tree is removed. The fields
    /// are declared in teardown order - `Drop` boots the jobs out, then `run` kills what is left,
    /// then `fixture` removes the tree - so nothing is ever left writing into a deleted directory.
    struct Lab {
        run: Run,
        fixture: Fixture,
        /// The executable the supervisor job runs: this build's CLI by default, or
        /// `$PROGRAM_ENV`'s path so a release run can bind the evidence to the shipped daemon bytes.
        program: PathBuf,
        labels: Vec<String>,
    }

    impl Lab {
        fn new(name: &str, program: &Path) -> Self {
            Self {
                run: Run::new(),
                fixture: Fixture::new(name),
                program: program.to_path_buf(),
                labels: Vec::new(),
            }
        }

        fn token(&self) -> String {
            self.fixture
                .root
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("run")
                .to_string()
        }

        /// A label for one role in this run, refused if it is not private.
        fn label(&self, role: &str) -> String {
            let label = format!("{LABEL_PREFIX}.{role}.{}", self.token());
            assert!(
                is_private_label(&label),
                "{label} would address a shipped job; an acceptance run must mint its own"
            );
            label
        }

        /// This fixture's environment as plist pairs: a LaunchAgent inherits nothing, so a job without
        /// these entries would start a daemon (or keeper) on the user's real runtime.
        ///
        /// `FERRYX_DAEMON_SUPERVISOR_NAME` is deliberately absent - it is what makes
        /// `supervisor::supported()` true, and a job carrying it would let the daemon install itself a
        /// login-time job under a shipped label.
        fn job_environment(&self) -> Vec<(String, String)> {
            let environment: Vec<(String, String)> = self
                .fixture
                .isolation_env()
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.to_string(),
                        value
                            .into_string()
                            .unwrap_or_else(|_| panic!("{key} must be UTF-8 to enter a plist")),
                    )
                })
                .collect();
            assert!(
                !environment
                    .iter()
                    .any(|(key, _)| key == "FERRYX_DAEMON_SUPERVISOR_NAME"),
                "a job that names a supervisor label would enable launchd self-management"
            );
            environment
        }

        fn write_plist(&self, label: &str, content: &str) -> PathBuf {
            let path = self.fixture.path("launchd").join(format!("{label}.plist"));
            std::fs::write(&path, content).expect("write the job plist");
            path
        }

        fn bootstrap(&mut self, label: &str, plist: &Path) -> Job {
            launchctl(&["bootstrap", &domain(), &plist.to_string_lossy()]).unwrap_or_else(|error| {
                panic!("bootstrap {label} from {}: {error}", plist.display())
            });
            self.labels.push(label.to_string());
            Job {
                label: label.to_string(),
                plist: plist.to_path_buf(),
            }
        }

        /// The supervisor job, exactly as production writes it: the same plist generator, the same
        /// `--daemon-crash-supervisor` chain, plus this fixture's environment.
        fn supervisor_job(&mut self) -> Job {
            let label = self.label("supervisor");
            let content = with_job_environment(
                generate_launchd_plist(&label, &self.program.to_string_lossy(), &self.fixture.path("logs"), true),
                &self.job_environment(),
            );
            assert_eq!(
                plist_label(&content).as_deref(),
                Some(label.as_str()),
                "the plist must carry this run's private label"
            );
            assert_eq!(
                supervisor_chain_from_plist(&content),
                Some(vec![SUPERVISOR_ARG.to_string()]),
                "the job must run the supervisor, never a bare daemon:\n{content}"
            );
            let path = self.write_plist(&label, &content);
            self.bootstrap(&label, &path)
        }

        /// The keeper job for this run: a private label, the real keeper binary, and the isolation
        /// environment a private label forwards.
        fn keeper_job(&mut self, binary: &Path) -> Job {
            let label = self.label("keeper");
            let env = self.fixture.isolation_env();
            // The fixture's plist override names the *shipped* QA keeper plist; a private run must
            // derive its own path from its own label, exactly as the production job does, so the two
            // can never be confused in `launchctl list` or on disk.
            let lookup = move |key: &str| {
                if key == PLIST_PATH_ENV {
                    return None;
                }
                env.iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| value.clone())
            };
            let service = keeper_service_with_label(&label, true, true, &lookup)
                .unwrap_or_else(|error| panic!("a private keeper job for {label}: {error}"));
            assert_eq!(service.label, label, "the job must keep this run's label");
            assert!(
                !service.environment.is_empty(),
                "a private label must forward the isolation environment, otherwise the keeper binds \
                 the user's own socket"
            );
            std::fs::create_dir_all(&service.log_dir).expect("the job's log directory");
            // The keeper job's own `EnvironmentVariables` must be exactly the subset of this
            // fixture's isolation variables - same keys, same values - because a job that disagrees
            // with the harness about the runtime is a job pointed at the user's socket. The merged
            // dict is applied once, so the plist never carries two `EnvironmentVariables` keys.
            let environment = self.job_environment();
            for (key, value) in &service.environment {
                let forwarded = environment
                    .iter()
                    .find(|(name, _)| name == key)
                    .unwrap_or_else(|| panic!("the job forwards {key}, which the fixture does not"));
                assert_eq!(&forwarded.1, value, "{key} must forward one value only");
            }
            let base = generate_keeper_launchd_plist(&label, &binary.to_string_lossy(), &service.log_dir, true);
            let content = with_job_environment(base, &environment);
            assert_eq!(
                plist_program_arguments(&content),
                Some(vec![binary.to_string_lossy().into_owned()]),
                "the keeper job runs the keeper binary and nothing else:\n{content}"
            );
            std::fs::create_dir_all(service.plist_path.parent().expect("a plist parent"))
                .expect("the launch agents directory");
            assert_eq!(
                service.plist_path,
                self.fixture
                    .path("home/Library/LaunchAgents")
                    .join(format!("{label}.plist")),
                "a private job derives its own plist path from its own label"
            );
            std::fs::write(&service.plist_path, &content).expect("write the keeper plist");
            self.bootstrap(&label, &service.plist_path)
        }

        /// A supervisor this harness starts itself, for the ownership case where the second
        /// supervisor must be a process launchd did *not* create. It is the same program the job runs,
        /// with the same environment, so it contends for the same runtime claim.
        fn spawn_supervisor_process(&mut self, role: &'static str, stderr: &str) -> u32 {
            let mut command = Command::new(&self.program);
            command.arg(SUPERVISOR_ARG);
            self.fixture.apply_env(&mut command);
            let stderr =
                std::fs::File::create(self.fixture.path(stderr)).expect("capture supervisor stderr");
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(stderr));
            self.run.spawn(role, command.spawn().expect("spawn a supervisor"))
        }

        fn log_text(&self) -> String {
            std::fs::read_to_string(self.fixture.restart_log()).unwrap_or_default()
        }

        /// Writes the combined gate's Phase 2 evidence when `$EVIDENCE_DIR_ENV` names a directory.
        ///
        /// The gate does not read a test's verdict; it reads `chain.json` plus the three raw
        /// artifacts and re-checks them itself (`evaluateLaunchdChainEvidence`): the supervisor's
        /// parent must be pid 1, the label must not be a production one, the plist must run
        /// `--daemon-crash-supervisor`, the captured `launchctl print` must show `state = running`,
        /// a SIGKILL must have produced exactly one restart and a clean exit none, the daemon pid must
        /// have changed, and the daemon bytes must hash to the candidate's.
        ///
        /// So the capture is written only from a run that observed all of it, and each raw artifact's
        /// digest is computed over the exact text written: a summary that names a capture it did not
        /// ship is refused by the gate, and an empty evidence directory is worse than none.
        fn write_chain_evidence(&self, dir: &Path, observed: &ChainObservation) -> PathBuf {
            std::fs::create_dir_all(dir).expect("the evidence directory");
            let plist = std::fs::read_to_string(&observed.plist).expect("the job plist is readable");
            let restart_log = self.log_text();
            assert!(
                restart_log.contains("decision=restart"),
                "evidence from a run that never restarted would claim the opposite"
            );
            assert!(
                observed.print.contains("state = running"),
                "the gate requires a running job in the captured print:\n{}",
                observed.print
            );
            assert!(
                observed.supervisor_ppid == 1,
                "the gate requires launchd (pid 1) to own the supervisor, saw {}",
                observed.supervisor_ppid
            );
            assert!(
                observed.daemon_before != observed.daemon_after,
                "the gate requires the daemon pid to change across the crash"
            );
            let daemon_sha256 = sha256_of(&self.program);
            let head = git_head(&self.fixture.root.path());
            let raw = [
                ("launchctl-print.txt", observed.print.clone()),
                ("job.plist", plist),
                ("restart-log.txt", restart_log),
            ];
            let mut recorded = Vec::new();
            for (name, text) in &raw {
                std::fs::write(dir.join(name), text).expect("write a raw artifact");
                recorded.push(serde_json::json!({
                    "name": name,
                    "sha256": sha256_text(text),
                }));
            }
            let chain = serde_json::json!({
                "schema": CHAIN_SCHEMA,
                "platform": "darwin",
                "label": observed.label,
                "supervisorPpid": observed.supervisor_ppid,
                "killSignal": observed.kill_signal,
                "restartsAfterAbnormal": observed.restarts_after_abnormal,
                "restartsAfterExitZero": observed.restarts_after_exit_zero,
                "daemonPidBefore": observed.daemon_before,
                "daemonPidAfter": observed.daemon_after,
                "daemonSha256": daemon_sha256,
                "head": head,
                "raw": recorded,
            });
            let path = dir.join("chain.json");
            std::fs::write(
                &path,
                format!("{}\n", serde_json::to_string_pretty(&chain).expect("serialize")),
            )
            .expect("write chain.json");
            path
        }

        fn diagnostics(&self) -> String {
            let mut out = String::new();
            for name in [
                "logs/supervisor.err",
                "logs/second-supervisor.err",
                "logs/daemon.stdout.log",
                "logs/daemon.stderr.log",
            ] {
                let path = self.fixture.path(name);
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push_str(&format!("--- {name} ---\n{text}\n"));
                }
            }
            if out.is_empty() {
                out.push_str("(no stderr was captured)\n");
            }
            out
        }
    }

    impl Drop for Lab {
        fn drop(&mut self) {
            for label in self.labels.iter().rev() {
                // Best effort by design: a job that already exited answers "no such process", which
                // is the outcome this teardown wanted.
                let _ = launchctl(&["bootout", &format!("{}/{}", domain(), label)]);
            }
        }
    }

    /// Where the gate's Phase 2 evidence should be written, or `None` when this run was not asked for
    /// it. A plain `cargo test` must not leave files behind.
    fn evidence_dir() -> Option<PathBuf> {
        std::env::var_os(EVIDENCE_DIR_ENV)
            .map(PathBuf::from)
            .filter(|dir| !dir.as_os_str().is_empty())
    }

    /// What the gate's Phase 2 evidence has to state, gathered from the run that observed it.
    ///
    /// Every field is measured, never assumed: `write_chain_evidence` refuses to write a capture that
    /// would not survive the gate's own checks, so an evidence directory either holds a real
    /// observation or holds nothing.
    struct ChainObservation {
        label: String,
        plist: PathBuf,
        print: String,
        supervisor_ppid: u32,
        kill_signal: &'static str,
        restarts_after_abnormal: usize,
        restarts_after_exit_zero: usize,
        daemon_before: u32,
        daemon_after: u32,
    }

    /// The sha256 of a file's bytes, as the gate's receipt lines record it.
    fn sha256_of(path: &Path) -> String {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        let mut file = std::fs::File::open(path)
            .unwrap_or_else(|error| panic!("hash {}: {error}", path.display()));
        std::io::copy(&mut file, &mut hasher).expect("read the program's bytes");
        format!("{:x}", hasher.finalize())
    }

    /// The sha256 of a captured text, over exactly the bytes written to disk.
    fn sha256_text(text: &str) -> String {
        use sha2::Digest as _;
        format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
    }

    /// The commit the evidence belongs to. Absent git metadata is reported as `null`, which the gate
    /// treats as a missing revision, not as a match.
    fn git_head(_inside: &Path) -> Option<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?;
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!head.is_empty()).then_some(head)
    }

    /// A bounded condition poll. The condition is re-checked every round and the deadline is the
    /// bound; the interval is only how often it is re-read, never the pass criterion.
    async fn await_condition(what: &str, budget: Duration, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + budget;
        loop {
            if check() {
                return;
            }
            if Instant::now() >= deadline {
                panic!("{what} did not happen within {budget:?}");
            }
            tokio::time::sleep(JOB_POLL).await;
        }
    }

    /// The pid's `comm` once it names a binary containing `want`, or the last value read when
    /// [`COMM_WAIT`] expires. The caller asserts, so a failure reports exactly the read the wait
    /// ended on rather than a fresh one that could disagree with the predicate that failed.
    ///
    /// [`COMM_WAIT`]'s doc is why this waits: a pid launchd has just reported may not have exec'd,
    /// so the first read is not a statement about which program the pid will run.
    async fn await_comm(pid: u32, want: &str) -> Option<String> {
        let deadline = Instant::now() + COMM_WAIT;
        loop {
            let observed = ps_field(pid, "comm");
            if observed.as_deref().is_some_and(|comm| comm.contains(want)) {
                return observed;
            }
            if Instant::now() >= deadline {
                return observed;
            }
            tokio::time::sleep(JOB_POLL).await;
        }
    }

    async fn await_job_pid(job: &Job, what: &str) -> u32 {
        let deadline = Instant::now() + JOB_WAIT;
        let mut last;
        loop {
            match job.print() {
                Ok(print) => {
                    if let Some(pid) = launchctl_job_pid(&print) {
                        return pid;
                    }
                    last = format!("registered but not running:\n{print}");
                }
                Err(error) => last = error,
            }
            assert!(
                Instant::now() < deadline,
                "{what} (job {}) never reported a pid within {JOB_WAIT:?} ({last})",
                job.label
            );
            tokio::time::sleep(JOB_POLL).await;
        }
    }

    async fn await_log(path: &Path, want: &str) -> String {
        let deadline = Instant::now() + LOG_WAIT;
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.contains(want) {
                return text;
            }
            assert!(
                Instant::now() < deadline,
                "the restart log never recorded {want:?} within {LOG_WAIT:?}; it holds:\n{text}"
            );
            tokio::time::sleep(WAKE_NET).await;
        }
    }

    async fn await_daemon(runtime: &Path, exclude: Option<u32>) -> u32 {
        let deadline = Instant::now() + JOB_WAIT;
        let mut last;
        loop {
            match control_pid(runtime).await {
                Ok(Some(pid)) if Some(pid) != exclude => return pid,
                Ok(Some(pid)) => last = format!("control endpoint still answers with pid {pid}"),
                Ok(None) => last = "no control endpoint yet".to_string(),
                Err(error) => last = format!("control endpoint error: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "no daemon answered control-v1 within {JOB_WAIT:?} ({last})"
            );
            tokio::time::sleep(WAKE_NET).await;
        }
    }

    async fn await_keeper(runtime: &Path) {
        let deadline = Instant::now() + JOB_WAIT;
        let mut last;
        loop {
            match KeeperClient::connect(runtime) {
                Ok(client) => {
                    drop(client);
                    return;
                }
                Err(error) => last = error.to_string(),
            }
            assert!(
                Instant::now() < deadline,
                "the keeper job never answered HELLO within {JOB_WAIT:?} ({last})"
            );
            tokio::time::sleep(WAKE_NET).await;
        }
    }

    /// A deliberate negative observation: the claim is "this never happens", which no event can
    /// signal, so the budget is the observation window and the first occurrence fails immediately.
    async fn assert_never_appears(path: &Path, needle: &str, baseline: usize, budget: Duration) {
        let deadline = Instant::now() + budget;
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            assert_eq!(
                count_log(&text, needle),
                baseline,
                "{needle} appeared {budget:?} after it was supposed never to:\n{text}"
            );
            if Instant::now() >= deadline {
                return;
            }
            tokio::time::sleep(WAKE_NET).await;
        }
    }

    /// Case (a): launchd runs the supervisor, the supervisor runs the daemon, and a crash is answered
    /// by one restart that leaves the keeper and the session it owns untouched.
    #[ignore = "needs a real Darwin launchd GUI session; set FERRYX_REAL_LAUNCHD_SUPERVISOR=1"]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_launchd_restarts_the_daemon_and_keeps_the_keeper_session() {
        let ready = require_real_launchd();
        let mut lab = Lab::new("launchd-restart", &ready.program);
        let runtime = lab.fixture.runtime();
        let log_path = lab.fixture.restart_log();

        let keeper_job = lab.keeper_job(&ready.keeper);
        await_keeper(&runtime).await;
        lab.fixture.activate_keeper_ownership().await;
        let keeper = await_job_pid(&keeper_job, "the keeper job").await;
        let keeper_comm = await_comm(keeper, KEEPER_BINARY_NAME).await;
        assert!(
            keeper_comm.as_deref().is_some_and(|comm| comm.contains(KEEPER_BINARY_NAME)),
            "the keeper job's pid must be the keeper binary, got {keeper_comm:?}"
        );
        lab.run.record("keeper", keeper);

        let supervisor_job = lab.supervisor_job();
        let supervisor = await_job_pid(&supervisor_job, "the supervisor job").await;
        let supervisor_comm = await_comm(supervisor, "ferryx").await;
        assert!(
            supervisor_comm.as_deref().is_some_and(|comm| comm.contains("ferryx")),
            "the supervisor job's pid must be this build's CLI, got {supervisor_comm:?}"
        );
        assert_eq!(
            ps_field(supervisor, "ppid").as_deref(),
            Some("1"),
            "launchd must be the supervisor's parent, or the job is not what is running it"
        );
        let supervisor_ppid = ps_field(supervisor, "ppid")
            .and_then(|ppid| ppid.parse::<u32>().ok())
            .expect("the supervisor's parent is readable");
        let print = supervisor_job.print().expect("the supervisor job prints");
        assert_eq!(
            supervisor_chain_from_print(&print),
            Some(vec![SUPERVISOR_ARG.to_string()]),
            "launchd must report the supervisor chain, not a bare daemon:\n{print}"
        );
        assert_eq!(
            launchctl_print_program(&print).as_deref(),
            Some(lab.program.to_string_lossy().as_ref()),
            "the program launchd registered must be the one this run bootstrapped:\n{print}"
        );
        assert!(
            print.contains("state = running"),
            "the job must be running while the daemon serves, which is the state the gate's evidence \
             must show:\n{print}"
        );
        // Kept for the evidence: once the daemon is stopped deliberately the job exits, and a print
        // taken then would say `not running` about the same job.
        let running_print = print.clone();
        lab.run.record("supervisor", supervisor);

        // The gate first: a refused supervisor still serves one daemon, but it exits 0 and leaves
        // that daemon reparented to launchd (`event=refusal code=no_keeper_ownership
        // decision=serve_once`, then `event=stop`). Parentage is only evidence of supervision once
        // the log says supervision was enabled, so asserting it earlier reports a closed gate as
        // if the spawn chain had detached the daemon.
        let gate = await_log(&log_path, GATE_ON).await;
        assert_log(&gate, GATE_ON, "the gate opens for the live keeper job");

        let daemon = await_daemon(&runtime, None).await;
        lab.run.record("daemon", daemon);
        assert_log(
            &gate,
            &format!("event=start pid={daemon}"),
            "the daemon serving this runtime must be the one the supervisor recorded starting, so \
             this is the supervised pid and not a leftover from an earlier job",
        );
        assert_eq!(
            ps_field(daemon, "ppid"),
            Some(supervisor.to_string()),
            "the daemon must be the supervisor's child, which is what makes this supervision and not \
             two independent jobs"
        );

        // A keeper-owned PTY, which is the state a restart must not destroy.
        let mut client = KeeperClient::connect(&runtime).expect("handshake with the keeper job");
        let spawn = client
            .spawn(
                "launchd-session",
                &["/bin/sh".to_string()],
                &[("PS1".to_string(), "$ ".to_string())],
                None,
                80,
                24,
                1,
            )
            .expect("the keeper job spawns a real PTY");
        lab.run.record("keeper-pty-child", spawn.pid);
        drop(client);

        lab.run.terminate(daemon).expect("crash the supervised daemon");
        let restarted_text = await_log(&log_path, "decision=restart").await;
        assert_log(
            &restarted_text,
            &format!("event=exit pid={daemon}"),
            "the crash is recorded against the pid that died",
        );
        let after = await_daemon(&runtime, Some(daemon)).await;
        lab.run.record("daemon", after);
        assert_ne!(after, daemon, "the restart must be a new daemon process");
        assert_eq!(
            count_log(&restarted_text, RESTART_LINE),
            1,
            "one crash is one restart: {restarted_text}"
        );
        assert_eq!(
            keeper_job.pid(),
            Some(keeper),
            "the keeper is not the process under supervision and must survive the crash"
        );
        assert!(
            process_is_alive(spawn.pid),
            "the keeper's PTY child must survive the daemon crash"
        );
        let mut after_restart = KeeperClient::connect(&runtime).expect("the keeper still answers");
        let owned = after_restart.ownership().expect("post-restart discovery");
        assert!(
            owned
                .iter()
                .any(|entry| entry.session_id == "launchd-session" && entry.pid == spawn.pid),
            "the session must still be owned by the same keeper child after the restart: {owned:?}"
        );
        after_restart
            .kill("launchd-session", spawn.epoch)
            .expect("the keeper kills the child it owns");

        // A deliberate stop still ends supervision, and launchd leaves a clean exit alone.
        shutdown_daemon(&runtime, after)
            .await
            .expect("stop the restarted daemon deliberately");
        let stopped = await_log(&log_path, "reason=intentional:clean_exit").await;
        let restarts_before_stop = count_log(&restarted_text, RESTART_LINE);
        let restarts_after_stop = count_log(&stopped, RESTART_LINE);
        assert_eq!(
            restarts_after_stop, restarts_before_stop,
            "the deliberate stop must not add a restart to the one the crash caused: {stopped}"
        );
        await_condition("the supervisor job exits after the deliberate stop", JOB_WAIT, || {
            supervisor_job.pid().is_none()
        })
        .await;

        // This is the only case that observes both halves the gate counts, so it is where the
        // Phase 2 evidence comes from. The evidence is written last, from what was measured.
        if let Some(dir) = evidence_dir() {
            let path = lab.write_chain_evidence(
                &dir,
                &ChainObservation {
                    label: supervisor_job.label.clone(),
                    plist: supervisor_job.plist.clone(),
                    print: running_print,
                    supervisor_ppid,
                    kill_signal: "SIGKILL",
                    restarts_after_abnormal: restarts_before_stop,
                    restarts_after_exit_zero: restarts_after_stop - restarts_before_stop,
                    daemon_before: daemon,
                    daemon_after: after,
                },
            );
            eprintln!("wrote the Phase 2 launchd chain evidence to {}", path.display());
        }
    }

    /// Case (b): with no keeper job there is nothing to supervise, so the runtime gets exactly one
    /// unsupervised daemon, and launchd's `KeepAlive { SuccessfulExit = false }` must not relaunch the
    /// job after it exits 0.
    #[ignore = "needs a real Darwin launchd GUI session; set FERRYX_REAL_LAUNCHD_SUPERVISOR=1"]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_launchd_without_a_keeper_serves_once_and_is_never_relaunched() {
        let ready = require_real_launchd();
        let mut lab = Lab::new("launchd-no-keeper", &ready.program);
        let runtime = lab.fixture.runtime();
        let log_path = lab.fixture.restart_log();
        assert!(
            !lab.fixture.keeper_socket().exists(),
            "this case must start without any keeper"
        );

        let supervisor_job = lab.supervisor_job();
        let text = await_log(&log_path, "decision=serve_once").await;
        assert_log(
            &text,
            "event=refusal code=no_live_keeper decision=serve_once",
            "the refusal is a typed gate decision",
        );
        assert_log(
            &text,
            GATE_OFF,
            "the gate must be recorded as closed",
        );
        assert_log(
            &text,
            "no live PTY keeper",
            "the banner must say why restart is off",
        );

        let daemon = await_daemon(&runtime, None).await;
        lab.run.record("daemon", daemon);
        let started = await_log(&log_path, "supervised=off").await;
        assert_log(
            &started,
            &format!("event=start pid={daemon} supervised=off decision=serve_once"),
            "the single allowed start must be recorded against the pid that serves this runtime",
        );
        assert_eq!(
            count_log(&started, "event=start"),
            1,
            "a refused gate may start exactly one daemon: {started}"
        );

        // launchd leaves a clean exit alone: the job is registered but not running.
        await_condition("the supervisor job exits 0", JOB_WAIT, || {
            supervisor_job.pid().is_none()
        })
        .await;
        let print = supervisor_job
            .print()
            .expect("a registered job reports its state");
        assert_eq!(
            launchctl_job_pid(&print),
            None,
            "no process may be left running under the supervisor label:\n{print}"
        );
        assert_eq!(
            supervisor_chain_from_print(&print),
            Some(vec![SUPERVISOR_ARG.to_string()]),
            "the idle job must still hold the supervisor chain, which is what launchd would relaunch:\n{print}"
        );

        // Nothing relaunches: no second gate, no second start, and the same daemon throughout.
        let text = lab.log_text();
        assert_eq!(
            count_log(&text, GATE_LINE),
            1,
            "the gate must not be decided again, which is what a relaunch would do: {text}"
        );
        assert_eq!(
            count_log(&text, RESTART_LINE),
            0,
            "an unsupervised daemon is never restarted: {text}"
        );
        assert_never_appears(&log_path, "event=start", 1, NEGATIVE_WINDOW).await;
        assert_eq!(
            control_pid(&runtime).await,
            Ok(Some(daemon)),
            "the one daemon must still be the one serving this runtime"
        );
        assert!(
            process_is_alive(daemon),
            "the daemon must survive the supervisor's clean exit, as it did when the job ran \
             --daemon directly"
        );
        shutdown_daemon(&runtime, daemon)
            .await
            .expect("stop the one daemon this case started");
    }

    /// Case (c): launchd owns the supervisor. A second supervisor - launchd's own re-bootstrap attempt,
    /// then a hand-started one - must not become a second daemon owner.
    #[ignore = "needs a real Darwin launchd GUI session; set FERRYX_REAL_LAUNCHD_SUPERVISOR=1"]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_launchd_owns_supervision_and_refuses_a_second_supervisor() {
        let ready = require_real_launchd();
        let mut lab = Lab::new("launchd-owner", &ready.program);
        let runtime = lab.fixture.runtime();
        let log_path = lab.fixture.restart_log();

        let keeper_job = lab.keeper_job(&ready.keeper);
        await_keeper(&runtime).await;
        lab.fixture.activate_keeper_ownership().await;
        let supervisor_job = lab.supervisor_job();
        let supervisor = await_job_pid(&supervisor_job, "the supervisor job").await;
        lab.run.record("supervisor", supervisor);
        let daemon = await_daemon(&runtime, None).await;
        lab.run.record("daemon", daemon);
        await_log(&log_path, GATE_ON).await;

        // launchd itself refuses to register the same label twice, so a job cannot become two jobs.
        let duplicate = launchctl(&[
            "bootstrap",
            &domain(),
            &supervisor_job.plist.to_string_lossy(),
        ]);
        assert!(
            duplicate.is_err(),
            "launchd must refuse a second registration of {}, got {duplicate:?}",
            supervisor_job.label
        );
        assert_eq!(
            supervisor_job.pid(),
            Some(supervisor),
            "the refusal must not have disturbed the running job"
        );

        // A supervisor started outside launchd must lose the runtime's claim instead of racing it.
        let second = lab.spawn_supervisor_process("second-supervisor", "logs/second-supervisor.err");
        assert_ne!(second, supervisor);
        let status = lab
            .run
            .await_exit(second, "the hand-started second supervisor")
            .await;
        assert!(
            status.success(),
            "a lost claim is a clean exit, got {status:?}\n{}",
            lab.diagnostics()
        );
        let text = lab.log_text();
        assert_log(
            &text,
            "event=refusal code=supervisor_already_running decision=stop",
            "the second supervisor's refusal must be on record",
        );
        assert_log(
            &text,
            "reason=already_supervised",
            "the stop reason names the cause",
        );
        assert_eq!(
            count_log(&text, GATE_LINE),
            1,
            "the second supervisor decided no gate, because it supervised nothing: {text}"
        );
        assert_eq!(
            count_log(&text, "event=start"),
            1,
            "a second supervisor must never start a daemon of its own: {text}"
        );
        assert_eq!(
            count_log(&text, BANNER_LINE),
            0,
            "a refused claim stays silent in the UI: the first supervisor is serving this runtime"
        );
        assert_eq!(
            control_pid(&runtime).await,
            Ok(Some(daemon)),
            "the supervised daemon must be untouched"
        );
        assert_eq!(
            supervisor_job.pid(),
            Some(supervisor),
            "the job launchd owns must still be the supervisor"
        );
        assert!(
            runtime.join(SUPERVISOR_LOCK_NAME).exists(),
            "the claim is a file in the runtime, which is what the second supervisor found"
        );

        // And the surviving supervisor still supervises: one crash, one restart.
        lab.run.terminate(daemon).expect("crash the supervised daemon");
        let restarted_text = await_log(&log_path, "decision=restart").await;
        assert_eq!(
            count_log(&restarted_text, RESTART_LINE),
            1,
            "the surviving supervisor keeps its budget: {restarted_text}"
        );
        let after = await_daemon(&runtime, Some(daemon)).await;
        lab.run.record("daemon", after);
        assert_eq!(
            ps_field(after, "ppid"),
            Some(supervisor.to_string()),
            "the restart is still the supervisor's child"
        );
        assert!(
            keeper_job.pid().is_some(),
            "the keeper job must still be running"
        );
        shutdown_daemon(&runtime, after)
            .await
            .expect("stop the restarted daemon deliberately");
    }

    /// Case (d): a handover under launchd. The predecessor retires as a handover - from the durable
    /// decision record the commit writes, not from its exit status - the supervisor exits 0, launchd
    /// does not relaunch it, and the successor plus the keeper keep serving.
    #[ignore = "needs a real Darwin launchd GUI session; set FERRYX_REAL_LAUNCHD_SUPERVISOR=1"]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_launchd_records_a_handover_and_does_not_restart() {
        let ready = require_real_launchd();
        let mut lab = Lab::new("launchd-handover", &ready.program);
        let runtime = lab.fixture.runtime();
        let log_path = lab.fixture.restart_log();

        let keeper_job = lab.keeper_job(&ready.keeper);
        await_keeper(&runtime).await;
        lab.fixture.activate_keeper_ownership().await;
        let supervisor_job = lab.supervisor_job();
        let supervisor = await_job_pid(&supervisor_job, "the supervisor job").await;
        lab.run.record("supervisor", supervisor);
        let predecessor = await_daemon(&runtime, None).await;
        lab.run.record("daemon", predecessor);
        await_log(&log_path, GATE_ON).await;

        let mut client = DataClient::connect(&lab.fixture.daemon_socket(), predecessor)
            .await
            .expect("data-protocol handshake with the supervised daemon");
        let legacy_socket = match client.request(&DaemonRequest::PrepareHandover).await {
            Ok(DaemonResponse::PrepareHandoverOk {
                legacy_socket_path,
                active_sessions,
            }) => {
                assert!(
                    active_sessions.is_empty(),
                    "this case exercises a zero-session handover: {active_sessions:?}"
                );
                PathBuf::from(legacy_socket_path)
            }
            other => panic!("PrepareHandover failed: {other:?}"),
        };
        assert!(
            legacy_socket.starts_with(&runtime),
            "the legacy socket must live in this case's runtime: {}",
            legacy_socket.display()
        );
        drop(client);

        // The supervisor only ever starts a plain `--daemon`, so the successor is started here, with
        // this fixture's environment, over the same real handover path.
        let stderr = std::fs::File::create(lab.fixture.path("logs/successor.err"))
            .expect("capture the successor's stderr");
        let mut command = lab.fixture.successor_command(&legacy_socket);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        let successor = lab
            .run
            .spawn("successor", command.spawn().expect("spawn the handover successor"));
        assert_ne!(successor, predecessor);

        let adopted = await_daemon(&runtime, Some(predecessor)).await;
        assert_eq!(
            adopted, successor,
            "the daemon serving the endpoint after commit must be the successor"
        );
        lab.run.forget(predecessor);

        let text = await_log(&log_path, "reason=intentional:handover").await;
        assert_log(
            &text,
            &format!("event=exit pid={predecessor}"),
            "the predecessor's exit is recorded against its own pid",
        );
        assert_log(
            &text,
            "kind=handover",
            "a committed handover is classified from its decision record",
        );
        assert_no_log(
            &text,
            RESTART_LINE,
            "a handover must never be answered by a restart",
        );

        let records: Vec<serde_json::Value> = std::fs::read_dir(&runtime)
            .expect("the runtime directory")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(&format!("legacy-{predecessor}-"))
                            && name.ends_with(".transaction")
                    })
            })
            .filter_map(|path| serde_json::from_slice(&std::fs::read(path).ok()?).ok())
            .collect();
        assert_eq!(
            records.len(),
            1,
            "the commit writes exactly one decision record for the retiring predecessor"
        );
        assert_eq!(
            records[0].get("state").and_then(|state| state.as_str()),
            Some("retired"),
            "the record must carry the committed state: {records:?}"
        );

        await_condition("the supervisor job exits 0 after the handover", JOB_WAIT, || {
            supervisor_job.pid().is_none()
        })
        .await;
        assert_never_appears(&log_path, RESTART_LINE, 0, NEGATIVE_WINDOW).await;
        assert!(
            process_is_alive(successor),
            "the successor must still serve this runtime after the supervisor exited"
        );
        assert!(
            keeper_job.pid().is_some(),
            "the keeper job is not the process under supervision and must keep running"
        );
        let mut keeper = KeeperClient::connect(&runtime).expect("the keeper still answers");
        let owned = keeper.ownership().expect("keeper discovery after the handover");
        assert!(
            owned.is_empty(),
            "a zero-session handover leaves the keeper owning nothing: {owned:?}"
        );
        shutdown_daemon(&runtime, successor)
            .await
            .expect("stop the successor this case started");
    }
}

/// Always runs, asked or not: it states which way the real-launchd cases will go, so a green run of
/// this file never reads as "the acceptance passed" when the cases were ignored. It also pins that
/// the combined-gate owner is pointed at a real command file - the path, not its prose, because prose
/// is not machine-consumed.
#[test]
fn the_real_launchd_cases_state_their_gate_and_the_command_file_exists() {
    let asked = real_launchd_asked();
    eprintln!(
        "real-launchd acceptance: {} ({REAL_LAUNCHD_ASK_ENV}={})",
        if asked {
            "asked for; prerequisites are enforced"
        } else {
            "not asked for; the cases stay ignored"
        },
        std::env::var(REAL_LAUNCHD_ASK_ENV).unwrap_or_else(|_| "unset".to_string())
    );
    let command_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|root| root.join(".omo/evidence/supervisor/GATE-COMMAND.md"));
    let path = command_file.expect("the repository root above src-tauri");
    // Asked means somebody intends to produce the gate's evidence from this tree, and the command
    // they must run is this file. Checked on every platform, not only on Darwin: off Darwin the
    // operand of `--ignored` is the guard below, so the command still has to be found.
    if asked || cfg!(target_os = "macos") {
        assert!(
            path.is_file(),
            "the Darwin gate command must exist at {} so the gate owner can run this acceptance",
            path.display()
        );
    }
}

/// The `--ignored` operand off Darwin, and the reason this file cannot pass by omission.
///
/// `mod real_launchd` is `#[cfg(target_os = "macos")]`, so on any other platform `--ignored` matches
/// **no test at all** and `cargo test` exits 0 having run nothing. A gate run would then read as a
/// pass that proved nothing about launchd - exactly the overclaim the plan forbids.
///
/// So this case takes the place of the four that do not exist, and behaves the way they do: it is
/// `#[ignore]`d, so a plain `cargo test` reports it `ignored` like its macOS siblings and runs
/// nothing; and when `--ignored` selects it - which is what the gate command does - it fails,
/// naming the platform. That is the same shape as `require_real_launchd`, which asserts the ask
/// variable before anything else, so `--ignored` without `FERRYX_REAL_LAUNCHD_SUPERVISOR=1` fails on
/// macOS too. There is no platform and no invocation in which the launchd cases and this gate agree
/// to call silence a pass.
#[cfg(not(target_os = "macos"))]
#[ignore = "launchd exists only on Darwin; this case fails if it is ever selected"]
#[test]
fn real_launchd_cases_fail_rather_than_vanish_off_darwin() {
    panic!(
        "the four launchd cases are compiled only for macOS, so `--ignored` selected none of them \
         and this {} run would have reported success having tested nothing. Run the Phase 2 launchd \
         gate on a Darwin host - see .omo/evidence/supervisor/GATE-COMMAND.md",
        std::env::consts::OS
    );
}
