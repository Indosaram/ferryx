//! Starts the always-on hub inside `ferryx-cli --daemon` and serves `ferryx-cli mcp`.
//!
//! The hub is not constructed by `DaemonServer::new`, so unit tests that build a daemon
//! do not open its database or a chat socket. Supervisor writes go through agent control,
//! which uses the shared reference-chat input queue and refuses a pane a person just typed in.
use std::sync::Arc;

use async_trait::async_trait;
use ferryx_hub::agent_control::{AgentControl, AgentOp, TaskLauncher};
use ferryx_hub::sessions::watch::Observation;
use ferryx_hub::{Hub, HubConfig, HubError};
use serde_json::{json, Value};

use crate::agent_control::{AgentControlOp, AgentControlState};
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
    /// The in-process gateway, when the bridge runs inside the daemon. A confirmed send needs
    /// it to watch the observer log; without it a send counts as started once accepted.
    gateway: Option<Arc<crate::remote::RemoteGatewayState>>,
}

fn hub_send_outcome(outcome: crate::agent_control::delivery::SendOutcome) -> ferryx_hub::goals::SendOutcome {
    use crate::agent_control::delivery::SendOutcome as Daemon;
    use ferryx_hub::goals::SendOutcome as Hub;
    match outcome {
        Daemon::Started => Hub::Started,
        Daemon::Uncertain { code } => Hub::Uncertain { code },
        Daemon::Refused { code } => Hub::Refused { code },
    }
}

#[async_trait]
impl AgentControl for DaemonBridge {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value, HubError> {
        let request = to_agent_control_op(&op).map_err(HubError::message)?;
        let host_id = host_id
            .filter(|host| *host != ferryx_hub::agent_control::LOCAL_HOST_ID && !host.is_empty())
            .map(str::to_owned);
        // A supervisor input carries this machine's lease fence, asked right before the send;
        // a machine that is not the lease holder now sends nothing.
        let (supervisor_fence, supervisor_account) = crate::agent_control::fence::wire(
            crate::agent_control::fence::stamp_for(&request).map_err(|refusal| {
                HubError::message(format!("{} {}", refusal.code(), refusal.message()))
            })?,
        );
        let response = self
            .client
            .send_request(DaemonRequest::AgentControl {
                request,
                host_id,
                supervisor_fence,
                supervisor_account,
            })
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

    async fn send_confirmed(
        &self,
        host_id: Option<&str>,
        session_id: &str,
        text: &str,
        request_id: &str,
    ) -> ferryx_hub::goals::SendOutcome {
        let local = host_id
            .is_none_or(|host| host.is_empty() || host == ferryx_hub::agent_control::LOCAL_HOST_ID);
        // A local send is confirmed only by the in-process observer. Without one (the
        // standalone `ferryx-cli mcp` process) a raw send could never be confirmed, so it is
        // not attempted and never reported as started.
        if local && self.gateway.is_none() {
            return ferryx_hub::goals::SendOutcome::Refused {
                code: ferryx_hub::goals::loop_driver::OBSERVER_UNAVAILABLE.into(),
            };
        }
        let Some(gateway) = self.gateway.as_ref().filter(|_| local) else {
            // A paired host's send counts as started only from that host's own start receipt
            // naming this request and session; its acceptance alone never does.
            let send = ferryx_hub::agent_control::WorkerSend {
                host_id: host_id.map(str::to_owned),
                session_id: session_id.to_owned(),
                text: text.to_owned(),
                request_id: request_id.to_owned(),
            };
            return self.deliver(&send).await.send_outcome(&send);
        };
        if let Err(refusal) = crate::agent_control::fence::admit_stamped(gateway).await {
            return ferryx_hub::goals::SendOutcome::Refused {
                code: refusal.code().to_owned(),
            };
        }
        hub_send_outcome(
            crate::agent_control::delivery::send_confirmed(
                gateway,
                session_id,
                text.to_owned(),
                request_id.to_owned(),
                crate::agent_control::delivery::turn_start_deadline(),
            )
            .await,
        )
    }

    /// One supervisor dispatch with a causal start receipt. On this machine the in-process
    /// observer attributes the start (`agent_control::delivery::deliver`); a paired host runs
    /// the same delivery and answers with its own receipt, and a host without
    /// `agentControlReceiptsV1` is never sent it. Without an observer nothing is sent.
    async fn deliver(
        &self,
        send: &ferryx_hub::agent_control::WorkerSend,
    ) -> ferryx_hub::agent_control::Delivery {
        use ferryx_hub::agent_control::Delivery;
        let deadline = crate::agent_control::delivery::turn_start_deadline();
        let op = AgentControlOp::Deliver {
            session_id: send.session_id.clone(),
            text: send.text.clone(),
            request_id: send.request_id.clone(),
            observe_ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
        };
        let remote = send
            .host_id
            .as_deref()
            .filter(|host| !host.is_empty() && *host != ferryx_hub::agent_control::LOCAL_HOST_ID);
        let Some(host_id) = remote else {
            let Some(gateway) = self.gateway.as_ref() else {
                return Delivery::Unsupported {
                    code: ferryx_hub::goals::loop_driver::OBSERVER_UNAVAILABLE.into(),
                };
            };
            // The supervisor's owner fence is asked inside the pane's input step, right before
            // the first byte: a pause, cancel or correction that landed while the delivery
            // waited for the pane refuses it with a code that proves nothing was typed. It is
            // one-shot: once the first byte is allowed, the ask before Enter always passes.
            // The lease fence is ratcheted on this machine's daemon before that.
            if let Err(refusal) = crate::agent_control::fence::admit_stamped(gateway).await {
                return Delivery::from_error_code(Some(refusal.code()));
            }
            let fence = crate::agent_control::delivery::supervisor_fence(&send.request_id);
            return match crate::remote::server::agent_control_service::handle_as(
                gateway,
                op,
                "owner",
                Some(fence),
            )
            .await
            {
                Ok(result) => crate::agent_control::delivery::hub_delivery_from_reply(&result),
                Err(error) => crate::agent_control::delivery::hub_delivery(Err(error)),
            };
        };
        let (supervisor_fence, supervisor_account) =
            match crate::agent_control::fence::stamp_for(&op) {
                Ok(fence) => crate::agent_control::fence::wire(fence),
                Err(refusal) => return Delivery::from_error_code(Some(refusal.code())),
            };
        let request = DaemonRequest::AgentControl {
            request: op,
            host_id: Some(host_id.to_owned()),
            supervisor_fence,
            supervisor_account,
        };
        match self.client.send_request(request).await {
            Ok(DaemonResponse::AgentControlOk { result }) => {
                crate::agent_control::delivery::hub_delivery_from_reply(&result)
            }
            Ok(DaemonResponse::Error { code, .. }) => Delivery::from_error_code(code.as_deref()),
            // Whether the daemon forwarded the request cannot be known.
            Ok(_) | Err(_) => Delivery::Uncertain {
                accepted: false,
                code: ferryx_hub::agent_control::SEND_FAILED.into(),
            },
        }
    }
}

/// A git remote URL in a form two checkouts of one repository share.
fn remote_key(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.strip_suffix(".git").unwrap_or(url).to_ascii_lowercase()
}

impl DaemonBridge {
    async fn paired(
        &self,
        host: &crate::paired_host::inventory::HostView,
        operation: crate::paired_host::client::Operation,
    ) -> Result<crate::paired_host::client::OperationResult, HubError> {
        let request = crate::paired_host::client::OperationRequest {
            host_id: host.host_id.clone(),
            generation: host.generation,
            operation,
        };
        self.client
            .paired_host_operation(request)
            .await
            .map(|response| response.result)
            .map_err(|error| HubError::message(format!("{} paired host {}", error.code, host.host_id)))
    }
}

#[async_trait]
impl TaskLauncher for DaemonBridge {
    async fn remote_hosts(&self) -> Result<Vec<String>, HubError> {
        use crate::paired_host::inventory::{AuthStatus, GrantScope};
        let hosts = self
            .client
            .paired_host_list()
            .await
            .map_err(|error| HubError::message(error.message))?;
        Ok(hosts
            .into_iter()
            .filter(|host| {
                host.grant_scope == GrantScope::Machine
                    && host.auth_status == AuthStatus::Paired
                    && host.online
                    && host.unavailable_reason.is_none()
            })
            .map(|host| host.host_id)
            .collect())
    }

    async fn launch_remote_pane(
        &self,
        host_id: &str,
        cwd: &str,
        lane_key: &str,
    ) -> Result<ferryx_hub::agent_control::RemotePane, HubError> {
        use crate::paired_host::client::{Operation, OperationResult};
        use crate::remote::machine_protocol as m;
        let output = ferryx_hub::child_env::command("git")
            .arg("-C")
            .arg(cwd)
            .args(["config", "--get", "remote.origin.url"])
            .output()
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        let origin = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !output.status.success() || origin.is_empty() {
            return Err(HubError::message(format!("{cwd} has no origin remote to match on a paired host")));
        }
        let host = self
            .client
            .paired_host_list()
            .await
            .map_err(|error| HubError::message(error.message))?
            .into_iter()
            .find(|host| host.host_id == host_id)
            .ok_or_else(|| HubError::message(format!("no paired host is named {host_id}")))?;
        let OperationResult::Projects(projects) = self.paired(&host, Operation::Projects).await? else {
            return Err(HubError::message("the paired host answered projects with another result"));
        };
        let wanted = remote_key(&origin);
        let workspace_id = projects
            .projects
            .iter()
            .find(|project| {
                project.metadata.availability == m::Availability::Ready
                    && project.metadata.git_remote.as_deref().is_some_and(|url| remote_key(url) == wanted)
            })
            .map(|project| project.remote_workspace_id.clone())
            .ok_or_else(|| HubError::message(format!("{host_id} has no ready project for {origin}")))?;
        let worktree = m::WorktreeIdentity { ws_id: workspace_id.clone(), slug: format!("lane-{lane_key}") };
        let created = self
            .paired(
                &host,
                Operation::CreateWorktree {
                    request: m::CreateWorktreeRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        workspace_id: workspace_id.clone(),
                        worktree: worktree.clone(),
                        base_ref: None,
                    },
                },
            )
            .await;
        // A lane relaunched after a failed start reuses the worktree it already has.
        if let Err(error) = created {
            if !error.to_string().starts_with("WORKTREE_EXISTS") {
                return Err(error);
            }
        }
        let OperationResult::CreateSession(session) = self
            .paired(
                &host,
                Operation::CreateSession {
                    request: m::CreateSessionRequest {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        workspace_id,
                        worktree: Some(worktree),
                        cols: 100,
                        rows: 32,
                        inherit_from_session_id: None,
                        cwd_relative: None,
                        startup: m::Startup::Shell,
                    },
                },
            )
            .await?
        else {
            return Err(HubError::message("the paired host answered create session with another result"));
        };
        Ok(ferryx_hub::agent_control::RemotePane { session_id: session.target.session_id, cwd: session.cwd })
    }

    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String, HubError> {
        let workspace = ferryx_hub::hub_env::var("FERRYX_HUB_WORKSPACE").map_err(|_| {
            HubError::config("FERRYX_HUB_WORKSPACE is required before the hub can launch a pane")
        })?;
        let shell = if command.is_empty() {
            None
        } else {
            Some(command.join(" "))
        };
        self.client
            .spawn_terminal(
                hub_spawn_request_id(),
                workspace,
                crate::daemon::session_service::SpawnOptions {
                    worktree: None,
                    cwd: Some(cwd.to_owned()),
                    cols: 100,
                    rows: 32,
                    shell,
                },
            )
            .await
            .map_err(|error| HubError::message(error.to_string()))
    }
}

/// A fresh daemon spawn request id for each pane the hub launches. The daemon treats a reused
/// id as a retry of the same spawn (session_service `handle_spawn`): with the same options it
/// returns the live pane it already made, with other options it refuses. One id per process
/// therefore gave a second supervisor goal the first goal's pane, or refused its launch.
fn hub_spawn_request_id() -> String {
    format!(
        "hub-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    )
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
        Some("cutover") => return hub_control(ferryx_hub::control::ControlOp::Cutover, false),
        Some("rollback") => return hub_control(ferryx_hub::control::ControlOp::Rollback, false),
        Some("intake") => return hub_control(ferryx_hub::control::ControlOp::Intake, false),
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
        Some("cron") => return hub_cron_cli(&args[3.min(args.len())..]),
        Some("lease") => return hub_lease_cli(&args[3.min(args.len())..]),
        Some("migrate") => {}
        _ => {
            eprintln!("usage: ferryx hub migrate [--from-omo <db>] [--cwd <dir>] [--imported-from <db>] [--dry-run]");
            eprintln!("       ferryx hub stop | start | status [--check]");
            eprintln!("       ferryx hub cutover | rollback | intake");
            eprintln!(
                "       ferryx hub env import [--from-plist <omo plist>] | ferryx hub env keys"
            );
            eprintln!("       ferryx hub cron list | ferryx hub cron model <job-id> <provider/model | --clear> | ferryx hub cron approve-rerun <problem-id>");
            eprintln!("       ferryx hub lease reset-capability");
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

/// `ferryx hub cron list` prints each job with the model it runs on; `ferryx hub cron model
/// <id> <provider/model>` pins one job to a model and `--clear` returns it to the background
/// model. Edits go to the hub database directly, so the daemon does not need a restart: the
/// scheduler reads the payload at every run. `ferryx hub cron approve-rerun <problem-id>`
/// approves one more verified rerun of a cron problem waiting on the owner, over the owner-only
/// hub control socket.
fn hub_cron_cli(args: &[String]) -> i32 {
    const USAGE: &str =
        "usage: ferryx hub cron list | ferryx hub cron model <job-id> <provider/model | --clear> | ferryx hub cron approve-rerun <problem-id>";
    enum Op {
        List,
        Model(String, Option<String>),
    }
    let op = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["list"] => Op::List,
        ["approve-rerun", id] => {
            return match id.parse::<i64>() {
                Ok(problem) => hub_control(ferryx_hub::control::ControlOp::ApproveRerun(problem), false),
                Err(_) => {
                    eprintln!("{USAGE}");
                    2
                }
            };
        }
        ["model", id, "--clear"] => Op::Model((*id).to_owned(), None),
        ["model", id, model] if !model.starts_with('-') => {
            Op::Model((*id).to_owned(), Some((*model).to_owned()))
        }
        _ => {
            eprintln!("{USAGE}");
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
    runtime.block_on(async move {
        let config = HubConfig::load().unwrap_or_default();
        let path = config.database_path.clone().unwrap_or_else(ferryx_hub::hub::default_database_path);
        if !path.exists() {
            eprintln!("hub database not found at {}", path.display());
            return 1;
        }
        let pool = match ferryx_hub::storage::connect(&path).await {
            Ok(pool) => pool,
            Err(error) => {
                eprintln!("hub database: {error}");
                return 1;
            }
        };
        match op {
            Op::List => {
                let jobs = match ferryx_hub::cron_exec::job_models(&pool).await {
                    Ok(jobs) => jobs,
                    Err(error) => {
                        eprintln!("cron list: {error}");
                        return 1;
                    }
                };
                for job in jobs {
                    let state = if job.enabled { "enabled" } else { "disabled" };
                    let source = if job.overridden { "job" } else { "default" };
                    println!("{}\t{state}\t{}\t{source}", job.id, job.model);
                }
                0
            }
            Op::Model(id, model) => {
                match ferryx_hub::cron_exec::set_model_override(&pool, &id, model.as_deref()).await {
                    Ok(Some(now)) => {
                        println!("{id} {now}");
                        0
                    }
                    Ok(None) => {
                        eprintln!("cron job {id} not found");
                        1
                    }
                    Err(error) => {
                        eprintln!("cron model: {error}");
                        2
                    }
                }
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

/// `ferryx hub lease reset-capability` forgets that this daemon's relay once served the
/// supervisor lease, for a deliberate downgrade to a relay without it. Without the reset such a
/// relay leaves the hub `RELAY_LEASE_UNSUPPORTED` (standby), never holder.
fn hub_lease_cli(args: &[String]) -> i32 {
    use crate::daemon::supervisor_lease_client::{lease_file_path, reset_capability_latch};
    match args.first().map(String::as_str) {
        Some("reset-capability") if args.len() == 1 => {
            let Some(path) = lease_file_path() else {
                eprintln!("cannot resolve the Ferryx data directory");
                return 1;
            };
            match reset_capability_latch(&path) {
                Ok(()) => {
                    println!("supervisor lease capability latch cleared ({})", path.display());
                    0
                }
                Err(error) => {
                    eprintln!("could not clear the lease capability latch: {error}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: ferryx hub lease reset-capability");
            2
        }
    }
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
#[cfg(any(unix, test))]
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
        runtime.block_on(async move {
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
        })
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

/// A swappable process-wide value. Under `cfg(test)` each test thread has its own, so tests
/// drive the product's accessors without sharing state with tests running beside them.
#[cfg(not(test))]
type Slot<T> = std::sync::RwLock<Option<T>>;
#[cfg(test)]
type Slot<T> = std::thread::LocalKey<std::cell::RefCell<Option<T>>>;

#[cfg(not(test))]
fn slot_get<T: Clone>(slot: &'static Slot<T>) -> Option<T> {
    slot.read().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(not(test))]
fn slot_set<T>(slot: &'static Slot<T>, value: Option<T>) {
    *slot.write().unwrap_or_else(|e| e.into_inner()) = value;
}

#[cfg(test)]
fn slot_get<T: Clone + 'static>(slot: &'static Slot<T>) -> Option<T> {
    slot.with(|value| value.borrow().clone())
}

#[cfg(test)]
fn slot_set<T: 'static>(slot: &'static Slot<T>, value: Option<T>) {
    slot.with(|current| *current.borrow_mut() = value);
}

/// Declares a [`Slot`], empty until set.
macro_rules! hub_slot {
    ($(#[$meta:meta])* $name:ident: $ty:ty) => {
        $(#[$meta])*
        #[cfg(not(test))]
        static $name: Slot<$ty> = std::sync::RwLock::new(None);
        #[cfg(test)]
        thread_local! {
            $(#[$meta])*
            static $name: std::cell::RefCell<Option<$ty>> = const { std::cell::RefCell::new(None) };
        }
    };
}

hub_slot! {
    /// The opened hub and its running generation, for the desktop's Supervisor view. Swappable:
    /// a promotion that imports a staged hub database replaces the hub (see [`promote_generation`]).
    SUPERVISOR_VIEW: SupervisorView
}

/// The current hub and running slot, `None` until this daemon opened the hub.
fn supervisor_view() -> Option<SupervisorView> {
    slot_get(&SUPERVISOR_VIEW)
}

/// What a start needs besides the running slot, swapped together with [`SUPERVISOR_VIEW`].
fn supervisor_launch() -> Option<HubLaunch> {
    slot_get(&SUPERVISOR_LAUNCH)
}

/// Publishes the hub and launch inputs the view, the phone and control.sock act on.
fn publish_hub(launch: &HubLaunch, running: &Arc<tokio::sync::Mutex<Option<Generation>>>) {
    slot_set(&SUPERVISOR_VIEW, Some((Arc::clone(&launch.hub), Arc::clone(running))));
    slot_set(&SUPERVISOR_LAUNCH, Some(launch.clone()));
}

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
    let Some((hub, running)) = supervisor_view() else {
        #[cfg(test)]
        let test_hub = state.supervisor_test_hub.read().clone();
        #[cfg(test)]
        if let Some((pool, _)) = test_hub {
            return json!({ "hub": { "available": true, "running": true, "error": null }, "lanes": [], "watched": [], "actions": actions, "supervisor": unified_snapshot(&pool).await });
        }
        return json!({ "hub": { "available": false, "running": false, "error": null }, "lanes": [], "watched": [], "actions": actions, "supervisor": null });
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
        "supervisor": unified_snapshot(&hub.pool).await,
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
    let Some((hub, _)) = supervisor_view() else { return };
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
    // The raw words: `turns::excerpt` redacts secrets before it caps the length. The audit
    // line is already cut, so a secret straddling its cut would slip past redaction.
    let text = match op {
        AgentControlOp::Send { text, .. } => text.clone(),
        AgentControlOp::Answer { answer, .. } => match &answer.custom_text {
            Some(text) => text.clone(),
            None => crate::agent_control::policy::audit_text(op).unwrap_or_default(),
        },
        _ => String::new(),
    };
    Ok(ferryx_hub::goals::turns::record_send_turn(pool, session_id, request_id, &text).await?.is_some())
}

/// Called after an agent-control mutation succeeded. Records the lane turn it drove, if any.
pub(crate) fn note_agent_control_input(op: &AgentControlOp) {
    let request_id = op.mutation_request_id();
    if ferryx_hub::goals::turns::origin_for_request_id(request_id.as_deref()).is_none() {
        return;
    }
    // The goal loop (`hub-goal-*`) and the supervisor dispatcher (`sv-*`) record their own
    // turn once the pane is seen starting it.
    if ferryx_hub::goals::turns::records_own_turn(request_id.as_deref()) {
        return;
    }
    let Some((hub, _)) = supervisor_view() else { return };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let pool = hub.pool.clone();
    let op = op.clone();
    runtime.spawn(async move {
        if let Err(error) = record_control_turn(&pool, &op).await {
            tracing::warn!(%error, "a lane turn was not recorded");
        }
    });
}

/// A worker's ordinary report for its own supervisor goal, from the daemon's agent-state
/// socket (`supervisorReport`): handed to the goal's wake service through the hub's lane
/// event path ([`ferryx_hub::lanes::dispatch::report_from_worker`]). The reply is one JSON
/// value: `{ok:true, ...}` or `{ok:false, code, message}`.
pub async fn supervisor_worker_report((session_id, event_id, body): (String, String, String)) -> Value {
    let Some((hub, _)) = supervisor_view() else {
        return json!({"ok": false, "code": "SUPERVISOR_UNAVAILABLE", "message": "the supervisor hub is not open in this daemon"});
    };
    match ferryx_hub::lanes::dispatch::report_from_worker(
        &hub.pool,
        ferryx_hub::agent_control::LOCAL_HOST_ID,
        &session_id,
        &event_id,
        &body,
    )
    .await
    {
        Ok(report) => {
            let mut value = json!(report);
            value["ok"] = json!(true);
            value
        }
        Err(error) => json!({"ok": false, "code": error.code(), "message": error.to_string()}),
    }
}

/// True once this daemon opened the hub, so the Supervisor view has lanes to show.
pub fn supervisor_available() -> bool {
    supervisor_view().is_some()
}

hub_slot! {
    /// What a start needs besides the running slot: the hub, its config and its bridge.
    SUPERVISOR_LAUNCH: HubLaunch
}

/// Why the hub was not started.
#[derive(Debug, thiserror::Error)]
pub enum HubStartError {
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("this daemon handed the hub over to its successor")]
    HandedOver,
    #[error("hub start was not persisted: {0}")]
    Storage(String),
    /// Another machine holds the supervisor lease; this daemon stays on standby.
    #[error("another machine runs the supervisor")]
    Standby {
        holder_machine_id: Option<String>,
        holder_label: Option<String>,
    },
}

/// Clears the persisted stop and runs a generation unless one already runs. Returns whether
/// this call started it. `hub start` on the control socket and a paired phone both land here.
async fn start_generation(
    running: &tokio::sync::Mutex<Option<Generation>>,
    launch: &HubLaunch,
) -> Result<bool, HubStartError> {
    let lease = lease_gate();
    gated_start(running, &launch.hub.pool, lease.as_deref(), || open_generation(launch)).await
}

/// The supervisor lease as the hub start gate sees it. `None` (no lease client) is today's
/// unleased behaviour: every start runs.
#[async_trait]
trait LeaseGate: Send + Sync {
    /// Holder or Provisional, re-checked against both clocks now.
    fn allows_hub(&self) -> bool;
    fn current_role(&self) -> crate::daemon::supervisor_lease_role::Role;
    /// One non-forced acquire; the role once it settled, `None` when the client ended.
    async fn try_acquire(&self) -> Option<crate::daemon::supervisor_lease_role::Role>;
}

#[async_trait]
impl LeaseGate for crate::daemon::supervisor_lease_client::SupervisorLeaseHandle {
    fn allows_hub(&self) -> bool {
        self.state().may_act_now()
    }

    fn current_role(&self) -> crate::daemon::supervisor_lease_role::Role {
        self.state().role()
    }

    async fn try_acquire(&self) -> Option<crate::daemon::supervisor_lease_role::Role> {
        self.acquire(false).await
    }
}

hub_slot! {
    /// The lease client this daemon runs, installed by [`start`] and rebound by [`follow_relay`].
    LEASE_GATE: Arc<dyn LeaseGate>
}

fn lease_gate() -> Option<Arc<dyn LeaseGate>> {
    slot_get(&LEASE_GATE)
}

fn set_lease_gate(gate: Option<Arc<dyn LeaseGate>>) {
    slot_set(&LEASE_GATE, gate);
}

/// The lease client's stop signal (the `wanted` watch [`follow_lease`] follows), installed by
/// [`spawn_lease_client`].
static LEASE_STOP: std::sync::RwLock<Option<Arc<tokio::sync::watch::Sender<bool>>>> =
    std::sync::RwLock::new(None);

/// Installs (or clears) the stop signal [`stand_down_now`] sends on.
pub(crate) fn install_lease_stop(stop: Option<Arc<tokio::sync::watch::Sender<bool>>>) {
    *LEASE_STOP.write().unwrap_or_else(|e| e.into_inner()) = stop;
}

/// Stands the hub down through the lease's own path: the same `false` the lease client's
/// stop sends, so [`follow_lease`] aborts the generation. A no-op without a lease client.
pub(crate) fn stand_down_now() {
    let stop = LEASE_STOP.read().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(stop) = stop {
        tracing::info!("supervisor fence refused; standing the hub down");
        stop.send_replace(false);
    }
}

fn standby_error(role: &crate::daemon::supervisor_lease_role::Role) -> HubStartError {
    use crate::daemon::supervisor_lease_role::Role;
    match role {
        Role::Standby { holder_machine_id, holder_label, .. } => HubStartError::Standby {
            holder_machine_id: holder_machine_id.clone(),
            holder_label: holder_label.clone(),
        },
        _ => HubStartError::Standby { holder_machine_id: None, holder_label: None },
    }
}

/// The control.sock answer to a start refused on standby: `{"status":"standby","holder":...}`.
/// `holder` is the holder's label (its machine id when it has none, null when unknown).
#[cfg(any(unix, test))]
fn standby_reply(error: &HubStartError) -> Value {
    let holder = match error {
        HubStartError::Standby { holder_machine_id, holder_label } => {
            holder_label.clone().or_else(|| holder_machine_id.clone())
        }
        _ => None,
    };
    json!({ "status": "standby", "op": "start", "holder": holder })
}

/// Clears the persisted stop and runs `spawn` into the empty slot, but only on the lease holder:
/// a standby first asks for the lease (never forced) and is refused with the holder when the
/// relay says no, before anything is written.
async fn gated_start<F, Fut>(
    running: &tokio::sync::Mutex<Option<Generation>>,
    pool: &ferryx_hub::goals::SqlitePool,
    lease: Option<&dyn LeaseGate>,
    spawn: F,
) -> Result<bool, HubStartError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Generation>,
{
    if HUB_TASKS.is_retired() {
        return Err(HubStartError::HandedOver);
    }
    if let Some(lease) = lease {
        if !lease.allows_hub() {
            let role = lease.try_acquire().await.unwrap_or_else(|| lease.current_role());
            if !lease.allows_hub() {
                return Err(standby_error(&role));
            }
        }
    }
    if let Err(error) = ferryx_hub::control::clear_stopped(pool).await {
        tracing::warn!(%error, "hub start did not clear the stopped flag");
        return Err(HubStartError::Storage(error.to_string()));
    }
    let mut slot = running.lock().await;
    if slot.is_some() {
        return Ok(false);
    }
    *slot = Some(spawn().await);
    Ok(true)
}

/// The side effects of opening a generation, behind a seam tests count.
#[async_trait]
trait GenerationEffects: Send + Sync {
    /// Posts the stored chat replies (Slack, Discord).
    async fn flush_pending(&self, pool: &ferryx_hub::goals::SqlitePool);
    /// Launches the supervisor pane.
    async fn launch_pane(&self, launch: &HubLaunch, cwd: &str, command: &[String]);
    /// Starts the generation's tasks.
    async fn spawn(&self, launch: &HubLaunch) -> Generation;
}

/// The daemon's real effects.
struct DaemonEffects;

#[async_trait]
impl GenerationEffects for DaemonEffects {
    async fn flush_pending(&self, pool: &ferryx_hub::goals::SqlitePool) {
        if let Err(error) = ferryx_hub::chat::reply::flush_pending(pool).await {
            tracing::warn!(%error, "stored chat replies were not flushed");
        }
    }

    async fn launch_pane(&self, launch: &HubLaunch, cwd: &str, command: &[String]) {
        if let Err(error) = launch.bridge.launch_pane(cwd, command).await {
            tracing::warn!(%error, "supervisor pane was not launched");
        }
    }

    async fn spawn(&self, launch: &HubLaunch) -> Generation {
        spawn_generation(launch).await
    }
}

/// The side effects of a running hub that must never run on a standby: stored chat replies
/// are posted and the supervisor pane is launched, then the generation's tasks start.
async fn open_generation(launch: &HubLaunch) -> Generation {
    launch.effects.flush_pending(&launch.hub.pool).await;
    if launch.config.supervisor_pane {
        let preset = ferryx_hub::meta::pane::SupervisorPanePreset::from_parts(
            launch.config.supervisor_cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            Vec::new(),
        );
        tracing::info!(title = %preset.title, "supervisor pane preset is armed");
        if !preset.command.is_empty() {
            launch.effects.launch_pane(launch, &preset.cwd.to_string_lossy(), &preset.command).await;
        }
    }
    launch.effects.spawn(launch).await
}

/// The generation a booting hub runs: none while the owner stopped it, none on a standby.
async fn boot_generation(launch: &HubLaunch, stopped: bool, lease: Option<&dyn LeaseGate>) -> Option<Generation> {
    let standby = lease.is_some_and(|lease| !lease.allows_hub());
    if stopped || standby {
        return None;
    }
    Some(open_generation(launch).await)
}

/// The rest of [`start`] once the hub is open: the boot generation (if any) goes into the
/// running slot, which is published with the hub for the view, the phone and control.sock.
async fn boot_hub(
    launch: &HubLaunch,
    stopped: bool,
    lease: Option<&dyn LeaseGate>,
) -> Arc<tokio::sync::Mutex<Option<Generation>>> {
    let generation = boot_generation(launch, stopped, lease).await;
    let running = Arc::new(tokio::sync::Mutex::new(generation));
    publish_hub(launch, &running);
    running
}

/// Lease loss: aborts the running generation and waits for its tasks. Unlike `hub stop` it
/// never writes hub_stopped, which stays the owner's choice. Returns whether one ran.
async fn stand_down(running: &tokio::sync::Mutex<Option<Generation>>) -> bool {
    let Some(generation) = running.lock().await.take() else {
        return false;
    };
    for task in &generation.tasks {
        task.abort();
    }
    for task in generation.tasks {
        let _ = task.await;
    }
    true
}

/// Follows the lease client's start/stop decisions: `false` stands the generation down,
/// `true` (promotion) runs `promote` into the empty slot.
async fn follow_lease<F, Fut>(
    mut wanted: tokio::sync::watch::Receiver<bool>,
    running: Arc<tokio::sync::Mutex<Option<Generation>>>,
    mut promote: F,
) where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<Generation>> + Send,
{
    while wanted.changed().await.is_ok() {
        let run = *wanted.borrow_and_update();
        if run {
            let mut slot = running.lock().await;
            if slot.is_none() {
                *slot = promote().await;
                if slot.is_some() {
                    tracing::info!("supervisor lease held; hub generation started");
                }
            }
        } else if stand_down(&running).await {
            tracing::info!("supervisor lease lost; hub generation stopped, hub_stopped unchanged");
        }
    }
}

/// `path` with `suffix` appended to its file name.
fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

/// `<hub.sqlite>.incoming`: a hub database staged by an import for the next promotion.
fn staged_database(database: &std::path::Path) -> std::path::PathBuf {
    with_suffix(database, ".incoming")
}

/// `<hub.sqlite>.pre-move-<ms>`: the backup a promotion keeps of the database it replaced.
const PRE_MOVE_SUFFIX: &str = ".pre-move-";

/// Folds the staged database's WAL into its main file and switches it to a rollback journal,
/// so the one file moved into place carries every committed row (a `-wal` beside
/// `.incoming` would not follow the rename).
async fn seal_staged_database(incoming: std::path::PathBuf) -> Result<(), HubError> {
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let conn = rusqlite::Connection::open(&incoming).map_err(|error| error.to_string())?;
        conn.busy_timeout(ferryx_hub::storage::BUSY_TIMEOUT).map_err(|error| error.to_string())?;
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if busy != 0 {
            return Err("its WAL is still in use".into());
        }
        let mode: String = conn
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(format!("journal mode stayed {mode}"));
        }
        conn.close().map_err(|(_, error)| error.to_string())
    })
    .await
    .map_err(|error| HubError::message(format!("staged hub database: {error}")))?
    .map_err(|error| HubError::message(format!("staged hub database was not sealed: {error}")))
}

/// Puts files moved aside back where they were, last first. Best effort: it runs on a failed
/// swap, whose own error is the one reported.
async fn restore_moved(moved: &[(std::path::PathBuf, std::path::PathBuf)]) {
    for (from, to) in moved.iter().rev() {
        if let Err(error) = tokio::fs::rename(to, from).await {
            tracing::warn!(%error, path = %from.display(), "hub database file was not restored after a failed swap");
        }
    }
}

/// Closes `pool` and moves a staged database over `database`. Returns whether one was staged.
/// Only called while no generation runs, so nothing else writes the closed pool. The replaced
/// database and any `-wal`/`-shm` beside it are kept as `<hub.sqlite>.pre-move-<ms>`(+suffix);
/// a failed move puts them back.
async fn swap_staged_database(
    pool: &ferryx_hub::goals::SqlitePool,
    database: &std::path::Path,
) -> Result<bool, HubError> {
    let incoming = staged_database(database);
    if !tokio::fs::try_exists(&incoming).await.unwrap_or(false) {
        return Ok(false);
    }
    seal_staged_database(incoming.clone()).await?;
    // Resolves once every connection of the old pool is closed.
    pool.close().await;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let backup = with_suffix(database, &format!("{PRE_MOVE_SUFFIX}{stamp}"));
    let mut moved = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let from = with_suffix(database, suffix);
        if !tokio::fs::try_exists(&from).await.unwrap_or(false) {
            continue;
        }
        let to = with_suffix(&backup, suffix);
        if let Err(error) = tokio::fs::rename(&from, &to).await {
            restore_moved(&moved).await;
            return Err(error.into());
        }
        moved.push((from, to));
    }
    if let Err(error) = tokio::fs::rename(&incoming, database).await {
        restore_moved(&moved).await;
        return Err(error.into());
    }
    Ok(true)
}

/// Promotion (Standby -> Holder): reopens the hub on a staged database first, then runs a
/// generation when the owner has not stopped the hub (or FERRYX_HUB=on).
async fn promote_generation() -> Option<Generation> {
    let (_, running) = supervisor_view()?;
    let mut launch = supervisor_launch()?;
    let database = launch
        .config
        .database_path
        .clone()
        .unwrap_or_else(ferryx_hub::hub::default_database_path);
    match swap_staged_database(&launch.hub.pool, &database).await {
        Ok(false) => {}
        Ok(true) => match Hub::open(launch.config.clone(), launch.bridge.clone()).await {
            Ok(hub) => {
                if let Err(error) = ferryx_hub::lanes::kibitzer::ensure_schema(&hub.pool).await {
                    tracing::warn!(%error, "kibitzer schema was not created");
                }
                launch.hub = hub;
                publish_hub(&launch, &running);
                tracing::info!(path = %database.display(), "hub reopened on the staged database");
            }
            Err(error) => {
                tracing::warn!(%error, "hub did not reopen on the staged database");
                return None;
            }
        },
        Err(error) => {
            tracing::warn!(%error, "staged hub database was not swapped in");
            return None;
        }
    }
    let forced_on = std::env::var("FERRYX_HUB").ok().as_deref() == Some("on");
    if !forced_on && ferryx_hub::control::is_stopped(&launch.hub.pool).await.unwrap_or(true) {
        tracing::info!("supervisor lease held; hub stays stopped until hub start");
        return None;
    }
    Some(open_generation(&launch).await)
}

/// The lease client's [`crate::daemon::supervisor_lease_client::HubSwitch`]: its start/stop decisions, followed by [`follow_lease`].
struct HubLeaseSwitch {
    wanted: Arc<tokio::sync::watch::Sender<bool>>,
}

impl crate::daemon::supervisor_lease_client::HubSwitch for HubLeaseSwitch {
    fn start_hub(&self) {
        self.wanted.send_replace(true);
    }

    fn stop_hub(&self, reason: crate::daemon::supervisor_lease_role::StandDownReason) {
        tracing::info!(?reason, "supervisor lease stand-down");
        self.wanted.send_replace(false);
    }
}

/// Starts the lease client over `relay`'s control channel; the caller tracks its task.
async fn spawn_lease_client(
    relay: crate::remote::relay_client::RelayClient,
    wanted: tokio::sync::watch::Sender<bool>,
    handover_manager: Arc<crate::daemon::handover::HandoverManager>,
) -> (Arc<dyn LeaseGate>, tokio::task::JoinHandle<()>) {
    use crate::daemon::supervisor_lease_client::{spawn_supervisor_lease_client, LeaseClientConfig};
    let enrollment = crate::ipc::run_blocking(|| Ok(crate::account::enroll_client::load_enrollment_record()))
        .await
        .ok()
        .flatten();
    let (machine_id, account) = match enrollment {
        Some(record) => (record.machine_record_id, Some(record.account_id)),
        None => ("local".to_owned(), None),
    };
    let mut config = LeaseClientConfig::system(machine_id, account);
    // A handover successor renews the lease its predecessor handed over at commit (same
    // machine id and fence) instead of acquiring a new one; a rebind finds it already taken.
    config.handed_over = handover_manager.take_handed_supervisor_lease();
    // One Arc for the switch and the stop signal: `stand_down_now` sends on the watch the
    // switch drives (clones of a watch sender share its channel, so `follow_relay`'s too).
    let wanted = Arc::new(wanted);
    install_lease_stop(Some(Arc::clone(&wanted)));
    let (handle, task) =
        spawn_supervisor_lease_client(config, Arc::new(relay), Arc::new(HubLeaseSwitch { wanted })).await;
    // A later handover commit freezes this client and hands its lease to the successor.
    let handover: Arc<dyn crate::daemon::handover::SupervisorLeaseHandover> = Arc::new(handle.state().clone());
    handover_manager.set_supervisor_lease(Some(handover));
    // Supervisor inputs are stamped with this lease's fence, checked right before each send;
    // a rebound client replaces the previous client's source.
    let source: Arc<dyn crate::agent_control::fence::SupervisorFenceSource> = Arc::new(handle.state().clone());
    crate::agent_control::fence::install_source(Some(source));
    let gate: Arc<dyn LeaseGate> = Arc::new(handle);
    (gate, task)
}

/// Bumped each time a relay client is published (the gateway's start, `replace_relay`).
static RELAY_PUBLISHED: std::sync::LazyLock<tokio::sync::watch::Sender<u64>> =
    std::sync::LazyLock::new(|| tokio::sync::watch::channel(0).0);

/// Called by the gateway right after it published a relay client in `relay_client`.
pub(crate) fn note_relay_published() {
    RELAY_PUBLISHED.send_modify(|published| *published += 1);
}

/// Binds the lease client to every relay client published after the hub started, so the hub
/// is never unleased once a relay exists: a first relay starts the client, a replaced relay
/// rebinds it (the previous client is aborted). A client that binds on standby stands the
/// running generation down through `wanted`.
async fn follow_relay<C, R, B, Fut>(
    mut published: tokio::sync::watch::Receiver<u64>,
    mut relay: R,
    mut bind: B,
    mut bound: Option<tokio::task::JoinHandle<()>>,
    wanted: tokio::sync::watch::Sender<bool>,
) where
    C: Send,
    R: FnMut() -> Option<C> + Send + 'static,
    B: FnMut(C) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = (Arc<dyn LeaseGate>, tokio::task::JoinHandle<()>)> + Send,
{
    while published.changed().await.is_ok() {
        let Some(client) = relay() else { continue };
        if let Some(previous) = bound.take() {
            previous.abort();
        }
        let (gate, task) = bind(client).await;
        let standby = !gate.allows_hub();
        set_lease_gate(Some(gate));
        bound = Some(task);
        if standby {
            wanted.send_replace(false);
        }
        tracing::info!(standby, "supervisor lease client bound to the published relay");
    }
}

/// A person starts the stopped hub from another surface. `by` names who, as for a lane pause.
/// Like `ferryx hub start`, it never restarts the daemon, so live PTY sessions stay up.
pub async fn start_supervisor(state: &RemoteGatewayState, by: &str) -> Result<bool, HubStartError> {
    let (Some((_, running)), Some(launch)) = (supervisor_view(), supervisor_launch()) else {
        return Err(HubStartError::Unavailable);
    };
    let started = start_generation(&running, &launch).await?;
    tracing::info!(by, started, "supervisor hub start requested");
    crate::remote::server::agent_control_service::publish_supervisor_changed(
        state,
        json!({ "hub": "started" }),
    );
    Ok(started)
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
    /// The lane is a supervisor goal's: the shared supervisor service answered (FORBIDDEN from
    /// a pane, a revision conflict, SUPERVISOR_OWNED for an adopted lane, ...).
    #[error("{0}")]
    Supervisor(ferryx_hub::supervisor::request::RequestFailure),
}

/// What the transport knows about a lane-pause caller. `peer_refusal` is the daemon's pane
/// peer check; it applies only when the lane is a supervisor goal's, so a legacy lane pause
/// keeps its pre-existing semantics everywhere.
#[derive(Debug, Clone, Default)]
pub struct PauseGate {
    pub peer_refusal: Option<String>,
}

/// A person pauses or resumes one lane. `by` names who: `desktop`, or `remote:<deviceId>` for
/// a paired phone. Returns whether the call changed the lane (false when it already was so).
pub async fn set_lane_paused(
    state: &RemoteGatewayState,
    lane_key: &str,
    paused: bool,
    by: &str,
) -> Result<bool, LanePauseError> {
    set_lane_paused_gated(state, lane_key, paused, by, PauseGate::default()).await
}

/// [`set_lane_paused`] behind the transport's peer gate. A supervisor goal's lane is paused or
/// resumed through the shared supervisor service (revision bump, `paused` run state); every
/// other lane keeps the legacy lane pause.
pub async fn set_lane_paused_gated(
    state: &RemoteGatewayState,
    lane_key: &str,
    paused: bool,
    by: &str,
    gate: PauseGate,
) -> Result<bool, LanePauseError> {
    use ferryx_hub::lanes::state::LaneStatus;
    use ferryx_hub::supervisor::request::{self, RequestContext, RequestFailure};
    if let Ok((pool, owner, control)) = supervisor_service(state).await {
        let owned = ferryx_hub::supervisor::supervisor_owns_lane(&pool, lane_key)
            .await
            .map_err(|error| LanePauseError::Storage(error.to_string()))?;
        if owned {
            if let Some(message) = gate.peer_refusal {
                return Err(LanePauseError::Supervisor(RequestFailure::new(
                    "FORBIDDEN",
                    message,
                )));
            }
            let owner = owner.ok_or_else(|| {
                LanePauseError::Supervisor(RequestFailure::new(
                    "UNAUTHORIZED",
                    "this installation has no single supervisor owner",
                ))
            })?;
            let adapter = if by.starts_with("remote:") {
                ferryx_hub::supervisor::ChannelProvider::Phone
            } else {
                ferryx_hub::supervisor::ChannelProvider::Desktop
            };
            let context = RequestContext {
                principal: owner,
                adapter,
                channel_binding_id: None,
                control,
                memory: None,
            };
            match request::lane_pause(&pool, &context, lane_key, paused).await {
                Ok(Some(changed)) => {
                    crate::remote::server::agent_control_service::publish_supervisor_changed(
                        state,
                        json!({ "laneKey": lane_key, "by": by }),
                    );
                    return Ok(changed);
                }
                Ok(None) => {}
                Err(failure) => return Err(LanePauseError::Supervisor(failure)),
            }
        }
    }
    let Some((hub, _)) = supervisor_view() else {
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

/// Capability a daemon advertises once it serves `DaemonRequest::SupervisorGoal`.
pub const SUPERVISOR_GOAL_CAPABILITY: &str = "supervisorGoalV1";

/// Capability a daemon advertises once it serves `DaemonRequest::SupervisorEngine`.
pub const SUPERVISOR_ENGINE_CAPABILITY: &str = "supervisorEngineV1";

/// Why a supervisor engine action was refused: the engine's typed fault (`MHC_*`, including
/// `MHC_NOT_RUNNING` when this daemon owns no engine) or an unavailable judge.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorEngineError {
    #[error("{0}")]
    Fault(ferryx_hub::meta::mhc_fault::SupervisorFault),
    /// The hub database is not open yet, so an assessment could not be recorded.
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("{0}")]
    Judge(String),
}

impl SupervisorEngineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Fault(fault) => fault.code(),
            Self::Unavailable => "SUPERVISOR_UNAVAILABLE",
            Self::Judge(_) => "JUDGE_UNAVAILABLE",
        }
    }
}

/// Capability a daemon advertises once it serves `DaemonRequest::SupervisorRequestV1`.
pub const SUPERVISOR_REQUEST_CAPABILITY: &str = "supervisorRequestV1";

/// Which adapter a supervisor request reached the shared service through, recorded as the
/// request's source adapter. The principal is always the installation owner, derived by the
/// daemon: `Desktop` is the desktop app, `Cli` the owner's `ferryx-cli` and `Mcp` the
/// `ferryx-cli mcp` server forwarding for an MCP client (all over the owner socket, behind
/// its pane peer check); `Remote` a paired device the gateway authorized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorCaller {
    Desktop,
    Cli,
    Mcp,
    Remote { device_id: String },
}

impl SupervisorCaller {
    fn adapter(&self) -> ferryx_hub::supervisor::ChannelProvider {
        use ferryx_hub::supervisor::ChannelProvider;
        match self {
            Self::Desktop => ChannelProvider::Desktop,
            Self::Cli => ChannelProvider::Cli,
            Self::Mcp => ChannelProvider::Mcp,
            Self::Remote { .. } => ChannelProvider::Phone,
        }
    }
}

impl From<ferryx_hub::goals::GoalError> for SupervisorEngineError {
    fn from(error: ferryx_hub::goals::GoalError) -> Self {
        match error {
            ferryx_hub::goals::GoalError::SupervisorFault(fault) => Self::Fault(fault),
            other => Self::Judge(other.to_string()),
        }
    }
}

/// The engine side of [`supervisor_engine`]: the installed engine's snapshot, and for
/// `Assess` one structured judgment through the supervisor judge, which records its
/// `judge_calls` row in `pool` (an assessment without the hub pool is refused, never left
/// unrecorded). Only the installed mhc engine answers; with none installed the action is
/// `MHC_NOT_RUNNING` and no other model is asked. Nothing is written to goals or dispatches
/// and nothing is typed into a pane.
async fn engine_view(
    installed: Option<ferryx_hub::meta::mhc_supervisor::MhcSupervisor>,
    pool: Option<ferryx_hub::goals::SqlitePool>,
    control: Option<Arc<dyn AgentControl>>,
    action: crate::daemon::protocol::SupervisorEngineAction,
) -> Result<Value, SupervisorEngineError> {
    use crate::daemon::protocol::SupervisorEngineAction as Action;
    let Some(supervisor) = installed else {
        return Err(SupervisorEngineError::Fault(
            ferryx_hub::meta::mhc_fault::SupervisorFault::not_running(
                "no mhc supervisor engine is installed in this daemon",
            ),
        ));
    };
    let assessment = match action {
        Action::Status => None,
        Action::Assess { prompt } => {
            let pool = pool.ok_or(SupervisorEngineError::Unavailable)?;
            let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env())
                .with_fallback(Arc::new(supervisor.model()))
                .with_pool(pool);
            let reply = ferryx_hub::usage::with_cause(
                ferryx_hub::usage::CallCause::new("owner_probe"),
                judge.structured(ferryx_hub::judge::Purpose::GapAssess, &prompt),
            )
            .await;
            match reply {
                Ok(reply) => Some(json!({ "purpose": "gap_assess", "reply": reply })),
                Err(error) => {
                    let error = SupervisorEngineError::from(error);
                    tracing::warn!(code = error.code(), %error, "a supervisor engine assessment failed");
                    return Err(error);
                }
            }
        }
        Action::AssessGoal { goal_id } => {
            let pool = pool.ok_or(SupervisorEngineError::Unavailable)?;
            let control = control.ok_or(SupervisorEngineError::Unavailable)?;
            let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env())
                .with_fallback(Arc::new(supervisor.model()))
                .with_pool(pool.clone());
            Some(assess_goal(&pool, &judge, control.as_ref(), &goal_id).await?)
        }
    };
    let mut view = json!({ "engine": supervisor.engine_snapshot().await });
    if let (Some(assessment), Some(fields)) = (assessment, view.as_object_mut()) {
        fields.insert("assessment".into(), assessment);
    }
    Ok(view)
}

/// One goal's real gap assessment through `judge`: the prompt the goal loop builds (stable
/// instruction prefix, then the goal's contract, evidence and recent turns), attributed to the
/// goal (`owner_assess`). Reads only; the assessment is returned, never applied.
async fn assess_goal(
    pool: &ferryx_hub::goals::SqlitePool,
    judge: &ferryx_hub::judge::Judge,
    control: &dyn AgentControl,
    goal_id: &str,
) -> Result<Value, SupervisorEngineError> {
    use ferryx_hub::goals::{assess, contract, evidence, prompts, turns};
    let unknown = || SupervisorEngineError::Judge(format!("NOT_FOUND: goal {goal_id} has no lane contract"));
    let goal = ferryx_hub::supervisor::goal_of(pool, goal_id)
        .await
        .map_err(|error| SupervisorEngineError::Judge(error.to_string()))?
        .ok_or_else(unknown)?;
    let contract = contract::contract_of(pool, &goal.lane_key).await?.ok_or_else(unknown)?;
    let bundle = evidence::collect(pool, control, &goal.lane_key).await?;
    let recent = turns::turns_of(pool, &goal.lane_key).await?;
    let prompt = prompts::gap_assess_prompt(&contract, &bundle, &recent);
    let prefix = ferryx_hub::usage::split_prompt(&prompt).map(|(prefix, _)| prefix.to_owned());
    let cause = ferryx_hub::usage::CallCause::new("owner_assess").goal(goal_id, Some(goal.revision));
    let assessment = ferryx_hub::usage::with_cause(cause, assess::assess(judge, &contract, &bundle, &recent)).await?;
    Ok(json!({
        "purpose": "gap_assess",
        "goalId": goal_id,
        "goalRevision": goal.revision,
        "laneKey": goal.lane_key,
        "prefixSha256": prefix.as_deref().map(ferryx_hub::usage::sha256_hex),
        "promptBytes": prompt.len(),
        "assessment": assessment,
    }))
}

/// Reads the retained mhc supervisor engine or asks it for one structured judgment, and adds
/// the supervisor's recent pane input (`actions`, as in [`supervisor_snapshot`]) so a caller
/// can see that nothing was typed. `hubOpen` says whether the hub database is open, which an
/// assessment needs to record its `judge_calls` row. `supervisorIdentities` are the hub's
/// supervisor identity rows (read-only; null while the hub is not open or the read failed), so
/// the identity the engine serves can be compared across engine and daemon restarts.
pub async fn supervisor_engine(
    state: &RemoteGatewayState,
    action: crate::daemon::protocol::SupervisorEngineAction,
) -> Result<Value, SupervisorEngineError> {
    let pool = supervisor_view().map(|(hub, _)| hub.pool.clone());
    let control = supervisor_view().map(|(hub, _)| Arc::clone(&hub.control));
    let hub_open = pool.is_some();
    // engine_view refuses first (MHC_NOT_RUNNING) when no engine is installed, so the identity
    // rows are read only for a view that is returned.
    let mut view = engine_view(ferryx_hub::meta::mhc_supervisor::installed(), pool.clone(), control, action).await?;
    let identities = match &pool {
        Some(pool) => match ferryx_hub::supervisor::identities(pool).await {
            Ok(rows) => json!(rows),
            Err(error) => {
                tracing::warn!(%error, "supervisor engine view could not read the supervisor identities");
                Value::Null
            }
        },
        None => Value::Null,
    };
    let actions = supervisor_snapshot(state).await.get("actions").cloned().unwrap_or_else(|| json!([]));
    if let Some(fields) = view.as_object_mut() {
        fields.insert("actions".into(), actions);
        fields.insert("hubOpen".into(), json!(hub_open));
        fields.insert("supervisorIdentities".into(), identities);
    }
    Ok(view)
}

/// The hub pool and its owner principal for the shared supervisor service, or
/// SUPERVISOR_UNAVAILABLE (503) when this daemon has no open hub. The owner is the
/// installation's one supervisor identity: callers here are the owner's own processes or
/// devices the gateway already authorized, never a body-supplied actor.
async fn supervisor_service(
    state: &RemoteGatewayState,
) -> Result<
    (
        ferryx_hub::goals::SqlitePool,
        Option<String>,
        Option<Arc<dyn AgentControl>>,
    ),
    ferryx_hub::supervisor::request::RequestFailure,
> {
    use ferryx_hub::supervisor::request::RequestFailure;
    #[cfg(test)]
    let test_hub = state.supervisor_test_hub.read().clone();
    #[cfg(test)]
    if let Some((pool, owner)) = test_hub {
        return Ok((pool, Some(owner), None));
    }
    let _ = state;
    let Some((hub, _)) = supervisor_view() else {
        return Err(RequestFailure::new(
            "SUPERVISOR_UNAVAILABLE",
            "the supervisor hub is not open in this daemon",
        ));
    };
    let owner = ferryx_hub::supervisor::admission::installation_owner(&hub.pool)
        .await
        .ok();
    Ok((hub.pool.clone(), owner, Some(Arc::clone(&hub.control))))
}

/// Serves one supervisor request envelope through the hub's shared service, the same one MCP
/// `supervisor.request` uses. A mutation tells the Supervisor views to refetch.
pub async fn supervisor_request(
    state: &RemoteGatewayState,
    caller: SupervisorCaller,
    envelope: Value,
) -> Result<Value, ferryx_hub::supervisor::request::RequestFailure> {
    use ferryx_hub::supervisor::request::{self, RequestContext, RequestFailure};
    // A malformed envelope is refused before the hub is even consulted.
    request::parse_envelope(envelope.clone())?;
    let (pool, owner, control) = supervisor_service(state).await?;
    let owner = owner.ok_or_else(|| {
        RequestFailure::new(
            "UNAUTHORIZED",
            "this installation has no single supervisor owner",
        )
    })?;
    let context = RequestContext {
        principal: owner,
        adapter: caller.adapter(),
        channel_binding_id: None,
        control,
        memory: ferryx_hub::meta::mhc_supervisor::installed_memory(),
    };
    let answer = request::handle(&pool, &context, envelope).await?;
    let mut payload =
        json!({ "supervisorRequest": answer.get("action").cloned().unwrap_or(Value::Null) });
    if let (Some(fields), Some(goal)) = (payload.as_object_mut(), answer.get("goalId")) {
        fields.insert("goalId".into(), goal.clone());
    }
    if let SupervisorCaller::Remote { device_id } = &caller {
        if let Some(fields) = payload.as_object_mut() {
            fields.insert("by".into(), json!(format!("remote:{device_id}")));
        }
    }
    crate::remote::server::agent_control_service::publish_supervisor_changed(state, payload);
    Ok(answer)
}

/// The owner-message provenance of one project after a cursor.
pub async fn supervisor_conversation(
    state: &RemoteGatewayState,
    caller: SupervisorCaller,
    project_id: &str,
    after: i64,
) -> Result<Value, ferryx_hub::supervisor::request::RequestFailure> {
    use ferryx_hub::supervisor::request::{self, RequestContext, RequestFailure};
    let (pool, owner, _) = supervisor_service(state).await?;
    let owner = owner.ok_or_else(|| {
        RequestFailure::new(
            "UNAUTHORIZED",
            "this installation has no single supervisor owner",
        )
    })?;
    let context = RequestContext {
        principal: owner,
        adapter: caller.adapter(),
        channel_binding_id: None,
        control: None,
        memory: None,
    };
    request::conversation(&pool, &context, project_id, after).await
}

/// The owner's native memory receipts with their provenance, read from the installed mhc
/// engine. A closed hub is SUPERVISOR_UNAVAILABLE; a daemon without the engine is
/// MEMORY_UNAVAILABLE, never an empty success.
pub async fn supervisor_memory(
    state: &RemoteGatewayState,
    caller: SupervisorCaller,
) -> Result<Value, ferryx_hub::supervisor::request::RequestFailure> {
    use ferryx_hub::supervisor::request::{self, RequestContext, RequestFailure};
    let (pool, owner, _) = supervisor_service(state).await?;
    let owner = owner.ok_or_else(|| {
        RequestFailure::new(
            "UNAUTHORIZED",
            "this installation has no single supervisor owner",
        )
    })?;
    let context = RequestContext {
        principal: owner,
        adapter: caller.adapter(),
        channel_binding_id: None,
        control: None,
        memory: ferryx_hub::meta::mhc_supervisor::installed_memory(),
    };
    request::memory_receipts(&pool, &context).await
}

/// Capability a daemon advertises once it serves `DaemonRequest::SupervisorReadV1`.
pub const SUPERVISOR_READ_CAPABILITY: &str = "supervisorReadV1";

/// The desktop's read of the unified supervisor besides the snapshot: the same conversation
/// page and memory view the remote `GET /api/v1/supervisor/{conversation,memory}` routes serve.
pub async fn supervisor_read(
    state: &RemoteGatewayState,
    request: crate::daemon::protocol::SupervisorReadAction,
) -> Result<Value, ferryx_hub::supervisor::request::RequestFailure> {
    use crate::daemon::protocol::SupervisorReadAction;
    match request {
        SupervisorReadAction::Conversation { project_id, after } => {
            supervisor_conversation(state, SupervisorCaller::Desktop, &project_id, after.unwrap_or(0)).await
        }
        SupervisorReadAction::Memory => supervisor_memory(state, SupervisorCaller::Desktop).await,
    }
}

/// The unified supervisor's part of the snapshot: identity, engine health and every goal's
/// run state, wait reason, next trigger and revision. Additive to the legacy lane view.
async fn unified_snapshot(pool: &ferryx_hub::goals::SqlitePool) -> Value {
    let mut unified = match ferryx_hub::supervisor::request::snapshot(pool).await {
        Ok(value) => value,
        Err(error) => json!({ "identity": null, "goals": [], "error": error.to_json() }),
    };
    let health = ferryx_hub::meta::mhc_supervisor::installed()
        .map(|engine| serde_json::to_value(engine.health()).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);
    if let Some(fields) = unified.as_object_mut() {
        fields.insert("health".into(), health);
    }
    unified
}

/// Why a goal action was refused. Each maps to one wire code the CLI turns into an exit code.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorGoalError {
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("{0}")]
    NotFound(String),
    /// The state moved under the request: nothing awaits approval, a turn is in flight, the
    /// contract revision changed, or the revision was already decided.
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Storage(String),
}

impl SupervisorGoalError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "SUPERVISOR_UNAVAILABLE",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) => "REQUEST_CONFLICT",
            Self::Invalid(_) => "INVALID_REQUEST",
            Self::Storage(_) => "INTERNAL",
        }
    }
}

impl From<ferryx_hub::goals::GoalError> for SupervisorGoalError {
    fn from(error: ferryx_hub::goals::GoalError) -> Self {
        use ferryx_hub::goals::GoalError;
        let message = error.to_string();
        match error {
            GoalError::NoContract(_) | GoalError::UnknownRequirement { .. } => Self::NotFound(message),
            GoalError::TurnInFlight(_) | GoalError::RevisionChanged { .. } | GoalError::TurnLimit { .. } => {
                Self::Conflict(message)
            }
            GoalError::EmptyContract
            | GoalError::InvalidContract(_)
            | GoalError::Unknown { .. }
            | GoalError::EvidenceRequired { .. }
            | GoalError::QuoteNotFound
            | GoalError::InvalidDecision(_) => Self::Invalid(message),
            GoalError::JudgeUnavailable | GoalError::SupervisorFault(_) | GoalError::Sql(_) | GoalError::Hub(_) => {
                Self::Storage(message)
            }
        }
    }
}

/// A revision accept/reject the store refused as invalid means the revision is missing or
/// already decided: the request raced the state, not a malformed request.
fn revision_refusal(error: ferryx_hub::goals::GoalError) -> SupervisorGoalError {
    match error {
        ferryx_hub::goals::GoalError::InvalidContract(message) => SupervisorGoalError::Conflict(message),
        other => other.into(),
    }
}

/// Turn limit of a new contract when the caller names none (the storage default).
const GOAL_DEFAULT_TURN_LIMIT: u32 = 20;

/// Shows or steers one lane's goal contract and returns the supervisor snapshot afterwards,
/// with a `goal` entry `{laneKey, goalContract}` for the lane the action named (present even
/// when the lane has ended and so left the snapshot's lane list). Every change tells the
/// Supervisor views to refetch.
pub async fn supervisor_goal(
    state: &RemoteGatewayState,
    action: crate::daemon::protocol::SupervisorGoalAction,
) -> Result<Value, SupervisorGoalError> {
    let Some((hub, _)) = supervisor_view() else {
        return Err(SupervisorGoalError::Unavailable);
    };
    let mutates = action.mutates();
    let (lane_key, changed) = apply_goal_action(&hub.pool, hub.control.as_ref(), action).await?;
    if mutates {
        let mut payload = json!({ "laneKey": lane_key });
        if let (Some(payload), Value::Object(extra)) = (payload.as_object_mut(), changed) {
            payload.extend(extra);
        }
        crate::remote::server::agent_control_service::publish_supervisor_changed(state, payload);
    }
    let view = ferryx_hub::lanes::supervised::goal_view(&hub.pool, &lane_key)
        .await
        .map_err(|error| SupervisorGoalError::Storage(error.to_string()))?;
    let mut snapshot = supervisor_snapshot(state).await;
    if let Some(fields) = snapshot.as_object_mut() {
        fields.insert("goal".into(), json!({ "laneKey": lane_key, "goalContract": view }));
    }
    Ok(snapshot)
}

/// Capability a daemon advertises once it serves `DaemonRequest::SupervisorDispatch`.
pub const SUPERVISOR_DISPATCH_CAPABILITY: &str = "supervisorDispatchV1";

/// Why a supervisor dispatch request was refused: the admission's typed code (e.g.
/// `UNAUTHORIZED`, `SESSION_ALREADY_BOUND`, `REQUEST_CONFLICT`), or an unopened hub.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorDispatchError {
    #[error("the supervisor hub is not open in this daemon")]
    Unavailable,
    #[error("{message}")]
    Refused { code: &'static str, message: String },
}

impl SupervisorDispatchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "SUPERVISOR_UNAVAILABLE",
            Self::Refused { code, .. } => code,
        }
    }
}

impl From<ferryx_hub::supervisor::admission::AdmissionError> for SupervisorDispatchError {
    fn from(error: ferryx_hub::supervisor::admission::AdmissionError) -> Self {
        Self::Refused { code: error.code(), message: error.to_string() }
    }
}

impl From<ferryx_hub::supervisor::SupervisorError> for SupervisorDispatchError {
    fn from(error: ferryx_hub::supervisor::SupervisorError) -> Self {
        Self::Refused { code: error.code(), message: error.to_string() }
    }
}

/// Owner work bound to a chosen pane (`submit`), or one goal's dispatch status (`status`),
/// on the daemon's own hub. Only the local daemon socket carries this request, so the sender
/// is any same-uid local process; it is admitted as the installation owner, as MCP
/// `supervisor.request` is. `status` only reads.
pub async fn supervisor_dispatch(
    state: &RemoteGatewayState,
    action: crate::daemon::protocol::SupervisorDispatchAction,
) -> Result<Value, SupervisorDispatchError> {
    let Some((hub, _)) = supervisor_view() else {
        return Err(SupervisorDispatchError::Unavailable);
    };
    let result = apply_dispatch_action(&hub.pool, action).await?;
    if let Some(goal_id) = result.pointer("/receipt/goal_id") {
        crate::remote::server::agent_control_service::publish_supervisor_changed(
            state,
            json!({ "goalId": goal_id }),
        );
    }
    Ok(result)
}

/// The store side of [`supervisor_dispatch`].
async fn apply_dispatch_action(
    pool: &ferryx_hub::goals::SqlitePool,
    action: crate::daemon::protocol::SupervisorDispatchAction,
) -> Result<Value, SupervisorDispatchError> {
    use crate::daemon::protocol::SupervisorDispatchAction as Action;
    use ferryx_hub::supervisor::{self as sv, admission};
    match action {
        Action::Submit { message_id, body, host_id, session_id, work } => {
            let actor = admission::ActorContext {
                principal: admission::installation_owner(pool).await?,
                source_adapter: sv::ChannelProvider::Cli,
                source_message_id: message_id,
                source_body: body,
                channel_binding_id: None,
                choices: admission::OwnerChoices { host_id, session_id, ..Default::default() },
            };
            let outcome = admission::submit_work(pool, &actor, &work).await?;
            serde_json::to_value(&outcome).map_err(|error| SupervisorDispatchError::Refused {
                code: "INTERNAL",
                message: error.to_string(),
            })
        }
        Action::Status { goal_id } => {
            let goal = sv::goal_of(pool, &goal_id).await?.ok_or_else(|| SupervisorDispatchError::Refused {
                code: "NOT_FOUND",
                message: format!("goal not found: {goal_id}"),
            })?;
            let binding = sv::execution_binding_of(pool, &goal_id).await?;
            let dispatches = sv::dispatches_of(pool, &goal_id).await?;
            Ok(json!({
                "goal": {
                    "goalId": goal.goal_id,
                    "laneKey": goal.lane_key,
                    "runState": goal.run_state,
                    "stateDetail": goal.state_detail,
                    "revision": goal.revision,
                    "updatedAt": goal.updated_at,
                },
                "binding": binding.map(|binding| json!({
                    "kind": binding.kind,
                    "hostId": binding.host_id,
                    "providerSessionId": binding.provider_session_id,
                    "runId": binding.run_id,
                    "generation": binding.generation,
                })),
                "dispatches": dispatches.into_iter().map(|dispatch| json!({
                    "requestId": dispatch.request_id,
                    "ordinal": dispatch.ordinal,
                    "generation": dispatch.generation,
                    "state": dispatch.state,
                    "runId": dispatch.run_id,
                    "providerSessionId": dispatch.provider_session_id,
                    "uncertainCode": dispatch.uncertain_code,
                    "acceptedAt": dispatch.accepted_at,
                    "startedAt": dispatch.started_at,
                    "settledAt": dispatch.settled_at,
                    "uncertainAt": dispatch.uncertain_at,
                    "createdAt": dispatch.created_at,
                })).collect::<Vec<_>>(),
            }))
        }
    }
}

/// The store side of [`supervisor_goal`]: checks the lane, refuses a change to a lane the
/// supervisor owns, and applies the action. Returns the lane the action named and what the
/// action changed.
async fn apply_goal_action(
    pool: &ferryx_hub::goals::SqlitePool,
    control: &dyn AgentControl,
    action: crate::daemon::protocol::SupervisorGoalAction,
) -> Result<(String, Value), SupervisorGoalError> {
    use crate::daemon::protocol::SupervisorGoalAction as Action;
    use ferryx_hub::goals::{contract, revision, GoalMode, ProposalState};
    // A revision names its lane only through the store; it is fenced like the lane actions,
    // so a legacy revision proposed before an adoption is not applied after it.
    let revision_lane = match &action {
        Action::AcceptRevision { id } | Action::RejectRevision { id } => {
            revision::lane_of(pool, *id).await?
        }
        _ => None,
    };
    if let Some(lane_key) = action.lane_key().or(revision_lane.as_deref()) {
        if ferryx_hub::lanes::state::status_of(pool, lane_key)
            .await
            .map_err(|error| SupervisorGoalError::Storage(error.to_string()))?
            .is_none()
        {
            return Err(SupervisorGoalError::NotFound(format!("lane not found: {lane_key}")));
        }
        // A supervisor goal's lane changes only through the supervisor (which bumps the goal
        // revision), like `lane.drive`: the legacy set, mode, approve, discard and revision
        // paths are refused with SUPERVISOR_OWNED.
        ferryx_hub::supervisor::refuse_legacy_goal_action(pool, lane_key, action.mutates())
            .await
            .map_err(|error| match error {
                ferryx_hub::supervisor::SupervisorError::Sql(error) => SupervisorGoalError::Storage(error.to_string()),
                refused => SupervisorGoalError::Conflict(refused.to_string()),
            })?;
    }
    let mut changed = json!({});
    let lane_key: String = match action {
        Action::Show { lane_key } => lane_key,
        Action::Set { lane_key, requirements, mode, turn_limit } => {
            let existing = contract::contract_of(pool, &lane_key).await?;
            let mode = mode
                .or(existing.as_ref().map(|contract| contract.mode))
                .unwrap_or(GoalMode::Supervised);
            let turn_limit = turn_limit
                .or(existing.as_ref().map(|contract| contract.turn_limit))
                .unwrap_or(GOAL_DEFAULT_TURN_LIMIT);
            contract::set_contract(pool, &lane_key, &requirements, mode, turn_limit).await?;
            lane_key
        }
        Action::Mode { lane_key, mode } => {
            contract::set_mode(pool, &lane_key, mode).await?;
            lane_key
        }
        Action::Approve { lane_key } => {
            let awaiting = contract::contract_of(pool, &lane_key)
                .await?
                .ok_or_else(|| SupervisorGoalError::NotFound(format!("lane {lane_key} has no goal contract")))?
                .proposal
                .is_some_and(|proposal| proposal.state == ProposalState::AwaitingApproval);
            if !awaiting {
                return Err(SupervisorGoalError::Conflict(format!(
                    "lane {lane_key} has no proposal awaiting approval"
                )));
            }
            let outcome = ferryx_hub::goals::r#loop::approve_proposal(pool, control, &lane_key)
                .await
                // The proposal moved between the check and the send (a person spoke, a pass ran).
                .map_err(|error| match error {
                    ferryx_hub::goals::GoalError::InvalidContract(message) => SupervisorGoalError::Conflict(message),
                    other => other.into(),
                })?;
            changed = json!({ "stopReason": outcome.stop_reason, "sent": outcome.sent });
            lane_key
        }
        Action::Discard { lane_key } => {
            if contract::contract_of(pool, &lane_key).await?.is_none() {
                return Err(SupervisorGoalError::NotFound(format!("lane {lane_key} has no goal contract")));
            }
            let moved = contract::set_proposal_state(
                pool,
                &lane_key,
                ProposalState::Discarded,
                Some(&[ProposalState::AwaitingApproval]),
            )
            .await?;
            if !moved {
                return Err(SupervisorGoalError::Conflict(format!(
                    "lane {lane_key} has no proposal awaiting approval"
                )));
            }
            lane_key
        }
        Action::AcceptRevision { id } => revision::accept(pool, id).await.map_err(revision_refusal)?.lane_key,
        Action::RejectRevision { id } => {
            // Read before the reject, which returns nothing.
            let lane_key = revision::lane_of(pool, id).await?;
            revision::reject(pool, id).await.map_err(revision_refusal)?;
            lane_key.ok_or_else(|| SupervisorGoalError::Conflict(format!("no revision {id}")))?
        }
    };
    Ok((lane_key, changed))
}

/// Handover commit: this daemon keeps serving retained sessions but no longer owns the hub.
/// Slack, the cron scheduler and sockets, `:9119`, the watchers and the control listener end
/// here, before the successor's hub starts, so there is never a second Socket Mode connection
/// or a second scheduler. `hub_stopped` is left as it is: the successor reads it to decide
/// whether to open the hub.
pub fn retire_for_handover() {
    // The lease client is aborted with the other hub tasks, without a release: the handover
    // (plan todo 9) freezes the lease and hands it to the successor on commit.
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
    let bridge = Arc::new(DaemonBridge {
        client: DaemonClient::new(),
        gateway: Some(Arc::clone(server.remote_state())),
    });
    let hub = Hub::open(config.clone(), bridge.clone()).await.map_err(|error| error.to_string())?;
    // Stored replies and the supervisor pane are side effects of a running hub: they happen in
    // `open_generation`, on the lease holder only.
    if let Err(error) = ferryx_hub::lanes::kibitzer::ensure_schema(&hub.pool).await {
        tracing::warn!(%error, "kibitzer schema was not created");
    }
    // Fail closed: a fresh or unreadable hub database does not open Slack next to omo.
    let forced_on = std::env::var("FERRYX_HUB").ok().as_deref() == Some("on");
    let persisted_stop = ferryx_hub::control::is_stopped(&hub.pool).await.unwrap_or(true);
    let stopped = !forced_on && persisted_stop;
    // This daemon owns the hub from here; a later handover commit ends it again.
    server.handover_manager.on_commit(retire_for_handover);
    // The supervisor lease decides which of the owner's machines runs the hub. Without a relay
    // client the hub runs unleased until one is published (see `follow_relay`).
    let (wanted, lease_decisions) = tokio::sync::watch::channel(false);
    // Subscribed before the relay is read, so a relay published from here on binds the client.
    let relay_published = RELAY_PUBLISHED.subscribe();
    let remote = Arc::clone(server.remote_state());
    let relay = remote.relay_client.read().clone();
    let (lease, lease_task) = match relay {
        Some(relay) => {
            let (gate, task) = spawn_lease_client(relay, wanted.clone(), Arc::clone(&server.handover_manager)).await;
            HUB_TASKS.track(&task);
            (Some(gate), Some(task))
        }
        None => (None, None),
    };
    set_lease_gate(lease.clone());
    let standby = lease.as_ref().is_some_and(|lease| !lease.allows_hub());
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
        effects: Arc::new(DaemonEffects),
    };
    let running = boot_hub(&launch, stopped, lease.as_deref()).await;
    let follower = tokio::spawn(follow_lease(lease_decisions, Arc::clone(&running), promote_generation));
    HUB_TASKS.track(&follower);
    let bind_wanted = wanted.clone();
    let bind_handover = Arc::clone(&launch.server.handover_manager);
    let relays = tokio::spawn(follow_relay(
        relay_published,
        move || remote.relay_client.read().clone(),
        move |relay: crate::remote::relay_client::RelayClient| {
            let wanted = bind_wanted.clone();
            let handover_manager = Arc::clone(&bind_handover);
            async move {
                let (gate, task) = spawn_lease_client(relay, wanted, handover_manager).await;
                HUB_TASKS.track(&task);
                (gate, task)
            }
        },
        lease_task,
        wanted,
    ));
    HUB_TASKS.track(&relays);
    serve_control(running, launch);
    if standby {
        tracing::info!("ferryx hub is on standby: another machine holds the supervisor lease");
    } else if stopped {
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
    /// Stored replies, the supervisor pane and the generation's tasks.
    effects: Arc<dyn GenerationEffects>,
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

#[cfg(unix)]
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

/// The supervisor's dispatcher for this hub generation. It drains committed ready intents at
/// start (the startup drain), then on every in-process admission wake and on a bounded rescan
/// that picks up admissions committed by `ferryx-cli mcp`. Only this daemon transmits: the
/// MCP process has no observer and only admits. `idle` is the observer loop's ledger of idle
/// readings, read after each committed start.
fn spawn_supervisor_dispatcher(
    launch: &HubLaunch,
    idle: Arc<ferryx_hub::supervisor::receipts::IdleLedger>,
) -> tokio::task::JoinHandle<()> {
    let dispatcher = ferryx_hub::supervisor::dispatcher::Dispatcher::new(
        launch.hub.pool.clone(),
        launch.bridge.clone(),
        launch.bridge.clone(),
        ferryx_hub::agent_control::LOCAL_HOST_ID,
    )
    .with_idle_ledger(idle)
    // Each generation is one holder term: its fence reaches every daemon before a dispatch.
    .with_fence_gate(crate::agent_control::fence::dispatcher_fence_gate(
        launch.bridge.client.clone(),
        launch.bridge.gateway.clone(),
    ));
    tokio::spawn(dispatcher.run(ferryx_hub::supervisor::dispatcher::dispatch_wake()))
}

/// Settles the supervisor PTY run bound to a local pane from an idle reading of it. Noted in
/// `idle` first: a run whose start commits after this reading finds it there.
fn settle_pty_run(
    pool: &ferryx_hub::goals::SqlitePool,
    idle: &ferryx_hub::supervisor::receipts::IdleLedger,
    instance: &str,
    observation: &crate::agent_control::events::AgentObservation,
) {
    idle.note_idle(&observation.session_id, instance, observation.sequence);
    let pool = pool.clone();
    let session_id = observation.session_id.clone();
    let instance = instance.to_owned();
    let sequence = observation.sequence;
    tokio::spawn(async move {
        use ferryx_hub::supervisor::receipts::{record_pty_settle_across_restart, PtySettle};
        // A run that started under an earlier daemon is ended by an idle reading of this one.
        match record_pty_settle_across_restart(
            &pool,
            ferryx_hub::agent_control::LOCAL_HOST_ID,
            &session_id,
            &instance,
            sequence,
        )
        .await
        {
            Ok(PtySettle::Settled(dispatch))
                if !dispatch
                    .run_id
                    .as_deref()
                    .is_some_and(|run| run.contains(&format!(":{instance}:"))) =>
            {
                tracing::info!(request_id = %dispatch.request_id, "a supervisor run started under an earlier daemon was settled by this daemon's idle reading");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, session_id = %session_id, "a supervisor run settle was not recorded");
            }
        }
    });
}

/// The supervisor's wake scheduler for this hub generation: dependency, settle, deadline,
/// owner and report wakes, decided by durable state checks before any judgment, so no model
/// runs while a goal waits. It recovers pending wakes and the state-event cursor at start.
fn spawn_supervisor_wakes(launch: &HubLaunch) -> tokio::task::JoinHandle<()> {
    let pool = launch.hub.pool.clone();
    // Settled runs are verified from their durable receipts: the deterministic state moves,
    // then the contract's assessment and blind convergence check on fresh evidence.
    // The pool is given explicitly: alerts (and with them the judge's default pool) are
    // installed later in this generation, and without a pool the configured supervisor
    // budget is not enforced and no judge_calls rows are written.
    let judge = std::sync::Arc::new(
        ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env())
            .with_pool(pool.clone()),
    );
    let driver = ferryx_hub::supervisor::completion::CompletionDriver::new(
        pool.clone(),
        judge,
        launch.bridge.clone(),
        ferryx_hub::agent_control::LOCAL_HOST_ID,
    );
    let handler = std::sync::Arc::new(ferryx_hub::supervisor::completion::CompletionHandler::new(
        pool.clone(),
        driver,
    ));
    let scheduler = ferryx_hub::supervisor::wakes::WakeScheduler::new(pool, handler);
    tokio::spawn(scheduler.run(ferryx_hub::supervisor::wakes::wake_signal()))
}

/// Forwards the store's goal-change announcements ([`ferryx_hub::supervisor::changes`]) to the
/// desktop and phone Supervisor views as `supervisor_changed {goalId}`, so a goal the
/// dispatcher, a worker receipt or the wake scheduler moved is refetched without an owner
/// action. A lagged forwarder sends one `{lagged: true}` change: the views refetch everything.
fn spawn_supervisor_change_forwarder(launch: &HubLaunch) -> tokio::task::JoinHandle<()> {
    use tokio::sync::broadcast::error::RecvError;
    let state = Arc::clone(launch.server.remote_state());
    let mut changes = ferryx_hub::supervisor::changes::subscribe();
    tokio::spawn(async move {
        loop {
            let payload = match changes.recv().await {
                Ok(goal_id) => json!({ "goalId": goal_id }),
                Err(RecvError::Lagged(_)) => json!({ "lagged": true }),
                Err(RecvError::Closed) => break,
            };
            crate::remote::server::agent_control_service::publish_supervisor_changed(&state, payload);
        }
    })
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
    let watch_bus = Arc::clone(&launch.server.remote_state().agent_events);
    let instance = watch_bus.instance().to_owned();
    let idle = Arc::new(ferryx_hub::supervisor::receipts::IdleLedger::default());
    let watch_idle = Arc::clone(&idle);
    let watch_hub = Arc::clone(&launch.hub);
    let watch_bridge = launch.bridge.clone();
    let watch_server = Arc::clone(&launch.server);
    // The omo session poller reports omo panes (by omo session id, on every host), so the pane
    // observer leaves them to it instead of sending a second notice under the pane id.
    let omo_polled = ferryx_hub::sessions::omo_poll::enabled();
    tasks.push(tokio::spawn(ferryx_hub::sessions::omo_poll::run(Arc::clone(&launch.hub))));
    tasks.push(tokio::spawn(async move {
        let mut agents = std::collections::HashMap::new();
        let mut runs = std::collections::HashMap::new();
        let mut worked = std::collections::HashSet::new();
        // Subscribed above, before this sweep: an idle reading that ended a supervisor run
        // while no hub generation was listening (a hub stop and start under a running daemon)
        // still settles it. A reading published after the subscription is also seen by the
        // loop; a second settle of the same run is a no-op.
        for latest in watch_bus.snapshot() {
            if matches!(latest.state, AgentControlState::Idle) && latest.prompt_id.is_none() {
                settle_pty_run(&watch_hub.pool, &watch_idle, &instance, &latest);
            }
        }
        loop {
            match events.recv().await {
                Ok(observation) => {
                    let idle = matches!(observation.state, AgentControlState::Idle);
                    if idle && observation.prompt_id.is_none() {
                        // The idle reading after a supervisor run's start reading settles it.
                        settle_pty_run(&watch_hub.pool, &watch_idle, &instance, &observation);
                    }
                    if matches!(observation.state, AgentControlState::Working | AgentControlState::Blocked) {
                        worked.insert(observation.session_id.clone());
                    } else if idle && observation.prompt_id.is_none() && worked.remove(&observation.session_id) {
                        settle_goal_turn(
                            Arc::clone(&watch_server),
                            Arc::clone(&watch_hub),
                            watch_bridge.clone(),
                            observation.session_id.clone(),
                        );
                    }
                    let run = state_run(&mut runs, &observation);
                    if let Err(error) = on_agent_event(&watch_hub, watch_bridge.as_ref(), &mut agents, omo_polled, run, observation).await {
                        tracing::debug!(%error, "hub observer event was not applied");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Readings were dropped, maybe an idle one that ends a supervisor run:
                    // settle against the latest reading of every pane instead.
                    for latest in watch_bus.snapshot() {
                        if matches!(latest.state, AgentControlState::Idle) && latest.prompt_id.is_none() {
                            settle_pty_run(&watch_hub.pool, &watch_idle, &instance, &latest);
                        }
                    }
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }));
    tasks.push(spawn_supervisor_dispatcher(launch, idle));
    tasks.push(spawn_supervisor_change_forwarder(launch));
    tasks.push(spawn_supervisor_wakes(launch));
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
    // Each Slack turn owns a short-lived mhc stdio RPC child. There is no shared
    // app-server, token file, readiness probe, or supervisor restart loop.
    #[cfg(feature = "hub-slack")]
    if !launch.config.supervisor_pane
        && !matches!(
            ferryx_hub::hub_env::var("FERRYX_HUB_SLACK_TURNS").ok().as_deref().map(str::trim),
            Some("0" | "off" | "false" | "no")
        )
    {
        let owner = launch.config.owner_slack_id.clone();
        let binary = ferryx_hub::meta::supervisor::mhc_bin();
        let config = ferryx_hub::chat::slack_turn::SlackTurnConfig::from_env(&binary, owner.as_deref());
        tracing::info!(
            binary = %binary,
            workspace = config.omo.workspace.is_some(),
            idle_secs = config.omo.idle_timeout.as_secs(),
            total_secs = config.omo.total_timeout.as_secs(),
            "slack turn path is armed (mhc RPC)"
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
    // The retained mhc supervisor engine: one owned `mhc --mode rpc --multi-session` stdio host
    // in `<data>/mhc-supervisor` with role-scoped sessions. Off unless FERRYX_HUB_MHC_SUPERVISOR
    // in the hub env file turns it on. Hub generations share the one process-wide instance;
    // each generation supervises it (launch, owned-exit watch, 5s..300s relaunch backoff).
    // Aborting the generation's tasks (hub stop, handover retirement) closes the engine's stdin
    // and kills it after a short grace; the next generation's supervise call re-enables it.
    let mhc_supervisor = ferryx_hub::meta::mhc_profile::MhcProfileConfig::from_env().map(|config| {
        ferryx_hub::meta::mhc_supervisor::install_with(|| {
            let data_dir = ferryx_hub::meta::supervisor::state_dir_from_home()
                .unwrap_or_else(|| std::path::PathBuf::from(".ferryx/hub"));
            ferryx_hub::meta::mhc_supervisor::MhcSupervisor::new(
                ferryx_hub::meta::mhc_profile::MhcProfile::new(&data_dir, config),
                std::sync::Arc::new(ferryx_hub::meta::mhc_supervisor::ProcessLauncher::new()),
            )
        })
    });
    if let Some(runtime) = &mhc_supervisor {
        tracing::info!(cwd = %runtime.profile().cwd().display(), "mhc supervisor engine is configured");
        // Every engine call (decision, convergence, conversation, ingest) is recorded with its
        // cause and usage in this hub's `supervisor_model_calls`.
        runtime.set_usage_sink(launch.hub.pool.clone()).await;
        // Slack owner conversations call the typed work tools; admission acts only for the
        // authenticated Slack turn in flight for the calling scope.
        #[cfg(feature = "hub-slack")]
        runtime.set_work_handler(std::sync::Arc::new(
            ferryx_hub::chat::slack_supervisor::SlackWorkTools::new(launch.hub.pool.clone()),
        ));
        tasks.push(tokio::spawn(runtime.clone().supervise()));
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
    // The kibitzer wakes the owner DM lead on its own clock; evaluate() enforces arming,
    // spacing, the daily cap and quiet hours, so a tick is cheap when nothing is due.
    {
        let pool = launch.hub.pool.clone();
        let (owner, fallback) = (lead_owner.clone(), lead_fallback.clone());
        let binary = ferryx_hub::meta::supervisor::mhc_bin();
        tasks.push(tokio::spawn(async move {
            let lead = ferryx_hub::lanes::lead::resolve(&pool, &owner, &fallback).await;
            if let Err(error) = ferryx_hub::lanes::kibitzer::ensure_lead(&pool, &lead).await {
                tracing::warn!(%error, %lead, "kibitzer lead was not registered");
            }
            let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env());
            // With the mhc supervisor configured it is the only model; there is no fallback.
            let model: std::sync::Arc<dyn ferryx_hub::lanes::kibitzer::ModelTurn> = match mhc_supervisor {
                Some(runtime) => std::sync::Arc::new(runtime.model()),
                None => std::sync::Arc::new(ferryx_hub::meta::backend::RpcModel::new(binary)),
            };
            loop {
                match ferryx_hub::lanes::kibitzer::evaluate(&pool, &lead, &judge, model.as_ref()).await {
                    Ok(true) => tracing::info!(%lead, "kibitzer woke the owner lead"),
                    Ok(false) => {}
                    Err(error) => tracing::warn!(%error, %lead, "kibitzer evaluation failed"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(5 * 60)).await;
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
    // omo's cron triage and auto-fix: queued cron problems go to the remediator. A job that
    // needs the owner is told in the owner DM; a verified fix is reported in the home channel.
    tasks.push(tokio::spawn(ferryx_hub::cron_remediate::serve(
        launch.hub.pool.clone(),
        Some(launch.hub.owner_session().storage_key()),
        Some(launch.hub.home_session().storage_key()),
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
                // A promotion may have reopened the hub on an imported database since bind.
                let launch = supervisor_launch().unwrap_or_else(|| launch.clone());
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
                                for task in &generation.tasks {
                                    task.abort();
                                }
                                // Wait for the aborted tasks to end, so a later `hub start`
                                // reconciles only after the predecessor dispatcher stopped.
                                for task in generation.tasks {
                                    let _ = task.await;
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
                        Ok(ferryx_hub::control::ControlOp::Start) => match start_generation(&running, &launch).await {
                            Ok(_) => "{\"status\":\"ok\",\"op\":\"start\"}\n",
                            Err(HubStartError::HandedOver) => {
                                "{\"status\":\"failed\",\"error\":\"this daemon handed the hub over to its successor\"}\n"
                            }
                            Err(error @ HubStartError::Standby { .. }) => {
                                let mut stream = reader.into_inner();
                                let _ = stream.write_all(format!("{}\n", standby_reply(&error)).as_bytes()).await;
                                return;
                            }
                            Err(_) => "{\"status\":\"failed\",\"error\":\"hub start was not persisted\"}\n",
                        },
                        Ok(op @ (ferryx_hub::control::ControlOp::Cutover
                        | ferryx_hub::control::ControlOp::Rollback
                        | ferryx_hub::control::ControlOp::Intake)) => {
                            use ferryx_hub::control::ControlOp;
                            use ferryx_hub::supervisor::cutover;
                            let pool = &launch.hub.pool;
                            // The installed engine's health is the only capability evidence;
                            // without an engine a cutover is refused (MHC_NOT_RUNNING).
                            let health = ferryx_hub::meta::mhc_supervisor::installed().map(|engine| engine.health());
                            let (name, result) = match op {
                                ControlOp::Cutover => ("cutover", cutover::cutover(pool, health.as_ref()).await),
                                ControlOp::Rollback => ("rollback", cutover::rollback(pool).await),
                                _ => (
                                    "intake",
                                    cutover::installation_intake(pool)
                                        .await
                                        .map(|intake| cutover::Transition { intake, steps: Vec::new() })
                                        .map_err(ferryx_hub::supervisor::SupervisorError::from),
                                ),
                            };
                            let mut reply = ferryx_hub::control::intake_reply(name, result);
                            if let (Some(fields), Some(health)) = (reply.as_object_mut(), health) {
                                fields.insert("engine".into(), serde_json::to_value(health).unwrap_or(Value::Null));
                            }
                            if name != "intake" && reply["status"] == "ok" {
                                crate::remote::server::agent_control_service::publish_supervisor_changed(
                                    launch.server.remote_state(),
                                    json!({ "intake": reply["intake"].clone() }),
                                );
                            }
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(format!("{reply}\n").as_bytes()).await;
                            return;
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
                        Ok(ferryx_hub::control::ControlOp::ApproveRerun(problem)) => {
                            let reply = ferryx_hub::control::approve_rerun_reply(problem).await;
                            let mut stream = reader.into_inner();
                            let _ = stream.write_all(reply.as_bytes()).await;
                            return;
                        }
                        Err(_) => "{\"status\":\"failed\",\"error\":\"expected stop, start, status, approve_rerun, cutover, rollback, or intake\"}\n",
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

/// A lane pane went Idle after a Working run: one turn settled. Lanes with a goal contract get
/// one supervisor goal-loop pass, off the event loop because a confirmed send waits on the
/// observer. The desktop refetches when the pass changed the lane's goal state.
fn settle_goal_turn(server: Arc<DaemonServer>, hub: Arc<Hub>, bridge: Arc<DaemonBridge>, session_id: String) {
    tokio::spawn(async move {
        let lane_key = match ferryx_hub::goals::turns::lane_of_session(&hub.pool, &session_id).await {
            Ok(Some(lane_key)) => lane_key,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%error, session_id = %session_id, "the settled pane's lane was not found");
                return;
            }
        };
        let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env());
        match ferryx_hub::goals::r#loop::on_turn_settled(&hub.pool, &judge, bridge.as_ref(), &lane_key, true).await {
            Ok(Some(outcome)) => server.remote_state().emit_desktop_event(
                crate::remote::server::agent_control_service::SUPERVISOR_CHANGED_EVENT,
                json!({
                    "laneKey": outcome.lane_key,
                    "stopReason": outcome.stop_reason,
                    "awaitingApproval": outcome.awaiting_approval,
                }),
            ),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, lane = %lane_key, "the supervisor goal loop did not run"),
        }
    });
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

/// Turns one pane observation into owner notices. The hub never types into the pane here: a
/// question or approval waits for the owner, who answers from the notice card or the terminal.
async fn on_agent_event(
    hub: &Arc<Hub>,
    control: &DaemonBridge,
    agents: &mut std::collections::HashMap<String, String>,
    omo_polled: bool,
    run: u64,
    observation: crate::agent_control::events::AgentObservation,
) -> Result<(), HubError> {
    let session_id = observation.session_id.clone();
    let mut question = None;
    let mut approval = None;
    let mut name = None;
    let wants_owner = observation.prompt_id.is_some() || matches!(observation.state, AgentControlState::Blocked);
    if wants_owner {
        if let Ok(value) = control.execute(None, AgentOp::Read { session_id: session_id.clone() }).await {
            if let Some(agent) = value.pointer("/session/agent").and_then(Value::as_str) {
                agents.insert(session_id.clone(), agent.to_owned());
            }
            name = value
                .pointer("/session/cwd")
                .and_then(Value::as_str)
                .and_then(|cwd| std::path::Path::new(cwd).file_name())
                .map(|base| base.to_string_lossy().into_owned());
            if observation.prompt_id.is_some() {
                question = value.pointer("/session/pending/question").and_then(Value::as_str).map(str::to_owned);
                approval = approval_card(&session_id, &value);
            }
        }
    }
    if omo_polled && agents.get(&session_id).map(String::as_str) == Some("omo") {
        return Ok(());
    }
    let target = format!("local:{session_id}");
    let watched = ferryx_hub::sessions::watch::Watcher::is_watched(&hub.pool, &target).await?;
    let shown = ferryx_hub::sessions::watch::short_id(&session_id).to_owned();
    let obs = Observation {
        target: target.clone(),
        machine: "local".into(),
        session_id: session_id.clone(),
        name: name.map_or(shown.clone(), |base| format!("{base} {shown}")),
        message_count: run,
        streaming: matches!(observation.state, AgentControlState::Working),
        blocking_question: matches!(observation.state, AgentControlState::Blocked) && question.is_none(),
        question,
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
    Ok(())
}

/// Forwards every writing `supervisor.request` (submit, adopt, a message with work, goal
/// control, memory) from `ferryx-cli mcp` to the daemon's `SupervisorRequestV1`, where the
/// peer check sees this MCP process: run from inside a Ferryx pane it is refused `FORBIDDEN`,
/// exactly like the CLI. Bounded by the CLI's goal timeout; a daemon that does not answer in
/// time is `SUPERVISOR_UNAVAILABLE` (the request may still commit; a replay with the same
/// `requestId` returns its result).
struct DaemonOwnerRoute;

#[async_trait]
impl ferryx_hub::mcp::OwnerRoute for DaemonOwnerRoute {
    async fn forward(
        &self,
        envelope: Value,
    ) -> Result<Value, ferryx_hub::supervisor::request::RequestFailure> {
        use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
        use ferryx_hub::supervisor::request::RequestFailure;
        let request = DaemonRequest::SupervisorRequestV1 {
            envelope,
            cli: true,
            mcp: true,
        };
        let exchange = crate::cli::supervisor_goal::exchange_once(
            SUPERVISOR_REQUEST_CAPABILITY,
            request,
            true,
        );
        let Ok(answer) =
            tokio::time::timeout(crate::cli::supervisor_goal::GOAL_CLI_TIMEOUT, exchange).await
        else {
            return Err(RequestFailure::new(
                "SUPERVISOR_UNAVAILABLE",
                "the daemon did not answer the supervisor request in time",
            ));
        };
        match answer {
            Ok(DaemonResponse::SupervisorRequestV1Ok { receipt }) => Ok(receipt),
            Ok(DaemonResponse::Error { code, message, .. }) => Err(RequestFailure::new(
                code.as_deref().unwrap_or("INTERNAL"),
                message,
            )),
            Ok(other) => Err(RequestFailure::new(
                "INTERNAL",
                format!("unexpected daemon response: {other:?}"),
            )),
            Err((value, _)) => Err(RequestFailure::new(
                value["code"].as_str().unwrap_or("SUPERVISOR_UNAVAILABLE"),
                value["message"]
                    .as_str()
                    .unwrap_or("the daemon could not be reached")
                    .to_owned(),
            )),
        }
    }
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
        let bridge = Arc::new(DaemonBridge { client: DaemonClient::new(), gateway: None });
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
            owner_route: Some(Arc::new(DaemonOwnerRoute)),
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
    fn each_hub_pane_launch_gets_its_own_spawn_request_id() {
        let first = hub_spawn_request_id();
        let second = hub_spawn_request_id();
        assert_ne!(
            first, second,
            "a reused id replays the earlier spawn's pane"
        );
        let prefix = format!("hub-{}-", std::process::id());
        assert!(
            first.starts_with(&prefix) && second.starts_with(&prefix),
            "{first} {second}"
        );
    }

    #[test]
    fn a_persons_relayed_words_are_not_supervisor_input() {
        assert!(relayed_from_a_person(Some(
            "slack-approval:local:pane-1:rev-42:prompt-9"
        )));
        assert!(relayed_from_a_person(Some("chat-lane:ledger-1700")));
        assert!(relayed_from_a_person(Some("lane-ledger-owner-msg7")));
        assert!(!relayed_from_a_person(Some("hub-auto-prompt-3")));
        assert!(!relayed_from_a_person(Some("lane-ledger-drive-2")));
        assert!(!relayed_from_a_person(None));
    }

    #[test]
    fn approve_rerun_needs_a_numeric_problem_id() {
        assert_eq!(hub_cron_cli(&["approve-rerun".into(), "abc".into()]), 2);
        assert_eq!(hub_cron_cli(&["approve-rerun".into()]), 2);
    }

    #[test]
    fn goal_refusals_map_to_the_cli_exit_codes() {
        use crate::agent_control::{exit_code_for, EXIT_INVALID, EXIT_NOT_FOUND, EXIT_REFUSED};
        use ferryx_hub::goals::GoalError;
        let conflict = SupervisorGoalError::from(GoalError::TurnInFlight("ledger".into()));
        assert_eq!(conflict.code(), "REQUEST_CONFLICT");
        assert_eq!(exit_code_for(conflict.code()), EXIT_REFUSED);
        let missing = SupervisorGoalError::from(GoalError::NoContract("ledger".into()));
        assert_eq!(exit_code_for(missing.code()), EXIT_NOT_FOUND);
        let invalid = SupervisorGoalError::from(GoalError::EmptyContract);
        assert_eq!(exit_code_for(invalid.code()), EXIT_INVALID);
        let decided = revision_refusal(GoalError::InvalidContract("revision 3 is already accepted".into()));
        assert_eq!(decided.code(), "REQUEST_CONFLICT");
        assert_eq!(SupervisorGoalError::Unavailable.code(), "SUPERVISOR_UNAVAILABLE");
    }

    /// The desktop goal actions reach the SUPERVISOR_OWNED fence: on a supervisor goal's lane a
    /// change is refused before anything is written, and show still answers.
    #[tokio::test]
    async fn a_desktop_goal_action_on_a_supervisor_lane_is_refused() {
        use crate::daemon::protocol::SupervisorGoalAction as Action;
        use ferryx_hub::goals::contract::{contract_of, RequirementSpec};
        use ferryx_hub::goals::GoalMode;
        use ferryx_hub::supervisor::{self as sv, AdmitRequest, ChannelProvider, ExecutionKind, ExecutionTarget};
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let owner = "slack:T1:U-owner";
        let (identity, project, _) =
            sv::provision_owner(&pool, owner, "ferryx", "github.com/example/ferryx", "host-a", "/repo/.git")
                .await
                .unwrap();
        let request = AdmitRequest {
            supervisor_id: identity.supervisor_id,
            owner_principal: owner.into(),
            source_adapter: ChannelProvider::Desktop,
            source_message_id: "d-1".into(),
            body: "Create result.txt containing READY.".into(),
            project_id: project.project_id,
            channel_binding_id: None,
            target: ExecutionTarget { kind: ExecutionKind::Pty, host_id: "host-a".into() },
            title: "Write result.txt".into(),
            cwd: "/repo".into(),
            instruction: "Create result.txt containing READY.".into(),
            requirements: vec![RequirementSpec {
                id: "r1".into(),
                text: "result.txt contains READY".into(),
                verifier: "test -f result.txt".into(),
            }],
            mode: GoalMode::Full,
            turn_limit: 5,
            operation_grant: sv::admission::OperationClass::BASELINE.to_vec(),
        };
        let lane = sv::admit_request(&pool, &request).await.unwrap().receipt.lane_key;
        let control = ferryx_hub::agent_control::RecordingControl::default();

        let refused = apply_goal_action(&pool, &control, Action::Mode { lane_key: lane.clone(), mode: GoalMode::Off })
            .await
            .unwrap_err();
        assert!(
            matches!(&refused, SupervisorGoalError::Conflict(message) if message.starts_with("SUPERVISOR_OWNED")),
            "{refused}"
        );
        assert_eq!(contract_of(&pool, &lane).await.unwrap().unwrap().mode, GoalMode::Full);
        let (shown, _) = apply_goal_action(&pool, &control, Action::Show { lane_key: lane.clone() })
            .await
            .unwrap();
        assert_eq!(shown, lane);
        // A revision reaches the fence through its lane: proposed with the owner's words (the
        // lane title), it is neither accepted nor rejected through the legacy path.
        use ferryx_hub::goals::revision::{self, RevisionChange};
        let proposed = revision::propose(
            &pool,
            &lane,
            &RevisionChange::Waive { req_id: "r1".into() },
            "Write result.txt",
        )
        .await
        .unwrap();
        let base = contract_of(&pool, &lane).await.unwrap().unwrap().revision;
        for action in [
            Action::AcceptRevision { id: proposed.id },
            Action::RejectRevision { id: proposed.id },
        ] {
            let refused = apply_goal_action(&pool, &control, action)
                .await
                .unwrap_err();
            assert!(
                matches!(&refused, SupervisorGoalError::Conflict(message) if message.starts_with("SUPERVISOR_OWNED")),
                "{refused}"
            );
        }
        assert_eq!(revision::list_pending(&pool, &lane).await.unwrap().len(), 1);
        assert_eq!(
            contract_of(&pool, &lane).await.unwrap().unwrap().revision,
            base
        );
        assert!(control.ops().is_empty());
    }

    /// `supervisor submit` binds the chosen pane at admission and `dispatch status` reads it
    /// back with its dispatch; a second goal on the same pane is refused SESSION_ALREADY_BOUND,
    /// and reading the status writes nothing.
    #[tokio::test]
    async fn dispatch_submit_binds_the_pane_and_status_reads_without_writing() {
        use crate::daemon::protocol::SupervisorDispatchAction as Action;
        use ferryx_hub::supervisor as sv;
        let pool = ferryx_hub::storage::memory().await.unwrap();
        sv::provision_owner(&pool, "qa-owner", "qa", "qa:qa", "paired-1", "/work/qa/.git").await.unwrap();
        let work: sv::admission::SubmitWork = serde_json::from_value(json!({
            "title": "Write result.txt",
            "instruction": "Create result.txt containing READY.",
            "requirements": [{"id": "r1", "text": "result.txt contains READY", "verifier": "test -f result.txt"}],
            "operations": ["edit"],
        }))
        .unwrap();
        let submit = |message_id: &str| Action::Submit {
            message_id: message_id.into(),
            body: "Create result.txt containing READY.".into(),
            host_id: Some("paired-1".into()),
            session_id: Some("pane-1".into()),
            work: Box::new(work.clone()),
        };
        let admitted = apply_dispatch_action(&pool, submit("m-1")).await.unwrap();
        assert_eq!(admitted["outcome"], "admitted", "{admitted}");
        let goal_id = admitted["receipt"]["goal_id"].as_str().unwrap().to_owned();
        let refused = apply_dispatch_action(&pool, submit("m-2")).await.unwrap_err();
        assert_eq!(refused.code(), sv::SESSION_ALREADY_BOUND, "{refused}");

        // Every row the status reads, in full (revision and timestamps included).
        let fingerprint = || async {
            (
                sv::goal_of(&pool, &goal_id).await.unwrap(),
                sv::execution_binding_of(&pool, &goal_id).await.unwrap(),
                sv::dispatches_of(&pool, &goal_id).await.unwrap(),
            )
        };
        let before = fingerprint().await;
        let status = apply_dispatch_action(&pool, Action::Status { goal_id: goal_id.clone() }).await.unwrap();
        assert_eq!(fingerprint().await, before, "a status read writes nothing");
        assert_eq!(status["goal"]["runState"], "ready", "{status}");
        assert_eq!(status["goal"]["revision"], 1);
        assert_eq!(status["binding"]["hostId"], "paired-1");
        assert_eq!(status["binding"]["providerSessionId"], "pane-1");
        let dispatches = status["dispatches"].as_array().unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0]["requestId"], admitted["receipt"]["dispatch_request_id"]);
        assert_eq!(dispatches[0]["state"], "ready");
        assert!(dispatches[0]["acceptedAt"].is_null() && dispatches[0]["uncertainCode"].is_null());

        let missing = apply_dispatch_action(&pool, Action::Status { goal_id: "00000000-0000-4000-8000-000000000000".into() })
            .await
            .unwrap_err();
        assert_eq!(missing.code(), "NOT_FOUND");
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

    #[tokio::test]
    async fn an_engine_action_without_an_installed_engine_is_mhc_not_running() {
        use crate::daemon::protocol::SupervisorEngineAction as Action;
        assert_eq!(SUPERVISOR_ENGINE_CAPABILITY, "supervisorEngineV1");
        for action in [
            Action::Status,
            Action::Assess { prompt: "grade this".into() },
            Action::AssessGoal { goal_id: "g-1".into() },
        ] {
            let refused = engine_view(None, None, None, action).await.unwrap_err();
            assert_eq!(refused.code(), "MHC_NOT_RUNNING");
            assert!(refused.to_string().starts_with("MHC_NOT_RUNNING:"), "{refused}");
        }
        assert_eq!(SupervisorEngineError::Unavailable.code(), "SUPERVISOR_UNAVAILABLE");
    }

    use crate::daemon::supervisor_lease_role::Role;

    /// A lease client stub: whether the hub may run now, and what an acquire turns it into.
    struct FakeLease {
        allows: std::sync::atomic::AtomicBool,
        acquire_grants: bool,
        acquires: std::sync::atomic::AtomicUsize,
    }

    impl FakeLease {
        fn new(allows: bool, acquire_grants: bool) -> Self {
            Self {
                allows: std::sync::atomic::AtomicBool::new(allows),
                acquire_grants,
                acquires: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    fn held_by_b() -> Role {
        Role::Standby {
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Laptop B".into()),
            fence: Some(7),
        }
    }

    #[async_trait]
    impl LeaseGate for FakeLease {
        fn allows_hub(&self) -> bool {
            self.allows.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn current_role(&self) -> Role {
            if self.allows_hub() { Role::Holder { fence: 7 } } else { held_by_b() }
        }

        async fn try_acquire(&self) -> Option<Role> {
            self.acquires.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.acquire_grants {
                self.allows.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Some(self.current_role())
        }
    }

    /// One generation task that never ends on its own; the receiver errors once it is aborted.
    fn pending_generation() -> (Generation, tokio::sync::oneshot::Receiver<()>) {
        let (alive, ended) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _alive = alive;
            std::future::pending::<()>().await;
        });
        (Generation { tasks: vec![task] }, ended)
    }

    /// Stands in for `open_generation` (stored replies, supervisor pane, then the tasks):
    /// counts how often a start reached it.
    fn counting_spawn(calls: &std::sync::atomic::AtomicUsize) -> impl std::future::Future<Output = Generation> + '_ {
        async move {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Generation { tasks: Vec::new() }
        }
    }

    #[tokio::test]
    async fn a_start_on_standby_is_refused_with_the_holder_and_spawns_nothing() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::control::mark_stopped(&pool).await.unwrap();
        let running = tokio::sync::Mutex::new(None);
        let lease = FakeLease::new(false, false);
        let calls = std::sync::atomic::AtomicUsize::new(0);

        let refused = gated_start(&running, &pool, Some(&lease as &dyn LeaseGate), || counting_spawn(&calls)).await.unwrap_err();

        assert!(
            matches!(&refused, HubStartError::Standby { holder_machine_id, holder_label }
                if holder_machine_id.as_deref() == Some("mach-b") && holder_label.as_deref() == Some("Laptop B")),
            "{refused:?}"
        );
        assert_eq!(lease.acquires.load(std::sync::atomic::Ordering::SeqCst), 1, "asked for the lease once, unforced");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0, "no replies flushed, no pane, no tasks");
        assert!(running.lock().await.is_none());
        assert_eq!(HUB_TASKS.live(), 0);
        assert!(ferryx_hub::control::is_stopped(&pool).await.unwrap(), "a refused start writes nothing");
        assert_eq!(
            standby_reply(&refused),
            json!({"status": "standby", "op": "start", "holder": "Laptop B"})
        );
        assert!(!ferryx_hub::control::reply_ok(&standby_reply(&refused).to_string()), "the CLI exits non-zero");
    }

    #[tokio::test]
    async fn a_standby_start_that_wins_the_lease_runs_the_generation() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::control::mark_stopped(&pool).await.unwrap();
        let running = tokio::sync::Mutex::new(None);
        let lease = FakeLease::new(false, true);
        let calls = std::sync::atomic::AtomicUsize::new(0);

        let started = gated_start(&running, &pool, Some(&lease as &dyn LeaseGate), || counting_spawn(&calls)).await.unwrap();

        assert!(started);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(running.lock().await.is_some());
        assert!(!ferryx_hub::control::is_stopped(&pool).await.unwrap(), "start clears the owner's stop");
    }

    #[tokio::test]
    async fn an_unleased_daemon_starts_as_before() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let running = tokio::sync::Mutex::new(None);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        assert!(gated_start(&running, &pool, None, || counting_spawn(&calls)).await.unwrap());
        assert!(!gated_start(&running, &pool, None, || counting_spawn(&calls)).await.unwrap(), "already running");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// Not cfg(unix): the stand-down must stop the hub on Windows too.
    #[tokio::test]
    async fn losing_the_lease_aborts_the_generation_and_regaining_runs_it_again() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::control::clear_stopped(&pool).await.unwrap();
        let (generation, ended) = pending_generation();
        let running = Arc::new(tokio::sync::Mutex::new(Some(generation)));
        let (wanted, decisions) = tokio::sync::watch::channel(true);
        let (promoted_tx, mut promoted) = tokio::sync::mpsc::unbounded_channel::<()>();
        let follower = tokio::spawn(follow_lease(decisions, Arc::clone(&running), move || {
            let promoted_tx = promoted_tx.clone();
            async move {
                let _ = promoted_tx.send(());
                Some(Generation { tasks: Vec::new() })
            }
        }));
        let limit = std::time::Duration::from_secs(10);

        wanted.send_replace(false);
        assert!(tokio::time::timeout(limit, ended).await.expect("bounded").is_err(), "the task was aborted");
        // The stand-down takes the generation out of the slot before it aborts the tasks.
        assert!(running.lock().await.is_none());
        assert!(!ferryx_hub::control::is_stopped(&pool).await.unwrap(), "lease loss leaves hub_stopped as it was");

        wanted.send_replace(true);
        tokio::time::timeout(limit, promoted.recv()).await.expect("bounded").expect("promoted");
        // The follower assigns the slot under the lock it held while promoting.
        assert!(running.lock().await.is_some(), "regaining the lease runs a generation again");
        follower.abort();
    }

    #[tokio::test]
    async fn a_staged_database_replaces_the_hub_database_on_promotion() {
        use ferryx_hub::supervisor as sv;
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("hub.sqlite");
        let local = ferryx_hub::storage::connect(&database).await.unwrap();
        assert!(sv::identities(&local).await.unwrap().is_empty());
        let incoming = staged_database(&database);
        let imported = ferryx_hub::storage::connect(&incoming).await.unwrap();
        sv::provision_owner(&imported, "slack:T1:U-owner", "ferryx", "github.com/example/ferryx", "host-b", "/repo/.git")
            .await
            .unwrap();
        imported.close().await;

        assert!(swap_staged_database(&local, &database).await.unwrap());
        assert!(local.is_closed(), "the old pool is closed before the swap");
        assert!(!incoming.exists());
        assert!(!with_suffix(&incoming, "-wal").exists(), "the staged WAL was folded in");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("hub.sqlite.pre-move-") && !name.ends_with("-wal") && !name.ends_with("-shm"))
            .collect();
        assert_eq!(backups.len(), 1, "the replaced database is kept: {backups:?}");
        let backup = ferryx_hub::storage::connect(&dir.path().join(&backups[0])).await.unwrap();
        assert!(sv::identities(&backup).await.unwrap().is_empty(), "the backup is the local database");
        backup.close().await;
        let reopened = ferryx_hub::storage::connect(&database).await.unwrap();
        assert_eq!(sv::identities(&reopened).await.unwrap().len(), 1, "the imported supervisor is there");
        assert!(!swap_staged_database(&reopened, &database).await.unwrap(), "nothing staged: nothing swapped");
        assert!(!reopened.is_closed());
    }

    /// Counts what opening a generation did: stored replies flushed, panes launched,
    /// generations spawned (with the pool each one ran on).
    struct CountingEffects {
        flushes: std::sync::atomic::AtomicUsize,
        panes: std::sync::atomic::AtomicUsize,
        spawned: tokio::sync::watch::Sender<usize>,
        pools: std::sync::Mutex<Vec<ferryx_hub::goals::SqlitePool>>,
    }

    impl CountingEffects {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                flushes: std::sync::atomic::AtomicUsize::new(0),
                panes: std::sync::atomic::AtomicUsize::new(0),
                spawned: tokio::sync::watch::channel(0).0,
                pools: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn counts(&self) -> (usize, usize, usize) {
            (
                self.flushes.load(std::sync::atomic::Ordering::SeqCst),
                self.panes.load(std::sync::atomic::Ordering::SeqCst),
                *self.spawned.borrow(),
            )
        }
    }

    #[async_trait]
    impl GenerationEffects for CountingEffects {
        async fn flush_pending(&self, _pool: &ferryx_hub::goals::SqlitePool) {
            self.flushes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        async fn launch_pane(&self, _launch: &HubLaunch, _cwd: &str, _command: &[String]) {
            self.panes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }

        async fn spawn(&self, launch: &HubLaunch) -> Generation {
            self.pools.lock().unwrap().push(launch.hub.pool.clone());
            self.spawned.send_modify(|spawned| *spawned += 1);
            Generation { tasks: Vec::new() }
        }
    }

    /// A launch over `pool` whose effects are counted and whose agent control records sends.
    fn test_launch(
        pool: ferryx_hub::goals::SqlitePool,
        config: HubConfig,
        effects: &Arc<CountingEffects>,
    ) -> (HubLaunch, Arc<ferryx_hub::agent_control::RecordingControl>) {
        let control = Arc::new(ferryx_hub::agent_control::RecordingControl::default());
        let hub = Hub::with_policy(
            pool,
            control.clone(),
            config.clone(),
            ferryx_hub::sessions::watch::NotifyPolicy::default(),
        );
        let launch = HubLaunch {
            server: Arc::new(DaemonServer::new()),
            config,
            hub,
            bridge: Arc::new(DaemonBridge { client: DaemonClient::new(), gateway: None }),
            #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
            profiles: ferryx_hub::chat::router::ProfileRouter::new(Vec::new()),
            #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
            meta_session_id: "meta".into(),
            effects: effects.clone(),
        };
        (launch, control)
    }

    /// A config that arms the supervisor pane, so a start that got past the gate would launch it.
    fn pane_config() -> HubConfig {
        HubConfig {
            supervisor_pane: true,
            supervisor_cwd: Some(std::env::temp_dir()),
            ..HubConfig::default()
        }
    }

    /// Boot (what `start` runs once the hub is open) on a standby: no stored reply is posted,
    /// no pane is launched, no generation runs, nothing is sent; the hub is still published.
    #[tokio::test]
    async fn a_standby_boot_flushes_nothing_launches_no_pane_and_runs_no_generation() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::control::clear_stopped(&pool).await.unwrap();
        let effects = CountingEffects::new();
        let (launch, control) = test_launch(pool, pane_config(), &effects);
        let lease = FakeLease::new(false, false);

        let running = boot_hub(&launch, false, Some(&lease as &dyn LeaseGate)).await;

        assert!(running.lock().await.is_none(), "a standby boots without a generation");
        assert_eq!(effects.counts(), (0, 0, 0), "no flush, no pane, no generation");
        assert!(control.ops().is_empty(), "nothing was sent");
        let (_, published) = supervisor_view().expect("the hub is published for the view");
        assert!(Arc::ptr_eq(&published, &running));

        // The same boot on the holder (pane off, so no launcher is installed) runs the effects.
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let (launch, _) = test_launch(pool, HubConfig::default(), &effects);
        let holder = FakeLease::new(true, false);
        let running = boot_hub(&launch, false, Some(&holder as &dyn LeaseGate)).await;
        assert!(running.lock().await.is_some());
        assert_eq!(effects.counts(), (1, 0, 1));
    }

    /// The control.sock / CLI / phone start path asks the installed lease, not a test-only one.
    #[tokio::test]
    async fn the_control_socket_start_is_refused_on_standby() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        ferryx_hub::control::mark_stopped(&pool).await.unwrap();
        let effects = CountingEffects::new();
        let (launch, control) = test_launch(pool.clone(), HubConfig::default(), &effects);
        let lease = Arc::new(FakeLease::new(false, false));
        set_lease_gate(Some(lease.clone() as Arc<dyn LeaseGate>));
        let running = tokio::sync::Mutex::new(None);

        let refused = start_generation(&running, &launch).await.unwrap_err();

        assert!(
            matches!(&refused, HubStartError::Standby { holder_label, .. } if holder_label.as_deref() == Some("Laptop B")),
            "{refused:?}"
        );
        assert_eq!(lease.acquires.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(effects.counts(), (0, 0, 0));
        assert!(control.ops().is_empty());
        assert!(running.lock().await.is_none());
        assert!(ferryx_hub::control::is_stopped(&pool).await.unwrap(), "a refused start writes nothing");
        set_lease_gate(None);
    }

    /// Lease loss on the published hub: the generation the view shows is aborted and the pool
    /// the view and the next generation use keeps hub_stopped=0; regaining runs one again.
    #[tokio::test]
    async fn a_stand_down_leaves_the_published_hub_unstopped_and_regaining_starts_it() {
        let dir = tempfile::tempdir().unwrap();
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let config = HubConfig { database_path: Some(dir.path().join("hub.sqlite")), ..HubConfig::default() };
        let effects = CountingEffects::new();
        let (launch, _) = test_launch(pool, config, &effects);
        let (generation, ended) = pending_generation();
        publish_hub(&launch, &Arc::new(tokio::sync::Mutex::new(Some(generation))));
        let (hub, running) = supervisor_view().unwrap();
        ferryx_hub::control::clear_stopped(&hub.pool).await.unwrap();
        let (wanted, decisions) = tokio::sync::watch::channel(true);
        let follower = tokio::spawn(follow_lease(decisions, Arc::clone(&running), promote_generation));
        let limit = std::time::Duration::from_secs(10);

        wanted.send_replace(false);
        assert!(tokio::time::timeout(limit, ended).await.expect("bounded").is_err(), "the task was aborted");
        assert!(running.lock().await.is_none());
        let (hub, _) = supervisor_view().unwrap();
        assert!(!ferryx_hub::control::is_stopped(&hub.pool).await.unwrap(), "the view's pool is not stopped");
        let launch = supervisor_launch().unwrap();
        assert!(!ferryx_hub::control::is_stopped(&launch.hub.pool).await.unwrap(), "the next generation's pool is not stopped");
        assert_eq!(effects.counts(), (0, 0, 0));

        let mut spawned = effects.spawned.subscribe();
        wanted.send_replace(true);
        tokio::time::timeout(limit, spawned.wait_for(|spawned| *spawned == 1))
            .await
            .expect("bounded")
            .expect("spawned");
        assert!(running.lock().await.is_some(), "regaining the lease runs a generation again");
        follower.abort();
    }

    /// Promotion with a staged database: the hub reopens on it before the first generation,
    /// which sees a goal that exists only in the imported database.
    #[tokio::test]
    async fn a_promotion_reopens_the_hub_on_the_staged_database_before_the_first_generation() {
        use ferryx_hub::goals::contract::RequirementSpec;
        use ferryx_hub::goals::GoalMode;
        use ferryx_hub::supervisor::{self as sv, AdmitRequest, ChannelProvider, ExecutionKind, ExecutionTarget};
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("hub.sqlite");
        let local = ferryx_hub::storage::connect(&database).await.unwrap();
        let imported = ferryx_hub::storage::connect(&staged_database(&database)).await.unwrap();
        let owner = "slack:T1:U-owner";
        let (identity, project, _) =
            sv::provision_owner(&imported, owner, "ferryx", "github.com/example/ferryx", "host-b", "/repo/.git")
                .await
                .unwrap();
        let request = AdmitRequest {
            supervisor_id: identity.supervisor_id,
            owner_principal: owner.into(),
            source_adapter: ChannelProvider::Desktop,
            source_message_id: "d-1".into(),
            body: "Create result.txt containing READY.".into(),
            project_id: project.project_id,
            channel_binding_id: None,
            target: ExecutionTarget { kind: ExecutionKind::Pty, host_id: "host-b".into() },
            title: "Write result.txt".into(),
            cwd: "/repo".into(),
            instruction: "Create result.txt containing READY.".into(),
            requirements: vec![RequirementSpec {
                id: "r1".into(),
                text: "result.txt contains READY".into(),
                verifier: "test -f result.txt".into(),
            }],
            mode: GoalMode::Full,
            turn_limit: 5,
            operation_grant: sv::admission::OperationClass::BASELINE.to_vec(),
        };
        let goal_id = sv::admit_request(&imported, &request).await.unwrap().receipt.goal_id;
        ferryx_hub::control::clear_stopped(&imported).await.unwrap();
        imported.close().await;
        assert!(sv::goal_of(&local, &goal_id).await.unwrap().is_none(), "the goal is only in the import");
        let config = HubConfig { database_path: Some(database.clone()), ..HubConfig::default() };
        let effects = CountingEffects::new();
        let (launch, _) = test_launch(local.clone(), config, &effects);
        let running = Arc::new(tokio::sync::Mutex::new(None));
        publish_hub(&launch, &running);

        let generation = promote_generation().await;

        assert!(generation.is_some(), "the imported hub is not stopped, so a generation runs");
        assert!(local.is_closed(), "the standby's pool was closed for the swap");
        let first = effects.pools.lock().unwrap().first().cloned().expect("one generation");
        assert!(sv::goal_of(&first, &goal_id).await.unwrap().is_some(), "the first generation sees the imported goal");
        let (hub, view_running) = supervisor_view().unwrap();
        assert!(Arc::ptr_eq(&view_running, &running), "the running slot is kept");
        assert!(sv::goal_of(&hub.pool, &goal_id).await.unwrap().is_some(), "the view shows the imported hub");
        assert!(std::fs::read_dir(dir.path())
            .unwrap()
            .any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with("hub.sqlite.pre-move-")));
        hub.pool.close().await;
    }

    /// A relay published after the hub started binds the lease client (a standby stands the
    /// generation down); a replaced relay rebinds it and the previous client ends. Runs on the
    /// current-thread test runtime: `follow_relay` finishes each bind before the test resumes.
    #[tokio::test]
    async fn a_relay_published_later_binds_and_a_replaced_relay_rebinds_the_lease_client() {
        set_lease_gate(None);
        let (published_tx, published) = tokio::sync::watch::channel(0u64);
        let relay = Arc::new(std::sync::Mutex::new(None::<bool>));
        let (bound_tx, mut bound) = tokio::sync::mpsc::unbounded_channel::<bool>();
        let (ended_tx, mut ended) = tokio::sync::mpsc::unbounded_channel::<tokio::sync::oneshot::Receiver<()>>();
        let (wanted, mut decisions) = tokio::sync::watch::channel(true);
        let current = Arc::clone(&relay);
        let follower = tokio::spawn(follow_relay(
            published,
            move || *current.lock().unwrap(),
            move |holder: bool| {
                let (task, alive) = {
                    let (generation, alive) = pending_generation();
                    (generation.tasks.into_iter().next().unwrap(), alive)
                };
                let _ = ended_tx.send(alive);
                let _ = bound_tx.send(holder);
                let gate: Arc<dyn LeaseGate> = Arc::new(FakeLease::new(holder, false));
                async move { (gate, task) }
            },
            None,
            wanted,
        ));
        let limit = std::time::Duration::from_secs(10);
        assert!(lease_gate().is_none(), "no relay yet: unleased");

        *relay.lock().unwrap() = Some(false);
        published_tx.send_modify(|n| *n += 1);
        assert!(!tokio::time::timeout(limit, bound.recv()).await.expect("bounded").unwrap());
        tokio::time::timeout(limit, decisions.changed()).await.expect("bounded").unwrap();
        assert!(!*decisions.borrow_and_update(), "bound on standby: the generation stands down");
        assert!(lease_gate().is_some_and(|gate| !gate.allows_hub()), "the hub is leased now");
        let first = ended.recv().await.unwrap();

        *relay.lock().unwrap() = Some(true);
        published_tx.send_modify(|n| *n += 1);
        assert!(tokio::time::timeout(limit, bound.recv()).await.expect("bounded").unwrap());
        assert!(tokio::time::timeout(limit, first).await.expect("bounded").is_err(), "the previous client ended");
        assert!(lease_gate().is_some_and(|gate| gate.allows_hub()), "rebound to the new relay");
        follower.abort();
        set_lease_gate(None);
    }
}
