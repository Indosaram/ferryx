//! Transient OpenSSH askpass credentials and interactive auth prompts. Only a random per-launch
//! capability crosses into the child environment; secrets never enter argv, environment, files or
//! logs.
//!
//! Every ssh launched from a plan that may prompt (`BatchMode` not `yes`) gets the askpass helper.
//! The helper forwards the question OpenSSH asks (password, key passphrase, keyboard-interactive /
//! OTP text) to the broker. A password prompt that names the launch's own endpoint is answered from
//! the stored credential; anything else (passphrase, OTP, a jump host's password, unknown text) is
//! surfaced to the UI and waits [`PROMPT_TIMEOUT`] for an answer.
use super::{SshAuthMethod, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::{Duration, Instant},
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
    /// `(user, host)` names an OpenSSH prompt may use for this endpoint, lowercased: the stored
    /// hostname and, for an ssh_config alias host, the alias.
    names: Vec<(Option<String>, String)>,
    /// The endpoint is an ssh_config alias, whose Host block may add a ProxyJump Ferryx cannot see.
    alias: bool,
}
static STORE: OnceLock<Mutex<HashMap<String, Credential>>> = OnceLock::new();
#[cfg(test)]
#[path = "password_tests.rs"]
mod tests;
fn store() -> &'static Mutex<HashMap<String, Credential>> {
    STORE.get_or_init(Default::default)
}
/// Derived from the argv ssh is launched with, exactly as [`environment`] derives it, so a stored
/// password is found for the endpoint ssh is pointed at (the alias for an ssh_config host). For a
/// manual host this is `user@hostname:port|jump`, unchanged.
fn key(host: &SshHost) -> String {
    let argv = super::exec::interactive_argv(host);
    endpoint_key(&argv, argv.last().map(String::as_str).unwrap_or_default())
}
fn endpoint_key(args: &[String], target: &str) -> String {
    let option = |name: &str| {
        args.windows(2)
            .find(|v| v[0] == name)
            .map(|v| v[1].as_str())
    };
    format!(
        "{}:{}|{}",
        target,
        option("-p").unwrap_or("22"),
        option("-J").unwrap_or("")
    )
}
fn error(message: &str) -> IpcError {
    super::runtime::error(IpcErrorCode::IoError, "authentication", message)
}
fn credential_names(host: &SshHost) -> Vec<(Option<String>, String)> {
    let user = host
        .username
        .as_deref()
        .filter(|user| !user.is_empty())
        .map(str::to_lowercase);
    let mut names = vec![(user.clone(), host.hostname.to_lowercase())];
    if let Some(alias) = host.config_alias() {
        names.push((user, alias.to_lowercase()));
    }
    names
}
pub fn set(host: &SshHost, password: Password) -> Result<(), IpcError> {
    super::direct::validate_host(host)?;
    if host.auth_method != SshAuthMethod::Password
        || password.0.is_empty()
        || password.0.len() > MAX_SECRET
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
                names: credential_names(host),
                alias: host.config_alias().is_some(),
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
                        let Ok(stream) = incoming else { break };
                        // A request may wait on the user; it never holds up the next one.
                        let spawned = std::thread::Builder::new()
                            .name("ssh-askpass-request".into())
                            .spawn(move || {
                                if let Err(e) = serve(stream) {
                                    tracing::debug!("SSH askpass request rejected: {e}");
                                }
                            });
                        if let Err(e) = spawned {
                            tracing::warn!("SSH askpass request thread could not start: {e}");
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

/// How long a prompt surfaced to the UI waits for an answer before it fails with a typed timeout.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(90);
/// Event the UI listens on for [`AuthPrompt`]s.
pub const AUTH_PROMPT_EVENT: &str = "ssh_auth_prompt";
const SESSION_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_PENDING_PROMPTS: usize = 16;
/// A key passphrase answered in the UI is reused for the identical prompt (it names the key file)
/// this long, so the several ssh launches of one connection test do not ask again. Never applied to
/// passwords, codes or other prompts.
const PASSPHRASE_REUSE: Duration = Duration::from_secs(60);
const MAX_SECRET: usize = 4096;

/// One ssh launch: the endpoint its argv names, keyed by the capability in its environment.
#[derive(Clone, Debug)]
struct Session {
    /// The capability itself; prompts of a launch whose earlier prompt timed out or was
    /// cancelled fail at once instead of asking again (ssh retries keyboard-interactive).
    id: String,
    endpoint: String,
    target: String,
    jump: bool,
    created: Instant,
}
static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    SESSIONS.get_or_init(Default::default)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptKind {
    Password,
    Passphrase,
    Code,
    Other,
}

fn classify(prompt: &str) -> PromptKind {
    let lower = prompt.to_lowercase();
    if lower.contains("passphrase") {
        PromptKind::Passphrase
    } else if [
        "code",
        "otp",
        "oath",
        "token",
        "verification",
        "one-time",
        "2fa",
        "authenticator",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        PromptKind::Code
    } else if lower.contains("password") {
        PromptKind::Password
    } else {
        PromptKind::Other
    }
}

/// The `[user@]host` an OpenSSH prompt names: `user@host's password: ` (password auth) or a
/// `(user@host) ` prefix (keyboard-interactive, OpenSSH 8.6+). Lowercased.
fn prompt_host(prompt: &str) -> Option<(Option<String>, String)> {
    let prompt = prompt.trim_start();
    let named = match prompt.strip_prefix('(') {
        Some(rest) => rest.split_once(')').map(|(named, _)| named),
        None => prompt.split_once("'s password").map(|(named, _)| named),
    }?;
    if named.is_empty() || named.contains(char::is_whitespace) {
        return None;
    }
    let named = named.to_lowercase();
    Some(match named.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_string()), host.to_string()),
        None => (None, named),
    })
}

/// OpenSSH's own password questions: `user@host's password: `, a bare `Password: `, or the
/// keyboard-interactive `(user@host) Password: ` PAM sends. A PAM prompt that merely mentions a
/// password ("One-time password", "New password") is not one.
fn is_plain_password_prompt(prompt: &str) -> bool {
    let lower = prompt.trim().to_lowercase();
    let rest = match lower.strip_prefix('(') {
        Some(rest) => rest.split_once(") ").map_or("", |(_, rest)| rest),
        None => lower.split_once("'s ").map_or(lower.as_str(), |(_, rest)| rest),
    };
    rest == "password:"
}

/// The stored password, only for a password prompt about this launch's own endpoint. A prompt that
/// names another host (a jump host) never receives it; a prompt that names no host receives it only
/// when no jump host can be involved.
fn stored_answer(session: &Session, prompt: &str) -> Option<Password> {
    if !is_plain_password_prompt(prompt) {
        return None;
    }
    let store = store().lock().ok()?;
    let credential = store.get(&session.endpoint)?;
    let allowed = match prompt_host(prompt) {
        Some((user, host)) => credential.names.iter().any(|(stored_user, stored_host)| {
            stored_host == &host
                && (stored_user.is_none() || user.is_none() || stored_user == &user)
        }),
        None => !session.jump && !credential.alias,
    };
    allowed.then(|| credential.password.clone())
}

/// A question OpenSSH asked, as sent to the UI. Carries no secret.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub id: String,
    pub host: String,
    pub prompt: String,
    pub kind: PromptKind,
    pub expires_in_ms: u64,
}

pub type PromptSink = Arc<dyn Fn(&AuthPrompt) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum PromptFailure {
    NoListener,
    Busy,
    Cancelled,
    TimedOut,
}

struct PendingPrompt {
    reply: mpsc::Sender<Option<Password>>,
    started: Instant,
}

#[derive(Default)]
struct PromptState {
    listeners: usize,
    sink: Option<PromptSink>,
    pending: HashMap<String, PendingPrompt>,
    /// Total time finished prompts waited on the user; see [`prompt_clock`].
    waited: Duration,
    passphrases: HashMap<String, (Password, Instant)>,
    /// Endpoints whose last prompt timed out, consumed by [`take_prompt_timeout`].
    timed_out: HashMap<String, Instant>,
    /// Launches whose prompt timed out or was cancelled.
    ended: HashMap<String, (PromptFailure, Instant)>,
}

/// UI prompt routing: listener registration, pending prompts and their answers.
#[derive(Default)]
pub struct Prompts {
    state: Mutex<PromptState>,
}

static PROMPTS: OnceLock<Prompts> = OnceLock::new();
pub fn prompts() -> &'static Prompts {
    PROMPTS.get_or_init(Default::default)
}

impl Prompts {
    fn lock(&self) -> std::sync::MutexGuard<'_, PromptState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Registers (`active`) or unregisters one UI listener. With no listener left every pending
    /// prompt is cancelled and new prompts fail at once instead of waiting for nobody.
    pub fn set_listener(&self, active: bool, sink: PromptSink) {
        let mut state = self.lock();
        if active {
            state.listeners += 1;
            state.sink = Some(sink);
            return;
        }
        state.listeners = state.listeners.saturating_sub(1);
        if state.listeners == 0 {
            state.sink = None;
            let pending: Vec<_> = state.pending.drain().map(|(_, pending)| pending).collect();
            for pending in pending {
                state.waited += pending.started.elapsed();
                let _ = pending.reply.send(None);
            }
        }
    }

    /// Delivers the UI's answer; `None` cancels. Fails with a typed timeout when the prompt is gone.
    pub fn answer(&self, id: &str, answer: Option<Password>) -> Result<(), IpcError> {
        if let Some(answer) = &answer {
            if answer.0.is_empty()
                || answer.0.len() > MAX_SECRET
                || answer.0.contains(['\n', '\r', '\0'])
            {
                return Err(error("Enter a nonempty response without line breaks"));
            }
        }
        let mut state = self.lock();
        let pending = state.pending.remove(id).ok_or_else(|| {
            IpcError::new(IpcErrorCode::Timeout, "This SSH prompt already expired")
                .with_details(serde_json::json!({"stage": "prompt"}))
        })?;
        state.waited += pending.started.elapsed();
        let _ = pending.reply.send(answer);
        Ok(())
    }

    fn clock(&self) -> Duration {
        let state = self.lock();
        state.waited
            + state
                .pending
                .values()
                .map(|pending| pending.started.elapsed())
                .sum::<Duration>()
    }

    fn ask(
        &self,
        session: &Session,
        prompt: &str,
        timeout: Duration,
    ) -> Result<Password, PromptFailure> {
        if let Some(password) = stored_answer(session, prompt) {
            return Ok(password);
        }
        let kind = classify(prompt);
        let (id, rx, sink) = {
            let mut state = self.lock();
            state.ended.retain(|_, (_, at)| at.elapsed() < SESSION_TTL);
            if let Some((failure, _)) = state.ended.get(&session.id) {
                return Err(failure.clone());
            }
            if kind == PromptKind::Passphrase {
                state
                    .passphrases
                    .retain(|_, (_, at)| at.elapsed() < PASSPHRASE_REUSE);
                if let Some((passphrase, _)) = state.passphrases.get(prompt) {
                    return Ok(passphrase.clone());
                }
            }
            let sink = match (&state.sink, state.listeners) {
                (Some(sink), listeners) if listeners > 0 => sink.clone(),
                _ => return Err(PromptFailure::NoListener),
            };
            if state.pending.len() >= MAX_PENDING_PROMPTS {
                return Err(PromptFailure::Busy);
            }
            let id = uuid::Uuid::new_v4().to_string();
            let (reply, rx) = mpsc::channel();
            state.pending.insert(
                id.clone(),
                PendingPrompt {
                    reply,
                    started: Instant::now(),
                },
            );
            (id, rx, sink)
        };
        let host = match prompt_host(prompt) {
            Some((Some(user), host)) => format!("{user}@{host}"),
            Some((None, host)) => host,
            None => session.target.clone(),
        };
        tracing::info!(prompt_id = %id, ?kind, %host, "SSH auth prompt sent to the UI");
        sink(&AuthPrompt {
            id: id.clone(),
            host,
            prompt: prompt.to_string(),
            kind,
            expires_in_ms: timeout.as_millis() as u64,
        });
        let result = rx.recv_timeout(timeout);
        let mut state = self.lock();
        if let Some(pending) = state.pending.remove(&id) {
            state.waited += pending.started.elapsed();
        }
        match result {
            Ok(Some(answer)) => {
                if kind == PromptKind::Passphrase {
                    state
                        .passphrases
                        .insert(prompt.to_string(), (answer.clone(), Instant::now()));
                }
                Ok(answer)
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                state
                    .ended
                    .insert(session.id.clone(), (PromptFailure::Cancelled, Instant::now()));
                Err(PromptFailure::Cancelled)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                state
                    .ended
                    .insert(session.id.clone(), (PromptFailure::TimedOut, Instant::now()));
                state.timed_out.retain(|_, at| at.elapsed() < SESSION_TTL);
                state
                    .timed_out
                    .insert(session.endpoint.clone(), Instant::now());
                Err(PromptFailure::TimedOut)
            }
        }
    }
}

/// Total time prompts have waited on the user, including prompts still waiting. Deadlines of ssh
/// operations are extended by its growth so a user typing an answer does not time the probe out.
pub fn prompt_clock() -> Duration {
    prompts().clock()
}

/// `tokio::time::timeout` whose budget excludes time spent waiting on auth prompts. `None` when
/// the budget ran out.
pub async fn with_prompt_deadline<F: std::future::Future>(
    budget: Duration,
    future: F,
) -> Option<F::Output> {
    let start = tokio::time::Instant::now();
    let clock = prompt_clock();
    let end = || start + budget + prompt_clock().saturating_sub(clock);
    tokio::pin!(future);
    loop {
        if let Ok(output) = tokio::time::timeout_at(end(), &mut future).await {
            return Some(output);
        }
        if end() <= tokio::time::Instant::now() {
            return None;
        }
    }
}

/// Whether the last prompt of the endpoint `args` launch ssh at timed out (consumes the mark).
pub fn take_prompt_timeout(args: &[String]) -> bool {
    let Some(target) = args.get(args.len().saturating_sub(2)) else {
        return false;
    };
    prompts()
        .lock()
        .timed_out
        .remove(&endpoint_key(args, target))
        .is_some()
}

/// The typed error for an auth prompt nobody answered in time.
pub fn prompt_timeout_error() -> IpcError {
    IpcError::new(
        IpcErrorCode::Timeout,
        format!(
            "SSH asked for a password, passphrase or code and no answer arrived within {} seconds",
            PROMPT_TIMEOUT.as_secs()
        ),
    )
    .with_details(serde_json::json!({"stage": "prompt", "reason": "prompt_timeout"}))
}

fn serve(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut token = [0; 36];
    stream.read_exact(&mut token)?;
    let session = sessions()
        .lock()
        .map_err(|_| std::io::Error::other("session lock failed"))?
        .get(std::str::from_utf8(&token).unwrap_or_default())
        .cloned()
        .ok_or_else(|| std::io::Error::other("unknown askpass capability"))?;
    let mut prompt = Vec::new();
    (&mut stream)
        .take(MAX_SECRET as u64 + 1)
        .read_to_end(&mut prompt)?;
    if prompt.len() > MAX_SECRET {
        return Err(std::io::Error::other("askpass prompt too long"));
    }
    let prompt = String::from_utf8_lossy(&prompt).into_owned();
    match prompts().ask(&session, &prompt, PROMPT_TIMEOUT) {
        Ok(answer) => stream.write_all(answer.0.as_bytes()),
        Err(failure) => {
            tracing::info!(?failure, "SSH auth prompt was not answered");
            Ok(())
        }
    }
}
/// SSH options are generated centrally; deriving the endpoint here keeps shell
/// plans secret-free and applies equally to pipe and portable-pty launches. Every plan that may
/// prompt (`BatchMode` not `yes`) gets a fresh capability bound to the endpoint its argv names.
pub fn environment(args: &[String]) -> Result<Vec<(String, String)>, IpcError> {
    let password_auth = args
        .iter()
        .any(|v| v.starts_with("PreferredAuthentications=password"));
    // An interactive pane (`-tt`) has a terminal: ssh asks passphrase, keyboard-interactive and OTP
    // questions there itself. Only its stored password still comes from the broker.
    let interactive = args.iter().any(|v| v == "-tt");
    // Only plans from `direct::ssh_plan` (which set `BatchMode=no`) may prompt.
    if !args.iter().any(|v| v == "BatchMode=no") || (interactive && !password_auth) {
        return Ok(vec![]);
    }
    let target = args
        .get(args.len().saturating_sub(2))
        .ok_or_else(|| IpcError::internal("Missing SSH target"))?;
    let key = endpoint_key(args, target);
    if password_auth
        && !store()
            .lock()
            .map_err(|_| IpcError::internal("SSH credential lock failed"))?
            .contains_key(&key)
    {
        return Err(error("SSH password required; enter it in machine settings"));
    }
    let token = uuid::Uuid::new_v4().to_string();
    {
        let mut sessions = sessions()
            .lock()
            .map_err(|_| IpcError::internal("SSH session lock failed"))?;
        sessions.retain(|_, session| session.created.elapsed() < SESSION_TTL);
        sessions.insert(
            token.clone(),
            Session {
                id: token.clone(),
                endpoint: key.clone(),
                target: target.clone(),
                jump: args.iter().any(|arg| arg == "-J"),
                created: Instant::now(),
            },
        );
    }
    let askpass = std::env::current_exe().map_err(|e| {
        tracing::warn!("SSH askpass helper path unavailable, ssh cannot prompt: {e}");
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
/// OpenSSH invokes the askpass helper with exactly one prompt argument.
fn askpass_prompt() -> Option<String> {
    let args: Vec<_> = std::env::args_os().collect();
    is_askpass_prompt_args(&args).then(|| args[1].to_string_lossy().into_owned())
}

/// A prompt-shaped single argument: it mentions a password or passphrase, or ends like a question
/// (`:` or `?`), which covers keyboard-interactive, OTP and localized prompts. Flags are excluded.
pub(crate) fn is_askpass_prompt_args<T: AsRef<std::ffi::OsStr>>(args: &[T]) -> bool {
    if args.len() != 2 {
        return false;
    }
    let arg = args[1].as_ref().to_string_lossy().to_lowercase();
    let trimmed = arg.trim_end();
    !arg.starts_with('-')
        && (arg.contains("password")
            || arg.contains("passphrase")
            || trimmed.ends_with(':')
            || trimmed.ends_with('?'))
}
/// The helper exits before any tracing subscriber exists, so it reports on stderr
/// too, which ssh inherits from the Ferryx pane or the captured pipe.
fn report_askpass(message: &str) {
    tracing::warn!("SSH askpass: {message}");
    eprintln!("ferryx askpass: {message}");
}
/// Must run before GUI/daemon initialization, including on Windows.
pub fn run_askpass() -> Option<i32> {
    run_askpass_with(
        std::env::var("FERRYX_SSH_ASKPASS_PORT").ok(),
        std::env::var("FERRYX_SSH_ASKPASS_TOKEN").ok(),
        askpass_prompt(),
    )
}

pub(crate) fn run_askpass_with(
    port: Option<String>,
    token: Option<String>,
    prompt: Option<String>,
) -> Option<i32> {
    let port = match port {
        Some(port) => port,
        None => {
            // A prompt-shaped command line without our environment means ssh ran
            // the helper and the askpass variables did not reach it, which is the
            // Windows failure this diagnostics set exists to expose.
            if prompt.is_some() {
                report_askpass(
                    "FERRYX_SSH_ASKPASS_PORT is unset for an askpass invocation; ssh ran the \
                     helper without the askpass environment",
                );
            }
            return None;
        }
    };
    // An ordinary Ferryx child inheriting an SSH environment is not an askpass
    // invocation. OpenSSH supplies exactly one prompt argument.
    let prompt = prompt?;
    let status = (|| -> std::io::Result<()> {
        let port: u16 = port.parse().map_err(std::io::Error::other)?;
        let token = token.ok_or_else(|| std::io::Error::other("missing token"))?;
        let mut stream = TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(3),
        )?;
        // The broker may wait for the user to answer in the UI.
        stream.set_read_timeout(Some(PROMPT_TIMEOUT + Duration::from_secs(10)))?;
        stream.write_all(token.as_bytes())?;
        stream.write_all(prompt.as_bytes())?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let mut answer = Vec::new();
        stream
            .take(MAX_SECRET as u64 + 1)
            .read_to_end(&mut answer)?;
        if answer.is_empty() || answer.len() > MAX_SECRET {
            return Err(std::io::Error::other("no answer for this prompt"));
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&answer)?;
        stdout.write_all(b"\n")?;
        Ok(())
    })();
    match status {
        Ok(()) => Some(0),
        Err(e) => {
            // ssh reads this status: anything non-zero aborts this authentication
            // method, and no line at all means ssh never ran the helper.
            report_askpass(&format!(
                "could not answer the prompt via 127.0.0.1:{port}: {e}"
            ));
            Some(1)
        }
    }
}
