//! Standalone headless helper. No GUI initialization or existing daemon adoption.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, VecDeque}, io::{Read, Write}, path::{Path, PathBuf}, sync::{Arc, Mutex, Condvar}};
use portable_pty::{CommandBuilder, PtySize};
use crate::scoped_contracts::{Epoch, TargetRef};
pub const MAX_FRAME: usize = 1024 * 1024;
pub const RING_BYTES: usize = 512 * 1024;
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request { pub protocol: u32, pub token: String, pub op: String, #[serde(default)] pub params: Value }
pub fn read_frame(reader: &mut impl Read) -> Result<Option<Value>, String> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) { Ok(0) => return Ok(None), Ok(_) => (), Err(e) => return Err(e.to_string()) }
    reader.read_exact(&mut header[1..]).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(header) as usize;
    if len > MAX_FRAME { return Err("INVALID_REQUEST: frame exceeds 1 MiB".into()); }
    let mut bytes = vec![0; len]; reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map(Some).map_err(|e| e.to_string())
}
pub fn write_frame(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FRAME { return Err("INVALID_REQUEST: frame exceeds 1 MiB".into()); }
    writer.write_all(&(bytes.len() as u32).to_be_bytes()).and_then(|_| writer.write_all(&bytes)).and_then(|_| writer.flush()).map_err(|e| e.to_string())
}
struct Output { next: u64, bytes: usize, chunks: VecDeque<(u64, Vec<u8>)>, exited: bool }
struct Session {
    target: TargetRef, pid: u32, cwd: PathBuf,
    master: Box<dyn portable_pty::MasterPty + Send>, writer: Box<dyn Write + Send>, child: Box<dyn portable_pty::Child + Send + Sync>,
    output: Arc<(Mutex<Output>, Condvar)>,
}
pub struct Runtime { root: PathBuf, host: String, owner: String, epoch: Epoch, token: String, sessions: Mutex<HashMap<String, Session>>, projects: Mutex<HashMap<String, PathBuf>> }
impl Runtime {
    pub fn new(root: PathBuf, host: String, token: String) -> Result<Self, String> {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?; super::private_file(&root)?;
        Ok(Self { root: root.canonicalize().map_err(|e| e.to_string())?, host, owner: uuid::Uuid::new_v4().to_string(), epoch: Epoch(rand::random()), token, sessions: Mutex::new(HashMap::new()), projects: Mutex::new(HashMap::new()) })
    }
    pub fn handle(&self, request: Request) -> Result<Value, String> {
        if request.protocol != 1 { return Err("UNSUPPORTED: helper protocol requires version 1".into()); }
        if request.token != self.token { return Err("UNAUTHORIZED".into()); }
        self.dispatch(&request.op, &request.params)
    }
    fn dispatch(&self, op: &str, p: &Value) -> Result<Value, String> {
        match op {
            "handshake" => Ok(json!({"protocol":1,"capabilities":["sshHelperV1"],"hostId":self.host,"ownerId":self.owner,"epoch":self.epoch,"os":std::env::consts::OS,"arch":std::env::consts::ARCH})),
            "project.register" => {
                let id = text(p, "id")?; let path = Path::new(text(p,"path")?).canonicalize().map_err(|e| e.to_string())?;
                if !path.starts_with(&self.root) || !path.is_dir() { return Err("FORBIDDEN: project outside configured runtime root".into()); }
                let mut projects = self.projects.lock().map_err(|e| e.to_string())?;
                if projects.get(id).is_some_and(|old| old != &path) { return Err("REQUEST_CONFLICT: project target is immutable".into()); }
                projects.insert(id.into(), path); Ok(json!({"projectId":id}))
            }
            "project.list" => Ok(json!(self.projects.lock().map_err(|e| e.to_string())?.keys().collect::<Vec<_>>())),
            "worktree.create" => {
                let projects = self.projects.lock().map_err(|e| e.to_string())?;
                let repo = projects.get(text(p,"projectId")?).ok_or("NOT_FOUND")?;
                let slug = text(p,"slug")?;
                if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err("INVALID_REQUEST: invalid slug".into()); }
                let base = repo.join(".orca-worktrees"); std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
                if !base.canonicalize().map_err(|e| e.to_string())?.starts_with(repo) { return Err("FORBIDDEN".into()); }
                let destination = base.join(format!("wt-{slug}"));
                let output = std::process::Command::new("git").arg("-C").arg(repo).args(["worktree","add","-b", &format!("orca/{slug}"), "--"]).arg(&destination).arg("HEAD").output().map_err(|e| e.to_string())?;
                if !output.status.success() { return Err(format!("REMOTE_GIT_FAILED: {}", String::from_utf8_lossy(&output.stderr))); }
                Ok(json!({"projectId":text(p,"projectId")?,"worktree":format!(".orca-worktrees/wt-{slug}")}))
            }
            "pty.spawn" => {
                let projects = self.projects.lock().map_err(|e| e.to_string())?;
                let root = projects.get(text(p,"projectId")?).ok_or("NOT_FOUND")?;
                let cwd = root.join(p.get("worktree").and_then(Value::as_str).unwrap_or(".")).canonicalize().map_err(|e| e.to_string())?;
                if !cwd.starts_with(root) { return Err("FORBIDDEN: cwd outside project".into()); }
                let pair = portable_pty::native_pty_system().openpty(PtySize { rows:24, cols:80, pixel_width:0,pixel_height:0 }).map_err(|e| e.to_string())?;
                let mut command = CommandBuilder::new(text(p,"program")?);
                if let Some(args) = p.get("args") { for arg in args.as_array().ok_or("INVALID_REQUEST: args")? { command.arg(arg.as_str().ok_or("INVALID_REQUEST: argv")?); } }
                command.cwd(&cwd);
                let child = pair.slave.spawn_command(command).map_err(|e| e.to_string())?; drop(pair.slave);
                let pid = child.process_id().ok_or("REMOTE_SPAWN_FAILED: no PID")?;
                let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
                let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
                let output = Arc::new((Mutex::new(Output {next:1,bytes:0,chunks:VecDeque::new(),exited:false}),Condvar::new()));
                let sink = output.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0;8192];
                    loop { let read = reader.read(&mut buffer); let (lock, signal) = &*sink; let mut state = lock.lock().expect("output mutex poisoned");
                        match read { Ok(n) if n > 0 => { let seq = state.next; state.next += 1; state.bytes += n; state.chunks.push_back((seq,buffer[..n].to_vec())); while state.bytes > RING_BYTES { let (_, bytes) = state.chunks.pop_front().unwrap(); state.bytes -= bytes.len(); } },
                        _ => { state.exited = true; signal.notify_all(); break; } }
                        signal.notify_all();
                    }
                });
                let id = uuid::Uuid::new_v4().to_string();
                let target = TargetRef { host_id:self.host.clone(),owner_id:self.owner.clone(),epoch:self.epoch,backend_session_id:id.clone() };
                self.sessions.lock().map_err(|e| e.to_string())?.insert(id,Session {target:target.clone(),pid,cwd,master:pair.master,writer,child,output});
                Ok(json!({"target":target,"pid":pid}))
            }
            "pty.list" => Ok(json!(self.sessions.lock().map_err(|e| e.to_string())?.values().map(|s| json!({"target":s.target,"pid":s.pid})).collect::<Vec<_>>())),
            "pty.read" | "pty.write" | "pty.resize" | "pty.stop" => {
                let target: TargetRef = serde_json::from_value(p.get("target").cloned().ok_or("INVALID_REQUEST: target required")?).map_err(|e| e.to_string())?;
                if target.host_id != self.host || target.owner_id != self.owner || target.epoch != self.epoch { return Err("TARGET_EXPIRED".into()); }
                let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
                let session = sessions.get_mut(&target.backend_session_id).ok_or("NOT_FOUND")?;
                match op {
                    "pty.write" => { session.writer.write_all(text(p,"text")?.as_bytes()).and_then(|_| session.writer.flush()).map_err(|e| e.to_string())?; Ok(json!({"accepted":true})) }
                    "pty.resize" => { let size = |key| p.get(key).and_then(Value::as_u64).filter(|n| *n > 0 && *n <= 65535).map(|n| n as u16).ok_or("INVALID_REQUEST: size"); session.master.resize(PtySize {rows:size("rows")?,cols:size("cols")?,pixel_width:0,pixel_height:0}).map_err(|e| e.to_string())?; Ok(json!({})) }
                    "pty.stop" => { if session.child.try_wait().map_err(|e| e.to_string())?.is_none() { session.child.kill().map_err(|e| e.to_string())?; session.child.wait().map_err(|e| e.to_string())?; } Ok(json!({"stopped":true})) }
                    _ => {
                        let output = session.output.clone(); let pid = session.pid; let cwd = session.cwd.clone(); drop(sessions);
                        let after = p.get("afterSequence").and_then(Value::as_u64).unwrap_or(0);
                        let wait = p.get("waitMs").and_then(Value::as_u64).unwrap_or(0).min(10000);
                        let (lock, signal) = &*output; let state = lock.lock().map_err(|e| e.to_string())?;
                        let (state, _) = signal.wait_timeout_while(state, std::time::Duration::from_millis(wait), |s| s.next <= after.saturating_add(1) && !s.exited).map_err(|e| e.to_string())?;
                        let first = state.chunks.front().map(|(seq,_)| *seq).unwrap_or(state.next);
                        Ok(json!({"target":target,"pid":pid,"cwd":cwd,"afterSequence":state.next-1,"gap":after.saturating_add(1)<first,"exited":state.exited,"chunks":state.chunks.iter().filter(|(seq,_)| *seq>after).map(|(seq,bytes)|json!({"sequence":seq,"bytes":bytes})).collect::<Vec<_>>()}))
                    }
                }
            }
            _ => Err("UNSUPPORTED: operation not allowlisted".into()),
        }
    }
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str,String> { value.get(key).and_then(Value::as_str).ok_or_else(||format!("INVALID_REQUEST: {key}")) }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_pty_survives_bridge_eof_and_replays() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path().to_owned(), "qa".into(), "private".into()).unwrap();
        let call = |op: &str, params| runtime.handle(Request { protocol: 1, token: "private".into(), op: op.into(), params });
        let project = dir.path().join("repo"); std::fs::create_dir(&project).unwrap();
        call("project.register", json!({"id":"p", "path": project})).unwrap();
        let spawned = call("pty.spawn", json!({"projectId":"p", "program": if cfg!(windows) {"cmd.exe"} else {"/bin/sh"}, "args":[]})).unwrap();
        let target = spawned["target"].clone();
        let mut eof = std::io::Cursor::new(Vec::<u8>::new()); assert!(read_frame(&mut eof).unwrap().is_none());
        call("pty.write", json!({"target":target, "text":"printf 'FERRYX_RETAINED:'; pwd\n"})).unwrap();
        let replay = call("pty.read", json!({"target":target,"afterSequence":0,"waitMs":2000})).unwrap();
        assert_eq!(replay["pid"], spawned["pid"]); assert_eq!(replay["target"], target);
        assert!(!replay["chunks"].as_array().unwrap().is_empty());
        call("pty.stop", json!({"target":target})).unwrap();
        assert!(call("Shutdown", json!({})).is_err());
    }
}
