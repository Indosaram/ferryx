//! Controller registry, origin grants, one-shot capabilities and tab leases (plan C1-C3).
//!
//! The state is pure: callers pass the current [`Instant`], so expiry is tested with an
//! injected clock. Secrets are never stored, only their SHA-256 digest, compared in constant
//! time. Persistence (`PersistedRegistry`) holds paired identities and grants the user chose
//! to keep; sessions, leases, pending approvals and one-shot grants live only in memory, so a
//! GUI restart drops them.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::catalog::{GrantScope, ToolId};
use super::policy::{Authorization, Obligation};
use super::protocol::{
    ApprovalId, ApprovalKind, ApprovalResolution, AuthorityError, ControllerDirectory,
    ControllerId, ControllerStanding, Principal, SessionEpoch,
    MAX_PENDING_APPROVALS_PER_CONTROLLER, PAIRING_APPROVAL_TTL,
};

/// A local tab lease lasts this long without a renewing action.
pub const TAB_LEASE_TTL: Duration = Duration::from_secs(15);

/// An exact web origin: scheme, host and effective port. No wildcards, no suffix matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Origin(String);

impl Origin {
    /// Parses the origin of an http(s) URL. Opaque origins (data:, file:, blob:, about:) and
    /// credential-bearing URLs are refused: agents get no authority over them.
    pub fn parse(raw: &str) -> Result<Self, AuthorityError> {
        let refused = AuthorityError::Forbidden {
            tool: None,
            reason: "agents can only be granted exact http(s) origins without credentials",
        };
        let url = url::Url::parse(raw.trim()).map_err(|_| refused.clone())?;
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            return Err(refused);
        }
        let host = url.host_str().ok_or_else(|| refused.clone())?;
        let port = url.port_or_known_default().ok_or(refused)?;
        Ok(Self(format!("{}://{}:{port}", url.scheme(), host.to_ascii_lowercase())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `about:blank` carries no page data: bookkeeping on it needs no origin grant.
fn is_blank(url: &str) -> bool {
    url.trim().eq_ignore_ascii_case("about:blank")
}

fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn digests_match(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter().zip(right.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// Minimum accepted secret length (hex of 32 random bytes is 64).
pub const MIN_SECRET_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControllerRecord {
    label: String,
    digest: [u8; 32],
    epoch: u64,
    revoked: bool,
}

/// What a controller may do on one origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginGrantEntry {
    pub origin: Origin,
    pub scopes: Vec<GrantScope>,
    pub persistent: bool,
}

#[derive(Debug, Clone)]
struct Pending {
    kind: ApprovalKind,
    /// Digest the pairing will store when approved.
    digest: Option<[u8; 32]>,
    expires: Instant,
    resolution: Option<ApprovalResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lease {
    controller: ControllerId,
    epoch: u64,
    expires: Instant,
}

/// Secret-free view of a pending approval for the app chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingApproval {
    pub id: ApprovalId,
    #[serde(flatten)]
    pub kind: ApprovalKind,
    pub expires_in_ms: u64,
}

/// Secret-free view of a controller for the app chrome and `browser controllers list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerSummary {
    pub id: ControllerId,
    pub label: String,
    pub revoked: bool,
    pub grants: Vec<OriginGrantEntry>,
}

/// What persists across GUI restarts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRegistry {
    controllers: Vec<(String, ControllerRecord)>,
    grants: Vec<(String, OriginGrantEntry)>,
}

/// Facts about the target tab the obligations are checked against.
#[derive(Debug, Clone, Copy)]
pub struct TabFacts<'a> {
    pub browser_id: &'a str,
    pub url: &'a str,
}

#[derive(Debug, Default)]
pub struct AuthorityState {
    controllers: HashMap<ControllerId, ControllerRecord>,
    grants: HashMap<ControllerId, HashMap<Origin, (HashSet<GrantScope>, bool)>>,
    one_shot: HashMap<ControllerId, HashSet<ToolId>>,
    pending: HashMap<ApprovalId, Pending>,
    leases: HashMap<String, Lease>,
    /// Tabs the desktop user took back; an agent cannot lease them until the user resumes.
    reclaimed: HashSet<String>,
    next_lease_epoch: u64,
}

impl ControllerDirectory for AuthorityState {
    fn standing(&self, id: &ControllerId) -> ControllerStanding {
        match self.controllers.get(id) {
            None => ControllerStanding::Unpaired,
            Some(record) if record.revoked => ControllerStanding::Revoked,
            Some(record) => ControllerStanding::Active { session_epoch: SessionEpoch(record.epoch) },
        }
    }
}

impl AuthorityState {
    pub fn from_persisted(persisted: PersistedRegistry) -> Self {
        let mut state = Self::default();
        for (id, record) in persisted.controllers {
            if let Ok(id) = ControllerId::parse(&id) {
                state.controllers.insert(id, record);
            }
        }
        for (id, entry) in persisted.grants {
            let Ok(id) = ControllerId::parse(&id) else { continue };
            state
                .grants
                .entry(id)
                .or_default()
                .insert(entry.origin, (entry.scopes.into_iter().collect(), true));
        }
        state
    }

    pub fn to_persisted(&self) -> PersistedRegistry {
        let mut controllers: Vec<_> =
            self.controllers.iter().map(|(id, record)| (id.to_string(), record.clone())).collect();
        controllers.sort_by(|a, b| a.0.cmp(&b.0));
        let mut grants: Vec<_> = self
            .grants
            .iter()
            .flat_map(|(id, origins)| {
                origins.iter().filter(|(_, (_, persistent))| *persistent).map(move |(origin, (scopes, _))| {
                    (id.to_string(), OriginGrantEntry { origin: origin.clone(), scopes: sorted(scopes), persistent: true })
                })
            })
            .collect();
        grants.sort_by(|a, b| (&a.0, &a.1.origin).cmp(&(&b.0, &b.1.origin)));
        PersistedRegistry { controllers, grants }
    }

    fn expire(&mut self, now: Instant) {
        for pending in self.pending.values_mut() {
            if pending.resolution.is_none() && now >= pending.expires {
                pending.resolution = Some(ApprovalResolution::Expired);
            }
        }
        self.leases.retain(|_, lease| now < lease.expires);
    }

    fn pending_for(&self, controller: &ControllerId) -> usize {
        self.pending
            .values()
            .filter(|pending| pending.resolution.is_none() && approval_controller(&pending.kind) == controller)
            .count()
    }

    fn open_approval(
        &mut self,
        kind: ApprovalKind,
        digest: Option<[u8; 32]>,
        now: Instant,
    ) -> Result<ApprovalId, AuthorityError> {
        self.expire(now);
        // A repeated request coalesces with the one still waiting.
        if let Some((id, _)) = self
            .pending
            .iter()
            .find(|(_, pending)| pending.resolution.is_none() && pending.kind == kind && pending.digest == digest)
        {
            return Ok(*id);
        }
        if self.pending_for(approval_controller(&kind)) >= MAX_PENDING_APPROVALS_PER_CONTROLLER {
            return Err(AuthorityError::Forbidden {
                tool: None,
                reason: "too many approval requests are waiting for this controller",
            });
        }
        let id = ApprovalId::generate();
        self.pending
            .insert(id, Pending { kind, digest, expires: now + PAIRING_APPROVAL_TTL, resolution: None });
        Ok(id)
    }

    /// Opens a pairing request. It grants nothing until the desktop user approves it.
    pub fn request_pairing(
        &mut self,
        controller: ControllerId,
        label: &str,
        secret: &str,
        now: Instant,
    ) -> Result<ApprovalId, AuthorityError> {
        if secret.len() < MIN_SECRET_LEN {
            return Err(AuthorityError::Forbidden { tool: None, reason: "controller secret is too short" });
        }
        if self.controllers.get(&controller).is_some_and(|record| !record.revoked) {
            return Err(AuthorityError::Forbidden { tool: None, reason: "this controller id is already paired" });
        }
        let label: String = label.chars().filter(|c| !c.is_control()).take(64).collect();
        let label = if label.trim().is_empty() { "unnamed controller".to_string() } else { label };
        self.open_approval(ApprovalKind::PairController { controller, label }, Some(digest(secret)), now)
    }

    /// Opens an origin grant request for a paired controller.
    pub fn request_origin_grant(
        &mut self,
        controller: ControllerId,
        origin: &str,
        scopes: &[GrantScope],
        now: Instant,
    ) -> Result<ApprovalId, AuthorityError> {
        let origin = Origin::parse(origin)?;
        let mut scopes: Vec<GrantScope> = scopes.to_vec();
        scopes.sort_by_key(|scope| *scope as u8);
        scopes.dedup();
        if scopes.is_empty() {
            return Err(AuthorityError::Forbidden { tool: None, reason: "a grant needs at least one scope" });
        }
        self.open_approval(
            ApprovalKind::OriginGrant { controller, origin: origin.0, scopes },
            None,
            now,
        )
    }

    /// Opens a one-shot file or native capability request (screenshot, state export, focus).
    pub fn request_capability(
        &mut self,
        controller: ControllerId,
        tool: ToolId,
        now: Instant,
    ) -> Result<ApprovalId, AuthorityError> {
        let kind = match tool.spec().scope {
            super::catalog::Scope::File => ApprovalKind::FileCapability { controller, tool },
            super::catalog::Scope::Native => ApprovalKind::NativeCapability { controller, tool },
            _ => {
                return Err(AuthorityError::Forbidden {
                    tool: Some(tool),
                    reason: "only file and native tools need a capability approval",
                })
            }
        };
        self.open_approval(kind, None, now)
    }

    /// Resolves a pending approval. Only app-chrome IPC calls this. An expired or already
    /// resolved request cannot be approved.
    pub fn resolve(
        &mut self,
        approval: ApprovalId,
        resolution: ApprovalResolution,
        now: Instant,
    ) -> Result<ApprovalKind, AuthorityError> {
        self.expire(now);
        let not_open = AuthorityError::Forbidden { tool: None, reason: "this approval request is no longer open" };
        let pending = self.pending.get_mut(&approval).ok_or_else(|| not_open.clone())?;
        if pending.resolution.is_some() || resolution == ApprovalResolution::Expired {
            return Err(not_open);
        }
        pending.resolution = Some(resolution);
        let (kind, secret_digest) = (pending.kind.clone(), pending.digest);
        if resolution == ApprovalResolution::Deny {
            return Ok(kind);
        }
        let persistent = resolution == ApprovalResolution::ApprovePersistent;
        match &kind {
            ApprovalKind::PairController { controller, label } => {
                let epoch = self.controllers.get(controller).map_or(1, |record| record.epoch + 1);
                self.controllers.insert(
                    *controller,
                    ControllerRecord {
                        label: label.clone(),
                        digest: secret_digest.ok_or(not_open)?,
                        epoch,
                        revoked: false,
                    },
                );
            }
            ApprovalKind::OriginGrant { controller, origin, scopes } => {
                let entry = self
                    .grants
                    .entry(*controller)
                    .or_default()
                    .entry(Origin(origin.clone()))
                    .or_insert_with(|| (HashSet::new(), persistent));
                entry.0.extend(scopes.iter().copied());
                entry.1 |= persistent;
            }
            ApprovalKind::FileCapability { controller, tool } | ApprovalKind::NativeCapability { controller, tool } => {
                self.one_shot.entry(*controller).or_default().insert(*tool);
            }
        }
        Ok(kind)
    }

    /// The outcome of an approval once it is resolved (or expired), else `None`.
    pub fn resolution(&mut self, approval: ApprovalId, now: Instant) -> Option<ApprovalResolution> {
        self.expire(now);
        self.pending.get(&approval).and_then(|pending| pending.resolution)
    }

    /// Forgets a resolved approval once its requester has read the outcome.
    pub fn forget_approval(&mut self, approval: ApprovalId) {
        self.pending.remove(&approval);
    }

    pub fn pending_approvals(&mut self, now: Instant) -> Vec<PendingApproval> {
        self.expire(now);
        let mut list: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.resolution.is_none())
            .map(|(id, pending)| PendingApproval {
                id: *id,
                kind: pending.kind.clone(),
                expires_in_ms: pending.expires.saturating_duration_since(now).as_millis() as u64,
            })
            .collect();
        list.sort_by_key(|approval| approval.expires_in_ms);
        list
    }

    /// Checks a presented credential and returns the host-constructed principal.
    pub fn authenticate(&self, controller: &str, secret: &str) -> Result<Principal, AuthorityError> {
        let id = ControllerId::parse(controller)?;
        let record = self.controllers.get(&id).ok_or(AuthorityError::PairingRequired)?;
        if !digests_match(&record.digest, &digest(secret)) {
            return Err(AuthorityError::PairingRequired);
        }
        if record.revoked {
            return Err(AuthorityError::Revoked { controller: id });
        }
        Ok(Principal::LocalController { id, session_epoch: SessionEpoch(record.epoch) })
    }

    /// Revokes a controller: its credential, sessions, leases, grants, one-shot capabilities
    /// and pending approvals end at once.
    pub fn revoke(&mut self, controller: &ControllerId) -> bool {
        let Some(record) = self.controllers.get_mut(controller) else {
            return false;
        };
        record.revoked = true;
        record.epoch += 1;
        self.grants.remove(controller);
        self.one_shot.remove(controller);
        self.leases.retain(|_, lease| lease.controller != *controller);
        for pending in self.pending.values_mut() {
            if pending.resolution.is_none() && approval_controller(&pending.kind) == controller {
                pending.resolution = Some(ApprovalResolution::Deny);
            }
        }
        true
    }

    /// Withdraws one origin grant from a controller.
    pub fn revoke_grant(&mut self, controller: &ControllerId, origin: &str) -> bool {
        let Ok(origin) = Origin::parse(origin) else { return false };
        self.grants.get_mut(controller).is_some_and(|origins| origins.remove(&origin).is_some())
    }

    pub fn has_grant(&self, controller: &ControllerId, url: &str, scope: GrantScope) -> bool {
        let Ok(origin) = Origin::parse(url) else { return false };
        self.grants
            .get(controller)
            .and_then(|origins| origins.get(&origin))
            .is_some_and(|(scopes, _)| scopes.contains(&scope))
    }

    pub fn controllers(&self) -> Vec<ControllerSummary> {
        let mut list: Vec<_> = self
            .controllers
            .iter()
            .map(|(id, record)| ControllerSummary {
                id: *id,
                label: record.label.clone(),
                revoked: record.revoked,
                grants: self
                    .grants
                    .get(id)
                    .map(|origins| {
                        let mut entries: Vec<_> = origins
                            .iter()
                            .map(|(origin, (scopes, persistent))| OriginGrantEntry {
                                origin: origin.clone(),
                                scopes: sorted(scopes),
                                persistent: *persistent,
                            })
                            .collect();
                        entries.sort_by(|a, b| a.origin.cmp(&b.origin));
                        entries
                    })
                    .unwrap_or_default(),
            })
            .collect();
        list.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.as_uuid().cmp(b.id.as_uuid())));
        list
    }

    /// The desktop user takes a tab back: the agent lease ends and stays off until resume.
    pub fn reclaim(&mut self, browser_id: &str) {
        self.leases.remove(browser_id);
        self.reclaimed.insert(browser_id.to_string());
    }

    /// The desktop user hands a reclaimed tab back to agents.
    pub fn resume(&mut self, browser_id: &str) {
        self.reclaimed.remove(browser_id);
    }

    pub fn release_tab(&mut self, browser_id: &str) {
        self.leases.remove(browser_id);
        self.reclaimed.remove(browser_id);
    }

    fn take_lease(&mut self, controller: ControllerId, browser_id: &str, now: Instant) -> Result<(), AuthorityError> {
        if self.reclaimed.contains(browser_id) {
            return Err(AuthorityError::Forbidden {
                tool: None,
                reason: "the desktop user took control of this tab; it returns to agents only when the user resumes it",
            });
        }
        match self.leases.get_mut(browser_id) {
            Some(lease) if lease.controller != controller => Err(AuthorityError::Forbidden {
                tool: None,
                reason: "another controller holds this tab",
            }),
            Some(lease) => {
                lease.expires = now + TAB_LEASE_TTL;
                Ok(())
            }
            None => {
                self.next_lease_epoch += 1;
                self.leases.insert(
                    browser_id.to_string(),
                    Lease { controller, epoch: self.next_lease_epoch, expires: now + TAB_LEASE_TTL },
                );
                Ok(())
            }
        }
    }

    /// Lease epoch of the current holder of a tab, for a recheck just before a side effect.
    pub fn lease_epoch(&self, browser_id: &str) -> Option<u64> {
        self.leases.get(browser_id).map(|lease| lease.epoch)
    }

    /// Enforces the obligations of an allowed request for a local controller. Desktop and
    /// remote principals are enforced by their own paths (implicit reclaim, driver broker).
    pub fn enforce(
        &mut self,
        principal: &Principal,
        authorization: &Authorization,
        tab: Option<TabFacts<'_>>,
        now: Instant,
    ) -> Result<(), AuthorityError> {
        self.expire(now);
        let Principal::LocalController { id, session_epoch } = principal else {
            if let (Principal::DesktopUser, Some(tab)) = (principal, tab) {
                if authorization.obligations.contains(&Obligation::ReclaimTab) {
                    self.reclaim(tab.browser_id);
                }
            }
            return Ok(());
        };
        match self.standing(id) {
            ControllerStanding::Active { session_epoch: current } if current == *session_epoch => {}
            ControllerStanding::Active { session_epoch: current } => {
                return Err(AuthorityError::StaleSession { presented: *session_epoch, current })
            }
            ControllerStanding::Revoked => return Err(AuthorityError::Revoked { controller: *id }),
            ControllerStanding::Unpaired => return Err(AuthorityError::PairingRequired),
        }
        let tool = authorization.tool;
        let missing = |reason: &'static str| AuthorityError::Forbidden { tool: Some(tool), reason };
        for obligation in &authorization.obligations {
            match obligation {
                Obligation::OriginGrant { scope } => {
                    let tab = tab.ok_or_else(|| missing("this request needs a target tab"))?;
                    if is_blank(tab.url) && *scope != GrantScope::Eval {
                        continue;
                    }
                    if !self.has_grant(id, tab.url, *scope) {
                        return Err(missing("no grant for this tab's origin and scope; request one and have the desktop user approve it"));
                    }
                }
                Obligation::DestinationOriginGrant { url } => {
                    if !is_blank(url) && !self.has_grant(id, url, GrantScope::Interact) {
                        return Err(missing("no interact grant for the destination origin"));
                    }
                }
                Obligation::TabLease => {
                    let tab = tab.ok_or_else(|| missing("this request needs a target tab"))?;
                    self.take_lease(*id, tab.browser_id, now)?;
                }
                Obligation::FileCapability | Obligation::NativeCapability => {
                    let granted = self.one_shot.get_mut(id).is_some_and(|tools| tools.remove(&tool));
                    if !granted {
                        return Err(missing("this tool needs a one-time approval from the desktop user"));
                    }
                }
                // Listing filters and the remaining kinds are applied by the dispatcher.
                Obligation::FilterToGranted
                | Obligation::DriverLease
                | Obligation::ReclaimTab
                | Obligation::NonBrowserPolicy => {}
            }
        }
        Ok(())
    }

    /// Whether a controller may see a tab in listings (granted origin, blank, or its lease).
    pub fn may_list(&self, controller: &ControllerId, browser_id: &str, url: &str) -> bool {
        is_blank(url)
            || self.has_grant(controller, url, GrantScope::Read)
            || self.leases.get(browser_id).is_some_and(|lease| lease.controller == *controller)
    }
}

fn sorted(scopes: &HashSet<GrantScope>) -> Vec<GrantScope> {
    let mut list: Vec<_> = scopes.iter().copied().collect();
    list.sort_by_key(|scope| *scope as u8);
    list
}

fn approval_controller(kind: &ApprovalKind) -> &ControllerId {
    match kind {
        ApprovalKind::PairController { controller, .. }
        | ApprovalKind::OriginGrant { controller, .. }
        | ApprovalKind::FileCapability { controller, .. }
        | ApprovalKind::NativeCapability { controller, .. } => controller,
    }
}

#[cfg(test)]
mod tests {
    use super::super::catalog::Platform;
    use super::super::policy::{authorize_cli, Caller, PolicyContext};
    use super::super::protocol::{BROWSER_CONTROLLER_PAIRING_REQUIRED, BROWSER_CONTROLLER_REVOKED, BROWSER_FORBIDDEN};
    use super::*;
    use crate::browser::model::{BrowserAutomationAction, BrowserAutomationRequest};
    use crate::ipc::browser_cli::BrowserCliRequest;

    const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";
    const ORIGIN: &str = "http://127.0.0.1:18761";

    fn paired(state: &mut AuthorityState, now: Instant) -> (ControllerId, Principal) {
        let id = ControllerId::generate();
        let approval = state.request_pairing(id, "qa-a", SECRET, now).unwrap();
        state.resolve(approval, ApprovalResolution::ApproveOnce, now).unwrap();
        let principal = state.authenticate(&id.to_string(), SECRET).unwrap();
        (id, principal)
    }

    fn grant(state: &mut AuthorityState, id: ControllerId, scopes: &[GrantScope], now: Instant) {
        let approval = state.request_origin_grant(id, ORIGIN, scopes, now).unwrap();
        state.resolve(approval, ApprovalResolution::ApproveOnce, now).unwrap();
    }

    fn click(browser_id: &str) -> BrowserCliRequest {
        BrowserCliRequest::Act {
            request: BrowserAutomationRequest {
                postcondition: None,
                browser_id: browser_id.into(),
                generation: 1,
                action: BrowserAutomationAction::Click { reference: "e1".into() },
            },
        }
    }

    fn act(state: &mut AuthorityState, principal: &Principal, url: &str, now: Instant) -> Result<(), AuthorityError> {
        let request = click("b1");
        let authorization = {
            let ctx = PolicyContext { controllers: &*state, platform: Platform::MacOs };
            authorize_cli(&Caller::Principal(principal.clone()), &request, &ctx)?
        };
        state.enforce(principal, &authorization, Some(TabFacts { browser_id: "b1", url }), now)
    }

    #[test]
    fn origins_are_exact_and_opaque_or_credentialed_urls_are_refused() {
        assert_eq!(Origin::parse("https://A.test/x?y").unwrap().as_str(), "https://a.test:443");
        assert_eq!(Origin::parse("http://127.0.0.1:18761/form").unwrap().as_str(), ORIGIN);
        assert_ne!(Origin::parse("http://127.0.0.1:18762/").unwrap().as_str(), ORIGIN);
        for raw in ["data:text/html,hi", "file:///etc/passwd", "about:blank", "https://u:p@a.test/", "*.a.test"] {
            assert!(Origin::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn pairing_grants_nothing_until_approved_and_a_wrong_secret_is_unpaired() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let id = ControllerId::generate();
        let approval = state.request_pairing(id, "qa-a", SECRET, now).unwrap();
        assert_eq!(state.authenticate(&id.to_string(), SECRET), Err(AuthorityError::PairingRequired));
        // A duplicate request coalesces.
        assert_eq!(state.request_pairing(id, "qa-a", SECRET, now).unwrap(), approval);
        state.resolve(approval, ApprovalResolution::ApproveOnce, now).unwrap();
        assert!(state.authenticate(&id.to_string(), SECRET).is_ok());
        let wrong = state.authenticate(&id.to_string(), &SECRET.replace('0', "1")).unwrap_err();
        assert_eq!(wrong.code(), BROWSER_CONTROLLER_PAIRING_REQUIRED);
        assert!(state.request_pairing(ControllerId::generate(), "x", "short", now).is_err());
    }

    #[test]
    fn an_expired_or_denied_request_cannot_be_approved() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let id = ControllerId::generate();
        let approval = state.request_pairing(id, "qa-a", SECRET, now).unwrap();
        let later = now + PAIRING_APPROVAL_TTL + Duration::from_millis(1);
        assert_eq!(state.resolution(approval, later), Some(ApprovalResolution::Expired));
        assert!(state.resolve(approval, ApprovalResolution::ApproveOnce, later).is_err());
        assert_eq!(state.authenticate(&id.to_string(), SECRET), Err(AuthorityError::PairingRequired));

        let denied = state.request_pairing(id, "qa-a", SECRET, later).unwrap();
        state.resolve(denied, ApprovalResolution::Deny, later).unwrap();
        assert!(state.resolve(denied, ApprovalResolution::ApproveOnce, later).is_err());
        assert!(state.pending_approvals(later).is_empty());
    }

    #[test]
    fn pending_requests_are_capped_per_controller() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, _) = paired(&mut state, now);
        for port in 0..MAX_PENDING_APPROVALS_PER_CONTROLLER {
            state
                .request_origin_grant(id, &format!("http://127.0.0.1:{}", 20000 + port), &[GrantScope::Read], now)
                .unwrap();
        }
        let error = state.request_origin_grant(id, "http://127.0.0.1:30000", &[GrantScope::Read], now).unwrap_err();
        assert_eq!(error.code(), BROWSER_FORBIDDEN);
    }

    #[test]
    fn act_needs_an_interact_grant_for_the_tabs_exact_origin() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, principal) = paired(&mut state, now);
        let page = "http://127.0.0.1:18761/form";
        assert_eq!(act(&mut state, &principal, page, now).unwrap_err().code(), BROWSER_FORBIDDEN);
        grant(&mut state, id, &[GrantScope::Read], now);
        assert_eq!(act(&mut state, &principal, page, now).unwrap_err().code(), BROWSER_FORBIDDEN);
        grant(&mut state, id, &[GrantScope::Interact], now);
        act(&mut state, &principal, page, now).unwrap();
        // Another port is another origin.
        assert!(act(&mut state, &principal, "http://127.0.0.1:18762/form", now).is_err());
    }

    #[test]
    fn a_revoked_grant_blocks_act() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, principal) = paired(&mut state, now);
        grant(&mut state, id, &[GrantScope::Read, GrantScope::Interact], now);
        let page = "http://127.0.0.1:18761/form";
        act(&mut state, &principal, page, now).unwrap();
        assert!(state.revoke_grant(&id, page));
        let error = act(&mut state, &principal, page, now).unwrap_err();
        assert_eq!(error.code(), BROWSER_FORBIDDEN);
    }

    #[test]
    fn a_revoked_controller_loses_credential_session_lease_and_pending_requests() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, principal) = paired(&mut state, now);
        grant(&mut state, id, &[GrantScope::Interact], now);
        act(&mut state, &principal, "http://127.0.0.1:18761/", now).unwrap();
        assert!(state.lease_epoch("b1").is_some());
        let waiting = state.request_origin_grant(id, "https://b.test", &[GrantScope::Read], now).unwrap();
        assert!(state.revoke(&id));
        assert_eq!(state.lease_epoch("b1"), None);
        assert_eq!(state.resolution(waiting, now), Some(ApprovalResolution::Deny));
        assert_eq!(
            state.authenticate(&id.to_string(), SECRET).unwrap_err().code(),
            BROWSER_CONTROLLER_REVOKED
        );
        // A session authenticated before the revocation is refused too.
        assert_eq!(
            act(&mut state, &principal, "http://127.0.0.1:18761/", now).unwrap_err().code(),
            BROWSER_CONTROLLER_REVOKED
        );
    }

    #[test]
    fn one_controller_holds_a_tab_until_its_lease_expires_or_the_user_reclaims_it() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (a, first) = paired(&mut state, now);
        let (b, second) = paired(&mut state, now);
        grant(&mut state, a, &[GrantScope::Interact], now);
        grant(&mut state, b, &[GrantScope::Interact], now);
        let page = "http://127.0.0.1:18761/";
        act(&mut state, &first, page, now).unwrap();
        assert_eq!(act(&mut state, &second, page, now).unwrap_err().code(), BROWSER_FORBIDDEN);
        let expired = now + TAB_LEASE_TTL + Duration::from_millis(1);
        act(&mut state, &second, page, expired).unwrap();

        state.reclaim("b1");
        assert!(act(&mut state, &second, page, expired).is_err());
        state.resume("b1");
        act(&mut state, &second, page, expired).unwrap();
    }

    #[test]
    fn file_and_native_capabilities_are_one_shot() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, principal) = paired(&mut state, now);
        grant(&mut state, id, &[GrantScope::Read], now);
        let request = BrowserCliRequest::Screenshot { browser_id: "b1".into(), out_path: "/tmp/x.png".into() };
        let authorization = {
            let ctx = PolicyContext { controllers: &state, platform: Platform::MacOs };
            authorize_cli(&Caller::Principal(principal.clone()), &request, &ctx).unwrap()
        };
        let tab = Some(TabFacts { browser_id: "b1", url: ORIGIN });
        assert!(state.enforce(&principal, &authorization, tab, now).is_err());
        let approval = state.request_capability(id, ToolId::Screenshot, now).unwrap();
        state.resolve(approval, ApprovalResolution::ApproveOnce, now).unwrap();
        state.enforce(&principal, &authorization, tab, now).unwrap();
        assert!(state.enforce(&principal, &authorization, tab, now).is_err());
        assert!(state.request_capability(id, ToolId::Act, now).is_err());
    }

    #[test]
    fn only_paired_identities_and_persistent_grants_survive_a_restart() {
        let now = Instant::now();
        let mut state = AuthorityState::default();
        let (id, principal) = paired(&mut state, now);
        grant(&mut state, id, &[GrantScope::Read], now);
        let keep = state.request_origin_grant(id, "https://keep.test", &[GrantScope::Interact], now).unwrap();
        state.resolve(keep, ApprovalResolution::ApprovePersistent, now).unwrap();
        act(&mut state, &principal, "about:blank", now).unwrap();

        let json = serde_json::to_string(&state.to_persisted()).unwrap();
        assert!(!json.contains(SECRET));
        let restored = AuthorityState::from_persisted(serde_json::from_str(&json).unwrap());
        assert!(restored.authenticate(&id.to_string(), SECRET).is_ok());
        assert!(restored.has_grant(&id, "https://keep.test/a", GrantScope::Interact));
        assert!(!restored.has_grant(&id, ORIGIN, GrantScope::Read));
        assert_eq!(restored.lease_epoch("b1"), None);
    }
}
