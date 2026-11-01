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
/// restarted from here.
pub(crate) async fn run_setup_on(
    pool: &ferryx_hub::goals::SqlitePool,
    env_file: &Path,
    request: SetupRunRequest,
    slack: &SlackOwner,
    account: &str,
    restart: bool,
) -> Result<Value, SetupError> {
    if !MhcProfileConfig::enabled_in(&read_env_at(env_file)) {
        enable_in_env_file(env_file)?;
    }
    setup::set_setup_flags(pool, Some(true), Some(request.import_memory)).await?;
    let mut restarted = false;
    if restart && ferryx_hub::meta::mhc_supervisor::installed().is_none() {
        restart_generation().await?;
        restarted = true;
    }
    let identity = ensure_owner(pool, slack, account).await?;
    let projects = map_projects(pool, &identity.supervisor_id, request.projects).await;
    let status = setup::setup_status(pool, engine_view(env_file)).await?;
    let mut answer = serde_json::to_value(status).unwrap_or(Value::Null);
    if let Some(fields) = answer.as_object_mut() {
        fields.insert("projectResults".into(), Value::Array(projects));
        fields.insert("restarted".into(), Value::Bool(restarted));
    }
    tracing::info!(restarted, "supervisor setup ran");
    Ok(answer)
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
    state: &RemoteGatewayState,
    request: SetupRunRequest,
) -> Result<Value, SetupError> {
    let (pool, live) = hub_pool(state)?;
    let env_file = env_file()?;
    let slack = if live { slack_owner(&pool).await } else { SlackOwner::default() };
    let answer = run_setup_on(&pool, &env_file, request, &slack, &local_account(), live).await?;
    crate::remote::server::agent_control_service::publish_supervisor_changed(state, json!({ "setup": "run" }));
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let first = run_setup_on(&pool, &env_file, request.clone(), &slack, "tester", false)
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
        let second = run_setup_on(&pool, &env_file, request, &slack, "someone-else", false)
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
        let answer = run_setup_on(&pool, &temp.path().join("env"), SetupRunRequest::default(), &slack, "tester", false)
            .await
            .unwrap();
        assert_eq!(answer["owner"]["ownerPrincipal"], "slack:T1:U9");
        assert_eq!(answer["memoryImport"]["enabled"], false);
        assert_eq!(answer["nextAction"], "restart_required");
    }
}
