//! Automatic supervisor setup: the one owner action that turns the bundled mhc engine on,
//! records the owner's setup choices, restarts the hub generation in-process so the engine is
//! installed (never the daemon, so PTY sessions stay up), creates the owner identity and maps
//! the requested projects. Every step is idempotent; the answer is the shared SetupStatus.
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use ferryx_hub::meta::mhc_profile::{MhcProfileConfig, ENABLE_KEY};
use ferryx_hub::meta::mhc_supervisor::{HealthState, SupervisorHealth};
use ferryx_hub::supervisor::setup::{self, EngineView};
use ferryx_hub::supervisor::SupervisorError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{start_generation, Generation, HubStartError, HUB_TASKS, SUPERVISOR_LAUNCH, SUPERVISOR_VIEW};
use crate::remote::state::RemoteGatewayState;

/// One project the owner asked setup to map: a display name and a local checkout path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupProject {
    #[serde(default)]
    pub name: String,
    pub path: String,
}

/// What one setup run does besides the steps it always takes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRunRequest {
    #[serde(default)]
    pub import_memory: bool,
    #[serde(default)]
    pub projects: Vec<SetupProject>,
}

/// Why setup did not run. Each maps to one stable wire code.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("this daemon handed the hub over to its successor")]
    HandedOver,
    #[error("hub env file: {0}")]
    EnvFile(String),
    #[error("the Slack owner {0} is configured but its Slack team could not be resolved yet; run setup again once Slack answers")]
    SlackTeamUnresolved(String),
    #[error("{0}")]
    Supervisor(#[from] SupervisorError),
}

/// The wire code of [`SetupError::SlackTeamUnresolved`]: retryable, nothing was provisioned.
pub const SLACK_TEAM_UNRESOLVED: &str = "SLACK_TEAM_UNRESOLVED";

impl SetupError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "SUPERVISOR_UNAVAILABLE",
            Self::HandedOver => "INVALID_STATE",
            Self::EnvFile(_) => "STORAGE",
            Self::SlackTeamUnresolved(_) => SLACK_TEAM_UNRESOLVED,
            Self::Supervisor(error) => error.code(),
        }
    }
}

impl From<HubStartError> for SetupError {
    fn from(error: HubStartError) -> Self {
        match error {
            HubStartError::Unavailable => Self::Unavailable,
            HubStartError::HandedOver => Self::HandedOver,
            HubStartError::Storage(message) => Self::EnvFile(message),
        }
    }
}

/// The Slack owner, when this hub knows both halves of slack:<team>:<user>.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SlackOwner {
    pub team: Option<String>,
    pub user: Option<String>,
}

/// The hub pool, and whether it is this daemon's live hub (whose generation setup may restart).
pub(super) fn hub_pool(state: &RemoteGatewayState) -> Result<(ferryx_hub::goals::SqlitePool, bool), SetupError> {
    #[cfg(test)]
    if let Some((pool, _)) = state.supervisor_test_hub.read().clone() {
        return Ok((pool, false));
    }
    let _ = state;
    let (hub, _) = SUPERVISOR_VIEW.get().ok_or(SetupError::Unavailable)?;
    Ok((hub.pool.clone(), true))
}

/// Runs blocking work (the env file reads and writes) on the blocking pool, never on an
/// async runtime thread.
async fn off_runtime<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, SetupError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| SetupError::EnvFile(format!("blocking task failed: {error}")))
}

/// Where setup reads and writes the engine flag: this user's hub env file and this process's
/// environment, or (tests) a given file and a given process value.
#[derive(Debug, Clone)]
pub(crate) enum HubEnv {
    Live,
    #[cfg(test)]
    At { file: PathBuf, process: Option<String> },
}

impl HubEnv {
    /// The env file entries as they are on disk now. Blocking.
    fn read(&self) -> BTreeMap<String, String> {
        match self {
            Self::Live => ferryx_hub::hub_env::read_file_env(),
            #[cfg(test)]
            Self::At { file, .. } => read_env_at(file),
        }
    }

    /// This process's ENABLE_KEY, which may turn the engine off but never on.
    fn process(&self) -> Option<String> {
        match self {
            Self::Live => std::env::var(ENABLE_KEY).ok(),
            #[cfg(test)]
            Self::At { process, .. } => process.clone(),
        }
    }

    /// Merges ENABLE_KEY=1 into the env file. Blocking.
    fn enable(&self) -> Result<(), SetupError> {
        let entries = BTreeMap::from([(ENABLE_KEY.to_owned(), "1".to_owned())]);
        let written = match self {
            Self::Live => ferryx_hub::hub_env::set_file_entries(&entries),
            #[cfg(test)]
            Self::At { file, .. } => ferryx_hub::hub_env::write_merged(file, &entries),
        };
        written.map(drop).map_err(|error| SetupError::EnvFile(error.to_string()))
    }
}

#[cfg(test)]
fn read_env_at(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .map(|text| ferryx_hub::hub_env::parse_env_file(&text))
        .unwrap_or_default()
}

/// The engine as setup sees it: the env file read now (off the runtime threads), this
/// process's override, and this process's engine.
pub(crate) async fn engine_view(env: &HubEnv) -> Result<EngineView, SetupError> {
    let reader = env.clone();
    let shared = off_runtime(move || reader.read()).await?;
    let installed = ferryx_hub::meta::mhc_supervisor::installed();
    Ok(EngineView {
        enabled: EngineView::enabled_for(&shared, env.process().as_deref()),
        installed: installed.is_some(),
        health: installed.map(|engine| engine.health()),
    })
}

/// This OS account's name, the local:<account> owner when Slack does not name one.
pub(crate) fn local_account() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

/// Slack's own auth.test team, when a bot token is configured.
#[cfg(feature = "hub-slack")]
async fn slack_auth_team() -> Option<String> {
    ferryx_hub::chat::slack::bot_token()?;
    ferryx_hub::chat::slack_backfill::identity()
        .await
        .ok()
        .map(|bot| bot.team_id)
        .filter(|team| !team.is_empty())
}

#[cfg(not(feature = "hub-slack"))]
async fn slack_auth_team() -> Option<String> {
    None
}

/// The Slack team this hub has seen (stored Slack sessions), else Slack's own auth.test.
async fn slack_owner(pool: &ferryx_hub::goals::SqlitePool) -> SlackOwner {
    let user = SUPERVISOR_LAUNCH
        .get()
        .and_then(|launch| launch.config.owner_slack_id.clone());
    let Some(user) = user else {
        return SlackOwner::default();
    };
    let team = match setup::known_slack_team(pool).await.ok().flatten() {
        Some(team) => Some(team),
        None => slack_auth_team().await,
    };
    SlackOwner { team, user: Some(user) }
}

/// Restarts this daemon's hub generation when one runs (see [`restart_if_running`]). Returns
/// whether it restarted. Never restarts the daemon or touches a PTY.
async fn restart_generation() -> Result<bool, SetupError> {
    let (Some((_, running)), Some(launch)) = (SUPERVISOR_VIEW.get(), SUPERVISOR_LAUNCH.get()) else {
        return Err(SetupError::Unavailable);
    };
    if HUB_TASKS.is_retired() {
        return Err(SetupError::HandedOver);
    }
    restart_if_running(running, || start_generation(running, launch)).await
}

/// Ends the running generation and starts a fresh one with start, as control stop then start
/// do, but without leaving the persisted stopped flag set. A hub the owner stopped stays
/// stopped (false): setup reports `restart_required` instead of starting it.
async fn restart_if_running<F, Fut>(
    running: &tokio::sync::Mutex<Option<Generation>>,
    start: F,
) -> Result<bool, SetupError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<bool, HubStartError>>,
{
    let Some(previous) = running.lock().await.take() else {
        return Ok(false);
    };
    previous.end().await;
    start().await?;
    Ok(true)
}

/// The installation's owner identity: the existing one, else a new one for the Slack owner
/// (when both team and user are known) or the local account. A configured Slack owner whose
/// team is not known yet is refused, never provisioned as the local account: that identity
/// would never match the owner's Slack messages.
async fn ensure_owner(
    pool: &ferryx_hub::goals::SqlitePool,
    slack: &SlackOwner,
    account: &str,
) -> Result<ferryx_hub::supervisor::SupervisorIdentity, SetupError> {
    if let Some(existing) = ferryx_hub::supervisor::identities(pool).await?.into_iter().next() {
        return Ok(existing);
    }
    if let (Some(user), None) = (&slack.user, &slack.team) {
        return Err(SetupError::SlackTeamUnresolved(user.clone()));
    }
    let principal = setup::default_owner_principal(slack.team.as_deref(), slack.user.as_deref(), account);
    Ok(ferryx_hub::supervisor::provision_identity(pool, &principal).await?)
}

/// Maps each requested checkout on this host the way ferryx-cli supervisor setup resolves a
/// local one. A path that is not a git checkout is refused on its own; the rest still map.
async fn map_projects(
    pool: &ferryx_hub::goals::SqlitePool,
    supervisor_id: &str,
    projects: Vec<SetupProject>,
) -> Vec<Value> {
    let host = ferryx_hub::agent_control::LOCAL_HOST_ID;
    let mut results = Vec::with_capacity(projects.len());
    for project in projects {
        let path = project.path.clone();
        let resolved = tokio::task::spawn_blocking(move || crate::cli::supervisor_goal::local_project_mapping(&path))
            .await
            .unwrap_or_else(|error| Err(("INTERNAL", error.to_string())));
        let (repo_identity, git_common_dir) = match resolved {
            Ok(mapping) => mapping,
            Err((code, message)) => {
                results.push(json!({ "path": project.path, "ok": false, "code": code, "message": message }));
                continue;
            }
        };
        let name = match project.name.trim() {
            "" => Path::new(&git_common_dir)
                .parent()
                .and_then(Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| repo_identity.clone()),
            name => name.to_owned(),
        };
        let mapped = async {
            let created = ferryx_hub::supervisor::create_project(pool, supervisor_id, &name, &repo_identity).await?;
            ferryx_hub::supervisor::bind_project_host(pool, &created.project_id, host, &git_common_dir).await?;
            Ok::<_, SupervisorError>(created)
        }
        .await;
        results.push(match mapped {
            Ok(created) => json!({
                "path": project.path,
                "ok": true,
                "projectId": created.project_id,
                "name": created.name,
                "repoIdentity": created.repo_identity,
                "gitCommonDir": git_common_dir,
            }),
            Err(error) => json!({ "path": project.path, "ok": false, "code": error.code(), "message": error.to_string() }),
        });
    }
    results
}

/// The setup run over an explicit pool and hub env. restart is false for a hub that is not
/// this daemon's live one (tests, the desktop-independent CLI), whose generation cannot be
/// restarted from here; a live hub the owner stopped is not restarted either. engine is the
/// health of the engine installed in the running generation, if any.
///
/// The owner is resolved first, so a refusal (a Slack owner whose team is not known yet)
/// changes nothing. The owner and projects are written before the generation restarts, so
/// the new generation's first Ready already finds them. An engine that is already Ready sees no new
/// Ready transition, so the run itself attempts the automatic cutover; the answer's
/// `seedDue` tells the caller to seed owner memory with that engine.
pub(crate) async fn run_setup_on(
    pool: &ferryx_hub::goals::SqlitePool,
    env: &HubEnv,
    request: SetupRunRequest,
    slack: &SlackOwner,
    account: &str,
    restart: bool,
    engine: Option<SupervisorHealth>,
) -> Result<Value, SetupError> {
    let identity = ensure_owner(pool, slack, account).await?;
    let writer = env.clone();
    off_runtime(move || {
        if MhcProfileConfig::enabled_in(&writer.read()) {
            Ok(())
        } else {
            writer.enable()
        }
    })
    .await??;
    setup::set_setup_flags(pool, Some(true), Some(request.import_memory)).await?;
    let projects = map_projects(pool, &identity.supervisor_id, request.projects).await;
    // An engine this process turns off never installs: a restart would change nothing.
    let may_restart = restart && engine.is_none() && engine_view(env).await?.enabled;
    let mut restarted = false;
    let mut cut_over = false;
    let mut seed_due = false;
    match &engine {
        None if may_restart => restarted = restart_generation().await?,
        Some(health) if health.state == HealthState::Ready => {
            cut_over = ferryx_hub::supervisor::cutover::auto_cutover(pool, Some(health)).await?.is_some();
            seed_due = request.import_memory;
        }
        _ => {}
    }
    let status = setup::setup_status(pool, engine_view(env).await?).await?;
    let mut answer = serde_json::to_value(status).unwrap_or(Value::Null);
    if let Some(fields) = answer.as_object_mut() {
        fields.insert("projectResults".into(), Value::Array(projects));
        fields.insert("restarted".into(), Value::Bool(restarted));
        fields.insert("cutOver".into(), Value::Bool(cut_over));
        fields.insert("seedDue".into(), Value::Bool(seed_due));
    }
    tracing::info!(restarted, cut_over, "supervisor setup ran");
    Ok(answer)
}

/// The generation's automatic setup task: on every engine transition to Ready it runs the
/// automatic cutover (which itself refuses unless the owner opted in, never after a rollback),
/// telling the Supervisor views when it moved the intake. After the first Ready it seeds owner
/// memory once when the owner opted in and no import finished yet. It waits on the engine's
/// health channel, never polls, and ends with the generation that spawned it.
pub(super) async fn auto_setup(
    engine: ferryx_hub::meta::mhc_supervisor::MhcSupervisor,
    pool: ferryx_hub::goals::SqlitePool,
    state: std::sync::Arc<RemoteGatewayState>,
    home: PathBuf,
) {
    let health = engine.subscribe_health();
    let seeded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let publish = || {
        crate::remote::server::agent_control_service::publish_supervisor_changed(&state, json!({ "setup": "cutover" }));
    };
    let seed = || {
        let (engine, pool, state, home) = (engine.clone(), pool.clone(), std::sync::Arc::clone(&state), home.clone());
        let seeded = std::sync::Arc::clone(&seeded);
        async move {
            use std::sync::atomic::Ordering;
            if !seeded.load(Ordering::SeqCst) {
                // Seeded only once an import ran or had already finished; a skip (no owner
                // or memory import off at that moment) leaves it to the next Ready or setup run.
                seeded.store(seed_if_wanted(&engine, &pool, &state, home).await, Ordering::SeqCst);
            }
        }
    };
    cut_over_on_ready(health, &pool, publish, seed).await;
}

/// The loop of [`auto_setup`] over one engine health channel: on every transition to Ready it
/// runs the automatic cutover (cut_over is told when it moved the intake), then on_ready.
/// Returns when the channel closes.
async fn cut_over_on_ready<C, R, Fut>(
    mut health: tokio::sync::watch::Receiver<SupervisorHealth>,
    pool: &ferryx_hub::goals::SqlitePool,
    mut cut_over: C,
    mut on_ready: R,
) where
    C: FnMut(),
    R: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut was_ready = false;
    loop {
        let current = health.borrow_and_update().clone();
        let ready = current.state == HealthState::Ready;
        if ready && !was_ready {
            match ferryx_hub::supervisor::cutover::auto_cutover(pool, Some(&current)).await {
                Ok(Some(transition)) => {
                    tracing::info!(steps = transition.steps.len(), "supervisor intake cut over automatically");
                    cut_over();
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(code = error.code(), "automatic cutover failed"),
            }
            on_ready().await;
        }
        was_ready = ready;
        if health.changed().await.is_err() {
            return;
        }
    }
}

/// Seeds owner memory when the owner opted in and no import finished yet. Returns whether
/// seeding is settled: an import ran, or one had already finished.
async fn seed_if_wanted(
    engine: &ferryx_hub::meta::mhc_supervisor::MhcSupervisor,
    pool: &ferryx_hub::goals::SqlitePool,
    state: &RemoteGatewayState,
    home: PathBuf,
) -> bool {
    let status = match setup::setup_status(pool, EngineView::default()).await {
        Ok(status) => status,
        Err(error) => {
            tracing::warn!(code = error.code(), "setup status was not read for memory seeding");
            return false;
        }
    };
    if status.memory_import.last_at.is_some() {
        return true;
    }
    let (Some(owner), true) = (status.owner, status.memory_import.enabled) else {
        return false;
    };
    let memory: std::sync::Arc<dyn ferryx_hub::meta::mhc_supervisor::OwnerMemory> = std::sync::Arc::new(engine.clone());
    match super::seed::seed_owner_memory(pool, memory, &owner.owner_principal, home).await {
        Ok(_) => {
            crate::remote::server::agent_control_service::publish_supervisor_changed(
                state,
                json!({ "setup": "memory" }),
            );
            true
        }
        Err(error) => {
            tracing::warn!(code = error.code(), "owner memory seeding failed");
            false
        }
    }
}

/// Adopts every adoptable legacy lane as the installation owner, each through the shared
/// supervisor request service exactly as one owner adopt request from caller does. One
/// refusal never fails the batch: it is listed with its code.
pub async fn supervisor_adopt_all(
    state: &RemoteGatewayState,
    caller: super::SupervisorCaller,
) -> Result<Value, SetupError> {
    let (pool, _) = hub_pool(state)?;
    let lanes = setup::adoptable_lane_keys(&pool).await?;
    let mut adopted = Vec::new();
    let mut refused = Vec::new();
    for lane in lanes {
        let envelope = json!({
            "requestId": format!("adopt-all:{lane}"),
            "action": "adopt",
            "payload": { "laneKey": lane },
        });
        match super::supervisor_request(state, caller.clone(), envelope).await {
            Ok(_) => adopted.push(Value::String(lane)),
            Err(failure) => refused.push(json!({ "lane": lane, "code": failure.code, "message": failure.message })),
        }
    }
    tracing::info!(adopted = adopted.len(), refused = refused.len(), "legacy lanes adopted");
    Ok(json!({ "adopted": adopted, "refused": refused }))
}

/// Serves one daemon `supervisorSetup` request from the desktop (or the owner's CLI).
pub async fn supervisor_setup(
    state: &std::sync::Arc<RemoteGatewayState>,
    request: crate::daemon::protocol::SupervisorSetupAction,
) -> Result<Value, SetupError> {
    use crate::daemon::protocol::SupervisorSetupAction;
    match request {
        SupervisorSetupAction::Status => supervisor_setup_status(state).await,
        SupervisorSetupAction::Run { import_memory, projects } => {
            supervisor_setup_run(state, SetupRunRequest { import_memory, projects }).await
        }
        SupervisorSetupAction::AdoptAll => supervisor_adopt_all(state, super::SupervisorCaller::Desktop).await,
    }
}

/// The snapshot's `setup` field: the SetupStatus, or null when it cannot be read.
pub(super) async fn snapshot_setup(pool: &ferryx_hub::goals::SqlitePool) -> Value {
    let engine = match engine_view(&HubEnv::Live).await {
        Ok(engine) => engine,
        Err(error) => {
            tracing::warn!(code = error.code(), "supervisor setup engine view was not read");
            return Value::Null;
        }
    };
    match setup::setup_status(pool, engine).await {
        Ok(status) => serde_json::to_value(status).unwrap_or(Value::Null),
        Err(error) => {
            tracing::warn!(code = error.code(), "supervisor setup status was not read");
            Value::Null
        }
    }
}

/// The shared SetupStatus of this installation. Reads only.
pub async fn supervisor_setup_status(state: &RemoteGatewayState) -> Result<Value, SetupError> {
    let (pool, _) = hub_pool(state)?;
    let status = setup::setup_status(&pool, engine_view(&HubEnv::Live).await?).await?;
    Ok(serde_json::to_value(status).unwrap_or(Value::Null))
}

/// Runs setup for the owner and returns the fresh SetupStatus (plus projectResults and
/// restarted). The Supervisor views are told to refetch.
pub async fn supervisor_setup_run(
    state: &std::sync::Arc<RemoteGatewayState>,
    request: SetupRunRequest,
) -> Result<Value, SetupError> {
    let (pool, live) = hub_pool(state)?;
    let slack = if live { slack_owner(&pool).await } else { SlackOwner::default() };
    let installed = if live { ferryx_hub::meta::mhc_supervisor::installed() } else { None };
    let health = installed.as_ref().map(|engine| engine.health());
    let answer = run_setup_on(&pool, &HubEnv::Live, request, &slack, &local_account(), live, health).await?;
    if answer["cutOver"] == true {
        crate::remote::server::agent_control_service::publish_supervisor_changed(state, json!({ "setup": "cutover" }));
    }
    if let (Some(engine), true) = (installed, answer["seedDue"] == true) {
        // The engine was already Ready, so its generation's task saw no new Ready: seed here,
        // off the request path (once; a finished import makes this a no-op).
        let state = std::sync::Arc::clone(state);
        let pool = pool.clone();
        super::HUB_TASKS.track(&tokio::spawn(async move {
            seed_if_wanted(&engine, &pool, &state, super::home_dir()).await;
        }));
    }
    crate::remote::server::agent_control_service::publish_supervisor_changed(state, json!({ "setup": "run" }));
    Ok(answer)
}

pub(super) const HUB_SETUP_USAGE: &str =
    "ferryx hub setup [--import-memory] [--project <path>]... [--json] | ferryx hub setup --status [--json]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct HubSetupArgs {
    status: bool,
    json: bool,
    request: SetupRunRequest,
}

fn parse_hub_setup(args: &[String]) -> Result<HubSetupArgs, String> {
    let mut parsed = HubSetupArgs { status: false, json: false, request: SetupRunRequest::default() };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--status" => parsed.status = true,
            "--json" => parsed.json = true,
            "--import-memory" => parsed.request.import_memory = true,
            "--project" => match rest.next().filter(|path| !path.starts_with("--") && !path.trim().is_empty()) {
                Some(path) => parsed.request.projects.push(SetupProject { name: String::new(), path: path.clone() }),
                None => return Err(format!("--project needs a path\nusage: {HUB_SETUP_USAGE}")),
            },
            other => return Err(format!("unknown argument {other}\nusage: {HUB_SETUP_USAGE}")),
        }
    }
    if parsed.status && (parsed.request.import_memory || !parsed.request.projects.is_empty()) {
        return Err(format!("--status takes no setup choices\nusage: {HUB_SETUP_USAGE}"));
    }
    Ok(parsed)
}

/// Through the running daemon when it serves setup; None when no daemon (or an older one)
/// answers, so the caller sets up the hub database directly.
async fn setup_via_daemon(action: crate::daemon::protocol::SupervisorSetupAction) -> Option<Result<Value, (String, String)>> {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
    let mutates = !matches!(action, crate::daemon::protocol::SupervisorSetupAction::Status);
    let request = DaemonRequest::SupervisorSetup { request: action };
    match crate::cli::supervisor_goal::exchange_once(super::SUPERVISOR_SETUP_CAPABILITY, request, mutates).await {
        Ok(DaemonResponse::SupervisorSetupOk { value }) => Some(Ok(value)),
        Ok(DaemonResponse::Error { message, code, .. }) => Some(Err((code.unwrap_or_else(|| "INTERNAL".into()), message))),
        Ok(other) => Some(Err(("INTERNAL".into(), format!("unexpected daemon response: {other:?}")))),
        Err((value, _)) => match value["code"].as_str() {
            Some("NOT_FOUND" | "UNSUPPORTED") => None,
            code => Some(Err((code.unwrap_or("INTERNAL").to_owned(), value["message"].as_str().unwrap_or_default().to_owned()))),
        },
    }
}

/// Setup over the hub database this user's hub opens, without a daemon: the env file and the
/// owner, projects and choices are written; the engine starts with the next hub generation.
async fn setup_locally(args: &HubSetupArgs) -> Result<Value, (String, String)> {
    let fail = |error: SetupError| (error.code().to_owned(), error.to_string());
    let config = ferryx_hub::HubConfig::load().map_err(|error| ("INVALID_REQUEST".to_owned(), format!("hub config: {error}")))?;
    let path = config.database_path.clone().unwrap_or_else(ferryx_hub::hub::default_database_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ("STORAGE".to_owned(), format!("cannot create {}: {error}", parent.display())))?;
    }
    let pool = ferryx_hub::storage::connect(&path).await.map_err(|error| ("STORAGE".to_owned(), error.to_string()))?;
    let answer = if args.status {
        match engine_view(&HubEnv::Live).await {
            Ok(engine) => setup::setup_status(&pool, engine)
                .await
                .map(|status| serde_json::to_value(status).unwrap_or(Value::Null))
                .map_err(|error| fail(error.into())),
            Err(error) => Err(fail(error)),
        }
    } else {
        let slack = match config.owner_slack_id.clone() {
            Some(user) => {
                let team = match setup::known_slack_team(&pool).await.ok().flatten() {
                    Some(team) => Some(team),
                    None => slack_auth_team().await,
                };
                SlackOwner { team, user: Some(user) }
            }
            None => SlackOwner::default(),
        };
        run_setup_on(&pool, &HubEnv::Live, args.request.clone(), &slack, &local_account(), false, None)
            .await
            .map_err(fail)
    };
    pool.close().await;
    answer
}

fn print_human(value: &Value) {
    let text = |key: &str| value[key].as_str().unwrap_or("-").to_owned();
    println!("next action      {}", text("nextAction"));
    println!("engine           enabled={} installed={} state={}", value["engineEnabled"], value["engineInstalled"], text("engineState"));
    if let Some(fault) = value["engineFault"].as_object() {
        println!("engine fault     {} {}", fault["code"].as_str().unwrap_or(""), fault["message"].as_str().unwrap_or(""));
    }
    println!("owner            {}", value["owner"]["ownerPrincipal"].as_str().unwrap_or("(none)"));
    println!("projects         {}", value["projects"]);
    println!("intake           unified={} legacy={}", value["intake"]["unifiedOpen"], value["intake"]["legacy"].as_str().unwrap_or("-"));
    println!("auto cutover     {}", value["autoCutover"]);
    println!("memory import    enabled={} sessions={} turns={}", value["memoryImport"]["enabled"], value["memoryImport"]["sessions"], value["memoryImport"]["turns"]);
    println!("adoptable lanes  {}", value["adoptable"]);
    for result in value["projectResults"].as_array().into_iter().flatten() {
        match result["ok"].as_bool() {
            Some(true) => println!("mapped           {} -> {}", result["path"].as_str().unwrap_or(""), result["repoIdentity"].as_str().unwrap_or("")),
            _ => println!("refused          {} {}: {}", result["path"].as_str().unwrap_or(""), result["code"].as_str().unwrap_or(""), result["message"].as_str().unwrap_or("")),
        }
    }
}

/// `ferryx hub setup ...`: automatic supervisor setup from the command line, with or without a
/// running daemon. Refused inside a Ferryx pane: setup is the owner's act.
pub(super) fn hub_setup_cli(args: &[String]) -> i32 {
    if std::env::var_os("FERRYX_SESSION_ID").is_some_and(|session| !session.is_empty()) {
        eprintln!("hub setup is the owner's act and is refused inside a Ferryx pane (FERRYX_SESSION_ID is set)");
        return crate::agent_control::EXIT_REFUSED;
    }
    let parsed = match parse_hub_setup(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("runtime: {error}");
            return 1;
        }
    };
    let action = if parsed.status {
        crate::daemon::protocol::SupervisorSetupAction::Status
    } else {
        crate::daemon::protocol::SupervisorSetupAction::Run {
            import_memory: parsed.request.import_memory,
            projects: parsed.request.projects.clone(),
        }
    };
    let answer = runtime.block_on(async {
        match setup_via_daemon(action).await {
            Some(answer) => answer,
            None => setup_locally(&parsed).await,
        }
    });
    match answer {
        Ok(value) if parsed.json => {
            println!("{value}");
            0
        }
        Ok(value) => {
            print_human(&value);
            0
        }
        Err((code, message)) => {
            if parsed.json {
                println!("{}", json!({ "ok": false, "code": code, "message": message }));
            } else {
                eprintln!("{code}: {message}");
            }
            crate::agent_control::exit_code_for(&code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn env_at(file: &Path) -> HubEnv {
        HubEnv::At { file: file.to_path_buf(), process: None }
    }

    #[test]
    fn the_setup_cli_parses_status_run_and_refuses_the_rest() {
        assert_eq!(parse_hub_setup(&words(&["--status", "--json"])), Ok(HubSetupArgs { status: true, json: true, request: SetupRunRequest::default() }));
        let run = parse_hub_setup(&words(&["--import-memory", "--project", "/a", "--project", "/b"])).unwrap();
        assert!(!run.status && !run.json && run.request.import_memory);
        assert_eq!(run.request.projects.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), vec!["/a", "/b"]);
        assert!(parse_hub_setup(&words(&["--project"])).is_err());
        assert!(parse_hub_setup(&words(&["--status", "--import-memory"])).is_err());
        assert!(parse_hub_setup(&words(&["--bogus"])).is_err());
    }

    fn git_repo(root: &Path) -> PathBuf {
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .status()
            .unwrap();
        assert!(status.success());
        repo
    }

    #[tokio::test]
    async fn setup_run_enables_the_engine_creates_the_owner_maps_projects_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let env_file = temp.path().join("hub").join("env");
        let repo = git_repo(temp.path());
        let not_git = temp.path().join("plain");
        std::fs::create_dir_all(&not_git).unwrap();
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let request = SetupRunRequest {
            import_memory: true,
            projects: vec![
                SetupProject { name: "qa".into(), path: repo.to_string_lossy().into_owned() },
                SetupProject { name: String::new(), path: not_git.to_string_lossy().into_owned() },
            ],
        };
        let slack = SlackOwner::default();

        let first = run_setup_on(&pool, &env_at(&env_file), request.clone(), &slack, "tester", false, None)
            .await
            .unwrap();
        assert_eq!(read_env_at(&env_file).get(ENABLE_KEY).map(String::as_str), Some("1"));
        assert_eq!(first["engineEnabled"], true);
        assert_eq!(first["autoCutover"], true);
        assert_eq!(first["memoryImport"]["enabled"], true);
        assert_eq!(first["owner"]["ownerPrincipal"], "local:tester");
        assert_eq!(first["projects"], 1);
        // No engine is installed in a unit test, so the generation still has to restart.
        assert_eq!(first["nextAction"], "restart_required");
        let results = first["projectResults"].as_array().unwrap();
        assert_eq!(results[0]["ok"], true, "{results:?}");
        assert_eq!(results[1]["ok"], false, "{results:?}");
        assert_eq!(results[1]["code"], "NOT_A_GIT_DIR");

        let env_before = std::fs::read_to_string(&env_file).unwrap();
        let second = run_setup_on(&pool, &env_at(&env_file), request, &slack, "someone-else", false, None)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&env_file).unwrap(), env_before);
        assert_eq!(second["owner"], first["owner"]);
        assert_eq!(second["projects"], 1);
        assert_eq!(second["projectResults"][0]["projectId"], first["projectResults"][0]["projectId"]);
    }

    #[tokio::test]
    async fn the_slack_owner_is_the_principal_when_team_and_user_are_known() {
        let temp = tempfile::tempdir().unwrap();
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let slack = SlackOwner { team: Some("T1".into()), user: Some("U9".into()) };
        let answer = run_setup_on(&pool, &env_at(&temp.path().join("env")), SetupRunRequest::default(), &slack, "tester", false, None)
            .await
            .unwrap();
        assert_eq!(answer["owner"]["ownerPrincipal"], "slack:T1:U9");
        assert_eq!(answer["memoryImport"]["enabled"], false);
        assert_eq!(answer["nextAction"], "restart_required");
    }

    #[tokio::test]
    async fn a_slack_owner_without_a_known_team_is_refused_and_nothing_is_provisioned() {
        let temp = tempfile::tempdir().unwrap();
        let env_file = temp.path().join("env");
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let slack = SlackOwner { team: None, user: Some("U9".into()) };
        let error = run_setup_on(&pool, &env_at(&env_file), SetupRunRequest::default(), &slack, "tester", false, None)
            .await
            .unwrap_err();
        assert_eq!(error.code(), SLACK_TEAM_UNRESOLVED);
        assert!(ferryx_hub::supervisor::identities(&pool).await.unwrap().is_empty(), "no local: fallback");
        assert!(!env_file.exists(), "a refused run writes nothing");
    }

    #[tokio::test]
    async fn a_process_disable_shows_the_engine_off_even_when_the_file_turns_it_on() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("env");
        std::fs::write(&file, format!("{ENABLE_KEY}=1\n")).unwrap();
        assert!(engine_view(&env_at(&file)).await.unwrap().enabled);
        let off = HubEnv::At { file: file.clone(), process: Some("0".into()) };
        assert!(!engine_view(&off).await.unwrap().enabled);
        // A process value never turns the engine on.
        let empty = temp.path().join("empty");
        let on = HubEnv::At { file: empty, process: Some("1".into()) };
        assert!(!engine_view(&on).await.unwrap().enabled);
        // Setup with the process override off answers enable_engine, never restart_required.
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let answer = run_setup_on(&pool, &off, SetupRunRequest::default(), &SlackOwner::default(), "tester", true, None)
            .await
            .unwrap();
        assert_eq!(answer["engineEnabled"], false);
        assert_eq!(answer["restarted"], false);
        assert_eq!(answer["nextAction"], "enable_engine");
    }

    #[tokio::test]
    async fn env_file_work_runs_off_the_async_runtime_thread() {
        let caller = std::thread::current().id();
        let worker = off_runtime(|| std::thread::current().id()).await.unwrap();
        assert_ne!(caller, worker);
    }

    #[tokio::test]
    async fn a_stopped_hub_stays_stopped_and_a_running_one_restarts() {
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let starts = &counter;
        let start = move || async move {
            starts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(true)
        };
        let stopped = tokio::sync::Mutex::new(None);
        assert!(!restart_if_running(&stopped, start).await.unwrap());
        assert_eq!(starts.load(std::sync::atomic::Ordering::SeqCst), 0, "a stopped hub is not started");
        assert!(stopped.lock().await.is_none());

        let task = tokio::spawn(std::future::pending::<()>());
        let running = tokio::sync::Mutex::new(Some(Generation { tasks: vec![task] }));
        assert!(restart_if_running(&running, start).await.unwrap());
        assert_eq!(starts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    fn ready_health() -> ferryx_hub::meta::mhc_supervisor::SupervisorHealth {
        ferryx_hub::meta::mhc_supervisor::SupervisorHealth {
            state: ferryx_hub::meta::mhc_supervisor::HealthState::Ready,
            generation: 1,
            pid: None,
            capabilities: ferryx_hub::meta::mhc_profile::REQUIRED_CAPABILITIES.iter().map(|c| (*c).to_owned()).collect(),
            server_version: None,
            fault: None,
            detail: None,
            queued_decisions: 0,
        }
    }

    #[tokio::test]
    async fn setup_on_an_already_ready_engine_cuts_over_without_a_restart() {
        let temp = tempfile::tempdir().unwrap();
        let env_file = temp.path().join("env");
        let pool = ferryx_hub::storage::memory().await.unwrap();
        // An upgraded installation: legacy history exists and the legacy intake is open.
        ferryx_hub::lanes::state::insert_lane(&pool, "nightly", "Nightly", "lead", "/repo", "file marker")
            .await
            .unwrap();
        ferryx_hub::supervisor::cutover::note_import(&pool).await.unwrap();
        let before = ferryx_hub::supervisor::cutover::installation_intake(&pool).await.unwrap();
        assert!(!before.unified_open);

        let answer = run_setup_on(
            &pool,
            &env_file,
            SetupRunRequest { import_memory: true, projects: Vec::new() },
            &SlackOwner::default(),
            "tester",
            false,
            Some(ready_health()),
        )
        .await
        .unwrap();
        assert_eq!(answer["restarted"], false);
        assert_eq!(answer["cutOver"], true, "{answer}");
        assert_eq!(answer["seedDue"], true);
        let after = ferryx_hub::supervisor::cutover::installation_intake(&pool).await.unwrap();
        assert!(after.unified_open);
        assert_eq!(after.legacy, ferryx_hub::supervisor::cutover::LegacyIntake::Stopped);

        // Idempotent: a second run moves nothing.
        let again = run_setup_on(&pool, &env_file, SetupRunRequest::default(), &SlackOwner::default(), "tester", false, Some(ready_health()))
            .await
            .unwrap();
        assert_eq!(again["cutOver"], false);
        assert_eq!(again["seedDue"], false);
    }

    fn health_in(state: ferryx_hub::meta::mhc_supervisor::HealthState) -> SupervisorHealth {
        SupervisorHealth { state, ..ready_health() }
    }

    /// Criterion 1, the upgrade flow: setup ran while no engine was installed (restart
    /// required); the restarted generation's engine starts and becomes Ready, and the
    /// generation's setup loop cuts the intake over on that transition, once.
    #[tokio::test]
    async fn the_generation_cuts_over_when_its_engine_turns_ready_after_a_restart() {
        use ferryx_hub::meta::mhc_supervisor::HealthState;
        let temp = tempfile::tempdir().unwrap();
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::lanes::state::insert_lane(&pool, "nightly", "Nightly", "lead", "/repo", "file marker")
            .await
            .unwrap();
        ferryx_hub::supervisor::cutover::note_import(&pool).await.unwrap();
        let answer = run_setup_on(&pool, &env_at(&temp.path().join("env")), SetupRunRequest::default(), &SlackOwner::default(), "tester", false, None)
            .await
            .unwrap();
        assert_eq!(answer["nextAction"], "restart_required");
        assert!(!ferryx_hub::supervisor::cutover::installation_intake(&pool).await.unwrap().unified_open);

        let (sender, receiver) = tokio::sync::watch::channel(health_in(HealthState::Stopped));
        let (ready_tx, mut ready_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let cut_overs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let loop_pool = pool.clone();
        let counted = std::sync::Arc::clone(&cut_overs);
        let task = tokio::spawn(async move {
            cut_over_on_ready(
                receiver,
                &loop_pool,
                || {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                },
                || {
                    let ready_tx = ready_tx.clone();
                    async move {
                        let _ = ready_tx.send(());
                    }
                },
            )
            .await;
        });
        sender.send(health_in(HealthState::Ready)).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), ready_rx.recv())
            .await
            .expect("the Ready transition is handled")
            .expect("loop alive");
        assert_eq!(cut_overs.load(std::sync::atomic::Ordering::SeqCst), 1);
        let after = ferryx_hub::supervisor::cutover::installation_intake(&pool).await.unwrap();
        assert!(after.unified_open);
        assert_eq!(after.legacy, ferryx_hub::supervisor::cutover::LegacyIntake::Stopped);

        // Ending the generation closes the channel and the loop ends.
        drop(sender);
        tokio::time::timeout(std::time::Duration::from_secs(10), task)
            .await
            .expect("loop ends with its channel")
            .unwrap();
        assert_eq!(cut_overs.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
