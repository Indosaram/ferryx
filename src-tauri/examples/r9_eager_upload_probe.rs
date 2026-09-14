//! Private actual-wire diagnostic, NOT an acceptance test. No PTYs or body injection.
//! Run with --locked --no-default-features; exit 0 means diagnosis and cleanup completed.
use anyhow::{anyhow, Context, Result};
use ferryx_lib::{daemon::server::DaemonServer, remote::{auth::{DeviceAccessScope, DevicePermission}, create_remote_router}};
use serde_json::{json, Value};
use std::{io, net::SocketAddr, path::Path, pin::Pin, sync::{Arc, Mutex}, task::{Context as TaskContext, Poll}, time::Duration};
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf}, net::{TcpListener, TcpStream}};

const IO_LIMIT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Trace { case: String, events: Vec<Value>, connections: usize }
type Log = Arc<Mutex<Trace>>;
fn event(log: &Log, mut value: Value) {
    let mut trace = log.lock().unwrap();
    value["seq"] = json!(trace.events.len());
    value["case"] = json!(trace.case);
    assert!(trace.events.len() < 50_000, "diagnostic event bound");
    trace.events.push(value);
}

// Observe actual successful socket reads/writes; never copy inbound credentials or bodies.
// Forward vectored writes unchanged, preserving the production transport's write shape.
struct ObservedIo { tcp: TcpStream, id: usize, log: Log }
impl AsyncRead for ObservedIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.tcp).poll_read(cx, buf);
        if let Poll::Ready(ref outcome) = result {
            event(&self.log, json!({"event":"server_read", "connection":self.id, "bytes":buf.filled().len()-before, "outcome":format!("{outcome:?}")}));
        }
        result
    }
}
impl AsyncWrite for ObservedIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.tcp).poll_write(cx, buf);
        if let Poll::Ready(ref outcome) = result {
            let n = outcome.as_ref().copied().unwrap_or(0);
            event(&self.log, json!({"event":"server_write", "connection":self.id, "outcome":format!("{outcome:?}"), "wire":String::from_utf8_lossy(&buf[..n])}));
        }
        result
    }
    fn is_write_vectored(&self) -> bool { self.tcp.is_write_vectored() }
    fn poll_write_vectored(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>, bufs: &[io::IoSlice<'_>]) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.tcp).poll_write_vectored(cx, bufs);
        if let Poll::Ready(ref outcome) = result {
            let mut left = outcome.as_ref().copied().unwrap_or(0);
            let mut wire = Vec::new();
            for buf in bufs { let n = left.min(buf.len()); wire.extend_from_slice(&buf[..n]); left -= n; }
            event(&self.log, json!({"event":"server_write", "connection":self.id, "outcome":format!("{outcome:?}"), "wire":String::from_utf8_lossy(&wire)}));
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> { Pin::new(&mut self.tcp).poll_flush(cx) }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.tcp).poll_shutdown(cx);
        if let Poll::Ready(ref outcome) = result { event(&self.log, json!({"event":"server_shutdown", "connection":self.id, "outcome":format!("{outcome:?}")})); }
        result
    }
}
impl Drop for ObservedIo {
    fn drop(&mut self) { event(&self.log, json!({"event":"server_socket_drop", "connection":self.id})); }
}
struct ObservedListener { listener: TcpListener, log: Log }
impl axum::serve::Listener for ObservedListener {
    type Io = ObservedIo;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        // An accept error fails this private fixture; no retry-until-pass loop.
        let (tcp, addr) = self.listener.accept().await.expect("private listener accept");
        let id = { let mut trace = self.log.lock().unwrap(); trace.connections += 1; trace.connections };
        event(&self.log, json!({"event":"server_accept", "connection":id, "peer":addr}));
        (ObservedIo { tcp, id, log:self.log.clone() }, addr)
    }
    fn local_addr(&self) -> io::Result<SocketAddr> { self.listener.local_addr() }
}

fn valid(status: u16, no_store: bool, body: &[u8], expected: u16, code: &str) -> bool {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    status == expected && no_store && value["error"]["code"] == code
        && value["error"]["retryable"] == false
        && value["error"]["requestId"].as_str().is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
        && value["error"]["message"].is_string() && value["error"]["details"].is_object()
}
fn wire_valid(wire: &[u8], expected: u16, code: &str) -> bool {
    let Some(boundary) = wire.windows(4).position(|v| v == b"\r\n\r\n") else { return false; };
    let head = String::from_utf8_lossy(&wire[..boundary]);
    let status = head.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let no_store = head.lines().any(|l| l.eq_ignore_ascii_case("cache-control: no-store"));
    let length = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|v| v.parse::<usize>().ok()));
    length == Some(wire.len()-boundary-4) && valid(status, no_store, &wire[boundary+4..], expected, code)
}

async fn raw(log: &Log, addr: SocketAddr, method: &str, path: &str, size: usize, token: Option<&str>, expected: u16, code: &str, mode: &str) -> Result<()> {
    let staged = mode == "raw-staged";
    let early_fin = mode == "raw-eager-fin";
    let tcp = TcpStream::connect(addr).await?;
    event(log, json!({"event":"raw_connect", "local":tcp.local_addr()?}));
    let (mut reader, mut writer) = tcp.into_split();
    let auth = token.map(|v| format!("Authorization: Bearer {v}\r\n")).unwrap_or_default();
    writer.write_all(format!("{method} /api/v1/{path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Content-Type: application/json\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let upload = if staged { if expected == 413 { 65_537 } else { 0 } } else { size };
    let send = async {
        let body = vec![b' '; upload];
        let mut sent = 0;
        let result = tokio::time::timeout(IO_LIMIT, async {
            while sent < body.len() {
                let n = writer.write(&body[sent..]).await?;
                if n == 0 { return Err(io::Error::new(io::ErrorKind::WriteZero, "raw upload write zero")); }
                sent += n;
                event(log, json!({"event":"raw_body_write", "accepted_bytes":sent}));
            }
            if early_fin { writer.shutdown().await?; event(log, json!({"event":"raw_half_close", "boundary":"full_upload"})); }
            Ok::<_, io::Error>(())
        }).await;
        event(log, json!({"event":"raw_upload_outcome", "attempted_bytes":upload, "accepted_bytes":sent, "outcome":format!("{result:?}")}));
        result.is_ok_and(|v| v.is_ok())
    };
    let receive = async {
        let mut wire = Vec::new();
        let result = tokio::time::timeout(IO_LIMIT, async {
            loop {
                let mut buffer = [0; 4096];
                let n = reader.read(&mut buffer).await?;
                event(log, json!({"event":"raw_read", "bytes":n}));
                if n == 0 { break; }
                wire.extend_from_slice(&buffer[..n]);
                if wire.len() > 8192 { return Err(io::Error::other("response bound exceeded")); }
            }
            Ok::<_, io::Error>(())
        }).await;
        let complete = wire_valid(&wire, expected, code);
        let received = result.as_ref().is_ok_and(|v| v.is_ok()) && complete;
        event(log, json!({"event":"raw_response_outcome", "outcome":format!("{result:?}"), "wire":String::from_utf8_lossy(&wire), "complete_valid_http_bytes":complete, "received_http_without_read_error":received}));
        received
    };
    let (sent, received) = tokio::join!(send, receive);
    if !early_fin {
        let result = tokio::time::timeout(IO_LIMIT, writer.shutdown()).await;
        event(log, json!({"event":"raw_half_close", "boundary":"after_response", "outcome":format!("{result:?}")}));
    }
    event(log, json!({"event":"raw_case", "transport_and_http_valid":sent && received}));
    Ok(())
}

fn client(fresh: bool) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder().no_proxy().http1_only().redirect(reqwest::redirect::Policy::none()).timeout(IO_LIMIT);
    Ok(if fresh { builder.pool_max_idle_per_host(0) } else { builder }.build()?)
}
async fn requests(log: &Log, addr: SocketAddr, grants: &[String]) -> Result<()> {
    for (method, path, size) in [("POST", "workspace/projects", 65_537), ("POST", "workspace/projects", 2_097_153), ("DELETE", "workspace/projects/missing", 2_097_153)] {
        for (label, token, expected, code) in [("machine", Some(grants[0].as_str()), 413, "PAYLOAD_TOO_LARGE"), ("mirror", Some(grants[1].as_str()), 403, "MACHINE_ACCESS_REQUIRED"), ("anonymous", None, 401, "UNAUTHORIZED")] {
            for mode in ["raw-eager", "raw-eager-fin", "raw-staged", "reqwest-fresh", "reqwest-pooled"] {
                let name = format!("{method}-{size}-{label}-{mode}");
                log.lock().unwrap().case = name;
                event(log, json!({"event":"case_start", "method":method, "size":size, "auth":label, "mode":mode, "expected":expected}));
                if mode.starts_with("raw") {
                    raw(log, addr, method, path, size, token, expected, code, mode).await?;
                } else {
                    let client = client(mode == "reqwest-fresh")?;
                    if mode == "reqwest-pooled" {
                        // A completed health response provides an explicit reusable connection;
                        // server connection IDs, not the client label, prove actual reuse.
                        let response = client.get(format!("http://{addr}/api/v1/health")).send().await?;
                        let status = response.status().as_u16();
                        response.bytes().await?;
                        event(log, json!({"event":"pool_prime_completed", "status":status}));
                    }
                    let mut request = client.request(method.parse()?, format!("http://{addr}/api/v1/{path}")).header("content-type", "application/json").body(vec![b' '; size]);
                    if let Some(token) = token { request = request.bearer_auth(token); }
                    match request.send().await {
                        Err(error) => event(log, json!({"event":"reqwest_outcome", "error_debug":format!("{error:?}"), "error_display":error.to_string(), "received_http":false})),
                        Ok(response) => {
                            let status = response.status().as_u16();
                            let headers = format!("{:?}", response.headers());
                            let no_store = response.headers().get("cache-control").is_some_and(|v| v == "no-store");
                            match response.bytes().await {
                                Ok(body) => event(log, json!({"event":"reqwest_outcome", "status":status, "headers":headers, "body":String::from_utf8_lossy(&body), "received_http":true, "valid":valid(status, no_store, &body, expected, code)})),
                                Err(error) => event(log, json!({"event":"reqwest_outcome", "status":status, "headers":headers, "body_error_debug":format!("{error:?}"), "received_http":false})),
                            }
                        }
                    }
                    drop(client);
                }
            }
        }
    }
    Ok(())
}

async fn scenario(root: &Path, log: &Log) -> Result<()> {
    use ferryx_lib::daemon::protocol::{DaemonRequest, DaemonResponse};
    use tokio::io::{AsyncBufReadExt, BufReader};
    let owner = Arc::new(DaemonServer::new_with_paths(Some(root.join("data/config.json")), Some(root.join("data/auth.json"))));
    let state = owner.remote_state().clone();
    // Do not call RemoteConfigure/start: setting the private fixture mode avoids
    // the owner pairing action's Off -> public relay auto-start branch.
    state.config.write().mode = ferryx_lib::remote::state::RemoteNetworkMode::LocalNetwork;
    let mut grants = Vec::new();
    for scope in [DeviceAccessScope::Machine, DeviceAccessScope::Mirror] {
        let (client, service) = tokio::io::duplex(8192);
        let task = tokio::spawn(owner.clone().handle_client(service));
        let mut client = BufReader::new(client);
        let request = if scope == DeviceAccessScope::Machine { DaemonRequest::RemoteCreateMachinePairingCode }
            else { DaemonRequest::RemoteCreatePairingCode { permission:Some(DevicePermission::Control) } };
        client.write_all(format!("{}\n", serde_json::to_string(&request)?).as_bytes()).await?;
        let mut line = String::new();
        tokio::time::timeout(IO_LIMIT, client.read_line(&mut line)).await??;
        let response: DaemonResponse = serde_json::from_str(&line)?;
        drop(client);
        tokio::time::timeout(IO_LIMIT, task).await? ?;
        let DaemonResponse::RemotePairingCodeOk { code:pin, .. } = response else { return Err(anyhow!("owner pairing action failed")); };
        let (token, device) = state.auth_manager.exchange_pairing_code(&pin, "r9-private").map_err(|e| anyhow!("{e:?}"))?;
        anyhow::ensure!(device.access_scope == scope && device.permission == DevicePermission::Control, "owner issued wrong grant");
        event(log, json!({"event":"grant", "scope":device.access_scope, "permission":device.permission}));
        grants.push(token);
    }
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let observed = ObservedListener { listener, log:log.clone() };
    let mut server = tokio::spawn(async move {
        axum::serve(observed, create_remote_router(state)).with_graceful_shutdown(async { stopped.await.expect("private shutdown signal"); }).await
    });
    let result = tokio::time::timeout(Duration::from_secs(120), requests(log, addr, &grants)).await;
    stop.send(()).map_err(|_| anyhow!("listener exited before shutdown"))?;
    let joined = tokio::time::timeout(IO_LIMIT, &mut server).await;
    let shutdown_ok = match joined {
        Ok(result) => { result??; true }
        Err(error) => { server.abort(); let outcome = server.await; event(log, json!({"event":"server_abort", "deadline":format!("{error:?}"), "joined":format!("{outcome:?}")})); false }
    };
    let refused = TcpStream::connect(addr).await.is_err();
    drop(owner);
    event(log, json!({"event":"listener_cleanup", "joined":true, "graceful":shutdown_ok, "connection_refused":refused, "no_pty":true}));
    result.context("scenario deadline")??;
    anyhow::ensure!(shutdown_ok && refused, "listener cleanup failed");
    Ok(())
}

fn main() -> Result<()> {
    if let Some(root) = std::env::var_os("R9_PRIVATE_ROOT") {
        let root = std::path::PathBuf::from(root);
        for key in ["HOME", "FERRYX_RUNTIME_DIR", "FERRYX_DATA_DIR", "FERRYX_SESSION_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR"] {
            anyhow::ensure!(std::path::PathBuf::from(std::env::var_os(key).context("missing isolated environment")?).starts_with(&root), "nonprivate environment: {key}");
        }
        let log = Log::default();
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
        let result = runtime.block_on(scenario(&root, &log));
        drop(runtime);
        for value in &log.lock().unwrap().events { println!("{value}"); }
        println!("{}", json!({"event":"runtime_cleanup", "joined":true, "outcome":format!("{result:?}")}));
        return result;
    }
    // Parent has not constructed any library objects. Set child environment before exec,
    // keeping library lazy globals and OS home resolution away from canonical state.
    let target = std::env::current_dir()?.join("src-tauri/target");
    let root = tempfile::Builder::new().prefix("r9-private-").tempdir_in(target)?;
    let path = root.path().to_owned();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let result = runtime.block_on(async {
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command.env("R9_PRIVATE_ROOT", &path).current_dir(&path).kill_on_drop(true);
        for (key, sub) in [("HOME","home"), ("FERRYX_RUNTIME_DIR","runtime"), ("FERRYX_DATA_DIR","data"), ("FERRYX_SESSION_DIR","sessions"), ("XDG_CONFIG_HOME","home/config"), ("XDG_DATA_HOME","home/data"), ("XDG_CACHE_HOME","home/cache"), ("TMPDIR","tmp")] {
            std::fs::create_dir_all(path.join(sub))?;
            command.env(key, path.join(sub));
        }
        let mut child = command.spawn()?;
        let pid = child.id();
        let status = match tokio::time::timeout(Duration::from_secs(150), child.wait()).await {
            Ok(status) => status?,
            Err(error) => { child.kill().await?; let status = child.wait().await?; println!("{}", json!({"event":"child_deadline", "error":format!("{error:?}"), "status":status.to_string()})); status }
        };
        println!("{}", json!({"event":"child_cleanup", "pid":pid, "reaped":true, "status":status.to_string()}));
        anyhow::ensure!(status.success(), "diagnostic child failed: {status}");
        Ok::<_, anyhow::Error>(())
    });
    drop(runtime);
    root.close()?;
    println!("{}", json!({"event":"root_cleanup", "removed":!path.exists(), "exit_zero_means":"diagnostic completion only; inspect all HTTP and transport failures"}));
    result
}
