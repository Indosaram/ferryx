use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{ErrorKind, Read},
    net::{TcpListener, TcpStream},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use super::Output;

pub const MAX_REPORT_BYTES: usize = 16 * 1024;
pub const TOTAL_DEADLINE: Duration = Duration::from_secs(2);
pub const MAX_CONCURRENT_HANDLERS: usize = 32;

/// Safety-net park interval for the accept loop while the listener has nothing to accept.
/// A stop request wakes the loop immediately through the condvar; this interval only
/// bounds a hypothetical missed notification, it is not the shutdown latency.
const ACCEPT_PARK_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStateSnapshot {
    pub revision: u64,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AgentStateSnapshot {
    pub fn to_response_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("revision".into(), Value::String(self.revision.to_string()));
        map.insert("state".into(), Value::String(self.state.clone()));
        if let Some(agent) = &self.agent {
            map.insert("agent".into(), Value::String(agent.clone()));
        }
        if let Some(provider_session) = &self.provider_session {
            map.insert("providerSession".into(), provider_session.clone());
        }
        if let Some(detail) = &self.detail {
            map.insert("detail".into(), Value::String(detail.clone()));
        }
        Value::Object(map)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentStateWireReport {
    #[serde(rename = "type")]
    msg_type: String,
    session_id: String,
    state: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    provider_session: Option<Value>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    token: Option<String>,
}

pub type AgentReportCallback =
    Arc<dyn Fn(&str, Option<&str>, Option<&Value>) -> Result<(), String> + Send + Sync>;

struct SessionRegistration {
    token: String,
    output: Arc<(Mutex<Output>, Condvar)>,
    on_report: Option<AgentReportCallback>,
}

/// Interruptible shutdown signal for the accept loop, shaped after the watchdog's
/// `Mutex<bool> + Condvar` handle (`src/watchdog/mod.rs`).
///
/// The accept loop parks on this condvar instead of blocking inside `accept()`, so
/// shutdown never allocates a wake socket: a process that is out of descriptors must
/// still be able to stop, and `Drop` can no longer depend on a `TcpStream::connect`
/// succeeding.
struct AgentStateShutdown {
    state: Mutex<AgentStateShutdownState>,
    condvar: Condvar,
}

#[derive(Default)]
struct AgentStateShutdownState {
    stop_requested: bool,
    accept_exited: bool,
}

impl AgentStateShutdown {
    fn new() -> Self {
        Self {
            state: Mutex::new(AgentStateShutdownState::default()),
            condvar: Condvar::new(),
        }
    }

    /// True once shutdown was requested. A poisoned lock reports stopped, so callers
    /// fail safe rather than ignoring a shutdown.
    fn is_stopped(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.stop_requested)
            .unwrap_or(true)
    }

    /// Requests shutdown and wakes every parked waiter. No socket, no file descriptor.
    fn request_stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stop_requested = true;
        }
        self.condvar.notify_all();
    }

    /// Parks until shutdown is requested or the safety-net timeout elapses.
    fn park_until_stop(&self, timeout: Duration) {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return,
        };
        let _ = self
            .condvar
            .wait_timeout_while(state, timeout, |state| !state.stop_requested);
    }

    fn mark_accept_exited(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.accept_exited = true;
        }
        self.condvar.notify_all();
    }

    /// Bounded wait for the accept loop to leave its park/accept iteration.
    fn wait_for_accept_exit(&self, timeout: Duration) -> bool {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return true,
        };
        match self
            .condvar
            .wait_timeout_while(state, timeout, |state| !state.accept_exited)
        {
            Ok((state, _)) => state.accept_exited,
            Err(_) => true,
        }
    }
}

pub struct AgentStateServer {
    port: u16,
    shutdown: Arc<AgentStateShutdown>,
    handler_count: Arc<(Mutex<usize>, Condvar)>,
    registry: Arc<Mutex<HashMap<String, SessionRegistration>>>,
    accept_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl AgentStateServer {
    pub fn bind() -> Result<Self, String> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to bind TCP listener: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Failed to read local port: {e}"))?
            .port();

        // The accept loop parks on a condvar instead of blocking inside `accept()`, so
        // shutdown stays interruptible without allocating a wake socket. Accepted streams
        // are switched back to blocking below: BSD/macOS inherits O_NONBLOCK through accept().
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to configure TCP listener: {e}"))?;

        let shutdown = Arc::new(AgentStateShutdown::new());
        let handler_count = Arc::new((Mutex::new(0), Condvar::new()));
        let registry = Arc::new(Mutex::new(HashMap::new()));

        let shutdown_for_thread = Arc::clone(&shutdown);
        let handlers_for_thread = Arc::clone(&handler_count);
        let registry_for_thread = Arc::clone(&registry);

        let accept_thread = std::thread::Builder::new()
            .name("ferryx-ssh-agent-state-accept".into())
            .spawn(move || {
                while !shutdown_for_thread.is_stopped() {
                    let stream = match listener.accept() {
                        Ok((s, _)) => s,
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            // Idle listener: park interruptibly instead of busy-looping.
                            shutdown_for_thread.park_until_stop(ACCEPT_PARK_TIMEOUT);
                            continue;
                        }
                        Err(_) => {
                            if shutdown_for_thread.is_stopped() {
                                break;
                            }
                            // A transient accept failure (aborted connection, exhausted
                            // descriptors) must not spin this loop hot.
                            shutdown_for_thread.park_until_stop(ACCEPT_PARK_TIMEOUT);
                            continue;
                        }
                    };

                    if shutdown_for_thread.is_stopped() {
                        break;
                    }

                    if stream.set_nonblocking(false).is_err() {
                        drop(stream);
                        continue;
                    }

                    {
                        let (lock, _) = &*handlers_for_thread;
                        let mut count = match lock.lock() {
                            Ok(c) => c,
                            Err(_) => break,
                        };
                        if *count >= MAX_CONCURRENT_HANDLERS {
                            drop(stream);
                            continue;
                        }
                        *count += 1;
                    }

                    let h_stop = Arc::clone(&shutdown_for_thread);
                    let h_handlers = Arc::clone(&handlers_for_thread);
                    let h_reg = Arc::clone(&registry_for_thread);

                    let spawned = std::thread::Builder::new()
                        .name("ferryx-ssh-agent-state-worker".into())
                        .spawn(move || {
                            let _guard = HandlerGuard(&h_handlers);
                            handle_connection(stream, &h_reg, &h_stop);
                        });

                    if spawned.is_err() {
                        let (lock, cvar) = &*handlers_for_thread;
                        if let Ok(mut count) = lock.lock() {
                            if *count > 0 {
                                *count -= 1;
                                cvar.notify_all();
                            }
                        }
                    }
                }
                // Tell the shutdown path the loop is done, so `Drop` never joins a thread
                // that is still parked.
                shutdown_for_thread.mark_accept_exited();
            })
            .map_err(|e| format!("Failed to spawn accept thread: {e}"))?;

        Ok(Self {
            port,
            shutdown,
            handler_count,
            registry,
            accept_thread: Mutex::new(Some(accept_thread)),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Registers the report endpoint for one PTY session.
    ///
    /// `pub(super)` on purpose: this is helper-internal plumbing, and a wider item
    /// visibility would expose the helper-owned `Output` guard type in a crate-visible
    /// signature (private-interface lint). The helper module and its test children are
    /// the only callers.
    #[cfg(test)]
    pub(super) fn register(
        &self,
        session_id: &str,
        token: &str,
        output: Arc<(Mutex<Output>, Condvar)>,
    ) {
        self.register_with_callback(session_id, token, output, None);
    }

    pub(super) fn register_with_callback(
        &self,
        session_id: &str,
        token: &str,
        output: Arc<(Mutex<Output>, Condvar)>,
        on_report: Option<AgentReportCallback>,
    ) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.insert(
                session_id.to_string(),
                SessionRegistration {
                    token: token.to_string(),
                    output,
                    on_report,
                },
            );
        }
    }

    pub fn rollback(&self, session_id: &str) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove(session_id);
        }
    }

    pub fn revoke(&self, session_id: &str) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove(session_id);
        }
    }

    #[cfg(test)]
    pub fn is_registered(&self, session_id: &str) -> bool {
        self.registry
            .lock()
            .map(|reg| reg.contains_key(session_id))
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub fn get_token(&self, session_id: &str) -> Option<String> {
        self.registry
            .lock()
            .ok()
            .and_then(|reg| reg.get(session_id).map(|r| r.token.clone()))
    }

    /// Test seam: request the same allocation-free stop signal `Drop` uses, without
    /// tearing the server down yet.
    #[cfg(test)]
    pub(super) fn request_stop_for_test(&self) {
        self.shutdown.request_stop();
    }

    /// Test seam: bounded wait until the accept loop observed the stop request.
    #[cfg(test)]
    pub(super) fn wait_for_accept_exit_for_test(&self, timeout: Duration) -> bool {
        self.shutdown.wait_for_accept_exit(timeout)
    }

    /// Test seam: block until the report ingested for `session_id` reached at least
    /// `revision`. This waits on the session's own report condvar — the same signal the
    /// ingest path notifies — so a test awaits the exact report instead of racing later
    /// `pty.read` calls against accept timing.
    #[cfg(test)]
    pub(super) fn wait_for_agent_revision_for_test(
        &self,
        session_id: &str,
        revision: u64,
        timeout: Duration,
    ) -> bool {
        let output = match self.registry.lock() {
            Ok(registry) => match registry.get(session_id) {
                Some(registration) => registration.output.clone(),
                None => return false,
            },
            Err(_) => return false,
        };
        let (lock, signal) = &*output;
        let state = match lock.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };
        // Bind the wait result to a local: the guard it returns borrows `output`, and as a
        // tail-expression temporary its drop scope would outlive the `Arc` local that owns
        // the mutex and condvar (E0597).
        let (state, _) = match signal
            .wait_timeout_while(state, timeout, |state| state.agent_revision < revision)
        {
            Ok(result) => result,
            Err(_) => return false,
        };
        state.agent_revision >= revision
    }
}

impl Drop for AgentStateServer {
    fn drop(&mut self) {
        // Allocation-independent wake: signal plus condvar notification. Nothing is
        // socketed here, so an fd-exhausted process can still tear the server down and no
        // failure mode can leave `accept()` blocked forever.
        self.shutdown.request_stop();

        if let Ok(mut handle_opt) = self.accept_thread.lock() {
            if let Some(handle) = handle_opt.take() {
                // Bounded wait for the parked accept loop to observe the stop request; if it
                // somehow does not exit, detach instead of blocking this thread forever.
                if self.shutdown.wait_for_accept_exit(TOTAL_DEADLINE) {
                    let _ = handle.join();
                }
            }
        }

        let (lock, cvar) = &*self.handler_count;
        if let Ok(guard) = lock.lock() {
            let _ = cvar.wait_timeout_while(guard, TOTAL_DEADLINE, |count| *count > 0);
        }
    }
}

struct HandlerGuard<'a>(&'a (Mutex<usize>, Condvar));

impl<'a> Drop for HandlerGuard<'a> {
    fn drop(&mut self) {
        let (lock, cvar) = self.0;
        if let Ok(mut count) = lock.lock() {
            if *count > 0 {
                *count -= 1;
                cvar.notify_all();
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    registry: &Mutex<HashMap<String, SessionRegistration>>,
    stop: &AgentStateShutdown,
) {
    if stop.is_stopped() {
        return;
    }

    let deadline = Instant::now() + TOTAL_DEADLINE;
    let _ = stream.set_write_timeout(Some(TOTAL_DEADLINE));

    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];

    loop {
        if stop.is_stopped() {
            return;
        }

        let now = Instant::now();
        let remaining = match deadline.checked_duration_since(now) {
            Some(d) if !d.is_zero() => d,
            _ => return,
        };
        if stream.set_read_timeout(Some(remaining)).is_err() {
            return;
        }

        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > MAX_REPORT_BYTES {
                    return;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') {
                    break;
                }
            }
            Err(_) => return,
        }
    }

    if !buf.contains(&b'\n') {
        return;
    }

    let line = match buf.split(|&b| b == b'\n').next() {
        Some(l) if !l.is_empty() => l,
        _ => return,
    };

    let report: AgentStateWireReport = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(_) => return,
    };

    if report.msg_type != "agentState" {
        return;
    }

    if !matches!(report.state.as_str(), "idle" | "working" | "blocked") {
        return;
    }

    let token = match &report.token {
        Some(t) if !t.is_empty() => t,
        _ => return,
    };

    let (output, on_report) = {
        let guard = match registry.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match guard.get(&report.session_id) {
            Some(reg) if reg.token == *token => (reg.output.clone(), reg.on_report.clone()),
            _ => return,
        }
    };

    if let Some(cb) = on_report {
        if let Err(e) = cb(&report.session_id, report.agent.as_deref(), report.provider_session.as_ref()) {
            eprintln!("Ferryx agent report persistence failed: {e}");
            return;
        }
    }

    let (lock, signal) = &*output;
    let mut out = match lock.lock() {
        Ok(o) => o,
        Err(_) => return,
    };

    if out.exited {
        return;
    }

    let next_revision = match out.agent_revision.checked_add(1) {
        Some(rev) => rev.max(1),
        None => return,
    };

    out.agent_revision = next_revision;
    out.agent_state = Some(AgentStateSnapshot {
        revision: next_revision,
        state: report.state,
        agent: report.agent,
        provider_session: report.provider_session,
        detail: report.detail,
    });
    signal.notify_all();
}

pub struct AgentRegistrationGuard {
    server: Arc<AgentStateServer>,
    session_id: String,
    defused: bool,
}

impl AgentRegistrationGuard {
    pub fn new(server: Arc<AgentStateServer>, session_id: String) -> Self {
        Self {
            server,
            session_id,
            defused: false,
        }
    }

    pub fn defuse(&mut self) {
        self.defused = true;
    }
}

impl Drop for AgentRegistrationGuard {
    fn drop(&mut self) {
        if !self.defused {
            self.server.rollback(&self.session_id);
        }
    }
}
