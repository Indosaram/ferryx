//! Agent reports are hints; only the PTY owner can admit provider metadata.
use crate::daemon::{
    protocol::{AgentStateReport, DaemonRequest, DaemonResponse, TerminalStartup},
    session_service::{DaemonSessionService, ProviderSessionClaimKey},
};
use crate::remote::machine_protocol::{RemoteTerminalTarget, Session};
use std::sync::Arc;

impl DaemonSessionService {
    /// Local authenticated IPC only: no HTTP metadata mutation surface exists.
    pub async fn validate_machine_agent_report(
        self: &Arc<Self>,
        target: RemoteTerminalTarget,
        report: AgentStateReport,
    ) -> Result<Session, String> {
        if report.session_id != target.session_id {
            return Err("SESSION_OWNERSHIP_CHANGED".into());
        }
        if let Some(peer) = self
            .session_router
            .find_legacy_peer_for_session(&target.session_id)
        {
            let response = peer
                .send_request(&DaemonRequest::MachineSessionMetadata {
                    target: target.clone(),
                    report,
                })
                .await
                .map_err(|_| "HOST_UNAVAILABLE")?;
            return match response {
                DaemonResponse::MachineSessionDetailOk {
                    detail: crate::remote::machine_protocol::SessionDetail::Running { session },
                } if session.target == target => Ok(session),
                DaemonResponse::Error { message } => Err(message),
                _ => Err("MACHINE_OWNER_UNSUPPORTED".into()),
            };
        }
        let owner = self.clone();
        crate::ipc::run_blocking(move || {
            owner
                .admit_agent_report(&target, &report)
                .map_err(crate::ipc::IpcError::internal)
        })
        .await
        .map_err(|error| error.to_string())
    }

    fn admit_agent_report(
        &self,
        target: &RemoteTerminalTarget,
        report: &AgentStateReport,
    ) -> Result<Session, String> {
        {
            let metadata = self.session_metadata.read();
            let session = metadata
                .get(&target.session_id)
                .and_then(|meta| meta.machine_session.as_ref())
                .ok_or("SESSION_NOT_FOUND")?;
            if session.target != *target {
                return Err("STALE_EPOCH".into());
            }
        }
        let agent = report.agent.as_deref().ok_or("AGENT_RESUME_UNSUPPORTED")?;
        // Admit only process-bound discovery adapters. Providers whose current
        // adapter falls back to newest-transcript/CWD guessing remain unsupported.
        if !matches!(
            agent,
            "omo" | "claude" | "codex" | "copilot" | "cursor" | "cursor-agent" | "kimi" | "gjc"
        ) {
            return Err("AGENT_RESUME_UNSUPPORTED".into());
        }
        let provider = report
            .provider_session
            .as_ref()
            .ok_or("AGENT_RESUME_INVALID")?;
        if provider.transcript_path.is_some() {
            return Err("AGENT_RESUME_INVALID".into());
        }
        let pty = self
            .terminal_service
            .get_session(&target.session_id)
            .ok_or("SESSION_EXPIRED")?;
        let pid = pty.pid().ok_or("SESSION_EXPIRED")?;
        let discovered = crate::ipc::agents::discover_agent_session_id(pid, agent)
            .ok_or("AGENT_PROVIDER_UNVERIFIED")?;
        if provider.id != discovered {
            return Err("AGENT_PROVIDER_MISMATCH".into());
        }
        crate::terminal::shell::resolve_agent_resume_plan(agent, provider)
            .map_err(|_| "AGENT_RESUME_INVALID")?;
        let startup = TerminalStartup::AgentResume {
            agent_type: agent.into(),
            provider_session: provider.clone(),
        };
        let cwd = crate::ipc::terminal::process_cwd(pid).ok_or("CWD_UNAVAILABLE")?;
        if agent == "omo" {
            let transcript_cwd =
                crate::terminal::resume_cwd::resolve_agent_resume_cwd(Some(&startup))
                    .map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")?
                    .ok_or("AGENT_TRANSCRIPT_UNVERIFIED")?;
            if std::fs::canonicalize(transcript_cwd).map_err(|_| "AGENT_TRANSCRIPT_UNVERIFIED")?
                != cwd
            {
                return Err("AGENT_CWD_MISMATCH".into());
            }
        }
        let claim =
            ProviderSessionClaimKey::from_startup(Some(&startup)).ok_or("AGENT_RESUME_INVALID")?;
        // Revalidate after discovery; use the spawn/catalog lock order.
        let _workspace_gate = self.workspace_service.mutation_gate.lock();
        let mut metadata = self.session_metadata.write();
        let meta = metadata
            .get_mut(&target.session_id)
            .ok_or("SESSION_EXPIRED")?;
        let session = meta.machine_session.as_mut().ok_or("SESSION_NOT_FOUND")?;
        if session.target != *target {
            return Err("STALE_EPOCH".into());
        }
        if !self
            .workspace_service
            .catalog
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .workspaces
            .contains_key(&session.workspace_id)
        {
            return Err("SESSION_OWNERSHIP_CHANGED".into());
        }
        if !matches!(
            pty.state(),
            crate::terminal::PtySessionState::Running | crate::terminal::PtySessionState::Starting
        ) {
            return Err("SESSION_EXPIRED".into());
        }
        let mut claims = self.provider_session_claims.lock();
        if claims
            .get(&claim)
            .is_some_and(|owner| owner != &target.session_id)
        {
            return Err("AGENT_SESSION_CONFLICT".into());
        }
        let mut updated = session.clone();
        updated.agent_type = Some(agent.into());
        updated.provider_session = Some(provider.clone());
        updated.cwd = cwd.to_str().ok_or("INVALID_PATH")?.into();
        if let Some((committed, revision)) = self
            .workspace_service
            .journal
            .update_session_metadata(&updated)?
        {
            if let Some(previous) = &meta.provider_claim {
                claims.remove(previous);
            }
            claims.insert(claim.clone(), target.session_id.clone());
            meta.provider_claim = Some(claim);
            meta.cwd = cwd;
            *session = committed.clone();
            self.workspace_service.machine_events.publish_revision(
                revision.0,
                "sessionMetadataChanged",
                Some(&committed.workspace_id),
                Some(&target.session_id),
                serde_json::json!(committed),
            );
        }
        Ok(session.clone())
    }
}
