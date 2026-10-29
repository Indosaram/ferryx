pub mod agent_extension;
pub mod agent_hook_install;
pub(crate) mod agent_observer;
pub(crate) mod agent_state;
pub mod client;
// Frozen, version-stable control channel ("control-v1"), separate from the data protocol.
pub mod control;
pub mod dag_service;
pub mod handover;
// Predecessor-authored handover request that authorizes one HandoverCandidate start (todo 8).
pub(crate) mod handover_request;
#[cfg(unix)]
pub mod handover_socket;
pub mod handover_transaction;
#[cfg(unix)]
pub mod handover_wire;
// macOS LaunchAgent ownership of the daemon's lifetime. The GUI arms it on startup and the daemon
// client arms it on demand; the `launchctl`-spawning paths inside are gated to
// `#[cfg(target_os = "macos")]`.
pub mod launchd;
pub(crate) mod logging;
pub mod manifest;
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
// Immutable generation-keyed copies of the daemon runtime outside the GUI install; the OS job of
// each generation runs its own copy, so replacing or moving the app never changes a live daemon.
pub mod runtime_stage;
pub mod server;
pub mod session_lifecycle;
pub mod session_service;
pub mod sidebar_snapshot;
pub mod split_journal;
// Portable facade over the OS service manager that owns the daemon's lifetime (macOS: `launchd`).
pub mod supervisor;
// Windows backend of the facade: per-user scheduled task + `--daemon-supervisor` watchdog. Its pure
// parts (task XML, watchdog decisions) build and test on every target.
pub mod supervisor_windows;
// systemd user unit and sd_notify for the Linux supervisor backend.
#[cfg(unix)]
pub mod supervisor_linux;
// Todo 9: the pending-update state when the live owner speaks another data protocol; only the
// control channel is used, and the owner is never ended without the user's exact-count consent.
pub mod update_pending;
pub mod workspace_service;
// Todo 13: refuse supervisor mutations from processes inside worker panes.
#[cfg(test)]
pub mod recovery_consistency_tests;
pub mod supervisor_peer;

/// Shared headless authority supplied only by the session-owning daemon.
pub struct MachineServices {
    pub sessions: std::sync::Arc<session_service::DaemonSessionService>,
    pub workspaces: std::sync::Arc<workspace_service::DaemonWorkspaceService>,
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

