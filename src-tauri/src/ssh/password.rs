//! Transient OpenSSH askpass credentials. Only a random capability crosses into
//! child environment; passwords never enter argv, environment, or files.
use super::{SshAuthMethod, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Password(String);
impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}
impl Password {
    pub fn new(value: String) -> Self {
        Self(value)
    }
}
struct Credential {
    password: Password,
    token: String,
}
static STORE: OnceLock<Mutex<HashMap<String, Credential>>> = OnceLock::new();
#[cfg(test)]
#[path = "password_tests.rs"]
mod tests;
fn store() -> &'static Mutex<HashMap<String, Credential>> {
    STORE.get_or_init(Default::default)
}
fn key(host: &SshHost) -> String {
    format!("{}|{}", host.key(), host.jump_host.as_deref().unwrap_or(""))
}
fn error(message: &str) -> IpcError {
    super::runtime::error(IpcErrorCode::IoError, "authentication", message)
}
pub fn set(host: &SshHost, password: Password) -> Result<(), IpcError> {
    super::direct::validate_host(host)?;
    if host.auth_method != SshAuthMethod::Password
        || password.0.is_empty()
        || password.0.len() > 4096
        || password.0.contains(['\n', '\r', '\0'])
    {
        return Err(error("Enter a nonempty SSH password without line breaks"));
    }
    store()
        .lock()
        .map_err(|_| IpcError::internal("SSH credential lock failed"))?
        .insert(
            key(host),
            Credential {
                password,
                token: uuid::Uuid::new_v4().to_string(),
            },
        );
    broker()?;
    Ok(())
}
pub fn clear(host: &SshHost) -> Result<(), IpcError> {
    store()
        .lock()
        .map_err(|_| IpcError::internal("SSH credential lock failed"))?
        .remove(&key(host));
    Ok(())
}
pub fn generation(host: &SshHost) -> Result<Option<String>, IpcError> {
    Ok(store()
        .lock()
        .map_err(|_| IpcError::internal("SSH credential lock failed"))?
        .get(&key(host))
        .map(|v| v.token.clone()))
}
pub fn clear_generation(host: &SshHost, generation: &str) -> Result<bool, IpcError> {
    let mut store = store()
        .lock()
        .map_err(|_| IpcError::internal("SSH credential lock failed"))?;
    if store.get(&key(host)).is_some_and(|v| v.token == generation) {
        store.remove(&key(host));
        return Ok(true);
    }
    Ok(false)
}
pub fn require(host: &SshHost) -> Result<(), IpcError> {
    if host.auth_method == SshAuthMethod::Password
        && !store()
            .lock()
            .map_err(|_| IpcError::internal("SSH credential lock failed"))?
            .contains_key(&key(host))
    {
        return Err(error("SSH password required; enter it in machine settings"));
    }
    Ok(())
}
static BROKER: OnceLock<Result<u16, String>> = OnceLock::new();
fn broker() -> Result<u16, IpcError> {
    BROKER
        .get_or_init(|| {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|e| {
                tracing::warn!("SSH askpass broker could not bind a loopback listener: {e}");
                e.to_string()
            })?;
            let port = listener.local_addr().map_err(|e| e.to_string())?.port();
            std::thread::Builder::new()
                .name("ssh-askpass".into())
                .spawn(move || {
                    for incoming in listener.incoming() {
                        let Ok(mut stream) = incoming else { break };
                        if let Err(e) = serve(&mut stream) {
                            tracing::debug!("SSH askpass request rejected: {e}");
                        }
                    }
                })
                .map_err(|e| {
                    tracing::warn!("SSH askpass broker thread could not start: {e}");
                    e.to_string()
                })?;
            tracing::debug!("SSH askpass broker listening on 127.0.0.1:{port}");
            Ok(port)
        })
        .clone()
        .map_err(IpcError::internal)
}
fn serve(stream: &mut TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut token = [0; 36];
    stream.read_exact(&mut token)?;
    let password = store()
        .lock()
        .map_err(|_| std::io::Error::other("credential lock failed"))?
        .values()
        .find(|v| v.token.as_bytes() == token)
        .map(|v| v.password.clone());
    if let Some(password) = password {
        stream.write_all(password.0.as_bytes())?;
    }
    Ok(())
}
/// SSH options are generated centrally; deriving the endpoint here keeps shell
/// plans secret-free and applies equally to pipe and portable-pty launches.
pub fn environment(args: &[String]) -> Result<Vec<(String, String)>, IpcError> {
    if !args
        .iter()
        .any(|v| v == "PreferredAuthentications=password")
    {
        return Ok(vec![]);
    }
    let target = args
        .get(args.len().saturating_sub(2))
        .ok_or_else(|| IpcError::internal("Missing SSH target"))?;
    let option = |name: &str| {
        args.windows(2)
            .find(|v| v[0] == name)
            .map(|v| v[1].as_str())
    };
    let key = format!(
        "{}:{}|{}",
        target,
        option("-p").unwrap_or("22"),
        option("-J").unwrap_or("")
    );
    let token = store()
        .lock()
        .map_err(|_| IpcError::internal("SSH credential lock failed"))?
        .get(&key)
        .map(|v| v.token.clone())
        .ok_or_else(|| error("SSH password required; enter it in machine settings"))?;
    let askpass = std::env::current_exe().map_err(|e| {
        tracing::warn!("SSH askpass helper path unavailable, password auth cannot prompt: {e}");
        IpcError::internal(e.to_string())
    })?;
    let port = broker()?;
    // SSH_ASKPASS_REQUIRE=force is an OpenSSH 8.4+ feature and Win32-OpenSSH
    // support for it (and for DISPLAY) is unverified. If Windows password auth
    // fails or hangs, check this environment contract first: without it ssh falls
    // back to a terminal prompt that a non-interactive child does not have.
    tracing::debug!(
        "SSH askpass environment set for {key}: SSH_ASKPASS={} SSH_ASKPASS_REQUIRE=force DISPLAY=ferryx:0 FERRYX_SSH_ASKPASS_PORT={port} token=present",
        askpass.display()
    );
    Ok(vec![
        ("SSH_ASKPASS".into(), askpass.to_string_lossy().into_owned()),
        ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
        ("DISPLAY".into(), "ferryx:0".into()),
        ("FERRYX_SSH_ASKPASS_PORT".into(), port.to_string()),
        ("FERRYX_SSH_ASKPASS_TOKEN".into(), token),
    ])
}
/// OpenSSH invokes the askpass helper with exactly one password prompt argument.
fn is_askpass_prompt() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    args.len() == 2
        && args[1]
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("password")
}
/// The helper exits before any tracing subscriber exists, so it reports on stderr
/// too, which ssh inherits from the Ferryx pane or the captured pipe.
fn report_askpass(message: &str) {
    tracing::warn!("SSH askpass: {message}");
    eprintln!("ferryx askpass: {message}");
}
/// Must run before GUI/daemon initialization, including on Windows.
pub fn run_askpass() -> Option<i32> {
    let port = match std::env::var("FERRYX_SSH_ASKPASS_PORT") {
        Ok(port) => port,
        Err(_) => {
            // A prompt-shaped command line without our environment means ssh ran
            // the helper and the askpass variables did not reach it, which is the
            // Windows failure this diagnostics set exists to expose.
            if is_askpass_prompt() {
                report_askpass(
                    "FERRYX_SSH_ASKPASS_PORT is unset for an askpass invocation; ssh ran the \
                     helper without the askpass environment",
                );
            }
            return None;
        }
    };
    // An ordinary Ferryx child inheriting an SSH environment is not an askpass
    // invocation. OpenSSH supplies exactly one password prompt argument.
    if !is_askpass_prompt() {
        return None;
    }
    let status = (|| -> std::io::Result<()> {
        let port: u16 = port.parse().map_err(std::io::Error::other)?;
        let token = std::env::var("FERRYX_SSH_ASKPASS_TOKEN").map_err(std::io::Error::other)?;
        let mut stream = TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(3),
        )?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.write_all(token.as_bytes())?;
        let mut password = Vec::new();
        stream.take(4097).read_to_end(&mut password)?;
        if password.is_empty() || password.len() > 4096 {
            return Err(std::io::Error::other("credential unavailable"));
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&password)?;
        stdout.write_all(b"\n")?;
        Ok(())
    })();
    match status {
        Ok(()) => Some(0),
        Err(e) => {
            // ssh reads this status: anything non-zero aborts password auth, and no
            // line at all means ssh never ran the helper (see the environment site).
            report_askpass(&format!(
                "could not serve the password prompt on 127.0.0.1:{port}: {e}"
            ));
            Some(1)
        }
    }
}
