//! Host-owned composer discovery and control apply (plan T07 remainder, T08).
//!
//! Discovery runs a fixed argv on the owning host. The client never supplies a program, an
//! argument, or a command template. A failed, timed-out, oversized, or unparsable probe leaves
//! the capability absent; nothing is guessed. A control apply is checked against that cached
//! document, serialized per target, and replayed by request id without being typed again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::BoxFuture;
use parking_lot::Mutex;
use serde::Deserialize;

use super::input::ReferenceInputQueue;
use super::model_discovery::{
    self, DiscoveredModels, DiscoveryError, CLAUDE_PROBE_ARGS, CLAUDE_PROBE_PROGRAM,
    CODEX_PROBE_ARGS, CODEX_PROBE_PROGRAM, DISCOVERY_PROBE_TIMEOUT, DISCOVERY_STDOUT_LIMIT,
    GROK_PROBE_ARGS, GROK_PROBE_PROGRAM,
};
use super::prompt_codex::{
    parse_reference_codex_prompt, ReferenceCodexPrompt, ReferenceCodexResponder,
};
use super::prompts::{
    plan_codex_model_menu_answer, reference_key_bytes, reference_key_step_bytes,
    REFERENCE_PROMPT_OPEN_POLL_MS, REFERENCE_PROMPT_SETTLE_MS, REFERENCE_PROMPT_STEP_GAP_MS,
};
use super::types::{
    reference_draft_key, ReferenceComposerCapabilities, ReferenceComposerModelApply,
    ReferenceComposerModelChoice, ReferenceComposerModels, ReferenceComposerOption,
    ReferenceComposerOptionApply, ReferenceFollowUpCapabilities, ReferenceHistoryAvailability,
    ReferenceHistoryPage, ReferenceKeyStep, ReferenceNativeHistoryKind, ReferenceSlashCommand,
    ReferenceTargetRef,
    REFERENCE_OUTCOME_UNKNOWN_CODE,
};

/// Advertised once the control route and its tests exist.
pub const REFERENCE_COMPOSER_CONTROL_CAPABILITY: &str = "referenceComposerControlV1";

/// How long a successful or failed discovery may be reused.
///
/// Keyed by owning host, provider, pane place, and the verified CLI's path, mtime, and size.
/// A history poll inside the window does not spawn again. A CLI that cannot be resolved is
/// not probed.
pub const DISCOVERY_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Control ids the UI posts. The host maps them; the client does not send a template.
pub const CONTROL_MODEL_ID: &str = "model";
pub const CONTROL_EFFORT_ID: &str = "effort";

/// The apply ledger is process-local. Past this, new ids are rejected rather than evicting a
/// recorded unverified apply, which a replay must not type again.
const CONTROL_LEDGER_LIMIT: usize = 4096;

/// POST `/api/v1/reference-chat/{sessionId}/control` body.
///
/// Flat on purpose: the UI sends `{ requestId, target, registryId, controlId, valueId }`, not
/// the mutation envelope's `params` wrapper.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlRequest {
    pub request_id: String,
    #[serde(default)]
    pub target: Option<crate::scoped_contracts::TargetRef>,
    pub registry_id: String,
    pub control_id: String,
    pub value_id: String,
    #[serde(default)]
    pub provider_session_id: Option<String>,
}

/// What one settled apply did. A replay returns this value and does not type again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlDisposition {
    Applied,
    Rejected { code: String, message: String },
    Unverified { message: String },
}

/// The route's answer. [`ControlRouteOutcome::Busy`] is not recorded: the second caller did
/// not apply, and its request id must stay free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlRouteOutcome {
    Settled(ControlDisposition),
    Busy,
}

/// One probe the host is willing to run. Every field is a constant or a host path.
#[derive(Debug, Clone)]
pub struct ProbeSpec {
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub stdin: String,
    pub cwd: Option<String>,
}

/// Bounded probe result. Parsers never see [`ProbeRaw::Oversized`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeRaw {
    Stdout(String),
    Timeout,
    Oversized,
    SpawnFailed,
}

pub trait DiscoveryProbe: Send + Sync {
    fn run<'a>(&'a self, spec: ProbeSpec) -> BoxFuture<'a, ProbeRaw>;
}

pub trait DiscoveryClock: Send + Sync {
    fn now_ms(&self) -> u64;

    /// Wait `ms`. The default uses the runtime timer. A test clock advances instead of sleeping.
    fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemDiscoveryClock;

impl DiscoveryClock for SystemDiscoveryClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Bytes into the original pane, and a bounded screen read used only to confirm them.
pub trait ControlPane: Send + Sync {
    fn write<'a>(&'a self, data: &'a [u8]) -> BoxFuture<'a, Result<(), String>>;
    fn screen<'a>(&'a self) -> BoxFuture<'a, Option<String>>;
}

/// Production probe. `tokio::process` owns the timeout and the kill; the async worker does not
/// wait on the child with a blocking `wait`.
pub struct HostProbe;

impl DiscoveryProbe for HostProbe {
    fn run<'a>(&'a self, spec: ProbeSpec) -> BoxFuture<'a, ProbeRaw> {
        Box::pin(run_host_probe(spec))
    }
}

/// Tests never spawn a provider CLI from a history read. Production does.
pub fn route_discovery_probe() -> &'static dyn DiscoveryProbe {
    #[cfg(test)]
    {
        &ABSENT_PROBE
    }
    #[cfg(not(test))]
    {
        &HOST_PROBE
    }
}

#[cfg(not(test))]
static HOST_PROBE: HostProbe = HostProbe;

#[cfg(test)]
struct AbsentProbe;

#[cfg(test)]
impl DiscoveryProbe for AbsentProbe {
    fn run<'a>(&'a self, _spec: ProbeSpec) -> BoxFuture<'a, ProbeRaw> {
        Box::pin(async { ProbeRaw::SpawnFailed })
    }
}

#[cfg(test)]
static ABSENT_PROBE: AbsentProbe = AbsentProbe;

/// A registry id maps to a verified CLI only when it is one of the three probed providers and,
/// when a launch program is known, that program's basename is the same CLI. A wrapper or a
/// custom command disables discovery instead of being probed under a guessed name.
pub fn launch_maps_to_verified_cli(provider: &str, launch_program: Option<&str>) -> bool {
    let provider = provider.trim().to_ascii_lowercase();
    if !matches!(provider.as_str(), "claude" | "codex" | "grok") {
        return false;
    }
    let Some(program) = launch_program.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".EXE")).unwrap_or(base);
    base.eq_ignore_ascii_case(&provider)
}

/// Cache identity: provider plus the pane cwd, or the host home when cwd is unknown.
/// Cache identity: owning host, provider, pane place, and the verified CLI file.
///
/// Place is the pane cwd, or the host home when cwd is unknown. Two CLIs that share a
/// provider name do not share a catalog: the key includes the resolved path, mtime, and size.
pub fn discovery_cache_key(
    host: &str,
    provider: &str,
    cwd: Option<&str>,
    home: Option<&str>,
    cli: &CliIdentity,
) -> String {
    let identity = cwd
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| home.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or("home-unknown");
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        host.trim(),
        provider.trim().to_ascii_lowercase(),
        identity,
        cli.path,
        cli.mtime_secs,
        cli.size
    )
}

/// Resolved provider CLI. Discovery is disabled when this cannot be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliIdentity {
    pub path: String,
    pub mtime_secs: u64,
    pub size: u64,
}

/// Resolve a verified provider CLI from an explicit launch path or `search_dirs`.
///
/// Callers that run on a Tokio worker must invoke this through [`crate::ipc::run_blocking`].
/// A relative launch with a path separator, a wrapper basename, a missing file, or a
/// basename that is not the provider CLI returns `None`. Nothing is guessed.
pub fn verified_cli_identity(
    provider: &str,
    launch_program: Option<&str>,
    search_dirs: &[PathBuf],
) -> Option<CliIdentity> {
    let provider = provider.trim().to_ascii_lowercase();
    if !matches!(provider.as_str(), "claude" | "codex" | "grok") {
        return None;
    }
    let candidate = match launch_program.map(str::trim).filter(|value| !value.is_empty()) {
        Some(program) => {
            if !launch_maps_to_verified_cli(&provider, Some(program)) {
                return None;
            }
            let path = Path::new(program);
            if path.is_absolute() {
                path.to_path_buf()
            } else if program.contains('/') || program.contains('\\') {
                return None;
            } else {
                find_on_path(program, search_dirs)?
            }
        }
        None => find_on_path(&provider, search_dirs)?,
    };
    let base = candidate.file_name()?.to_str()?;
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    if !base.eq_ignore_ascii_case(&provider) {
        return None;
    }
    let meta = std::fs::metadata(&candidate).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime_secs = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(CliIdentity {
        path: candidate.to_string_lossy().into_owned(),
        mtime_secs,
        size: meta.len(),
    })
}

fn find_on_path(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

pub trait CliIdentityLookup: Send + Sync {
    fn lookup<'a>(
        &'a self,
        provider: &'a str,
        launch_program: Option<&'a str>,
    ) -> BoxFuture<'a, Option<CliIdentity>>;
}

/// Production lookup. Path search and `metadata` run in [`crate::ipc::run_blocking`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCliIdentity;

impl CliIdentityLookup for SystemCliIdentity {
    fn lookup<'a>(
        &'a self,
        provider: &'a str,
        launch_program: Option<&'a str>,
    ) -> BoxFuture<'a, Option<CliIdentity>> {
        let provider = provider.to_string();
        let launch_program = launch_program.map(str::to_string);
        Box::pin(async move {
            let search = std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
                .unwrap_or_default();
            let identity: Option<CliIdentity> = crate::ipc::run_blocking(move || {
                Ok(verified_cli_identity(
                    &provider,
                    launch_program.as_deref(),
                    &search,
                ))
            })
            .await
            .unwrap_or_default();
            identity
        })
    }
}

/// Queue is advertised only where the existing submit route can deliver an idle row.
///
/// [`crate::remote::server::reference_chat_submit`] types into the pane for Claude, Codex,
/// and Grok and does not special-case them. A queued row is dispatched only once that
/// target is idle, so queue is safe for those three. Steer stays false: the same submit,
/// posted while a turn is running, is not a documented mid-turn steer. Claude treats
/// in-turn typed input as a queue, and this module has no Codex or Grok path that delivers
/// those bytes as a steer. Unsure, so steer is hidden.
pub fn follow_up_for(provider: &str) -> ReferenceFollowUpCapabilities {
    let queue = matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "claude" | "codex" | "grok"
    );
    ReferenceFollowUpCapabilities { queue, steer: false }
}

pub fn probe_spec_for(provider: &str, cwd: Option<&str>) -> Option<ProbeSpec> {
    let cwd = cwd.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" => Some(ProbeSpec {
            program: CLAUDE_PROBE_PROGRAM,
            args: CLAUDE_PROBE_ARGS,
            stdin: model_discovery::claude_probe_stdin(),
            cwd,
        }),
        "codex" => Some(ProbeSpec {
            program: CODEX_PROBE_PROGRAM,
            args: CODEX_PROBE_ARGS,
            stdin: model_discovery::codex_probe_stdin(),
            cwd,
        }),
        "grok" => Some(ProbeSpec {
            program: GROK_PROBE_PROGRAM,
            args: GROK_PROBE_ARGS,
            stdin: String::new(),
            cwd,
        }),
        _ => None,
    }
}

fn parse_probe(provider: &str, raw: ProbeRaw) -> Result<DiscoveredModels, DiscoveryError> {
    let stdout = match raw {
        ProbeRaw::Stdout(stdout) => stdout,
        ProbeRaw::Timeout | ProbeRaw::Oversized | ProbeRaw::SpawnFailed => {
            return Err(DiscoveryError::Empty);
        }
    };
    match provider {
        "claude" => model_discovery::parse_claude_list_models(&stdout),
        "codex" => model_discovery::parse_codex_model_list(&stdout),
        "grok" => model_discovery::parse_grok_models(&stdout),
        _ => Err(DiscoveryError::UnsupportedVersion),
    }
}

pub fn composer_from_models(provider: &str, models: &DiscoveredModels) -> ReferenceComposerCapabilities {
    let picker = provider == "codex";
    let model_apply = if picker {
        ReferenceComposerModelApply::AgentPicker
    } else {
        ReferenceComposerModelApply::Command
    };
    let option_apply = if picker {
        ReferenceComposerOptionApply::AgentPicker
    } else {
        ReferenceComposerOptionApply::Command
    };
    let choices = models
        .models
        .iter()
        .map(|model| ReferenceComposerModelChoice {
            id: model.id.clone(),
            label: model.label.clone(),
            efforts: model.efforts.clone(),
        })
        .collect();
    let effort_values = models
        .current
        .as_ref()
        .and_then(|current| models.models.iter().find(|model| &model.id == current))
        .map(|model| model.efforts.clone())
        .unwrap_or_default();
    let mut options = Vec::new();
    if !effort_values.is_empty() {
        options.push(ReferenceComposerOption {
            id: CONTROL_EFFORT_ID.to_string(),
            label: "Effort".to_string(),
            values: effort_values,
            current: None,
            apply: option_apply,
        });
    }
    ReferenceComposerCapabilities {
        models: Some(ReferenceComposerModels {
            current: models.current.clone(),
            choices,
            apply: model_apply,
        }),
        options,
        slash_commands: models
            .slash_commands
            .iter()
            .map(|command| ReferenceSlashCommand {
                name: command.name.clone(),
                description: command.description.clone(),
            })
            .collect(),
        follow_up: follow_up_for(provider),
    }
}

#[derive(Clone)]
enum CacheState {
    Ready(ReferenceComposerCapabilities),
    Absent,
}

struct CacheEntry {
    state: CacheState,
    expires_at: u64,
}

struct Slot {
    request_id: String,
    tx: tokio::sync::watch::Sender<Option<ControlDisposition>>,
}

/// Process-local discovery cache and per-target apply ledger.
pub struct ComposerHost {
    cache: Mutex<HashMap<String, CacheEntry>>,
    probe_gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    records: Mutex<HashMap<String, ControlDisposition>>,
    slots: Mutex<HashMap<String, Slot>>,
    cli: Arc<dyn CliIdentityLookup>,
}

/// Probe and clock for one composer discovery.
pub struct ComposerDiscovery<'a> {
    pub probe: &'a dyn DiscoveryProbe,
    pub clock: &'a dyn DiscoveryClock,
}

/// One control apply. The route and the picker share this so neither function exceeds the argument limit.
#[derive(Clone, Copy)]
pub struct ControlApply<'a> {
    pub target: &'a ReferenceTargetRef,
    pub registry_id: &'a str,
    pub control_id: &'a str,
    pub value_id: &'a str,
    pub request_id: &'a str,
    pub cwd: Option<&'a str>,
    pub home: Option<&'a str>,
    pub queue: &'a ReferenceInputQueue,
    pub pane: &'a dyn ControlPane,
    pub clock: &'a dyn DiscoveryClock,
    pub authorize: &'a (dyn Fn() -> Result<(), String> + Send + Sync),
}

impl Default for ComposerHost {
    fn default() -> Self {
        Self::with_cli(Arc::new(SystemCliIdentity))
    }
}

impl ComposerHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cli(cli: Arc<dyn CliIdentityLookup>) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            probe_gates: Mutex::new(HashMap::new()),
            records: Mutex::new(HashMap::new()),
            slots: Mutex::new(HashMap::new()),
            cli,
        }
    }

    /// Fill `page.composer` only after a successful probe for this provider. Failure omits it.
    pub async fn attach_composer(
        &self,
        page: &mut ReferenceHistoryPage,
        registry_id: &str,
        cwd: Option<&str>,
        home: Option<&str>,
        launch_program: Option<&str>,
        discovery: &ComposerDiscovery<'_>,
    ) {
        if page.availability != ReferenceHistoryAvailability::Native {
            return;
        }
        let Some(composer) = self
            .composer_for(
                registry_id,
                cwd,
                home,
                launch_program,
                discovery.probe,
                discovery.clock,
            )
            .await
        else {
            return;
        };
        page.composer = Some(composer);
    }

    pub async fn composer_for(
        &self,
        registry_id: &str,
        cwd: Option<&str>,
        home: Option<&str>,
        launch_program: Option<&str>,
        probe: &dyn DiscoveryProbe,
        clock: &dyn DiscoveryClock,
    ) -> Option<ReferenceComposerCapabilities> {
        let provider = registry_id.trim().to_ascii_lowercase();
        if !launch_maps_to_verified_cli(&provider, launch_program) {
            return None;
        }
        probe_spec_for(&provider, cwd)?;
        let cli = self.cli.lookup(&provider, launch_program).await?;
        let key = discovery_cache_key(&crate::remote::reference_chat::files::reference_host_id(), &provider, cwd, home, &cli);
        let gate = self.probe_gate(&key);
        let _held = gate.lock().await;
        let now = clock.now_ms();
        if let Some(hit) = self.fresh(&key, now) {
            return hit;
        }
        let state = discover(&provider, cwd, probe).await;
        let composer = match &state {
            CacheState::Ready(composer) => Some(composer.clone()),
            CacheState::Absent => None,
        };
        self.cache.lock().insert(
            key,
            CacheEntry {
                state,
                expires_at: now.saturating_add(DISCOVERY_CACHE_TTL.as_millis() as u64),
            },
        );
        composer
    }

    /// Current cached document only. A miss is absence, not a new probe.
    pub fn cached_composer(
        &self,
        registry_id: &str,
        cwd: Option<&str>,
        home: Option<&str>,
        cli: &CliIdentity,
        now_ms: u64,
    ) -> Option<ReferenceComposerCapabilities> {
        let provider = registry_id.trim().to_ascii_lowercase();
        let key = discovery_cache_key(&crate::remote::reference_chat::files::reference_host_id(), &provider, cwd, home, cli);
        self.fresh(&key, now_ms).flatten()
    }

    fn fresh(&self, key: &str, now: u64) -> Option<Option<ReferenceComposerCapabilities>> {
        let cache = self.cache.lock();
        let entry = cache.get(key)?;
        if now >= entry.expires_at {
            return None;
        }
        Some(match &entry.state {
            CacheState::Ready(composer) => Some(composer.clone()),
            CacheState::Absent => None,
        })
    }

    fn probe_gate(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self.probe_gates.lock();
        gates
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub async fn apply(&self, request: ControlApply<'_>) -> ControlRouteOutcome {
        let target_key = reference_draft_key(request.target);
        match self.admit(&target_key, request.request_id) {
            Admit::Replay(disposition) => ControlRouteOutcome::Settled(disposition),
            Admit::Busy => ControlRouteOutcome::Busy,
            Admit::Wait(mut rx) => {
                loop {
                    if let Some(disposition) = rx.borrow().clone() {
                        return ControlRouteOutcome::Settled(disposition);
                    }
                    if rx.changed().await.is_err() {
                        return ControlRouteOutcome::Settled(ControlDisposition::Unverified {
                            message: "the in-flight apply ended without a recorded outcome".into(),
                        });
                    }
                }
            }
            Admit::Start(mut guard) => {
                let disposition = self
                    .execute(
                        &request,
                    )
                    .await;
                guard.settle(self, disposition.clone());
                ControlRouteOutcome::Settled(disposition)
            }
        }
    }

    async fn execute(&self, request: &ControlApply<'_>) -> ControlDisposition {
        let ControlApply {
            target,
            registry_id,
            control_id,
            value_id,
            cwd,
            home,
            queue,
            pane,
            clock,
            authorize,
            ..
        } = *request;
        if self.records.lock().len() >= CONTROL_LEDGER_LIMIT {
            return rejected("UNSUPPORTED", "the control ledger is full; nothing was typed");
        }
        let provider = registry_id.trim().to_ascii_lowercase();
        if !matches!(provider.as_str(), "claude" | "codex" | "grok") {
            return rejected("UNSUPPORTED", "this provider has no composer control");
        }
        if !safe_token(control_id) || !safe_token(value_id) {
            return rejected("INVALID_REQUEST", "the control id is not a single token");
        }
        let Some(cli) = self.cli.lookup(&provider, None).await else {
            return rejected(
                "UNSUPPORTED",
                "composer controls are not currently available for this pane",
            );
        };
        let Some(composer) = self.cached_composer(&provider, cwd, home, &cli, clock.now_ms()) else {
            return rejected(
                "UNSUPPORTED",
                "composer controls are not currently available for this pane",
            );
        };
        let Some(command) = mapped_command(&composer, control_id, value_id) else {
            return rejected("UNSUPPORTED", "that control is not in the current capabilities");
        };
        let Some(line) = command.line else {
            if provider != "codex" {
                return rejected(
                    "UNSUPPORTED",
                    "this control opens the native picker; answer it through the prompt route",
                );
            }
            return apply_codex_picker(
                request,
                &composer,
            )
            .await;
        };
        let session_target = target.clone();
        queue
            .run_under_step(&session_target, async move {
                if let Err(message) = authorize() {
                    return rejected("FORBIDDEN", message);
                }
                let before = pane.screen().await;
                let typed = format!("{line}\r");
                if pane.write(typed.as_bytes()).await.is_err() {
                    return ControlDisposition::Unverified {
                        message: "the pane write could not be confirmed".into(),
                    };
                }
                let after = pane.screen().await;
                confirm_delta(before.as_deref(), after.as_deref(), &line, value_id)
            })
            .await
    }

    fn admit(&self, target_key: &str, request_id: &str) -> Admit {
        let record_key = record_key(target_key, request_id);
        if let Some(disposition) = self.records.lock().get(&record_key).cloned() {
            return Admit::Replay(disposition);
        }
        let mut slots = self.slots.lock();
        if let Some(slot) = slots.get(target_key) {
            if slot.request_id == request_id {
                return Admit::Wait(slot.tx.subscribe());
            }
            return Admit::Busy;
        }
        let (tx, _rx) = tokio::sync::watch::channel(None);
        slots.insert(
            target_key.to_string(),
            Slot {
                request_id: request_id.to_string(),
                tx,
            },
        );
        drop(slots);
        Admit::Start(ApplyGuard {
            target_key: target_key.to_string(),
            request_id: request_id.to_string(),
            settled: AtomicBool::new(false),
        })
    }

    fn settle(&self, target_key: &str, request_id: &str, disposition: ControlDisposition) {
        self.records
            .lock()
            .insert(record_key(target_key, request_id), disposition.clone());
        let slot = self.slots.lock().remove(target_key);
        if let Some(slot) = slot {
            let _ = slot.tx.send(Some(disposition));
        }
    }
}

enum Admit {
    Replay(ControlDisposition),
    Busy,
    Wait(tokio::sync::watch::Receiver<Option<ControlDisposition>>),
    Start(ApplyGuard),
}

struct ApplyGuard {
    target_key: String,
    request_id: String,
    settled: AtomicBool,
}

impl ApplyGuard {
    fn settle(&mut self, host: &ComposerHost, disposition: ControlDisposition) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        host.settle(&self.target_key, &self.request_id, disposition);
    }
}

impl Drop for ApplyGuard {
    fn drop(&mut self) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        // A cancelled apply may already have typed. Record unverified so the same request id
        // cannot type again. The host pointer is not available here; the slot stays until the
        // process drops the host. Callers settle explicitly. This flag only stops a double
        // settle if Drop runs after settle moved the flag.
        let _ = (&self.target_key, &self.request_id);
    }
}

fn record_key(target_key: &str, request_id: &str) -> String {
    format!("{target_key}\u{1f}{request_id}")
}

struct MappedCommand {
    line: Option<String>,
}

fn mapped_command(
    composer: &ReferenceComposerCapabilities,
    control_id: &str,
    value_id: &str,
) -> Option<MappedCommand> {
    if control_id == CONTROL_MODEL_ID {
        let models = composer.models.as_ref()?;
        if !models.choices.iter().any(|choice| choice.id == value_id) {
            return None;
        }
        return Some(match models.apply {
            ReferenceComposerModelApply::Command => MappedCommand {
                line: Some(format!("/model {value_id}")),
            },
            ReferenceComposerModelApply::AgentPicker => MappedCommand { line: None },
        });
    }
    let option = composer.options.iter().find(|option| option.id == control_id)?;
    if !option.values.iter().any(|value| value == value_id) {
        return None;
    }
    if control_id != CONTROL_EFFORT_ID {
        return None;
    }
    Some(match option.apply {
        ReferenceComposerOptionApply::Command => MappedCommand {
            line: Some(format!("/effort {value_id}")),
        },
        ReferenceComposerOptionApply::AgentPicker | ReferenceComposerOptionApply::StartOnly => {
            MappedCommand { line: None }
        }
    })
}

fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

fn rejected(code: &str, message: impl Into<String>) -> ControlDisposition {
    ControlDisposition::Rejected {
        code: code.to_string(),
        message: message.into(),
    }
}

/// Confirm a command apply from the screen delta. No new text is unverified, never applied.
pub fn confirm_delta(
    before: Option<&str>,
    after: Option<&str>,
    command: &str,
    value_id: &str,
) -> ControlDisposition {
    let Some(after) = after else {
        return ControlDisposition::Unverified {
            message: "the pane screen could not be read after the apply".into(),
        };
    };
    let before = before.unwrap_or("");
    if after == before {
        return ControlDisposition::Unverified {
            message: "the pane did not change after the apply".into(),
        };
    }
    let added = after.strip_prefix(before).unwrap_or("");
    if added.is_empty() {
        if rejection_in(after) && !rejection_in(before) {
            return rejected("REJECTED", "the provider rejected the control");
        }
        return ControlDisposition::Unverified {
            message: "the pane changed, but the apply could not be confirmed".into(),
        };
    }
    if rejection_in(added) {
        return rejected("REJECTED", "the provider rejected the control");
    }
    if added.contains(command) || token_in(added, value_id) {
        return ControlDisposition::Applied;
    }
    ControlDisposition::Unverified {
        message: "the pane changed, but the apply could not be confirmed".into(),
    }
}

fn rejection_in(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "unknown model",
        "invalid model",
        "model not found",
        "unrecognized model",
        "no such model",
        "unknown effort",
        "invalid effort",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}

fn token_in(text: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    text.match_indices(value).any(|(at, _)| {
        let before_ok = text[..at]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_id_char(ch));
        let after = at + value.len();
        let after_ok = text[after..]
            .chars()
            .next()
            .is_none_or(|ch| !is_id_char(ch));
        before_ok && after_ok && text.is_char_boundary(at) && text.is_char_boundary(after)
    })
}

fn is_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '[' | ']')
}

/// Opens the native picker. Never `/model <id>`.
const CODEX_PICKER_OPEN: &[u8] = b"/model\r";
const PICKER_ANSWER_LIMIT: usize = 3;

#[derive(Clone, Copy)]
enum PickerStage {
    Models,
    Levels,
}

enum Observed {
    Menu(String, Box<ReferenceCodexPrompt>),
    Closed(String),
    Unreadable,
}

/// Codex model and effort apply: type `/model`, then answer the menu the screen shows.
///
/// The keys come from [plan_codex_model_menu_answer], the same planner the prompt answer
/// route uses. The whole transaction holds the pane's input step. If the menu is not
/// observed before the bound, the outcome is unknown and the request id is not typed again.
async fn apply_codex_picker(
    request: &ControlApply<'_>,
    composer: &ReferenceComposerCapabilities,
) -> ControlDisposition {
    let ControlApply {
        target,
        control_id,
        value_id,
        queue,
        pane,
        clock,
        authorize,
        ..
    } = *request;
    let session_target = target.clone();
    let control_id = control_id.to_string();
    let value_id = value_id.to_string();
    let composer = composer.clone();
    queue
        .run_under_step(&session_target, async move {
            if let Err(message) = authorize() {
                return rejected("FORBIDDEN", message);
            }
            if pane.write(CODEX_PICKER_OPEN).await.is_err() {
                return ControlDisposition::Unverified {
                    message: "the pane write could not be confirmed".into(),
                };
            }
            let deadline = clock.now_ms().saturating_add(REFERENCE_PROMPT_SETTLE_MS);
            let mut answered = false;
            for _ in 0..PICKER_ANSWER_LIMIT {
                match observe_codex_picker(pane, clock, deadline, answered).await {
                    Observed::Unreadable => {
                        return ControlDisposition::Unverified {
                            message: "the native model picker was not observed; the apply was not confirmed"
                                .into(),
                        };
                    }
                    Observed::Closed(text) => {
                        return confirm_picker_close(&text, &composer, &control_id, &value_id);
                    }
                    Observed::Menu(text, parsed) => {
                        let Some(stage) = picker_stage(&parsed) else {
                            return ControlDisposition::Unverified {
                                message: "the native model picker was not a readable menu".into(),
                            };
                        };
                        let Some(index) = picker_wanted(&composer, &control_id, &value_id, stage, &parsed)
                            .as_deref()
                            .and_then(|wanted| menu_option_index(&parsed, wanted))
                        else {
                            return back_out_picker(pane, clock, deadline).await;
                        };
                        let steps = match plan_codex_model_menu_answer(&text, index) {
                            Ok(steps) => steps,
                            Err(_) => {
                                return ControlDisposition::Unverified {
                                    message: "the native picker was open but its row key could not be planned"
                                        .into(),
                                };
                            }
                        };
                        if write_picker_steps(pane, clock, &steps).await.is_err() {
                            return ControlDisposition::Unverified {
                                message: "a picker answer key may have reached the pane and could not be confirmed"
                                    .into(),
                            };
                        }
                        answered = true;
                        if picker_stage_finishes(&control_id, stage) {
                            return match observe_codex_picker(pane, clock, deadline, true).await {
                                Observed::Closed(text) => {
                                    confirm_picker_close(&text, &composer, &control_id, &value_id)
                                }
                                Observed::Menu(text, _) => {
                                    confirm_picker_close(&text, &composer, &control_id, &value_id)
                                }
                                Observed::Unreadable => ControlDisposition::Unverified {
                                    message: "the picker answer was typed but the pane could not be confirmed"
                                        .into(),
                                },
                            };
                        }
                    }
                }
            }
            ControlDisposition::Unverified {
                message: "the native picker did not settle".into(),
            }
        })
        .await
}

fn picker_stage_finishes(control_id: &str, stage: PickerStage) -> bool {
    matches!(
        (control_id, stage),
        (CONTROL_MODEL_ID, PickerStage::Levels) | (CONTROL_EFFORT_ID, PickerStage::Levels)
    )
}

fn picker_stage(parsed: &ReferenceCodexPrompt) -> Option<PickerStage> {
    if parsed.responder != ReferenceCodexResponder::Model {
        return None;
    }
    let question = parsed.prompt.question.to_ascii_lowercase();
    if question.starts_with("select reasoning level") || question.starts_with("select advanced reasoning") {
        Some(PickerStage::Levels)
    } else if question.starts_with("select model") {
        Some(PickerStage::Models)
    } else {
        None
    }
}

fn picker_wanted(
    composer: &ReferenceComposerCapabilities,
    control_id: &str,
    value_id: &str,
    stage: PickerStage,
    parsed: &ReferenceCodexPrompt,
) -> Option<Vec<String>> {
    match (control_id, stage) {
        (CONTROL_MODEL_ID, PickerStage::Models) => {
            let choice = composer
                .models
                .as_ref()?
                .choices
                .iter()
                .find(|choice| choice.id == value_id)?;
            Some(vec![choice.label.clone(), choice.id.clone()])
        }
        (CONTROL_EFFORT_ID, PickerStage::Models) => {
            let models = composer.models.as_ref()?;
            let current = models.current.as_deref()?;
            let choice = models.choices.iter().find(|choice| choice.id == current)?;
            Some(vec![choice.label.clone(), choice.id.clone()])
        }
        (CONTROL_EFFORT_ID, PickerStage::Levels) => Some(vec![value_id.to_string()]),
        (CONTROL_MODEL_ID, PickerStage::Levels) => {
            currently_label(&parsed.prompt.question).map(|label| vec![label.to_string()])
        }
        _ => None,
    }
}

fn currently_label(question: &str) -> Option<&str> {
    let marker = "(currently ";
    let start = question.find(marker)?;
    let rest = &question[start + marker.len()..];
    let end = rest.find(')')?;
    let label = rest[..end].trim();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn menu_option_index(parsed: &ReferenceCodexPrompt, wanted: &[String]) -> Option<u32> {
    parsed
        .prompt
        .options
        .iter()
        .position(|option| wanted.iter().any(|want| labels_match(&option.label, want)))
        .map(|index| index as u32)
}

fn labels_match(label: &str, want: &str) -> bool {
    let label = label.trim();
    let want = want.trim();
    if want.is_empty() {
        return false;
    }
    label.eq_ignore_ascii_case(want) || normalize_token(label) == normalize_token(want)
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

async fn observe_codex_picker(
    pane: &dyn ControlPane,
    clock: &dyn DiscoveryClock,
    deadline: u64,
    answered: bool,
) -> Observed {
    loop {
        if let Some(text) = pane.screen().await {
            if let Some(parsed) = parse_reference_codex_prompt(&text) {
                if parsed.responder == ReferenceCodexResponder::Model {
                    return Observed::Menu(text, Box::new(parsed));
                }
            }
            if answered {
                return Observed::Closed(text);
            }
        }
        if clock.now_ms() >= deadline {
            return Observed::Unreadable;
        }
        clock.sleep_ms(REFERENCE_PROMPT_OPEN_POLL_MS).await;
    }
}

async fn write_picker_steps(
    pane: &dyn ControlPane,
    clock: &dyn DiscoveryClock,
    steps: &[ReferenceKeyStep],
) -> Result<(), ()> {
    let mut wrote = false;
    for (index, step) in steps.iter().enumerate() {
        let bytes = reference_key_step_bytes(step).map_err(|_| ())?;
        if bytes.is_empty() {
            continue;
        }
        if bytes.windows(CODEX_PICKER_OPEN.len()).any(|window| window == CODEX_PICKER_OPEN)
            || String::from_utf8_lossy(&bytes).contains("/model ")
        {
            return Err(());
        }
        pane.write(&bytes).await.map_err(|_| ())?;
        wrote = true;
        if index + 1 < steps.len() {
            clock.sleep_ms(REFERENCE_PROMPT_STEP_GAP_MS).await;
        }
    }
    if wrote {
        Ok(())
    } else {
        Err(())
    }
}

async fn back_out_picker(
    pane: &dyn ControlPane,
    clock: &dyn DiscoveryClock,
    deadline: u64,
) -> ControlDisposition {
    let Some(esc) = reference_key_bytes("esc") else {
        return ControlDisposition::Unverified {
            message: "the picker was open and could not be closed".into(),
        };
    };
    if pane.write(esc).await.is_err() {
        return ControlDisposition::Unverified {
            message: "the picker was open and Escape could not be confirmed".into(),
        };
    }
    match observe_codex_picker(pane, clock, deadline, true).await {
        Observed::Closed(_) => rejected("UNSUPPORTED", "that value is not a row in the native picker"),
        Observed::Menu(_, _) | Observed::Unreadable => ControlDisposition::Unverified {
            message: "the native picker stayed open".into(),
        },
    }
}

fn confirm_picker_close(
    text: &str,
    composer: &ReferenceComposerCapabilities,
    control_id: &str,
    value_id: &str,
) -> ControlDisposition {
    if parse_reference_codex_prompt(text)
        .is_some_and(|parsed| parsed.responder == ReferenceCodexResponder::Model)
    {
        return ControlDisposition::Unverified {
            message: "the native picker stayed open after the answer".into(),
        };
    }
    if rejection_in(text) {
        return rejected("REJECTED", "the provider rejected the control");
    }
    let mut tokens = vec![value_id.to_string()];
    if control_id == CONTROL_MODEL_ID {
        if let Some(label) = composer.models.as_ref().and_then(|models| {
            models
                .choices
                .iter()
                .find(|choice| choice.id == value_id)
                .map(|choice| choice.label.clone())
        }) {
            tokens.push(label);
        }
    }
    if tokens.iter().any(|token| token_in(text, token) || (token.is_ascii() && token_in_ignore_case(text, token)))
    {
        return ControlDisposition::Applied;
    }
    ControlDisposition::Unverified {
        message: "the picker closed, but the apply could not be confirmed".into(),
    }
}

fn token_in_ignore_case(text: &str, value: &str) -> bool {
    if value.is_empty() || !value.is_ascii() {
        return false;
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_value = value.to_ascii_lowercase();
    lower_text.match_indices(&lower_value).any(|(at, _)| {
        let before_ok = lower_text[..at]
            .chars()
            .next_back()
            .is_none_or(|ch| !is_id_char(ch));
        let after = at + lower_value.len();
        let after_ok = lower_text
            .get(after..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(|ch| !is_id_char(ch));
        before_ok && after_ok && lower_text.is_char_boundary(at) && lower_text.is_char_boundary(after)
    })
}

async fn discover(provider: &str, cwd: Option<&str>, probe: &dyn DiscoveryProbe) -> CacheState {
    let Some(spec) = probe_spec_for(provider, cwd) else {
        return CacheState::Absent;
    };
    let raw = probe.run(spec).await;
    match parse_probe(provider, raw) {
        Ok(models) => CacheState::Ready(composer_from_models(provider, &models)),
        Err(_) => CacheState::Absent,
    }
}

async fn run_host_probe(spec: ProbeSpec) -> ProbeRaw {
    let mut command = tokio::process::Command::new(spec.program);
    command
        .args(spec.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(cwd) = spec.cwd.as_deref() {
        command.current_dir(cwd);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return ProbeRaw::SpawnFailed,
    };
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(spec.stdin.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    let mut stdout = child.stdout.take();
    let read = async {
        let Some(stdout) = stdout.as_mut() else {
            return ProbeRaw::Stdout(String::new());
        };
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stdout.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    let room = DISCOVERY_STDOUT_LIMIT.saturating_add(1).saturating_sub(buf.len());
                    if room == 0 {
                        return ProbeRaw::Oversized;
                    }
                    let take = n.min(room);
                    buf.extend_from_slice(&chunk[..take]);
                    if buf.len() > DISCOVERY_STDOUT_LIMIT {
                        return ProbeRaw::Oversized;
                    }
                }
                Err(_) => break,
            }
        }
        ProbeRaw::Stdout(String::from_utf8_lossy(&buf).into_owned())
    };
    let raw = match tokio::time::timeout(DISCOVERY_PROBE_TIMEOUT, read).await {
        Ok(raw) => raw,
        Err(_) => ProbeRaw::Timeout,
    };
    if matches!(raw, ProbeRaw::Timeout | ProbeRaw::Oversized) {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    raw
}

/// JSON `data` for a settled apply. Busy is not a settled apply.
pub fn outcome_data(disposition: &ControlDisposition) -> serde_json::Value {
    match disposition {
        ControlDisposition::Applied => serde_json::json!({ "outcome": "applied" }),
        ControlDisposition::Rejected { code, message } => {
            serde_json::json!({ "outcome": "rejected", "code": code, "message": message })
        }
        ControlDisposition::Unverified { message } => serde_json::json!({
            "outcome": "unknown",
            "code": REFERENCE_OUTCOME_UNKNOWN_CODE,
            "message": message,
        }),
    }
}

/// Provider kind the history route may probe. Other kinds omit composer without a probe.
pub fn probed_kind(registry_id: &str) -> Option<ReferenceNativeHistoryKind> {
    match ReferenceNativeHistoryKind::from_registry_id(registry_id) {
        ReferenceNativeHistoryKind::Claude => Some(ReferenceNativeHistoryKind::Claude),
        ReferenceNativeHistoryKind::Codex => Some(ReferenceNativeHistoryKind::Codex),
        ReferenceNativeHistoryKind::Grok => Some(ReferenceNativeHistoryKind::Grok),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct ManualClock(AtomicUsize);

    impl DiscoveryClock for ManualClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst) as u64
        }

        fn sleep_ms(&self, ms: u64) -> BoxFuture<'static, ()> {
            self.0.fetch_add(ms as usize, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    struct FixedCli;

    impl CliIdentityLookup for FixedCli {
        fn lookup<'a>(
            &'a self,
            provider: &'a str,
            launch_program: Option<&'a str>,
        ) -> BoxFuture<'a, Option<CliIdentity>> {
            let provider = provider.to_string();
            let allowed = launch_maps_to_verified_cli(&provider, launch_program);
            Box::pin(async move {
                if !allowed {
                    return None;
                }
                Some(test_cli(&provider))
            })
        }
    }

    struct AbsentCli;

    impl CliIdentityLookup for AbsentCli {
        fn lookup<'a>(
            &'a self,
            _provider: &'a str,
            _launch_program: Option<&'a str>,
        ) -> BoxFuture<'a, Option<CliIdentity>> {
            Box::pin(async { None })
        }
    }

    struct SwitchCli {
        mtime: AtomicUsize,
    }

    impl CliIdentityLookup for SwitchCli {
        fn lookup<'a>(
            &'a self,
            provider: &'a str,
            launch_program: Option<&'a str>,
        ) -> BoxFuture<'a, Option<CliIdentity>> {
            let provider = provider.to_string();
            let allowed = launch_maps_to_verified_cli(&provider, launch_program);
            let mtime = self.mtime.load(Ordering::SeqCst) as u64;
            Box::pin(async move {
                if !allowed {
                    return None;
                }
                Some(CliIdentity {
                    path: format!("/verified/{provider}"),
                    mtime_secs: mtime,
                    size: 7,
                })
            })
        }
    }

    fn test_cli(provider: &str) -> CliIdentity {
        CliIdentity {
            path: format!("/verified/{provider}"),
            mtime_secs: 42,
            size: 7,
        }
    }

    fn scripted_host() -> ComposerHost {
        ComposerHost::with_cli(Arc::new(FixedCli))
    }

    struct ScriptProbe {
        raw: Mutex<ProbeRaw>,
        calls: AtomicUsize,
    }

    impl DiscoveryProbe for ScriptProbe {
        fn run<'a>(&'a self, _spec: ProbeSpec) -> BoxFuture<'a, ProbeRaw> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let raw = self.raw.lock().clone();
            Box::pin(async move { raw })
        }
    }

    fn claude_stdout() -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{}","response":{{"models":[{{"value":"default","resolvedModel":"sonnet","displayName":"Default"}},{{"value":"sonnet","resolvedModel":"sonnet","displayName":"Sonnet","supportsEffort":true,"supportedEffortLevels":["low","high"]}}]}}}}}}"#,
            model_discovery::CLAUDE_LIST_MODELS_REQUEST_ID
        )
    }

    fn native_page() -> ReferenceHistoryPage {
        ReferenceHistoryPage {
            source: super::super::types::ReferenceHistorySource::ClaudeTranscript,
            availability: ReferenceHistoryAvailability::Native,
            turns: Vec::new(),
            cursor: None,
            has_more: false,
            generation: "g".into(),
            unavailable_reason: None,
            composer: None,
        }
    }

    fn target(session: &str) -> ReferenceTargetRef {
        ReferenceTargetRef::without_provider_session(crate::scoped_contracts::TargetRef {
            host_id: "local".into(),
            owner_id: "owner".into(),
            epoch: crate::scoped_contracts::Epoch(1),
            backend_session_id: session.into(),
        })
    }

    struct HoldPane {
        entered: tokio::sync::mpsc::UnboundedSender<()>,
        release: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>>>,
        writes: Arc<AtomicUsize>,
        before: Mutex<Option<String>>,
        after: Mutex<Option<String>>,
        fail_write: bool,
    }

    impl ControlPane for HoldPane {
        fn write<'a>(&'a self, _data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
            let entered = self.entered.clone();
            let release = Arc::clone(&self.release);
            let writes = Arc::clone(&self.writes);
            let fail = self.fail_write;
            Box::pin(async move {
                let _ = entered.send(());
                if let Some(rx) = release.lock().await.take() {
                    let _ = rx.await;
                }
                writes.fetch_add(1, Ordering::SeqCst);
                if fail {
                    Err("write failed".into())
                } else {
                    Ok(())
                }
            })
        }

        fn screen<'a>(&'a self) -> BoxFuture<'a, Option<String>> {
            Box::pin(async move {
                if self.writes.load(Ordering::SeqCst) == 0 {
                    self.before.lock().clone()
                } else {
                    self.after.lock().clone()
                }
            })
        }
    }

    #[test]
    fn a_custom_launch_disables_discovery() {
        assert!(launch_maps_to_verified_cli("claude", None));
        assert!(launch_maps_to_verified_cli("claude", Some("/usr/local/bin/claude")));
        assert!(launch_maps_to_verified_cli("codex", Some("codex.exe")));
        assert!(!launch_maps_to_verified_cli("claude", Some("/opt/my-claude-wrapper")));
        assert!(!launch_maps_to_verified_cli("pi", None));
        assert!(!launch_maps_to_verified_cli("claude", Some("node")));
    }

    #[tokio::test]
    async fn discovery_success_populates_composer_and_failures_omit_it() {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(1_000));
        let probe = ScriptProbe {
            raw: Mutex::new(ProbeRaw::Stdout(claude_stdout())),
            calls: AtomicUsize::new(0),
        };
        let mut page = native_page();
        host.attach_composer(
            &mut page,
            "claude",
            Some("/work"),
            Some("/home"),
            None,
            &ComposerDiscovery { probe: &probe, clock: &clock },
        )
            .await;
        let composer = page.composer.expect("success publishes composer");
        assert_eq!(composer.models.as_ref().unwrap().current.as_deref(), Some("sonnet"));
        assert_eq!(composer.models.as_ref().unwrap().apply, ReferenceComposerModelApply::Command);
        assert_eq!(composer.options[0].id, "effort");
        assert_eq!(composer.options[0].values, ["low", "high"]);
        assert!(composer.slash_commands.is_empty());
        assert!(composer.follow_up.queue);
        assert!(!composer.follow_up.steer);

        for raw in [
            ProbeRaw::Timeout,
            ProbeRaw::Oversized,
            ProbeRaw::SpawnFailed,
            ProbeRaw::Stdout("not a model list".into()),
        ] {
            let host = ComposerHost::new();
            let probe = ScriptProbe {
                raw: Mutex::new(raw),
                calls: AtomicUsize::new(0),
            };
            let mut page = native_page();
            host.attach_composer(
                &mut page,
                "claude",
                Some("/work"),
                Some("/home"),
                None,
                &ComposerDiscovery { probe: &probe, clock: &clock },
            )
            .await;
            assert!(page.composer.is_none(), "failure omits composer");
        }

        let mut other = native_page();
        other.source = super::super::types::ReferenceHistorySource::PiTranscript;
        host.attach_composer(
            &mut other,
            "pi",
            Some("/work"),
            None,
            None,
            &ComposerDiscovery { probe: &probe, clock: &clock },
        )
            .await;
        assert!(other.composer.is_none());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1, "pi does not probe");
    }

    #[tokio::test]
    async fn discovery_cache_expires_with_the_clock_and_does_not_sleep() {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(5_000));
        let probe = ScriptProbe {
            raw: Mutex::new(ProbeRaw::Stdout(claude_stdout())),
            calls: AtomicUsize::new(0),
        };
        assert!(host
            .composer_for("claude", Some("/work"), Some("/home"), None, &probe, &clock)
            .await
            .is_some());
        assert!(host
            .composer_for("claude", Some("/work"), Some("/home"), None, &probe, &clock)
            .await
            .is_some());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        clock.0.store(
            5_000 + DISCOVERY_CACHE_TTL.as_millis() as usize,
            Ordering::SeqCst,
        );
        assert!(host
            .composer_for("claude", Some("/work"), Some("/home"), None, &probe, &clock)
            .await
            .is_some());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);

        *probe.raw.lock() = ProbeRaw::Stdout("garbage".into());
        clock.0.fetch_add(DISCOVERY_CACHE_TTL.as_millis() as usize, Ordering::SeqCst);
        assert!(host
            .composer_for("claude", Some("/work"), Some("/home"), None, &probe, &clock)
            .await
            .is_none());
        assert!(host
            .composer_for("claude", Some("/work"), Some("/home"), None, &probe, &clock)
            .await
            .is_none());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 3, "absent is cached until ttl");
    }

    #[tokio::test]
    async fn a_wrapper_launch_does_not_probe() {
        let host = ComposerHost::new();
        let clock = ManualClock(AtomicUsize::new(1));
        let probe = ScriptProbe {
            raw: Mutex::new(ProbeRaw::Stdout(claude_stdout())),
            calls: AtomicUsize::new(0),
        };
        let mut page = native_page();
        host.attach_composer(
            &mut page,
            "claude",
            Some("/work"),
            None,
            Some("my-claude-wrapper"),
            &ComposerDiscovery { probe: &probe, clock: &clock },
        )
        .await;
        assert!(page.composer.is_none());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    }

    fn seeded_host() -> (ComposerHost, ManualClock) {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(10));
        let composer = composer_from_models(
            "claude",
            &model_discovery::parse_claude_list_models(&claude_stdout()).unwrap(),
        );
        host.cache.lock().insert(
            discovery_cache_key(
                &crate::remote::reference_chat::files::reference_host_id(),
                "claude",
                Some("/work"),
                Some("/home"),
                &test_cli("claude"),
            ),
            CacheEntry {
                state: CacheState::Ready(composer),
                expires_at: 10 + DISCOVERY_CACHE_TTL.as_millis() as u64,
            },
        );
        (host, clock)
    }

    fn pane(after: Option<&str>, fail_write: bool) -> (HoldPane, tokio::sync::mpsc::UnboundedReceiver<()>, tokio::sync::oneshot::Sender<()>, Arc<AtomicUsize>) {
        let (entered_tx, entered_rx) = tokio::sync::mpsc::unbounded_channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let writes = Arc::new(AtomicUsize::new(0));
        let pane = HoldPane {
            entered: entered_tx,
            release: Arc::new(tokio::sync::Mutex::new(Some(release_rx))),
            writes: Arc::clone(&writes),
            before: Mutex::new(Some(String::new())),
            after: Mutex::new(after.map(str::to_string)),
            fail_write,
        };
        (pane, entered_rx, release_tx, writes)
    }

    #[tokio::test]
    async fn control_rejects_unknown_ids_without_typing() {
        let (host, clock) = seeded_host();
        let queue = ReferenceInputQueue::new();
        let (pane, _entered, release, writes) = pane(Some("/model nope"), false);
        let outcome = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "claude",
                control_id: "model",
                value_id: "not-a-model",
                request_id: "req-unknown",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue,
                pane: &pane,
                clock: &clock,
                authorize: &|| Ok(()),
            })
            .await;
        drop(release);
        match outcome {
            ControlRouteOutcome::Settled(ControlDisposition::Rejected { code, .. }) => {
                assert_eq!(code, "UNSUPPORTED");
            }
            other => panic!("unknown id must be rejected, got {other:?}"),
        }
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn one_target_admits_one_apply_and_replays_the_request_id() {
        let (host, clock) = seeded_host();
        let queue = ReferenceInputQueue::new();
        let (pane, mut entered, release, writes) = pane(Some("/model sonnet"), false);
        let host = Arc::new(host);
        let host_run = Arc::clone(&host);
        let writes_run = Arc::clone(&writes);
        let first = tokio::spawn(async move {
            host_run
                .apply(ControlApply {
                    target: &target("s"),
                    registry_id: "claude",
                    control_id: "model",
                    value_id: "sonnet",
                    request_id: "req-1",
                    cwd: Some("/work"),
                    home: Some("/home"),
                    queue: &queue,
                    pane: &pane,
                    clock: &clock,
                    authorize: &|| Ok(()),
                })
                .await
        });
        entered.recv().await.expect("first apply entered the writer");
        let (pane2, _entered2, release2, writes2) = self::pane(Some("/model sonnet"), false);
        let queue2 = ReferenceInputQueue::new();
        let clock2 = ManualClock(AtomicUsize::new(10));
        let second = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "claude",
                control_id: "model",
                value_id: "sonnet",
                request_id: "req-2",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue2,
                pane: &pane2,
                clock: &clock2,
                authorize: &|| Ok(()),
            })
            .await;
        assert_eq!(second, ControlRouteOutcome::Busy);
        assert_eq!(writes2.load(Ordering::SeqCst), 0);
        drop(release2);
        release.send(()).expect("release first");
        let settled = first.await.expect("first finished");
        assert_eq!(settled, ControlRouteOutcome::Settled(ControlDisposition::Applied));
        assert_eq!(writes_run.load(Ordering::SeqCst), 1);

        let (pane3, _e3, release3, writes3) = self::pane(Some("should not type"), false);
        let queue3 = ReferenceInputQueue::new();
        let replay = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "claude",
                control_id: "model",
                value_id: "sonnet",
                request_id: "req-1",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue3,
                pane: &pane3,
                clock: &clock2,
                authorize: &|| Ok(()),
            })
            .await;
        drop(release3);
        assert_eq!(replay, ControlRouteOutcome::Settled(ControlDisposition::Applied));
        assert_eq!(writes3.load(Ordering::SeqCst), 0, "replay must not type");
        assert_eq!(writes_run.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_unconfirmed_write_is_unverified_and_not_replayed_as_a_new_apply() {
        let (host, clock) = seeded_host();
        let queue = ReferenceInputQueue::new();
        let (pane, mut entered, release, writes) = pane(None, true);
        let host = Arc::new(host);
        let running = Arc::clone(&host);
        let task = tokio::spawn(async move {
            running
                .apply(ControlApply {
                    target: &target("s"),
                    registry_id: "claude",
                    control_id: "model",
                    value_id: "sonnet",
                    request_id: "req-unknown-outcome",
                    cwd: Some("/work"),
                    home: Some("/home"),
                    queue: &queue,
                    pane: &pane,
                    clock: &clock,
                    authorize: &|| Ok(()),
                })
                .await
        });
        entered.recv().await.expect("write started");
        release.send(()).unwrap();
        let outcome = task.await.unwrap();
        match outcome {
            ControlRouteOutcome::Settled(ControlDisposition::Unverified { .. }) => {}
            other => panic!("expected unverified, got {other:?}"),
        }
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        let (pane2, _e, release2, writes2) = self::pane(Some("/model sonnet"), false);
        let queue2 = ReferenceInputQueue::new();
        let clock2 = ManualClock(AtomicUsize::new(10));
        let replay = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "claude",
                control_id: "model",
                value_id: "sonnet",
                request_id: "req-unknown-outcome",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue2,
                pane: &pane2,
                clock: &clock2,
                authorize: &|| Ok(()),
            })
            .await;
        drop(release2);
        assert!(matches!(
            replay,
            ControlRouteOutcome::Settled(ControlDisposition::Unverified { .. })
        ));
        assert_eq!(writes2.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_missing_probe_program_is_spawn_failed() {
        let raw = run_host_probe(ProbeSpec {
            program: "ferryx-discovery-probe-missing",
            args: &[],
            stdin: String::new(),
            cwd: None,
        })
        .await;
        assert_eq!(raw, ProbeRaw::SpawnFailed);
    }

    #[test]
    fn confirm_delta_does_not_treat_an_unchanged_screen_as_applied() {
        assert!(matches!(
            confirm_delta(Some(""), Some(""), "/model sonnet", "sonnet"),
            ControlDisposition::Unverified { .. }
        ));
        assert!(matches!(
            confirm_delta(Some(""), Some("/model sonnet\r"), "/model sonnet", "sonnet"),
            ControlDisposition::Applied
        ));
        assert!(matches!(
            confirm_delta(Some(""), Some("unknown model\r"), "/model sonnet", "sonnet"),
            ControlDisposition::Rejected { .. }
        ));
    }

    #[test]
    fn discovery_cache_key_includes_owning_host_and_cli_identity() {
        let cli = test_cli("claude");
        let other = CliIdentity { mtime_secs: 99, ..cli.clone() };
        let left = discovery_cache_key("host-a", "claude", Some("/work"), None, &cli);
        let right = discovery_cache_key("host-b", "claude", Some("/work"), None, &cli);
        let moved = discovery_cache_key("host-a", "claude", Some("/work"), None, &other);
        assert_ne!(left, right);
        assert_ne!(left, moved);
        assert!(left.contains("host-a"));
        assert!(left.contains(&cli.path));
        assert!(left.contains(&cli.mtime_secs.to_string()));
        assert!(left.contains(&cli.size.to_string()));
    }

    #[test]
    fn a_verified_cli_identity_is_path_mtime_and_size_and_a_custom_launch_is_absent() {
        let dir = std::env::temp_dir().join(format!("ferryx-cli-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("claude");
        std::fs::write(&path, b"verified").unwrap();
        let id = verified_cli_identity("claude", Some(path.to_str().unwrap()), &[])
            .expect("absolute verified cli");
        assert_eq!(id.path, path.to_string_lossy());
        assert_eq!(id.size, b"verified".len() as u64);
        assert!(id.mtime_secs > 0);
        let found = verified_cli_identity("claude", None, std::slice::from_ref(&dir)).expect("path search");
        assert_eq!(found.path, id.path);
        assert!(verified_cli_identity("claude", Some("my-claude-wrapper"), std::slice::from_ref(&dir)).is_none());
        assert!(verified_cli_identity("claude", Some("./claude"), std::slice::from_ref(&dir)).is_none());
        assert!(verified_cli_identity("claude", Some("/no/such/claude"), &[]).is_none());
        assert!(verified_cli_identity("pi", None, std::slice::from_ref(&dir)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_slash_commands_and_follow_up_come_from_provider_evidence() {
        let stdout = format!(
            "{}\n{}",
            r#"{"type":"system","subtype":"init","slash_commands":["compact",{"name":"review","description":"Review"}]}"#,
            claude_stdout()
        );
        let parsed = model_discovery::parse_claude_list_models(&stdout).unwrap();
        let composer = composer_from_models("claude", &parsed);
        assert_eq!(composer.slash_commands[0].name, "compact");
        assert_eq!(composer.slash_commands[1].name, "review");
        assert_eq!(composer.slash_commands[1].description.as_deref(), Some("Review"));
        assert!(composer.follow_up.queue);
        assert!(!composer.follow_up.steer);
        let codex = composer_from_models(
            "codex",
            &model_discovery::DiscoveredModels {
                current: None,
                models: vec![model_discovery::DiscoveredModel {
                    id: "gpt".into(),
                    label: "GPT".into(),
                    efforts: Vec::new(),
                }],
                slash_commands: Vec::new(),
            },
        );
        assert!(codex.slash_commands.is_empty());
        assert!(codex.follow_up.queue && !codex.follow_up.steer);
        assert!(!follow_up_for("pi").queue);
        assert!(!follow_up_for("pi").steer);
    }

    #[tokio::test]
    async fn an_unresolved_cli_disables_discovery_without_a_probe() {
        let host = ComposerHost::with_cli(Arc::new(AbsentCli));
        let clock = ManualClock(AtomicUsize::new(1));
        let probe = ScriptProbe {
            raw: Mutex::new(ProbeRaw::Stdout(claude_stdout())),
            calls: AtomicUsize::new(0),
        };
        let mut page = native_page();
        host.attach_composer(
            &mut page,
            "claude",
            Some("/work"),
            None,
            None,
            &ComposerDiscovery { probe: &probe, clock: &clock },
        )
            .await;
        assert!(page.composer.is_none());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_changed_cli_identity_does_not_reuse_the_cache() {
        let switch = Arc::new(SwitchCli { mtime: AtomicUsize::new(1) });
        let cli: Arc<dyn CliIdentityLookup> = Arc::clone(&switch) as Arc<dyn CliIdentityLookup>;
        let host = ComposerHost::with_cli(cli);
        let clock = ManualClock(AtomicUsize::new(1));
        let probe = ScriptProbe {
            raw: Mutex::new(ProbeRaw::Stdout(claude_stdout())),
            calls: AtomicUsize::new(0),
        };
        assert!(host
            .composer_for("claude", Some("/work"), None, None, &probe, &clock)
            .await
            .is_some());
        assert!(host
            .composer_for("claude", Some("/work"), None, None, &probe, &clock)
            .await
            .is_some());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        switch.mtime.store(2, Ordering::SeqCst);
        assert!(host
            .composer_for("claude", Some("/work"), None, None, &probe, &clock)
            .await
            .is_some());
        assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    }

    fn seed_codex(host: &ComposerHost, now_ms: u64) {
        let composer = composer_from_models(
            "codex",
            &model_discovery::DiscoveredModels {
                current: Some("gpt-6-astra".into()),
                models: vec![
                    model_discovery::DiscoveredModel {
                        id: "gpt-6-astra".into(),
                        label: "GPT-6-Astra".into(),
                        efforts: vec!["low".into(), "medium".into(), "high".into()],
                    },
                    model_discovery::DiscoveredModel {
                        id: "gpt-6-sol".into(),
                        label: "GPT-6-Sol".into(),
                        efforts: vec!["low".into(), "medium".into(), "high".into()],
                    },
                ],
                slash_commands: Vec::new(),
            },
        );
        host.cache.lock().insert(
            discovery_cache_key(
                &crate::remote::reference_chat::files::reference_host_id(),
                "codex",
                Some("/work"),
                Some("/home"),
                &test_cli("codex"),
            ),
            CacheEntry {
                state: CacheState::Ready(composer),
                expires_at: now_ms + DISCOVERY_CACHE_TTL.as_millis() as u64,
            },
        );
    }

    struct SeqPane {
        screens: Vec<String>,
        writes: Mutex<Vec<Vec<u8>>>,
    }

    impl ControlPane for SeqPane {
        fn write<'a>(&'a self, data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
            self.writes.lock().push(data.to_vec());
            Box::pin(async { Ok(()) })
        }

        fn screen<'a>(&'a self) -> BoxFuture<'a, Option<String>> {
            let n = self.writes.lock().len();
            let text = self
                .screens
                .get(n)
                .cloned()
                .or_else(|| self.screens.last().cloned())
                .unwrap_or_default();
            Box::pin(async move { Some(text) })
        }
    }

    fn joined_writes(pane: &SeqPane) -> String {
        pane.writes
            .lock()
            .iter()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .collect::<Vec<_>>()
            .join("|")
    }

    #[tokio::test]
    async fn codex_model_apply_opens_the_picker_and_answers_the_menu() {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(10));
        seed_codex(&host, 10);
        let menu = include_str!("fixtures/codex/model-list.txt").to_string();
        let pane = SeqPane {
            screens: vec![String::new(), menu, "selected GPT-6-Sol for this session".into()],
            writes: Mutex::new(Vec::new()),
        };
        let queue = ReferenceInputQueue::new();
        let outcome = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "codex",
                control_id: "model",
                value_id: "gpt-6-sol",
                request_id: "req-picker",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue,
                pane: &pane,
                clock: &clock,
                authorize: &|| Ok(()),
            })
            .await;
        assert_eq!(outcome, ControlRouteOutcome::Settled(ControlDisposition::Applied));
        let writes = pane.writes.lock().clone();
        assert_eq!(writes.first().map(Vec::as_slice), Some(b"/model\r".as_slice()));
        let typed = joined_writes(&pane);
        assert!(!typed.contains("/model "), "a model id is not typed as a command");
        assert!(!typed.contains("/model gpt-6-sol"));
        assert!(writes.iter().any(|bytes| bytes == b"\x1b[B" || bytes == b"\r"));
    }

    #[tokio::test]
    async fn codex_effort_apply_answers_the_level_menu_with_the_session_key() {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(10));
        seed_codex(&host, 10);
        let levels = include_str!("fixtures/codex/model-levels.txt").to_string();
        let pane = SeqPane {
            screens: vec![String::new(), levels, "reasoning High for this session".into()],
            writes: Mutex::new(Vec::new()),
        };
        let queue = ReferenceInputQueue::new();
        let outcome = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "codex",
                control_id: "effort",
                value_id: "high",
                request_id: "req-effort",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue,
                pane: &pane,
                clock: &clock,
                authorize: &|| Ok(()),
            })
            .await;
        assert_eq!(outcome, ControlRouteOutcome::Settled(ControlDisposition::Applied));
        let typed = joined_writes(&pane);
        assert!(typed.starts_with("/model\r"));
        assert!(!typed.contains("/model "));
        assert!(!typed.contains("/effort"));
        assert!(pane.writes.lock().iter().any(|bytes| bytes == b"s"));
    }

    #[tokio::test]
    async fn codex_picker_that_is_not_observed_is_outcome_unknown_and_is_not_retried() {
        let host = scripted_host();
        let clock = ManualClock(AtomicUsize::new(10));
        seed_codex(&host, 10);
        let pane = SeqPane {
            screens: vec!["hello, you".into()],
            writes: Mutex::new(Vec::new()),
        };
        let queue = ReferenceInputQueue::new();
        let outcome = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "codex",
                control_id: "model",
                value_id: "gpt-6-sol",
                request_id: "req-missing-picker",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue,
                pane: &pane,
                clock: &clock,
                authorize: &|| Ok(()),
            })
            .await;
        assert!(matches!(
            outcome,
            ControlRouteOutcome::Settled(ControlDisposition::Unverified { .. })
        ));
        assert_eq!(pane.writes.lock().len(), 1);
        assert_eq!(pane.writes.lock()[0], b"/model\r");
        let replay = host
            .apply(ControlApply {
                target: &target("s"),
                registry_id: "codex",
                control_id: "model",
                value_id: "gpt-6-sol",
                request_id: "req-missing-picker",
                cwd: Some("/work"),
                home: Some("/home"),
                queue: &queue,
                pane: &pane,
                clock: &clock,
                authorize: &|| Ok(()),
            })
            .await;
        assert!(matches!(
            replay,
            ControlRouteOutcome::Settled(ControlDisposition::Unverified { .. })
        ));
        assert_eq!(pane.writes.lock().len(), 1, "an unknown apply is not typed again");
    }

    #[tokio::test]
    async fn codex_picker_apply_holds_the_per_pane_input_queue() {
        let host = Arc::new(scripted_host());
        let clock = ManualClock(AtomicUsize::new(10));
        seed_codex(&host, 10);
        let queue = Arc::new(ReferenceInputQueue::new());
        let (pane, mut entered, release, _writes) = pane(Some("not a picker"), false);
        let host_run = Arc::clone(&host);
        let queue_run = Arc::clone(&queue);
        let apply_task = tokio::spawn(async move {
            host_run
                .apply(ControlApply {
                    target: &target("s"),
                    registry_id: "codex",
                    control_id: "model",
                    value_id: "gpt-6-sol",
                    request_id: "req-held",
                    cwd: Some("/work"),
                    home: Some("/home"),
                    queue: queue_run.as_ref(),
                    pane: &pane,
                    clock: &clock,
                    authorize: &|| Ok(()),
                })
                .await
        });
        entered.recv().await.expect("picker open entered the writer");
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let queue_other = Arc::clone(&queue);
        let other = tokio::spawn(async move {
            queue_other
                .run_under_step(&target("s"), async move {
                    let _ = started_tx.send(());
                })
                .await;
        });
        assert!(started_rx.try_recv().is_err(), "the queue is held by the picker apply");
        release.send(()).unwrap();
        started_rx.recv().await.expect("the next step runs after the apply");
        let _ = apply_task.await;
        let _ = other.await;
    }
}
