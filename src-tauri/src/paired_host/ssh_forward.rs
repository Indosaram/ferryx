//! `AttachPath::SshForward`: reaches a paired host's `ferryx --daemon` loopback gateway over a
//! single `ssh -W 127.0.0.1:<port>` stream so Direct, SSH, and Relay all talk to the same remote
//! daemon and compete on end-to-end handshake speed.
//!
//! Candidate SSH hosts are never trusted by hostname alone: the Noise IK handshake inside
//! `paired_host::attach` cryptographically binds `machine_id` and `machine_attach_public_key`,
//! and only a verified win records the `ssh_host_id <-> machine_id` link.

use super::inventory::PublishedOrigin;
use super::path_select::{AttachPath, CandidatePath};
use crate::ssh::SshHost;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::process::Command;

pub const SSH_FORWARD_PROBE_BUDGET: Duration = Duration::from_secs(4);
const MAX_SSH_CANDIDATES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ForwardKey {
    host_id: String,
    target: String,
    port: Option<u16>,
    identity_file: Option<String>,
    jump_host: Option<String>,
    remote_port: u16,
}

impl ForwardKey {
    fn of(host: &SshHost, remote_port: u16) -> Self {
        Self {
            host_id: host.id.clone(),
            target: host.target(),
            port: host.port,
            identity_file: host.identity_file.clone(),
            jump_host: host.jump_host.clone(),
            remote_port,
        }
    }
}

struct ForwardHandle {
    origin: String,
    _accept_task: tokio::task::AbortHandle,
}

#[derive(Default)]
struct SshLinkState {
    cached_hosts: Option<Vec<SshHost>>,
    machine_to_ssh: HashMap<String, String>,
    ssh_to_machine: HashMap<String, String>,
    origin_to_ssh: HashMap<String, String>,
    forwards: HashMap<ForwardKey, ForwardHandle>,
}

static STATE: LazyLock<Mutex<SshLinkState>> = LazyLock::new(|| Mutex::new(SshLinkState::default()));

pub fn ssh_forward_enabled() -> bool {
    !matches!(
        std::env::var("FERRYX_SSH_FORWARD").as_deref(),
        Ok("0" | "false" | "FALSE")
    )
}

pub fn register_ssh_hosts(hosts: Vec<SshHost>) {
    STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .cached_hosts = Some(hosts);
}

pub fn record_verified_link(ssh_host_id: &str, machine_id: &str) {
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    state
        .machine_to_ssh
        .insert(machine_id.to_owned(), ssh_host_id.to_owned());
    state
        .ssh_to_machine
        .insert(ssh_host_id.to_owned(), machine_id.to_owned());
}

pub fn record_verified_origin(origin: &str, machine_id: &str) {
    let ssh_host_id = STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .origin_to_ssh
        .get(origin)
        .cloned();
    if let Some(ssh_host_id) = ssh_host_id {
        record_verified_link(&ssh_host_id, machine_id);
    }
}

pub fn verified_ssh_host_id(machine_id: &str) -> Option<String> {
    STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .machine_to_ssh
        .get(machine_id)
        .cloned()
}

pub fn verified_machine_id(ssh_host_id: &str) -> Option<String> {
    STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ssh_to_machine
        .get(ssh_host_id)
        .cloned()
}

pub fn normalize_machine_token(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(prefix) = trimmed.strip_suffix(".local") {
        return prefix.to_owned();
    }
    if let Some((head, tail)) = trimmed.split_once('.') {
        if tail.ends_with(".ts.net") || tail == "ts.net" {
            return head.to_owned();
        }
    }
    trimmed
}

fn origin_host_and_port(origin: &str) -> Option<(String, Option<u16>)> {
    let url = reqwest::Url::parse(origin).ok()?;
    let host = url.host_str()?.trim_matches(['[', ']']).to_ascii_lowercase();
    Some((host, url.port()))
}

pub fn remote_gateway_port(published_origins: &[PublishedOrigin]) -> u16 {
    published_origins
        .iter()
        .find_map(|o| origin_host_and_port(&o.origin).and_then(|(_, port)| port))
        .unwrap_or(crate::remote::state::REMOTE_GATEWAY_PORT)
}

pub fn match_ssh_hosts(
    hosts: &[SshHost],
    verified_id: Option<&str>,
    explicit_ssh_host_id: Option<&str>,
    display_label: &str,
    published_origins: &[PublishedOrigin],
) -> Vec<SshHost> {
    let enabled: Vec<&SshHost> = hosts
        .iter()
        .filter(|h| !h.disabled.unwrap_or(false) && !h.hostname.trim().is_empty())
        .collect();
    if enabled.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |host: &SshHost| {
        if out.len() < MAX_SSH_CANDIDATES && seen.insert(host.id.clone()) {
            out.push(host.clone());
        }
    };

    if let Some(id) = explicit_ssh_host_id {
        if let Some(host) = enabled.iter().find(|h| h.id == id) {
            push(host);
        }
    }
    if let Some(id) = verified_id {
        if let Some(host) = enabled.iter().find(|h| h.id == id) {
            push(host);
        }
    }

    let published_hosts: HashSet<String> = published_origins
        .iter()
        .filter_map(|o| origin_host_and_port(&o.origin).map(|(h, _)| h))
        .collect();
    if !published_hosts.is_empty() {
        let mut by_origin: Vec<&SshHost> = enabled
            .iter()
            .copied()
            .filter(|h| published_hosts.contains(&h.hostname.trim().to_ascii_lowercase()))
            .collect();
        by_origin.sort_by_key(|h| usize::from(h.username.is_none()));
        for host in by_origin {
            push(host);
        }
    }

    let wanted = normalize_machine_token(display_label);
    if !wanted.is_empty() {
        let mut matched_ips = HashSet::new();
        let mut direct_label_matches = Vec::new();
        for host in &enabled {
            let label_norm = normalize_machine_token(&host.label);
            let host_norm = normalize_machine_token(&host.hostname);
            if label_norm == wanted || host_norm == wanted {
                matched_ips.insert(host.hostname.trim().to_ascii_lowercase());
                direct_label_matches.push(*host);
            }
        }
        let mut siblings: Vec<&SshHost> = enabled
            .iter()
            .copied()
            .filter(|h| matched_ips.contains(&h.hostname.trim().to_ascii_lowercase()))
            .collect();
        siblings.sort_by_key(|h| {
            (
                usize::from(h.username.is_none()),
                usize::from(h.id.starts_with("tailscale:")),
            )
        });
        for host in siblings.into_iter().chain(direct_label_matches) {
            push(host);
        }
    }

    out
}

pub fn forward_args(host: &SshHost, remote_port: u16) -> Vec<String> {
    forward_args_reusing(host, remote_port, None)
}

/// Argv for one stdio forward, optionally riding the daemon's existing SSH master.
///
/// Reuse is what keeps a retried attach from becoming a redial storm: every cold -W attempt
/// repeats TCP, KEX, authentication and host-key verification, so a caller that retries twice a
/// second pins a core spawning ssh processes. Handing the client the master's control socket
/// turns those retries into channel opens on a connection that already exists. The reuse options
/// come first because OpenSSH honours the earliest -o for a given keyword.
pub fn forward_args_reusing(
    host: &SshHost,
    remote_port: u16,
    master_socket: Option<&std::path::Path>,
) -> Vec<String> {
    let mut args = match master_socket {
        Some(socket) => crate::ssh::master::client_reuse_args(socket),
        None => Vec::new(),
    };
    args.extend([
        "-W".to_owned(),
        format!("127.0.0.1:{remote_port}"),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "StrictHostKeyChecking=yes".to_owned(),
        "-o".to_owned(),
        "UpdateHostKeys=no".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=3".to_owned(),
        "-o".to_owned(),
        "ConnectionAttempts=1".to_owned(),
        "-o".to_owned(),
        "ServerAliveInterval=15".to_owned(),
        "-o".to_owned(),
        "ServerAliveCountMax=2".to_owned(),
        "-o".to_owned(),
        "PermitLocalCommand=no".to_owned(),
        "-o".to_owned(),
        "ClearAllForwardings=yes".to_owned(),
    ]);
    if let Some(port) = host.port {
        args.push("-p".to_owned());
        args.push(port.to_string());
    }
    if let Some(identity) = &host.identity_file {
        if !identity.trim().is_empty() {
            args.push("-i".to_owned());
            args.push(identity.clone());
        }
    }
    if let Some(jump) = &host.jump_host {
        if !jump.trim().is_empty() {
            args.push("-J".to_owned());
            args.push(jump.clone());
        }
    }
    args.push(host.target());
    args
}

async fn load_hosts_snapshot() -> Vec<SshHost> {
    if let Some(hosts) = STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .cached_hosts
        .clone()
    {
        return hosts;
    }
    let path: PathBuf = crate::daemon::server::daemon_ssh_store_path();
    let loaded = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice::<crate::ipc::ssh::SshHostStore>(&bytes)
            .ok()
            .map(|store| store.hosts)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    if !loaded.is_empty() {
        let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
        if state.cached_hosts.is_none() {
            state.cached_hosts = Some(loaded.clone());
        }
    }
    loaded
}

async fn pump_stdio_forward(mut tcp: tokio::net::TcpStream, host: SshHost, remote_port: u16) {
    let program = crate::ssh::exec::resolve_ssh_program().unwrap_or_else(|_| "ssh".to_owned());
    // Held for the whole stream: the lease is what keeps this master counted as active, so the
    // idle reaper cannot close the socket while bytes are still flowing through it.
    let lease = crate::ssh::master::acquire(&host).await.ok().flatten();
    crate::ssh::master::note_path("ssh_forward", lease.is_some());
    let args = forward_args_reusing(&host, remote_port, lease.as_ref().map(|held| held.socket()));
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = cmd.spawn() else {
        return;
    };
    let (Some(mut child_in), Some(mut child_out)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill().await;
        return;
    };
    let (mut tcp_rx, mut tcp_tx) = tcp.split();
    let to_ssh = tokio::io::copy(&mut tcp_rx, &mut child_in);
    let from_ssh = tokio::io::copy(&mut child_out, &mut tcp_tx);
    let _ = tokio::select! {
        r = to_ssh => r,
        r = from_ssh => r,
    };
    let _ = child.kill().await;
}

pub async fn ensure_forward_origin(host: &SshHost, remote_port: u16) -> Option<String> {
    let key = ForwardKey::of(host, remote_port);
    if let Some(origin) = STATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .forwards
        .get(&key)
        .map(|h| h.origin.clone())
    {
        return Some(origin);
    }
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .ok()?;
    let local_addr = listener.local_addr().ok()?;
    let origin = format!("http://{local_addr}");
    let host_bg = host.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let _ = tcp.set_nodelay(true);
            let h = host_bg.clone();
            tokio::spawn(pump_stdio_forward(tcp, h, remote_port));
        }
    });
    let mut state = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(existing) = state.forwards.get(&key) {
        task.abort();
        return Some(existing.origin.clone());
    }
    state
        .origin_to_ssh
        .insert(origin.clone(), host.id.clone());
    state.forwards.insert(
        key,
        ForwardHandle {
            origin: origin.clone(),
            _accept_task: task.abort_handle(),
        },
    );
    Some(origin)
}

pub async fn candidates_for_paired(
    machine_id: &str,
    explicit_ssh_host_id: Option<&str>,
    display_label: &str,
    published_origins: &[PublishedOrigin],
    bearer: &str,
) -> Vec<CandidatePath> {
    if !ssh_forward_enabled() {
        return Vec::new();
    }
    let hosts = load_hosts_snapshot().await;
    if hosts.is_empty() {
        return Vec::new();
    }
    let verified = verified_ssh_host_id(machine_id);
    let matched = match_ssh_hosts(
        &hosts,
        verified.as_deref(),
        explicit_ssh_host_id,
        display_label,
        published_origins,
    );
    if matched.is_empty() {
        return Vec::new();
    }
    let remote_port = remote_gateway_port(published_origins);
    let mut out = Vec::with_capacity(matched.len());
    for host in matched {
        if let Some(origin) = ensure_forward_origin(&host, remote_port).await {
            let mut candidate = CandidatePath::with_expected_machine_id(
                AttachPath::SshForward,
                origin,
                Some(bearer.to_owned()),
                Some(machine_id.to_owned()),
            );
            candidate.probe_budget = Some(SSH_FORWARD_PROBE_BUDGET);
            out.push(candidate);
        }
    }
    out
}

#[cfg(test)]
pub(crate) async fn spawn_test_forward<C, F, Fut>(connector: F) -> (String, tokio::task::AbortHandle)
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = std::io::Result<C>> + Send + 'static,
{
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let connector = Arc::new(connector);
    let handle = tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            let connector = Arc::clone(&connector);
            tokio::spawn(async move {
                if let Ok(mut remote) = connector().await {
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut remote).await;
                }
            });
        }
    });
    (origin, handle.abort_handle())
}

#[cfg(test)]
#[path = "ssh_forward_tests.rs"]
mod tests;
