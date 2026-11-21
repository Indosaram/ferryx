pub mod agent_extension;
pub mod agent_hook_install;
pub(crate) mod agent_observer;
pub(crate) mod agent_state;
pub mod client;
// Frozen, version-stable control channel ("control-v1"), separate from the data protocol.
pub mod control;
// Abnormal-exit supervision: the restart decision, its crash log, and the keeper gate that keeps
// supervised restart off unless a live PTY keeper is registered (Phase 2; the keeper itself is
// Phase 3 and is reachable only through `crash_supervisor::KeeperPresence`).
pub mod crash_supervisor;
pub mod dag_service;
pub mod handover;
// Predecessor-authored handover request that authorizes one HandoverCandidate start (todo 8).
pub(crate) mod handover_request;
#[cfg(unix)]
pub mod handover_socket;
pub mod handover_transaction;
#[cfg(unix)]
pub mod handover_wire;
pub mod keeper_client;
// Independent macOS LaunchAgent for the PTY keeper: its own label, its own log files, and its own
// generation directory under `~/.ferryx/keeper/<version>/` so replacing the app bundle never
// replaces the process that owns the masters. `launchctl` service ownership does not exist off
// macOS, and the keeper itself is introduced on macOS first, so the module stays behind the gate
// instead of pretending to manage a service it cannot.
#[cfg(target_os = "macos")]
pub mod keeper_launchd;
// Production keeper integration: durable per-session ownership, the enable/disable and rollback
// gates, keeper-backed session I/O, attach/recovery with checkpoint plus bounded replay, and the
// checkpoint producer. The keeper is a separate binary reached only across `keeper_client`'s wire.
#[cfg(unix)]
pub mod keeper_session;
#[cfg(target_os = "macos")]
pub mod keeper_startup;
// macOS LaunchAgent ownership of the daemon's lifetime. The GUI arms it on startup and the daemon
// client arms it on demand; the `launchctl`-spawning paths inside are gated to
// `#[cfg(target_os = "macos")]`.
pub mod launchd;
pub(crate) mod logging;
pub mod manifest;
pub mod parent_notice;
pub(crate) mod owner_admission;
pub mod protocol;
pub mod proxy;
// Pane-liveness QA barrier producers (handover transfer/rollback, held remote RPC).
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub mod qa_producers;
// QA-only compile-time salt that makes builds B and C hash to distinct generation keys (todo 12).
#[cfg(feature = "local-split-qa")]
pub mod qa_build_salt;
pub mod resource_usage;
// The production daemon rollback: the plan's rule that a build without keeper support cannot become
// the running daemon while any keeper-owned session lives, refusing with the typed count, plus the
// plist-only switch that a successful rollback performs. `service rollback` reaches this, and nothing
// here signals, stops or restarts a running daemon.
pub mod rollback;
// Immutable generation-keyed copies of the daemon runtime outside the GUI install; the OS job of
// each generation runs its own copy, so replacing or moving the app never changes a live daemon.
pub mod runtime_stage;
pub mod server;
pub mod session_lifecycle;
pub mod session_service;
pub mod sidebar_snapshot;
// Todo 7: daemon ownership of tunnel definitions, runtimes and the host observer.
pub mod tunnel_dispatch;
#[cfg(all(test, unix))]
mod tunnel_dispatch_tests;
pub mod split_journal;
// Portable facade over the OS service manager that owns the daemon's lifetime (macOS: `launchd`).
pub mod supervisor;
// Todo 3 part A: pure supervisor lease role machine (Holder/Provisional/Standby/Unleased).
pub mod supervisor_lease_role;
// Windows backend of the facade: per-user scheduled task + `--daemon-supervisor` watchdog. Its pure
// parts (task XML, watchdog decisions) build and test on every target.
pub mod supervisor_windows;
// systemd user unit and sd_notify for the Linux supervisor backend.
#[cfg(unix)]
pub mod supervisor_linux;
// Todo 3: talks to the relay supervisor lease and publishes the hub role.
pub mod supervisor_lease_client;
// Todo 9: the pending-update state when the live owner speaks another data protocol; only the
// control channel is used, and the owner is never ended without the user's exact-count consent.
pub mod update_pending;
pub mod workspace_service;
// Todo 9: the end-to-end forwarding proof runs a real loopback sshd, so it is Unix-only.
#[cfg(all(test, unix))]
pub mod tunnel_forwarding;
// Todo 13: refuse supervisor mutations from processes inside worker panes.
#[cfg(test)]
pub mod recovery_consistency_tests;
pub mod supervisor_peer;
pub mod supervisor_peer_win;

/// Shared headless authority supplied only by the session-owning daemon.
pub struct MachineServices {
    pub sessions: std::sync::Arc<session_service::DaemonSessionService>,
    pub workspaces: std::sync::Arc<workspace_service::DaemonWorkspaceService>,
}

/// Whether a refusal to arm a socket timeout is Darwin's `EINVAL` for a socket whose peer has already
/// both half-closed, rather than a real failure to bound the socket.
///
/// xnu's `sosetopt` answers `EINVAL` for `SO_RCVTIMEO`/`SO_SNDTIMEO` once `SS_CANTRCVMORE` and
/// `SS_CANTSENDMORE` are both set, while Linux accepts the same call on the same socket. On such a
/// socket the bound is moot - every read returns `EOF` and every write fails immediately - so a
/// caller may carry on without it and let the following read or write report the real outcome.
///
/// Only that exact combination answers `true`. An `EINVAL` on a still-connected socket, and every
/// other error, stays a real error: this predicate exists to skip a bound that cannot matter, never
/// to swallow a failure to bound a live peer.
#[cfg(unix)]
pub(crate) fn refused_timeout_on_hung_up_peer(
    stream: &std::os::unix::net::UnixStream,
    error: &std::io::Error,
) -> bool {
    use std::os::fd::AsRawFd as _;

    if error.kind() != std::io::ErrorKind::InvalidInput {
        return false;
    }
    let mut descriptor = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `descriptor` is one valid pollfd and a zero millisecond timeout only inspects state.
    let ready = unsafe { libc::poll(&mut descriptor, 1, 0) };
    ready > 0 && descriptor.revents & libc::POLLHUP != 0
}

pub use client::*;
pub use handover::*;
#[cfg(unix)]
pub use handover_socket::*;
pub use handover_transaction::*;
#[cfg(unix)]
pub use handover_wire::*;
pub use launchd::*;
pub use manifest::*;
pub use protocol::*;
pub use proxy::*;
pub use server::*;
pub use session_lifecycle::*;

