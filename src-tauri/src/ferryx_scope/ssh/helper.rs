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
    reader.read_exact(&mut header[1..]).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME {
        return Err("INVALID_REQUEST: frame exceeds 1 MiB".into());
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map(Some).map_err(|e| e.to_string())
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

struct Output {
    next: u64,
    bytes: usize,
    chunks: VecDeque<(u64, Vec<u8>)>,
    exited: bool,
}

struct Session {
    target: TargetRef,
    pid: u32,
    cwd: PathBuf,
    cols: u16,
    rows: u16,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
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
    InProgress {
        params: Value,
    },
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
                if matches!(spawns.get(&self.req_id), Some(SpawnState::InProgress { .. })) {
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
    sessions: Mutex<HashMap<String, Session>>,
    projects: Mutex<HashMap<String, PathBuf>>,
    spawns: Mutex<HashMap<String, SpawnState>>,
    spawns_cv: Condvar,
    #[cfg(test)]
    spawn_hook: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
}

impl Runtime {
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg(test)]
    pub fn set_spawn_hook(&self, hook: Option<Arc<dyn Fn(&str) + Send + Sync>>) {
        *self.spawn_hook.lock().unwrap() = hook;
    }

    pub fn new(root: PathBuf, host: String, token: String) -> Result<Self, String> {
        super::private_file(&root)?;
        Ok(Self {
            root: root.canonicalize().map_err(|e| e.to_string())?,
            host,
            owner: uuid::Uuid::new_v4().to_string(),
            epoch: Epoch(rand::random()),
            token,
            sessions: Mutex::new(HashMap::new()),
            projects: Mutex::new(HashMap::new()),
            spawns: Mutex::new(HashMap::new()),
            spawns_cv: Condvar::new(),
            #[cfg(test)]
            spawn_hook: Mutex::new(None),
        })
    }

    pub fn handle(&self, request: Request) -> Result<Value, String> {
        if request.protocol != 1 {
            return Err("UNSUPPORTED: helper protocol requires version 1".into());
        }
        if request.token != self.token {
            return Err("UNAUTHORIZED".into());
        }
        self.dispatch(&request.op, &request.params)
    }

    fn dispatch(&self, op: &str, p: &Value) -> Result<Value, String> {
        match op {
            "handshake" => Ok(json!({
                "protocol": 1,
                "capabilities": ["sshHelperV1"],
                "hostId": self.host,
                "ownerId": self.owner,
                "epoch": self.epoch,
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
            })),
            "project.register" => {
                let id = text(p, "id")?;
                let raw_path = text(p, "path")?;
                let path = Path::new(raw_path)
                    .canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid project path: {e}"))?;
                if !path.is_dir() {
                    return Err("INVALID_REQUEST: project path must be an existing directory".into());
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
                if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err("INVALID_REQUEST: invalid slug".into());
                }
                let base = repo.join(".orca-worktrees");
                std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
                if !base.canonicalize().map_err(|e| e.to_string())?.starts_with(repo) {
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
                    return Err(format!("REMOTE_GIT_FAILED: {}", String::from_utf8_lossy(&output.stderr)));
                }
                Ok(json!({
                    "projectId": text(p, "projectId")?,
                    "worktree": format!(".orca-worktrees/wt-{slug}"),
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
                                spawns = self.spawns_cv.wait_timeout(spawns, std::time::Duration::from_secs(10)).map_err(|e| e.to_string())?.0;
                            }
                            None => {
                                spawns.insert(
                                    req_id.to_string(),
                                    SpawnState::InProgress {
                                        params: p.clone(),
                                    },
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
                let worktree_rel = p.get("worktree").and_then(Value::as_str).unwrap_or(".");
                let cwd = root
                    .join(worktree_rel)
                    .canonicalize()
                    .map_err(|e| format!("INVALID_REQUEST: invalid cwd: {e}"))?;
                if !cwd.starts_with(root) {
                    return Err("FORBIDDEN: cwd outside project".into());
                }

                let cols = parse_u16_dim(p.get("cols"), 80, "cols")?;
                let rows = parse_u16_dim(p.get("rows"), 24, "rows")?;
                let (program, args) = resolve_program_and_args(p)?;

                let pair = portable_pty::native_pty_system().openpty(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                }).map_err(|e| e.to_string())?;

                let id = uuid::Uuid::new_v4().to_string();
                let mut command = CommandBuilder::new(&program);
                for arg in &args {
                    command.arg(arg);
                }
                command.cwd(&cwd);
                command.env("FERRYX_SESSION_ID", &id);
                if std::env::var("TERM").map(|t| t == "dumb" || t.is_empty()).unwrap_or(true) {
                    command.env("TERM", "xterm-256color");
                }
                if let Some(env_obj) = p.get("env").and_then(Value::as_object) {
                    for (k, v) in env_obj {
                        if let Some(s) = v.as_str() {
                            command.env(k, s);
                        }
                    }
                }

                let child = pair.slave.spawn_command(command).map_err(|e| e.to_string())?;
                drop(pair.slave);
                let pid = child.process_id().ok_or("REMOTE_SPAWN_FAILED: no PID")?;

                let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
                let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
                let output = Arc::new((
                    Mutex::new(Output {
                        next: 1,
                        bytes: 0,
                        chunks: VecDeque::new(),
                        exited: false,
                    }),
                    Condvar::new(),
                ));
                let sink = output.clone();
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
                });

                let target = TargetRef {
                    host_id: self.host.clone(),
                    owner_id: self.owner.clone(),
                    epoch: self.epoch,
                    backend_session_id: id.clone(),
                };

                self.sessions.lock().map_err(|e| e.to_string())?.insert(
                    id,
                    Session {
                        target: target.clone(),
                        pid,
                        cwd,
                        cols,
                        rows,
                        master: pair.master,
                        writer,
                        child,
                        output,
                    },
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
            "pty.list" => {
                let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                let list: Vec<_> = sessions
                    .values()
                    .map(|s| {
                        let (lock, _) = &*s.output;
                        let (cursor, exited) = if let Ok(state) = lock.lock() {
                            (state.next.saturating_sub(1).to_string(), state.exited)
                        } else {
                            ("0".to_string(), false)
                        };
                        json!({
                            "target": s.target,
                            "pid": s.pid,
                            "cwd": s.cwd,
                            "cols": s.cols,
                            "rows": s.rows,
                            "cursor": cursor,
                            "exited": exited,
                        })
                    })
                    .collect();
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

                let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                let session = sessions.get_mut(&target.backend_session_id).ok_or("NOT_FOUND")?;

                match op {
                    "pty.describe" => {
                        let output = session.output.clone();
                        let pid = session.pid;
                        let cwd = session.cwd.clone();
                        let cols = session.cols;
                        let rows = session.rows;
                        drop(sessions);

                        let (lock, _) = &*output;
                        let state = lock.lock().map_err(|e| e.to_string())?;
                        let cursor = state.next.saturating_sub(1).to_string();
                        Ok(json!({
                            "target": target,
                            "pid": pid,
                            "cwd": cwd,
                            "cols": cols,
                            "rows": rows,
                            "cursor": cursor,
                            "exited": state.exited,
                        }))
                    }
                    "pty.write" => {
                        let bytes: Vec<u8> = if let Some(b64) = p.get("data").and_then(Value::as_str) {
                            BASE64_STANDARD
                                .decode(b64)
                                .map_err(|e| format!("INVALID_REQUEST: invalid base64 data: {e}"))?
                        } else if let Some(txt) = p.get("text").and_then(Value::as_str) {
                            txt.as_bytes().to_vec()
                        } else {
                            return Err("INVALID_REQUEST: data (base64) or text required".into());
                        };
                        session
                            .writer
                            .write_all(&bytes)
                            .and_then(|_| session.writer.flush())
                            .map_err(|e| e.to_string())?;
                        Ok(json!({ "accepted": true }))
                    }
                    "pty.resize" => {
                        let cols = parse_u16_dim(p.get("cols"), 0, "cols")?;
                        let rows = parse_u16_dim(p.get("rows"), 0, "rows")?;
                        if cols == 0 || rows == 0 {
                            return Err("INVALID_REQUEST: cols and rows must be between 1 and 65535".into());
                        }
                        session
                            .master
                            .resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            })
                            .map_err(|e| e.to_string())?;
                        session.cols = cols;
                        session.rows = rows;
                        Ok(json!({ "cols": cols, "rows": rows }))
                    }
                    "pty.stop" => {
                        if session.child.try_wait().map_err(|e| e.to_string())?.is_none() {
                            session.child.kill().map_err(|e| e.to_string())?;
                            session.child.wait().map_err(|e| e.to_string())?;
                        }
                        Ok(json!({ "stopped": true }))
                    }
                    _ => {
                        let output = session.output.clone();
                        let pid = session.pid;
                        let cwd = session.cwd.clone();
                        drop(sessions);

                        let after = parse_cursor(p)?;
                        let wait = p.get("waitMs").and_then(Value::as_u64).unwrap_or(0).min(10000);

                        let (lock, signal) = &*output;
                        let state = lock.lock().map_err(|e| e.to_string())?;
                        let (state, _) = signal
                            .wait_timeout_while(
                                state,
                                std::time::Duration::from_millis(wait),
                                |s| s.next <= after.saturating_add(1) && !s.exited,
                            )
                            .map_err(|e| e.to_string())?;

                        let first = state.chunks.front().map(|(seq, _)| *seq).unwrap_or(state.next);
                        let gap = after.saturating_add(1) < first;

                        let max_response_len: usize = 900 * 1024;
                        let mut response_chunks = Vec::new();
                        let mut current_size_estimate = 256;
                        let mut last_seq = after;

                        for (seq, chunk_bytes) in state.chunks.iter().filter(|(seq, _)| *seq > after) {
                            let b64 = BASE64_STANDARD.encode(chunk_bytes);
                            let chunk_est = b64.len() + chunk_bytes.len() * 4 + 80;
                            if !response_chunks.is_empty() && current_size_estimate + chunk_est > max_response_len {
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

                        while serde_json::to_vec(&response).map(|v| v.len()).unwrap_or(0) >= MAX_FRAME
                            && !response["chunks"].as_array().unwrap().is_empty()
                        {
                            if let Some(arr) = response.get_mut("chunks").and_then(Value::as_array_mut) {
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
            let n = v.as_u64().ok_or_else(|| format!("INVALID_REQUEST: {name} must be integer"))?;
            if n == 0 || n > 65535 {
                return Err(format!("INVALID_REQUEST: {name} must be between 1 and 65535"));
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
            let n = s
                .parse::<u64>()
                .map_err(|e| format!("INVALID_REQUEST: {name} must be canonical decimal u64: {e}"))?;
            if n.to_string() != *s {
                return Err(format!("INVALID_REQUEST: {name} must be canonical decimal u64 string"));
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

#[path = "helper_core_tests.rs"]
#[cfg(test)]
mod helper_core_tests;
