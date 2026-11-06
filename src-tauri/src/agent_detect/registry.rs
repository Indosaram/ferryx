//! One table per coding agent.
//!
//! Every list Ferryx keeps about an agent - the pane-process discovery allowlist, the launch-argv
//! matcher, the resume flags and id shapes, the agents whose CLI leaves no process-bound session,
//! the transcript store roots and the launcher variants - is derived from AGENTS here. Adding an
//! agent means one entry in this table plus its resume arm in terminal::shell (which needs that
//! agent's own CLI contract), never a hunt through five modules.
//!
//! Activity detection is deliberately NOT part of this table: agent_detect::SHIPPED_MANIFESTS holds
//! screen rules measured per TUI, and an agent that runs another agent's TUI (maho runs omo's) is
//! detected by that manifest instead of by a duplicated one.

/// How a live session id is discovered for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discovery {
    /// The agent's session environment, then a session file held open (omo, maho).
    EnvironmentThenOpenFiles,
    /// A store the agent writes outside any open handle (antigravity).
    AntigravityStore,
    /// The agent's project store, read without an open handle (opencode).
    OpencodeStore,
    /// The senpi/pi store named by PI_SESSION_FILE or an open file (pi).
    PiStore,
    /// An open session file, else the resume flag in the launch argv.
    OpenFilesThenResumeArgv,
    /// Only the resume flag in the launch argv (crush).
    ResumeArgvOnly,
    /// An open session file, falling back to the pane's root process.
    OpenFiles,
}

/// The shape an agent's session id is accepted in. The validators live in ipc::agents; this table
/// only decides which one applies, so a new agent reuses an existing shape instead of a new parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdShape {
    Uuid,
    /// A UUID, or a transcript-path spelling of one (omp).
    OmpPathOrUuid,
    /// cline's epoch_base36 session id.
    ClineSession,
    /// crush's 16-hex XXH3 prefix of the session UUID, or the UUID itself.
    CrushHashOrUuid,
    /// The 16-hex suffix of the agent's store file (gjc).
    Hex16,
}

pub struct AgentSpec {
    /// The agent id the rest of Ferryx uses (discovery, resume, machine admission, UI).
    pub id: &'static str,
    pub display_name: &'static str,
    /// Executable and entry-point basenames that launch this agent, lowercase. These name the
    /// agent both in a pane's foreground process and in a launch argv.
    pub command_names: &'static [&'static str],
    /// Names accepted only in a launch argv, never as a pane's foreground executable: a binary
    /// the agent's CLI shares with an unrelated program that a pane may also be running.
    pub argv_only_names: &'static [&'static str],
    /// Extra ids this agent answers to.
    pub aliases: &'static [&'static str],
    /// Package directories that identify the agent behind a generic entry point (\`cli.js\`).
    pub package_names: &'static [&'static str],
    /// Lowercase substrings of a launch argv that name this agent when its executable does not
    /// (a bundled entry script, an npm shim's payload).
    pub argv_markers: &'static [&'static str],
    /// Launch argv substrings that make a match false: another instance of the same binary that
    /// serves other panes rather than running this agent.
    pub excluded_args: &'static [&'static str],
    /// The agent runs as an interpreted entry point (bun/node/python plus its script).
    pub interpreted_entry: bool,
    pub discovery: Discovery,
    /// Flags whose value is the conversation, in the launch argv.
    pub resume_flags: &'static [&'static str],
    pub session_id_shape: SessionIdShape,
    /// The CLI leaves no process-bound record of a fresh session, so its store file plus the
    /// store-recorded cwd stand in for that proof.
    pub no_process_bound_session: bool,
    /// Path segments that identify this agent's transcript store on disk.
    pub store_markers: &'static [&'static str],
    /// The binary a resume spawns when the session arrives without a launcher.
    pub resume_program: &'static str,
    /// The flags a resume passes before the id.
    pub resume_args: &'static [&'static str],
    /// Launcher variants of this agent: (wire name, binary). A session carrying a launcher is only
    /// valid for an agent listed here.
    pub launch_variants: &'static [(&'static str, &'static str)],
}
pub const AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: "claude",
        display_name: "Claude Code",
        command_names: &["claude"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume", "-r", "--session-id"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: true,
        store_markers: &["/.claude/projects/"],
        resume_program: "claude",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "codex",
        display_name: "Codex",
        command_names: &["codex"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFiles,
        resume_flags: &[],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.codex/sessions/"],
        resume_program: "codex",
        resume_args: &["resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "copilot",
        display_name: "Copilot",
        command_names: &["copilot"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.copilot/session-state/"],
        resume_program: "copilot",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "cursor",
        display_name: "Cursor",
        command_names: &["cursor-agent", "agent"],
        // A pane running the bare `cursor` binary is the Cursor editor, not its agent, so that
        // name is argv-only: `cursor --resume <id>` still identifies a resumed agent.
        argv_only_names: &["cursor"],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFiles,
        resume_flags: &[],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.cursor/chats/"],
        resume_program: "cursor-agent",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "kimi",
        display_name: "Kimi",
        command_names: &["kimi"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: true,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--session", "-S"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.kimi/sessions/"],
        resume_program: "kimi",
        resume_args: &["--session"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "omo",
        display_name: "OMO",
        command_names: &["omo"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &["omo-ai"],
        argv_markers: &["/omo.js", "/senpi/dist/cli.js"],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::EnvironmentThenOpenFiles,
        resume_flags: &["--session"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.omo/agent/sessions/", "/.omo/sessions/"],
        resume_program: "omo",
        resume_args: &["--session"],
        launch_variants: &[("mhc", "mhc")],
    },
    AgentSpec {
        id: "maho",
        display_name: "Maho",
        command_names: &["mhc"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::EnvironmentThenOpenFiles,
        resume_flags: &["--session"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.maho/agent/sessions/"],
        resume_program: "mhc",
        resume_args: &["--session"],
        launch_variants: &[("mhc", "mhc")],
    },
    AgentSpec {
        id: "gjc",
        display_name: "Gajae Code",
        command_names: &["gjc"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &["/gjc.js"],
        excluded_args: &[],
        interpreted_entry: true,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume"],
        session_id_shape: SessionIdShape::Hex16,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "gjc",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "antigravity",
        display_name: "Antigravity",
        command_names: &["agy"],
        argv_only_names: &[],
        aliases: &["agy", "antigravity-cli"],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::AntigravityStore,
        resume_flags: &["--conversation"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &["/.gemini/antigravity-cli/conversations/"],
        resume_program: "agy",
        resume_args: &["--conversation"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "opencode",
        display_name: "OpenCode",
        command_names: &["opencode"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpencodeStore,
        resume_flags: &["--session"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "opencode",
        resume_args: &["--session"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "pi",
        display_name: "Pi",
        command_names: &["pi", "pi.js"],
        argv_only_names: &[],
        aliases: &["senpi"],
        package_names: &["@code-yeongyu/senpi"],
        argv_markers: &["/pi-coding-agent"],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::PiStore,
        resume_flags: &["--session"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "pi",
        resume_args: &["--session"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "omp",
        display_name: "OMP",
        command_names: &["omp"],
        argv_only_names: &[],
        aliases: &["oh-my-pi"],
        package_names: &["oh-my-pi", "omp"],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: true,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume"],
        session_id_shape: SessionIdShape::OmpPathOrUuid,
        no_process_bound_session: true,
        store_markers: &[],
        resume_program: "omp",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "grok",
        display_name: "Grok",
        command_names: &["grok"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume", "-r"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "grok",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "droid",
        display_name: "Droid",
        command_names: &["droid"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--resume", "-r", "resume"],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "droid",
        resume_args: &["--resume"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "cline",
        display_name: "Cline",
        command_names: &["cline"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &["--cline-hub-daemon"],
        interpreted_entry: false,
        discovery: Discovery::OpenFilesThenResumeArgv,
        resume_flags: &["--id"],
        session_id_shape: SessionIdShape::ClineSession,
        no_process_bound_session: true,
        store_markers: &[],
        resume_program: "cline",
        resume_args: &["--id"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "crush",
        display_name: "Crush",
        command_names: &["crush"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: false,
        discovery: Discovery::ResumeArgvOnly,
        resume_flags: &["--session", "-s"],
        session_id_shape: SessionIdShape::CrushHashOrUuid,
        no_process_bound_session: true,
        store_markers: &[],
        resume_program: "crush",
        resume_args: &["--session"],
        launch_variants: &[],
    },
    AgentSpec {
        id: "aider",
        display_name: "Aider",
        command_names: &["aider"],
        argv_only_names: &[],
        aliases: &[],
        package_names: &[],
        argv_markers: &[],
        excluded_args: &[],
        interpreted_entry: true,
        discovery: Discovery::OpenFiles,
        resume_flags: &[],
        session_id_shape: SessionIdShape::Uuid,
        no_process_bound_session: false,
        store_markers: &[],
        resume_program: "aider",
        resume_args: &[],
        launch_variants: &[],
    },
];

pub fn by_id(id: &str) -> Option<&'static AgentSpec> {
    AGENTS.iter().find(|spec| spec.id.eq_ignore_ascii_case(id.trim()))
}

/// The spec an agent id, alias or command name resolves to.
pub fn resolve(value: &str) -> Option<&'static AgentSpec> {
    let normalized = value.trim();
    AGENTS.iter().find(|spec| {
        spec.id.eq_ignore_ascii_case(normalized)
            || spec
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(normalized))
            || spec
                .command_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(normalized))
    })
}

/// Whether Ferryx tracks a pane session for this agent at all.
pub fn is_known(id: &str) -> bool {
    by_id(id).is_some()
}

pub fn discovery(id: &str) -> Option<Discovery> {
    by_id(id).map(|spec| spec.discovery)
}

pub fn session_id_shape(id: &str) -> Option<SessionIdShape> {
    by_id(id).map(|spec| spec.session_id_shape)
}

pub fn resume_flags(id: &str) -> &'static [&'static str] {
    by_id(id).map(|spec| spec.resume_flags).unwrap_or(&[])
}

pub fn no_process_bound_session(id: &str) -> bool {
    by_id(id).is_some_and(|spec| spec.no_process_bound_session)
}

pub fn store_markers(id: &str) -> Option<&'static [&'static str]> {
    by_id(id)
        .map(|spec| spec.store_markers)
        .filter(|markers| !markers.is_empty())
}

/// The (program, args) a resume spawns when the caller has no launcher binary of its own.
pub fn resume_plan(id: &str) -> Option<(&'static str, &'static [&'static str])> {
    let spec = by_id(id)?;
    (!spec.resume_program.is_empty()).then_some((spec.resume_program, spec.resume_args))
}

/// Whether a session carrying this launcher may belong to the agent, and the binary it names.
pub fn launch_binary(id: &str, launcher: &str) -> Option<&'static str> {
    by_id(id)?.launch_variants.iter().find_map(|(wire, binary)| {
        wire.eq_ignore_ascii_case(launcher).then_some(*binary)
    })
}

/// Every agent id in the table, for callers that need the list itself.
pub fn ids() -> impl Iterator<Item = &'static str> {
    AGENTS.iter().map(|spec| spec.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maho_is_its_own_agent_with_its_own_store() {
        let maho = by_id("maho").expect("maho must be a first-class agent");
        assert_eq!(maho.resume_program, "mhc");
        assert_eq!(maho.command_names, ["mhc"]);
        assert_eq!(maho.store_markers, ["/.maho/agent/sessions/"]);
        assert!(!maho.store_markers.iter().any(|marker| marker.contains("/.omo")));
        assert_eq!(maho.launch_variants, [("mhc", "mhc")]);
        // The split is the point: the mhc binary is maho's, and omo only keeps the launcher so a
        // session recorded before the split still resumes.
        assert!(resolve("mhc").is_some_and(|spec| spec.id == "maho"));
    }

    #[test]
    fn omo_keeps_the_mhc_launcher_variant_for_sessions_recorded_before_the_split() {
        assert_eq!(launch_binary("omo", "mhc"), Some("mhc"));
        assert_eq!(launch_binary("maho", "mhc"), Some("mhc"));
        assert_eq!(launch_binary("claude", "mhc"), None);
    }

    #[test]
    fn every_listed_agent_resolves_by_its_own_id_and_commands() {
        for spec in AGENTS {
            assert_eq!(resolve(spec.id).map(|found| found.id), Some(spec.id));
            for name in spec.command_names {
                assert_eq!(resolve(name).map(|found| found.id), Some(spec.id));
            }
        }
    }

    #[test]
    fn agent_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for spec in AGENTS {
            assert!(seen.insert(spec.id), "duplicate agent id {}", spec.id);
        }
    }

    #[test]
    fn only_agents_without_process_bound_sessions_are_marked() {
        let marked: Vec<&str> = AGENTS
            .iter()
            .filter(|spec| spec.no_process_bound_session)
            .map(|spec| spec.id)
            .collect();
        assert_eq!(marked, ["claude", "omp", "cline", "crush"]);
    }

    #[test]
    fn the_cursor_editor_binary_names_the_agent_only_in_a_launch_argv() {
        // A pane running the Cursor editor is not a cursor-agent pane; only `cursor --resume <id>`
        // identifies a resumed agent, which is what argv matching reads.
        let cursor = by_id("cursor").expect("cursor must be listed");
        assert!(cursor.command_names.contains(&"cursor-agent"));
        assert!(!cursor.command_names.contains(&"cursor"));
        assert!(cursor.argv_only_names.contains(&"cursor"));
        assert!(resolve("cursor-agent").is_some_and(|spec| spec.id == "cursor"));
    }

    #[test]
    fn a_command_name_belongs_to_exactly_one_agent() {
        for spec in AGENTS {
            for name in spec.command_names {
                let owners: Vec<&str> = AGENTS
                    .iter()
                    .filter(|other| other.command_names.contains(name))
                    .map(|other| other.id)
                    .collect();
                assert_eq!(owners, [spec.id], "command {name} is claimed by {owners:?}");
            }
        }
    }
}
