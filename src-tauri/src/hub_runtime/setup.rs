//! Automatic supervisor setup: the one owner action that turns the bundled mhc engine on,
//! records the owner's setup choices, restarts the hub generation in-process so the engine is
//! installed (never the daemon, so PTY sessions stay up), creates the owner identity and maps
//! the requested projects. Every step is idempotent; the answer is the shared SetupStatus.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ferryx_hub::meta::mhc_profile::{MhcProfileConfig, ENABLE_KEY};
use ferryx_hub::supervisor::setup::{self, EngineView};
use ferryx_hub::supervisor::SupervisorError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{start_generation, HubStartError, HUB_TASKS, SUPERVISOR_LAUNCH, SUPERVISOR_VIEW};
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
    #[error("{0}")]
    Supervisor(#[from] SupervisorError),
}

impl SetupError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "SUPERVISOR_UNAVAILABLE",
            Self::HandedOver => "INVALID_STATE",
            Self::EnvFile(_) => "STORAGE",
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

pub(crate) fn env_file() -> Result<PathBuf, SetupError> {
    ferryx_hub::hub_env::env_file_path()
        .ok_or_else(|| SetupError::EnvFile("neither HOME nor USERPROFILE is set".into()))
}

fn read_env_at(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .map(|text| ferryx_hub::hub_env::parse_env_file(&text))
        .unwrap_or_default()
}

/// The engine as setup sees it: the env file at env_file read now, and this process's engine.
pub(crate) fn engine_view(env_file: &Path) -> EngineView {
    let installed = ferryx_hub::meta::mhc_supervisor::installed();
    EngineView {
        enabled: MhcProfileConfig::enabled_in(&read_env_at(env_file)),
        installed: installed.is_some(),
        health: installed.map(|engine| engine.health()),
    }
}

/// Merges ENABLE_KEY=1 into the env file, creating its directory (0700) when missing.
fn enable_in_env_file(env_file: &Path) -> Result<(), SetupError> {
    if let Some(parent) = env_file.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        if !parent.exists() {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(parent)
                .map_err(|error| SetupError::EnvFile(format!("{}: {error}", parent.display())))?;
        }
    }
    let entries = BTreeMap::from([(ENABLE_KEY.to_owned(), "1".to_owned())]);
    ferryx_hub::hub_env::write_merged(env_file, &entries)
        .map(|_| ())
        .map_err(|error| SetupError::EnvFile(error.to_string()))
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
    if ferryx_hub::chat::slack::bot_token().is_none() {
        return None;
    }
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

/// Stops the running generation and starts a fresh one, as control stop then start do, but
/// without leaving the persisted stopped flag set. Never restarts the daemon or touches a PTY.
async fn restart_generation() -> Result<(), SetupError> {
    let (Some((_, running)), Some(launch)) = (SUPERVISOR_VIEW.get(), SUPERVISOR_LAUNCH.get()) else {
        return Err(SetupError::Unavailable);
    };
    if HUB_TASKS.is_retired() {
        return Err(SetupError::HandedOver);
    }
    let previous = running.lock().await.take();
    if let Some(generation) = previous {
        generation.end().await;
    }
    start_generation(running, launch).await?;
    Ok(())
}

/// The installation's owner identity: the existing one, else a new one for the Slack owner
/// (when both team and user are known) or the local account.
async fn ensure_owner(
    pool: &ferryx_hub::goals::SqlitePool,
    slack: &SlackOwner,
    account: &str,
) -> Result<ferryx_hub::supervisor::SupervisorIdentity, SetupError> {
    if let Some(existing) = ferryx_hub::supervisor::identities(pool).await?.into_iter().next() {
        return Ok(existing);
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

/// The setup run over an explicit pool and env file. restart is false for a hub that is not
/// this daemon's live one (tests, the desktop-independent CLI), whose generation cannot be
/// restarted from here. engine is the health of the engine installed in the running
/// generation, if any.
///
/// The owner and projects are written before the generation restarts, so the new
/// generation's first Ready already finds them. An engine that is already Ready sees no new
/// Ready transition, so the run itself attempts the automatic cutover; the answer's
/// `seedDue` tells the caller to seed owner memory with that engine.
pub(crate) async fn run_setup_on(
    pool: &ferryx_hub::goals::SqlitePool,
    env_file: &Path,
    request: SetupRunRequest,
    slack: &SlackOwner,
    account: &str,
    restart: bool,
    engine: Option<ferryx_hub::meta::mhc_supervisor::SupervisorHealth>,
) -> Result<Value, SetupError> {
    use ferryx_hub::meta::mhc_supervisor::HealthState;
    if !MhcProfileConfig::enabled_in(&read_env_at(env_file)) {
        enable_in_env_file(env_file)?;
    }
    setup::set_setup_flags(pool, Some(true), Some(request.import_memory)).await?;
    let identity = ensure_owner(pool, slack, account).await?;
    let projects = map_projects(pool, &identity.supervisor_id, request.projects).await;
    let mut restarted = false;
    let mut cut_over = false;
    let mut seed_due = false;
    match &engine {
        None if restart => {
            restart_generation().await?;
            restarted = true;
        }
        Some(health) if health.state == HealthState::Ready => {
            cut_over = ferryx_hub::supervisor::cutover::auto_cutover(pool, Some(health)).await?.is_some();
            seed_due = request.import_memory;
        }
        _ => {}
    }
    let status = setup::setup_status(pool, engine_view(env_file)).await?;
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
    use ferryx_hub::meta::mhc_supervisor::HealthState;
    let mut health = engine.subscribe_health();
    let mut was_ready = false;
    let mut seeded = false;
    loop {
        let current = health.borrow_and_update().clone();
        let ready = current.state == HealthState::Ready;
        if ready && !was_ready {
            match ferryx_hub::supervisor::cutover::auto_cutover(&pool, Some(&current)).await {
                Ok(Some(transition)) => {
                    tracing::info!(steps = transition.steps.len(), "supervisor intake cut over automatically");
                    crate::remote::server::agent_control_service::publish_supervisor_changed(
                        &state,
                        json!({ "setup": "cutover" }),
                    );
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(code = error.code(), "automatic cutover failed"),
            }
            if !seeded {
                // Seeded only once an import ran or had already finished; a skip (no owner
                // or memory import off at that moment) leaves it to the next Ready or setup run.
                seeded = seed_if_wanted(&engine, &pool, &state, home.clone()).await;
            }
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
    let Ok(env_file) = env_file() else {
        return Value::Null;
    };
    match setup::setup_status(pool, engine_view(&env_file)).await {
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
    let status = setup::setup_status(&pool, engine_view(&env_file()?)).await?;
    Ok(serde_json::to_value(status).unwrap_or(Value::Null))
}

/// Runs setup for the owner and returns the fresh SetupStatus (plus projectResults and
/// restarted). The Supervisor views are told to refetch.
pub async fn supervisor_setup_run(
    state: &std::sync::Arc<RemoteGatewayState>,
    request: SetupRunRequest,
) -> Result<Value, SetupError> {
    let (pool, live) = hub_pool(state)?;
    let env_file = env_file()?;
    let slack = if live { slack_owner(&pool).await } else { SlackOwner::default() };
    let installed = if live { ferryx_hub::meta::mhc_supervisor::installed() } else { None };
    let health = installed.as_ref().map(|engine| engine.health());
    let answer = run_setup_on(&pool, &env_file, request, &slack, &local_account(), live, health).await?;
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
    let env_file = env_file().map_err(fail)?;
    let answer = if args.status {
        setup::setup_status(&pool, engine_view(&env_file))
            .await
            .map(|status| serde_json::to_value(status).unwrap_or(Value::Null))
            .map_err(|error| fail(error.into()))
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
        run_setup_on(&pool, &env_file, args.request.clone(), &slack, &local_account(), false, None)
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

        let first = run_setup_on(&pool, &env_file, request.clone(), &slack, "tester", false, None)
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
        let second = run_setup_on(&pool, &env_file, request, &slack, "someone-else", false, None)
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
        let answer = run_setup_on(&pool, &temp.path().join("env"), SetupRunRequest::default(), &slack, "tester", false, None)
            .await
            .unwrap();
        assert_eq!(answer["owner"]["ownerPrincipal"], "slack:T1:U9");
        assert_eq!(answer["memoryImport"]["enabled"], false);
        assert_eq!(answer["nextAction"], "restart_required");
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
}
