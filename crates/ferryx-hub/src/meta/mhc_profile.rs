//! Launch profile and durable session identities of the retained mhc supervisor engine.
//!
//! The engine always runs as `mhc --mode rpc --multi-session --no-model-fallback
//! --ttsr-rules-disabled=repetitive-turns --omo-senpi-onboarding-disabled` over stdio, in
//! the fixed profile directory `<data>/mhc-supervisor`, whatever directory the hub itself was
//! started from. Its native memory identity is pinned by a profile-local `.omo/omo.jsonc`; no
//! global configuration is written. Session roles get their policy from host-supplied session
//! context only, so transcript text can never widen a session's authority.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::Result;

/// Directory under the hub data directory the engine runs in.
pub const PROFILE_DIRNAME: &str = "mhc-supervisor";
/// Instructions the engine reads from its own working directory into every session's system
/// prompt (mhc `maho-core/src/context_files.rs`, the `AGENTS.md` context file). The engine's
/// supervisor work tools are built in, but nothing in the engine says what the *hub* expects of
/// them: an owner message that hands over a session is meant to arrive as a `supervisor_submit_work`
/// call, and only the hub knows the phrases and the addressed-session trailer that make that
/// request a handover. Teaching it here is the whole of the contract, so no engine release is
/// needed to change it, and a pinned engine that predates any of this still learns it.
pub const PROFILE_INSTRUCTIONS: &str = "\
# Ferryx supervisor

This file is the hub's contract for the sessions it opens in this directory. The section on
supervisor tools applies only to a session that has `supervisor_submit_work` and
`supervisor_work_status`; a session without them (a decision, convergence or memory turn) has no
work to submit and ignores it.

## The supervisor tools

You are the Ferryx supervisor. You answer the owner in the app chat. Whatever else such a session
can do, the hub gives you two authorities: `supervisor_submit_work` starts or continues work, and
`supervisor_work_status` reads it back. Use them for anything about work; do not describe work you
have not submitted through the tool.

## Handing over a session the owner already runs

Some owner messages do not ask for new work: they hand you an agent session the owner is already
running. The app always marks one the same way. The owner's own text says \"이 세션 맡아\" or
\"take this session\", and the message ends with a trailer naming the sessions the owner addressed:

    [The owner addressed these sessions with @: local:<session-id>]

For such a message, call `supervisor_submit_work` exactly once, with:

- `title`: two to five words naming what the work continues, such as \"Continue the release fix\".
- `requirements`: the owner's instruction, restated as one checkable requirement.
- `worker`: \"any\".

The hub reads the trailer, binds the goal to that pane and observes it; it does not need you to
read the pane, to type into it, or to reach it in any other way. So never report that you cannot
reach a session while `supervisor_submit_work` is available: that tool *is* the way to reach it.
Do not ask which session either. The trailer names it, and a message naming several or none is
answered by asking the owner, not by guessing.

## Pausing and resuming work you own

An owner message saying \"이 세션 멈춰\" or \"pause this session\" pauses the loop of the session it
addresses, and one saying \"이 세션 계속\" or \"resume this session\" resumes it. Submit those through
`supervisor_submit_work` as well; the hub recognizes them from the owner's words.

## Reading work back

`supervisor_work_status` reports the admitted work of this conversation. Pass the `goal_id` of an
earlier receipt to read one goal instead.
";
/// Bare executable name, looked up on the hub child PATH when neither `FERRYX_HUB_MHC_BIN` nor
/// a bundled mhc is found ([`MhcProfileConfig::from_env_in`] defaults to the bundled one).
pub const DEFAULT_MHC_BIN: &str = "mhc";
/// Arguments of the owned engine. `--no-model-fallback` keeps the engine from substituting a
/// model on errors; the supervisor never does either.
///
/// `--ttsr-rules-disabled=repetitive-turns` turns off only the engine's repeated-turn guard
/// (mhc maho-ext-ttsr index.rs:22-23,54, repetitive_turns_lane.rs:9,14-18): a durable decision
/// session legitimately answers the same verdict for an unchanged goal, and that guard aborts
/// a streamed turn that repeats the previous persisted answer, so every repeated verdict was
/// lost as a fault. The other stream rules stay on.
///
/// `--omo-senpi-onboarding-disabled` turns off the omo onboarding component (mhc
/// maho-omo-onboarding component.rs:15-20): on the first session it ever opens it triggers its
/// own model turn ("Read the onboarding skill ... Greet the user first.") in the supervisor's
/// identity, which a supervisor session has no use for.
pub const LAUNCH_ARGS: [&str; 6] = [
    "--mode",
    "rpc",
    "--multi-session",
    "--no-model-fallback",
    "--ttsr-rules-disabled=repetitive-turns",
    "--omo-senpi-onboarding-disabled",
];
/// Capabilities the engine must advertise before any session opens: the host baseline the
/// client relies on, then the plan's supervisor capabilities. `memory_forget` also stands for
/// `memory_correct`; there is no separate capability to guess. `memory_write_deny` alone does
/// not pin which tools a denied session loses (mhc 8c81d1a09 advertised it before the effect
/// exclusions existed), so [`MEMORY_WRITE_DENY_EFFECTS_CAPABILITY`] is required as well.
pub const REQUIRED_CAPABILITIES: [&str; 9] = [
    "multi_session",
    "session_context",
    "durable_session_id",
    "memory_write_deny",
    MEMORY_WRITE_DENY_EFFECTS_CAPABILITY,
    "tools_exclude",
    "memory_receipts",
    "memory_forget",
    "supervisor_work_tools",
];
/// The engine guarantees that a `memory_write = deny` session never registers the effect set
/// of mhc `MEMORY_WRITE_DENY_EXCLUDED_TOOL_NAMES` (shells and process steering, `eval`,
/// `apply_patch`, the LSP/ast-grep writers, `generate_image`, `computer`, the child-session
/// spawners) nor the `task_*`/`team_*` families (mhc 3584f7d4 semantics). Decision,
/// convergence and conversation sessions rely on it.
pub const MEMORY_WRITE_DENY_EFFECTS_CAPABILITY: &str = "memory_write_deny_effects";
/// The engine honors `context.effect_tools = "deny"`, withholding that same effect set (prefix
/// families included) from a session whose memory writes stay allowed. The owner-ingest session
/// requires it: without it owner ingest fails as `MHC_CAPABILITY_MISSING` before any session
/// opens. There is no explicit fallback list, which could not name the prefix families and
/// which mhc refuses when it names a tool the build does not register. Other roles do not
/// need it.
pub const EFFECT_TOOLS_DENY_CAPABILITY: &str = "effect_tools_deny";
/// Session context key of [`EFFECT_TOOLS_DENY_CAPABILITY`].
pub const EFFECT_TOOLS_CONTEXT_KEY: &str = "effect_tools";
/// Hub env key that turns the engine on. Turned on from the hub env file only, which every hub
/// process (daemon, MCP server, CLI) shares; one process's environment can only turn it off
/// for that process (see [`MhcProfileConfig::from_env_in`]).
pub const ENABLE_KEY: &str = "FERRYX_HUB_MHC_SUPERVISOR";
/// Lock file under the hub data directory held by the process that owns the engine, from
/// before its launch until the owned child is reaped.
pub const LOCK_FILENAME: &str = "mhc-supervisor.lock";
/// Client capability that makes the host forward extension RPC events (the work-tool requests).
pub const CLIENT_CAPABILITIES: [&str; 1] = ["extension_events"];
/// Environment variables the engine's stdio host reads its client capabilities from (mhc
/// maho-rpc custom_capability.rs:50, `env_value("RPC_CLIENT_CAPABILITIES")` over the brand
/// prefixes MAHO, SENPI, PI, maho-core brand.rs:173,209-236). On stdio there is no
/// connection, so `set_client_info` registers nothing (multi_session_host.rs:245-253): every
/// session binding, including each worker's (multi_session_host.rs:388,396), takes the host
/// options' capabilities parsed from this variable (host_runtime.rs:643-645,672-674). Without
/// it a conversation session never forwards `supervisor.work_request`, and its work tool
/// waits for a reply that cannot come. Every prefix is set, so an inherited value under any
/// of them cannot shadow the hub's.
pub const CLIENT_CAPABILITIES_ENV: [&str; 3] = [
    "MAHO_RPC_CLIENT_CAPABILITIES",
    "SENPI_RPC_CLIENT_CAPABILITIES",
    "PI_RPC_CLIENT_CAPABILITIES",
];
/// Tools no decision, convergence, conversation or ingest session may register: terminal
/// commands and file mutation. Memory write tools are withheld by `memory_write = deny`.
pub const DEFAULT_EFFECT_TOOLS: [&str; 3] = ["bash", "edit", "write"];
/// Tools a decision session additionally never registers: it only judges, so it must not
/// create or update a goal (the engine's goal extension, maho-ext-goal extension.rs:24-37) or
/// publish browser state to the app (maho-omo-browser-bridge index.rs:9,97-124). `get_goal`
/// reads and stays.
pub const DECISION_EXCLUDED_TOOLS: [&str; 3] = ["create_goal", "update_goal", "omo_browser_bridge"];
/// Tools a conversation session additionally never registers: its turn answers a chat message
/// and nobody can answer an in-engine question there, so the turn would block until its
/// deadline. The conversation asks in its reply instead.
pub const CONVERSATION_EXCLUDED_TOOLS: [&str; 1] = ["ask_user_question"];
/// Concurrent decision contexts.
pub const DECISION_SLOTS: usize = 4;
/// Most recent owner source message ids remembered for idempotent ingest.
pub const INGESTED_IDS_KEPT: usize = 2048;
/// Session `context` limits of the engine (`rpc_input_validation.rs`).
pub const CONTEXT_MAX_KEYS: usize = 32;
pub const CONTEXT_MAX_VALUE_BYTES: usize = 16 * 1024;
pub const CONTEXT_MAX_TOTAL_BYTES: usize = 32 * 1024;

/// Operator configuration, read from the hub environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MhcProfileConfig {
    pub binary: String,
    /// Provider/model of conversation sessions (the owner's own conversations). Never changed
    /// at runtime; `None` leaves the engine default.
    pub conversation_model: Option<(String, String)>,
    /// Provider/model of every other role: owner ingest, decision and convergence. Never
    /// changed at runtime; `None` leaves the engine default.
    pub background_model: Option<(String, String)>,
    pub thinking_level: Option<String>,
    /// Tool names withheld from every supervisor session, [`DEFAULT_EFFECT_TOOLS`] included.
    pub effect_tools: Vec<String>,
    pub timeouts: MhcTimeouts,
}

/// Bounded deadlines of the engine client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MhcTimeouts {
    /// One command and its response.
    pub request: Duration,
    /// One prompt until `agent_settled`.
    pub turn: Duration,
    /// Waiting for an aborted turn to settle before its context is reused.
    pub drain: Duration,
    /// Longest accepted JSONL line from the engine.
    pub max_line_bytes: usize,
}

impl Default for MhcTimeouts {
    fn default() -> Self {
        Self {
            request: Duration::from_secs(30),
            turn: Duration::from_secs(600),
            drain: Duration::from_secs(30),
            max_line_bytes: 8 * 1024 * 1024,
        }
    }
}

impl Default for MhcProfileConfig {
    fn default() -> Self {
        Self {
            binary: DEFAULT_MHC_BIN.to_owned(),
            conversation_model: None,
            background_model: None,
            thinking_level: None,
            effect_tools: DEFAULT_EFFECT_TOOLS
                .iter()
                .map(|tool| (*tool).to_owned())
                .collect(),
            timeouts: MhcTimeouts::default(),
        }
    }
}

/// `key` from this process's environment, else from the hub env file entries `shared`.
fn env_text(key: &str, shared: &BTreeMap<String, String>) -> Option<String> {
    std::env::var(key)
        .ok()
        .or_else(|| shared.get(key).cloned())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// `provider/model` split at the first `/`; `None` when either side is empty.
fn split_model(value: &str) -> Option<(String, String)> {
    let (provider, model) = value.trim().split_once('/')?;
    let (provider, model) = (provider.trim(), model.trim());
    (!provider.is_empty() && !model.is_empty()).then(|| (provider.to_owned(), model.to_owned()))
}

/// The explicit `provider_key` + `model_key` pair. Both or neither: a half-set pair is ignored
/// with a warning.
fn explicit_pair(
    provider_key: &str,
    model_key: &str,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Option<(String, String)> {
    match (lookup(provider_key), lookup(model_key)) {
        (Some(provider), Some(model)) => Some((provider, model)),
        (None, None) => None,
        _ => {
            tracing::warn!(
                "{provider_key} and {model_key} must be set together; the pair was ignored"
            );
            None
        }
    }
}

/// `key` (`provider/model`) from `lookup`; a value without both parts is ignored with a
/// warning.
fn split_route(key: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Option<(String, String)> {
    let value = lookup(key)?;
    let route = split_model(&value);
    if route.is_none() {
        tracing::warn!(value = %value, "{key} is not provider/model; the supervisor engine ignored it");
    }
    route
}

/// 1/true/yes/on, any case, surrounding whitespace ignored.
fn is_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// 0/false/no/off, any case, surrounding whitespace ignored.
fn is_off(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// A tool name the engine accepts in `tools_exclude` (`^[A-Za-z0-9_.-]{1,64}$`).
pub fn is_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl MhcProfileConfig {
    /// True when the shared hub env (the hub env file entries) turns the engine on
    /// (1/true/yes/on).
    pub fn enabled_in(shared: &BTreeMap<String, String>) -> bool {
        shared.get(ENABLE_KEY).is_some_and(|value| is_on(value))
    }

    /// [`Self::from_env_in`] over the hub env file entries as they are on disk now (re-read on
    /// every call, so a hub generation restarted after the file changed sees the change) and
    /// this process's [`ENABLE_KEY`].
    pub fn from_env() -> Option<Self> {
        let process = std::env::var(ENABLE_KEY).ok();
        Self::from_env_in(&crate::hub_env::read_file_env(), process.as_deref())
    }

    /// `None` unless [`ENABLE_KEY`] in the shared hub env file entries (`shared`) turns the
    /// engine on, so every hub process agrees on whether the supervisor is mhc. An explicit
    /// disable (0/false/no/off) in this process's environment (`process`) wins over the file
    /// and keeps this process from launching the engine; the shared file still names mhc, so
    /// its judge holds as `MHC_NOT_RUNNING` instead of switching to another model. A process
    /// value never turns the engine on. `FERRYX_HUB_MHC_BIN`, the per-role models of
    /// [`Self::route_models`], `FERRYX_HUB_MHC_THINKING` and `FERRYX_HUB_MHC_EFFECT_TOOLS`
    /// (extra comma-separated names)
    /// refine it for the owning daemon (this process's environment first, then `shared`).
    /// Without `FERRYX_HUB_MHC_BIN` the engine is the mhc bundled next to the running
    /// executable, else bare `mhc` on the hub child PATH
    /// ([`crate::meta::supervisor::resolve_mhc_bin`]).
    pub fn from_env_in(shared: &BTreeMap<String, String>, process: Option<&str>) -> Option<Self> {
        let process_off = process.is_some_and(is_off);
        if !Self::enabled_in(shared) {
            if process.is_some_and(|value| !value.trim().is_empty() && !is_off(value)) {
                tracing::warn!(
                    "FERRYX_HUB_MHC_SUPERVISOR turns the engine on from the hub env file only; the process environment value was ignored"
                );
            }
            return None;
        }
        if process_off {
            tracing::warn!(
                "FERRYX_HUB_MHC_SUPERVISOR in the process environment disables the engine the hub env file turns on; supervisor decisions hold as MHC_NOT_RUNNING"
            );
            return None;
        }
        let mut config = Self {
            binary: crate::meta::supervisor::resolve_mhc_bin(
                env_text("FERRYX_HUB_MHC_BIN", shared),
                std::env::current_exe().ok().as_deref(),
            ),
            ..Self::default()
        };
        config.route_models(&|key: &str| env_text(key, shared));
        config.thinking_level = env_text("FERRYX_HUB_MHC_THINKING", shared);
        if let Some(extra) = env_text("FERRYX_HUB_MHC_EFFECT_TOOLS", shared) {
            config.add_effect_tools(&extra);
        }
        Some(config)
    }

    /// Resolves the model of each role from `lookup` (trimmed, non-empty values). Conversation:
    /// `FERRYX_HUB_MHC_PROVIDER` + `FERRYX_HUB_MHC_MODEL`, else `OMON_DEFAULT_MODEL`. Owner
    /// ingest, decision and convergence: `FERRYX_HUB_MHC_BACKGROUND_PROVIDER` +
    /// `FERRYX_HUB_MHC_BACKGROUND_MODEL`, else `OMON_BACKGROUND_MODEL`
    /// ([`crate::model_policy::background_model`]). Each explicit pair is both or neither: a
    /// half-set pair is ignored with a warning and the `OMON_*` model applies. With neither,
    /// the role runs on the engine default.
    pub fn route_models(&mut self, lookup: &dyn Fn(&str) -> Option<String>) {
        self.conversation_model =
            explicit_pair("FERRYX_HUB_MHC_PROVIDER", "FERRYX_HUB_MHC_MODEL", lookup)
                .or_else(|| split_route("OMON_DEFAULT_MODEL", lookup));
        self.background_model = explicit_pair(
            "FERRYX_HUB_MHC_BACKGROUND_PROVIDER",
            "FERRYX_HUB_MHC_BACKGROUND_MODEL",
            lookup,
        )
        .or_else(|| split_route("OMON_BACKGROUND_MODEL", lookup));
    }

    /// Adds valid comma-separated tool names; the defaults are never removed.
    pub fn add_effect_tools(&mut self, extra: &str) {
        for name in extra
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            if !is_tool_name(name) {
                tracing::warn!(tool = %name, "ignored an invalid supervisor effect tool name");
                continue;
            }
            if !self.effect_tools.iter().any(|existing| existing == name) {
                self.effect_tools.push(name.to_owned());
            }
        }
    }
}

/// A conversation scope: the project and channel binding the hub resolved for the session.
/// It is derived by the hub, never from model or transcript text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationScope {
    pub project_id: Option<String>,
    /// Provider, workspace, account and channel/thread of the binding, as one stable key.
    pub channel_key: String,
}

impl ConversationScope {
    /// Stable opaque key of the scope; the raw channel identifiers never reach the engine.
    pub fn key(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.project_id.as_deref().unwrap_or("").as_bytes());
        digest.update([0u8]);
        digest.update(self.channel_key.as_bytes());
        let hash = digest.finalize();
        hash.iter()
            .take(16)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

/// What a supervisor session is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRole {
    /// The one serialized session that learns from authenticated owner-authored text.
    OwnerIngest,
    /// A retained per-scope conversation; memory writes denied, typed work tools allowed.
    Conversation(ConversationScope),
    /// One of the bounded decision slots; memory writes denied, no effect tools.
    Decision(usize),
    /// A fresh one-shot decision context for a convergence check.
    Convergence,
}

/// The resolved session policy of one role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSpec {
    /// Durable key in the session store (`owner-ingest`, `decision-0`, `conversation-<hash>`).
    pub key: String,
    pub role: &'static str,
    /// Host-supplied session context: the only source of the session's policy.
    pub context: BTreeMap<String, String>,
    /// Resumed across restarts under a persisted identity.
    pub durable: bool,
    /// Owner cancellation reaches it.
    pub cancellable: bool,
    /// Provider/model the role's sessions open with and its answers must come from
    /// ([`MhcProfile::model_for`]); `None` is the engine default.
    pub model: Option<(String, String)>,
}

/// Paths and policy of the owned engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MhcProfile {
    state_dir: PathBuf,
    cwd: PathBuf,
    pub config: MhcProfileConfig,
}

impl MhcProfile {
    /// Profile under the hub data directory. A relative data directory is resolved against the
    /// current directory once, here; everything after uses the absolute result.
    pub fn new(data_dir: &Path, config: MhcProfileConfig) -> Self {
        let parent = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::resolve(data_dir, &parent, config)
    }

    /// [`Self::new`] with the parent directory given explicitly.
    pub fn resolve(data_dir: &Path, parent_cwd: &Path, config: MhcProfileConfig) -> Self {
        let state_dir = if data_dir.is_absolute() {
            data_dir.to_path_buf()
        } else {
            parent_cwd.join(data_dir)
        };
        Self {
            cwd: state_dir.join(PROFILE_DIRNAME),
            state_dir,
            config,
        }
    }

    /// The engine's working directory, `<data>/mhc-supervisor`.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn sessions_path(&self) -> PathBuf {
        self.state_dir.join("mhc-supervisor-sessions.json")
    }

    pub fn pid_path(&self) -> PathBuf {
        self.state_dir.join("mhc-supervisor.pid")
    }

    pub fn log_path(&self) -> PathBuf {
        self.state_dir.join("mhc-supervisor.log")
    }

    /// `<data>/mhc-supervisor.lock`, the cross-process engine ownership lock.
    pub fn lock_path(&self) -> PathBuf {
        self.state_dir.join(LOCK_FILENAME)
    }

    /// Takes the exclusive OS lock on [`Self::lock_path`] without waiting. `Ok(None)` while
    /// another holder (any process, or another handle in this one) owns it.
    pub fn try_lock(&self) -> std::io::Result<Option<ProfileLock>> {
        std::fs::create_dir_all(&self.state_dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.lock_path())?;
        match file.try_lock() {
            Ok(()) => Ok(Some(ProfileLock { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Profile-local omo config that pins the native memory identity.
    pub fn memory_config_path(&self) -> PathBuf {
        self.cwd.join(".omo").join("omo.jsonc")
    }

    pub fn launch_args(&self) -> Vec<String> {
        LAUNCH_ARGS.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Environment the engine is launched with on top of the hub child environment.
    pub fn launch_env(&self) -> Vec<(String, String)> {
        CLIENT_CAPABILITIES_ENV
            .iter()
            .map(|name| ((*name).to_owned(), CLIENT_CAPABILITIES.join(",")))
            .collect()
    }

    /// Required capabilities the engine did not advertise, in [`REQUIRED_CAPABILITIES`] order.
    pub fn missing_capabilities(&self, advertised: &[String]) -> Vec<String> {
        REQUIRED_CAPABILITIES
            .iter()
            .filter(|required| !advertised.iter().any(|name| name == *required))
            .map(|required| (*required).to_owned())
            .collect()
    }

    /// Capabilities beyond [`REQUIRED_CAPABILITIES`] that the role of `spec` needs and
    /// `advertised` lacks: the owner-ingest session needs [`EFFECT_TOOLS_DENY_CAPABILITY`].
    pub fn missing_role_capabilities(&self, spec: &RoleSpec, advertised: &[String]) -> Vec<String> {
        let needed: &[&str] = if spec.role == "owner_ingest" {
            &[EFFECT_TOOLS_DENY_CAPABILITY]
        } else {
            &[]
        };
        needed
            .iter()
            .filter(|needed| !advertised.iter().any(|name| name == *needed))
            .map(|needed| (*needed).to_owned())
            .collect()
    }

    /// `<profile>/AGENTS.md`: the hub's supervisor contract, read into every session's system
    /// prompt by the engine.
    pub fn instructions_path(&self) -> PathBuf {
        self.cwd.join("AGENTS.md")
    }

    /// Creates the profile directory (owner-only on unix), the profile-local memory identity
    /// config and the instructions the engine loads from its working directory. Nothing outside
    /// `<data>` is written.
    pub fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(&self.cwd)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.cwd, std::fs::Permissions::from_mode(0o700))?;
        }
        let path = self.memory_config_path();
        let wanted = memory_identity_config();
        if std::fs::read_to_string(&path).ok().as_deref() != Some(wanted.as_str()) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, wanted)?;
        }
        // Rewritten whenever the shipped contract changes, so a hub upgrade reaches engines
        // already running out of this profile directory.
        let instructions = self.instructions_path();
        if std::fs::read_to_string(&instructions).ok().as_deref() != Some(PROFILE_INSTRUCTIONS) {
            std::fs::write(&instructions, PROFILE_INSTRUCTIONS)?;
        }
        Ok(())
    }

    /// Owner-only append log for the engine's stderr, rotated past 10 MiB.
    pub fn open_log(&self) -> Option<std::fs::File> {
        let path = self.log_path();
        std::fs::create_dir_all(&self.state_dir).ok()?;
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 10 * 1024 * 1024) {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            let file = options.open(&path).ok()?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .ok()?;
            Some(file)
        }
        #[cfg(not(unix))]
        {
            options.open(&path).ok()
        }
    }

    /// Records the owned child as `<pid> <hub pid>`.
    pub fn record_pid(&self, pid: u32) -> Result<()> {
        std::fs::create_dir_all(&self.state_dir)?;
        std::fs::write(self.pid_path(), format!("{pid} {}", std::process::id()))?;
        Ok(())
    }

    /// The recorded owned child, when the record belongs to this hub process.
    pub fn owned_pid(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.pid_path()).ok()?;
        let mut parts = text.split_whitespace();
        let pid = parts.next()?.parse().ok()?;
        let owner: u32 = parts.next()?.parse().ok()?;
        (owner == std::process::id()).then_some(pid)
    }

    pub fn forget_pid(&self, pid: u32) {
        if self.owned_pid() == Some(pid) {
            let _ = std::fs::remove_file(self.pid_path());
        }
    }

    /// The provider/model a role runs on: conversations on the owner's conversation model,
    /// every other role on the background model.
    pub fn model_for(&self, role: &SessionRole) -> Option<(String, String)> {
        match role {
            SessionRole::Conversation(_) => self.config.conversation_model.clone(),
            SessionRole::OwnerIngest | SessionRole::Decision(_) | SessionRole::Convergence => {
                self.config.background_model.clone()
            }
        }
    }

    fn effect_tools(&self) -> String {
        self.config.effect_tools.join(",")
    }

    /// The session policy of a role. The owner-ingest session learns (memory writes allowed)
    /// but asks the engine to withhold the whole effect set ([`EFFECT_TOOLS_DENY_CAPABILITY`]).
    pub fn role_spec(&self, role: &SessionRole) -> RoleSpec {
        let mut context = BTreeMap::new();
        let (key, name, durable, cancellable) = match role {
            SessionRole::OwnerIngest => {
                context.insert("tools_exclude".to_owned(), self.effect_tools());
                context.insert(EFFECT_TOOLS_CONTEXT_KEY.to_owned(), "deny".to_owned());
                ("owner-ingest".to_owned(), "owner_ingest", true, false)
            }
            SessionRole::Conversation(scope) => {
                let scope_key = scope.key();
                let tools = std::iter::once(self.effect_tools())
                    .chain(CONVERSATION_EXCLUDED_TOOLS.iter().map(|tool| (*tool).to_owned()))
                    .collect::<Vec<_>>()
                    .join(",");
                context.insert("tools_exclude".to_owned(), tools);
                context.insert("memory_write".to_owned(), "deny".to_owned());
                context.insert("supervisor_tools".to_owned(), "work".to_owned());
                context.insert("ferryx_scope".to_owned(), scope_key.clone());
                (
                    format!("conversation-{scope_key}"),
                    "conversation",
                    true,
                    false,
                )
            }
            SessionRole::Decision(slot) => {
                let tools = std::iter::once(self.effect_tools())
                    .chain(
                        DECISION_EXCLUDED_TOOLS
                            .iter()
                            .map(|tool| (*tool).to_owned()),
                    )
                    .collect::<Vec<_>>()
                    .join(",");
                context.insert("tools_exclude".to_owned(), tools);
                context.insert("memory_write".to_owned(), "deny".to_owned());
                (format!("decision-{slot}"), "decision", true, true)
            }
            SessionRole::Convergence => {
                context.insert("tools_exclude".to_owned(), self.effect_tools());
                context.insert("memory_write".to_owned(), "deny".to_owned());
                ("convergence".to_owned(), "convergence", false, true)
            }
        };
        context.insert("ferryx_role".to_owned(), name.to_owned());
        RoleSpec {
            key,
            role: name,
            context,
            durable,
            cancellable,
            model: self.model_for(role),
        }
    }

    /// `open_session` for a role. The engine's mixed wire casing is kept: `retain_on_disconnect`
    /// and `auto_title` are snake_case, the rest camelCase.
    pub fn open_session_command(
        &self,
        spec: &RoleSpec,
        durable_session_id: &str,
        session_path: Option<&str>,
    ) -> Value {
        let mut command = json!({
            "type": "open_session",
            "cwd": self.cwd.to_string_lossy(),
            "auto_title": false,
            "retain_on_disconnect": false,
            "durableSessionId": durable_session_id,
            "context": spec.context,
        });
        if let Some(path) = session_path {
            command["sessionPath"] = json!(path);
        }
        if let Some((provider, model)) = &spec.model {
            command["provider"] = json!(provider);
            command["modelId"] = json!(model);
        }
        if let Some(level) = &self.config.thinking_level {
            command["thinkingLevel"] = json!(level);
        }
        command
    }
}

/// The exclusive engine ownership lock; released when dropped.
#[derive(Debug)]
pub struct ProfileLock {
    _file: std::fs::File,
}

/// True while some holder owns `<state_dir>/mhc-supervisor.lock`: a live engine owner exists.
/// A missing or free lock file is false. The probe takes the lock only for an instant.
pub fn profile_lock_held(state_dir: &Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(state_dir.join(LOCK_FILENAME))
    else {
        return false;
    };
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// `{"memory":{"agent":"ferryx-supervisor"}}`: the omo memory component resolves the native
/// identity from this explicit value, so it is the same from any parent directory.
pub fn memory_identity_config() -> String {
    format!(
        "{}\n",
        json!({"memory": {"agent": crate::supervisor::MEMORY_NAME}})
    )
}

/// Engine session `context` limits: key shape, count and sizes.
pub fn context_error(context: &BTreeMap<String, String>) -> Option<String> {
    if context.len() > CONTEXT_MAX_KEYS {
        return Some(format!("context has more than {CONTEXT_MAX_KEYS} keys"));
    }
    let mut total = 0usize;
    for (key, value) in context {
        let mut chars = key.chars();
        let shaped = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !shaped {
            return Some(format!("context key {key:?} is not ^[a-z][a-z0-9_]*$"));
        }
        if value.len() > CONTEXT_MAX_VALUE_BYTES {
            return Some(format!(
                "context value of {key} exceeds {CONTEXT_MAX_VALUE_BYTES} bytes"
            ));
        }
        total += key.len() + value.len();
    }
    (total > CONTEXT_MAX_TOTAL_BYTES)
        .then(|| format!("context exceeds {CONTEXT_MAX_TOTAL_BYTES} bytes"))
}

/// One persisted session identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSession {
    pub durable_session_id: String,
    pub session_file: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionStoreFile {
    version: u32,
    #[serde(default)]
    sessions: BTreeMap<String, PersistedSession>,
    #[serde(default)]
    ingested: Vec<String>,
}

/// Durable session identities by role key, plus ingested owner source ids.
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
    file: SessionStoreFile,
}

impl SessionStore {
    /// Loads the store. A missing file is empty; an unreadable one is set aside as
    /// `.corrupt` (never silently overwritten) and the store starts empty. Entries whose id is
    /// not a UUID or whose session file is not absolute are dropped.
    pub fn load(path: PathBuf) -> Self {
        let file = match std::fs::read_to_string(&path) {
            Err(_) => SessionStoreFile::default(),
            Ok(text) => match serde_json::from_str::<SessionStoreFile>(&text) {
                Ok(mut file) => {
                    file.sessions.retain(|_, session| {
                        uuid::Uuid::parse_str(&session.durable_session_id).is_ok()
                            && session
                                .session_file
                                .as_deref()
                                .is_none_or(|file| Path::new(file).is_absolute())
                    });
                    file
                }
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "mhc supervisor session store is unreadable; it was set aside");
                    let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
                    SessionStoreFile::default()
                }
            },
        };
        Self { path, file }
    }

    pub fn get(&self, key: &str) -> Option<&PersistedSession> {
        self.file.sessions.get(key)
    }

    /// Every persisted role binding, by role key.
    pub fn sessions(&self) -> &BTreeMap<String, PersistedSession> {
        &self.file.sessions
    }

    /// Re-reads the store from disk: another supervisor process may have written it while it
    /// owned the profile.
    pub fn reload(&mut self) {
        *self = Self::load(self.path.clone());
    }

    /// The role key persisted under `durable_session_id`.
    pub fn key_of(&self, durable_session_id: &str) -> Option<&str> {
        self.file
            .sessions
            .iter()
            .find(|(_, session)| session.durable_session_id == durable_session_id)
            .map(|(key, _)| key.as_str())
    }

    pub fn set(&mut self, key: &str, session: PersistedSession) -> Result<()> {
        if self.file.sessions.get(key) == Some(&session) {
            return Ok(());
        }
        self.file.sessions.insert(key.to_owned(), session);
        self.save()
    }

    /// Drops a role binding and persists the drop, so the role's next open starts a fresh
    /// durable identity with no session file to resume.
    pub fn remove(&mut self, key: &str) -> Result<bool> {
        if self.file.sessions.remove(key).is_none() {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    pub fn was_ingested(&self, source_id: &str) -> bool {
        self.file.ingested.iter().any(|id| id == source_id)
    }

    pub fn note_ingested(&mut self, source_id: &str) -> Result<()> {
        if self.was_ingested(source_id) {
            return Ok(());
        }
        self.file.ingested.push(source_id.to_owned());
        let excess = self.file.ingested.len().saturating_sub(INGESTED_IDS_KEPT);
        if excess > 0 {
            self.file.ingested = self.file.ingested.split_off(excess);
        }
        self.save()
    }

    /// Writes through a uniquely named temporary file and a rename, owner-only on unix, so two
    /// writers never share a temporary file.
    fn save(&mut self) -> Result<()> {
        self.file.version = 1;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.file)
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        let temp = self.path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&temp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) =
                std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))
            {
                let _ = std::fs::remove_file(&temp);
                return Err(error.into());
            }
        }
        if let Err(error) = std::fs::rename(&temp, &self.path) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ferryx-mhc-profile-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn routed(entries: &[(&str, &str)]) -> MhcProfileConfig {
        let env: BTreeMap<String, String> = entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let mut config = MhcProfileConfig::default();
        config.route_models(&|key: &str| {
            env.get(key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        });
        config
    }

    fn pair(provider: &str, model: &str) -> Option<(String, String)> {
        Some((provider.to_owned(), model.to_owned()))
    }

    fn scope() -> SessionRole {
        SessionRole::Conversation(ConversationScope {
            project_id: None,
            channel_key: "app:owner".to_owned(),
        })
    }

    const BACKGROUND_ROLES: [SessionRole; 3] = [
        SessionRole::OwnerIngest,
        SessionRole::Decision(0),
        SessionRole::Convergence,
    ];

    const OWNER_ENV: [(&str, &str); 2] = [
        ("OMON_DEFAULT_MODEL", "mahoquot/claude-opus-5-5"),
        ("OMON_BACKGROUND_MODEL", "mahoquot/gpt-6-luna"),
    ];

    fn profile(config: MhcProfileConfig) -> MhcProfile {
        MhcProfile::resolve(Path::new("/data"), Path::new("/parent"), config)
    }

    #[test]
    fn the_conversation_runs_on_the_owner_default_model_without_explicit_settings() {
        let profile = profile(routed(&OWNER_ENV));
        assert_eq!(
            profile.model_for(&scope()),
            pair("mahoquot", "claude-opus-5-5")
        );
        // Split at the first '/': the model id keeps any further slashes.
        let nested = routed(&[("OMON_DEFAULT_MODEL", " router/vendor/model-x ")]);
        assert_eq!(nested.conversation_model, pair("router", "vendor/model-x"));
    }

    #[test]
    fn background_roles_run_on_the_background_model() {
        let profile = profile(routed(&OWNER_ENV));
        for role in &BACKGROUND_ROLES {
            assert_eq!(
                profile.model_for(role),
                pair("mahoquot", "gpt-6-luna"),
                "{role:?}"
            );
            let spec = profile.role_spec(role);
            assert_eq!(spec.model, pair("mahoquot", "gpt-6-luna"), "{role:?}");
            let command = profile.open_session_command(&spec, "id", None);
            assert_eq!(command["provider"], json!("mahoquot"), "{role:?}");
            assert_eq!(command["modelId"], json!("gpt-6-luna"), "{role:?}");
        }
        let command = profile.open_session_command(&profile.role_spec(&scope()), "id", None);
        assert_eq!(command["provider"], json!("mahoquot"));
        assert_eq!(command["modelId"], json!("claude-opus-5-5"));
    }

    #[test]
    fn explicit_settings_override_the_owner_models_per_role() {
        let mut env = OWNER_ENV.to_vec();
        env.extend([
            ("FERRYX_HUB_MHC_BACKGROUND_PROVIDER", "judge"),
            ("FERRYX_HUB_MHC_BACKGROUND_MODEL", "small"),
        ]);
        let config = routed(&env);
        assert_eq!(config.background_model, pair("judge", "small"));
        assert_eq!(
            config.conversation_model,
            pair("mahoquot", "claude-opus-5-5")
        );

        env.extend([
            ("FERRYX_HUB_MHC_PROVIDER", "owner"),
            ("FERRYX_HUB_MHC_MODEL", "large"),
        ]);
        let config = routed(&env);
        assert_eq!(config.conversation_model, pair("owner", "large"));
        assert_eq!(config.background_model, pair("judge", "small"));
    }

    #[test]
    fn a_half_set_pair_is_ignored_and_the_owner_model_applies() {
        let mut env = OWNER_ENV.to_vec();
        env.extend([
            ("FERRYX_HUB_MHC_BACKGROUND_PROVIDER", "judge"),
            ("FERRYX_HUB_MHC_MODEL", "large"),
        ]);
        let config = routed(&env);
        assert_eq!(config.background_model, pair("mahoquot", "gpt-6-luna"));
        assert_eq!(
            config.conversation_model,
            pair("mahoquot", "claude-opus-5-5")
        );
    }

    #[test]
    fn without_any_model_setting_every_role_uses_the_engine_default() {
        let config = routed(&[
            ("OMON_BACKGROUND_MODEL", "no-slash"),
            ("OMON_DEFAULT_MODEL", " "),
        ]);
        assert_eq!(config.conversation_model, None);
        assert_eq!(config.background_model, None);
        let profile = profile(config);
        for role in BACKGROUND_ROLES.iter().cloned().chain([scope()]) {
            let command = profile.open_session_command(&profile.role_spec(&role), "id", None);
            assert!(command.get("provider").is_none() && command.get("modelId").is_none());
        }
    }

    /// The engine reads `<cwd>/AGENTS.md` into the system prompt of every session it opens in
    /// this directory, so that file is the only channel the hub has for teaching the supervisor
    /// tools without an engine release. Proven on a real engine: a sentinel line written there
    /// reached the conversation session's system prompt verbatim
    /// (`context_files.rs` on mhc 2026.11.16).
    #[test]
    fn prepare_writes_the_contract_where_the_engine_reads_instructions() {
        let dir = TempDir::new();
        let profile = MhcProfile::new(dir.path(), routed(&OWNER_ENV));
        profile.prepare().unwrap();
        let path = profile.instructions_path();
        assert_eq!(path, profile.cwd().join("AGENTS.md"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PROFILE_INSTRUCTIONS);
    }

    /// Both tools the engine actually registers are named in the contract; a pinned engine that
    /// advertises `supervisor_work_tools` registers exactly these two names
    /// (`maho-cli/src/cli/supervisor_work_tools.rs` `SUPERVISOR_WORK_TOOL_NAMES`).
    #[test]
    fn the_contract_names_the_tools_the_pinned_engine_registers() {
        for tool in ["supervisor_submit_work", "supervisor_work_status"] {
            assert!(
                PROFILE_INSTRUCTIONS.contains(tool),
                "the contract must name {tool}"
            );
        }
    }

    /// A handover is recognized by the owner's own words plus the addressed-session trailer, so
    /// the contract has to teach exactly those. Pinned against the hub's own matchers: a phrase
    /// the hub stopped honouring, or a trailer it stopped writing, would leave the model
    /// instructed to submit a request the hub never treats as a handover.
    #[test]
    fn the_contract_teaches_the_phrases_and_trailer_the_hub_matches() {
        for phrase in crate::supervisor::app_work::handover_phrases() {
            assert!(
                PROFILE_INSTRUCTIONS.contains(phrase),
                "the contract must teach the handover phrase {phrase:?}"
            );
        }
        for phrase in crate::supervisor::app_work::ownership_phrases() {
            assert!(
                PROFILE_INSTRUCTIONS.contains(phrase),
                "the contract must teach the ownership phrase {phrase:?}"
            );
        }
        // The trailer is written by `transcript::turn_message`; the contract teaches its shape.
        let trailer = crate::supervisor::transcript::turn_message(
            "take this session",
            &[crate::supervisor::panel_contract::SessionRef::new("local", "s-1")],
        );
        let marker = trailer
            .rfind('[')
            .map(|at| &trailer[at..])
            .and_then(|trail| trail.split_once(" local:s-1").map(|(head, _)| head))
            .expect("the trailer names the addressed session after its opening marker");
        assert!(
            PROFILE_INSTRUCTIONS.contains(marker),
            "the contract must teach the trailer {marker:?}"
        );
    }
}
