//! Blocking portable stdio adapter. Call via spawn_blocking from async application code.
use super::helper::{read_frame, write_frame, Request, Runtime};
use serde_json::{json, Value};
use std::{io::{Read, Write}, path::{Path, PathBuf}, sync::Arc};

#[derive(serde::Serialize, serde::Deserialize)]
struct Endpoint { address: String, token: String }

fn reply(runtime: &Runtime, value: Value) -> Value {
    match serde_json::from_value::<Request>(value).map_err(|e|e.to_string()).and_then(|r|runtime.handle(r)) {
        Ok(data) => json!({"ok":true,"data":data}), Err(error) => json!({"ok":false,"error":error}),
    }
}
fn serve(mut stream: impl Read + Write, runtime: Arc<Runtime>) -> Result<(),String> {
    while let Some(request) = read_frame(&mut stream)? { write_frame(&mut stream, &reply(&runtime,request))?; }
    Ok(()) // Bridge EOF releases only this connection; Runtime owns every PTY.
}
fn endpoint(root: &Path) -> Result<Endpoint,String> { serde_json::from_slice(&std::fs::read(root.join("endpoint.json")).map_err(|e| format!("REMOTE_RUNTIME_MISSING: start ferryx-remote-helper daemon --root <private-root>: {e}"))?).map_err(|e|e.to_string()) }

pub fn daemon(root: PathBuf, host: String) -> Result<(),String> {
    std::fs::create_dir_all(&root).map_err(|e|e.to_string())?; super::private_file(&root)?;
    // Never replace an existing runtime endpoint or adopt the normal desktop daemon.
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let path = root.join("endpoint.json");
    let mut file = options.open(&path).map_err(|e|format!("REMOTE_RUNTIME_CONFLICT: {e}"))?; super::private_file(&path)?;
    let token = format!("{}{}",uuid::Uuid::new_v4(),uuid::Uuid::new_v4());
    let runtime = Arc::new(Runtime::new(root.clone(),host,token.clone())?);
    #[cfg(unix)] let (listener, address) = {
        let socket = root.join("helper.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).map_err(|e|e.to_string())?;
        super::private_file(&socket)?; (listener,socket.to_string_lossy().into_owned())
    };
    #[cfg(not(unix))] let (listener, address) = {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).map_err(|e|e.to_string())?;
        let address = listener.local_addr().map_err(|e|e.to_string())?.to_string(); (listener,address)
    };
    file.write_all(&serde_json::to_vec(&Endpoint {address,token}).map_err(|e|e.to_string())?).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    println!("{}",json!({"event":"ready","protocol":1})); std::io::stdout().flush().map_err(|e|e.to_string())?;
    for incoming in listener.incoming() { let stream = incoming.map_err(|e|e.to_string())?; let runtime = runtime.clone();
        std::thread::spawn(move || { if let Err(error) = serve(stream,runtime) { eprintln!("SSH_HELPER_CONNECTION: {error}"); } });
    }
    Ok(())
}

pub fn bridge(root: &Path, mut input: impl Read, mut output: impl Write) -> Result<(),String> {
    let endpoint = endpoint(root)?;
    #[cfg(unix)] let mut stream = std::os::unix::net::UnixStream::connect(endpoint.address).map_err(|e|e.to_string())?;
    #[cfg(not(unix))] let mut stream = {
        let address: std::net::SocketAddr = endpoint.address.parse().map_err(|e: std::net::AddrParseError|e.to_string())?;
        if !address.ip().is_loopback() { return Err("FORBIDDEN: helper IPC must be loopback".into()); }
        std::net::TcpStream::connect(address).map_err(|e|e.to_string())?
    };
    while let Some(mut request) = read_frame(&mut input)? {
        let op = request.get("op").and_then(Value::as_str).unwrap_or("");
        if !matches!(op,"handshake"|"project.register"|"project.list"|"worktree.create"|"pty.spawn"|"pty.list"|"pty.read"|"pty.write"|"pty.resize"|"pty.stop") {
            write_frame(&mut output,&json!({"ok":false,"error":"UNSUPPORTED: operation not allowlisted"}))?; continue;
        }
        request["token"] = json!(endpoint.token);
        write_frame(&mut stream,&request)?;
        let response = read_frame(&mut stream)?.ok_or("REMOTE_UNAVAILABLE: daemon EOF")?;
        write_frame(&mut output,&response)?;
    }
    Ok(())
}

/// Binary entry: integrator calls this instead of any Tauri initialization.
pub fn run(args: impl IntoIterator<Item=String>) -> Result<(),String> {
    let args: Vec<_> = args.into_iter().collect();
    let flag = |name: &str| args.windows(2).find(|pair|pair[0]==name).map(|pair|pair[1].clone());
    let root = flag("--root").or_else(||std::env::var("FERRYX_REMOTE_ROOT").ok()).map(PathBuf::from).ok_or("REMOTE_RUNTIME_MISSING: configure FERRYX_REMOTE_ROOT or --root")?;
    match args.first().map(String::as_str) {
        Some("daemon") => daemon(root,flag("--host-id").ok_or("INVALID_REQUEST: --host-id required")?),
        Some("bridge") if args.iter().any(|arg|arg=="--stdio") => bridge(&root,std::io::stdin().lock(),std::io::stdout().lock()),
        _ => Err("Usage: ferryx-remote-helper daemon --root <private-root> --host-id <id> | bridge --stdio [--root <private-root>]".into()),
    }
}
