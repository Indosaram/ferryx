//! Cross-pane grants: which Ferryx pane may mutate which other pane (plan todo 5).
//!
//! Every process inside a Ferryx pane runs as the owner's uid and can reach the owner socket, so
//! the socket alone does not tell the owner from a worker agent in another pane. The daemon
//! classifies each connection's peer (`supervisor_peer::classify`) and applies this policy to
//! every mutating request:
//!
//! - an `Owner` peer (outside every pane: the desktop app, an ssh shell, the in-process hub,
//!   `ferryx-cli mcp` run outside a pane) is allowed everything;
//! - a `Pane { src }` peer may act on its own session, spawn and manage worktrees in its own
//!   workspace, and do anything else only through a grant the owner created
//!   (`src_session|src_workspace -> dst_session|dst_workspace|*`, a set of ops); otherwise the
//!   request is refused `FORBIDDEN` with `PANE_GRANT_REQUIRED` (pane targets) or
//!   `OWNER_ACTION_REQUIRED` (owner-wide actions);
//! - `TokenAuthenticated` (Windows loopback) and `Unknown` (in-process pairs) peers keep the
//!   pre-existing behaviour for every variant gated here until the Windows transport can name
//!   its peer (todo 12); they may still not change grants.
//!
//! Supervisor fence fields never grant pane access: the gate runs before the fence is admitted,
//! so a refused request never ratchets `supervisor-fence.json`.
//!
//! The grants live in `pane-grants.json` (atomic tmp + rename) and ride the handover decision
//! record, so a successor serves the same grants.
use crate::daemon::protocol::DaemonRequest;
use crate::daemon::supervisor_peer::PeerClass;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// File name of the grant store, beside the daemon's other identity state.
pub const PANE_GRANTS_FILE: &str = "pane-grants.json";
/// Capability string a daemon that serves `PaneGrant` advertises.
pub const PANE_GRANTS_CAPABILITY: &str = "paneGrantsV1";
/// Refusal reason for a pane acting on another pane without a grant.
pub const PANE_GRANT_REQUIRED: &str = "PANE_GRANT_REQUIRED";
/// Refusal reason for an owner-wide action from inside a pane without an `admin` grant.
pub const OWNER_ACTION_REQUIRED: &str = "OWNER_ACTION_REQUIRED";
/// Where grants are made.
pub const GRANT_HINT: &str = "open Settings > Pane access";
/// The target that matches every session and workspace.
pub const ANY_TARGET: &str = "*";
/// More grants than this are refused (256 panes x 16 grants), which keeps the file well under
/// 1 MiB.
pub const MAX_GRANTS: usize = 4096;
/// Longest accepted session or workspace id in a grant.
const MAX_ID_LEN: usize = 256;

/// What a grant allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GrantOp {
    /// Type into the target: write, paste, prompt answers, signals, agent send/answer/stop.
    Write,
    /// Change the target's lifecycle: close, hibernate, suspend, resume, resize, reset state.
    Control,
    /// Spawn panes and create or delete worktrees in the target workspace.
    Spawn,
    /// Owner-wide actions (upgrade, pairing, remote config, shutdown); implies every other op.
    Admin,
}

impl GrantOp {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "write" => Some(Self::Write),
            "control" => Some(Self::Control),
            "spawn" => Some(Self::Spawn),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

/// One grant: panes whose session or workspace is `from` may perform `ops` on `to` (a session
/// id, a workspace id or `*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneGrant {
    pub from: String,
    pub to: String,
    pub ops: BTreeSet<GrantOp>,
}

impl PaneGrant {
    fn validate(&self) -> Result<(), String> {
        for (name, value) in [("from", &self.from), ("to", &self.to)] {
            if value.is_empty() || value.len() > MAX_ID_LEN || value.chars().any(char::is_control) {
                return Err(format!("grant `{name}` must be 1..={MAX_ID_LEN} printable characters"));
            }
        }
        if self.from == ANY_TARGET {
            return Err("grant `from` must name one session or workspace".into());
        }
        if self.ops.is_empty() {
            return Err("a grant needs at least one op".into());
        }
        if self.ops.contains(&GrantOp::Admin) && self.to != ANY_TARGET {
            return Err("the admin op is granted on target `*` only".into());
        }
        Ok(())
    }
}

/// What a request needs from a pane peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Acts on one session.
    Session { session_id: String, op: GrantOp },
    /// Acts on one workspace (spawn, worktrees).
    Workspace { workspace_id: String, op: GrantOp },
    /// Acts on a target this daemon cannot name (a paired host's pane): needs a `*` grant.
    AnyTarget { op: GrantOp },
    /// Owner-wide: needs an `admin` grant.
    Admin,
    /// Changes the grants themselves: only the owner.
    OwnerOnly,
}

/// The gate a request passes, or `None` for a request a pane may always send (reads).
pub fn gate_for(request: &DaemonRequest) -> Option<Gate> {
    use crate::agent_control::AgentControlOp as Op;
    let session = |session_id: &str, op| Some(Gate::Session { session_id: session_id.to_owned(), op });
    let workspace = |workspace_id: &str| Some(Gate::Workspace { workspace_id: workspace_id.to_owned(), op: GrantOp::Spawn });
    match request {
        DaemonRequest::Write { session_id, .. }
        | DaemonRequest::BeginPaste { session_id }
        | DaemonRequest::PasteWrite { session_id, .. }
        | DaemonRequest::CancelPaste { session_id, .. }
        | DaemonRequest::AgentPromptAnswer { session_id, .. }
        | DaemonRequest::Signal { session_id, .. } => session(session_id, GrantOp::Write),
        DaemonRequest::Close { session_id }
        | DaemonRequest::Hibernate { session_id }
        | DaemonRequest::Suspend { session_id }
        | DaemonRequest::Resume { session_id }
        | DaemonRequest::Resize { session_id, .. }
        | DaemonRequest::SetScrollbackBudget { session_id, .. }
        | DaemonRequest::ResetAgentState { session_id } => session(session_id, GrantOp::Control),
        DaemonRequest::AgentControl { request, host_id, .. } => {
            let target = match request {
                Op::Send { session_id, .. }
                | Op::Answer { session_id, .. }
                | Op::Stop { session_id, .. }
                | Op::Deliver { session_id, .. } => session_id,
                Op::List | Op::Read { .. } | Op::Wait { .. } | Op::Events { .. } => return None,
            };
            match host_id {
                None => session(target, GrantOp::Write),
                Some(_) => Some(Gate::AnyTarget { op: GrantOp::Write }),
            }
        }
        DaemonRequest::Spawn { workspace_id, .. }
        | DaemonRequest::CreateWorktree { workspace_id, .. }
        | DaemonRequest::DeleteWorktree { workspace_id, .. } => workspace(workspace_id),
        DaemonRequest::PaneGrant { request } => match request {
            PaneGrantAction::List | PaneGrantAction::CheckAdmin => None,
            PaneGrantAction::Add { .. } | PaneGrantAction::Remove { .. } => Some(Gate::OwnerOnly),
        },
        DaemonRequest::SetAgentIntegration { .. }
        | DaemonRequest::LayoutApply { .. }
        | DaemonRequest::RegisterWorkspace { .. }
        | DaemonRequest::UnregisterWorkspace { .. }
        | DaemonRequest::SaveSession { .. }
        | DaemonRequest::ClearSession
        | DaemonRequest::SyncDesktopSidebar { .. }
        | DaemonRequest::UpgradeBinary { .. }
        | DaemonRequest::PrepareHandover
        | DaemonRequest::TransferSessions { .. }
        | DaemonRequest::CommitHandover { .. }
        | DaemonRequest::Shutdown
        | DaemonRequest::RemoteConfigure { .. }
        | DaemonRequest::RemoteCreateMachinePairingCode
        | DaemonRequest::RemoteCreatePairingCode { .. }
        | DaemonRequest::RemoteRevokeDevice { .. }
        | DaemonRequest::RemoteSetDevicePermission { .. }
        | DaemonRequest::PairedHostOperation { .. }
        | DaemonRequest::PairedHostPair { .. }
        | DaemonRequest::PairedHostMigrateLegacy { .. }
        | DaemonRequest::PairedHostRedeemGrant { .. }
        | DaemonRequest::PairedHostForget { .. }
        | DaemonRequest::PairedHostRevoke { .. }
        | DaemonRequest::RemoteDesktopHostSet { .. }
        | DaemonRequest::RemoteDesktopHostRespond { .. }
        | DaemonRequest::AgentControlSettingsSet { .. }
        | DaemonRequest::UploadClipboardImage { .. } => Some(Gate::Admin),
        _ => None,
    }
}

/// Why a request was refused: the wire code is always `FORBIDDEN`; `reason` is
/// [`PANE_GRANT_REQUIRED`] or [`OWNER_ACTION_REQUIRED`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: &'static str,
    pub message: String,
}

/// Grant change requests on the wire (`DaemonRequest::PaneGrant`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum PaneGrantAction {
    /// Adds `grant` (its ops merge into an existing grant with the same `from` and `to`).
    Add { grant: PaneGrant },
    /// Removes the grant from `from` to `to`.
    #[serde(rename_all = "camelCase")]
    Remove { from: String, to: String },
    List,
    /// Whether the calling peer may take owner-wide actions (outside every pane, or a pane
    /// holding an `admin` grant). Used by in-pane `ferryx-cli up`/`pair`.
    CheckAdmin,
}

/// The grant store.
#[derive(Debug, Default)]
pub struct PaneGrants {
    path: Option<PathBuf>,
    grants: parking_lot::RwLock<Vec<PaneGrant>>,
}

impl PaneGrants {
    /// The grants saved at `path` (none when it is missing or unreadable; an unreadable file is
    /// logged and fails closed: no grants). Blocking.
    pub fn load(path: Option<PathBuf>) -> Self {
        let grants = path
            .as_deref()
            .and_then(|path| match std::fs::read(path) {
                Ok(bytes) => serde_json::from_slice::<Vec<PaneGrant>>(&bytes)
                    .map_err(|error| tracing::warn!(%error, "pane-grants.json is unreadable; no pane grants apply"))
                    .ok(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    tracing::warn!(%error, "pane-grants.json could not be read; no pane grants apply");
                    None
                }
            })
            .unwrap_or_default();
        Self { path, grants: parking_lot::RwLock::new(grants) }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn list(&self) -> Vec<PaneGrant> {
        self.grants.read().clone()
    }

    /// Adds (or widens) a grant and saves the store. Blocking.
    pub fn add(&self, grant: PaneGrant) -> Result<Vec<PaneGrant>, String> {
        grant.validate()?;
        let mut grants = self.grants.write();
        let mut next = grants.clone();
        match next.iter_mut().find(|known| known.from == grant.from && known.to == grant.to) {
            Some(known) => known.ops.extend(grant.ops),
            None if next.len() >= MAX_GRANTS => return Err(format!("at most {MAX_GRANTS} pane grants are kept")),
            None => next.push(grant),
        }
        self.save(&next)?;
        *grants = next.clone();
        Ok(next)
    }

    /// Removes the grant from `from` to `to` and saves the store. Blocking.
    pub fn remove(&self, from: &str, to: &str) -> Result<Vec<PaneGrant>, String> {
        let mut grants = self.grants.write();
        let next: Vec<PaneGrant> = grants
            .iter()
            .filter(|grant| !(grant.from == from && grant.to == to))
            .cloned()
            .collect();
        self.save(&next)?;
        *grants = next.clone();
        Ok(next)
    }

    /// Replaces every grant (a successor adopting the predecessor's record). Blocking.
    pub fn replace(&self, next: Vec<PaneGrant>) -> Result<(), String> {
        let mut grants = self.grants.write();
        self.save(&next)?;
        *grants = next;
        Ok(())
    }

    fn save(&self, grants: &[PaneGrant]) -> Result<(), String> {
        let Some(path) = &self.path else { return Ok(()) };
        write_atomic(path, &serde_json::to_vec_pretty(grants).map_err(|error| error.to_string())?)
    }

    /// Whether a grant lets the pane `src` (in workspace `src_workspace`) perform `op` on a
    /// target named by any of `targets`.
    fn allows(&self, src: &str, src_workspace: Option<&str>, targets: &[&str], op: GrantOp) -> bool {
        self.grants.read().iter().any(|grant| {
            (grant.from == src || Some(grant.from.as_str()) == src_workspace)
                && (grant.to == ANY_TARGET || targets.contains(&grant.to.as_str()))
                && (grant.ops.contains(&op) || grant.ops.contains(&GrantOp::Admin))
        })
    }

    /// Whether `class` may take owner-wide actions.
    pub fn admin_allowed(&self, class: &PeerClass, workspace_of: impl Fn(&str) -> Option<String>) -> bool {
        decide(class, &Gate::Admin, self, workspace_of).is_ok()
    }
}

/// tmp + fsync + rename, so a crash leaves the old or the new file, never half of one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })
}

/// The policy: may a peer of `class` pass `gate`? `workspace_of` names a session's workspace.
pub fn decide(
    class: &PeerClass,
    gate: &Gate,
    grants: &PaneGrants,
    workspace_of: impl Fn(&str) -> Option<String>,
) -> Result<(), Refusal> {
    let src = match class {
        PeerClass::Owner => return Ok(()),
        // Unidentified peers keep today's behaviour for every newly gated request (Windows
        // loopback until todo 12, in-process pairs); they never change grants.
        PeerClass::TokenAuthenticated | PeerClass::Unknown => {
            return match gate {
                Gate::OwnerOnly => Err(owner_only()),
                _ => Ok(()),
            }
        }
        PeerClass::Pane { session_id, .. } => session_id.as_str(),
    };
    let src_workspace = workspace_of(src);
    let src_workspace = src_workspace.as_deref();
    let pane_grant_required = |target: &str| Refusal {
        reason: PANE_GRANT_REQUIRED,
        message: format!("{PANE_GRANT_REQUIRED}: the pane {src} has no grant for {target}; {GRANT_HINT}"),
    };
    match gate {
        Gate::Session { session_id, op } => {
            if session_id == src {
                return Ok(());
            }
            let dst_workspace = workspace_of(session_id);
            let mut targets = vec![session_id.as_str()];
            targets.extend(dst_workspace.as_deref());
            if grants.allows(src, src_workspace, &targets, *op) {
                Ok(())
            } else {
                Err(pane_grant_required(session_id))
            }
        }
        Gate::Workspace { workspace_id, op } => {
            if Some(workspace_id.as_str()) == src_workspace
                || grants.allows(src, src_workspace, &[workspace_id.as_str()], *op)
            {
                Ok(())
            } else {
                Err(pane_grant_required(workspace_id))
            }
        }
        Gate::AnyTarget { op } => {
            if grants.allows(src, src_workspace, &[], *op) {
                Ok(())
            } else {
                Err(pane_grant_required("a paired host's pane"))
            }
        }
        Gate::Admin => {
            if grants.allows(src, src_workspace, &[], GrantOp::Admin) {
                Ok(())
            } else {
                Err(Refusal {
                    reason: OWNER_ACTION_REQUIRED,
                    message: format!("{OWNER_ACTION_REQUIRED}: run from Settings > Pane access or outside a pane (the pane {src} has no admin grant)"),
                })
            }
        }
        Gate::OwnerOnly => Err(owner_only()),
    }
}

fn owner_only() -> Refusal {
    Refusal {
        reason: OWNER_ACTION_REQUIRED,
        message: format!("only the owner may change pane grants; {GRANT_HINT} or run `ferryx-cli grants` outside any pane"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::AgentControlOp;

    fn pane(id: &str) -> PeerClass {
        PeerClass::Pane { session_id: id.into(), why: "test".into() }
    }

    fn workspaces(session: &str) -> Option<String> {
        match session {
            "a" | "a2" => Some("ws-a".into()),
            "b" => Some("ws-b".into()),
            _ => None,
        }
    }

    fn grant(from: &str, to: &str, ops: &[GrantOp]) -> PaneGrant {
        PaneGrant { from: from.into(), to: to.into(), ops: ops.iter().copied().collect() }
    }

    fn write(session: &str) -> DaemonRequest {
        DaemonRequest::Write { session_id: session.into(), data: b"x".to_vec() }
    }

    fn agents(op: AgentControlOp) -> DaemonRequest {
        DaemonRequest::AgentControl { request: op, host_id: None, supervisor_fence: None, supervisor_account: None }
    }

    fn check(class: &PeerClass, request: &DaemonRequest, grants: &PaneGrants) -> Result<(), &'static str> {
        match gate_for(request) {
            None => Ok(()),
            Some(gate) => decide(class, &gate, grants, workspaces).map_err(|refusal| refusal.reason),
        }
    }

    #[test]
    fn a_pane_needs_a_grant_for_another_pane_but_not_for_itself() {
        let grants = PaneGrants::default();
        assert_eq!(check(&pane("a"), &write("b"), &grants), Err(PANE_GRANT_REQUIRED));
        assert_eq!(check(&pane("a"), &write("a"), &grants), Ok(()));
        assert_eq!(check(&PeerClass::Owner, &write("b"), &grants), Ok(()));
        grants.add(grant("a", "b", &[GrantOp::Write])).unwrap();
        assert_eq!(check(&pane("a"), &write("b"), &grants), Ok(()));
        // The op set is honoured: write does not allow closing.
        assert_eq!(
            check(&pane("a"), &DaemonRequest::Close { session_id: "b".into() }, &grants),
            Err(PANE_GRANT_REQUIRED)
        );
        // The grant is directional.
        assert_eq!(check(&pane("b"), &write("a"), &grants), Err(PANE_GRANT_REQUIRED));
    }

    #[test]
    fn workspace_grants_and_spawn_in_the_own_workspace() {
        let grants = PaneGrants::default();
        let spawn = |ws: &str| DaemonRequest::Spawn {
            client_request_id: "r".into(),
            workspace_id: ws.into(),
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            shell: None,
            startup: None,
            local_split: None,
            session_id: None,
        };
        assert_eq!(check(&pane("a"), &spawn("ws-a"), &grants), Ok(()));
        assert_eq!(check(&pane("a"), &spawn("ws-b"), &grants), Err(PANE_GRANT_REQUIRED));
        // A grant from the source workspace to the target workspace covers every pane in both.
        grants.add(grant("ws-a", "ws-b", &[GrantOp::Write, GrantOp::Spawn])).unwrap();
        assert_eq!(check(&pane("a2"), &spawn("ws-b"), &grants), Ok(()));
        assert_eq!(check(&pane("a2"), &write("b"), &grants), Ok(()));
    }

    #[test]
    fn grants_change_only_from_the_owner() {
        let grants = PaneGrants::default();
        let add = DaemonRequest::PaneGrant {
            request: PaneGrantAction::Add { grant: grant("a", "b", &[GrantOp::Write]) },
        };
        for class in [pane("a"), PeerClass::TokenAuthenticated, PeerClass::Unknown] {
            assert_eq!(check(&class, &add, &grants), Err(OWNER_ACTION_REQUIRED), "{class:?}");
        }
        assert_eq!(check(&PeerClass::Owner, &add, &grants), Ok(()));
        let list = DaemonRequest::PaneGrant { request: PaneGrantAction::List };
        assert_eq!(check(&pane("a"), &list, &grants), Ok(()));
    }

    /// Windows token peers (and in-process pairs) keep today's behaviour for every newly gated
    /// request until todo 12.
    #[test]
    fn token_peers_are_exempt_from_the_new_gates() {
        let grants = PaneGrants::default();
        for request in [write("b"), DaemonRequest::Shutdown, DaemonRequest::UpgradeBinary { new_binary_path: None }] {
            assert_eq!(check(&PeerClass::TokenAuthenticated, &request, &grants), Ok(()));
        }
    }

    #[test]
    fn invalid_grants_are_refused() {
        let grants = PaneGrants::default();
        assert!(grants.add(grant("", "b", &[GrantOp::Write])).is_err());
        assert!(grants.add(grant("a", "b", &[])).is_err());
        assert!(grants.add(grant("*", "b", &[GrantOp::Write])).is_err());
        assert!(grants.add(grant("a", "b", &[GrantOp::Admin])).is_err(), "admin is on * only");
        assert!(grants.add(grant("a", "b\u{7}", &[GrantOp::Write])).is_err());
        assert!(grants.list().is_empty());
    }

    /// Every ferryx-cli verb an in-pane shell uses today, with its expected outcome from pane
    /// `a` (own session `a`, another pane `b`). Verbs that never reach the daemon (`open`, `api`)
    /// and the supervisor verbs (gated by `supervisor_peer` itself) are not mutations here.
    #[test]
    fn in_pane_cli_verbs_regression_table() {
        let none = PaneGrants::default();
        let admin = PaneGrants::default();
        admin.add(grant("a", ANY_TARGET, &[GrantOp::Admin])).unwrap();
        let send = |to: &str| agents(AgentControlOp::Send { session_id: to.into(), text: "echo hi".into(), request_id: None });
        let configure = DaemonRequest::RemoteConfigure { config: Default::default() };
        let upgrade = DaemonRequest::UpgradeBinary { new_binary_path: None };
        let pairing = DaemonRequest::RemoteCreatePairingCode { permission: None };
        let table: Vec<(&str, DaemonRequest, Result<(), &str>, Result<(), &str>)> = vec![
            ("agents list", agents(AgentControlOp::List), Ok(()), Ok(())),
            ("agents read b", agents(AgentControlOp::Read { session_id: "b".into() }), Ok(()), Ok(())),
            ("agents events", agents(AgentControlOp::Events { after: 0 }), Ok(()), Ok(())),
            ("agents send a", send("a"), Ok(()), Ok(())),
            ("agents send b", send("b"), Err(PANE_GRANT_REQUIRED), Ok(())),
            ("agents stop b", agents(AgentControlOp::Stop { session_id: "b".into(), request_id: None }), Err(PANE_GRANT_REQUIRED), Ok(())),
            ("supervisor snapshot", DaemonRequest::SupervisorSnapshot, Ok(()), Ok(())),
            ("grants list", DaemonRequest::PaneGrant { request: PaneGrantAction::List }, Ok(()), Ok(())),
            ("up (remote configure)", configure, Err(OWNER_ACTION_REQUIRED), Ok(())),
            ("remote pair generate", pairing, Err(OWNER_ACTION_REQUIRED), Ok(())),
            ("upgradeBinary", upgrade, Err(OWNER_ACTION_REQUIRED), Ok(())),
            ("shutdown", DaemonRequest::Shutdown, Err(OWNER_ACTION_REQUIRED), Ok(())),
        ];
        for (verb, request, without, with_admin) in table {
            assert_eq!(check(&pane("a"), &request, &none), without, "{verb} without grants");
            assert_eq!(check(&pane("a"), &request, &admin), with_admin, "{verb} with admin");
        }
        assert!(!none.admin_allowed(&pane("a"), workspaces));
        assert!(admin.admin_allowed(&pane("a"), workspaces));
        assert!(none.admin_allowed(&PeerClass::Owner, workspaces));
    }

    #[test]
    fn grants_persist_atomically_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PANE_GRANTS_FILE);
        let grants = PaneGrants::load(Some(path.clone()));
        grants.add(grant("a", "b", &[GrantOp::Write])).unwrap();
        grants.add(grant("a", "b", &[GrantOp::Control])).unwrap();
        let reloaded = PaneGrants::load(Some(path.clone()));
        assert_eq!(reloaded.list(), vec![grant("a", "b", &[GrantOp::Write, GrantOp::Control])]);
        reloaded.remove("a", "b").unwrap();
        assert!(PaneGrants::load(Some(path.clone())).list().is_empty());
        // No temporary file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten()
            .filter(|entry| entry.file_name() != PANE_GRANTS_FILE)
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // A corrupt file fails closed: no grants.
        std::fs::write(&path, b"{not json").unwrap();
        assert!(PaneGrants::load(Some(path)).list().is_empty());
    }

    /// 256 panes x 10 grants each, every op, keeps pane-grants.json under 1 MiB.
    #[test]
    fn size_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PANE_GRANTS_FILE);
        let grants = PaneGrants::load(Some(path.clone()));
        let mut all = Vec::new();
        for pane in 0..256 {
            for target in 0..10 {
                all.push(grant(
                    &format!("00000000-0000-4000-8000-{pane:012}"),
                    &format!("00000000-0000-4000-9000-{target:012}"),
                    &[GrantOp::Write, GrantOp::Control, GrantOp::Spawn],
                ));
            }
        }
        grants.replace(all).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        assert!(size < 1024 * 1024, "pane-grants.json is {size} bytes");
        assert_eq!(PaneGrants::load(Some(path)).list().len(), 2560);
    }
}
