//! Trusted binding from a daemon-owned JSON-RPC child, never an HTTP metadata hint.
use super::{agent_state::AgentState, protocol::{AgentProviderSession, AgentProviderSessionKey, AgentStateOrigin, TerminalStartup}, session_service::{DaemonSessionService, ProviderSessionClaimKey}};
use crate::{ferryx_scope::chat::{ChatError, Claim, Claims, Result}, scoped_contracts::{CanonicalProvider, ConversationClaimKey, ConversationOwner, TargetRef}};
use parking_lot::Mutex;
use std::sync::{Arc, atomic::{AtomicU32, Ordering}};

pub(crate) struct ManagedClaims(pub Arc<DaemonSessionService>);
struct ManagedClaim {
    service: Arc<DaemonSessionService>,
    target: TargetRef,
    pid: AtomicU32,
    key: Mutex<Option<ProviderSessionClaimKey>>,
}

impl Claims for ManagedClaims {
    fn acquire(&self, owner: ConversationOwner, resume: Option<ConversationClaimKey>) -> Result<Box<dyn Claim>> {
        // Adopting an arbitrary native conversation is deliberately not a start operation.
        if resume.is_some() { return Err(ChatError::Unsupported); }
        let ConversationOwner::Managed { target } = owner else { return Err(ChatError::Invalid) };
        let pty = self.0.terminal_service.get_session(&target.backend_session_id).ok_or(ChatError::Stale)?;
        if !matches!(pty.state(), crate::terminal::PtySessionState::Running | crate::terminal::PtySessionState::Starting) {
            return Err(ChatError::Stale);
        }
        Ok(Box::new(ManagedClaim { service: self.0.clone(), target, pid: AtomicU32::new(0), key: Mutex::new(None) }))
    }
}

impl Claim for ManagedClaim {
    fn started(&self, pid: u32) -> Result<()> {
        if pid == 0 { return Err(ChatError::Exited); }
        self.pid.store(pid, Ordering::Release);
        Ok(())
    }

    fn bind(&self, key: ConversationClaimKey) -> Result<()> {
        if self.pid.load(Ordering::Acquire) == 0 || key.host_id != self.target.host_id || key.provider != CanonicalProvider::Codex || key.conversation_id.is_empty() {
            return Err(ChatError::Invalid);
        }
        let metadata = self.service.session_metadata.read();
        if !metadata.contains_key(&self.target.backend_session_id) { return Err(ChatError::Stale); }
        let provider = AgentProviderSession { key: AgentProviderSessionKey::SessionId, id: key.conversation_id, transcript_path: None };
        let startup = TerminalStartup::AgentResume { agent_type: "codex".into(), provider_session: provider.clone() };
        let claim = ProviderSessionClaimKey::from_startup(Some(&startup)).ok_or(ChatError::Invalid)?;
        let mut claims = self.service.provider_session_claims.lock();
        if claims.contains_key(&claim) { return Err(ChatError::ProviderOwned); }
        claims.insert(claim.clone(), self.target.backend_session_id.clone());
        *self.key.lock() = Some(claim);
        // The thread id is returned on this child's private stdout after initialize;
        // it is not supplied by the remote caller or guessed from a working directory.
        self.service.agent_states.publish_canonical(AgentState {
            session_id: self.target.backend_session_id.clone(), state: "idle".into(),
            agent: Some("codex".into()), provider_session: Some(provider), detail: None,
            origin: AgentStateOrigin::Agent,
        });
        Ok(())
    }
}

impl Drop for ManagedClaim {
    fn drop(&mut self) {
        if let Some(key) = self.key.lock().take() {
            let mut claims = self.service.provider_session_claims.lock();
            if claims.get(&key) == Some(&self.target.backend_session_id) { claims.remove(&key); }
        }
        // Keep the exact provider id for history after child exit. The supervisor
        // drops this claim only after reaping, while provider/pump removal is fenced.
    }
}
