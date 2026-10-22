//! Installs the Ferryx agent-state extension into agents that expose a lifecycle extension API.
//!
//! Agents that report their own state are authoritative, so this integration replaces screen
//! inference for those sessions. The file is owned by Ferryx: it is rewritten whenever the bundled
//! version changes, and left untouched otherwise so an unchanged install costs no disk writes.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const EXTENSION_SOURCE: &str =
    include_str!("../../resources/agent-extensions/ferryx-agent-state.ts");
pub const EXTENSION_FILE_NAME: &str = "ferryx-agent-state.ts";

/// Opencode exposes no command hook, only a plugin API, so its reporter is a plugin file that
/// sends the wire line itself (facts/local-1-evidence/opencode-events.ndjson).
pub const PLUGIN_SOURCE: &str =
    include_str!("../../resources/agent-extensions/ferryx-opencode-plugin.js");
pub const PLUGIN_FILE_NAME: &str = "ferryx-agent-state.js";

/// Agents whose reporter is a plugin file: the agent's registry id, the config directory whose
/// existence gates the install, and the subdirectory the plugin goes in. The config directory is
/// `$XDG_CONFIG_HOME` (default `~/.config`) rooted, matching the herdr plugin's own location.
pub const PLUGIN_AGENTS: [(&str, &str, &str); 1] = [("opencode", "opencode", "plugins")];

/// The opt-out setting file, kept beside \`remote-config.json\` in the per-user remote data dir.
pub const AGENT_INTEGRATION_FILE_NAME: &str = "agent-integration.json";

#[derive(Debug, Serialize, Deserialize)]
struct PersistedAgentIntegration {
    enabled: bool,
}

/// Whether Ferryx installs its agent-state extension and exposes the agent-state endpoint to new
/// panes. The daemon owns one instance; every pane it spawns reads the in-memory flag, so a change
/// applies to the next pane without a restart.
#[derive(Debug)]
pub struct AgentIntegration {
    enabled: AtomicBool,
    path: Option<PathBuf>,
    write_lock: std::sync::Mutex<()>,
}

impl AgentIntegration {
    /// An unpersisted setting, used where no daemon-owned setting exists.
    pub fn in_memory(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            path: None,
            write_lock: std::sync::Mutex::new(()),
        }
    }

    /// Loads the setting from \`path\`. An absent, unreadable or malformed file means enabled: the
    /// integration is on by default and only an explicit, readable opt-out turns it off.
    pub fn load(path: Option<PathBuf>) -> Self {
        let enabled = path.as_deref().is_none_or(read_enabled);
        Self {
            enabled: AtomicBool::new(enabled),
            path,
            write_lock: std::sync::Mutex::new(()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Persists \`enabled\` and then publishes it in memory. A failed write leaves both unchanged,
    /// so the flag never disagrees with what the next boot will read. Blocking: performs disk I/O.
    pub fn set(&self, enabled: bool) -> std::io::Result<()> {
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(path) = self.path.as_deref() {
            crate::remote::auth::write_private_json(path, &PersistedAgentIntegration { enabled })?;
        }
        self.enabled.store(enabled, Ordering::Release);
        Ok(())
    }
}

fn read_enabled(path: &Path) -> bool {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "Unreadable agent integration setting; keeping it enabled");
            return true;
        }
    };
    match serde_json::from_slice::<PersistedAgentIntegration>(&bytes) {
        Ok(setting) => setting.enabled,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "Malformed agent integration setting; keeping it enabled");
            true
        }
    }
}

/// The setting file path for the current environment: the same \`<data>/remote\` directory that
/// holds \`remote-config.json\`, so \`FERRYX_DATA_DIR\` relocates both together.
pub fn agent_integration_path() -> Option<PathBuf> {
    agent_integration_path_with_env(|key| std::env::var_os(key))
}

fn agent_integration_path_with_env(lookup: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    crate::remote::auth::resolve_canonical_remote_dir(lookup)
        .map(|dir| dir.join(AGENT_INTEGRATION_FILE_NAME))
}

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

/// The variable naming senpi's agent directory. senpi is the engine the measured `pi` command runs,
/// and it autoloads `<agentDir>/extensions/*.ts` with agentDir `$SENPI_CODING_AGENT_DIR`, else
/// `~/.senpi/agent` (facts/local-1.md:341-376, facts/local.md:181,189). Upstream pi keeps
/// `~/.pi/agent`, which [`EXTENSION_AGENTS`] covers; both are installed where they exist.
pub const SENPI_AGENT_DIR_ENV: &str = "SENPI_CODING_AGENT_DIR";

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

/// The gate directory and the plugin directory of each plugin-style agent.
fn plugin_dirs() -> Vec<(&'static str, PathBuf, PathBuf)> {
    plugin_dirs_with_env(|key| std::env::var_os(key))
}

fn plugin_dirs_with_env(
    get_env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, PathBuf, PathBuf)> {
    let Some(home) = get_env("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| get_env("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let config_home = get_env("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    PLUGIN_AGENTS
        .iter()
        .map(|(agent, dir, plugins)| {
            let agent_dir = config_home.join(dir);
            let plugin_dir = agent_dir.join(plugins);
            (*agent, agent_dir, plugin_dir)
        })
        .collect()
}

/// Writes the plugin where the agent loads it from. The plugin subdirectory is created on demand,
/// because an opencode install that has never had a plugin does not have one.
fn install_plugin_into(agent_dir: &Path, plugin_dir: &Path) -> std::io::Result<bool> {
    if !agent_dir.is_dir() {
        return Ok(false);
    }
    let target = plugin_dir.join(PLUGIN_FILE_NAME);
    if let Ok(existing) = std::fs::read_to_string(&target) {
        if existing == PLUGIN_SOURCE {
            return Ok(false);
        }
    }
    std::fs::create_dir_all(plugin_dir)?;
    let tmp = plugin_dir.join(format!(".{PLUGIN_FILE_NAME}.tmp"));
    std::fs::write(&tmp, PLUGIN_SOURCE)?;
    std::fs::rename(&tmp, &target)?;
    Ok(true)
}

fn install_plugin_dirs(dirs: Vec<(&'static str, PathBuf, PathBuf)>) {
    for (agent, agent_dir, plugin_dir) in dirs {
        match install_plugin_into(&agent_dir, &plugin_dir) {
            Ok(true) => {
                tracing::info!(agent, dir = %plugin_dir.display(), "Installed Ferryx agent state plugin")
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(agent, dir = %plugin_dir.display(), %error, "Failed to install Ferryx agent state plugin")
            }
        }
    }
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
        .chain(std::iter::once(("pi", senpi_extension_dir(&home, &get_env))))
        // omo's launcher points SENPI_CODING_AGENT_DIR at ~/.omo/agent, so senpi's directory can
        // be one already listed. A directory is installed once: the payload tells omo from pi at
        // runtime (`reportedAgentId` in the extension), and two declared identities for one file
        // would rewrite it on every boot.
        .fold(Vec::new(), |mut dirs: Vec<(&'static str, PathBuf)>, (agent, dir)| {
            if !dirs.iter().any(|(_, listed)| same_dir(listed, &dir)) {
                dirs.push((agent, dir));
            }
            dirs
        })
}

fn senpi_extension_dir(home: &Path, get_env: &impl Fn(&str) -> Option<OsString>) -> PathBuf {
    get_env(SENPI_AGENT_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".senpi").join("agent"))
        .join("extensions")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
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
    install_into_dirs(extension_dirs());
    install_plugin_dirs(plugin_dirs());
}

/// The daemon's boot install: skipped entirely while the integration is turned off.
pub fn install_agent_state_extension_if_enabled(setting: &AgentIntegration) -> bool {
    // The hook command names this executable: the daemon's own staged copy, which carries the
    // `agent-hook` reporter. Without a resolvable path only the hook install is skipped.
    let exe = std::env::current_exe().ok();
    let hooks = crate::daemon::agent_hook_install::hook_targets();
    install_targets_if_enabled(setting, extension_dirs(), plugin_dirs(), &hooks, exe.as_deref())
}

#[cfg(test)]
fn install_into_dirs_if_enabled(
    setting: &AgentIntegration,
    dirs: Vec<(&'static str, PathBuf)>,
) -> bool {
    install_targets_if_enabled(setting, dirs, Vec::new(), &[], None)
}

/// Installs every kind of target behind the one setting: extension files, plugin files and merged
/// hook entries.
fn install_targets_if_enabled(
    setting: &AgentIntegration,
    dirs: Vec<(&'static str, PathBuf)>,
    plugins: Vec<(&'static str, PathBuf, PathBuf)>,
    hooks: &[crate::daemon::agent_hook_install::HookFileTarget],
    exe: Option<&Path>,
) -> bool {
    if !setting.enabled() {
        tracing::info!("Agent state integration is turned off; skipping the extension install");
        return false;
    }
    install_into_dirs(dirs);
    install_plugin_dirs(plugins);
    if let Some(exe) = exe {
        crate::daemon::agent_hook_install::install_hook_targets(hooks, exe);
    }
    true
}

fn install_into_dirs(dirs: Vec<(&'static str, PathBuf)>) {
    for (agent, dir) in dirs {
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalFailure {
    pub path: String,
    pub error: String,
}

/// What a local removal did to each extension file Ferryx may have installed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalReport {
    /// Files deleted because their bytes were exactly a Ferryx payload.
    pub removed: Vec<String>,
    /// Paths left in place because they are not a byte-exact Ferryx payload (user-modified, a
    /// directory, or a symlink).
    pub kept_modified: Vec<String>,
    /// Paths that could not be inspected or deleted.
    pub failed: Vec<RemovalFailure>,
}

/// Deletes the Ferryx agent-state extension from every agent extension directory, but only where
/// the file is byte-for-byte a payload Ferryx writes. Directories are never deleted. Hook entries
/// Ferryx merged into an agent's settings file are deleted the same way: only groups with exactly
/// the shape Ferryx writes go, and the file itself stays.
pub fn remove_agent_state_extension() -> RemovalReport {
    let mut report = remove_payload_files(
        extension_dirs()
            .into_iter()
            .map(|(_, dir)| (dir.join(EXTENSION_FILE_NAME), is_ferryx_payload as fn(&[u8]) -> bool))
            .chain(plugin_dirs().into_iter().map(|(_, _, dir)| {
                (dir.join(PLUGIN_FILE_NAME), is_ferryx_plugin as fn(&[u8]) -> bool)
            })),
    );
    remove_hooks_into_report(&crate::daemon::agent_hook_install::hook_targets(), &mut report);
    report
}

/// A settings file is listed under `removed` when Ferryx entries were deleted from it.
fn remove_hooks_into_report(
    targets: &[crate::daemon::agent_hook_install::HookFileTarget],
    report: &mut RemovalReport,
) {
    for target in targets {
        let display = target.file.to_string_lossy().into_owned();
        match crate::daemon::agent_hook_install::remove_hooks_from(target) {
            Ok(0) => {}
            Ok(_) => report.removed.push(display),
            Err(error) => report.failed.push(RemovalFailure { path: display, error: error.to_string() }),
        }
    }
}

fn is_ferryx_payload(bytes: &[u8]) -> bool {
    // Any agent's payload counts, so a file written with another agent's identity is still ours.
    EXTENSION_AGENTS
        .iter()
        .filter_map(|(_, agent)| extension_source_for(agent))
        .any(|source| source.as_bytes() == bytes)
}

fn is_ferryx_plugin(bytes: &[u8]) -> bool {
    PLUGIN_SOURCE.as_bytes() == bytes
}

#[cfg(test)]
fn remove_from_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> RemovalReport {
    remove_payload_files(
        dirs.into_iter()
            .map(|dir| (dir.join(EXTENSION_FILE_NAME), is_ferryx_payload as fn(&[u8]) -> bool)),
    )
}

/// Deletes each path whose bytes are exactly a payload Ferryx writes, and reports the rest.
fn remove_payload_files(
    targets: impl IntoIterator<Item = (PathBuf, fn(&[u8]) -> bool)>,
) -> RemovalReport {
    let mut report = RemovalReport::default();
    for (target, is_payload) in targets {
        let display = target.to_string_lossy().into_owned();
        let metadata = match std::fs::symlink_metadata(&target) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                report.failed.push(RemovalFailure { path: display, error: error.to_string() });
                continue;
            }
        };
        if !metadata.file_type().is_file() {
            report.kept_modified.push(display);
            continue;
        }
        match std::fs::read(&target) {
            Ok(bytes) if is_payload(&bytes) => match std::fs::remove_file(&target) {
                Ok(()) => report.removed.push(display),
                Err(error) => {
                    report.failed.push(RemovalFailure { path: display, error: error.to_string() })
                }
            },
            Ok(_) => report.kept_modified.push(display),
            Err(error) => {
                report.failed.push(RemovalFailure { path: display, error: error.to_string() })
            }
        }
    }
    report
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
                source.contains("agent: reportedAgentId()"),
                "the {agent} payload must carry its identity on every report"
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
    fn bundled_extension_settles_and_blocks_on_the_events_omp_actually_emits() {
        // Measured on omp 18.12.6 (.omo-parity/facts/raw-omp/ext-events.log): a run ends with
        // `agent_end` carrying `willContinue`, and `agent_settled` never fires, so an extension
        // that only listens for `agent_settled` leaves omp panes working forever.
        assert!(
            EXTENSION_SOURCE.contains("\"agent_end\""),
            "extension must settle omp runs on agent_end"
        );
        assert!(
            EXTENSION_SOURCE.contains("willContinue !== false"),
            "agent_end must settle only when the runtime says no run follows"
        );
        assert!(
            EXTENSION_SOURCE.contains("\"agent_settled\""),
            "omo and pi still settle through agent_settled"
        );
        // The omp approval dialog raises these, never the question tool's events.
        assert!(
            EXTENSION_SOURCE.contains("\"tool_approval_requested\""),
            "an omp tool approval must report blocked"
        );
        assert!(
            EXTENSION_SOURCE.contains("\"tool_approval_resolved\""),
            "a resolved omp tool approval must release the block"
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

    fn temp_root(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("ferryx-t8-{tag}-"))
            .tempdir()
            .expect("temp root")
    }

    fn agent_dirs(root: &Path) -> Vec<PathBuf> {
        EXTENSION_AGENTS
            .iter()
            .map(|(dir, _)| {
                let path = root.join(dir).join("agent").join("extensions");
                std::fs::create_dir_all(&path).unwrap();
                path
            })
            .collect()
    }

    #[test]
    fn remove_deletes_files_matching_any_agent_payload() {
        let root = temp_root("remove-any");
        let dirs = agent_dirs(root.path());
        // .omo gets its own payload, .pi gets omo's (a wrong-identity install from an older
        // build), .omp gets pi's: all three are Ferryx bytes and must go.
        let payloads = [
            extension_source_for("omo").unwrap(),
            extension_source_for("omo").unwrap(),
            extension_source_for("pi").unwrap(),
        ];
        for (dir, payload) in dirs.iter().zip(payloads.iter()) {
            std::fs::write(dir.join(EXTENSION_FILE_NAME), payload).unwrap();
        }
        let report = remove_from_dirs(dirs.clone());
        assert_eq!(report.removed.len(), 3, "{report:?}");
        assert!(report.kept_modified.is_empty() && report.failed.is_empty(), "{report:?}");
        for dir in &dirs {
            assert!(!dir.join(EXTENSION_FILE_NAME).exists());
            assert!(dir.is_dir(), "the extension directory itself must stay");
        }
        // A second removal finds nothing and reports nothing.
        assert_eq!(remove_from_dirs(dirs), RemovalReport::default());
    }

    #[test]
    fn remove_keeps_user_modified_file_and_reports_path() {
        let root = temp_root("remove-kept");
        let dirs = agent_dirs(root.path());
        let edited = format!("{}\n// user edit\n", extension_source_for("omo").unwrap());
        let target = dirs[0].join(EXTENSION_FILE_NAME);
        std::fs::write(&target, &edited).unwrap();
        std::fs::write(dirs[1].join(EXTENSION_FILE_NAME), extension_source_for("pi").unwrap())
            .unwrap();

        let report = remove_from_dirs(dirs.clone());
        assert_eq!(report.kept_modified, vec![target.to_string_lossy().into_owned()]);
        assert_eq!(report.removed, vec![dirs[1].join(EXTENSION_FILE_NAME).to_string_lossy().into_owned()]);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), edited, "a user file is never touched");
    }

    #[test]
    fn remove_never_deletes_directories() {
        let root = temp_root("remove-dirs");
        let dirs = agent_dirs(root.path());
        // A directory occupying the file name is kept and reported, with its contents intact.
        let squatter = dirs[0].join(EXTENSION_FILE_NAME);
        std::fs::create_dir(&squatter).unwrap();
        std::fs::write(squatter.join("inner.ts"), extension_source_for("omo").unwrap()).unwrap();
        // A missing extension directory is a no-op, not an error and not a creation.
        let absent = root.path().join("absent").join("extensions");
        let report = remove_from_dirs(vec![dirs[0].clone(), dirs[1].clone(), absent.clone()]);
        assert_eq!(report.kept_modified, vec![squatter.to_string_lossy().into_owned()]);
        assert!(report.removed.is_empty() && report.failed.is_empty(), "{report:?}");
        assert!(squatter.join("inner.ts").is_file());
        assert!(dirs.iter().all(|dir| dir.is_dir()));
        assert!(!absent.exists());
    }

    #[test]
    fn disabled_integration_skips_boot_install() {
        let root = temp_root("boot-install");
        let dirs: Vec<(&'static str, PathBuf)> = EXTENSION_AGENTS
            .iter()
            .zip(agent_dirs(root.path()))
            .map(|((_, agent), dir)| (*agent, dir))
            .collect();
        let off = AgentIntegration::in_memory(false);
        assert!(!install_into_dirs_if_enabled(&off, dirs.clone()));
        assert!(dirs.iter().all(|(_, dir)| !dir.join(EXTENSION_FILE_NAME).exists()));

        let on = AgentIntegration::in_memory(true);
        assert!(install_into_dirs_if_enabled(&on, dirs.clone()));
        for (agent, dir) in &dirs {
            assert_eq!(
                std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).unwrap(),
                extension_source_for(agent).unwrap()
            );
        }
    }

    fn hook_targets_in(home: &Path) -> Vec<crate::daemon::agent_hook_install::HookFileTarget> {
        let home = home.as_os_str().to_os_string();
        let targets = crate::daemon::agent_hook_install::hook_targets_with_env(move |key| {
            (key == "HOME").then(|| home.clone())
        });
        for target in &targets {
            std::fs::create_dir_all(&target.agent_dir).unwrap();
        }
        targets
    }

    fn plugin_dirs_in(home: &Path) -> Vec<(&'static str, PathBuf, PathBuf)> {
        let home = home.as_os_str().to_os_string();
        let dirs = plugin_dirs_with_env(move |key| (key == "HOME").then(|| home.clone()));
        for (_, agent_dir, _) in &dirs {
            std::fs::create_dir_all(agent_dir).unwrap();
        }
        dirs
    }

    /// Everything an install wrote for one target, whatever its shape: the hook file's text, or
    /// every script in the hook directory.
    fn installed_text(target: &crate::daemon::agent_hook_install::HookFileTarget) -> String {
        if target.file.is_dir() {
            let mut names: Vec<_> = std::fs::read_dir(&target.file)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            names.sort();
            return names
                .iter()
                .map(|path| std::fs::read_to_string(path).unwrap())
                .collect::<String>();
        }
        std::fs::read_to_string(&target.file).unwrap_or_default()
    }

    #[test]
    fn disabled_integration_skips_the_hook_install() {
        let root = temp_root("hook-gate");
        let hooks = hook_targets_in(root.path());
        let plugins = plugin_dirs_in(root.path());
        let exe = root.path().join("bin").join("ferryx");
        let off = AgentIntegration::in_memory(false);
        assert!(!install_targets_if_enabled(
            &off,
            Vec::new(),
            plugins.clone(),
            &hooks,
            Some(&exe)
        ));
        assert!(hooks.iter().all(|target| !target.file.exists()));
        assert!(plugins.iter().all(|(_, _, dir)| !dir.join(PLUGIN_FILE_NAME).exists()));

        let on = AgentIntegration::in_memory(true);
        assert!(install_targets_if_enabled(&on, Vec::new(), plugins.clone(), &hooks, Some(&exe)));
        // Every agent's install, in whatever shape, has to end up naming the reporter command for
        // each of that agent's events.
        for target in &hooks {
            let written = installed_text(target);
            assert!(!written.is_empty(), "{} wrote nothing", target.agent);
            for event in target.events {
                let command =
                    crate::daemon::agent_hook_install::hook_command(&exe, target.agent, event);
                assert!(written.contains(&command), "{} {event}: {written}", target.agent);
            }
        }
        for (_, _, dir) in &plugins {
            assert_eq!(
                std::fs::read_to_string(dir.join(PLUGIN_FILE_NAME)).unwrap(),
                PLUGIN_SOURCE
            );
        }
    }

    #[test]
    fn removal_reports_settings_files_it_cleaned() {
        let root = temp_root("hook-remove");
        let hooks = hook_targets_in(root.path());
        let exe = root.path().join("bin").join("ferryx");
        let user = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"orca-hook"}]}]}}"#;
        std::fs::write(&hooks[0].file, user).unwrap();
        crate::daemon::agent_hook_install::install_hook_targets(&hooks, &exe);

        let mut report = RemovalReport::default();
        remove_hooks_into_report(&hooks, &mut report);
        let files: Vec<String> =
            hooks.iter().map(|target| target.file.to_string_lossy().into_owned()).collect();
        assert_eq!(report.removed, files, "every target is cleaned: {report:?}");
        assert!(report.failed.is_empty() && report.kept_modified.is_empty(), "{report:?}");
        let kept: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&hooks[0].file).unwrap()).unwrap();
        assert_eq!(kept, serde_json::from_str::<serde_json::Value>(user).unwrap());

        let mut again = RemovalReport::default();
        remove_hooks_into_report(&hooks, &mut again);
        assert_eq!(again, RemovalReport::default());
    }

    #[test]
    fn the_opencode_plugin_is_installed_where_opencode_loads_it() {
        let root = temp_root("opencode-plugin");
        let dirs = plugin_dirs_in(root.path());
        let (agent, agent_dir, plugin_dir) = dirs[0].clone();
        assert_eq!(agent, "opencode");
        assert_eq!(agent_dir, root.path().join(".config").join("opencode"));
        assert_eq!(plugin_dir, agent_dir.join("plugins"));
        let target = plugin_dir.join(PLUGIN_FILE_NAME);

        // The plugin subdirectory is created on demand.
        assert!(!plugin_dir.exists());
        assert!(install_plugin_into(&agent_dir, &plugin_dir).unwrap());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), PLUGIN_SOURCE);
        assert!(!install_plugin_into(&agent_dir, &plugin_dir).unwrap(), "a reinstall is a no-op");
        assert_eq!(std::fs::read_dir(&plugin_dir).unwrap().count(), 1, "no temp file is left");

        // Only the exact bytes Ferryx wrote are deleted.
        let report = remove_payload_files(vec![(target.clone(), is_ferryx_plugin as fn(&[u8]) -> bool)]);
        assert_eq!(report.removed, vec![target.to_string_lossy().into_owned()], "{report:?}");
        assert!(!target.exists() && plugin_dir.is_dir(), "the directory itself stays");

        std::fs::write(&target, format!("{PLUGIN_SOURCE}// mine\n")).unwrap();
        let report = remove_payload_files(vec![(target.clone(), is_ferryx_plugin as fn(&[u8]) -> bool)]);
        assert_eq!(report.kept_modified, vec![target.to_string_lossy().into_owned()], "{report:?}");
        assert!(target.exists());
    }

    #[test]
    fn an_opencode_install_that_is_not_there_is_skipped() {
        let root = temp_root("opencode-absent");
        let home = root.path().as_os_str().to_os_string();
        let dirs = plugin_dirs_with_env(move |key| (key == "HOME").then(|| home.clone()));
        let (_, agent_dir, plugin_dir) = &dirs[0];
        assert!(!install_plugin_into(agent_dir, plugin_dir).unwrap());
        assert!(!plugin_dir.exists());
        assert!(plugin_dirs_with_env(|_| None).is_empty());
    }

    #[test]
    fn the_opencode_plugin_follows_the_xdg_config_home() {
        let root = temp_root("opencode-xdg");
        let xdg = root.path().join("elsewhere");
        let (home, config) =
            (root.path().as_os_str().to_os_string(), xdg.as_os_str().to_os_string());
        let dirs = plugin_dirs_with_env(move |key| match key {
            "HOME" => Some(home.clone()),
            "XDG_CONFIG_HOME" => Some(config.clone()),
            _ => None,
        });
        assert_eq!(dirs[0].2, xdg.join("opencode").join("plugins"));
    }

    /// The plugin is the whole of opencode's F8 support, so its mapping is asserted here against
    /// the event names measured in `opencode-events.ndjson`.
    #[test]
    fn the_opencode_plugin_maps_the_measured_events() {
        for event in [
            "session.status",
            "session.idle",
            "session.updated",
            "permission.asked",
            "permission.replied",
        ] {
            assert!(PLUGIN_SOURCE.contains(event), "the plugin must handle {event}");
        }
        for state in ["idle", "working", "blocked"] {
            assert!(PLUGIN_SOURCE.contains(state), "the plugin must report {state}");
        }
        // It talks the same wire as the extension and the reporter.
        for marker in [
            "agentState",
            "FERRYX_AGENT_STATE_SOCKET",
            "FERRYX_AGENT_STATE_PORT",
            "FERRYX_AGENT_STATE_TOKEN",
            "FERRYX_SESSION_ID",
        ] {
            assert!(PLUGIN_SOURCE.contains(marker), "the plugin must use {marker}");
        }
        // It captures the session a fresh opencode pane opens with, under the key the resume plan
        // requires, and it ignores child sessions.
        assert!(PLUGIN_SOURCE.contains("\"session_id\""));
        assert!(PLUGIN_SOURCE.contains("sessionID"));
        assert!(PLUGIN_SOURCE.contains("parentID"));
        assert!(PLUGIN_SOURCE.contains(&format!("AGENT_ID = \"{}\";", "opencode")));
    }

    /// The id the plugin publishes has to survive the daemon's resume-plan gate, exactly as the
    /// reporter's does for the hook-style agents.
    #[test]
    fn an_opencode_provider_session_resolves_to_a_resume_plan() {
        let provider = crate::daemon::protocol::AgentProviderSession {
            key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
            id: "ses_e9d886f2bffeJp9DsEMPfnB25I".to_string(),
            transcript_path: None,
        };
        let plan = crate::terminal::shell::resolve_agent_resume_plan("opencode", &provider)
            .expect("opencode resumes by session id");
        assert_eq!(plan.program, "opencode");
        assert!(plan.args.iter().any(|arg| arg == "ses_e9d886f2bffeJp9DsEMPfnB25I"), "{plan:?}");
    }

    /// Runs \`env\` in a pane spawned by \`manager\` and returns everything it printed.
    #[cfg(unix)]
    pub(crate) async fn pane_env(manager: &crate::terminal::pty::PtyManager) -> String {
        let mut cmd = portable_pty::CommandBuilder::new("/usr/bin/env");
        // The variable must be absent because Ferryx withheld it, not because the test process
        // happened not to have it: seed it as an inherited value first.
        cmd.env("FERRYX_AGENT_STATE_SOCKET", "/inherited/should-not-survive.sock");
        let (_id, mut rx) = manager.spawn(cmd, 200, 50).expect("spawn env pane");
        let mut out = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(chunk) = rx.recv().await {
                out.extend_from_slice(&chunk);
            }
        })
        .await
        .expect("env pane output ends when env exits");
        String::from_utf8_lossy(&out).into_owned()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn disabled_integration_skips_env_injection() {
        let setting = std::sync::Arc::new(AgentIntegration::in_memory(false));
        let manager = crate::terminal::pty::PtyManager::new()
            .with_agent_integration(std::sync::Arc::clone(&setting));
        let env = pane_env(&manager).await;
        assert!(env.contains("FERRYX_SESSION_ID="), "the pane ran env: {env}");
        assert!(!env.contains("FERRYX_AGENT_STATE_SOCKET"), "{env}");

        setting.set(true).unwrap();
        let env = pane_env(&manager).await;
        assert!(
            env.contains(&format!(
                "FERRYX_AGENT_STATE_SOCKET={}",
                crate::daemon::agent_state_socket_path()
            )),
            "{env}"
        );
    }

    #[test]
    fn setting_file_respects_ferryx_data_dir() {
        let root = temp_root("data-dir");
        let data = root.path().join("data");
        let path = agent_integration_path_with_env(|key| {
            (key == "FERRYX_DATA_DIR").then(|| data.clone().into_os_string())
        })
        .unwrap();
        // Same directory remote-config.json lives in.
        assert_eq!(path, data.join("remote").join(AGENT_INTEGRATION_FILE_NAME));
        assert_eq!(
            path.parent().unwrap(),
            crate::remote::auth::resolve_canonical_remote_dir(|key| {
                (key == "FERRYX_DATA_DIR").then(|| data.clone().into_os_string())
            })
            .unwrap()
        );

        let setting = AgentIntegration::load(Some(path.clone()));
        assert!(setting.enabled(), "absent file means enabled");
        setting.set(false).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({ "enabled": false }));
        assert!(!AgentIntegration::load(Some(path)).enabled(), "the opt-out survives a restart");
    }

    #[test]
    fn malformed_setting_file_defaults_to_enabled() {
        let root = temp_root("malformed");
        for body in ["", "{", "not json", "{\"enabled\":\"no\"}", "[]"] {
            let path = root.path().join("agent-integration.json");
            std::fs::write(&path, body).unwrap();
            assert!(AgentIntegration::load(Some(path)).enabled(), "{body:?} must read as enabled");
        }
    }

    #[test]
    fn userprofile_only_installs_existing_agent_extensions() {
        let root = std::env::temp_dir().join(format!("p09-home-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            let expected: Vec<(&str, PathBuf)> = EXTENSION_AGENTS
                .iter()
                .map(|(dir, agent)| (*agent, root.join(dir).join("agent/extensions")))
                .chain(std::iter::once(("pi", root.join(".senpi/agent/extensions"))))
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

    /// `HOME` at `home`, plus `SENPI_CODING_AGENT_DIR` when given; nothing else is visible.
    fn senpi_env(home: &Path, senpi: Option<&Path>) -> impl Fn(&str) -> Option<OsString> {
        let home = home.as_os_str().to_os_string();
        let senpi = senpi.map(|dir| dir.as_os_str().to_os_string());
        move |key: &str| match key {
            "HOME" => Some(home.clone()),
            SENPI_AGENT_DIR_ENV => senpi.clone(),
            _ => None,
        }
    }

    #[test]
    fn pi_installs_into_the_senpi_agent_dir() {
        // The measured pi is senpi; it autoloads ~/.senpi/agent/extensions (facts/local-1.md:373).
        let root = temp_root("senpi-default");
        let senpi = root.path().join(".senpi").join("agent").join("extensions");
        std::fs::create_dir_all(&senpi).unwrap();
        let dirs = extension_dirs_with_env(senpi_env(root.path(), None));
        assert!(dirs.contains(&("pi", senpi.clone())), "{dirs:?}");
        install_into_dirs(dirs);
        assert_eq!(
            std::fs::read_to_string(senpi.join(EXTENSION_FILE_NAME)).unwrap(),
            extension_source_for("pi").unwrap()
        );
        // Only directories that exist are installed into: no other agent home was created.
        assert!(!root.path().join(".pi").exists() && !root.path().join(".omo").exists());
    }

    #[test]
    fn pi_honours_senpi_coding_agent_dir() {
        let root = temp_root("senpi-env");
        let custom = root.path().join("custom-agent");
        let target = custom.join("extensions");
        std::fs::create_dir_all(&target).unwrap();
        let default_dir = root.path().join(".senpi").join("agent").join("extensions");
        std::fs::create_dir_all(&default_dir).unwrap();
        let dirs = extension_dirs_with_env(senpi_env(root.path(), Some(&custom)));
        assert!(dirs.contains(&("pi", target.clone())), "{dirs:?}");
        assert!(!dirs.iter().any(|(_, dir)| *dir == default_dir), "the override replaces ~/.senpi/agent");
        install_into_dirs(dirs);
        assert_eq!(
            std::fs::read_to_string(target.join(EXTENSION_FILE_NAME)).unwrap(),
            extension_source_for("pi").unwrap()
        );
        assert!(!default_dir.join(EXTENSION_FILE_NAME).exists());
        // An empty value is no override.
        let empty = extension_dirs_with_env(senpi_env(root.path(), Some(Path::new(""))));
        assert!(empty.contains(&("pi", default_dir)), "{empty:?}");
    }

    #[test]
    fn upstream_pi_and_senpi_are_both_installed() {
        let root = temp_root("senpi-and-pi");
        let upstream = root.path().join(".pi").join("agent").join("extensions");
        let senpi = root.path().join(".senpi").join("agent").join("extensions");
        for dir in [&upstream, &senpi] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let dirs = extension_dirs_with_env(senpi_env(root.path(), None));
        assert!(dirs.contains(&("pi", upstream.clone())) && dirs.contains(&("pi", senpi.clone())), "{dirs:?}");
        install_into_dirs(dirs);
        for dir in [&upstream, &senpi] {
            assert_eq!(
                std::fs::read_to_string(dir.join(EXTENSION_FILE_NAME)).unwrap(),
                extension_source_for("pi").unwrap(),
                "{}",
                dir.display()
            );
        }
    }

    #[test]
    fn omo_and_pi_sharing_one_dir_install_once_and_resolve_identity_at_runtime() {
        let root = temp_root("senpi-shared");
        let omo = root.path().join(".omo").join("agent");
        std::fs::create_dir_all(omo.join("extensions")).unwrap();
        // Without the override senpi has its own directory.
        let separate = extension_dirs_with_env(senpi_env(root.path(), None));
        assert_eq!(separate.len(), EXTENSION_AGENTS.len() + 1, "{separate:?}");
        // omo's launcher exports SENPI_CODING_AGENT_DIR=~/.omo/agent, so a pi started there loads
        // omo's directory: it is listed once, under omo, and never rewritten per boot.
        let shared = extension_dirs_with_env(senpi_env(root.path(), Some(&omo)));
        let hits: Vec<_> = shared.iter().filter(|(_, dir)| same_dir(dir, &omo.join("extensions"))).collect();
        assert_eq!(hits, vec![&("omo", omo.join("extensions"))], "{shared:?}");
        assert_eq!(shared.len(), EXTENSION_AGENTS.len(), "{shared:?}");
        install_into_dirs(shared.clone());
        let target = omo.join("extensions").join(EXTENSION_FILE_NAME);
        let written = std::fs::read(&target).unwrap();
        install_into_dirs(shared);
        assert_eq!(std::fs::read(&target).unwrap(), written, "a second boot leaves the file alone");
        // The file in the shared directory decides omo versus pi from its own process, so either
        // declared payload reports the pane's real agent (ui/src/lib/agentStateExtension.test.ts).
        for agent in ["omo", "pi"] {
            let source = extension_source_for(agent).unwrap();
            assert!(source.contains("agent: reportedAgentId()"), "{agent}");
            assert!(source.contains("omo-ai[\\\\/]plugin"), "{agent}");
        }
    }

    #[test]
    fn removal_covers_the_senpi_dir_byte_exactly() {
        let root = temp_root("senpi-remove");
        let senpi = root.path().join(".senpi").join("agent").join("extensions");
        let upstream = root.path().join(".pi").join("agent").join("extensions");
        for dir in [&senpi, &upstream] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let dirs = extension_dirs_with_env(senpi_env(root.path(), None));
        install_into_dirs(dirs.clone());
        // A stale omo-identity file in the senpi dir is still Ferryx's; a user edit is not.
        std::fs::write(senpi.join(EXTENSION_FILE_NAME), extension_source_for("omo").unwrap()).unwrap();
        let edited = format!("{}// mine\n", extension_source_for("pi").unwrap());
        std::fs::write(upstream.join(EXTENSION_FILE_NAME), &edited).unwrap();
        let report = remove_from_dirs(dirs.into_iter().map(|(_, dir)| dir));
        assert_eq!(report.removed, vec![senpi.join(EXTENSION_FILE_NAME).to_string_lossy().into_owned()], "{report:?}");
        assert_eq!(report.kept_modified, vec![upstream.join(EXTENSION_FILE_NAME).to_string_lossy().into_owned()]);
        assert!(senpi.is_dir());
        assert_eq!(std::fs::read_to_string(upstream.join(EXTENSION_FILE_NAME)).unwrap(), edited);
    }
}
