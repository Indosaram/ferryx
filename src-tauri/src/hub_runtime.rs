//! Starts the always-on hub inside `ferryx-cli --daemon` and serves `ferryx-cli mcp`.
//!
//! The hub is not constructed by `DaemonServer::new`, so unit tests that build a daemon
//! do not open its database or a chat socket. Supervisor writes go through agent control,
//! which uses the shared reference-chat input queue and refuses a pane a person just typed in.
use std::sync::Arc;

use async_trait::async_trait;
use ferryx_hub::agent_control::{AgentControl, AgentOp, TaskLauncher};
use ferryx_hub::security::{auto_answer, AutoDecision, Reading};
use ferryx_hub::sessions::watch::Observation;
use ferryx_hub::{Hub, HubConfig, HubError};
use serde_json::{json, Value};

use crate::agent_control::{AgentControlOp, AgentControlState, AgentStateSource};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use crate::daemon::server::DaemonServer;
use crate::remote::state::RemoteGatewayState;
use crate::remote::reference_chat::types::{ReferencePart, ReferencePromptAnswer, ReferenceTurn};

/// Maps a hub operation onto the daemon's agent-control request.
/// The request id is the one the caller supplied. A lost answer is not given a new id.
pub fn to_agent_control_op(op: &AgentOp) -> Result<AgentControlOp, String> {
    Ok(match op {
        AgentOp::List => AgentControlOp::List,
        AgentOp::Read { session_id } => AgentControlOp::Read { session_id: session_id.clone() },
        AgentOp::Send { session_id, text, request_id } => AgentControlOp::Send {
            session_id: session_id.clone(),
            text: text.clone(),
            request_id: Some(request_id.clone()),
        },
        AgentOp::Answer { session_id, prompt_id, screen_revision, answer, request_id } => {
            let answer: ReferencePromptAnswer = serde_json::from_value(answer.clone())
                .map_err(|error| format!("answer: {error}"))?;
            if !answer.is_single_choice() {
                return Err("an answer must be exactly one of optionIndex, optionIndices, or customText".into());
            }
            AgentControlOp::Answer {
                session_id: session_id.clone(),
                prompt_id: prompt_id.clone(),
                screen_revision: screen_revision.clone(),
                answer,
                request_id: Some(request_id.clone()),
            }
        }
        AgentOp::Stop { session_id, request_id } => AgentControlOp::Stop {
            session_id: session_id.clone(),
            request_id: Some(request_id.clone()),
        },
        AgentOp::Wait { session_id, until, after_revision, after, timeout_ms } => AgentControlOp::Wait {
            session_id: session_id.clone(),
            until: until.clone(),
            after_revision: after_revision.clone(),
            after: *after,
            timeout_ms: *timeout_ms,
        },
        AgentOp::Events { after } => AgentControlOp::Events { after: *after },
    })
}

/// Text lines from a ferryx transcript parser. Callers that only have a screen use the
/// screen-revision diff in `ferryx_hub::sessions::external` instead.
pub fn history_lines(agent: &str, transcript: &str) -> Vec<String> {
    let turns = match agent {
        "claude" => crate::remote::reference_chat::history_claude::parse_claude_transcript(transcript).unwrap_or_default(),
        "codex" => crate::remote::reference_chat::history_parse::parse_codex_transcript(transcript),
        "omo" => {
            let mut titles = std::collections::HashMap::new();
            crate::remote::reference_chat::history_omo::parse_omo_transcript(transcript, 40, false, &mut titles)
        }
        _ => Vec::new(),
    };
    lines_of(&turns)
}

fn lines_of(turns: &[ReferenceTurn]) -> Vec<String> {
    turns
        .iter()
        .flat_map(|turn| {
            turn.parts.iter().filter_map(|part| match part {
                ReferencePart::Text { text, .. } | ReferencePart::Thinking { text } => Some(text.clone()),
                _ => None,
            })
        })
        .filter(|text| !text.trim().is_empty())
        .collect()
}

struct DaemonBridge {
    client: DaemonClient,
}

#[async_trait]
impl AgentControl for DaemonBridge {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value, HubError> {
        let request = to_agent_control_op(&op).map_err(HubError::message)?;
        let host_id = host_id.filter(|host| *host != "local" && !host.is_empty()).map(str::to_owned);
        let response = self
            .client
            .send_request(DaemonRequest::AgentControl { request, host_id })
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        match response {
            DaemonResponse::AgentControlOk { result } => Ok(result),
            DaemonResponse::Error { message, code, .. } => {
                Err(HubError::message(format!("{} {message}", code.unwrap_or_default())))
            }
            _ => Err(HubError::message("the daemon returned an unexpected agent-control response")),
        }
    }
}

#[async_trait]
impl TaskLauncher for DaemonBridge {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String, HubError> {
        let workspace = ferryx_hub::hub_env::var("FERRYX_HUB_WORKSPACE").map_err(|_| {
            HubError::config("FERRYX_HUB_WORKSPACE is required before the hub can launch a pane")
        })?;
        let shell = if command.is_empty() { None } else { Some(command.join(" ")) };
        self.client
            .spawn_terminal(format!("hub-{}", std::process::id()), workspace, None, Some(cwd.to_owned()), 100, 32, shell)
            .await
            .map_err(|error| HubError::message(error.to_string()))
    }
}

/// `ferryx hub migrate [--from-omo <db>] [--dry-run]` copies live gateway state.
/// `ferryx hub stop` and `ferryx hub start` talk to the hub control socket.
/// They do not restart the daemon, so live PTY sessions stay up.
/// The report is row counts only.
pub fn run_hub_cli(args: &[String]) -> i32 {
    let mut source = None;
    let mut cwd = None;
    let mut record_as = None;
    let mut dry = false;
    let mut rest = args.iter().skip(2);
    match rest.next().map(String::as_str) {
        Some("stop") => return hub_control(ferryx_hub::control::ControlOp::Stop, false),
        Some("start") => return hub_control(ferryx_hub::control::ControlOp::Start, false),
        Some("status") => {
            return match rest.next().map(String::as_str) {
                None => hub_control(ferryx_hub::control::ControlOp::Status, false),
                Some("--check") => hub_control(ferryx_hub::control::ControlOp::Status, true),
                Some(other) => {
                    eprintln!("unknown argument {other}");
                    2
                }
            };
        }
        Some("env") => return hub_env_cli(&args[3.min(args.len())..]),
        Some("migrate") => {}
        _ => {
            eprintln!("usage: ferryx hub migrate [--from-omo <db>] [--cwd <dir>] [--imported-from <db>] [--dry-run]");
            eprintln!("       ferryx hub stop | start | status [--check]");
            eprintln!(
                "       ferryx hub env import [--from-plist <omo plist>] | ferryx hub env keys"
            );
            return 2;
        }
    }
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => dry = true,
            "--from-omo" => source = rest.next().map(std::path::PathBuf::from),
            "--cwd" => cwd = rest.next().map(std::path::PathBuf::from),
            "--imported-from" => record_as = rest.next().map(std::path::PathBuf::from),
            other if !other.starts_with('-') && source.is_none() => {
                source = Some(std::path::PathBuf::from(other));
            }
            other => {
                eprintln!("unknown argument {other}");
                return 2;
            }
        }
    }
    let source = match source {
        Some(path) => path,
        None => {
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            match ferryx_hub::import::discover_omo_database(&home) {
                Some(path) => path,
                None => {
                    eprintln!("omo database not found. Pass --from-omo PATH.");
                    eprintln!("Looked at the gateway plist WorkingDirectory, ~/code/project/omon-gateway/omon_gateway.db, then ~/.omon/gateway.db.");
                    return 1;
                }
            }
        }
    };
    println!("source {}", source.display());
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("runtime: {error}");
            return 1;
        }
    };
    runtime.block_on(async move {
        let config = match HubConfig::load() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("config: {error}");
                return 1;
            }
        };
        let path = config.database_path.clone().unwrap_or_else(ferryx_hub::hub::default_database_path);
        match ferryx_hub::import::open_and_migrate(&path, &source, dry, cwd.as_deref(), record_as.as_deref()).await {
            Ok(report) => {
                println!("dry_run {dry}");
                println!("issues {}", report.issues);
                println!("teams {}", report.teams);
                println!("states {}", report.states);
                println!("labels {}", report.labels);
                println!("activity {}", report.activity);
                println!("cron_jobs {}", report.cron_jobs);
                println!("inbox_items {}", report.inbox_items);
                println!("proposals {}", report.proposals);
                println!("allowlist {}", report.allowlist);
                println!("bot_profiles {}", report.bot_profiles);
                println!("lanes {}", report.lanes);
                println!("lane_threads {}", report.lane_threads);
                println!("cron_jobs_enabled {}", report.cron_jobs_enabled);
                if let Some(channel) = report.slack_home_channel.as_deref() {
                    println!("slack_home_channel {channel}");
                }
                for (name, count) in &report.tables {
                    println!("table {name} {count}");
                }
                let plist = omo_plist_path();
                if !dry && plist.exists() {
                    match import_omo_env(&plist) {
                        Ok(keys) => println!("env_keys_imported {}", keys.len()),
                        Err(error) => eprintln!("omo env was not imported: {error}"),
                    }
                }
                0
            }
            Err(error) => {
                eprintln!("migrate: {error}");
                1
            }
        }
    })
}

fn omo_plist_path() -> std::path::PathBuf {
    home_dir().join("Library/LaunchAgents/ai.omon.gateway.plist")
}

/// Imports omo's launchd `EnvironmentVariables` into the hub env file so the hub does not
/// depend on the daemon's launchd environment. Prints key names only, never values.
///
/// `DISCORD_*` keys are skipped (their names go to stderr): Discord stays off unless
/// `FERRYX_HUB_DISCORD=1` is set by hand.
fn import_omo_env(plist: &std::path::Path) -> Result<Vec<String>, String> {
    let dest = ferryx_hub::hub_env::env_file_path().ok_or("HOME is not set")?;
    let mut incoming =
        ferryx_hub::hub_env::plist_environment(plist).map_err(|error| error.to_string())?;
    for key in ferryx_hub::hub_env::drop_discord_keys(&mut incoming) {
        eprintln!("skipped {key} (Discord is enabled only by FERRYX_HUB_DISCORD=1)");
    }
    ferryx_hub::hub_env::write_merged(&dest, &incoming).map_err(|error| error.to_string())?;
    Ok(incoming.keys().cloned().collect())
}

fn hub_env_cli(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("import") => {
            let mut plist = omo_plist_path();
            let mut rest = args.iter().skip(1);
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--from-plist" => match rest.next() {
                        Some(path) => plist = std::path::PathBuf::from(path),
                        None => {
                            eprintln!("--from-plist needs a path");
                            return 2;
                        }
                    },
                    other => {
                        eprintln!("unknown argument {other}");
                        return 2;
                    }
                }
            }
            match import_omo_env(&plist) {
                Ok(keys) => {
                    if let Some(dest) = ferryx_hub::hub_env::env_file_path() {
                        println!("env_file {}", dest.display());
                    }
                    println!("imported_keys {}", keys.len());
                    for key in keys {
                        println!("key {key}");
                    }
                    println!("the running hub reads the file at its next start (handover or hub restart)");
                    0
                }
                Err(error) => {
                    eprintln!("hub env import: {error}");
                    1
                }
            }
        }
        Some("keys") => {
            let Some(path) = ferryx_hub::hub_env::env_file_path() else {
                eprintln!("HOME is not set");
                return 1;
            };
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    println!("env_file {}", path.display());
                    for key in ferryx_hub::hub_env::parse_env_file(&text).keys() {
                        println!("key {key}");
                    }
                    0
                }
                Err(error) => {
                    eprintln!("{}: {error}", path.display());
                    1
                }
            }
        }
        _ => {
            eprintln!(
                "usage: ferryx hub env import [--from-plist <omo plist>] | ferryx hub env keys"
            );
            2
        }
    }
}

/// Exit code for a control reply. A `"status":"failed"` reply is a failure, so the cutover
/// script never boots omo out after a start that was not persisted, and rollback never loads
/// omo after a stop that did not take. `--check` also needs `"healthy":true`.
fn control_exit_code(reply: &str, check: bool) -> i32 {
    let ok = if check {
        ferryx_hub::control::reply_healthy(reply)
    } else {
        ferryx_hub::control::reply_ok(reply)
    };
    if ok {
        0
    } else {
        1
    }
}

fn hub_control(op: ferryx_hub::control::ControlOp, check: bool) -> i32 {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = ferryx_hub::control::control_socket(&home);
    #[cfg(unix)]
    {
        let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("runtime: {error}");
                return 1;
            }
        };
        return runtime.block_on(async move {
            match ferryx_hub::control::send_control(&path, op).await {
                Ok(reply) => {
                    println!("{}", reply.trim());
                    control_exit_code(&reply, check)
                }
                Err(error) => {
                    eprintln!("hub control: {error}");
                    eprintln!("the daemon was not restarted, so live PTY sessions were left running");
                    1
                }
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (op, path, check);
        eprintln!("hub control uses a unix socket");
        1
    }
}

/// Every hub task in this daemon: the generation (Slack, cron, HTTP, watchers) and the control
/// listener. A handover commit retires it from a synchronous callback.
static HUB_TASKS: ferryx_hub::lifecycle::TaskRegistry = ferryx_hub::lifecycle::TaskRegistry::new();

/// The opened hub and its running generation, for the desktop's Supervisor view.
type SupervisorView = (Arc<Hub>, Arc<tokio::sync::Mutex<Option<Generation>>>);

/// The opened hub and its running generation, for the desktop's Supervisor view.
static SUPERVISOR_VIEW: std::sync::OnceLock<SupervisorView> = std::sync::OnceLock::new();

/// Supervisor input lines the snapshot carries, newest first.
const SUPERVISOR_ACTIONS_MAX: usize = 50;

/// What the desktop shows about supervision: whether the hub runs, the lanes it drives, the
/// panes it watches, and the recent input it typed (or was refused). Only owner-socket input
/// counts: the hub and agents on this machine's control socket. A phone or browser device
/// typing over the gateway is a person, not the supervisor.
pub async fn supervisor_snapshot(state: &RemoteGatewayState) -> Value {
    let actions: Vec<_> = state
        .agent_control_audit
        .recent()
        .into_iter()
        .rev()
        .filter(|record| {
            record.is_input()
                && record.device_id == "owner"
                && !relayed_from_a_person(record.request_id.as_deref())
        })
        .take(SUPERVISOR_ACTIONS_MAX)
        .collect();
    let Some((hub, running)) = SUPERVISOR_VIEW.get() else {
        return json!({ "hub": { "available": false, "running": false, "error": null }, "lanes": [], "watched": [], "actions": actions });
    };
    // A daemon that handed the hub to its successor no longer drives anything.
    let running = running.lock().await.is_some() && !HUB_TASKS.is_retired();
    let lanes = ferryx_hub::lanes::supervised::supervised_lanes(&hub.pool).await;
    let watched = ferryx_hub::lanes::supervised::watched_local_sessions(&hub.pool).await;
    let error = lanes.as_ref().err().or(watched.as_ref().err()).map(ToString::to_string);
    json!({
        "hub": { "available": true, "running": running, "error": error },
        "lanes": lanes.unwrap_or_default(),
        "watched": watched.unwrap_or_default(),
        "actions": actions,
    })
}

/// The hub also relays a person's own words into panes over the owner socket: a Slack approval
/// click, a chat message, a reply in a lane thread. Those are the person typing, not the supervisor.
fn relayed_from_a_person(request_id: Option<&str>) -> bool {
    ferryx_hub::goals::turns::relayed_from_a_person(request_id)
}

/// Coalesces raw typing into bursts: at most one admitted note per pane per
/// `HUMAN_TYPING_WINDOW_MS`.
#[derive(Default)]
struct KeystrokeBursts {
    recorded_at: parking_lot::Mutex<std::collections::HashMap<String, u64>>,
}

impl KeystrokeBursts {
    fn admit(&self, session_id: &str, now_ms: u64) -> bool {
        let window = crate::agent_control::policy::HUMAN_TYPING_WINDOW_MS;
        let mut recorded = self.recorded_at.lock();
        // Only stamps inside the window can still coalesce, so the map stays as small as the
        // set of panes typed into just now.
        recorded.retain(|_, at| now_ms.saturating_sub(*at) < window);
        if recorded.contains_key(session_id) {
            return false;
        }
        recorded.insert(session_id.to_owned(), now_ms);
        true
    }
}

static KEYSTROKE_BURSTS: std::sync::LazyLock<KeystrokeBursts> = std::sync::LazyLock::new(KeystrokeBursts::default);

/// One human_correction turn, with no text, for the first note of a typing burst on a lane's
/// pane. Returns whether a turn was recorded.
async fn record_human_keystrokes(
    pool: &ferryx_hub::goals::SqlitePool,
    bursts: &KeystrokeBursts,
    session_id: &str,
    now_ms: u64,
) -> Result<bool, HubError> {
    if !bursts.admit(session_id, now_ms) {
        return Ok(false);
    }
    Ok(ferryx_hub::goals::turns::record_keystroke_turn(pool, session_id).await?.is_some())
}

/// A person typed raw keys into `session_id`. When the pane belongs to a lane, the hub records
/// one human_correction turn per typing burst. The keys themselves are never stored.
pub(crate) fn note_lane_keystrokes(session_id: &str, now_ms: u64) {
    let Some((hub, _)) = SUPERVISOR_VIEW.get() else { return };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let pool = hub.pool.clone();
    let session_id = session_id.to_owned();
    runtime.spawn(async move {
        if let Err(error) = record_human_keystrokes(&pool, &KEYSTROKE_BURSTS, &session_id, now_ms).await {
            tracing::warn!(%error, session_id = %session_id, "a human typing turn was not recorded");
        }
    });
}

/// The turn an accepted agent-control send drove on a lane's pane: a person's relayed words
/// (chat, lane-thread reply, Slack approval) as human_correction, a `hub-goal-*` supervisor
/// send as supervisor_proposal. Other request ids record nothing.
async fn record_control_turn(pool: &ferryx_hub::goals::SqlitePool, op: &AgentControlOp) -> Result<bool, HubError> {
    let (session_id, request_id) = match op {
        AgentControlOp::Send { session_id, request_id, .. } | AgentControlOp::Answer { session_id, request_id, .. } => {
            (session_id, request_id.as_deref())
        }
        _ => return Ok(false),
    };
    let text = crate::agent_control::policy::audit_text(op).unwrap_or_default();
    Ok(ferryx_hub::goals::turns::record_send_turn(pool, session_id, request_id, &text).await?.is_some())
}

/// Called after an agent-control mutation succeeded. Records the lane turn it drove, if any.
pub(crate) fn note_agent_control_input(op: &AgentControlOp) {
    if ferryx_hub::goals::turns::origin_for_request_id(op.mutation_request_id().as_deref()).is_none() {
        return;
    }
    let Some((hub, _)) = SUPERVISOR_VIEW.get() else { return };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let pool = hub.pool.clone();
    let op = op.clone();
    runtime.spawn(async move {
        if let Err(error) = record_control_turn(&pool, &op).await {
            tracing::warn!(%error, "a lane turn was not recorded");
        }
    });
}

/// True once this daemon opened the hub, so the Supervisor view has lanes to show.
pub fn supervisor_available() -> bool {
    SUPERVISOR_VIEW.get().is_some()
}

/// Why a lane was not paused or resumed.
#[derive(Debug, thiserror::Error)]
pub enum LanePauseError {
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("lane not found: {0}")]
    NotFound(String),
    /// The lane is closed, failed or verified done; its pause no longer means anything.
    #[error("cannot pause or resume a {0} lane")]
    Ended(&'static str),
    #[error("{0}")]
    Storage(String),
}

/// A person pauses or resumes one lane. `by` names who: `desktop`, or `remote:<deviceId>` for
/// a paired phone. Returns whether the call changed the lane (false when it already was so).
pub async fn set_lane_paused(
    state: &RemoteGatewayState,
    lane_key: &str,
    paused: bool,
    by: &str,
) -> Result<bool, LanePauseError> {
    use ferryx_hub::lanes::state::LaneStatus;
    let Some((hub, _)) = SUPERVISOR_VIEW.get() else {
        return Err(LanePauseError::Unavailable);
    };
    let status = ferryx_hub::lanes::state::status_of(&hub.pool, lane_key)
        .await
        .map_err(|error| LanePauseError::Storage(error.to_string()))?
        .ok_or_else(|| LanePauseError::NotFound(lane_key.to_owned()))?;
    match status {
        LaneStatus::Closed | LaneStatus::Failed | LaneStatus::VerifiedDone => {
            return Err(LanePauseError::Ended(status.as_str()));
        }
        LaneStatus::Queued
        | LaneStatus::Working
        | LaneStatus::Blocked
        | LaneStatus::DoneClaimed
        | LaneStatus::NotDone => {}
    }
    let changed = if paused {
        ferryx_hub::lanes::pause::pause(&hub.pool, lane_key, by).await
    } else {
        ferryx_hub::lanes::pause::resume(&hub.pool, lane_key).await
    }
    .map_err(|error| LanePauseError::Storage(error.to_string()))?;
    crate::remote::server::agent_control_service::publish_supervisor_changed(
        state,
        json!({ "laneKey": lane_key }),
    );
    Ok(changed)
}

/// Handover commit: this daemon keeps serving retained sessions but no longer owns the hub.
/// Slack, the cron scheduler and sockets, `:9119`, the watchers and the control listener end
/// here, before the successor's hub starts, so there is never a second Socket Mode connection
/// or a second scheduler. `hub_stopped` is left as it is: the successor reads it to decide
/// whether to open the hub.
pub fn retire_for_handover() {
    if let Some(aborted) = HUB_TASKS.retire() {
        tracing::info!(aborted, "hub stopped at handover commit; the successor owns Slack, cron, :9119 and control.sock");
    }
}

/// Opens the hub after the daemon has announced ready. Failures are returned to the caller,
/// which logs them and leaves the daemon up.
pub async fn start(server: Arc<DaemonServer>) -> Result<(), String> {
    if std::env::var("FERRYX_HUB").ok().as_deref() == Some("off") {
        return Ok(());
    }
    // launchd gives the daemon a minimal PATH; resolve the login-shell PATH once so inbox
    // watchers, cron scripts, and every other hub child find imsg, zele, bun, and friends.
    if let Err(error) = tokio::task::spawn_blocking(ferryx_hub::child_env::init).await {
        tracing::warn!(%error, "hub child PATH was not resolved at start");
    }
    let config = HubConfig::load().map_err(|error| error.to_string())?;
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    let profiles = ferryx_hub::chat::router::ProfileRouter::new(config.profile_routes.clone());
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    let meta_session_id = config.meta_session_id.clone().unwrap_or_else(|| "meta".into());
    let bridge = Arc::new(DaemonBridge { client: DaemonClient::new() });
    let hub = Hub::open(config.clone(), bridge.clone()).await.map_err(|error| error.to_string())?;
    if let Err(error) = ferryx_hub::chat::reply::flush_pending(&hub.pool).await {
        tracing::warn!(%error, "stored chat replies were not flushed");
    }
    if config.supervisor_pane {
        let preset = ferryx_hub::meta::pane::SupervisorPanePreset::from_parts(
            config.supervisor_cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            config.supervisor_command.clone(),
        );
        tracing::info!(title = %preset.title, "supervisor pane preset is armed");
        if !preset.command.is_empty() {
            if let Err(error) = bridge.launch_pane(&preset.cwd.to_string_lossy(), &preset.command).await {
                tracing::warn!(%error, "supervisor pane was not launched");
            }
        }
    }
    if let Err(error) = ferryx_hub::lanes::kibitzer::ensure_schema(&hub.pool).await {
        tracing::warn!(%error, "kibitzer schema was not created");
    }
    // Fail closed: a fresh or unreadable hub database does not open Slack next to omo.
    let forced_on = std::env::var("FERRYX_HUB").ok().as_deref() == Some("on");
    let persisted_stop = ferryx_hub::control::is_stopped(&hub.pool).await.unwrap_or(true);
    let stopped = !forced_on && persisted_stop;
    // This daemon owns the hub from here; a later handover commit ends it again.
    server.handover_manager.on_commit(retire_for_handover);
    // A predecessor from before 2026.1022.7 keeps its hub through the drain. Stop it over
    // control.sock; its stop persists hub_stopped=1, so the value read above is put back.
    #[cfg(unix)]
    if ferryx_hub::lifecycle::preempt_foreign_hub(&ferryx_hub::control::control_socket(&home_dir()))
        .await
        .is_some()
        && !persisted_stop
    {
        if let Err(error) = ferryx_hub::control::clear_stopped(&hub.pool).await {
            tracing::warn!(%error, "hub_stopped was not restored after stopping the previous hub");
        }
    }
    let launch = HubLaunch {
        server,
        config,
        hub,
        bridge,
        #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
        profiles,
        #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
        meta_session_id,
    };
    let generation = if stopped {
        None
    } else {
        Some(spawn_generation(&launch).await)
    };
    let running = Arc::new(tokio::sync::Mutex::new(generation));
    let _ = SUPERVISOR_VIEW.set((Arc::clone(&launch.hub), Arc::clone(&running)));
    serve_control(running, launch);
    if stopped {
        tracing::info!("ferryx hub stayed stopped until hub start (or FERRYX_HUB=on)");
    } else {
        tracing::info!("ferryx hub is up");
    }
    Ok(())
}

#[derive(Clone)]
struct HubLaunch {
    server: Arc<DaemonServer>,
    config: HubConfig,
    hub: Arc<Hub>,
    bridge: Arc<DaemonBridge>,
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    profiles: ferryx_hub::chat::router::ProfileRouter,
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    meta_session_id: String,
}

/// Whether the hub HTTP listener is bound, for `hub status`. The guard clears it when the
/// listener task ends or is aborted by `hub stop`.
static HTTP_BOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct HttpBound;

impl HttpBound {
    fn mark() -> Self {
        HTTP_BOUND.store(true, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for HttpBound {
    fn drop(&mut self) {
        HTTP_BOUND.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn live_hub(running: bool, config: &HubConfig) -> ferryx_hub::control::LiveHub {
    ferryx_hub::control::LiveHub {
        running,
        http_addr: config.tracker_addr.clone().unwrap_or_else(|| "127.0.0.1:9119".into()),
        http_bound: running && HTTP_BOUND.load(std::sync::atomic::Ordering::SeqCst),
        #[cfg(feature = "hub-slack")]
        slack_configured: ferryx_hub::chat::slack::app_token().is_some(),
        #[cfg(feature = "hub-slack")]
        slack: Some(ferryx_hub::chat::slack::socket_mode_status()),
        #[cfg(not(feature = "hub-slack"))]
        slack_configured: false,
        #[cfg(not(feature = "hub-slack"))]
        slack: None,
        #[cfg(feature = "hub-discord")]
        discord_configured: ferryx_hub::chat::discord::active_bot_token().is_some(),
        #[cfg(not(feature = "hub-discord"))]
        discord_configured: false,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

struct Generation {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

async fn spawn_generation(launch: &HubLaunch) -> Generation {
    let mut tasks = Vec::new();
    #[cfg(feature = "hub-discord")]
    {
        let control: std::sync::Arc<dyn ferryx_hub::agent_control::AgentControl> =
            launch.bridge.clone();
        ferryx_hub::voice_session::install(std::sync::Arc::new(
            ferryx_hub::voice_session::VoiceTurnContext {
                pool: launch.hub.pool.clone(),
                profiles: launch.profiles.clone(),
                meta_session_id: launch.meta_session_id.clone(),
                control,
            },
        ));
    }
    let mut events = launch.server.remote_state().agent_events.subscribe();
    let watch_hub = Arc::clone(&launch.hub);
    let watch_bridge = launch.bridge.clone();
    let model_url = launch.config.supervisor_listen_url.clone();
    let lead = launch.config.meta_session_id.clone().unwrap_or_else(|| "meta".into());
    tasks.push(tokio::spawn(async move {
        let mut answered = std::collections::HashSet::new();
        let mut runs = std::collections::HashMap::new();
        loop {
            match events.recv().await {
                Ok(observation) => {
                    let idle = matches!(observation.state, AgentControlState::Idle);
                    let run = state_run(&mut runs, &observation);
                    if let Err(error) = on_agent_event(&watch_hub, watch_bridge.as_ref(), &mut answered, run, observation).await {
                        tracing::debug!(%error, "hub observer event was not applied");
                    }
                    if idle {
                        if let Some(url) = model_url.clone().filter(|url| !url.trim().is_empty()) {
                            let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env());
                            let model = ferryx_hub::meta::backend::UrlModel::new(url);
                            if let Err(error) = ferryx_hub::lanes::kibitzer::evaluate(&watch_hub.pool, &lead, &judge, &model).await {
                                tracing::debug!(%error, "kibitzer evaluation failed");
                            }
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));
    let addr = launch.config.tracker_addr.clone().unwrap_or_else(|| "127.0.0.1:9119".into());
    let router = ferryx_hub::http::router(launch.hub.pool.clone(), ferryx_hub::http::web_root());
    tasks.push(tokio::spawn(async move {
        // A draining predecessor may still hold the port; keep trying until it lets go.
        let listener = ferryx_hub::lifecycle::bind_tcp_with_retry(&addr, Default::default()).await;
        let _bound = HttpBound::mark();
        tracing::info!(%addr, "hub http listening");
        if let Err(error) = axum::serve(listener, router).await {
            tracing::warn!(%error, "hub http stopped");
        }
    }));
    // The omo app-server answers Slack messages (one turn per Slack thread), as it did under
    // omo-gateway. It is supervised unless the supervisor pane preset is used instead, or
    // FERRYX_HUB_SLACK_TURNS=0 turns the path off.
    let slack_turns = !launch.config.supervisor_pane
        && !matches!(
            ferryx_hub::hub_env::var("FERRYX_HUB_SLACK_TURNS").ok().as_deref().map(str::trim),
            Some("0" | "off" | "false" | "no")
        );
    let omo_url = launch.config.supervisor_listen_url.clone().or_else(|| {
        slack_turns.then(|| ferryx_hub::cron_exec::OMO_APP_SERVER.to_owned())
    });
    if let Some(url) = (!launch.config.supervisor_pane).then_some(omo_url).flatten() {
        let state_dir = ferryx_hub::meta::supervisor::state_dir_from_home().unwrap_or_else(|| std::path::PathBuf::from(".ferryx/hub"));
        let probe = ferryx_hub::meta::supervisor::DaemonSupervisor::new(url.clone(), Vec::new(), state_dir.clone());
        let token_path = match probe.ensure_token_file() {
            Ok(path) => Some(path),
            Err(error) => {
                tracing::warn!(%error, "omo app-server token file was not written");
                None
            }
        };
        let command = if !launch.config.supervisor_command.is_empty() {
            launch.config.supervisor_command.clone()
        } else if let Some(path) = &token_path {
            ferryx_hub::meta::supervisor::app_server_command(&ferryx_hub::meta::supervisor::omo_bin(), &url, path)
        } else {
            Vec::new()
        };
        #[cfg(feature = "hub-slack")]
        if slack_turns {
            let owner = launch.config.owner_slack_id.clone();
            let config = ferryx_hub::chat::slack_turn::SlackTurnConfig::from_env(&url, token_path.clone(), owner.as_deref());
            tracing::info!(
                url = %url,
                workspace = config.omo.workspace.is_some(),
                idle_secs = config.omo.idle_timeout.as_secs(),
                total_secs = config.omo.total_timeout.as_secs(),
                "slack turn path is armed (omo app-server)"
            );
            ferryx_hub::chat::slack_turn::install(config);
            // omo's startup sweep: claims a restart left pending are never re-run.
            match ferryx_hub::chat::report_claims::recover_stale(&launch.hub.pool).await {
                Ok(0) => {}
                Ok(swept) => tracing::info!(swept, "stale report claims marked unmeasurable"),
                Err(error) => tracing::warn!(%error, "stale report claims were not swept"),
            }
            // omo's missed-message backfill: answer what arrived while the hub was down, then
            // reconcile every few minutes. It reads with the bot token; Socket Mode is untouched.
            if let Some(config) = ferryx_hub::chat::slack_turn::installed() {
                if ferryx_hub::chat::slack::app_token().is_some() {
                    tasks.push(tokio::spawn(ferryx_hub::chat::slack_backfill::run_forever(
                        config,
                        launch.hub.pool.clone(),
                    )));
                }
            }
        }
        tasks.push(tokio::spawn(async move {
            let supervisor = ferryx_hub::meta::supervisor::DaemonSupervisor::new(url, command, state_dir);
            let mut announced = false;
            let mut backoff = std::time::Duration::from_secs(5);
            loop {
                match supervisor.ensure().await {
                    Ok(pid) => {
                        if supervisor.wait_responsive(std::time::Duration::from_secs(60)).await {
                            if !announced {
                                tracing::info!(pid = ?pid, "omo app-server is ready");
                                announced = true;
                            }
                            backoff = std::time::Duration::from_secs(5);
                        } else {
                            tracing::warn!(pid = ?pid, "omo app-server did not start listening");
                            announced = false;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "omo app-server was not started");
                        announced = false;
                    }
                }
                tokio::time::sleep(backoff).await;
                if !supervisor.responsive().await {
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(300));
                } else {
                    backoff = std::time::Duration::from_secs(15);
                }
            }
        }));
    }
    #[cfg(unix)]
    {
        let home = home_dir();
        for path in ferryx_hub::schedule::native_socket_paths(&home) {
            let pool = launch.hub.pool.clone();
            tasks.push(tokio::spawn(ferryx_hub::lifecycle::serve_native_socket_with_retry(
                path,
                pool,
                Default::default(),
            )));
        }
    }
    let cron_pool = launch.hub.pool.clone();
    tasks.push(tokio::spawn(async move {
        loop {
            match ferryx_hub::schedule::drive_due_now(&cron_pool).await {
                Ok(deliveries) => {
                    for item in deliveries {
                        let error = post_cron_delivery(&item).await;
                        if let Err(error) = ferryx_hub::schedule::remember_delivery(
                            &cron_pool,
                            &item.job_id,
                            error.as_deref(),
                        )
                        .await
                        {
                            tracing::warn!(%error, job_id = %item.job_id, "cron delivery was not recorded");
                        }
                    }
                }
                Err(error) => tracing::warn!(%error, "cron drive failed"),
            }
            let wait = ferryx_hub::schedule::next_wait_now(&cron_pool)
                .await
                .unwrap_or(std::time::Duration::from_secs(60 * 60));
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = ferryx_hub::schedule::scheduler_wake().notified() => {}
            }
        }
    }));
    #[cfg(feature = "hub-slack")]
    if ferryx_hub::chat::slack::app_token().is_some() {
        let owner = launch.config.owner_slack_id.clone().unwrap_or_default();
        let slack_bridge = launch.bridge.clone();
        let slack_profiles = launch.profiles.clone();
        let slack_meta = launch.meta_session_id.clone();
        let slack_pool = launch.hub.pool.clone();
        tasks.push(tokio::spawn(async move {
            let owner_for_frames = owner.clone();
            if let Err(error) = ferryx_hub::chat::slack::connect_socket_mode(move |text| {
                let bridge = slack_bridge.clone();
                let profiles = slack_profiles.clone();
                let meta = slack_meta.clone();
                let owner = owner_for_frames.clone();
                let pool = slack_pool.clone();
                async move {
                    if let Err(error) = ferryx_hub::chat::slack::apply_frame_with(&text, &owner, &meta, &profiles, bridge.as_ref(), Some(&pool), ferryx_hub::hub_env::var("SLACK_BOT_USER_ID").ok().as_deref()).await {
                        tracing::warn!(%error, "slack frame was not applied");
                    }
                }
            })
            .await
            {
                tracing::warn!(%error, "slack socket mode stopped");
            }
        }));
    }
    #[cfg(feature = "hub-discord")]
    if ferryx_hub::chat::discord::bot_token().is_some() && !ferryx_hub::chat::discord::enabled() {
        tracing::info!("discord is off (set FERRYX_HUB_DISCORD=1 to enable it)");
    }
    #[cfg(feature = "hub-discord")]
    if ferryx_hub::chat::discord::active_bot_token().is_some() {
        let discord_bridge = launch.bridge.clone();
        let discord_profiles = launch.profiles.clone();
        let discord_meta = launch.meta_session_id.clone();
        let discord_pool = launch.hub.pool.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(error) = ferryx_hub::chat::discord::connect_gateway(move |frame| {
                let bridge = discord_bridge.clone();
                let profiles = discord_profiles.clone();
                let meta = discord_meta.clone();
                let pool = discord_pool.clone();
                async move {
                    if let Ok(ferryx_hub::chat::discord::DiscordDispatch::Inbound(inbound)) =
                        ferryx_hub::chat::discord::handle_dispatch(&frame)
                    {
                        // The hub config has no owner Discord id, so only author-less frames are skipped.
                        if let Err(error) = ferryx_hub::inbox_watch::note_gateway(&pool, &inbound, "").await {
                            tracing::warn!(%error, "discord message was not recorded for the inbox");
                        }
                    }
                    if let Err(error) = ferryx_hub::chat::discord::apply_dispatch(&pool, &frame, &meta, &profiles, bridge.as_ref()).await {
                        tracing::warn!(%error, "discord frame was not applied");
                    }
                }
            })
            .await
            {
                tracing::warn!(%error, "discord gateway stopped");
            }
        }));
    }
    if ferryx_hub::inbox_watch::inbox_enabled() {
        let inbox_pool = launch.hub.pool.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(error) = ferryx_hub::inbox_watch::spawn(inbox_pool).await {
                tracing::warn!(%error, "inbox watchers stopped");
            }
        }));
    }
    // omo-gateway's lead is the owner's Slack DM session, not a lane name: action-loop items,
    // owner alerts and cron problems queued for "meta" were never delivered.
    let lead_fallback = launch
        .config
        .meta_session_id
        .clone()
        .unwrap_or_else(|| "meta".into());
    let lead_owner = launch
        .config
        .owner_slack_id
        .clone()
        .filter(|owner| !owner.trim().is_empty())
        .unwrap_or_else(|| ferryx_hub::config::DEFAULT_SLACK_OWNER_ID.to_owned());
    if ferryx_hub::autonomy::action_loop_enabled() {
        let action_pool = launch.hub.pool.clone();
        let (owner, fallback) = (lead_owner.clone(), lead_fallback.clone());
        tasks.push(tokio::spawn(async move {
            let lead = ferryx_hub::lanes::lead::resolve(&action_pool, &owner, &fallback).await;
            if let Err(error) = ferryx_hub::autonomy::serve(action_pool, lead).await {
                tracing::warn!(%error, "action loop stopped");
            }
        }));
    }
    #[cfg(feature = "hub-slack")]
    if let Some(config) = ferryx_hub::chat::slack_turn::installed() {
        let pool = launch.hub.pool.clone();
        let (owner, fallback) = (lead_owner, lead_fallback);
        tasks.push(tokio::spawn(async move {
            let lead = ferryx_hub::lanes::lead::resolve(&pool, &owner, &fallback).await;
            ferryx_hub::chat::slack_turn::serve_lane_events(config, pool, lead).await;
        }));
    }
    tasks.push(tokio::spawn(ferryx_hub::owner_alert::serve_delivery(
        launch.hub.pool.clone(),
        launch.hub.owner_session().storage_key(),
    )));
    tasks.push(tokio::spawn(ferryx_hub::retention::serve(launch.hub.pool.clone())));
    // The tracker's Sessions board and SES issues read and drive panes through the hub's control.
    ferryx_hub::tracker_sessions::install(launch.bridge.clone());
    // omo's judge breaker alert reaches the owner through the owner alert sweep.
    ferryx_hub::judge::install_alerts(
        launch.hub.pool.clone(),
        launch.hub.owner_session().storage_key(),
    );
    // omo's Full Disk Access owner DM, once per hub binary.
    {
        let pool = launch.hub.pool.clone();
        let owner = launch.hub.owner_session().storage_key();
        tasks.push(tokio::spawn(async move {
            let denied = crate::permissions::check_full_disk_access()
                == crate::permissions::PermissionStatus::Denied;
            let identity = std::env::current_exe()
                .and_then(std::fs::metadata)
                .map(|meta| {
                    let modified = meta
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |since| since.as_secs());
                    format!("{}:{modified}", meta.len())
                })
                .unwrap_or_default();
            let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
                return;
            };
            let marker = home.join(".ferryx").join("hub").join("fda-dm-marker");
            if let Err(error) = ferryx_hub::owner_alert::alert_missing_full_disk_access(
                &pool, &owner, denied, &marker, &identity,
            )
            .await
            {
                tracing::warn!(%error, "full disk access alert failed");
            }
        }));
    }
    // omo's cron triage and auto-fix: queued cron problems go to the remediator.
    tasks.push(tokio::spawn(ferryx_hub::cron_remediate::serve(
        launch.hub.pool.clone(),
        Some(launch.hub.owner_session().storage_key()),
    )));
    for task in &tasks {
        HUB_TASKS.track(task);
    }
    Generation { tasks }
}

async fn post_cron_delivery(item: &ferryx_hub::schedule::DueDelivery) -> Option<String> {
    let mut errors = Vec::new();
    for target in &item.targets {
        let posted = match target.platform.as_str() {
            "slack" => post_slack_delivery(&target.chat_id, target.thread_id.as_deref(), &item.text).await,
            "discord" => post_discord_delivery(&target.chat_id, &item.text).await,
            other => Err(ferryx_hub::HubError::message(format!(
                "unconfigured delivery transport: {other}"
            ))),
        };
        match posted {
            Ok(true) => {}
            Ok(false) => errors.push(format!(
                "unconfigured delivery transport: {}",
                target.platform
            )),
            Err(error) => errors.push(error.to_string()),
        }
    }
    if errors.is_empty() {
        if let Some(ack) = &item.ack {
            if let Err(error) = ferryx_hub::schedule::run_ack(ack).await {
                return Some(error.to_string());
            }
        }
        None
    } else {
        Some(errors.join("; "))
    }
}

async fn post_slack_delivery(channel: &str, thread: Option<&str>, text: &str) -> ferryx_hub::Result<bool> {
    #[cfg(feature = "hub-slack")]
    {
        return ferryx_hub::chat::slack::deliver(channel, thread, text).await;
    }
    #[cfg(not(feature = "hub-slack"))]
    {
        let _ = (channel, thread, text);
        Ok(false)
    }
}

async fn post_discord_delivery(channel: &str, text: &str) -> ferryx_hub::Result<bool> {
    #[cfg(feature = "hub-discord")]
    {
        return ferryx_hub::chat::discord::deliver(channel, text).await;
    }
    #[cfg(not(feature = "hub-discord"))]
    {
        let _ = (channel, text);
        Ok(false)
    }
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn serve_control(running: Arc<tokio::sync::Mutex<Option<Generation>>>, launch: HubLaunch) {
    #[cfg(unix)]
    {
        let path = ferryx_hub::control::control_socket(&home_dir());
        let control = tokio::spawn(async move {
            let mut backoff = ferryx_hub::lifecycle::BindBackoff::default();
            let listener = loop {
                match ferryx_hub::lifecycle::bind_control_socket(&path).await {
                    Ok(listener) => break listener,
                    Err(error) => {
                        tracing::warn!(%error, "hub control socket was not bound; retrying");
                        tokio::time::sleep(backoff.step()).await;
                    }
                }
            };
            tracing::info!(path = %path.display(), "hub control socket listening");
            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        tracing::warn!(%error, "hub control accept failed");
                        continue;
                    }
                };
                let running = Arc::clone(&running);
                let launch = launch.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                    let mut reader = tokio::io::BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.is_err() {
                        return;
                    }
                    let reply = match ferryx_hub::control::parse_control(&line) {
                        Ok(ferryx_hub::control::ControlOp::Stop) => {
                            let persisted = ferryx_hub::control::mark_stopped(&launch.hub.pool).await;
                            if let Some(generation) = running.lock().await.take() {
                                for task in generation.tasks {
                                    task.abort();
                                }
                            }
                            match persisted {
                                Ok(()) => "{\"status\":\"ok\",\"op\":\"stop\"}\n",
                                Err(error) => {
                                    tracing::warn!(%error, "hub stop was not persisted");
                                    "{\"status\":\"failed\",\"error\":\"hub stop was not persisted\"}\n"
                                }
                            }
                        }
                        Ok(ferryx_hub::control::ControlOp::Start) if HUB_TASKS.is_retired() => {
                            "{\"status\":\"failed\",\"error\":\"this daemon handed the hub over to its successor\"}\n"
                        }
                        Ok(ferryx_hub::control::ControlOp::Start) => {
                            match ferryx_hub::control::clear_stopped(&launch.hub.pool).await {
                                Ok(()) => {
                                    let mut slot = running.lock().await;
                                    if slot.is_none() {
                                        *slot = Some(spawn_generation(&launch).await);
                                    }
                                    "{\"status\":\"ok\",\"op\":\"start\"}\n"
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "hub start did not clear the stopped flag");
                                    "{\"status\":\"failed\",\"error\":\"hub start was not persisted\"}\n"
                                }
                            }
                        }
                        Ok(ferryx_hub::control::ControlOp::Status) => {
                            let running_now = running.lock().await.is_some();
                            let report = ferryx_hub::control::status_report(
                                &launch.hub.pool,
                                &live_hub(running_now, &launch.config),
                            )
                            .await;
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(format!("{report}\n").as_bytes()).await;
                            return;
                        }
                        Err(_) => "{\"status\":\"failed\",\"error\":\"expected stop, start, or status\"}\n",
                    };
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
        HUB_TASKS.track(&control);
    }
    #[cfg(not(unix))]
    {
        let _ = (running, launch);
    }
}

/// Sequence of the first observation in this session's current state run. A blocked pane
/// re-observed while its screen redraws is the same run, so its notice keeps one dedupe key;
/// each new run (a new turn, a new block) gets a new key.
fn state_run(
    runs: &mut std::collections::HashMap<String, (AgentControlState, Option<String>, u64)>,
    observation: &crate::agent_control::events::AgentObservation,
) -> u64 {
    let current = (observation.state, observation.prompt_id.clone(), observation.sequence);
    let entry = runs
        .entry(observation.session_id.clone())
        .or_insert_with(|| current.clone());
    if entry.0 != current.0 || entry.1 != current.1 {
        *entry = current;
    }
    entry.2
}

/// The pending prompt of an `agents.read` result as a card the owner notice can carry.
fn approval_card(session_id: &str, read: &Value) -> Option<ferryx_hub::sessions::watch::ApprovalCard> {
    let pending = read.pointer("/session/pending")?;
    let prompt_id = pending.get("id").and_then(Value::as_str)?.to_owned();
    let screen_revision = read.pointer("/session/screenRevision").and_then(Value::as_str)?.to_owned();
    let options: Vec<String> = pending
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .map(|option| option.get("label").and_then(Value::as_str).unwrap_or("").to_owned())
        .collect();
    (!options.is_empty() && pending.get("multiSelect").and_then(Value::as_bool) != Some(true)).then(|| {
        ferryx_hub::sessions::watch::ApprovalCard { session_id: session_id.to_owned(), prompt_id, screen_revision, options }
    })
}

async fn on_agent_event(
    hub: &Arc<Hub>,
    control: &DaemonBridge,
    answered: &mut std::collections::HashSet<String>,
    run: u64,
    observation: crate::agent_control::events::AgentObservation,
) -> Result<(), HubError> {
    let session_id = observation.session_id.clone();
    let mut question = None;
    let mut approval = None;
    let mut screen_readable = observation.screen_revision.is_some() && observation.state_source != AgentStateSource::None;
    if observation.prompt_id.is_some() {
        if let Ok(value) = control.execute(None, AgentOp::Read { session_id: session_id.clone() }).await {
            question = value.pointer("/session/pending/question").and_then(Value::as_str).map(str::to_owned);
            approval = approval_card(&session_id, &value);
            if value.pointer("/session/screen/text").and_then(Value::as_str).unwrap_or("").is_empty() {
                screen_readable = false;
            }
        }
    }
    let target = format!("local:{session_id}");
    let watched = ferryx_hub::sessions::watch::Watcher::is_watched(&hub.pool, &target).await?;
    let obs = Observation {
        target: target.clone(),
        machine: "local".into(),
        session_id: session_id.clone(),
        name: session_id.clone(),
        message_count: run,
        streaming: matches!(observation.state, AgentControlState::Working),
        blocking_question: matches!(observation.state, AgentControlState::Blocked) && question.is_none(),
        question: question.clone(),
        last_assistant: String::new(),
        watched,
        machine_failed: false,
        failed: observation.kind.as_deref().is_some_and(|kind| {
            let kind = kind.to_ascii_lowercase();
            kind.contains("fail") || kind.contains("error") || kind.contains("crash")
        }),
        approval,
    };
    hub.ingest(&obs).await?;
    let reading = Reading {
        state_source: match observation.state_source {
            AgentStateSource::Provider => "provider".into(),
            AgentStateSource::Screen => "screen".into(),
            AgentStateSource::None => "none".into(),
        },
        kind: observation.kind.clone(),
        screen_readable,
    };
    let suggested = question.unwrap_or_default();
    if let (Some(prompt_id), Some(revision)) = (observation.prompt_id.clone(), observation.screen_revision.clone()) {
        if answered.insert(prompt_id.clone()) {
            match auto_answer(&reading, &suggested) {
                // A person paused this pane's lane: the supervisor keeps watching but types
                // nothing. Escalations still reach the owner.
                AutoDecision::Answer { .. }
                    if ferryx_hub::lanes::supervised::session_paused(&hub.pool, &session_id).await? =>
                {
                    // Forget the prompt so a resume can still answer it.
                    answered.remove(&prompt_id);
                }
                AutoDecision::Answer { .. } => {
                    let _ = control
                        .execute(
                            None,
                            AgentOp::Answer {
                                session_id,
                                prompt_id: prompt_id.clone(),
                                screen_revision: revision,
                                answer: json!({"optionIndex": 0}),
                                request_id: format!("hub-auto-{prompt_id}"),
                            },
                        )
                        .await;
                }
                AutoDecision::EscalateOwner => {
                    // A card is the only notice for this prompt. The plain line is for an
                    // escalation the watcher cannot turn into buttons.
                    if let Some(text) = ferryx_hub::sessions::watch::escalation_plain_text(
                        &prompt_id,
                        &target,
                        obs.approval.is_some(),
                    ) {
                        let session = hub.owner_session();
                        let _ = ferryx_hub::chat::reply::chat_reply(&hub.pool, &session, &text, None).await;
                    }
                }
                AutoDecision::Hold => {}
            }
        }
    }
    Ok(())
}

/// `ferryx-cli mcp` — a stdio MCP server over this machine's daemon.
pub fn run_mcp() -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ferryx mcp: {error}");
            return 1;
        }
    };
    runtime.block_on(async {
        let bridge = Arc::new(DaemonBridge { client: DaemonClient::new() });
        let config = HubConfig::load().unwrap_or_default();
        // The same database the daemon's hub opened: the config file path wins, then
        // FERRYX_HUB_DB, then the default.
        let path = config.database_path.clone().unwrap_or_else(ferryx_hub::hub::default_database_path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let pool = match ferryx_hub::storage::connect(&path).await {
            Ok(pool) => pool,
            Err(error) => {
                eprintln!("ferryx mcp: {error}");
                return 1;
            }
        };
        let context = Arc::new(ferryx_hub::mcp::McpContext {
            control: bridge.clone(),
            launcher: bridge,
            pool,
            meta_session_id: config.meta_session_id.unwrap_or_else(|| "meta".into()),
            tracker_token: config.tracker_owner_token.unwrap_or_default(),
        });
        if let Err(error) = ferryx_hub::mcp::serve(context).await {
            eprintln!("ferryx mcp: {error}");
            return 1;
        }
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_persons_relayed_words_are_not_supervisor_input() {
        assert!(relayed_from_a_person(Some("slack-approval:local:pane-1:rev-42:prompt-9")));
        assert!(relayed_from_a_person(Some("chat-lane:ledger-1700")));
        assert!(relayed_from_a_person(Some("lane-ledger-owner-msg7")));
        assert!(!relayed_from_a_person(Some("hub-auto-prompt-3")));
        assert!(!relayed_from_a_person(Some("lane-ledger-drive-2")));
        assert!(!relayed_from_a_person(None));
    }

    #[test]
    fn a_failed_control_reply_exits_nonzero() {
        assert_eq!(control_exit_code("{\"status\":\"ok\",\"op\":\"start\"}\n", false), 0);
        assert_eq!(
            control_exit_code("{\"status\":\"failed\",\"error\":\"hub start was not persisted\"}\n", false),
            1
        );
        assert_eq!(control_exit_code("{\"status\":\"ok\",\"healthy\":false}", false), 0);
        assert_eq!(control_exit_code("{\"status\":\"ok\",\"healthy\":false}", true), 1);
        assert_eq!(control_exit_code("{\"status\":\"ok\",\"healthy\":true}", true), 0);
    }

    async fn lane_pool() -> ferryx_hub::goals::SqlitePool {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::lanes::state::insert_lane(&pool, "ledger", "Ledger", "lead", "/tmp", "file marker")
            .await
            .unwrap();
        ferryx_hub::lanes::state::set_pane(&pool, "ledger", "pane-1").await.unwrap();
        pool
    }

    async fn turn_rows(pool: &ferryx_hub::goals::SqlitePool) -> Vec<(String, Option<String>)> {
        ferryx_hub::goals::turns::turns_of(pool, "ledger")
            .await
            .unwrap()
            .into_iter()
            .map(|turn| (turn.origin.as_str().to_owned(), turn.text_excerpt))
            .collect()
    }

    #[tokio::test]
    async fn five_keystroke_notes_in_one_window_record_one_turn_without_text() {
        let pool = lane_pool().await;
        let bursts = KeystrokeBursts::default();
        let start = 1_000_000;
        let mut recorded = 0;
        for offset in 0..5 {
            if record_human_keystrokes(&pool, &bursts, "pane-1", start + offset * 100).await.unwrap() {
                recorded += 1;
            }
        }
        assert_eq!(recorded, 1);
        assert_eq!(turn_rows(&pool).await, vec![("human_correction".to_owned(), None)]);
        let window = crate::agent_control::policy::HUMAN_TYPING_WINDOW_MS;
        assert!(record_human_keystrokes(&pool, &bursts, "pane-1", start + window).await.unwrap());
        assert_eq!(turn_rows(&pool).await.len(), 2);
    }

    #[tokio::test]
    async fn keystrokes_in_a_pane_without_a_lane_record_nothing() {
        let pool = lane_pool().await;
        let bursts = KeystrokeBursts::default();
        assert!(!record_human_keystrokes(&pool, &bursts, "pane-9", 1_000).await.unwrap());
        assert!(turn_rows(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn a_relayed_send_records_a_human_correction_with_a_short_excerpt() {
        let pool = lane_pool().await;
        let op = AgentControlOp::Send {
            session_id: "pane-1".into(),
            text: format!("please also fix the docs {}", "and more ".repeat(80)),
            request_id: Some("lane-ledger-owner-msg7".into()),
        };
        assert!(record_control_turn(&pool, &op).await.unwrap());
        let rows = turn_rows(&pool).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "human_correction");
        let excerpt = rows[0].1.clone().unwrap();
        assert!(excerpt.starts_with("please also fix the docs"));
        assert!(excerpt.chars().count() <= 240);
    }

    #[tokio::test]
    async fn a_supervisor_goal_send_records_a_supervisor_proposal() {
        let pool = lane_pool().await;
        let op = AgentControlOp::Send {
            session_id: "pane-1".into(),
            text: "run the tests".into(),
            request_id: Some("hub-goal-ledger-1".into()),
        };
        assert!(record_control_turn(&pool, &op).await.unwrap());
        let auto = AgentControlOp::Send {
            session_id: "pane-1".into(),
            text: "1".into(),
            request_id: Some("hub-auto-prompt-3".into()),
        };
        assert!(!record_control_turn(&pool, &auto).await.unwrap());
        assert_eq!(
            turn_rows(&pool).await,
            vec![("supervisor_proposal".to_owned(), Some("run the tests".to_owned()))]
        );
    }

    #[test]
    fn claude_and_codex_history_parsers_feed_the_hub_transcript() {
        let claude = history_lines("claude", "");
        assert!(claude.is_empty());
        let codex = history_lines("codex", "");
        assert!(codex.is_empty());
    }

    #[test]
    fn an_answer_op_keeps_the_callers_request_id() {
        let op = AgentOp::Answer {
            session_id: "pane".into(),
            prompt_id: "prompt".into(),
            screen_revision: "rev".into(),
            answer: json!({"optionIndex": 0}),
            request_id: "slack-approval-prompt".into(),
        };
        let mapped = to_agent_control_op(&op).unwrap();
        match mapped {
            AgentControlOp::Answer { request_id, screen_revision, answer, .. } => {
                assert_eq!(request_id.as_deref(), Some("slack-approval-prompt"));
                assert_eq!(screen_revision, "rev");
                assert_eq!(answer.option_index, Some(0));
            }
            other => panic!("expected an answer, got {other:?}"),
        }
    }
}
