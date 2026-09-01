//! Installs the Ferryx agent-state extension into agents that expose a lifecycle extension API.
//!
//! Agents that report their own state are authoritative, so this integration replaces screen
//! inference for those sessions. The file is owned by Ferryx: it is rewritten whenever the bundled
//! version changes, and left untouched otherwise so an unchanged install costs no disk writes.

use std::path::{Path, PathBuf};

const EXTENSION_SOURCE: &str =
    include_str!("../../resources/agent-extensions/ferryx-agent-state.ts");
const EXTENSION_FILE_NAME: &str = "ferryx-agent-state.ts";

/// Extension directories of agents that share the same lifecycle extension API.
fn extension_dirs() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    ["\u{2e}omo", ".pi", ".omp"]
        .iter()
        .map(|agent| home.join(agent).join("agent").join("extensions"))
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn install_into(dir: &Path) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    let target = dir.join(EXTENSION_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&target) {
        if existing == EXTENSION_SOURCE {
            return Ok(false);
        }
    }
    let tmp = dir.join(format!(".{EXTENSION_FILE_NAME}.tmp"));
    std::fs::write(&tmp, EXTENSION_SOURCE)?;
    std::fs::rename(&tmp, &target)?;
    Ok(true)
}

pub fn install_agent_state_extension() {
    for dir in extension_dirs() {
        match install_into(&dir) {
            Ok(true) => {
                tracing::info!(dir = %dir.display(), "Installed Ferryx agent state extension")
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(dir = %dir.display(), %error, "Failed to install Ferryx agent state extension")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_skips_absent_directory() {
        let dir = std::env::temp_dir().join(format!("ferryx-ext-absent-{}", std::process::id()));
        assert!(!install_into(&dir).expect("absent dir is not an error"));
    }

    #[test]
    fn install_writes_then_becomes_idempotent() {
        let dir = std::env::temp_dir().join(format!("ferryx-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");

        assert!(install_into(&dir).expect("first install writes"));
        let written = std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).expect("read back");
        assert_eq!(written, EXTENSION_SOURCE);

        assert!(
            !install_into(&dir).expect("second install is a no-op"),
            "an unchanged extension must not be rewritten"
        );

        std::fs::write(dir.join(EXTENSION_FILE_NAME), "stale contents").expect("stale");
        assert!(install_into(&dir).expect("stale install rewrites"));
        assert_eq!(
            std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).expect("read back"),
            EXTENSION_SOURCE
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bundled_extension_reports_only_known_states() {
        assert!(EXTENSION_SOURCE.contains("FERRYX_AGENT_STATE_SOCKET"));
        assert!(EXTENSION_SOURCE.contains("FERRYX_SESSION_ID"));
        for state in ["working", "blocked", "idle"] {
            assert!(
                EXTENSION_SOURCE.contains(state),
                "extension must be able to report {state}"
            );
        }
    }

    #[test]
    fn bundled_extension_derives_provider_session_from_session_manager() {
        // The real pi runtime never exposes ctx.providerSession; the agent's own
        // session identity is only reachable via ctx.sessionManager.getSessionId().
        // A regression to mock-only shapes silently disables provider capture.
        assert!(
            EXTENSION_SOURCE.contains("sessionManager"),
            "extension must read the provider session from ctx.sessionManager"
        );
        assert!(
            EXTENSION_SOURCE.contains("getSessionId"),
            "extension must derive the provider session id via getSessionId"
        );
        assert!(
            EXTENSION_SOURCE.contains("\"session_id\""),
            "provider reference must use the daemon's session_id key"
        );
    }
}
