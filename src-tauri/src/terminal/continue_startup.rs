//! Host-built continuation. The client names a provider session and a mode; it never names argv.
use crate::daemon::protocol::ContinueMode;

pub const AGENT_CONTINUE_UNSUPPORTED: &str = "AGENT_CONTINUE_UNSUPPORTED";
pub const AGENT_CONTINUE_CONFLICT: &str = "AGENT_CONTINUE_CONFLICT";

/// Where the continue request is admitted. Machine spawns only resume agents whose store can
/// prove a cwd; local spawns may use the verified resume builder, including transcript-backed ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContinueSurface {
    Local,
    Machine,
}

/// A pane the owning host already knows holds this provider session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinueSource {
    pub session_id: String,
    pub running: bool,
    pub working: bool,
}

pub fn continue_supported(
    capabilities: &[&str],
    agent_type: &str,
    mode: ContinueMode,
    surface: ContinueSurface,
) -> Result<(), &'static str> {
    if !capabilities.contains(&crate::daemon::protocol::TERMINAL_CONTINUE_CAPABILITY) {
        return Err(AGENT_CONTINUE_UNSUPPORTED);
    }
    let agent = agent_type.trim().to_ascii_lowercase();
    if mode == ContinueMode::Fork && !matches!(agent.as_str(), "claude" | "codex") {
        return Err(AGENT_CONTINUE_UNSUPPORTED);
    }
    match surface {
        ContinueSurface::Machine => {
            if crate::terminal::resume_cwd::machine_spawn_admits_agent(&agent) {
                Ok(())
            } else {
                Err(AGENT_CONTINUE_UNSUPPORTED)
            }
        }
        ContinueSurface::Local => {
            if mode == ContinueMode::Fork || local_resume_builder_supports(&agent) {
                Ok(())
            } else {
                Err(AGENT_CONTINUE_UNSUPPORTED)
            }
        }
    }
}

fn local_resume_builder_supports(agent: &str) -> bool {
    let provider = crate::daemon::protocol::AgentProviderSession {
        key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
        id: "continue-probe".into(),
        transcript_path: Some(if cfg!(windows) {
            r"C:\ferryx\continue-probe.json".into()
        } else {
            "/ferryx/continue-probe.json".into()
        }),
        launcher: None,
    };
    crate::terminal::shell::resolve_agent_resume_plan(agent, &provider).is_ok()
}

/// Resume needs an exited source: no running pane owns the provider session. Fork needs a
/// non-working source. Neither result stops or kills the source pane.
pub fn admit_continue(mode: ContinueMode, sources: &[ContinueSource]) -> Result<(), String> {
    let conflict = match mode {
        ContinueMode::Resume => sources.iter().find(|source| source.running),
        ContinueMode::Fork => sources
            .iter()
            .find(|source| source.running && source.working),
    };
    match conflict {
        Some(source) => Err(source.session_id.clone()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, running: bool, working: bool) -> ContinueSource {
        ContinueSource {
            session_id: id.into(),
            running,
            working,
        }
    }

    #[test]
    fn capability_absence_and_unverified_fork_are_unsupported() {
        assert_eq!(
            continue_supported(&[], "claude", ContinueMode::Resume, ContinueSurface::Local),
            Err(AGENT_CONTINUE_UNSUPPORTED)
        );
        assert_eq!(
            continue_supported(
                &[crate::daemon::protocol::TERMINAL_CONTINUE_CAPABILITY],
                "grok",
                ContinueMode::Fork,
                ContinueSurface::Local
            ),
            Err(AGENT_CONTINUE_UNSUPPORTED)
        );
        assert_eq!(
            continue_supported(
                &[crate::daemon::protocol::TERMINAL_CONTINUE_CAPABILITY],
                "not-a-provider",
                ContinueMode::Resume,
                ContinueSurface::Local
            ),
            Err(AGENT_CONTINUE_UNSUPPORTED)
        );
        assert!(continue_supported(
            &[crate::daemon::protocol::TERMINAL_CONTINUE_CAPABILITY],
            "claude",
            ContinueMode::Fork,
            ContinueSurface::Machine
        )
        .is_ok());
        assert!(continue_supported(
            &[crate::daemon::protocol::TERMINAL_CONTINUE_CAPABILITY],
            "grok",
            ContinueMode::Resume,
            ContinueSurface::Machine
        )
        .is_ok());
    }

    #[test]
    fn live_claim_and_working_source_are_refused_without_stopping_the_source() {
        let live = [source("pane-live", true, false)];
        assert_eq!(
            admit_continue(ContinueMode::Resume, &live),
            Err("pane-live".to_string())
        );
        assert_eq!(admit_continue(ContinueMode::Fork, &live), Ok(()));

        let working = [source("pane-working", true, true)];
        assert_eq!(
            admit_continue(ContinueMode::Fork, &working),
            Err("pane-working".to_string())
        );
        assert_eq!(
            admit_continue(ContinueMode::Resume, &working),
            Err("pane-working".to_string())
        );

        let exited = [source("pane-exited", false, true)];
        assert_eq!(admit_continue(ContinueMode::Resume, &exited), Ok(()));
        assert_eq!(admit_continue(ContinueMode::Fork, &exited), Ok(()));
        assert_eq!(admit_continue(ContinueMode::Resume, &[]), Ok(()));
    }
}
