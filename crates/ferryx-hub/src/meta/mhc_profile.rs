//! Launch profile and durable session identities of the retained mhc supervisor engine.
//!
//! The engine always runs as `mhc --mode rpc --multi-session --no-model-fallback` over stdio, in
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
/// Executable looked up on the hub child PATH when none is configured.
pub const DEFAULT_MHC_BIN: &str = "mhc";
/// Arguments of the owned engine. `--no-model-fallback` keeps the engine from substituting a
/// model on errors; the supervisor never does either.
pub const LAUNCH_ARGS: [&str; 4] = ["--mode", "rpc", "--multi-session", "--no-model-fallback"];
/// Capabilities the engine must advertise before any session opens: the host baseline the
/// client relies on, then the plan's supervisor capabilities. `memory_forget` also stands for
/// `memory_correct`; there is no separate capability to guess.
pub const REQUIRED_CAPABILITIES: [&str; 8] = [
    "multi_session",
    "session_context",
    "durable_session_id",
    "memory_write_deny",
    "tools_exclude",
    "memory_receipts",
    "memory_forget",
    "supervisor_work_tools",
];
/// Client capability that makes the host forward extension RPC events (the work-tool requests).
pub const CLIENT_CAPABILITIES: [&str; 1] = ["extension_events"];
/// Tools no decision, convergence, conversation or ingest session may register: terminal
/// commands and file mutation. Memory write tools are withheld by `memory_write = deny`.
pub const DEFAULT_EFFECT_TOOLS: [&str; 3] = ["bash", "edit", "write"];
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
    /// Explicit provider/model policy. Both or neither are sent; the engine default applies
    /// otherwise. Never changed at runtime.
    pub provider: Option<String>,
    pub model: Option<String>,
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
            provider: None,
            model: None,
            thinking_level: None,
            effect_tools: DEFAULT_EFFECT_TOOLS.iter().map(|tool| (*tool).to_owned()).collect(),
            timeouts: MhcTimeouts::default(),
        }
    }
}

fn env_text(key: &str) -> Option<String> {
    crate::hub_env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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
    /// `None` unless `FERRYX_HUB_MHC_SUPERVISOR` turns the engine on (1/true/yes/on).
    /// `FERRYX_HUB_MHC_BIN`, `FERRYX_HUB_MHC_PROVIDER` + `FERRYX_HUB_MHC_MODEL`,
    /// `FERRYX_HUB_MHC_THINKING` and `FERRYX_HUB_MHC_EFFECT_TOOLS` (extra comma-separated names)
    /// refine it.
    pub fn from_env() -> Option<Self> {
        let enabled = env_text("FERRYX_HUB_MHC_SUPERVISOR").is_some_and(|value| {
            matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
        });
        if !enabled {
            return None;
        }
        let mut config = Self::default();
        if let Some(binary) = env_text("FERRYX_HUB_MHC_BIN") {
            config.binary = binary;
        }
        match (env_text("FERRYX_HUB_MHC_PROVIDER"), env_text("FERRYX_HUB_MHC_MODEL")) {
            (Some(provider), Some(model)) => {
                config.provider = Some(provider);
                config.model = Some(model);
            }
            (None, None) => {}
            _ => tracing::warn!(
                "FERRYX_HUB_MHC_PROVIDER and FERRYX_HUB_MHC_MODEL must be set together; the engine default model applies"
            ),
        }
        config.thinking_level = env_text("FERRYX_HUB_MHC_THINKING");
        if let Some(extra) = env_text("FERRYX_HUB_MHC_EFFECT_TOOLS") {
            config.add_effect_tools(&extra);
        }
        Some(config)
    }

    /// Adds valid comma-separated tool names; the defaults are never removed.
    pub fn add_effect_tools(&mut self, extra: &str) {
        for name in extra.split(',').map(str::trim).filter(|name| !name.is_empty()) {
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

    /// Profile-local omo config that pins the native memory identity.
    pub fn memory_config_path(&self) -> PathBuf {
        self.cwd.join(".omo").join("omo.jsonc")
    }

    pub fn launch_args(&self) -> Vec<String> {
        LAUNCH_ARGS.iter().map(|arg| (*arg).to_owned()).collect()
    }

    /// Required capabilities the engine did not advertise, in [`REQUIRED_CAPABILITIES`] order.
    pub fn missing_capabilities(&self, advertised: &[String]) -> Vec<String> {
        REQUIRED_CAPABILITIES
            .iter()
            .filter(|required| !advertised.iter().any(|name| name == *required))
            .map(|required| (*required).to_owned())
            .collect()
    }

    /// Creates the profile directory (owner-only on unix) and the profile-local memory identity
    /// config. Nothing outside `<data>` is written.
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

    fn effect_tools(&self) -> String {
        self.config.effect_tools.join(",")
    }

    /// The session policy of a role.
    pub fn role_spec(&self, role: &SessionRole) -> RoleSpec {
        let mut context = BTreeMap::new();
        context.insert("tools_exclude".to_owned(), self.effect_tools());
        let (key, name, durable, cancellable) = match role {
            SessionRole::OwnerIngest => ("owner-ingest".to_owned(), "owner_ingest", true, false),
            SessionRole::Conversation(scope) => {
                let scope_key = scope.key();
                context.insert("memory_write".to_owned(), "deny".to_owned());
                context.insert("supervisor_tools".to_owned(), "work".to_owned());
                context.insert("ferryx_scope".to_owned(), scope_key.clone());
                (format!("conversation-{scope_key}"), "conversation", true, false)
            }
            SessionRole::Decision(slot) => {
                context.insert("memory_write".to_owned(), "deny".to_owned());
                (format!("decision-{slot}"), "decision", true, true)
            }
            SessionRole::Convergence => {
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
        if let (Some(provider), Some(model)) = (&self.config.provider, &self.config.model) {
            command["provider"] = json!(provider);
            command["modelId"] = json!(model);
        }
        if let Some(level) = &self.config.thinking_level {
            command["thinkingLevel"] = json!(level);
        }
        command
    }
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
            return Some(format!("context value of {key} exceeds {CONTEXT_MAX_VALUE_BYTES} bytes"));
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

    /// Writes through a temporary file and a rename, owner-only on unix.
    fn save(&mut self) -> Result<()> {
        self.file.version = 1;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.file)
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }
}
