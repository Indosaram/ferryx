//! Desktop-hosted agent-state ingress and ssh remote forwarding for helper sessions.
//!
//! Helper-spawned remote PTYs receive `FERRYX_SESSION_ID` only, so the installed omo
//! extension has no endpoint to report state to and remote agent state is lost on every
//! platform. This module closes that gap without touching the bridge protocol:
//!
//! 1. one loopback TCP ingress per desktop process (`AgentStateIngress`), each accepted
//!    report authenticated by a per-session random token;
//! 2. one `ssh -N -R 127.0.0.1:0:127.0.0.1:<ingress port>` child per remote session, so the
//!    extension can reach that ingress from the remote loopback interface;
//! 3. env delivery through the existing spawn params, so the PTY inherits the endpoint.
//!
//! Reports are validated, mapped to the daemon session id, and published into the same
//! [`crate::daemon::agent_state::AgentStateHub`] the local unix-socket path feeds. Failure to
//! establish the channel degrades to "no agent state" and never blocks the terminal.
use super::{password, SshAuthMethod, SshHost};
use crate::daemon::agent_state::{AgentState, AgentStateHub};
use crate::daemon::protocol::{AgentProviderSession, AgentStateOrigin};
use crate::ipc::{IpcError, IpcErrorCode};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Ceiling for one extension report line; anything larger is dropped unread.
pub(crate) const MAX_REPORT_BYTES: usize = 16 * 1024;
/// Bound on how long one accepted connection may produce its single line.
const REPORT_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Bound on waiting for OpenSSH to allocate the remote forward port.
const FORWARD_ALLOCATION_TIMEOUT: Duration = Duration::from_secs(10);

/// One extension report. The wire struct is deliberately separate from
/// [`crate::daemon::protocol::AgentStateReport`] because that type has no token field: serde
/// would drop the token and authentication would be impossible.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct IngressReport {
    session_id: String,
    state: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    provider_session: Option<AgentProviderSession>,
}

#[derive(Clone)]
struct Route {
    hub: Arc<AgentStateHub>,
    remote_session_id: String,
    local_session_id: Option<String>,
}

/// Loopback ingress shared by every remote session of this desktop process.
pub(crate) struct AgentStateIngress {
    port: u16,
    routes: Arc<Mutex<HashMap<String, Route>>>,
}

static INGRESS: OnceLock<Option<AgentStateIngress>> = OnceLock::new();

impl AgentStateIngress {
    /// Returns the process ingress, binding it on first use. `None` means the ingress could
    /// not bind; callers then run the session without agent state.
    pub(crate) fn global() -> Option<&'static AgentStateIngress> {
        INGRESS
            .get_or_init(|| match Self::bind() {
                Ok(ingress) => Some(ingress),
                Err(error) => {
                    tracing::warn!(
                        stage = "agent_state_ingress",
                        %error,
                        "Agent state ingress unavailable; remote sessions run without agent state"
                    );
                    None
                }
            })
            .as_ref()
    }

    fn bind() -> Result<Self, IpcError> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?
            .port();
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::new(Mutex::new(HashMap::new()));
        let accept_routes = Arc::clone(&routes);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let routes = Arc::clone(&accept_routes);
                tokio::spawn(async move {
                    if let Err(error) = serve_report(stream, routes).await {
                        tracing::debug!(%error, "Agent state report rejected");
                    }
                });
            }
        });
        Ok(Self { port, routes })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Registers one pending route and returns its token plus the remote-side session id the
    /// extension will report (the PTY spawn overrides `FERRYX_SESSION_ID` with this value).
    pub(crate) async fn register(&self, hub: Arc<AgentStateHub>) -> (String, String) {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let remote_session_id = uuid::Uuid::new_v4().to_string();
        self.routes.lock().await.insert(
            token.clone(),
            Route {
                hub,
                remote_session_id: remote_session_id.clone(),
                local_session_id: None,
            },
        );
        (token, remote_session_id)
    }

    /// Binds the reported remote id to the daemon session id once the spawn returned.
    pub(crate) async fn bind_local(&self, token: &str, local_session_id: &str) {
        if let Some(route) = self.routes.lock().await.get_mut(token) {
            route.local_session_id = Some(local_session_id.to_string());
        }
    }

    pub(crate) async fn revoke(&self, token: &str) {
        self.routes.lock().await.remove(token);
    }
}

/// One accepted connection: exactly one bounded newline-delimited report, then close.
async fn serve_report(
    stream: tokio::net::TcpStream,
    routes: Arc<Mutex<HashMap<String, Route>>>,
) -> std::io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader);
    let mut line = String::new();
    let read = tokio::time::timeout(REPORT_READ_TIMEOUT, lines.read_line(&mut line))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "report timed out"))??;
    if read == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "empty report",
        ));
    }
    if line.len() > MAX_REPORT_BYTES {
        return Ok(());
    }
    match resolve_report(&line, &routes).await {
        Some((hub, state)) => hub.publish_canonical(state),
        None => tracing::debug!("Agent state report rejected: unknown token, unbound route, or invalid payload"),
    }
    let _ = writer.write_all(b"\n").await;
    Ok(())
}

/// Pure validation/mapping: returns the hub and the state to publish only for an
/// authenticated report whose route is bound to a daemon session.
pub(crate) async fn resolve_report(
    line: &str,
    routes: &Arc<Mutex<HashMap<String, Route>>>,
) -> Option<(Arc<AgentStateHub>, AgentState)> {
    let report: IngressReport = serde_json::from_str(line).ok()?;
    if !matches!(report.state.as_str(), "working" | "blocked" | "idle") {
        return None;
    }
    let token = report.token.as_deref().filter(|token| !token.is_empty())?;
    let (hub, local_session_id, remote_session_id) = {
        let guard = routes.lock().await;
        let route = guard.get(token)?;
        (
            Arc::clone(&route.hub),
            route.local_session_id.clone()?,
            route.remote_session_id.clone(),
        )
    };
    // The helper may report its own id when the injected FERRYX_SESSION_ID did not win, so a
    // report is accepted only when it names this route's remote id or its daemon session id.
    if report.session_id != remote_session_id && report.session_id != local_session_id {
        return None;
    }
    let provider_session = report.provider_session.filter(|provider| {
        report
            .agent
            .as_deref()
            .is_some_and(|agent| crate::terminal::shell::resolve_agent_resume_plan(agent, provider).is_ok())
    });
    Some((
        hub,
        AgentState {
            session_id: local_session_id,
            state: report.state,
            agent: report.agent,
            provider_session,
            origin: AgentStateOrigin::Agent,
        },
    ))
}

/// `ssh -N -R 127.0.0.1:0:127.0.0.1:<local port>` against one host. Kept separate from
/// [`super::direct::ssh_plan`] because that planner sets `ClearAllForwardings=yes`, which
/// silently disables remote forwarding.
pub(crate) fn forward_plan(
    host: &SshHost,
    local_port: u16,
) -> Result<crate::terminal::shell::ShellCommandPlan, IpcError> {
    super::direct::validate_host(host)?;
    password::require(host)?;
    let mut args = super::exec::interactive_argv(host);
    let program = args.remove(0);
    args.remove(0);
    args.insert(0, "-N".to_string());
    let mut options = vec![
        if host.auth_method == SshAuthMethod::Password {
            "BatchMode=no"
        } else {
            "BatchMode=yes"
        },
        "StrictHostKeyChecking=yes",
        "UpdateHostKeys=no",
        "ConnectTimeout=5",
        "ConnectionAttempts=1",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=2",
        "PermitLocalCommand=no",
        "RemoteCommand=none",
        "ControlMaster=no",
        "ControlPath=none",
        "ExitOnForwardFailure=yes",
    ];
    if host.auth_method == SshAuthMethod::Password {
        options.extend([
            "PreferredAuthentications=password",
            "PasswordAuthentication=yes",
            "PubkeyAuthentication=no",
            "KbdInteractiveAuthentication=no",
            "NumberOfPasswordPrompts=1",
        ]);
    }
    for option in options {
        args.splice(0..0, ["-o".to_string(), option.to_string()]);
    }
    let target = args
        .pop()
        .ok_or_else(|| IpcError::internal("Missing SSH target"))?;
    args.push("-R".to_string());
    args.push(format!("127.0.0.1:0:127.0.0.1:{local_port}"));
    args.push(target);
    Ok(crate::terminal::shell::ShellCommandPlan { program, args })
}

/// Reads the remote port OpenSSH allocated for `-R ...:0`.
pub(crate) fn parse_allocated_port(line: &str) -> Option<u16> {
    let rest = line.split("Allocated port ").nth(1)?;
    rest.split_whitespace().next()?.parse::<u16>().ok()
}

/// Live forwarding child owned by the session that created it.
pub(crate) struct ForwardGuard {
    child: tokio::process::Child,
    pub(crate) remote_port: u16,
}

impl ForwardGuard {
    pub(crate) async fn close(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

/// Starts the per-session forwarding child and waits for the allocated remote port.
pub(crate) async fn start_forward(
    host: &SshHost,
    local_port: u16,
) -> Result<ForwardGuard, IpcError> {
    let plan = forward_plan(host, local_port)?;
    let mut child = tokio::process::Command::new(&plan.program)
        .args(&plan.args)
        .envs(password::environment(&plan.args)?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to start SSH forward: {error}"),
            )
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| IpcError::internal("Missing SSH forward stderr"))?;
    let mut lines = BufReader::new(stderr).lines();
    let deadline = tokio::time::Instant::now() + FORWARD_ALLOCATION_TIMEOUT;
    let mut seen = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            let _ = child.start_kill();
            return Err(super::runtime::error(
                IpcErrorCode::IoError,
                "agent_state_forward",
                "SSH forward did not allocate a remote port in time",
            ));
        }
        match tokio::time::timeout(remaining, lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if let Some(port) = parse_allocated_port(&line) {
                    return Ok(ForwardGuard {
                        child,
                        remote_port: port,
                    });
                }
                if seen.len() < 4096 {
                    seen.push_str(&line);
                    seen.push('\n');
                }
            }
            Ok(Ok(None)) | Ok(Err(_)) => {
                let status = child.wait().await.ok();
                let _ = child.start_kill();
                return Err(super::runtime::error(
                    IpcErrorCode::IoError,
                    "agent_state_forward",
                    &format!(
                        "SSH forward exited before allocating a port (status {:?}): {}",
                        status.map(|status| status.code()),
                        seen.trim()
                    ),
                ));
            }
            Err(_) => {
                let _ = child.start_kill();
                return Err(super::runtime::error(
                    IpcErrorCode::IoError,
                    "agent_state_forward",
                    "SSH forward allocation timed out",
                ));
            }
        }
    }
}

/// Channels attached to live sessions, keyed by daemon session id.
fn channels() -> &'static Mutex<HashMap<String, (String, ForwardGuard)>> {
    static CHANNELS: OnceLock<Mutex<HashMap<String, (String, ForwardGuard)>>> = OnceLock::new();
    CHANNELS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) async fn attach(local_session_id: &str, token: String, forward: ForwardGuard) {
    channels()
        .lock()
        .await
        .insert(local_session_id.to_string(), (token, forward));
}

/// Kills the forwarding child and forgets the route. Idempotent.
pub(crate) async fn detach(local_session_id: &str) {
    let entry = channels().lock().await.remove(local_session_id);
    if let Some((token, forward)) = entry {
        if let Some(ingress) = AgentStateIngress::global() {
            ingress.revoke(&token).await;
        }
        forward.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::{SshHostSource, SshHost};

    fn host() -> SshHost {
        SshHost {
            id: "qa-agent-forward".into(),
            label: "qa".into(),
            hostname: "omarchy".into(),
            username: Some("indo".into()),
            port: None,
            identity_file: None,
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
        }
    }

    #[test]
    fn forward_plan_requests_loopback_remote_forward_without_clear_all_forwardings() {
        let plan = forward_plan(&host(), 41_000).expect("plan");
        let args = &plan.args;
        assert_eq!(plan.program, "ssh");
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-R", "127.0.0.1:0:127.0.0.1:41000"]),
            "missing loopback -R in {args:?}"
        );
        assert!(args.contains(&"-N".to_string()), "-N required: {args:?}");
        assert!(
            !args.contains(&"-tt".to_string()),
            "forwarding connection must not request a terminal: {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-o", "ExitOnForwardFailure=yes"]),
            "allocation failures must be fatal: {args:?}"
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg.contains("ClearAllForwardings")),
            "ClearAllForwardings would silently disable the forward: {args:?}"
        );
        assert_eq!(args.last().map(String::as_str), Some("indo@omarchy"));
    }

    #[test]
    fn parse_allocated_port_reads_openssh_notice() {
        assert_eq!(
            parse_allocated_port("Allocated port 44021 for remote forward to 127.0.0.1:41000"),
            Some(44021)
        );
        assert_eq!(parse_allocated_port("Warning: remote port forwarding failed"), None);
    }

    /// Opt-in live proof against a real host: run with
    /// `cargo test --lib -- --ignored ssh::agent_forward::tests::live_forward`.
    #[tokio::test]
    #[ignore = "live SSH proof: needs an SSH host running bash (FERRYX_QA_SSH_HOST, default omarchy)"]
    async fn live_forward_delivers_remote_report_into_the_hub() {
        let host_name = std::env::var("FERRYX_QA_SSH_HOST").unwrap_or_else(|_| "omarchy".to_string());
        let mut ssh_host = host();
        ssh_host.hostname = host_name.clone();
        let hub = Arc::new(AgentStateHub::default());
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("bind loopback ingress");
        listener.set_nonblocking(true).expect("nonblocking ingress");
        let port = listener.local_addr().expect("ingress addr").port();
        let listener = tokio::net::TcpListener::from_std(listener).expect("tokio ingress");
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::new(Mutex::new(HashMap::new()));
        let accept_routes = Arc::clone(&routes);
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let _ = serve_report(stream, accept_routes).await;
            }
        });
        let token = "live-token".to_string();
        routes.lock().await.insert(
            token.clone(),
            Route {
                hub: Arc::clone(&hub),
                remote_session_id: "remote-live".into(),
                local_session_id: Some("local-live".into()),
            },
        );
        let mut subscription = hub.subscribe("local-live");
        let forward = start_forward(&ssh_host, port).await.expect("forward");
        let payload = format!(
            r#"{{"type":"agentState","sessionId":"remote-live","state":"working","token":"{token}"}}"#
        );
        // base64 keeps the remote command free of quoting collisions with bash -c.
        use base64::prelude::BASE64_STANDARD;
        use base64::Engine as _;
        let payload_b64 = BASE64_STANDARD.encode(payload.as_bytes());
        let script = format!(
            "bash -c 'exec 3<>/dev/tcp/127.0.0.1/{}; printf %s {} | base64 -d >&3'",
            forward.remote_port, payload_b64
        );
        let ssh_program = crate::ssh::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
        let status = tokio::process::Command::new(&ssh_program)
            .args([
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=6",
                &host_name,
                &script,
            ])
            .status()
            .await
            .expect("remote probe");
        assert!(status.success(), "remote probe exited with {status}");
        let update = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match subscription.receiver.recv().await {
                    Ok(update) if update.state.session_id == "local-live" => return update,
                    Ok(_) => continue,
                    Err(error) => panic!("hub channel closed: {error}"),
                }
            }
        })
        .await
        .expect("remote report must reach the local hub");
        assert_eq!(update.state.state, "working");
        assert_eq!(update.state.origin, AgentStateOrigin::Agent);
        forward.close().await;
    }

    #[tokio::test]
    async fn unresolved_and_unauthenticated_reports_never_publish() {
        let hub = Arc::new(AgentStateHub::default());
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        drop(listener);
        let routes: Arc<Mutex<HashMap<String, Route>>> = Arc::new(Mutex::new(HashMap::new()));
        routes.lock().await.insert(
            "good-token".into(),
            Route {
                hub: Arc::clone(&hub),
                remote_session_id: "remote-1".into(),
                local_session_id: Some("local-1".into()),
            },
        );
        let (_, published) = resolve_report(
            r#"{"sessionId":"remote-1","state":"working","token":"good-token","type":"agentState"}"#,
            &routes,
        )
        .await
        .expect("authenticated report must resolve");
        assert_eq!(published.session_id, "local-1");
        assert_eq!(published.state, "working");
        assert!(resolve_report(
            r#"{"sessionId":"remote-1","state":"working","token":"other"}"#,
            &routes
        )
        .await
        .is_none());
        routes.lock().await.insert(
            "pending-token".into(),
            Route {
                hub: Arc::clone(&hub),
                remote_session_id: "remote-2".into(),
                local_session_id: None,
            },
        );
        assert!(resolve_report(
            r#"{"sessionId":"remote-2","state":"idle","token":"pending-token"}"#,
            &routes
        )
        .await
        .is_none());
        assert!(resolve_report(
            r#"{"sessionId":"remote-1","state":"scanning","token":"good-token"}"#,
            &routes
        )
        .await
        .is_none());
        assert!(resolve_report(
            r#"{"sessionId":"someone-else","state":"idle","token":"good-token"}"#,
            &routes
        )
        .await
        .is_none());
    }
}
