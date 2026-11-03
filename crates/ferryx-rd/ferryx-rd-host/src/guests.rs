//! Local host controls. The standalone daemon has no host-control IPC server;
//! listing/kicking sessions and driving consent are library API-only operations.
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use ferryx_rd_proto::{
    GuestPermissions, HostStatus, InputEventType, PairingTrust, SecureAttentionResult,
    SessionEndReason,
};

#[derive(Debug, Clone)]
pub struct GuestSession {
    pub id: String,
    pub pairing_id: String,
    pub name: String,
    pub peer: SocketAddr,
    pub trust: PairingTrust,
    pub connected_at: Instant,
}

#[derive(Debug, Clone)]
pub enum HostEvent {
    GuestConnected(GuestSession),
    GuestDisconnected(GuestSession),
    ConsentRequested(PendingRequest),
}

#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub id: String,
    pub name: String,
    pub expires_at: Instant,
}

pub trait SecureAttentionHandler: Send + Sync {
    /// Called only for a locally authorized owner, never ordinary key injection.
    fn request(&self, guest: &GuestSession) -> SecureAttentionResult;
}

pub(crate) enum GuestCommand {
    End(SessionEndReason),
    Status(HostStatus),
    Permissions(GuestPermissions),
}

#[derive(Default)]
pub struct GuestRegistry {
    state: Mutex<RegistryState>,
    store: Option<crate::session::PairingStore>,
}

#[derive(Default)]
struct RegistryState {
    sessions: BTreeMap<String, (GuestSession, mpsc::Sender<GuestCommand>)>,
    events: Vec<mpsc::Sender<HostEvent>>,
    pending: BTreeMap<String, (PendingRequest, mpsc::SyncSender<Option<PairingTrust>>)>,
    sas: Option<Arc<dyn SecureAttentionHandler>>,
}

impl GuestRegistry {
    pub(crate) fn with_store(store: crate::session::PairingStore) -> Self {
        Self {
            state: Mutex::new(RegistryState::default()),
            store: Some(store),
        }
    }

    pub fn set_guest_permissions(
        &self,
        pairing_id: &str,
        permissions: GuestPermissions,
    ) -> Result<(), crate::session::SessionError> {
        if let Some(store) = &self.store {
            if store.load(pairing_id)?.is_some() {
                let (trust, _) = store.authorization(pairing_id)?;
                store.set_authorization(pairing_id, trust, Some(permissions))?;
            }
        }
        self.permissions(pairing_id, permissions);
        Ok(())
    }

    pub fn sessions(&self) -> Vec<GuestSession> {
        self.state
            .lock()
            .expect("guest registry poisoned")
            .sessions
            .values()
            .map(|(g, _)| g.clone())
            .collect()
    }

    pub fn subscribe(&self) -> mpsc::Receiver<HostEvent> {
        let (tx, rx) = mpsc::channel();
        self.state
            .lock()
            .expect("guest registry poisoned")
            .events
            .push(tx);
        rx
    }

    pub fn kick(&self, id: &str) -> bool {
        self.state
            .lock()
            .expect("guest registry poisoned")
            .sessions
            .get(id)
            .is_some_and(|(_, tx)| tx.send(GuestCommand::End(SessionEndReason::Kicked)).is_ok())
    }

    pub fn kick_all(&self) -> usize {
        self.state
            .lock()
            .expect("guest registry poisoned")
            .sessions
            .values()
            .filter(|(_, tx)| tx.send(GuestCommand::End(SessionEndReason::Kicked)).is_ok())
            .count()
    }

    pub fn pending_requests(&self) -> Vec<PendingRequest> {
        self.state
            .lock()
            .expect("guest registry poisoned")
            .pending
            .values()
            .filter(|(p, _)| Instant::now() < p.expires_at)
            .map(|(p, _)| p.clone())
            .collect()
    }

    pub fn respond(&self, id: &str, trust: Option<PairingTrust>) -> bool {
        let mut state = self.state.lock().expect("guest registry poisoned");
        let Some((request, tx)) = state.pending.remove(id) else {
            return false;
        };
        Instant::now() < request.expires_at && tx.try_send(trust).is_ok()
    }

    pub fn set_secure_attention_handler(&self, handler: Arc<dyn SecureAttentionHandler>) {
        self.state.lock().expect("guest registry poisoned").sas = Some(handler);
    }

    pub(crate) fn secure_attention(&self, guest: &GuestSession) -> SecureAttentionResult {
        if !cfg!(target_os = "windows") {
            return SecureAttentionResult::Unsupported;
        }
        if guest.trust != PairingTrust::Owner {
            return SecureAttentionResult::PermissionDenied;
        }
        let handler = self
            .state
            .lock()
            .expect("guest registry poisoned")
            .sas
            .clone();
        handler.map_or(SecureAttentionResult::Unsupported, |h| h.request(guest))
    }

    pub(crate) fn permissions(&self, pairing_id: &str, permissions: GuestPermissions) {
        for (guest, tx) in self
            .state
            .lock()
            .expect("guest registry poisoned")
            .sessions
            .values()
        {
            if guest.pairing_id == pairing_id {
                let _ = tx.send(GuestCommand::Permissions(permissions));
            }
        }
    }

    pub(crate) fn request(
        &self,
        request: PendingRequest,
        tx: mpsc::SyncSender<Option<PairingTrust>>,
    ) {
        let mut state = self.state.lock().expect("guest registry poisoned");
        state
            .pending
            .insert(request.id.clone(), (request.clone(), tx));
        state.events.retain(|tx| {
            tx.send(HostEvent::ConsentRequested(request.clone()))
                .is_ok()
        });
    }

    pub(crate) fn finish_request(&self, id: &str) {
        self.state
            .lock()
            .expect("guest registry poisoned")
            .pending
            .remove(id);
    }

    pub(crate) fn join(
        self: &Arc<Self>,
        guest: GuestSession,
    ) -> (GuestGuard, mpsc::Receiver<GuestCommand>) {
        let (tx, rx) = mpsc::channel();
        let mut state = self.state.lock().expect("guest registry poisoned");
        state.sessions.insert(guest.id.clone(), (guest.clone(), tx));
        state
            .events
            .retain(|tx| tx.send(HostEvent::GuestConnected(guest.clone())).is_ok());
        publish_status(&state);
        crate::host_log(&format!("guest connected: {} ({})", guest.name, guest.id));
        (
            GuestGuard {
                registry: self.clone(),
                id: guest.id,
            },
            rx,
        )
    }
}

fn publish_status(state: &RegistryState) {
    let status = HostStatus {
        guest_names: state
            .sessions
            .values()
            .map(|(g, _)| g.name.clone())
            .collect(),
    };
    for (_, tx) in state.sessions.values() {
        let _ = tx.send(GuestCommand::Status(status.clone()));
    }
}

pub(crate) struct GuestGuard {
    registry: Arc<GuestRegistry>,
    id: String,
}

impl Drop for GuestGuard {
    fn drop(&mut self) {
        let mut state = self.registry.state.lock().expect("guest registry poisoned");
        if let Some((guest, _)) = state.sessions.remove(&self.id) {
            crate::host_log(&format!(
                "guest disconnected: {} ({})",
                guest.name, guest.id
            ));
            state
                .events
                .retain(|tx| tx.send(HostEvent::GuestDisconnected(guest.clone())).is_ok());
            publish_status(&state);
        }
    }
}

pub(crate) fn intersect(a: GuestPermissions, b: GuestPermissions) -> GuestPermissions {
    GuestPermissions {
        keyboard: a.keyboard && b.keyboard,
        mouse: a.mouse && b.mouse,
        gamepad: a.gamepad && b.gamepad,
        clipboard: a.clipboard && b.clipboard,
        audio: a.audio && b.audio,
    }
}

pub(crate) fn allows(p: GuestPermissions, event: InputEventType) -> bool {
    use InputEventType::*;
    match event {
        KeyDown | KeyUp | FlagsChanged | UnicodeChar => p.keyboard,
        GamepadAxis | GamepadButtonDown | GamepadButtonUp => p.gamepad,
        Reset => true,
        _ => p.mouse,
    }
}

pub(crate) fn valid_trust(trust: PairingTrust, now: u64) -> bool {
    match trust {
        PairingTrust::Owner => true,
        PairingTrust::GuestOnce => false,
        PairingTrust::GuestExpiring { expires_at_unix_ms } => now < expires_at_unix_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guest(id: &str) -> GuestSession {
        GuestSession {
            id: id.into(),
            pairing_id: id.into(),
            name: "same name".into(),
            peer: "127.0.0.1:1234".parse().unwrap(),
            trust: PairingTrust::Owner,
            connected_at: Instant::now(),
        }
    }

    #[test]
    fn targeted_kick_keeps_other_session_registered() {
        let registry = Arc::new(GuestRegistry::default());
        let (_a, a) = registry.join(guest("a"));
        let (_b, b) = registry.join(guest("b"));
        while a.try_recv().is_ok() {}
        while b.try_recv().is_ok() {}
        assert!(registry.kick("a"));
        assert!(matches!(
            a.try_recv(),
            Ok(GuestCommand::End(SessionEndReason::Kicked))
        ));
        assert!(b.try_recv().is_err());
        assert_eq!(registry.sessions().len(), 2);
    }

    #[test]
    fn kick_all_targets_each_session() {
        let registry = Arc::new(GuestRegistry::default());
        let (_a, a) = registry.join(guest("a"));
        let (_b, b) = registry.join(guest("b"));
        while a.try_recv().is_ok() {}
        while b.try_recv().is_ok() {}
        assert_eq!(registry.kick_all(), 2);
        assert!(matches!(
            a.try_recv(),
            Ok(GuestCommand::End(SessionEndReason::Kicked))
        ));
        assert!(matches!(
            b.try_recv(),
            Ok(GuestCommand::End(SessionEndReason::Kicked))
        ));
    }

    #[test]
    fn join_leave_publishes_complete_occupancy_and_named_events() {
        let registry = Arc::new(GuestRegistry::default());
        let events = registry.subscribe();
        let (_a, a) = registry.join(guest("a"));
        let (b, _) = registry.join(guest("b"));
        assert!(matches!(a.try_recv(), Ok(GuestCommand::Status(s)) if s.active_guest_count() == 1));
        assert!(
            matches!(a.try_recv(), Ok(GuestCommand::Status(s)) if s.guest_names == vec!["same name", "same name"])
        );
        drop(b);
        assert!(matches!(a.try_recv(), Ok(GuestCommand::Status(s)) if s.active_guest_count() == 1));
        assert!(matches!(events.try_recv(), Ok(HostEvent::GuestConnected(g)) if g.id == "a"));
        assert!(matches!(events.try_recv(), Ok(HostEvent::GuestConnected(g)) if g.id == "b"));
        assert!(
            matches!(events.try_recv(), Ok(HostEvent::GuestDisconnected(g)) if g.name == "same name")
        );
    }

    #[test]
    fn expired_consent_cannot_be_approved() {
        let registry = GuestRegistry::default();
        let (tx, rx) = mpsc::sync_channel(1);
        registry.request(
            PendingRequest {
                id: "r".into(),
                name: "guest".into(),
                expires_at: Instant::now(),
            },
            tx,
        );
        assert!(!registry.respond("r", Some(PairingTrust::Owner)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn consent_delivers_explicit_host_trust() {
        let registry = GuestRegistry::default();
        let (tx, rx) = mpsc::sync_channel(1);
        registry.request(
            PendingRequest {
                id: "r".into(),
                name: "guest".into(),
                expires_at: Instant::now() + std::time::Duration::from_secs(60),
            },
            tx,
        );
        assert!(registry.respond("r", Some(PairingTrust::GuestOnce)));
        assert_eq!(rx.try_recv().unwrap(), Some(PairingTrust::GuestOnce));
        assert!(registry.pending_requests().is_empty());
    }

    #[test]
    fn permissions_cannot_exceed_ceiling_and_gate_categories() {
        let grant = GuestPermissions {
            keyboard: true,
            mouse: true,
            gamepad: true,
            clipboard: true,
            audio: true,
        };
        let ceiling = GuestPermissions {
            keyboard: false,
            mouse: true,
            gamepad: false,
            clipboard: false,
            audio: false,
        };
        let effective = intersect(grant, ceiling);
        assert_eq!(effective, ceiling);
        assert!(!allows(effective, InputEventType::KeyDown));
        assert!(allows(effective, InputEventType::MouseMove));
        assert!(!allows(effective, InputEventType::GamepadButtonDown));
        assert!(allows(effective, InputEventType::Reset));
    }

    #[test]
    fn guest_once_and_expired_guests_never_reconnect() {
        assert!(valid_trust(PairingTrust::Owner, u64::MAX));
        assert!(!valid_trust(PairingTrust::GuestOnce, 0));
        assert!(valid_trust(
            PairingTrust::GuestExpiring {
                expires_at_unix_ms: 10
            },
            9
        ));
        assert!(!valid_trust(
            PairingTrust::GuestExpiring {
                expires_at_unix_ms: 10
            },
            10
        ));
    }

    #[test]
    fn secure_attention_requires_owner_and_native_broker() {
        let registry = GuestRegistry::default();
        let mut guest = guest("a");
        guest.trust = PairingTrust::GuestOnce;
        assert_eq!(
            registry.secure_attention(&guest),
            if cfg!(target_os = "windows") {
                SecureAttentionResult::PermissionDenied
            } else {
                SecureAttentionResult::Unsupported
            }
        );
        guest.trust = PairingTrust::Owner;
        assert_eq!(
            registry.secure_attention(&guest),
            SecureAttentionResult::Unsupported
        );
    }
}
