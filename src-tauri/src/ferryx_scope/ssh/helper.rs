//! Standalone headless helper. No GUI initialization or existing daemon adoption.
use base64::prelude::*;
use portable_pty::{CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

use crate::scoped_contracts::{Epoch, TargetRef};
pub(super) use super::{private_file, validate_private};

#[path = "../../dag/paths.rs"]
mod dag_paths;

#[path = "../../dag/journal.rs"]
pub mod dag_journal;

#[path = "dag_stream.rs"]
pub mod dag_stream;

#[path = "agent_state.rs"]
pub mod agent_state;

#[path = "boot_identity.rs"]
pub mod boot_identity;

#[path = "recovery.rs"]
pub mod recovery;

pub const MAX_FRAME: usize = 1024 * 1024;
pub const RING_BYTES: usize = 512 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub protocol: u32,
    pub token: String,
    pub op: String,
    #[serde(default)]
    pub params: Value,
}

pub fn read_frame(reader: &mut impl Read) -> Result<Option<Value>, String> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => (),
        Err(e) => return Err(e.to_string()),
    }
    reader
        .read_exact(&mut header[1..])
        .map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME {
        return Err("INVALID_REQUEST: frame exceeds 1 MiB".into());
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| e.to_string())
}

pub fn write_frame(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME {
        return Err("INVALID_REQUEST: frame exceeds 1 MiB".into());
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

pub(crate) struct Output {
    next: u64,
    bytes: usize,
    chunks: VecDeque<(u64, Vec<u8>)>,
    exited: bool,
    pub(crate) agent_revision: u64,
    pub(crate) agent_state: Option<agent_state::AgentStateSnapshot>,
}

struct Session {
    target: TargetRef,
    pid: u32,
    cwd: PathBuf,
    dimensions: Mutex<(u16, u16)>,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    output: Arc<(Mutex<Output>, Condvar)>,
}

#[derive(Clone)]
struct SpawnRecord {
    target: TargetRef,
    pid: u32,
    params: Value,
}

#[derive(Clone)]
enum SpawnState {
    InProgress { params: Value },
    Completed(SpawnRecord),
}

struct SpawnReservationGuard<'a> {
    req_id: String,
    spawns: &'a Mutex<HashMap<String, SpawnState>>,
    cv: &'a Condvar,
    completed: bool,
}

impl<'a> Drop for SpawnReservationGuard<'a> {
    fn drop(&mut self) {
        if !self.completed {
            if let Ok(mut spawns) = self.spawns.lock() {
                if matches!(
                    spawns.get(&self.req_id),
                    Some(SpawnState::InProgress { .. })
                ) {
                    spawns.remove(&self.req_id);
                    self.cv.notify_all();
                }
            }
        }
    }
}

pub struct Runtime {
    root: PathBuf,
    host: String,
    owner: String,
    epoch: Epoch,
    token: String,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    projects: Mutex<HashMap<String, PathBuf>>,
    spawns: Mutex<HashMap<String, SpawnState>>,
    spawns_cv: Condvar,
    dag_streams: dag_stream::DagStreams,
    pub(crate) agent_state: Option<Arc<agent_state::AgentStateServer>>,
    pub(crate) recovery_store: Arc<recovery::RecoveryStore>,
    boot_id_source: Arc<Mutex<boot_identity::BootIdSource>>,
    logical_to_backend: Mutex<HashMap<String, String>>,
    backend_to_logical: Mutex<HashMap<String, String>>,
    #[cfg(test)]
    spawn_hook: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
    #[cfg(test)]
    executable_resolver: Mutex<Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>>,
}

impl Runtime {
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub fn set_spawn_hook(&self, hook: Option<Arc<dyn Fn(&str) + Send + Sync>>) {
        *self.spawn_hook.lock().unwrap() = hook;
    }

    #[cfg(test)]
    pub fn set_executable_resolver_for_test(
        &self,
        resolver: Option<Arc<dyn Fn(&str) -> Option<String> + Send + Sync>>,
    ) {
        *self.executable_resolver.lock().unwrap() = resolver;
    }

    pub fn new(root: PathBuf, host: String, token: String) -> Result<Self, String> {
        Self::new_with_options(root, host, token, None, None)
    }

    pub fn new_with_options(
        root: PathBuf,
        host: String,
        token: String,
        storage_root: Option<PathBuf>,
        boot_id_source: Option<boot_identity::BootIdSource>,
    ) -> Result<Self, String> {
        super::private_file(&root)?;
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        let recovery_base = storage_root.unwrap_or_else(|| canonical_root.join("recovery"));
        let recovery_store = Arc::new(recovery::RecoveryStore::new(recovery_base, &host)?);
        let boot_id = Arc::new(Mutex::new(boot_id_source.unwrap_or_default()));
        let agent_state = match agent_state::AgentStateServer::bind() {
            Ok(server) => Some(Arc::new(server)),
            Err(err) => {
                eprintln!("Ferryx SSH helper agent state listener unavailable: {err}");
                None
            }
        };
        Ok(Self {
            root: canonical_root,
            host,
            owner: uuid::Uuid::new_v4().to_string(),
            epoch: Epoch(rand::random()),
            token,
            sessions: Mutex::new(HashMap::new()),
            projects: Mutex::new(HashMap::new()),
            spawns: Mutex::new(HashMap::new()),
            spawns_cv: Condvar::new(),
            dag_streams: dag_stream::DagStreams::new(),
            agent_state,
            recovery_store,
            boot_id_source: boot_id,
            logical_to_backend: Mutex::new(HashMap::new()),
            backend_to_logical: Mutex::new(HashMap::new()),
            #[cfg(test)]
            spawn_hook: Mutex::new(None),
            #[cfg(test)]
            executable_resolver: Mutex::new(None),
        })
    }

    pub fn current_boot_id(&self) -> Result<String, String> {
        self.boot_id_source.lock().unwrap().resolve()
    }

    #[cfg(test)]
    pub fn set_boot_id_for_test(&self, boot_id: String) {
        *self.boot_id_source.lock().unwrap() = boot_identity::BootIdSource::Injected(boot_id);
    }

    #[cfg(test)]
    pub fn recovery_store(&self) -> &Arc<recovery::RecoveryStore> {
        &self.recovery_store
    }

    pub fn handle(&self, request: Request) -> Result<Value, String> {
        self.handle_on_connection(request, dag_stream::DETACHED_CONNECTION)
    }

    /// Handles a request on behalf of a specific bridge connection. DAG
    /// subscriptions opened here are released when that connection closes.
    pub fn handle_on_connection(&self, request: Request, connection: u64) -> Result<Value, String> {
        if request.protocol != 1 {
            return Err("UNSUPPORTED: helper protocol requires version 1".into());
        }
        if request.token != self.token {
            return Err("UNAUTHORIZED".into());
        }
        self.dispatch_on_connection(&request.op, &request.params, connection)
    }

    /// Releases every DAG subscription owned by a closed bridge connection.
    /// PTY sessions are untouched: they outlive their connection by design.
    pub fn release_connection(&self, connection: u64) {
        self.dag_streams.close_connection(connection);
    }

    #[cfg(test)]
    pub fn dag_streams(&self) -> &dag_stream::DagStreams {
        &self.dag_streams
    }

    #[cfg(test)]
    pub fn agent_state_server(&self) -> Option<&Arc<agent_state::AgentStateServer>> {
        self.agent_state.as_ref()
    }

    #[cfg(test)]
    pub fn session_token(&self, session_id: &str) -> Option<String> {
        self.agent_state
            .as_ref()
            .and_then(|s| s.get_token(session_id))
    }

    fn dispatch_on_connection(
        &self,
        op: &str,
        p: &Value,
        connection: u64,
    ) -> Result<Value, String> {
        match op {
            "handshake" => {
                // Shared with the `--capabilities` CLI: the compile surface is the
                // constant, and the runtime-conditional token is advertised only
                // when the agent-state listener actually bound.
                let capabilities: Vec<String> = super::process::HELPER_CAPABILITIES
                    .iter()
                    .filter(|capability| {
                        self.agent_state.is_some()
                            || **capability != super::process::CONDITIONAL_AGENT_STATE
                    })
                    .map(|capability| (*capability).to_string())
                    .collect();
                Ok(json!({
                    "protocol": 1,
                    "capabilities": capabilities,
                    "hostId": self.host,
                    "ownerId": self.owner,
                    "epoch": self.epoch,
                    "os": std::env::consts::OS,
                    "arch": std::env::consts::ARCH,
                    "helperVersion": super::process::HELPER_VERSION,
                }))
            }
            "project.register" => {
                let id = text(p, "id")?;
                let raw_path = text(p, "path")?;
                let path = Path::new(raw_path)
                    .canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid project path: {e}"))?;
                if !path.is_dir() {
                    return Err(
                        "INVALID_REQUEST: project path must be an existing directory".into(),
                    );
                }
                let mut projects = self.projects.lock().map_err(|e| e.to_string())?;
                if projects.get(id).is_some_and(|old| old != &path) {
                    return Err("REQUEST_CONFLICT: project target is immutable".into());
                }
                projects.insert(id.to_string(), path);
                Ok(json!({ "projectId": id }))
            }
            "project.list" => Ok(json!(self
                .projects
                .lock()
                .map_err(|e| e.to_string())?
                .keys()
                .collect::<Vec<_>>())),
            "worktree.create" => {
                let projects = self.projects.lock().map_err(|e| e.to_string())?;
                let repo = projects.get(text(p, "projectId")?).ok_or("NOT_FOUND")?;
                let slug = text(p, "slug")?;
                if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err("INVALID_REQUEST: invalid slug".into());
                }
                let base = repo.join(".orca-worktrees");
                std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
                if !base
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(repo)
                {
                    return Err("FORBIDDEN".into());
                }
                let destination = base.join(format!("wt-{slug}"));
                let output = std::process::Command::new("git")
                    .arg("-C")
                    .arg(repo)
                    .args(["worktree", "add", "-b", &format!("orca/{slug}"), "--"])
                    .arg(&destination)
                    .arg("HEAD")
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output.status.success() {
                    return Err(format!(
                        "REMOTE_GIT_FAILED: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                Ok(json!({
                    "projectId": text(p, "projectId")?,
                    "worktree": format!(".orca-worktrees/wt-{slug}"),
                }))
            }
            "dag.subscribe" => {
                let project_id = text(p, "projectId")?;
                let root = {
                    let projects = self.projects.lock().map_err(|e| e.to_string())?;
                    projects.get(project_id).cloned()
                };
                let root = root.ok_or_else(|| "NOT_FOUND".to_string())?;
                let runs_dir = dag_paths::resolve_dag_runs_dir(&root);
                let mut frame = self.dag_streams.subscribe(runs_dir, connection)?;
                frame["projectId"] = json!(project_id);
                Ok(frame)
            }
            "dag.next" => {
                let id = text(p, "subscriptionId")?;
                let wait_ms = p.get("waitMs").and_then(Value::as_u64).unwrap_or(0);
                self.dag_streams.next(id, wait_ms)
            }
            "dag.unsubscribe" => {
                let id = text(p, "subscriptionId")?;
                self.dag_streams.unsubscribe(id)
            }
            "dag.inventory" => {
                let project_id = text(p, "projectId")?;
                let root = {
                    let projects = self.projects.lock().map_err(|e| e.to_string())?;
                    projects.get(project_id).cloned()
                };
                let root = root.ok_or_else(|| "NOT_FOUND".to_string())?;
                let runs_dir = dag_paths::resolve_dag_runs_dir(&root);
                let mut runs = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&runs_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(val) = serde_json::from_str::<Value>(&content) {
                                    runs.push(val);
                                }
                            }
                        }
                    }
                }
                Ok(json!({
                    "projectId": project_id,
                    "runs": runs,
                }))
            }
            "dag.poll" => {
                let project_id = text(p, "projectId")?;
                let after_mtime_ms = p.get("afterMtimeMs").and_then(Value::as_u64).unwrap_or(0);
                let known_runs = p.get("knownRuns").and_then(Value::as_array);
                let root = {
                    let projects = self.projects.lock().map_err(|e| e.to_string())?;
                    projects.get(project_id).cloned()
                };
                let root = root.ok_or_else(|| "NOT_FOUND".to_string())?;
                let runs_dir = dag_paths::resolve_dag_runs_dir(&root);
                let mut runs = Vec::new();
                let mut max_mtime = after_mtime_ms;
                if let Ok(entries) = std::fs::read_dir(&runs_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().is_some_and(|e| e == "json") {
                            if let Ok(meta) = entry.metadata() {
                                let mtime_ms = meta
                                    .modified()
                                    .ok()
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0);
                                if mtime_ms >= after_mtime_ms {
                                    if let Ok(content) = std::fs::read_to_string(&path) {
                                        if let Ok(val) = serde_json::from_str::<Value>(&content) {
                                            // A known run can change within the cursor's
                                            // timestamp tick. Replay the boundary rather
                                            // than treating run identity as a revision.
                                            if mtime_ms > max_mtime {
                                                max_mtime = mtime_ms;
                                            }
                                            if !known_runs.is_some_and(|known| known.contains(&val))
                                            {
                                                runs.push(val);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(json!({
                    "projectId": project_id,
                    "maxMtimeMs": max_mtime,
                    "runs": runs,
                }))
            }
            "pty.spawn" => {
                let client_req_id = p.get("clientRequestId").and_then(Value::as_str);
                let mut reservation_guard = if let Some(req_id) = client_req_id {
                    let mut spawns = self.spawns.lock().map_err(|e| e.to_string())?;
                    loop {
                        match spawns.get(req_id) {
                            Some(SpawnState::Completed(existing)) => {
                                if spawn_params_match(&existing.params, p) {
                                    return Ok(json!({
                                        "target": existing.target,
                                        "pid": existing.pid,
                                    }));
                                } else {
                                    return Err("REQUEST_CONFLICT: clientRequestId already used with different parameters".into());
                                }
                            }
                            Some(SpawnState::InProgress { params }) => {
                                if !spawn_params_match(params, p) {
                                    return Err("REQUEST_CONFLICT: clientRequestId already used with different parameters".into());
                                }
                                spawns = self
                                    .spawns_cv
                                    .wait_timeout(spawns, std::time::Duration::from_secs(10))
                                    .map_err(|e| e.to_string())?
                                    .0;
                            }
                            None => {
                                spawns.insert(
                                    req_id.to_string(),
                                    SpawnState::InProgress { params: p.clone() },
                                );
                                break;
                            }
                        }
                    }
                    drop(spawns);

                    #[cfg(test)]
                    {
                        let hook_opt = self.spawn_hook.lock().unwrap().clone();
                        if let Some(hook) = hook_opt {
                            hook(req_id);
                        }
                    }

                    Some(SpawnReservationGuard {
                        req_id: req_id.to_string(),
                        spawns: &self.spawns,
                        cv: &self.spawns_cv,
                        completed: false,
                    })
                } else {
                    None
                };

                let projects = self.projects.lock().map_err(|e| e.to_string())?;
                let root = projects.get(text(p, "projectId")?).ok_or("NOT_FOUND")?;
                let project_root = root.clone();
                let worktree_rel = p.get("worktree").and_then(Value::as_str).unwrap_or(".");
                let cwd = root
                    .join(worktree_rel)
                    .canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid cwd: {e}"))?;
                if !cwd.starts_with(root) {
                    return Err("FORBIDDEN: cwd outside project".into());
                }
                let canonical_cwd = cwd.clone();
                let cwd = prepare_spawn_cwd(&cwd);
                ensure_cwd_spawnable(&cwd)?;

                let cols = parse_u16_dim(p.get("cols"), 80, "cols")?;
                let rows = parse_u16_dim(p.get("rows"), 24, "rows")?;
                let (program, args) = resolve_program_and_args(p)?;

                let pair = portable_pty::native_pty_system()
                    .openpty(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| e.to_string())?;

                let id = uuid::Uuid::new_v4().to_string();
                let client_req_id = p.get("clientRequestId").and_then(Value::as_str);
                let logical_session_id = client_req_id.map(ToString::to_string);

                let target = TargetRef {
                    host_id: self.host.clone(),
                    owner_id: self.owner.clone(),
                    epoch: self.epoch,
                    backend_session_id: id.clone(),
                };

                if let Some(ref lid) = logical_session_id {
                    let current_boot = self.current_boot_id()?;
                    let record = recovery::RecoveryRecord {
                        logical_session_id: lid.clone(),
                        host: self.host.clone(),
                        exact_previous_target: target.clone(),
                        boot_id: current_boot,
                        project_id: text(p, "projectId")?.to_string(),
                        project_root: project_root.clone(),
                        worktree: p.get("worktree").and_then(Value::as_str).map(ToString::to_string),
                        cwd: canonical_cwd,
                        cols,
                        rows,
                        agent: None,
                        provider_session: None,
                        disabled: false,
                        updated_at: recovery::current_timestamp()?,
                    };
                    self.recovery_store.save_record(&record)?;
                    self.logical_to_backend.lock().unwrap().insert(lid.clone(), id.clone());
                    self.backend_to_logical.lock().unwrap().insert(id.clone(), lid.clone());
                }

                let output = Arc::new((
                    Mutex::new(Output {
                        next: 1,
                        bytes: 0,
                        chunks: VecDeque::new(),
                        exited: false,
                        agent_revision: 0,
                        agent_state: None,
                    }),
                    Condvar::new(),
                ));

                let (agent_token_opt, mut registration_guard) = if let Some(server) = &self.agent_state {
                    let tok = format!("{:032x}", rand::random::<u128>());
                    let on_report = if let Some(ref lid) = logical_session_id {
                        let store = self.recovery_store.clone();
                        let lid_clone = lid.clone();
                        let expected_target = target.clone();
                        Some(Arc::new(move |_sess_id: &str, agent_opt: Option<&str>, prov_opt: Option<&Value>| {
                            store.update_agent_state_for_target(&lid_clone, &expected_target, agent_opt.map(ToString::to_string), prov_opt.cloned())
                        }) as agent_state::AgentReportCallback)
                    } else {
                        None
                    };
                    server.register_with_callback(&id, &tok, output.clone(), on_report);
                    let guard = agent_state::AgentRegistrationGuard::new(server.clone(), id.clone());
                    (Some(tok), Some(guard))
                } else {
                    (None, None)
                };

                let mut command = CommandBuilder::new(&program);
                for arg in &args {
                    command.arg(arg);
                }
                command.cwd(&cwd);

                scrub_reserved_env_vars(&mut command);

                if std::env::var("TERM")
                    .map(|t| t == "dumb" || t.is_empty())
                    .unwrap_or(true)
                {
                    command.env("TERM", "xterm-256color");
                }
                if let Some(env_obj) = p.get("env").and_then(Value::as_object) {
                    for (k, v) in env_obj {
                        if let Some(s) = v.as_str() {
                            command.env(k, s);
                        }
                    }
                }

                scrub_reserved_env_vars(&mut command);

                command.env("FERRYX_SESSION_ID", &id);
                if let (Some(server), Some(tok)) = (&self.agent_state, &agent_token_opt) {
                    command.env("FERRYX_AGENT_STATE_PORT", server.port().to_string());
                    command.env("FERRYX_AGENT_STATE_TOKEN", tok);
                } else {
                    command.env_remove("FERRYX_AGENT_STATE_PORT");
                    command.env_remove("FERRYX_AGENT_STATE_TOKEN");
                }

                let child = match pair.slave.spawn_command(command) {
                    Ok(c) => c,
                    Err(e) => {
                        if let Some(ref lid) = logical_session_id {
                            self.recovery_store.mark_disabled_for_target(lid, &target)?;
                        }
                        return Err(e.to_string());
                    }
                };
                drop(pair.slave);
                let pid = match child.process_id() {
                    Some(p) => p,
                    None => return Err("REMOTE_SPAWN_FAILED: no PID".into()),
                };

                if let Some(guard) = registration_guard.as_mut() {
                    guard.defuse();
                }

                let child_arc = Arc::new(Mutex::new(child));

                let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
                let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
                let sink = output.clone();
                let agent_server_for_exit = self.agent_state.clone();
                let session_id_for_exit = id.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0; 8192];
                    loop {
                        let read = reader.read(&mut buffer);
                        let (lock, signal) = &*sink;
                        let mut state = lock.lock().expect("output mutex poisoned");
                        match read {
                            Ok(n) if n > 0 => {
                                let seq = state.next;
                                state.next += 1;
                                state.bytes += n;
                                state.chunks.push_back((seq, buffer[..n].to_vec()));
                                while state.bytes > RING_BYTES {
                                    let (_, bytes) = state.chunks.pop_front().unwrap();
                                    state.bytes -= bytes.len();
                                }
                            }
                            _ => {
                                state.exited = true;
                                signal.notify_all();
                                break;
                            }
                        }
                        signal.notify_all();
                    }
                    if let Some(server) = agent_server_for_exit {
                        server.revoke(&session_id_for_exit);
                    }
                });

                self.sessions.lock().map_err(|e| e.to_string())?.insert(
                    id,
                    Arc::new(Session {
                        target: target.clone(),
                        pid,
                        cwd,
                        dimensions: Mutex::new((cols, rows)),
                        master: Mutex::new(pair.master),
                        writer: Mutex::new(writer),
                        child: child_arc,
                        output,
                    }),
                );

                if let Some(guard) = reservation_guard.as_mut() {
                    let mut spawns = self.spawns.lock().map_err(|e| e.to_string())?;
                    spawns.insert(
                        guard.req_id.clone(),
                        SpawnState::Completed(SpawnRecord {
                            target: target.clone(),
                            pid,
                            params: p.clone(),
                        }),
                    );
                    guard.completed = true;
                    self.spawns_cv.notify_all();
                }

                Ok(json!({ "target": target, "pid": pid }))
            }
            "pty.recover" => {
                let logical_id = text(p, "logicalSessionId")?;
                let prev_target: TargetRef = serde_json::from_value(
                    p.get("previousTarget")
                        .cloned()
                        .ok_or("INVALID_REQUEST: previousTarget required")?,
                )
                .map_err(|e| format!("INVALID_REQUEST: invalid previousTarget: {e}"))?;

                if prev_target.host_id != self.host {
                    return Err("TARGET_EXPIRED: previousTarget host does not match helper host".into());
                }

                let _transaction = self.recovery_store.transaction()?;
                let recovery_lock = self.recovery_store.get_recovery_lock(logical_id);
                let _guard = recovery_lock.lock().map_err(|e| e.to_string())?;

                let record = self
                    .recovery_store
                    .load_record_unlocked(logical_id)?
                    .ok_or_else(|| "NOT_FOUND: no recovery record for logical session".to_string())?;

                if record.disabled {
                    return Err("RECOVERY_REFUSED: session was already terminated".into());
                }

                let current_boot = self.current_boot_id()?;

                if let Some(receipt) = self.recovery_store.load_completion(logical_id)? {
                    if receipt.source_target == prev_target && receipt.boot_id == current_boot {
                        if receipt.target != record.exact_previous_target
                            || receipt.target.owner_id != self.owner || receipt.target.epoch != self.epoch
                        {
                            return Err("RECOVERY_REFUSED: recovered execution belongs to another helper".into());
                        }
                        let session = self.sessions.lock().map_err(|e| e.to_string())?
                            .get(&receipt.target.backend_session_id).cloned();
                        if let Some(session) = session {
                            let alive = session.child.lock().map_err(|e| e.to_string())?
                                .try_wait().map_err(|e| e.to_string())?.is_none();
                            if !alive || session.output.0.lock().map_err(|e| e.to_string())?.exited {
                                return Err("RECOVERY_REFUSED: recovered execution has exited".into());
                            }
                            if receipt.pid != session.pid || receipt.boot_id != record.boot_id {
                                return Err("CORRUPT_COMPLETION: live execution identity mismatch".into());
                            }
                            self.recovery_store.remove_pre_launch(logical_id, receipt.provider_key.as_deref())?;
                            return Ok(json!({
                                "target": receipt.target,
                                "pid": receipt.pid,
                            }));
                        }
                        return Err("RECOVERY_REFUSED: recovered execution is no longer available".into());
                    }
                }

                if record.exact_previous_target != prev_target {
                    return Err("REQUEST_CONFLICT: previousTarget does not match recorded target".into());
                }

                if current_boot == record.boot_id {
                    return Err("RECOVERY_REFUSED: automatic recovery requires OS boot identity change".into());
                }

                if let Some(marker) = self.recovery_store.load_pre_launch(logical_id)? {
                    if marker.host != self.host || marker.source_target != prev_target {
                        return Err("REQUEST_CONFLICT: interrupted launch identity mismatch".into());
                    }
                    if marker.boot_id == current_boot {
                        return Err("RECOVERY_FAILED: ambiguous interrupted launch".into());
                    }
                    self.recovery_store.remove_pre_launch(logical_id, marker.provider_key.as_deref())?;
                }

                if !record.project_root.is_absolute() {
                    return Err("INVALID_REQUEST: project root must be absolute".into());
                }
                if !record.project_root.exists() {
                    return Err("INVALID_REQUEST: project root no longer exists".into());
                }
                recovery::validate_recovery_directory(&record.project_root)?;
                let canonical_project_root = record.project_root.canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid project root: {e}"))?;
                if canonical_project_root != record.project_root {
                    return Err("INVALID_REQUEST: project root path mismatch".into());
                }

                {
                    let mut projects = self.projects.lock().map_err(|e| e.to_string())?;
                    if let Some(existing) = projects.get(&record.project_id) {
                        if existing != &canonical_project_root {
                            return Err("REQUEST_CONFLICT: project ID mapped to different root".into());
                        }
                    } else {
                        projects.insert(record.project_id.clone(), canonical_project_root.clone());
                    }
                }

                if !record.cwd.is_absolute() {
                    return Err("INVALID_REQUEST: cwd must be absolute".into());
                }
                if !record.cwd.exists() {
                    return Err("INVALID_REQUEST: recorded cwd does not exist".into());
                }
                recovery::validate_recovery_directory(&record.cwd)?;
                let canonical_cwd = record.cwd.canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid cwd: {e}"))?;

                if canonical_cwd != record.cwd {
                    return Err("INVALID_REQUEST: cwd path mismatch".into());
                }
                if !canonical_cwd.starts_with(&canonical_project_root) {
                    return Err("FORBIDDEN: cwd outside authorized project root".into());
                }

                let resume_cmd = recovery::resolve_resume_command(&record, &canonical_cwd)?;

                if let Some(existing_receipt) = self.recovery_store.load_completion_by_provider(&resume_cmd.provider_key)? {
                    if existing_receipt.logical_session_id != logical_id && existing_receipt.boot_id == current_boot {
                        return Err("REQUEST_CONFLICT: provider session already recovered under another logical session".into());
                    }
                }

                if let Some(marker) = self.recovery_store.check_provider_pre_launch(&resume_cmd.provider_key)? {
                    if marker.boot_id == current_boot {
                        return Err("REQUEST_CONFLICT: conflicting simultaneous recovery for same provider session".into());
                    }
                    self.recovery_store.remove_pre_launch(&marker.logical_session_id, marker.provider_key.as_deref())?;
                }

                let cwd = prepare_spawn_cwd(&canonical_cwd);
                ensure_cwd_spawnable(&cwd)?;

                let attempt_id = uuid::Uuid::new_v4().to_string();
                let marker = recovery::PreLaunchMarker {
                    logical_session_id: logical_id.to_string(),
                    attempt_id: attempt_id.clone(),
                    host: self.host.clone(),
                    boot_id: current_boot.clone(),
                    source_target: prev_target.clone(),
                    provider_key: Some(resume_cmd.provider_key.clone()),
                    timestamp: recovery::current_timestamp()?,
                };

                let cols = record.cols.max(1);
                let rows = record.rows.max(1);

                let pair = portable_pty::native_pty_system()
                    .openpty(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| e.to_string())?;

                let id = uuid::Uuid::new_v4().to_string();
                let target = TargetRef {
                    host_id: self.host.clone(),
                    owner_id: self.owner.clone(),
                    epoch: self.epoch,
                    backend_session_id: id.clone(),
                };
                let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
                let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

                let output = Arc::new((
                    Mutex::new(Output {
                        next: 1,
                        bytes: 0,
                        chunks: VecDeque::new(),
                        exited: false,
                        agent_revision: 0,
                        agent_state: None,
                    }),
                    Condvar::new(),
                ));

                let (agent_token_opt, mut registration_guard) = if let Some(server) = &self.agent_state {
                    let tok = format!("{:032x}", rand::random::<u128>());
                    let store = self.recovery_store.clone();
                    let lid_clone = logical_id.to_string();
                    let expected_target = target.clone();
                    let on_report = Some(Arc::new(move |_sess_id: &str, agent_opt: Option<&str>, prov_opt: Option<&Value>| {
                        store.update_agent_state_for_target(&lid_clone, &expected_target, agent_opt.map(ToString::to_string), prov_opt.cloned())
                    }) as agent_state::AgentReportCallback);
                    server.register_with_callback(&id, &tok, output.clone(), on_report);
                    let guard = agent_state::AgentRegistrationGuard::new(server.clone(), id.clone());
                    (Some(tok), Some(guard))
                } else {
                    (None, None)
                };

                let program_to_run = {
                    #[cfg(test)]
                    {
                        let resolver = self.executable_resolver.lock().unwrap().clone();
                        if let Some(r) = resolver {
                            r(&resume_cmd.program).unwrap_or(resume_cmd.program)
                        } else {
                            resume_cmd.program
                        }
                    }
                    #[cfg(not(test))]
                    resume_cmd.program
                };

                let mut command = recovery::resume_command_builder(&program_to_run, &resume_cmd.args)?;
                command.cwd(&cwd);

                scrub_reserved_env_vars(&mut command);

                if std::env::var("TERM")
                    .map(|t| t == "dumb" || t.is_empty())
                    .unwrap_or(true)
                {
                    command.env("TERM", "xterm-256color");
                }

                scrub_reserved_env_vars(&mut command);

                command.env("FERRYX_SESSION_ID", &id);
                if let (Some(server), Some(tok)) = (&self.agent_state, &agent_token_opt) {
                    command.env("FERRYX_AGENT_STATE_PORT", server.port().to_string());
                    command.env("FERRYX_AGENT_STATE_TOKEN", tok);
                } else {
                    command.env_remove("FERRYX_AGENT_STATE_PORT");
                    command.env_remove("FERRYX_AGENT_STATE_TOKEN");
                }

                self.recovery_store.record_pre_launch(&marker)?;
                let child = match pair.slave.spawn_command(command) {
                    Ok(c) => c,
                    Err(e) => {
                        self.recovery_store.remove_pre_launch(logical_id, Some(&resume_cmd.provider_key))?;
                        return Err(e.to_string());
                    }
                };
                drop(pair.slave);
                let pid = match child.process_id() {
                    Some(p) => p,
                    None => return Err("REMOTE_SPAWN_FAILED: no PID".into()),
                };

                if let Some(guard) = registration_guard.as_mut() {
                    guard.defuse();
                }

                let child_arc = Arc::new(Mutex::new(child));

                let sink = output.clone();
                let agent_server_for_exit = self.agent_state.clone();
                let session_id_for_exit = id.clone();

                std::thread::spawn(move || {
                    let mut buffer = [0; 8192];
                    loop {
                        let read = reader.read(&mut buffer);
                        let (lock, signal) = &*sink;
                        let mut state = lock.lock().expect("output mutex poisoned");
                        match read {
                            Ok(n) if n > 0 => {
                                let seq = state.next;
                                state.next += 1;
                                state.bytes += n;
                                state.chunks.push_back((seq, buffer[..n].to_vec()));
                                while state.bytes > RING_BYTES {
                                    let (_, bytes) = state.chunks.pop_front().unwrap();
                                    state.bytes -= bytes.len();
                                }
                            }
                            _ => {
                                state.exited = true;
                                signal.notify_all();
                                break;
                            }
                        }
                        signal.notify_all();
                    }
                    if let Some(server) = agent_server_for_exit {
                        server.revoke(&session_id_for_exit);
                    }
                });

                self.sessions.lock().map_err(|e| e.to_string())?.insert(
                    id.clone(),
                    Arc::new(Session {
                        target: target.clone(),
                        pid,
                        cwd: canonical_cwd,
                        dimensions: Mutex::new((cols, rows)),
                        master: Mutex::new(pair.master),
                        writer: Mutex::new(writer),
                        child: child_arc,
                        output,
                    }),
                );

                self.logical_to_backend
                    .lock()
                    .unwrap()
                    .insert(logical_id.to_string(), id.clone());
                self.backend_to_logical
                    .lock()
                    .unwrap()
                    .insert(id.clone(), logical_id.to_string());

                let mut updated_record = record;
                updated_record.exact_previous_target = target.clone();
                updated_record.boot_id = current_boot.clone();
                updated_record.updated_at = recovery::current_timestamp()?;
                self.recovery_store.save_record_unlocked(&updated_record)?;

                let receipt = recovery::CompletionReceipt {
                    logical_session_id: logical_id.to_string(),
                    attempt_id,
                    source_target: prev_target,
                    target: target.clone(),
                    pid,
                    boot_id: current_boot,
                    provider_key: Some(resume_cmd.provider_key),
                    timestamp: recovery::current_timestamp()?,
                };
                self.recovery_store.record_completion(&receipt)?;

                Ok(json!({ "target": target, "pid": pid }))
            }
            "pty.list" => {
                let sessions: Vec<Arc<Session>> = {
                    let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                    sessions.values().cloned().collect()
                };
                let list: Vec<_> = sessions
                    .iter()
                    .map(|s| {
                        let (cols, rows) = *s.dimensions.lock().map_err(|e| e.to_string())?;
                        let (lock, _) = &*s.output;
                        let (cursor, exited) = if let Ok(state) = lock.lock() {
                            (state.next.saturating_sub(1).to_string(), state.exited)
                        } else {
                            ("0".to_string(), false)
                        };
                        Ok::<_, String>(json!({
                            "target": s.target,
                            "pid": s.pid,
                            "cwd": s.cwd,
                            "cols": cols,
                            "rows": rows,
                            "cursor": cursor,
                            "exited": exited,
                        }))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(json!(list))
            }
            "pty.read" | "pty.write" | "pty.resize" | "pty.stop" | "pty.describe" => {
                let target: TargetRef = serde_json::from_value(
                    p.get("target")
                        .cloned()
                        .ok_or("INVALID_REQUEST: target required")?,
                )
                .map_err(|e| e.to_string())?;

                if target.host_id != self.host
                    || target.owner_id != self.owner
                    || target.epoch != self.epoch
                {
                    return Err("TARGET_EXPIRED".into());
                }

                let session = {
                    let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                    sessions
                        .get(&target.backend_session_id)
                        .cloned()
                        .ok_or("NOT_FOUND")?
                };

                match op {
                    "pty.describe" => {
                        if session.child.lock().map_err(|e| e.to_string())?
                            .try_wait().map_err(|e| e.to_string())?.is_some_and(|status| status.success())
                        {
                            if let Some(logical_id) = self.backend_to_logical.lock().map_err(|e| e.to_string())?
                                .get(&target.backend_session_id).cloned()
                            {
                                self.recovery_store.mark_disabled_for_target(&logical_id, &target)?;
                            }
                        }
                        let (cols, rows) = *session.dimensions.lock().map_err(|e| e.to_string())?;
                        let (lock, _) = &*session.output;
                        let state = lock.lock().map_err(|e| e.to_string())?;
                        let cursor = state.next.saturating_sub(1).to_string();
                        Ok(json!({
                            "target": target,
                            "pid": session.pid,
                            "cwd": session.cwd,
                            "cols": cols,
                            "rows": rows,
                            "cursor": cursor,
                            "exited": state.exited,
                        }))
                    }
                    "pty.write" => {
                        let bytes: Vec<u8> = if let Some(b64) =
                            p.get("data").and_then(Value::as_str)
                        {
                            BASE64_STANDARD
                                .decode(b64)
                                .map_err(|e| format!("INVALID_REQUEST: invalid base64 data: {e}"))?
                        } else if let Some(txt) = p.get("text").and_then(Value::as_str) {
                            txt.as_bytes().to_vec()
                        } else {
                            return Err("INVALID_REQUEST: data (base64) or text required".into());
                        };
                        let mut writer = session.writer.lock().map_err(|e| e.to_string())?;
                        writer
                            .write_all(&bytes)
                            .and_then(|_| writer.flush())
                            .map_err(|e| e.to_string())?;
                        Ok(json!({ "accepted": true }))
                    }
                    "pty.resize" => {
                        let cols = parse_u16_dim(p.get("cols"), 0, "cols")?;
                        let rows = parse_u16_dim(p.get("rows"), 0, "rows")?;
                        if cols == 0 || rows == 0 {
                            return Err(
                                "INVALID_REQUEST: cols and rows must be between 1 and 65535".into(),
                            );
                        }
                        let master = session.master.lock().map_err(|e| e.to_string())?;
                        master
                            .resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            })
                            .map_err(|e| e.to_string())?;
                        *session.dimensions.lock().map_err(|e| e.to_string())? = (cols, rows);
                        Ok(json!({ "cols": cols, "rows": rows }))
                    }
                    "pty.stop" => {
                        let logical_id = self.backend_to_logical.lock().map_err(|e| e.to_string())?
                            .get(&target.backend_session_id).cloned();
                        if let Some(lid) = logical_id {
                            self.recovery_store.mark_disabled_for_target(&lid, &target)?;
                        }
                        let mut child = session.child.lock().map_err(|e| e.to_string())?;
                        if child
                            .try_wait()
                            .map_err(|e| e.to_string())?
                            .is_none()
                        {
                            child.kill().map_err(|e| e.to_string())?;
                            child.wait().map_err(|e| e.to_string())?;
                        }
                        if let Some(server) = &self.agent_state {
                            server.revoke(&target.backend_session_id);
                        }
                        Ok(json!({ "stopped": true }))
                    }
                    _ => {
                        let output = session.output.clone();
                        let pid = session.pid;
                        let cwd = session.cwd.clone();

                        let after = parse_cursor(p)?;
                        let agent_after_revision = parse_agent_after_revision(p)?;
                        let wait = p
                            .get("waitMs")
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                            .min(10000);

                        let (lock, signal) = &*output;
                        let state = lock.lock().map_err(|e| e.to_string())?;

                        if let Some(requested) = agent_after_revision {
                            if requested > state.agent_revision {
                                return Err(
                                    "INVALID_REQUEST: agentAfterRevision exceeds current revision"
                                        .into(),
                                );
                            }
                        }

                        let (state, _) = signal
                            .wait_timeout_while(
                                state,
                                std::time::Duration::from_millis(wait),
                                |s| {
                                    let no_output = s.next <= after.saturating_add(1);
                                    let revision_unchanged = match agent_after_revision {
                                        None => true,
                                        Some(req) => s.agent_revision == req,
                                    };
                                    no_output && !s.exited && revision_unchanged
                                },
                            )
                            .map_err(|e| e.to_string())?;

                        let first = state
                            .chunks
                            .front()
                            .map(|(seq, _)| *seq)
                            .unwrap_or(state.next);
                        let gap = after.saturating_add(1) < first;

                        let max_response_len: usize = 900 * 1024;
                        let mut response_chunks = Vec::new();
                        let mut current_size_estimate = 256;
                        let mut last_seq = after;

                        for (seq, chunk_bytes) in
                            state.chunks.iter().filter(|(seq, _)| *seq > after)
                        {
                            let b64 = BASE64_STANDARD.encode(chunk_bytes);
                            let chunk_est = b64.len() + chunk_bytes.len() * 4 + 80;
                            if !response_chunks.is_empty()
                                && current_size_estimate + chunk_est > max_response_len
                            {
                                break;
                            }
                            current_size_estimate += chunk_est;
                            last_seq = *seq;
                            response_chunks.push(json!({
                                "cursor": seq.to_string(),
                                "sequence": seq,
                                "data": b64,
                                "bytes": chunk_bytes,
                            }));
                        }

                        let cursor_num = if response_chunks.is_empty() {
                            after.max(state.next.saturating_sub(1))
                        } else {
                            last_seq
                        };
                        let cursor_str = cursor_num.to_string();

                        let agent_state_json = match agent_after_revision {
                            Some(requested) if state.agent_revision > requested => {
                                state.agent_state.as_ref().map(|s| s.to_response_json())
                            }
                            _ => None,
                        };

                        let mut response = json!({
                            "target": target,
                            "pid": pid,
                            "cwd": cwd,
                            "cursor": cursor_str,
                            "afterSequence": cursor_num,
                            "gap": gap,
                            "exited": state.exited,
                            "chunks": response_chunks,
                        });

                        if let Some(agent_state) = agent_state_json {
                            response["agentState"] = agent_state;
                        }

                        while serde_json::to_vec(&response).map(|v| v.len()).unwrap_or(0)
                            >= MAX_FRAME
                            && !response["chunks"].as_array().unwrap().is_empty()
                        {
                            if let Some(arr) =
                                response.get_mut("chunks").and_then(Value::as_array_mut)
                            {
                                arr.pop();
                                let new_cur = arr
                                    .last()
                                    .and_then(|c| c.get("cursor"))
                                    .and_then(Value::as_str)
                                    .unwrap_or(&after.to_string())
                                    .to_string();
                                let new_seq = arr
                                    .last()
                                    .and_then(|c| c.get("sequence"))
                                    .and_then(Value::as_u64)
                                    .unwrap_or(after);
                                response["cursor"] = json!(new_cur);
                                response["afterSequence"] = json!(new_seq);
                            }
                        }

                        Ok(response)
                    }
                }
            }
            _ => Err("UNSUPPORTED: operation not allowlisted".into()),
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if let Ok(sessions) = self.sessions.lock() {
            for session in sessions.values() {
                if let Ok(mut child) = session.child.lock() {
                    match child.try_wait() {
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            if let Err(error) = child.kill() {
                                eprintln!("Ferryx helper child teardown failed: {error}");
                            }
                            if let Err(error) = child.wait() {
                                eprintln!("Ferryx helper child reap failed: {error}");
                            }
                        }
                        Err(error) => eprintln!("Ferryx helper child status failed: {error}"),
                    }
                }
            }
        }
    }
}

fn is_reserved_agent_var(key: &str) -> bool {
    key.eq_ignore_ascii_case("FERRYX_AGENT_STATE_SOCKET")
        || key.eq_ignore_ascii_case("FERRYX_AGENT_STATE_PORT")
        || key.eq_ignore_ascii_case("FERRYX_AGENT_STATE_TOKEN")
        || key.eq_ignore_ascii_case("FERRYX_SESSION_ID")
}

fn scrub_reserved_env_vars(cmd: &mut CommandBuilder) {
    for (k, _) in std::env::vars() {
        if is_reserved_agent_var(&k) {
            cmd.env_remove(&k);
        }
    }
    for var in [
        "FERRYX_AGENT_STATE_SOCKET",
        "ferryx_agent_state_socket",
        "FERRYX_AGENT_STATE_PORT",
        "ferryx_agent_state_port",
        "FERRYX_AGENT_STATE_TOKEN",
        "ferryx_agent_state_token",
        "FERRYX_SESSION_ID",
        "ferryx_session_id",
    ] {
        cmd.env_remove(var);
    }
}

fn parse_agent_after_revision(p: &Value) -> Result<Option<u64>, String> {
    match p.get("agentAfterRevision") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let n = s.parse::<u64>().map_err(|e| {
                format!("INVALID_REQUEST: agentAfterRevision must be canonical decimal u64: {e}")
            })?;
            if n.to_string() != *s {
                return Err(
                    "INVALID_REQUEST: agentAfterRevision must be canonical decimal u64 string"
                        .into(),
                );
            }
            Ok(Some(n))
        }
        Some(_) => Err(
            "INVALID_REQUEST: agentAfterRevision must be canonical decimal u64 string".into(),
        ),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("INVALID_REQUEST: {key}"))
}

fn parse_u16_dim(val: Option<&Value>, default: u16, name: &str) -> Result<u16, String> {
    match val {
        None => Ok(default),
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| format!("INVALID_REQUEST: {name} must be integer"))?;
            if n == 0 || n > 65535 {
                return Err(format!(
                    "INVALID_REQUEST: {name} must be between 1 and 65535"
                ));
            }
            Ok(n as u16)
        }
    }
}

fn parse_cursor(p: &Value) -> Result<u64, String> {
    if let Some(val) = p.get("cursor") {
        return parse_canonical_u64(val, "cursor");
    }
    if let Some(val) = p.get("afterCursor") {
        return parse_canonical_u64(val, "afterCursor");
    }
    if let Some(val) = p.get("afterSequence") {
        return parse_canonical_u64(val, "afterSequence");
    }
    Ok(0)
}

fn parse_canonical_u64(val: &Value, name: &str) -> Result<u64, String> {
    match val {
        Value::String(s) => {
            let n = s.parse::<u64>().map_err(|e| {
                format!("INVALID_REQUEST: {name} must be canonical decimal u64: {e}")
            })?;
            if n.to_string() != *s {
                return Err(format!(
                    "INVALID_REQUEST: {name} must be canonical decimal u64 string"
                ));
            }
            Ok(n)
        }
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| format!("INVALID_REQUEST: {name} must be non-negative integer")),
        _ => Err(format!("INVALID_REQUEST: {name} must be string or integer")),
    }
}

fn spawn_params_match(a: &Value, b: &Value) -> bool {
    let eq_field = |k: &str| a.get(k) == b.get(k);
    let worktree_a = a.get("worktree").and_then(Value::as_str).unwrap_or(".");
    let worktree_b = b.get("worktree").and_then(Value::as_str).unwrap_or(".");
    let cols_a = a.get("cols").and_then(Value::as_u64).unwrap_or(80);
    let cols_b = b.get("cols").and_then(Value::as_u64).unwrap_or(80);
    let rows_a = a.get("rows").and_then(Value::as_u64).unwrap_or(24);
    let rows_b = b.get("rows").and_then(Value::as_u64).unwrap_or(24);
    eq_field("projectId")
        && worktree_a == worktree_b
        && eq_field("program")
        && eq_field("args")
        && cols_a == cols_b
        && rows_a == rows_b
        && eq_field("env")
}

fn resolve_program_and_args(p: &Value) -> Result<(String, Vec<String>), String> {
    let program_opt = p
        .get("program")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match program_opt {
        Some(prog) => {
            let args = if let Some(args_val) = p.get("args") {
                let arr = args_val
                    .as_array()
                    .ok_or("INVALID_REQUEST: args must be an array")?;
                let mut out = Vec::with_capacity(arr.len());
                for a in arr {
                    out.push(
                        a.as_str()
                            .ok_or("INVALID_REQUEST: argv element must be string")?
                            .to_string(),
                    );
                }
                out
            } else {
                Vec::new()
            };
            Ok((prog.to_string(), args))
        }
        None => {
            let (prog, default_args) = default_platform_login_shell();
            let args = if let Some(args_val) = p.get("args") {
                let arr = args_val
                    .as_array()
                    .ok_or("INVALID_REQUEST: args must be an array")?;
                let mut out = Vec::with_capacity(arr.len());
                for a in arr {
                    out.push(
                        a.as_str()
                            .ok_or("INVALID_REQUEST: argv element must be string")?
                            .to_string(),
                    );
                }
                out
            } else {
                default_args
            };
            Ok((prog, args))
        }
    }
}

fn default_platform_login_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        let shell = std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
        (shell, Vec::new())
    }
    #[cfg(target_os = "macos")]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/zsh".to_string());
        (shell, vec!["-l".to_string()])
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                if Path::new("/bin/bash").exists() {
                    "/bin/bash".to_string()
                } else {
                    "/bin/sh".to_string()
                }
            });
        (shell, vec!["-l".to_string()])
    }
}

// Normalize a process cwd so that callers handing the path to a platform shell
// (e.g. Windows `cmd.exe` via COMSPEC) do not pass a Win32 verbatim
// (`\\?\C:\...`) or verbatim-UNC (`\\?\UNC\server\share`) form that the shell
// rejects and silently falls back to the Windows directory. POSIX paths and
// already-canonical Windows drive paths pass through unchanged.
fn prepare_spawn_cwd(path: &Path) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path.to_path_buf();
    };

    if let Some(rest) = path_str.strip_prefix(r"\\?\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return PathBuf::from(format!(r"\\{}", &rest[4..]));
        }

        let bytes = rest.as_bytes();
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

// Rejects UNC working directories before spawning: the default Windows login
// shell (cmd.exe) cannot use a UNC cwd and silently falls back to the Windows
// directory, so a UNC project would launch outside the requested path. Fail
// explicitly instead. POSIX canonical paths always start with '/', so this
// check never fires there.
fn ensure_cwd_spawnable(cwd: &Path) -> Result<(), String> {
    if cwd.as_os_str().to_string_lossy().starts_with(r"\\") {
        return Err(
            r"UNSUPPORTED: UNC cwd cannot be used by the default cmd.exe shell; map a drive letter (net use X: \\server\share) or launch a UNC-capable shell"
                .to_string(),
        );
    }
    Ok(())
}

#[path = "helper_core_tests.rs"]
#[cfg(test)]
mod helper_core_tests;

#[path = "reboot_recovery_tests.rs"]
#[cfg(test)]
mod reboot_recovery_tests;

#[cfg(test)]
mod ferryx_scope {
    pub mod ssh {
        pub mod helper {
            pub mod prepare_spawn_cwd_tests {
                use super::super::super::super::*;
                use std::path::{Path, PathBuf};

                #[test]
                fn verbatim_drive_path_strips_prefix() {
                    assert_eq!(
                        prepare_spawn_cwd(Path::new(r"\\?\C:\Users\sook\.ferryx\project")),
                        PathBuf::from(r"C:\Users\sook\.ferryx\project")
                    );
                }

                #[test]
                fn verbatim_unc_strips_prefix() {
                    assert_eq!(
                        prepare_spawn_cwd(Path::new(r"\\?\UNC\server\share\repo")),
                        PathBuf::from(r"\\server\share\repo")
                    );
                }

                #[test]
                fn posix_path_unchanged() {
                    assert_eq!(
                        prepare_spawn_cwd(Path::new("/home/sook/repo")),
                        PathBuf::from("/home/sook/repo")
                    );
                }

                #[test]
                fn plain_drive_path_unchanged() {
                    assert_eq!(
                        prepare_spawn_cwd(Path::new(r"C:\Users\sook\repo")),
                        PathBuf::from(r"C:\Users\sook\repo")
                    );
                }

                #[test]
                fn unc_cwd_rejected_with_explicit_error() {
                    let err = ensure_cwd_spawnable(Path::new(r"\\server\share\repo")).unwrap_err();
                    assert!(
                        err.contains("UNSUPPORTED: UNC cwd"),
                        "expected UNC rejection, got: {err}"
                    );
                }

                #[test]
                fn verbatim_unc_normalizes_then_rejects() {
                    let normalized = prepare_spawn_cwd(Path::new(r"\\?\UNC\server\share\repo"));
                    assert_eq!(normalized, PathBuf::from(r"\\server\share\repo"));
                    let err = ensure_cwd_spawnable(&normalized).unwrap_err();
                    assert!(
                        err.contains("UNSUPPORTED: UNC cwd"),
                        "expected UNC rejection after normalization, got: {err}"
                    );
                }

                #[test]
                fn drive_and_posix_cwd_spawnable() {
                    assert!(ensure_cwd_spawnable(Path::new(r"C:\Users\sook\repo")).is_ok());
                    assert!(ensure_cwd_spawnable(Path::new("/home/sook/repo")).is_ok());
                }

                #[test]
                fn rejection_of_cwd_outside_root_forbidden() {
                    let runtime_dir = tempfile::tempdir().unwrap();
                    let project_dir = tempfile::tempdir().unwrap();
                    let runtime = Runtime::new(
                        runtime_dir.path().to_path_buf(),
                        "test-host".to_string(),
                        "tok".to_string(),
                    )
                    .unwrap();
                    runtime
                        .handle(Request {
                            protocol: 1,
                            token: "tok".to_string(),
                            op: "project.register".to_string(),
                            params: json!({
                                "id": "proj-out",
                                "path": project_dir.path().to_string_lossy(),
                            }),
                        })
                        .unwrap();

                    let res = runtime.handle(Request {
                        protocol: 1,
                        token: "tok".to_string(),
                        op: "pty.spawn".to_string(),
                        params: json!({
                            "projectId": "proj-out",
                            "worktree": "..",
                        }),
                    });
                    assert!(res.is_err());
                    let err = res.unwrap_err();
                    assert!(
                        err.contains("FORBIDDEN: cwd outside project"),
                        "expected FORBIDDEN error, got: {err}"
                    );
                }
            }
        }
    }
}
