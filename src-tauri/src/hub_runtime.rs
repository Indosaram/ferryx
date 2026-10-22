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
        let workspace = std::env::var("FERRYX_HUB_WORKSPACE").map_err(|_| {
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
    let mut dry = false;
    let mut rest = args.iter().skip(2);
    match rest.next().map(String::as_str) {
        Some("stop") => return hub_control(ferryx_hub::control::ControlOp::Stop),
        Some("start") => return hub_control(ferryx_hub::control::ControlOp::Start),
        Some("migrate") => {}
        _ => {
            eprintln!("usage: ferryx hub migrate [--from-omo <db>] [--cwd <dir>] [--dry-run]");
            eprintln!("       ferryx hub stop | start");
            return 2;
        }
    }
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => dry = true,
            "--from-omo" => source = rest.next().map(std::path::PathBuf::from),
            "--cwd" => cwd = rest.next().map(std::path::PathBuf::from),
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
        match ferryx_hub::import::open_and_migrate(&path, &source, dry, cwd.as_deref()).await {
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
                0
            }
            Err(error) => {
                eprintln!("migrate: {error}");
                1
            }
        }
    })
}

fn hub_control(op: ferryx_hub::control::ControlOp) -> i32 {
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
                    0
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
        let _ = (op, path);
        eprintln!("hub control uses a unix socket");
        1
    }
}

/// Opens the hub after the daemon has announced ready. Failures are returned to the caller,
/// which logs them and leaves the daemon up.
pub async fn start(server: Arc<DaemonServer>) -> Result<(), String> {
    if std::env::var("FERRYX_HUB").ok().as_deref() == Some("off") {
        return Ok(());
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
    let stopped = ferryx_hub::control::is_stopped(&hub.pool).await.unwrap_or(false);
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
    serve_control(running, launch);
    if stopped {
        tracing::info!("ferryx hub stayed stopped until hub start");
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
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                tracing::info!(%addr, "hub http listening");
                if let Err(error) = axum::serve(listener, router).await {
                    tracing::warn!(%error, "hub http stopped");
                }
            }
            Err(error) => tracing::warn!(%error, %addr, "hub http did not bind"),
        }
    }));
    if let Some(url) = (!launch.config.supervisor_pane).then(|| launch.config.supervisor_listen_url.clone()).flatten() {
        let command = launch.config.supervisor_command.clone();
        let state_dir = ferryx_hub::meta::supervisor::state_dir_from_home().unwrap_or_else(|| std::path::PathBuf::from(".ferryx/hub"));
        tasks.push(tokio::spawn(async move {
            let supervisor = ferryx_hub::meta::supervisor::DaemonSupervisor::new(url, command, state_dir);
            if let Err(error) = supervisor.ensure().await {
                tracing::warn!(%error, "omo app-server was not started");
            } else {
                tracing::info!("omo app-server backend is armed");
            }
        }));
    }
    #[cfg(unix)]
    {
        let home = home_dir();
        for path in ferryx_hub::schedule::native_socket_paths(&home) {
            let pool = launch.hub.pool.clone();
            let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            tasks.push(tokio::spawn(async move {
                if let Err(error) = ferryx_hub::schedule::serve_native_socket(path, pool, stop_rx).await {
                    tracing::warn!(%error, "cron socket was not bound");
                }
            }));
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
                    if let Err(error) = ferryx_hub::chat::slack::apply_frame_with(&text, &owner, &meta, &profiles, bridge.as_ref(), Some(&pool), std::env::var("SLACK_BOT_USER_ID").ok().as_deref()).await {
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
    if ferryx_hub::chat::discord::bot_token().is_some() {
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
                        if let Err(error) = ferryx_hub::inbox_watch::note_gateway(&pool, &inbound).await {
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
    if ferryx_hub::autonomy::action_loop_enabled() {
        let action_pool = launch.hub.pool.clone();
        let lead = launch
            .config
            .meta_session_id
            .clone()
            .unwrap_or_else(|| "meta".into());
        tasks.push(tokio::spawn(async move {
            if let Err(error) = ferryx_hub::autonomy::serve(action_pool, lead).await {
                tracing::warn!(%error, "action loop stopped");
            }
        }));
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
        tokio::spawn(async move {
            let listener = match ferryx_hub::schedule::bind_native_socket(&path).await {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::warn!(%error, "hub control socket was not bound");
                    return;
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
                        Err(_) => "{\"status\":\"failed\",\"error\":\"expected stop or start\"}\n",
                    };
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        });
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
