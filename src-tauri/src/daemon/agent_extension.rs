//! Installs the Ferryx agent-state extension into agents that expose a lifecycle extension API.
//!
//! Agents that report their own state are authoritative, so this integration replaces screen
//! inference for those sessions. The file is owned by Ferryx: it is rewritten whenever the bundled
//! version changes, and left untouched otherwise so an unchanged install costs no disk writes.

use std::path::{Path, PathBuf};

pub const EXTENSION_SOURCE: &str =
    include_str!("../../resources/agent-extensions/ferryx-agent-state.ts");
pub const EXTENSION_FILE_NAME: &str = "ferryx-agent-state.ts";

/// The agent the bundled body declares, and the identity every rewrite starts from.
pub const BASE_AGENT_ID: &str = "omo";

/// The extension directories that load this integration, each with the registry id the agent
/// inside them must report as.
///
/// The id is not decoration: it is what `AgentStateReport.agent` carries, and the daemon keys both
/// its machine-admission list and its history reader on it. The bundled body ships declaring
/// [`BASE_AGENT_ID`], so installing those bytes unchanged into `.pi` would have a pi pane report
/// itself as omo - a mislabelled identity no report can correct afterwards.
pub const EXTENSION_AGENTS: [(&str, &str); 3] = [("\u{2e}omo", "omo"), (".pi", "pi"), (".omp", "omp")];

/// The header line the extension carries its integration id on.
const INTEGRATION_ID_PREFIX: &str = "// FERRYX_INTEGRATION_ID=";
/// The declaration the extension carries its own agent id on.
const AGENT_ID_DECLARATION: &str = "const AGENT_ID =";

/// Rewrite a bundled body so it declares `agent`'s own identity.
///
/// Returns `None` when the body no longer carries the two lines to rewrite, so a body change fails
/// the install loudly instead of silently writing a mislabelled identity.
fn rewrite_identity(source: &str, agent: &str) -> Option<String> {
    let integration_line = format!("{INTEGRATION_ID_PREFIX}{BASE_AGENT_ID}");
    let declaration = format!("{AGENT_ID_DECLARATION} \"{BASE_AGENT_ID}\";");
    if !source.contains(&integration_line) || !source.contains(&declaration) {
        return None;
    }
    Some(
        source
            .replace(&integration_line, &format!("{INTEGRATION_ID_PREFIX}{agent}"))
            .replace(&declaration, &format!("{AGENT_ID_DECLARATION} \"{agent}\";")),
    )
}

/// The extension payload installed for one agent.
pub fn extension_source_for(agent: &str) -> Option<String> {
    rewrite_identity(EXTENSION_SOURCE, agent)
}

/// Extension directories of agents that share the same lifecycle extension API.
fn extension_dirs() -> Vec<(&'static str, PathBuf)> {
    extension_dirs_with_env(|key| std::env::var_os(key))
}

fn extension_dirs_with_env(
    get_env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, PathBuf)> {
    let Some(home) = get_env("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| get_env("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    EXTENSION_AGENTS
        .iter()
        .map(|(dir, agent)| (*agent, home.join(dir).join("agent").join("extensions")))
        .collect()
}

fn install_into(dir: &Path, agent: &str) -> std::io::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    let Some(source) = extension_source_for(agent) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the bundled extension no longer declares its agent identity",
        ));
    };
    let target = dir.join(EXTENSION_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&target) {
        if existing == source {
            return Ok(false);
        }
    }
    let tmp = dir.join(format!(".{EXTENSION_FILE_NAME}.tmp"));
    std::fs::write(&tmp, &source)?;
    std::fs::rename(&tmp, &target)?;
    Ok(true)
}

pub fn install_agent_state_extension() {
    for (agent, dir) in extension_dirs() {
        match install_into(&dir, agent) {
            Ok(true) => {
                tracing::info!(agent, dir = %dir.display(), "Installed Ferryx agent state extension")
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(agent, dir = %dir.display(), %error, "Failed to install Ferryx agent state extension")
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
        assert!(!install_into(&dir, BASE_AGENT_ID).expect("absent dir is not an error"));
    }

    #[test]
    fn install_writes_then_becomes_idempotent() {
        let dir = std::env::temp_dir().join(format!("ferryx-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");

        assert!(install_into(&dir, BASE_AGENT_ID).expect("first install writes"));
        let written = std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).expect("read back");
        assert_eq!(written, extension_source_for(BASE_AGENT_ID).unwrap());

        assert!(
            !install_into(&dir, BASE_AGENT_ID).expect("second install is a no-op"),
            "an unchanged extension must not be rewritten"
        );

        std::fs::write(dir.join(EXTENSION_FILE_NAME), "stale contents").expect("stale");
        assert!(install_into(&dir, BASE_AGENT_ID).expect("stale install rewrites"));
        assert_eq!(
            std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).expect("read back"),
            extension_source_for(BASE_AGENT_ID).unwrap()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The identity lines are the only difference between the payloads, so mask them and compare.
    fn masked_identity(source: &str, agent: &str) -> String {
        source
            .replace(
                &format!("{INTEGRATION_ID_PREFIX}{agent}"),
                "// FERRYX_INTEGRATION_ID=<agent>",
            )
            .replace(
                &format!("{AGENT_ID_DECLARATION} \"{agent}\";"),
                "const AGENT_ID = \"<agent>\";",
            )
    }

    #[test]
    fn each_agent_gets_a_payload_declaring_its_own_identity() {
        for (dir, agent) in EXTENSION_AGENTS {
            assert!(dir.starts_with('.'), "extension directories are dot directories");
            let source = extension_source_for(agent).expect("the bundled body declares its identity");
            assert!(
                source.contains(&format!("{AGENT_ID_DECLARATION} \"{agent}\";")),
                "the {agent} payload must declare {agent} as its own agent id"
            );
            assert!(
                source.contains(&format!("{INTEGRATION_ID_PREFIX}{agent}")),
                "the {agent} payload must carry {agent} as its integration id"
            );
            if agent != BASE_AGENT_ID {
                assert!(
                    !source.contains(&format!("{AGENT_ID_DECLARATION} \"{BASE_AGENT_ID}\";")),
                    "the {agent} payload must not keep {BASE_AGENT_ID}'s declaration"
                );
            }
        }
    }

    #[test]
    fn the_rewrite_changes_the_identity_lines_and_nothing_else() {
        for (_, agent) in EXTENSION_AGENTS {
            let source = extension_source_for(agent).unwrap();
            assert_eq!(
                masked_identity(&source, agent),
                masked_identity(EXTENSION_SOURCE, BASE_AGENT_ID),
                "the {agent} payload must be the bundled body with only its identity rewritten"
            );
        }
    }

    #[test]
    fn the_generated_payload_still_publishes_the_provider_transcript_path() {
        // Local consumers resolve a session's transcript from this path (the daemon stores the
        // published provider session as-is for a session it does not own), so the identity rewrite
        // must not touch it.
        for (_, agent) in EXTENSION_AGENTS {
            let source = extension_source_for(agent).unwrap();
            assert!(
                source.contains("transcriptPath"),
                "the {agent} payload must keep reporting the provider transcript path"
            );
            assert!(
                source.contains("agent: AGENT_ID"),
                "the {agent} payload must carry its declared identity on every report"
            );
        }
    }

    #[test]
    fn a_body_that_no_longer_declares_its_identity_is_refused() {
        assert!(rewrite_identity("const X = 1;\n", "pi").is_none());
        assert!(rewrite_identity("", "pi").is_none());
        assert!(rewrite_identity(EXTENSION_SOURCE, "pi").is_some());
    }

    #[test]
    fn install_writes_each_agents_own_payload() {
        let root = std::env::temp_dir().join(format!("ferryx-ext-agents-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        for (dir, agent) in EXTENSION_AGENTS {
            let target_dir = root.join(dir).join("agent").join("extensions");
            std::fs::create_dir_all(&target_dir).unwrap();
            assert!(install_into(&target_dir, agent).unwrap());
            let written =
                std::fs::read_to_string(target_dir.join(EXTENSION_FILE_NAME)).unwrap();
            assert_eq!(
                written,
                extension_source_for(agent).unwrap(),
                "the {agent} install must write that agent's own payload"
            );
            assert!(
                !install_into(&target_dir, agent).unwrap(),
                "an unchanged {agent} install must not be rewritten"
            );
        }
        std::fs::remove_dir_all(&root).ok();
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

    #[test]
    fn bundled_extension_publishes_a_rotated_provider_session() {
        // Starting a new conversation (`/new`) keeps the activity state, so an extension that
        // only publishes on state changes never tells Ferryx which conversation the pane moved
        // to, and restore resumes the one it was opened with.
        assert!(
            EXTENSION_SOURCE.contains("lastProviderSessionId"),
            "extension must remember the provider session id it last published"
        );
        assert!(
            EXTENSION_SOURCE.contains("!rotated"),
            "a rotated provider session must publish even when the activity state repeats"
        );
    }

    #[test]
    fn bundled_extension_listens_to_the_events_senpi_actually_emits() {
        // The ask machinery emits `herdr:blocked`; `ask-user:asked` is the same machine's own
        // namespace and carries `waitForAnswer`. A subscription to a name nobody emits leaves
        // the blocked count permanently zero, so an agent waiting on the user looks idle.
        assert!(
            EXTENSION_SOURCE.contains("\"herdr:blocked\""),
            "extension must subscribe to the event the ask machinery emits"
        );
        assert!(
            EXTENSION_SOURCE.contains("\"ask-user:asked\""),
            "extension must read waitForAnswer from ask-user:asked"
        );
        assert!(
            !EXTENSION_SOURCE.contains("\"ferryx:blocked\""),
            "no producer emits ferryx:blocked; subscribing to it silently disables blocked state"
        );
    }

    #[test]
    fn bundled_extension_ignores_questions_the_agent_keeps_working_through() {
        // `waitForAnswer: false` means the question stays open while the agent keeps working, so
        // counting it would show a false "needs you" row for a session that is not waiting.
        assert!(
            EXTENSION_SOURCE.contains("waitForAnswer === false"),
            "a non-blocking question must be excluded from the blocked count"
        );
        assert!(
            EXTENSION_SOURCE.contains("nonBlockingIds"),
            "the non-blocking set must exist so the later release does not unpublish a real block"
        );
    }

    #[test]
    fn bundled_extension_sends_the_blocked_detail() {
        // The inbox shows the question text on the row; without `detail` on the wire the row can
        // only name a state word, and the daemon drops an unknown field silently.
        assert!(
            EXTENSION_SOURCE.contains("detail"),
            "the extension must send the blocked detail"
        );
        assert!(
            EXTENSION_SOURCE.contains("questionLabel"),
            "the detail must be derived from the question, not from a bare state word"
        );
    }
}

#[cfg(test)]
mod p09_tests {
    use super::*;

    #[test]
    fn userprofile_only_installs_existing_agent_extensions() {
        let root = std::env::temp_dir().join(format!("p09-home-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            let expected: Vec<(&str, PathBuf)> = EXTENSION_AGENTS
                .iter()
                .map(|(dir, agent)| (*agent, root.join(dir).join("agent/extensions")))
                .collect();
            for (_, dir) in &expected {
                std::fs::create_dir_all(dir).unwrap();
            }
            let dirs = extension_dirs_with_env(|key| {
                (key == "USERPROFILE").then(|| root.clone().into_os_string())
            });
            assert_eq!(dirs, expected);
            for (agent, dir) in dirs {
                assert!(install_into(&dir, agent).unwrap());
                assert_eq!(
                    std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).unwrap(),
                    extension_source_for(agent).unwrap(),
                    "each directory must receive the payload of the agent that loads it"
                );
                assert!(!install_into(&dir, agent).unwrap());
            }
            assert!(extension_dirs_with_env(|_| None).is_empty());
        });
        std::fs::remove_dir_all(&root).unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
