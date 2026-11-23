//! MODE=A dispatch for a cloud host: a local unix socket that is an SSH stream-local forward of
//! the box's RPC socket, plus the per-machine SSH files it rides on.
//!
//! Task 1's second spike (`.omo/evidence/ferryx-cloud-host-sandbox/task-1-spike-r2.txt`) wrote
//! `ENGINE=omo`, `FINAL_MODE=A` and `KEYROOT_SYNC=no`: with `omo` as the box engine (the npm
//! `omo-ai` runtime, not the `mhc` tarball), the four `host session` verbs ran *on this machine*
//! against the forwarded socket and returned `spike-ok`, so dispatch ships as a forward and no
//! key root is copied. The first spike had already shown the forward itself is never refused -
//! `status`/`open`/`state`/`read`/`wait`/`close` all answered valid JSON through it - so its
//! stall belonged to the engine, not to this file.
//!
//! Two things this module does NOT do, on purpose:
//!
//! - It never edits `~/.ssh/config` or the user's `~/.ssh/known_hosts`. Each machine gets its own
//!   `<name>.conf` and `<name>.known_hosts` under `~/.ssh/ferryx-cloud/`, and `-F` makes ssh read
//!   exactly one of each. A cloud box therefore cannot inherit local host trust or add to it.
//! - It never trusts `ExitOnForwardFailure` to prove the remote end. The spike's negative case is
//!   the reason: ssh creates the local socket immediately even when the remote path does not
//!   exist, because it cannot learn that until a connection arrives. So [`start`] connects
//!   through the local socket itself, speaks one `get_protocol_info` line, and requires a reply
//!   that parses as a JSON object whose `type` is `protocol_info` - a bare "it parsed as JSON"
//!   test accepts a listener that merely mirrors the request back, because the request is
//!   itself valid JSON.
//!
//! Two failure modes are constrained by the process table rather than by this module's own
//! handles, so both are enforced by the kernel:
//!
//! - **The forward must die with the process that owns it.** `kill_on_drop` only runs when the
//!   guard is dropped; a `SIGKILL` of the owner (a crashed GUI, a killed daemon) skips it and
//!   leaves ssh serving a socket nobody owns. Closing ssh's stdin does not help - `ssh -N` was
//!   measured to ignore stdin EOF - and macOS has no `PR_SET_PDEATHSIG`, so ssh is given a
//!   **controlling terminal** instead: the owner holds the pty master, so when the owner dies for
//!   any reason the kernel hangs the terminal up and ssh takes its default `SIGHUP`. Linux has
//!   `PR_SET_PDEATHSIG` as an alternative, but it has no macOS equivalent and one mechanism is
//!   better than two, so the pty is used on every unix and nothing needs a platform branch.
//! - **The socket name belongs to the forward that published it.** A second forward for the same
//!   machine name re-binds the path to a fresh inode, so teardown compares the `(dev, ino)` it
//!   recorded at bind before unlinking: removing by name alone would delete the newer forward's
//!   entry and leave its listener alive on an inode no caller can name.

use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::ffi::CString;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// How long ssh gets to create the local socket. A forward that has not bound by then is not a
/// slow start, it is a broken one.
pub const LOCAL_SOCKET_BUDGET: Duration = Duration::from_secs(15);

/// How long the remote end gets to answer the probe once the local socket exists.
pub const PROBE_BUDGET: Duration = Duration::from_secs(10);

/// How often [`start`] re-checks for the local socket. Small enough that a healthy forward is not
/// held up, large enough that a stalled ssh does not spin a core for the whole budget.
const BIND_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The one line the readiness probe sends. It is the RPC host's own information request, so an
/// answer also proves the far side is the RPC daemon and not some other unix service that merely
/// accepts connections.
const PROBE_REQUEST: &str = r#"{"type":"get_protocol_info"}"#;

/// The `type` a real `get_protocol_info` reply carries.
///
/// Requiring it is what separates "a socket that speaks JSON" from "the RPC daemon": the request
/// line is itself valid JSON, so a listener that mirrors the request back answers with
/// `get_protocol_info`, which parses - and is not an answer.
const PROBE_REPLY_TYPE: &str = "protocol_info";

/// Ceiling for the probe reply. The real answer is a short JSON object; anything larger is not it.
const PROBE_REPLY_LIMIT: usize = 64 * 1024;

/// Directory under the user's `~/.ssh` holding one conf/known_hosts pair per cloud machine.
const CLOUD_DIR: &str = "ferryx-cloud";

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidArgument, message)
}

fn io_error(message: impl std::fmt::Display) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, message.to_string())
}

/// Everything needed to pin one cloud machine's SSH configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineSsh {
    /// Per-machine file stem and `Host` alias. A bare token, never a path.
    pub name: String,
    pub hostname: String,
    /// The provider's ssh username. It is not restricted to a POSIX login: Freestyle's gateway
    /// authenticates `<vm-id>:<token>`, so it is validated by [`validate_cloud_username`] rather
    /// than by the ordinary `User` token rule.
    pub username: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    /// The host key the provider returned, as an OpenSSH public-key line. `None` means the
    /// provider offered none, and the machine is pinned on first use instead.
    pub pinned_host_key: Option<String>,
    /// The provider CLI's `ProxyCommand` value, emitted verbatim. Codespaces' `cs.<name>.main`
    /// has no DNS record, so `gh cs ssh -c <name> --stdio` is the only route to the box; a conf
    /// without this line cannot connect at all.
    pub proxy_command: Option<String>,
    /// A short-lived certificate that authenticates with `identity_file` (OpenSSH
    /// `CertificateFile`). InstaBox refuses the bare key with `Permission denied (publickey)`.
    pub certificate_file: Option<PathBuf>,
}

/// The files written for one machine, and the alias its conf declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineFiles {
    pub alias: String,
    pub conf: PathBuf,
    pub known_hosts: PathBuf,
}

/// Where the forward ends: a local unix socket and the box's absolute RPC socket path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    pub local_socket: PathBuf,
    /// Absolute POSIX path on the box, e.g. `/home/daytona/.ferryx-cloud/rpc.sock`.
    pub remote_socket: String,
}

impl ForwardSpec {
    pub fn new(
        local_socket: impl Into<PathBuf>,
        remote_socket: impl Into<String>,
    ) -> Result<Self, IpcError> {
        let spec = Self {
            local_socket: local_socket.into(),
            remote_socket: remote_socket.into(),
        };
        spec.validate()?;
        Ok(spec)
    }

    /// The local path stays inside a directory we own and the remote path is absolute: a
    /// relative or control-character path would let one machine's forward resolve somewhere the
    /// caller never named.
    fn validate(&self) -> Result<(), IpcError> {
        let local = &self.local_socket;
        if !local.is_absolute() {
            return Err(invalid("Forwarded socket path must be absolute"));
        }
        if local.to_string_lossy().chars().any(char::is_control) {
            return Err(invalid(
                "Forwarded socket path must not contain control characters",
            ));
        }
        crate::ssh::direct::validate_remote_path(&self.remote_socket)?;
        Ok(())
    }
}

/// The file identity of the socket name this forward bound.
///
/// Teardown must remove this forward's own name and nothing else. A second forward for the same
/// machine name re-binds the path to a fresh inode, so removing by name alone would delete the
/// name the newer forward published and leave its listener alive on an inode no caller can reach.
/// (`paired_host::socket_forward` guards the same way for the paired-relay transport.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublishedName {
    dev: u64,
    ino: u64,
}

fn published_name(path: &Path) -> Option<PublishedName> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(path).ok()?;
    Some(PublishedName {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// Removes `path` only while it still names the inode recorded at bind.
fn remove_published(path: &Path, published: Option<PublishedName>) {
    let still_published_here = match published {
        Some(published) => published_name(path) == Some(published),
        // The bind identity could not be read: remove the name rather than leave a socket behind
        // that this forward would no longer be serving.
        None => true,
    };
    if still_published_here {
        let _ = std::fs::remove_file(path);
    }
}

/// A running forward. Dropping it kills ssh and unlinks the local socket; [`shutdown`] does the
/// same and additionally waits for the child to be reaped.
///
/// [`shutdown`]: SocketForward::shutdown
pub struct SocketForward {
    child: Option<tokio::process::Child>,
    local: PathBuf,
    pid: Option<u32>,
    /// The socket name's identity when this forward bound it, so teardown can tell its own entry
    /// from one a later forward for the same name has published since.
    published: Option<PublishedName>,
    /// The pty master whose terminal ssh holds. This process dying closes it, the kernel hangs the
    /// terminal up, and ssh takes its default `SIGHUP` - the only mechanism that also covers a
    /// `SIGKILL` of the owner, where `Drop` never runs.
    terminal: Option<OwnedFd>,
}

impl std::fmt::Debug for SocketForward {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SocketForward")
            .field("local", &self.local)
            .field("pid", &self.pid)
            .finish()
    }
}

impl SocketForward {
    /// The local socket `mhc host session ... --socket <this>` receives.
    pub fn local_socket(&self) -> &Path {
        &self.local
    }

    /// The ssh process that owns the forward, for diagnostics and leak checks.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Ends the forward and waits for ssh to exit, so the caller is never racing a reaper.
    pub async fn shutdown(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        self.terminal = None;
        remove_published(&self.local, self.published);
    }
}

impl Drop for SocketForward {
    fn drop(&mut self) {
        // Drop cannot await, so the kill is a request; `kill_on_drop` at spawn is the backstop
        // that guarantees the child cannot outlive this guard.
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        // Closing the master hangs ssh's controlling terminal up, so the child also ends on a path
        // where the explicit kill above was refused.
        self.terminal = None;
        remove_published(&self.local, self.published);
    }
}

/// `~/.ssh/ferryx-cloud`, created 0700 by [`write_machine_files`].
pub fn config_dir() -> Result<PathBuf, IpcError> {
    let home = dirs::home_dir().ok_or_else(|| io_error("No home directory for cloud SSH files"))?;
    Ok(home.join(".ssh").join(CLOUD_DIR))
}

/// The local socket path the plan fixes: `$TMPDIR/ferryx-cloud-<uid>/<name>.sock`.
pub fn default_local_socket(name: &str) -> Result<PathBuf, IpcError> {
    validate_name(name)?;
    let uid = unsafe { libc::geteuid() };
    Ok(std::env::temp_dir()
        .join(format!("ferryx-cloud-{uid}"))
        .join(format!("{name}.sock")))
}

/// Writes the machine's files where production reads them (`~/.ssh/ferryx-cloud/`).
pub fn write_machine_files(machine: &MachineSsh) -> Result<MachineFiles, IpcError> {
    write_machine_files_in(&config_dir()?, machine)
}

/// Same, into `root`. Split out so tests own a disposable directory instead of `~/.ssh`.
pub fn write_machine_files_in(root: &Path, machine: &MachineSsh) -> Result<MachineFiles, IpcError> {
    validate_name(&machine.name)?;
    // The hostname, port and identity go through the one rule the rest of the SSH module uses, so
    // a cloud machine cannot reach ssh with a value an ordinary host would be refused. The
    // username is the one exception: Freestyle's gateway authenticates `<vm-id>:<token>` (see
    // `FreestyleProvider::ssh_target`), and `:` is not a `User` token character under that rule,
    // which is what failed `connect` for a correct target in the task-14 live smoke. It is held
    // to its own rule instead - see `validate_cloud_username`.
    crate::ssh::direct::validate_host(&ssh_host(machine))?;
    if let Some(username) = &machine.username {
        validate_cloud_username(username)?;
    }
    // `ProxyCommand` is the one directive emitted as a raw command line rather than a quoted
    // value, so a newline in it would append directives of the provider's choosing to this conf.
    // The value comes from a provider CLI, i.e. from off the machine, so it is refused rather
    // than trusted.
    if let Some(proxy) = &machine.proxy_command {
        if proxy.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
            return Err(invalid("ProxyCommand must be a single line"));
        }
    }
    // `CertificateFile` is emitted quoted, so a newline in it cannot append a directive - but it
    // still reaches ssh as a broken path and makes ssh reject the whole conf with a message about
    // quotes that names nothing. The two sibling provider strings are refused on the same rule so
    // a control character is one error, not three different ones.
    if let Some(certificate) = &machine.certificate_file {
        if certificate
            .as_os_str()
            .as_bytes()
            .iter()
            .any(|byte| *byte < 0x20 || *byte == 0x7f)
        {
            return Err(invalid("CertificateFile must be a single line"));
        }
    }
    let files = MachineFiles {
        alias: machine.name.clone(),
        conf: root.join(format!("{}.conf", machine.name)),
        known_hosts: root.join(format!("{}.known_hosts", machine.name)),
    };
    ensure_private_dir(root)?;
    let known_hosts_text = match &machine.pinned_host_key {
        Some(key) => known_hosts_entry(&known_hosts_pattern(&machine.hostname, machine.port), key)?,
        None => String::new(),
    };
    write_private(&files.known_hosts, known_hosts_text.as_bytes())?;
    write_private(&files.conf, machine_conf_text(machine, &files).as_bytes())?;
    Ok(files)
}

fn ssh_host(machine: &MachineSsh) -> SshHost {
    SshHost {
        id: machine.name.clone(),
        label: machine.name.clone(),
        hostname: machine.hostname.clone(),
        // Validation only, and deliberately empty: a cloud provider may carry a credential in the
        // username (Freestyle's `<vm-id>:<token>`), which this struct's rule refuses. The
        // username is validated by `validate_cloud_username` instead.
        username: None,
        port: machine.port,
        identity_file: machine.identity_file.clone(),
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: if machine.identity_file.is_some() {
            SshAuthMethod::Key
        } else {
            SshAuthMethod::Agent
        },
        disabled: None,
        shell: None,
    }
}

/// The name becomes a file stem and a `Host` pattern, so it is a bare token: no separators, no
/// leading dash (an option), no leading dot (a hidden file, or `..`).
fn validate_name(name: &str) -> Result<(), IpcError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !ok {
        return Err(invalid(
            "Cloud machine name must be a bare token of letters, digits, '.', '_' or '-'",
        ));
    }
    Ok(())
}

/// The longest cloud username accepted. Freestyle's `<vm-id>:<token>` is a 35-character VM id, a
/// `:` and an API token - hundreds of bytes of headroom below this - so the bound only stops a
/// provider from handing ssh an unbounded value.
const CLOUD_USERNAME_MAX: usize = 512;

/// The one rule for a cloud-provided SSH username.
///
/// It cannot be `direct::validate_host`'s `User` rule: that refuses `:`, and Freestyle's gateway
/// wants exactly `<vm-id>:<token>` as the username, with the token as the credential. The value
/// still reaches ssh as a single `User` value in a quoted config line, so it is held to what that
/// really is - one token with no whitespace, quote, control character or shell metacharacter:
///
/// - a leading `-` would turn the destination into an option or a config override;
/// - `%` is left out because ssh expands `%`-tokens in config values and no provider login needs
///   one;
/// - `;`, `&`, `|`, `$`, `` ` ``, `/` and `\` are refused because a provider string that reaches
///   a shell or a path must never look like syntax.
fn validate_cloud_username(username: &str) -> Result<(), IpcError> {
    let ok = !username.is_empty()
        && username.len() <= CLOUD_USERNAME_MAX
        && !username.starts_with('-')
        && username.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '+' | '@' | '=')
        });
    if !ok {
        return Err(invalid(
            "Cloud SSH username must be one token without whitespace, quotes or control characters",
        ));
    }
    Ok(())
}

/// known_hosts matches the host actually connected to - never our alias - and brackets only a
/// non-default port or an IPv6 literal.
fn known_hosts_pattern(hostname: &str, port: Option<u16>) -> String {
    match port {
        Some(port) if port != 22 => format!("[{hostname}]:{port}"),
        _ if hostname.contains(':') => format!("[{hostname}]"),
        _ => hostname.to_string(),
    }
}

/// One known_hosts line from the provider's public key.
///
/// Providers return the key line with a trailing newline, so that is trimmed first. What remains
/// must be exactly one line: an interior newline would smuggle a second entry into the trust
/// file, so control characters are refused rather than stripped.
fn known_hosts_entry(pattern: &str, key: &str) -> Result<String, IpcError> {
    let key = key.trim();
    if key.chars().any(char::is_control) {
        return Err(invalid("Pinned host key must be a single line"));
    }
    let fields: Vec<&str> = key.split_whitespace().collect();
    let kind = fields.first().copied().unwrap_or_default();
    let looks_like_key =
        kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-");
    if fields.len() < 2 || !looks_like_key {
        return Err(invalid(
            "Pinned host key must be an OpenSSH public key line",
        ));
    }
    Ok(format!("{pattern} {}\n", fields.join(" ")))
}

/// ssh_config values are whitespace separated, so a path (a home directory may contain a space)
/// is quoted with its own quotes and backslashes escaped.
fn conf_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn machine_conf_text(machine: &MachineSsh, files: &MachineFiles) -> String {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut text =
        String::from("# Generated by ferryx for one cloud machine; rewritten on every start.\n");
    text.push_str(&format!("Host {}\n", files.alias));
    text.push_str(&format!("  HostName {}\n", conf_value(&machine.hostname)));
    if let Some(user) = &machine.username {
        // Quoted like every other value, so a username carrying `:` (Freestyle's
        // `<vm-id>:<token>`) is one `User` value rather than a directive; quotes, backslashes and
        // whitespace are already refused by `validate_cloud_username`.
        text.push_str(&format!("  User {}\n", conf_value(user)));
    }
    if let Some(port) = machine.port {
        text.push_str(&format!("  Port {port}\n"));
    }
    if let Some(identity) = &machine.identity_file {
        text.push_str(&format!("  IdentityFile {}\n", conf_value(identity)));
        text.push_str("  IdentitiesOnly yes\n");
    }
    if let Some(certificate) = &machine.certificate_file {
        // Short-lived certificate next to the key (`instabox` writes `<sandbox-id>-cert.pub`, a
        // name ssh cannot derive from the key, so it must be named here).
        text.push_str(&format!(
            "  CertificateFile {}\n",
            conf_value(&certificate.to_string_lossy())
        ));
    }
    if let Some(proxy) = &machine.proxy_command {
        // Verbatim, unquoted: it is a command line ssh runs itself, and quoting it would make
        // ssh look for a binary literally named `gh cs ssh ...`. `write_machine_files_in` has
        // already refused any value carrying a newline, so it cannot smuggle a second directive.
        text.push_str(&format!("  ProxyCommand {proxy}\n"));
    }
    // The per-machine files are the only trust store consulted: the user's global known_hosts is
    // neither read nor written, so a cloud machine cannot inherit or poison local host trust.
    text.push_str(&format!(
        "  UserKnownHostsFile {}\n",
        conf_value(&files.known_hosts.to_string_lossy())
    ));
    text.push_str(&format!("  GlobalKnownHostsFile {null}\n"));
    text.push_str(if machine.pinned_host_key.is_some() {
        "  StrictHostKeyChecking yes\n"
    } else {
        "  StrictHostKeyChecking accept-new\n"
    });
    text.push_str("  UpdateHostKeys no\n");
    text.push_str("  HashKnownHosts no\n");
    text.push_str("  BatchMode yes\n");
    text.push_str("  ControlMaster no\n");
    text.push_str("  ControlPath none\n");
    text.push_str("  PermitLocalCommand no\n");
    text.push_str("  RequestTTY no\n");
    text.push_str("  ConnectTimeout 10\n");
    text.push_str("  ServerAliveInterval 15\n");
    text.push_str("  ServerAliveCountMax 3\n");
    text.push_str("  LogLevel ERROR\n");
    // Deliberately absent: ClearAllForwardings. It clears the command-line `-L` on some OpenSSH
    // versions, and this conf exists precisely so one `-L` can be passed on purpose.
    text
}

/// The argv for one forward: the per-machine conf, then `-N` with the unix-socket `-L`.
///
/// `ExitOnForwardFailure=yes` makes ssh exit instead of running forwardless, and
/// `StreamLocalBindUnlink=yes` lets a socket left by a crashed run be replaced. Neither can prove
/// the *remote* path exists - only the probe in [`start`] can.
pub fn forward_argv(files: &MachineFiles, spec: &ForwardSpec) -> Result<Vec<String>, IpcError> {
    forward_argv_with(&crate::ssh::exec::resolve_ssh_program()?, files, spec)
}

/// [`forward_argv`] with the ssh program named by the caller.
///
/// Split out so a test can hand the forward a stub program and observe what only the stub can
/// show - the mode ssh leaves its socket in, and a run that never binds - without rewriting this
/// process's `PATH`.
fn forward_argv_with(
    program: &str,
    files: &MachineFiles,
    spec: &ForwardSpec,
) -> Result<Vec<String>, IpcError> {
    spec.validate()?;
    let local = spec.local_socket.to_string_lossy().into_owned();
    Ok(vec![
        program.to_string(),
        "-N".to_string(),
        "-o".to_string(),
        "ExitOnForwardFailure=yes".to_string(),
        "-o".to_string(),
        "StreamLocalBindUnlink=yes".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=3".to_string(),
        "-F".to_string(),
        files.conf.to_string_lossy().into_owned(),
        "-L".to_string(),
        format!("{local}:{}", spec.remote_socket),
        files.alias.clone(),
    ])
}

/// Writes the machine's files under `~/.ssh/ferryx-cloud/` and starts its forward.
pub async fn start(machine: &MachineSsh, spec: &ForwardSpec) -> Result<SocketForward, IpcError> {
    let files = write_machine_files(machine)?;
    start_with_files(&files, spec).await
}

/// [`start`] against files the caller already wrote (tests own a disposable directory).
///
/// Readiness is proven, not assumed: the local socket must appear within [`LOCAL_SOCKET_BUDGET`],
/// it is chmod 0600, and the far side must answer [`PROBE_REQUEST`] with a JSON object whose `type`
/// is `protocol_info` within [`PROBE_BUDGET`]. Any failure kills ssh and removes the socket, so a
/// failed start leaves neither a child nor a file behind.
pub async fn start_with_files(
    files: &MachineFiles,
    spec: &ForwardSpec,
) -> Result<SocketForward, IpcError> {
    start_with_program(&crate::ssh::exec::resolve_ssh_program()?, files, spec).await
}

/// [`start_with_files`] against a named ssh program (the seam a stub-driven test uses).
async fn start_with_program(
    program: &str,
    files: &MachineFiles,
    spec: &ForwardSpec,
) -> Result<SocketForward, IpcError> {
    let argv = forward_argv_with(program, files, spec)?;
    let local = spec.local_socket.clone();
    if let Some(parent) = local.parent() {
        ensure_private_dir(parent)?;
    }
    // A stale socket from a crashed run is not a forward: unlinking it means the readiness check
    // below can only succeed for a socket this ssh created (`StreamLocalBindUnlink=yes` makes ssh
    // itself replace a leftover entry, but it only does so for the name it is told to bind, so
    // removing it first is what makes "this ssh bound it" checkable at all).
    let _ = std::fs::remove_file(&local);

    // The child holds a controlling terminal this process owns the master of: when the owner dies
    // for any reason - including `SIGKILL`, where `Drop` never runs - the kernel hangs the terminal
    // up and ssh takes its default `SIGHUP`, so no ssh survives to serve a socket nobody owns.
    let (terminal, slave_name) = open_terminal()?;

    let mut command = crate::util::no_window_tokio_command(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // SAFETY: the closure runs between `fork` and `exec`, where only async-signal-safe calls are
    // permitted: `setsid`, `open`, `ioctl`, `dup2` and `close` on a path captured before the fork.
    // It allocates nothing and touches no shared state.
    let slave_for_child = slave_name.clone();
    unsafe {
        command.as_std_mut().pre_exec(move || {
            become_terminal_owner(&slave_for_child)
                .map_err(|error| std::io::Error::other(format!("terminal: {error}")))
        });
    }
    let child = command
        .spawn()
        .map_err(|error| io_error(format!("Failed to start SSH forward: {error}")))?;
    let pid = child.id();
    let mut guard = SocketForward {
        child: Some(child),
        local: local.clone(),
        pid,
        published: None,
        terminal: Some(terminal),
    };

    match establish(&mut guard, spec).await {
        Ok(()) => Ok(guard),
        Err(error) => {
            // The guard's own teardown is what guarantees "no leftover child, no leftover file".
            guard.shutdown().await;
            // The ended pid rides the error so a caller - and the tests - can prove the child is
            // gone with `kill -0` instead of trusting that teardown ran.
            Err(match pid {
                Some(pid) => error.with_details(serde_json::json!({ "pid": pid })),
                None => error,
            })
        }
    }
}

async fn establish(guard: &mut SocketForward, spec: &ForwardSpec) -> Result<(), IpcError> {
    let local = guard.local.clone();
    let child = guard
        .child
        .as_mut()
        .ok_or_else(|| IpcError::internal("Forward guard has no child"))?;

    // Bound 1: the local socket must exist. A child that exits first is reported immediately
    // rather than after the budget, carrying ssh's own diagnosis.
    //
    // A poll is unavoidable here: no process we can see - not ssh, not sshd - signals when the
    // forward is bound, and a filesystem watch would be a new dependency for a 15 s wait. The
    // interval keeps a stalled ssh from burning a core for the whole budget while staying far
    // below the time a healthy forward needs to bind.
    let appeared = tokio::time::timeout(LOCAL_SOCKET_BUDGET, async {
        loop {
            if local.exists() {
                return Ok(());
            }
            if let Ok(Some(status)) = child.try_wait() {
                let detail = match drain_stderr(child).await {
                    Some(text) if !text.trim().is_empty() => format!("; ssh: {}", text.trim()),
                    _ => String::new(),
                };
                return Err(io_error(format!(
                    "SSH forward exited before binding {}: {status}{detail}",
                    local.display()
                )));
            }
            tokio::time::sleep(BIND_POLL_INTERVAL).await;
        }
    })
    .await;
    match appeared {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            return Err(io_error(format!(
                "SSH forward did not create {} within {}s",
                local.display(),
                LOCAL_SOCKET_BUDGET.as_secs()
            )))
        }
    }

    // Recorded while the name is certainly this forward's own, so teardown can tell its entry from
    // one a later forward for the same name has since published.
    guard.published = published_name(&local);

    // Bound 2: the socket is ours alone. ssh creates it 0600 already; enforcing it means a
    // different umask or a system ssh cannot widen it behind our back.
    let to_secure = local.clone();
    let secured = crate::ipc::run_blocking(move || -> Result<bool, IpcError> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&to_secure, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| io_error(format!("Cannot secure {}: {error}", to_secure.display())))?;
        let mode = std::fs::metadata(&to_secure)
            .map_err(|error| io_error(format!("Cannot stat {}: {error}", to_secure.display())))?
            .permissions()
            .mode()
            & 0o777;
        Ok(mode == 0o600)
    })
    .await?;
    if !secured {
        return Err(io_error(format!(
            "Forwarded socket {} is not owner-only (0600)",
            local.display()
        )));
    }

    // Bound 3: the remote end answers. This is the check `ExitOnForwardFailure` cannot make: a
    // missing remote path still binds the local socket, and the first connection then sees EOF.
    match tokio::time::timeout(PROBE_BUDGET, probe(&local, child)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(IpcError::new(
            error.code.clone(),
            format!(
                "SSH forward to {} is not answering: {error}",
                spec.remote_socket
            ),
        )),
        Err(_) => Err(io_error(format!(
            "SSH forward to {} did not answer the readiness probe within {}s",
            spec.remote_socket,
            PROBE_BUDGET.as_secs()
        ))),
    }
}

/// Opens a pty pair and returns the master together with the slave's path.
///
/// The master is marked `CLOEXEC`: if the child inherited a copy, the terminal would never be hung
/// up when this process died (the child would still hold a master reference), which is precisely
/// the property the forward depends on. Measured on macOS with OpenSSH 10.5 - with the master
/// inherited the ssh child survived the owner's `SIGKILL`; with it `CLOEXEC` the child died.
fn open_terminal() -> Result<(OwnedFd, CString), IpcError> {
    // SAFETY: `posix_openpt`/`grantpt`/`unlockpt`/`ptsname` are async-signal-free libc calls on a
    // descriptor this function just created; `ptsname` returns a static string this process copies
    // before any other call can run, and the descriptor is immediately owned by `OwnedFd` so it
    // cannot leak on an early return.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master < 0 {
            return Err(io_error(format!(
                "Cannot open a terminal for the SSH forward: {}",
                std::io::Error::last_os_error()
            )));
        }
        let master = OwnedFd::from_raw_fd(master);
        if libc::grantpt(master.as_raw_fd()) != 0 || libc::unlockpt(master.as_raw_fd()) != 0 {
            return Err(io_error(format!(
                "Cannot prepare the forward terminal: {}",
                std::io::Error::last_os_error()
            )));
        }
        // Cleared in the child, not inherited - see the note above.
        if libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) != 0 {
            return Err(io_error(format!(
                "Cannot mark the forward terminal close-on-exec: {}",
                std::io::Error::last_os_error()
            )));
        }
        let name = libc::ptsname(master.as_raw_fd());
        if name.is_null() {
            return Err(io_error("Cannot name the forward terminal"));
        }
        let name = std::ffi::CStr::from_ptr(name).to_owned();
        Ok((master, name))
    }
}

/// `TIOCSCTTY` is not declared for Apple targets by `libc` 0.2, though `ioctl.h` defines it (read
/// from the SDK on the build host: `0x20007461`, the same value the BSDs use).
#[cfg(target_os = "macos")]
const TIOCSCTTY_REQUEST: libc::c_ulong = 0x2000_7461;
#[cfg(not(target_os = "macos"))]
const TIOCSCTTY_REQUEST: libc::c_ulong = libc::TIOCSCTTY as libc::c_ulong;

/// Makes the child a session leader whose controlling terminal is the pty slave.
///
/// Runs after `fork`, before `exec`, so it must stay async-signal-safe: no allocation, no locks,
/// no formatting. `setsid` detaches the child from this process's terminal and makes it a session
/// leader with no controlling terminal, which is the precondition `TIOCSCTTY` requires.
fn become_terminal_owner(slave: &CString) -> Result<(), String> {
    // SAFETY: async-signal-safe calls on a path captured before the fork; every failure is
    // reported as an error rather than ignored, so a forward that could not be supervised never
    // reports success.
    unsafe {
        if libc::setsid() < 0 {
            return Err("setsid failed".to_string());
        }
        let fd = libc::open(slave.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        if fd < 0 {
            return Err("cannot open the terminal slave".to_string());
        }
        if libc::ioctl(fd, TIOCSCTTY_REQUEST, 0) != 0 {
            let _ = libc::close(fd);
            return Err("cannot attach the controlling terminal".to_string());
        }
        // The slave stays open as stdin: with every descriptor closed the terminal has no holder
        // in the child, and a hangup on owner death was measured to be ignored. ssh reads no stdin
        // under `-N`, so passing the tty here changes nothing it does.
        if fd != 0 {
            if libc::dup2(fd, 0) < 0 {
                let _ = libc::close(fd);
                return Err("cannot redirect stdin".to_string());
            }
            let _ = libc::close(fd);
        }
        Ok(())
    }
}

/// Reads whatever ssh has written to stderr. A live child keeps the pipe open, so this is only
/// meaningful once it has exited; the read is bounded either way.
async fn drain_stderr(child: &mut tokio::process::Child) -> Option<String> {
    use tokio::io::AsyncReadExt;
    let stderr = child.stderr.take()?;
    let mut bytes = Vec::new();
    let _ = stderr
        .take(PROBE_REPLY_LIMIT as u64)
        .read_to_end(&mut bytes)
        .await;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Sends one probe line through the forwarded socket and requires a JSON reply.
///
/// `child` is only inspected to explain an EOF: a closed connection is a definite answer, so the
/// error names ssh's own stderr when it exited instead of leaving the operator with "EOF".
async fn probe(local: &Path, child: &mut tokio::process::Child) -> Result<(), IpcError> {
    let mut stream = tokio::net::UnixStream::connect(local)
        .await
        .map_err(|error| io_error(format!("Cannot connect to forwarded socket: {error}")))?;
    {
        use tokio::io::AsyncWriteExt;
        stream
            .write_all(format!("{PROBE_REQUEST}\n").as_bytes())
            .await
            .map_err(|error| io_error(format!("Cannot write the readiness probe: {error}")))?;
    }
    let mut reply = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        use tokio::io::AsyncReadExt;
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|error| io_error(format!("Cannot read the readiness reply: {error}")))?;
        if count == 0 {
            let detail = match child.try_wait() {
                Ok(Some(_)) => match drain_stderr(child).await {
                    Some(text) if !text.trim().is_empty() => format!("; ssh: {}", text.trim()),
                    _ => String::new(),
                },
                _ => String::new(),
            };
            return Err(io_error(format!(
                "Forwarded socket closed before answering the readiness probe{detail}"
            )));
        }
        reply.extend_from_slice(&chunk[..count]);
        if reply.len() > PROBE_REPLY_LIMIT {
            return Err(io_error("Readiness reply exceeded the probe limit"));
        }
        if let Some(end) = reply.iter().position(|byte| *byte == b'\n') {
            reply.truncate(end);
            break;
        }
        // A reply that is already complete JSON needs no newline to be accepted.
        if serde_json::from_slice::<serde_json::Value>(&reply).is_ok() {
            break;
        }
    }
    validate_reply(&reply)
}

/// Requires the reply to be a JSON object whose `type` is the daemon's answer.
///
/// Split out from [`probe`] so the rule can be asserted directly: it is the only thing standing
/// between "some local process answered" and "the box's RPC daemon answered".
fn validate_reply(reply: &[u8]) -> Result<(), IpcError> {
    let value: serde_json::Value = serde_json::from_slice(reply)
        .map_err(|error| io_error(format!("Readiness reply is not JSON: {error}")))?;
    // The reply must be the daemon's *answer*, not a mirror of our request: the request line is
    // itself valid JSON, so a listener that echoes it back parses perfectly. Real omo/mhc RPC
    // daemons answer `get_protocol_info` with
    // `{"type":"response","command":"get_protocol_info","success":true,...}`; legacy fixtures use
    // `{"type":"protocol_info",...}`. Both are distinct from the echoed request (`type:
    // "get_protocol_info"`).
    match value.get("type").and_then(|kind| kind.as_str()) {
        Some(PROBE_REPLY_TYPE) => Ok(()),
        Some("response")
            if value.get("command").and_then(|cmd| cmd.as_str()) == Some("get_protocol_info")
                && value.get("success").and_then(|ok| ok.as_bool()) != Some(false) =>
        {
            Ok(())
        }
        Some(other) => Err(io_error(format!(
            "Readiness reply is not a {PROBE_REPLY_TYPE} answer (type: {other})"
        ))),
        None => Err(io_error(format!(
            "Readiness reply carries no type; expected {PROBE_REPLY_TYPE}"
        ))),
    }
}

/// Creates `dir` 0700 without following a symlink planted where it should be.
///
/// `create_dir_all` plus `chmod` is not safe here: `chmod` follows symlinks, so a link standing at
/// the socket directory would silently re-mode whatever it points at (a pre-created
/// `/tmp/ferryx-cloud-<uid>` on a shared `/tmp`, or any caller-supplied path). The directory is
/// therefore created one component at a time, the final component is checked with `lstat` before
/// and after the mode is applied, and the `chmod` is aimed at an open descriptor rather than a
/// path so it cannot be redirected.
///
/// Only the *final* component is held to the no-symlink rule. Ancestors are followed as the
/// platform intends - on macOS `$TMPDIR` and even `/var` are symlinks (`/var -> /private/var`), so
/// refusing a symlinked ancestor would reject every ordinary temp path; refusing one here would
/// break the default socket location without protecting anything, because the entry this function
/// owns and modes is the last component only.
fn ensure_private_dir(dir: &Path) -> Result<(), IpcError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    create_dir_no_follow(dir)?;
    // `lstat`, never `stat`: a symlink swapped in since the create above must not be chmod'd
    // through, and must not be accepted as the directory either.
    let meta = std::fs::symlink_metadata(dir)
        .map_err(|error| io_error(format!("Cannot stat {}: {error}", dir.display())))?;
    if meta.file_type().is_symlink() {
        return Err(invalid(format!(
            "Refusing symlinked socket directory {}",
            dir.display()
        )));
    }
    if !meta.is_dir() {
        return Err(invalid(format!(
            "Refusing non-directory socket path {}",
            dir.display()
        )));
    }
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err(invalid(format!(
            "Refusing socket directory {} owned by uid {}",
            dir.display(),
            meta.uid()
        )));
    }
    // `O_NOFOLLOW | O_DIRECTORY`: the directory is opened, not resolved through a link, and the
    // mode is applied to that descriptor - so a link swapped in after the `lstat` above still
    // cannot receive the `chmod`.
    {
        use std::os::unix::fs::OpenOptionsExt;
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(dir)
            .map_err(|error| {
                io_error(format!(
                    "Cannot open {} without following a link: {error}",
                    dir.display()
                ))
            })?;
        handle
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error(format!("Cannot secure {}: {error}", dir.display())))?;
    }
    // The mode must have landed on this directory, not on a target a symlink redirects to.
    let after = std::fs::symlink_metadata(dir)
        .map_err(|error| io_error(format!("Cannot stat {}: {error}", dir.display())))?;
    if after.file_type().is_symlink() {
        return Err(invalid(format!(
            "Refusing symlinked socket directory {}",
            dir.display()
        )));
    }
    if after.permissions().mode() & 0o777 != 0o700 {
        return Err(io_error(format!(
            "{} is not owner-only after the update",
            dir.display()
        )));
    }
    Ok(())
}

/// `create_dir_all` that refuses a symlink standing where a component must be a directory.
///
/// An existing ancestor is followed (see [`ensure_private_dir`]); a component this call has to
/// create must not be a symlink, and a component that already exists as a symlink at the *final*
/// position is refused here before the mode is touched.
fn create_dir_no_follow(dir: &Path) -> Result<(), IpcError> {
    let mut current = PathBuf::new();
    let last = dir.components().count();
    for (index, component) in dir.components().enumerate() {
        current.push(component);
        let is_final = index + 1 == last;
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if is_final {
                    return Err(invalid(format!(
                        "Refusing symlinked socket directory {}",
                        current.display()
                    )));
                }
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(invalid(format!(
                    "Refusing non-directory socket path {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    // Another forward for this user created it between the stat and the create; a
                    // directory is what we wanted either way.
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(io_error(format!(
                            "Cannot create {}: {error}",
                            current.display()
                        )))
                    }
                }
            }
            Err(error) => {
                return Err(io_error(format!(
                    "Cannot stat {}: {error}",
                    current.display()
                )))
            }
        }
    }
    Ok(())
}

/// Creates the file 0600 and normalizes an existing one, so a file left wide by an earlier run is
/// tightened rather than trusted.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), IpcError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error(format!("Cannot open {}: {error}", path.display())))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| io_error(format!("Cannot secure {}: {error}", path.display())))?;
    file.write_all(bytes)
        .map_err(|error| io_error(format!("Cannot write {}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command as StdCommand;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn current_user() -> String {
        std::env::var("USER").expect("fixture login user")
    }

    fn find_tool(candidates: &[&str]) -> Option<PathBuf> {
        candidates
            .iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
    }

    const SSHD: &[&str] = &["/usr/sbin/sshd", "/usr/local/sbin/sshd", "/sbin/sshd"];
    const KEYGEN: &[&str] = &[
        "/usr/bin/ssh-keygen",
        "/usr/local/bin/ssh-keygen",
        "/bin/ssh-keygen",
    ];

    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .expect("path exists")
            .permissions()
            .mode()
            & 0o777
    }

    /// What a stand-in listener does when the probe line arrives.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ProbeAnswer {
        /// The daemon's own reply: a JSON object typed `protocol_info`.
        ProtocolInfo,
        /// Reflects the request straight back. The request line is valid JSON, so this passes any
        /// "did it parse?" test while proving nothing about who answered - the shape that made the
        /// old probe accept a mirror.
        Mirror,
        /// Accepts and never answers, the shape of a wedged daemon.
        Silent,
    }

    /// A unix listener standing in for the box's RPC socket: it answers the readiness probe with
    /// JSON and echoes every other payload back byte for byte.
    struct Echo {
        path: PathBuf,
        served: Arc<AtomicUsize>,
        _dir: tempfile::TempDir,
    }

    impl Echo {
        fn start() -> Self {
            Self::start_answering(ProbeAnswer::ProtocolInfo)
        }

        fn start_answering(answer: ProbeAnswer) -> Self {
            let dir = tempfile::tempdir().expect("echo directory");
            let path = dir.path().join("remote.sock");
            let listener = tokio::net::UnixListener::bind(&path).expect("bind echo socket");
            let served = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&served);
            tokio::spawn(async move {
                loop {
                    let Ok((mut stream, _)) = listener.accept().await else {
                        return;
                    };
                    let counter = Arc::clone(&counter);
                    tokio::spawn(async move {
                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                        let mut buffer = vec![0u8; 4096];
                        loop {
                            let count = match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(count) => count,
                            };
                            counter.fetch_add(1, Ordering::SeqCst);
                            let payload = &buffer[..count];
                            let is_probe = payload
                                .windows(PROBE_REQUEST.len())
                                .any(|window| window == PROBE_REQUEST.as_bytes());
                            if is_probe && answer == ProbeAnswer::Silent {
                                // Hold the connection open without answering; the probe's own
                                // budget has to end it.
                                std::future::pending::<()>().await;
                                return;
                            }
                            let reply: &[u8] = if !is_probe {
                                payload
                            } else if answer == ProbeAnswer::Mirror {
                                payload
                            } else {
                                b"{\"type\":\"protocol_info\",\"protocol_version\":1}\n"
                            };
                            if stream.write_all(reply).await.is_err() {
                                return;
                            }
                        }
                    });
                }
            });
            Self {
                path,
                served,
                _dir: dir,
            }
        }

        fn accepts(&self) -> usize {
            self.served.load(Ordering::SeqCst)
        }
    }

    /// A private loopback sshd with its own host key, authorized key and port.
    struct Fixture {
        dir: tempfile::TempDir,
        port: u16,
        child: Option<std::process::Child>,
        sshd_log: String,
    }

    impl Fixture {
        /// `None` when this host has no sshd: the todo allows the live case to be skipped, and
        /// task 13 then carries the live proof.
        fn start() -> Option<Self> {
            let sshd = find_tool(SSHD)?;
            let keygen = find_tool(KEYGEN)?;
            use std::os::unix::process::CommandExt;
            let dir = tempfile::tempdir().expect("fixture directory");
            let root = dir.path();
            for name in ["host_key", "user_key"] {
                let output = StdCommand::new(&keygen)
                    .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                    .arg(root.join(name))
                    .output()
                    .expect("ssh-keygen runs");
                assert!(
                    output.status.success(),
                    "ssh-keygen {name}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
            let port = reservation.local_addr().expect("reserved address").port();
            drop(reservation);
            let config = root.join("sshd_config");
            std::fs::write(
                &config,
                format!(
                    "Port {port}\n\
                     ListenAddress 127.0.0.1\n\
                     HostKey {}\n\
                     AuthorizedKeysFile {}\n\
                     PubkeyAuthentication yes\n\
                     PasswordAuthentication no\n\
                     KbdInteractiveAuthentication no\n\
                     UsePAM no\n\
                     StrictModes no\n\
                     AllowStreamLocalForwarding yes\n\
                     AllowUsers {}\n\
                     LogLevel INFO\n",
                    root.join("host_key").display(),
                    root.join("user_key.pub").display(),
                    current_user(),
                ),
            )
            .expect("sshd config");
            let mut command = StdCommand::new(sshd);
            command
                .args(["-D", "-e", "-f"])
                .arg(&config)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .process_group(0);
            let mut child = command.spawn().expect("private loopback sshd prerequisite");
            let stderr = child.stderr.take().expect("sshd stderr pipe");
            let mut log = String::new();
            // Readiness is the server's own "listening" line, not a timer: the first connection is
            // attempted as soon as sshd announces itself.
            let reader = std::io::BufReader::new(stderr);
            let mut listening = false;
            for line in std::io::BufRead::lines(reader) {
                let Ok(line) = line else { break };
                log.push_str(&line);
                log.push('\n');
                if line.to_ascii_lowercase().contains("server listening") {
                    listening = true;
                    break;
                }
            }
            if !listening {
                let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.wait();
                panic!("sshd did not report a listening socket; log:\n{log}");
            }
            Some(Self {
                dir,
                port,
                child: Some(child),
                sshd_log: log,
            })
        }

        /// Production files, in the fixture's own directory, pinned to this sshd's own host key so
        /// the strict host-key path is what the tests exercise.
        fn files(&self, name: &str) -> MachineFiles {
            let host_key = std::fs::read_to_string(self.dir.path().join("host_key.pub"))
                .expect("fixture host key");
            let machine = MachineSsh {
                name: name.to_string(),
                hostname: "127.0.0.1".to_string(),
                username: Some(current_user()),
                port: Some(self.port),
                identity_file: Some(
                    self.dir
                        .path()
                        .join("user_key")
                        .to_string_lossy()
                        .into_owned(),
                ),
                pinned_host_key: Some(host_key),
                proxy_command: None,
                certificate_file: None,
            };
            write_machine_files_in(&self.dir.path().join("ssh"), &machine).expect("machine files")
        }

        fn spec(&self, name: &str, remote: &Path) -> ForwardSpec {
            ForwardSpec::new(
                self.dir.path().join("ssh").join(format!("{name}.sock")),
                remote.to_string_lossy().into_owned(),
            )
            .expect("forward spec")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(mut child) = self.child.take() {
                // The group leader is sshd, so a per-connection child ends with it.
                let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.wait();
            }
        }
    }

    /// A child still holding this machine's conf is a leak the caller could not see. The conf path
    /// is unique per fixture, so any match is ours.
    fn leftover_ssh(files: &MachineFiles) -> Vec<String> {
        let conf = files.conf.to_string_lossy().into_owned();
        let output = StdCommand::new("/bin/ps")
            .args(["-Ao", "args="])
            .output()
            .expect("ps runs");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.contains(&conf) && !line.contains("ps -Ao"))
            .map(str::to_string)
            .collect()
    }

    fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// The pid a failed [`start_with_files`] reports for the child it ended.
    fn failed_pid(error: &IpcError) -> u32 {
        error
            .details
            .as_ref()
            .and_then(|details| details.get("pid"))
            .and_then(|pid| pid.as_u64())
            .expect("a failed start reports the ssh pid it ended") as u32
    }

    /// Waits for a process to be gone. The child is not ours to wait on after the guard dropped
    /// it, so polling `kill -0` is the only observation available; the bound makes an unproven
    /// claim fail loudly instead of hanging.
    async fn await_dead(pid: u32) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if !alive(pid) {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn a_forward_round_trips_bytes_and_owns_its_socket() {
        let Some(fixture) = Fixture::start() else {
            eprintln!("SKIPPED: no sshd");
            return;
        };
        let echo = Echo::start();
        let files = fixture.files("fx-t6-round-trip");
        let spec = fixture.spec("fx-t6-round-trip", &echo.path);
        let forward = start_with_files(&files, &spec)
            .await
            .unwrap_or_else(|error| panic!("forward failed: {error}\nsshd:\n{}", fixture.sshd_log));

        // The socket the probe already answered through is owner-only.
        assert_eq!(mode_of(forward.local_socket()), 0o600);

        // Bytes round-trip: a payload that is not the probe line comes back unchanged.
        let payload = b"fx-round-trip-0123456789";
        let mut stream = tokio::net::UnixStream::connect(forward.local_socket())
            .await
            .expect("connect through the forwarded socket");
        {
            use tokio::io::AsyncWriteExt;
            stream.write_all(payload).await.expect("write payload");
        }
        let mut received = vec![0u8; payload.len()];
        {
            use tokio::io::AsyncReadExt;
            stream
                .read_exact(&mut received)
                .await
                .expect("read the echo");
        }
        assert_eq!(
            &received, payload,
            "bytes must round-trip through the forward"
        );
        drop(stream);
        assert!(
            echo.accepts() > 0,
            "the listener must have served the probe and the echo"
        );

        // `shutdown` awaits the child, so "no leftover child" is asserted on a settled state
        // rather than by waiting.
        let pid = forward.pid().expect("forward pid");
        let local = forward.local_socket().to_path_buf();
        forward.shutdown().await;
        assert!(!local.exists(), "shutdown must remove the local socket");
        assert!(!alive(pid), "shutdown must leave no ssh child (pid {pid})");
        assert!(
            leftover_ssh(&files).is_empty(),
            "no ssh may still reference {}: {:?}",
            files.conf.display(),
            leftover_ssh(&files)
        );
    }

    #[tokio::test]
    async fn dropping_the_guard_removes_the_child_and_the_socket() {
        let Some(fixture) = Fixture::start() else {
            eprintln!("SKIPPED: no sshd");
            return;
        };
        let echo = Echo::start();
        let files = fixture.files("fx-t6-drop");
        let spec = fixture.spec("fx-t6-drop", &echo.path);
        let forward = start_with_files(&files, &spec)
            .await
            .unwrap_or_else(|error| panic!("forward failed: {error}\nsshd:\n{}", fixture.sshd_log));
        let pid = forward.pid().expect("forward pid");
        let local = forward.local_socket().to_path_buf();
        drop(forward);
        // The file removal is synchronous; the child's death is an external state change, awaited
        // under a bound.
        assert!(
            !local.exists(),
            "dropping the guard must remove the local socket"
        );
        assert!(
            await_dead(pid).await,
            "dropping the guard must end the ssh child (pid {pid})"
        );
        assert!(
            leftover_ssh(&files).is_empty(),
            "no ssh may still reference {}: {:?}",
            files.conf.display(),
            leftover_ssh(&files)
        );
    }

    #[tokio::test]
    async fn a_missing_remote_socket_fails_the_probe_and_leaves_nothing_behind() {
        let Some(fixture) = Fixture::start() else {
            eprintln!("SKIPPED: no sshd");
            return;
        };
        let files = fixture.files("fx-t6-missing");
        // No listener at this path: ssh still binds the local socket, which is exactly why the
        // readiness probe - not ExitOnForwardFailure - has to decide.
        let absent = fixture.dir.path().join("DOES-NOT-EXIST.sock");
        let spec = fixture.spec("fx-t6-missing", &absent);
        let started =
            tokio::time::timeout(Duration::from_secs(25), start_with_files(&files, &spec))
                .await
                .expect("the readiness probe must fail within 25s");
        let error = started
            .err()
            .expect("a missing remote socket must not report success");
        assert_eq!(error.code, IpcErrorCode::IoError, "{error}");
        assert!(
            error.message.contains("readiness probe") || error.message.contains("not answering"),
            "the failure must name the probe: {}",
            error.message
        );
        // `kill -0` on the pid the failure reports: the child must be gone, not merely unlinked.
        let pid = failed_pid(&error);
        assert!(
            !alive(pid),
            "a failed start must kill the ssh child (pid {pid})"
        );
        assert!(
            !spec.local_socket.exists(),
            "a failed start must not leave the local socket behind"
        );
        assert!(
            leftover_ssh(&files).is_empty(),
            "a failed start must kill ssh: {:?}",
            leftover_ssh(&files)
        );
    }

    /// An alias nothing answers on. ssh fails before binding, and its own diagnosis is what the
    /// caller sees - the alias does not squat the whole 15 s bind budget.
    #[tokio::test]
    async fn an_unreachable_alias_fails_with_ssh_error_and_leaves_nothing_behind() {
        let Some(fixture) = Fixture::start() else {
            eprintln!("SKIPPED: no sshd");
            return;
        };
        // Reserved then released: nothing listens on this port, so the connection is refused.
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let dead_port = reservation.local_addr().expect("reserved address").port();
        drop(reservation);

        let host_key = std::fs::read_to_string(fixture.dir.path().join("host_key.pub"))
            .expect("fixture host key");
        let machine = MachineSsh {
            name: "fx-t6-unreachable".to_string(),
            hostname: "127.0.0.1".to_string(),
            username: Some(current_user()),
            port: Some(dead_port),
            identity_file: Some(
                fixture
                    .dir
                    .path()
                    .join("user_key")
                    .to_string_lossy()
                    .into_owned(),
            ),
            pinned_host_key: Some(host_key),
            proxy_command: None,
            certificate_file: None,
        };
        let files = write_machine_files_in(&fixture.dir.path().join("ssh"), &machine)
            .expect("machine files");
        let spec = ForwardSpec::new(
            fixture
                .dir
                .path()
                .join("ssh")
                .join("fx-t6-unreachable.sock"),
            "/home/daytona/.ferryx-cloud/rpc.sock",
        )
        .expect("forward spec");

        let started = tokio::time::timeout(Duration::from_secs(25), start_with_files(&files, &spec))
            .await
            .expect("an unreachable alias must not spend the whole bind budget");
        let error = started
            .err()
            .expect("an unreachable alias must not report success");
        assert_eq!(error.code, IpcErrorCode::IoError, "{error}");
        // ssh's own words reach the caller, so the failure is diagnosable without the ssh log.
        assert!(
            error.message.contains("ssh: ") || error.message.contains("exited before binding"),
            "the failure must carry ssh's diagnosis: {}",
            error.message
        );
        let pid = failed_pid(&error);
        assert!(
            !alive(pid),
            "a failed start must kill the ssh child (pid {pid})"
        );
        assert!(
            !spec.local_socket.exists(),
            "a failed start must not leave the local socket behind"
        );
        assert!(
            leftover_ssh(&files).is_empty(),
            "a failed start must kill ssh: {:?}",
            leftover_ssh(&files)
        );
    }

    #[test]
    fn a_name_is_a_bare_file_stem_and_never_a_path() {
        for bad in [
            "",
            "..",
            ".hidden",
            "-oProxyCommand",
            "a/b",
            "a\\b",
            "a b",
            "a\nb",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} must be refused");
        }
        for good in ["fx-daytona-1", "fx_e2e.e2b", "ab"] {
            assert!(validate_name(good).is_ok(), "{good:?} must be accepted");
        }
    }

    #[test]
    fn a_pinned_key_is_one_line_and_must_be_a_public_key() {
        let pattern = known_hosts_pattern("127.0.0.1", Some(2222));
        assert_eq!(pattern, "[127.0.0.1]:2222");
        assert_eq!(known_hosts_pattern("box.example", None), "box.example");
        assert_eq!(known_hosts_pattern("box.example", Some(22)), "box.example");
        assert_eq!(known_hosts_pattern("fd00::5", None), "[fd00::5]");
        let entry = known_hosts_entry(&pattern, "ssh-ed25519 AAAAB3 comment").unwrap();
        assert_eq!(entry, "[127.0.0.1]:2222 ssh-ed25519 AAAAB3 comment\n");
        // A provider key line arrives with a trailing newline; that is the normal shape.
        assert_eq!(
            known_hosts_entry(&pattern, "ssh-ed25519 AAAAB3 comment\n").unwrap(),
            "[127.0.0.1]:2222 ssh-ed25519 AAAAB3 comment\n"
        );
        for bad in [
            "",
            "AAAAB3",
            "ssh-ed25519",
            "ssh-ed25519 AAAAB3\nssh-rsa AAAA",
            "ssh-ed25519 AAAAB3\r\nx",
        ] {
            assert!(
                known_hosts_entry(&pattern, bad).is_err(),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn the_conf_pins_the_host_key_and_never_the_global_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut machine = MachineSsh {
            name: "fx-pinned".into(),
            hostname: "10.0.0.5".into(),
            username: Some("daytona".into()),
            port: Some(2222),
            identity_file: Some("/keys/id ed25519".into()),
            pinned_host_key: Some("ssh-ed25519 AAAAB3".into()),
            proxy_command: None,
            certificate_file: None,
        };
        let files = write_machine_files_in(dir.path(), &machine).unwrap();
        let text = std::fs::read_to_string(&files.conf).unwrap();
        assert!(text.contains("Host fx-pinned\n"));
        assert!(text.contains("  HostName \"10.0.0.5\"\n"));
        assert!(text.contains("  User \"daytona\"\n"));
        assert!(text.contains("  Port 2222\n"));
        assert!(
            text.contains("  IdentityFile \"/keys/id ed25519\"\n"),
            "{text}"
        );
        assert!(text.contains("  StrictHostKeyChecking yes\n"));
        assert!(text.contains("  GlobalKnownHostsFile /dev/null\n"));
        assert!(text.contains(&format!(
            "  UserKnownHostsFile \"{}\"\n",
            files.known_hosts.display()
        )));
        // The conf never forwards by itself and never names the user's global files.
        for forbidden in [
            "~/.ssh/config",
            "~/.ssh/known_hosts",
            "  LocalForward",
            "  RemoteForward",
            "  DynamicForward",
            "  ClearAllForwardings",
        ] {
            assert!(
                !text.contains(forbidden),
                "conf must not contain {forbidden}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&files.known_hosts).unwrap(),
            "[10.0.0.5]:2222 ssh-ed25519 AAAAB3\n"
        );

        // No provider key: first use is pinned instead, still in the per-machine file.
        machine.pinned_host_key = None;
        let files = write_machine_files_in(dir.path(), &machine).unwrap();
        let text = std::fs::read_to_string(&files.conf).unwrap();
        assert!(
            text.contains("  StrictHostKeyChecking accept-new\n"),
            "{text}"
        );
        assert!(std::fs::read_to_string(&files.known_hosts)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_conf_emits_the_proxy_command_and_certificate_a_cli_provider_needs() {
        // D1/D5: `SshTarget` carried both, but a conf that omits them cannot connect - `cs.<n>.main`
        // has no DNS record, and InstaBox refuses the bare key. This is the consumer that makes
        // carrying them mean something, so the assertion is on the written bytes.
        let dir = tempfile::tempdir().unwrap();
        let machine = MachineSsh {
            name: "fx-cli".into(),
            hostname: "cs.fx-cli.main".into(),
            username: Some("vscode".into()),
            port: Some(22),
            identity_file: Some("/Users/x/.ssh/codespaces.auto".into()),
            pinned_host_key: None,
            proxy_command: Some(
                "/opt/homebrew/bin/gh cs ssh -c fx-cli --stdio -- -i /Users/x/.ssh/codespaces.auto"
                    .into(),
            ),
            certificate_file: Some(PathBuf::from("/cfg/m_abc-cert.pub")),
        };
        let files = write_machine_files_in(dir.path(), &machine).unwrap();
        let text = std::fs::read_to_string(&files.conf).unwrap();
        assert!(
            text.contains(
                "  ProxyCommand /opt/homebrew/bin/gh cs ssh -c fx-cli --stdio -- \
                 -i /Users/x/.ssh/codespaces.auto\n"
            ),
            "ProxyCommand must be emitted verbatim, unquoted: {text}"
        );
        assert!(
            text.contains("  CertificateFile \"/cfg/m_abc-cert.pub\"\n"),
            "CertificateFile must be emitted: {text}"
        );
        // The bounds every other provider gets still apply with a proxy in place.
        assert!(text.contains("  BatchMode yes\n"), "{text}");
        assert!(text.contains("  StrictHostKeyChecking accept-new\n"), "{text}");
        assert_eq!(mode_of(&files.conf), 0o600);
    }

    /// Todo 21: a real Freestyle target must write files, not fail validation.
    ///
    /// Task 14's live smoke created a VM and then `connect` failed with `Invalid SSH hostname,
    /// username, port or jump host`: `FreestyleProvider::ssh_target` puts the credential in the
    /// *username* as `<vm-id>:<token>` at `<vm-id>.beta-ssh.freestyle.sh`, and the ordinary `User`
    /// token rule refuses `:`. The target is correct and the rule had to be narrower, so this
    /// fixture is the exact shape the provider builds.
    #[test]
    fn a_freestyle_ssh_target_writes_its_conf() {
        let dir = tempfile::tempdir().unwrap();
        // Composed exactly as `FreestyleProvider::ssh_target_for` composes it: the VM id in the
        // host under `.beta-ssh.freestyle.sh`, and `<vm-id>:<token>` as the username.
        let vm_id = "vm-8b09e5167bb24f98a3158f28261b1936";
        let host = format!("{vm_id}.beta-ssh.freestyle.sh");
        let user = format!("{vm_id}:fstok_abc123-XYZ.9");
        let machine = MachineSsh {
            name: "fx-smoke-freestyle".into(),
            hostname: host.clone(),
            username: Some(user.clone()),
            port: Some(22),
            // `SshIdentity::Agent`: the gateway authenticates the username, so there is no key.
            identity_file: None,
            pinned_host_key: None,
            proxy_command: None,
            certificate_file: None,
        };
        let files = write_machine_files_in(dir.path(), &machine)
            .expect("a real Freestyle target must not be refused as an invalid host");
        let text = std::fs::read_to_string(&files.conf).unwrap();
        // Exact lines: the value is emitted quoted, and the `:` stays inside the one `User` value.
        let hostname_line = format!("  HostName \"{host}\"\n");
        assert!(
            text.contains(&hostname_line),
            "the hostname must be written verbatim: {text}"
        );
        let user_line = format!("  User \"{user}\"\n");
        assert!(
            text.contains(&user_line),
            "the <vm-id>:<token> username must be written as one quoted value: {text}"
        );
        // One `User` line: the credential in the username appended no directive of its own.
        assert_eq!(text.matches("  User ").count(), 1, "{text}");
        assert!(text.contains("  Port 22\n"), "{text}");
        // Nothing upstream of the username was loosened: the identity-less shape is Agent auth and
        // the conf still pins the trust store, so the `:` is the only thing that changed.
        assert!(!text.contains("  IdentityFile"), "{text}");
        let pin_line = "  StrictHostKeyChecking accept-new\n";
        assert!(text.contains(pin_line), "{text}");
        assert_eq!(mode_of(&files.conf), 0o600);
    }

    /// The username rule admits `:` and nothing that could turn one value into syntax.
    #[test]
    fn a_cloud_username_is_one_token_and_malicious_ones_are_refused() {
        // Accepted: the real Freestyle shape, and the plain logins other providers use.
        for good in [
            "vm-8b09e5167bb24f98a3158f28261b1936:fstok_abc123-XYZ.9",
            "daytona",
            "vscode",
            "user.name_1-2+tag@realm=x",
            "a:b:c",
        ] {
            assert!(
                validate_cloud_username(good).is_ok(),
                "{good:?} must be accepted"
            );
        }
        // Refused: the forms that would reach ssh as an option, a second directive, a shell
        // command, an argv split, or nothing at all.
        for bad in [
            "",
            "-oProxyCommand=id",
            "user\nProxyCommand id",
            "user\rHost evil",
            "user; rm -rf ~",
            "user name",
            "user\tid",
            "user&whoami",
            "user|id",
            "user$(id)",
            "user`id`",
            "user/path",
            "user\\path",
            "user\"quoted\"",
            "user'quoted'",
            "user\u{0}id",
            "user\u{7f}",
            "us%er",
            "üser",
        ] {
            assert!(
                validate_cloud_username(bad).is_err(),
                "{bad:?} must be refused"
            );
        }
        // The length bound is generous enough for a vm-id plus a token, and still a bound.
        assert!(validate_cloud_username(&"a".repeat(CLOUD_USERNAME_MAX)).is_ok());
        assert!(validate_cloud_username(&"a".repeat(CLOUD_USERNAME_MAX + 1)).is_err());
    }

    /// The rejection happens inside `write_machine_files_in`, i.e. before anything is written.
    #[test]
    fn a_malicious_cloud_username_never_reaches_a_written_conf() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "-oProxyCommand=id",
            "user\nProxyCommand id",
            "user; rm -rf ~",
            "user name",
            "",
        ] {
            let machine = MachineSsh {
                name: "fx-bad-user".into(),
                hostname: "vm-8b09e5167bb24f98a3158f28261b1936.beta-ssh.freestyle.sh".into(),
                username: Some(bad.into()),
                port: Some(22),
                identity_file: None,
                pinned_host_key: None,
                proxy_command: None,
                certificate_file: None,
            };
            let error = write_machine_files_in(dir.path(), &machine)
                .expect_err("a malicious username must not reach the conf");
            assert!(
                error.message.contains("username"),
                "the refusal must name the field for {bad:?}: {}",
                error.message
            );
            assert!(
                !dir.path().join("fx-bad-user.conf").exists(),
                "nothing may be written for {bad:?}"
            );
        }
    }

    #[test]
    fn a_multiline_certificate_file_is_refused_like_a_proxy_command() {
        // F5: `CertificateFile` reaches ssh as a quoted value, so a newline cannot inject a
        // directive - but it does make ssh reject the whole conf with a message about quotes that
        // names nothing. The sibling provider strings are refused on one rule, so this one is too.
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "/cfg/abc-cert.pub\nProxyCommand /bin/sh -c evil",
            "/cfg/abc-cert.pub\rHost evil",
            "/cfg/abc-cert.pub\u{0}suffix",
        ] {
            let machine = MachineSsh {
                name: "fx-cert-inject".into(),
                hostname: "10.0.0.7".into(),
                username: None,
                port: None,
                identity_file: None,
                pinned_host_key: None,
                proxy_command: None,
                certificate_file: Some(PathBuf::from(bad)),
            };
            let error = write_machine_files_in(dir.path(), &machine)
                .expect_err("a multi-line CertificateFile must not reach the conf");
            assert!(
                error.message.contains("CertificateFile"),
                "the refusal must name the field: {}",
                error.message
            );
            assert!(
                !dir.path().join("fx-cert-inject.conf").exists(),
                "nothing may be written for a refused machine"
            );
        }
        // A plain path is still accepted, and is emitted quoted.
        let machine = MachineSsh {
            name: "fx-cert-ok".into(),
            hostname: "10.0.0.8".into(),
            username: None,
            port: None,
            identity_file: None,
            pinned_host_key: None,
            proxy_command: None,
            certificate_file: Some(PathBuf::from("/cfg/abc-cert.pub")),
        };
        let files = write_machine_files_in(dir.path(), &machine).unwrap();
        assert!(std::fs::read_to_string(&files.conf)
            .unwrap()
            .contains("  CertificateFile \"/cfg/abc-cert.pub\"\n"));
    }

    #[test]
    fn a_multiline_proxy_command_is_refused_rather_than_appended() {
        // The value comes from a provider CLI, i.e. from off this machine. Emitted unquoted, a
        // newline in it would append directives to this conf (a second `Host`, a `LocalForward`),
        // so it is refused instead of trusted.
        let dir = tempfile::tempdir().unwrap();
        let machine = MachineSsh {
            name: "fx-inject".into(),
            hostname: "10.0.0.6".into(),
            username: None,
            port: None,
            identity_file: None,
            pinned_host_key: None,
            proxy_command: Some("gh cs ssh --stdio\n  LocalForward 22 127.0.0.1:22".into()),
            certificate_file: None,
        };
        let error = write_machine_files_in(dir.path(), &machine)
            .expect_err("a multi-line ProxyCommand must not reach the conf");
        assert!(
            error.message.contains("ProxyCommand"),
            "{}",
            error.message
        );
        // Nothing was written for that machine.
        assert!(!dir.path().join("fx-inject.conf").exists());
    }

    #[test]
    fn the_written_files_are_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ssh");
        std::fs::create_dir_all(&root).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        let machine = MachineSsh {
            name: "fx-modes".into(),
            hostname: "127.0.0.1".into(),
            username: None,
            port: None,
            identity_file: None,
            pinned_host_key: Some("ssh-ed25519 AAAAB3".into()),
            proxy_command: None,
            certificate_file: None,
        };
        let files = write_machine_files_in(&root, &machine).unwrap();
        assert_eq!(mode_of(&root), 0o700);
        assert_eq!(mode_of(&files.conf), 0o600);
        assert_eq!(mode_of(&files.known_hosts), 0o600);

        // An existing file left wide is tightened, not trusted.
        std::fs::set_permissions(&files.conf, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_machine_files_in(&root, &machine).unwrap();
        assert_eq!(mode_of(&files.conf), 0o600);
    }

    #[test]
    fn the_forward_argv_is_a_stream_local_forward_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let machine = MachineSsh {
            name: "fx-argv".into(),
            hostname: "127.0.0.1".into(),
            username: None,
            port: None,
            identity_file: None,
            pinned_host_key: None,
            proxy_command: None,
            certificate_file: None,
        };
        let files = write_machine_files_in(dir.path(), &machine).unwrap();
        let local = dir.path().join("fx-argv.sock");
        let spec = ForwardSpec::new(&local, "/home/daytona/.ferryx-cloud/rpc.sock").unwrap();
        let args = forward_argv(&files, &spec).unwrap();

        assert_eq!(args[1], "-N");
        assert_eq!(
            args.iter().filter(|arg| *arg == "-L").count(),
            1,
            "exactly one forward"
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("fx-argv"),
            "the alias is the destination and comes last"
        );
        assert_eq!(
            args[args.len() - 2],
            format!("{}:/home/daytona/.ferryx-cloud/rpc.sock", local.display())
        );
        let conf_index = args
            .iter()
            .position(|arg| arg == "-F")
            .expect("-F is present");
        assert_eq!(Path::new(&args[conf_index + 1]), files.conf);
        for required in [
            "ExitOnForwardFailure=yes",
            "StreamLocalBindUnlink=yes",
            "ServerAliveInterval=15",
            "ServerAliveCountMax=3",
        ] {
            assert!(args.iter().any(|arg| arg == required), "missing {required}");
        }
        // No remote command: MODE=A runs the verbs locally, so there is nothing to quote or shell.
        assert!(
            !args.iter().any(|arg| arg.contains(' ')),
            "a forward argv carries no command string: {args:?}"
        );

        // The remote half must be an absolute POSIX path, and the local half absolute too.
        assert!(ForwardSpec::new(&local, "relative/rpc.sock").is_err());
        assert!(ForwardSpec::new(&local, "/abs\0path").is_err());
        assert!(ForwardSpec::new("relative.sock", "/abs/rpc.sock").is_err());
    }

    #[test]
    fn the_default_socket_is_under_a_private_temp_directory() {
        let socket = default_local_socket("fx-e2e-daytona").unwrap();
        assert!(socket.is_absolute());
        assert!(socket.ends_with("fx-e2e-daytona.sock"));
        let parent = socket
            .parent()
            .and_then(Path::file_name)
            .expect("socket has a named parent")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            parent,
            format!("ferryx-cloud-{}", unsafe { libc::geteuid() })
        );
        assert!(default_local_socket("../escape").is_err());
    }

    /// F4: only the daemon's own answer proves the far end.
    ///
    /// A listener that mirrors the request back is the case that made the old JSON-only check
    /// insufficient: the request line is valid JSON, so it parses. The rule has to look at the
    /// reply's `type`.
    #[test]
    fn a_mirrored_request_is_not_a_protocol_info_answer() {
        // The daemon's answer is accepted, with and without the trailing newline.
        for good in [
            r#"{"type":"protocol_info","protocol_version":1}"#,
            "{\"type\":\"protocol_info\",\"protocol_version\":1}\n",
            r#"{"protocol_version":1,"type":"protocol_info"}"#,
        ] {
            assert!(validate_reply(good.trim_end().as_bytes()).is_ok(), "{good}");
        }
        // A mirror of our own request parses as JSON and is still not an answer.
        let mirrored = format!("{PROBE_REQUEST}\n");
        assert!(
            serde_json::from_str::<serde_json::Value>(mirrored.trim()).is_ok(),
            "the premise: the request line is itself valid JSON"
        );
        let error = validate_reply(mirrored.trim().as_bytes())
            .expect_err("a mirror-echo must not pass the probe");
        assert!(
            error.message.contains("protocol_info"),
            "the refusal must name what was expected: {}",
            error.message
        );
        // Not JSON at all, a JSON scalar, and an object with no type are all refused.
        for bad in [
            "",
            "hello\n",
            "42",
            r#"["protocol_info"]"#,
            r#"{"protocol":"protocol_info"}"#,
            r#"{"type":7}"#,
        ] {
            assert!(
                validate_reply(bad.trim_end().as_bytes()).is_err(),
                "{bad:?} must not pass the probe"
            );
        }
    }

    /// The probe's budget is what bounds a daemon that accepts and never answers.
    ///
    /// The production socket only exists while ssh runs, so the deadline is asserted against
    /// [`probe`] directly: a listener that holds the connection open must make it fail, and the
    /// outer budget must be the thing that ends it rather than a listener choice.
    ///
    /// Driven through `start_with_program` and a stub that binds but never answers, because that is
    /// where the budget is applied: `probe` itself has no deadline inside it, so a failure here can
    /// only come from `establish`'s `tokio::time::timeout` around it.
    #[tokio::test]
    async fn a_silent_listener_cannot_hold_the_probe_past_its_budget() {
        assert!(
            PROBE_BUDGET <= Duration::from_secs(10),
            "the probe budget is part of the 25s start bound (15s bind + this): {PROBE_BUDGET:?}"
        );
        assert!(
            LOCAL_SOCKET_BUDGET + PROBE_BUDGET <= Duration::from_secs(25),
            "bind plus probe must stay inside the 25s the contract names"
        );

        let Some(stub) = StubSsh::binding_silently() else {
            eprintln!("SKIPPED: no python3 for the stub ssh");
            return;
        };
        let files = stub.files();
        let spec = stub.spec("fx-silent");
        let started = tokio::time::Instant::now();
        // The margin is generous so a slow host reports a real failure rather than a flaky one.
        let outcome = tokio::time::timeout(
            PROBE_BUDGET + Duration::from_secs(20),
            start_with_program(stub.path_str(), &files, &spec),
        )
        .await
        .expect("a start must not outlive its probe budget by more than the margin");
        let elapsed = started.elapsed();
        let error = outcome.expect_err("a silent listener must not report the far end ready");
        assert!(
            error.message.contains("readiness probe"),
            "the failure must name the probe: {}",
            error.message
        );
        // It failed because the budget ran out, not because the socket vanished immediately.
        assert!(
            elapsed >= PROBE_BUDGET / 2,
            "a silent listener must be given its budget before failing, got {elapsed:?}"
        );
        // And the budget really is the bound: the whole start stays inside bind + probe.
        assert!(
            elapsed < LOCAL_SOCKET_BUDGET + PROBE_BUDGET + Duration::from_secs(20),
            "the start must fail near bind + probe, took {elapsed:?}"
        );
        assert!(
            !spec.local_socket.exists(),
            "a failed start removes the socket its stub bound"
        );
    }

    /// F1: teardown removes this forward's own name, never one a newer forward published.
    ///
    /// Driven through the real filesystem: `Drop` cannot await and the second forward is what a
    /// retry or a second window produces, so both entries have to coexist by inode.
    #[test]
    fn teardown_only_removes_the_name_this_forward_published() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fx-published.sock");
        // The older forward's entry, then the newer one re-binding the same name while the older
        // listener still holds its socket inode open (preventing ext4 immediate inode reuse on
        // Linux and matching the real overlap between two live forwards).
        let _older_listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let older = published_name(&path).expect("bind identity");
        // A second bind on the same path fails while the first listener lives, which is exactly
        // why production unlinks first; do that here too so the newer entry is a new inode.
        std::fs::remove_file(&path).unwrap();
        let _newer_listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let newer = published_name(&path).expect("newer bind identity");
        assert_ne!(older, newer, "a re-bind must be a different inode");

        // Dropping the older forward must leave the newer entry alone.
        remove_published(&path, Some(older));
        assert!(
            std::fs::symlink_metadata(&path).is_ok(),
            "the older forward must not unlink the name the newer one published"
        );
        assert_eq!(
            published_name(&path),
            Some(newer),
            "the name still resolves to the newer listener's inode"
        );

        // The owner of the name removes it on teardown.
        remove_published(&path, Some(newer));
        assert!(
            std::fs::symlink_metadata(&path).is_err(),
            "the forward that owns the name removes it"
        );
    }

    /// F3: a symlink planted at the socket directory is refused, not chmod'd through.
    #[test]
    fn a_symlinked_socket_directory_is_refused_rather_than_chmodded_through() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        std::fs::create_dir(&victim).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = dir.path().join("ferryx-cloud-link");
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        let error = ensure_private_dir(&link).expect_err("a symlinked socket dir must be refused");
        assert!(
            error.message.contains("symlink"),
            "the refusal must name the reason: {}",
            error.message
        );
        // The symlink is untouched, and the directory it points at kept its mode.
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link is still there")
                .file_type()
                .is_symlink(),
            "the link must still be a link"
        );
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o755,
            "the target directory must not have been chmod'd through the link"
        );

        // A regular file where the directory belongs is refused too, not replaced.
        let file = dir.path().join("ferryx-cloud-file");
        std::fs::write(&file, b"not a directory").unwrap();
        assert!(ensure_private_dir(&file).is_err());

        // A fresh directory is created 0700, and an existing one is tightened.
        let fresh = dir.path().join("fresh");
        ensure_private_dir(&fresh).expect("a plain new directory is created");
        assert_eq!(
            std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o700
        );
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&fresh).unwrap();
        assert_eq!(
            std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
            0o700,
            "an existing directory is tightened, not trusted"
        );
    }

    /// The socket mode is enforced by this module, not merely inherited from ssh.
    ///
    /// A stub ssh binds the socket wide (0666) so the assertion is about our own `chmod`: with the
    /// mode left to ssh, a wider umask or a different ssh implementation would publish a socket
    /// any local account could connect to.
    #[tokio::test]
    async fn the_forward_secures_a_socket_ssh_bound_too_widely() {
        let Some(stub) = StubSsh::binding_wide() else {
            eprintln!("SKIPPED: no python3 for the stub ssh");
            return;
        };
        let files = stub.files();
        let spec = stub.spec("fx-wide");
        let started = start_with_program(stub.path_str(), &files, &spec).await;
        let forward = match started {
            Ok(forward) => forward,
            Err(error) => panic!(
                "a wide socket must be tightened, not refused: {error}\nstub: {}",
                stub.log()
            ),
        };
        assert_eq!(
            mode_of(forward.local_socket()),
            0o600,
            "the module must set 0600 itself"
        );
        forward.shutdown().await;
        assert!(
            !spec.local_socket.exists(),
            "teardown removes the socket it published"
        );
    }

    /// A stale socket left by a crashed run must not survive into a start.
    ///
    /// `StreamLocalBindUnlink=yes` makes ssh replace the *name*, but it only does so for the name it
    /// was told to bind, so the entry has to be gone before the probe can mean "this ssh bound it":
    /// the probe below connects through whatever is at the path, and a leftover listener would
    /// answer it.
    #[tokio::test]
    async fn a_stale_socket_from_a_crashed_run_is_replaced_not_reused() {
        let Some(stub) = StubSsh::never_binding() else {
            eprintln!("SKIPPED: no python3 for the stub ssh");
            return;
        };
        let files = stub.files();
        let spec = stub.spec("fx-stale");
        // A live listener squatting the path, pretending to be a forward from a crashed run. It
        // answers the probe correctly, so a start that reused it would report ready.
        let squatter = Echo::start_answering(ProbeAnswer::ProtocolInfo);
        std::fs::copy(&squatter.path, &spec.local_socket).ok();
        if !spec.local_socket.exists() {
            // `copy` cannot copy a socket; bind a fresh listener at the target path instead.
            let listener = tokio::net::UnixListener::bind(&spec.local_socket).unwrap();
            drop(listener);
            let _ = std::fs::remove_file(&spec.local_socket);
            std::os::unix::fs::symlink(&squatter.path, &spec.local_socket).unwrap();
        }
        // The stub ssh never binds, so the only way this can report success is by reusing the
        // entry that was already there.
        let started = tokio::time::timeout(
            LOCAL_SOCKET_BUDGET + Duration::from_secs(5),
            start_with_program(stub.path_str(), &files, &spec),
        )
        .await
        .expect("the bind budget bounds a stub that never binds");
        assert!(
            started.is_err(),
            "a start must not succeed on a socket the previous run left behind"
        );
        assert!(
            std::fs::symlink_metadata(&spec.local_socket).is_err(),
            "the stale entry is gone: it was unlinked, and this ssh never bound it"
        );
    }

    /// A stub ssh, so a test can drive the paths only a misbehaving ssh reaches.
    ///
    /// Real ssh always binds 0600 and always replaces a stale entry, so neither of those two rules
    /// can be observed through it - and a mutation that removed them left every real-ssh test
    /// green. The stub is a python3 script, present on every host this crate builds for.
    struct StubSsh {
        dir: tempfile::TempDir,
        script: PathBuf,
    }

    impl StubSsh {
        /// Writes a python3 stub whose source is given line by line, so indentation is explicit and
        /// no Rust string-continuation rules can strip it.
        fn write(python: &Path, lines: &[&str]) -> Self {
            let stub = Self::create();
            let mut body = format!("#!{}\n", python.display());
            for line in lines {
                body.push_str(line);
                body.push('\n');
            }
            std::fs::write(&stub.script, body).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub.script, std::fs::Permissions::from_mode(0o755)).unwrap();
            stub
        }

        fn binding_wide() -> Option<Self> {
            let python = find_tool(PYTHON3)?;
            // Binds the path from our own `-L local:remote` argument, mode 0666, then serves the
            // probe itself with a daemon-shaped reply.
            Some(Self::write(
                &python,
                &[
                    "import os, socket, sys",
                    "spec = sys.argv[sys.argv.index('-L') + 1]",
                    "path = spec.rsplit(':', 1)[0]",
                    "if os.path.exists(path): os.unlink(path)",
                    "s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)",
                    "s.bind(path)",
                    "os.chmod(path, 0o666)",
                    "s.listen(8)",
                    "while True:",
                    "    c, _ = s.accept()",
                    "    c.recv(4096)",
                    "    c.sendall(b'{\"type\":\"protocol_info\",\"protocol_version\":1}\\n')",
                    "    c.close()",
                ],
            ))
        }

        fn never_binding() -> Option<Self> {
            let python = find_tool(PYTHON3)?;
            Some(Self::write(&python, &["import time", "time.sleep(120)"]))
        }

        /// Binds the socket, then accepts and never answers: the shape a wedged daemon has, and the
        /// only trigger for the probe budget.
        fn binding_silently() -> Option<Self> {
            let python = find_tool(PYTHON3)?;
            Some(Self::write(
                &python,
                &[
                    "import os, socket, sys",
                    "spec = sys.argv[sys.argv.index('-L') + 1]",
                    "path = spec.rsplit(':', 1)[0]",
                    "if os.path.exists(path): os.unlink(path)",
                    "s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)",
                    "s.bind(path)",
                    "s.listen(8)",
                    "held = []",
                    "while True:",
                    "    c, _ = s.accept()",
                    "    held.append(c)",
                ],
            ))
        }

        fn create() -> Self {
            let dir = tempfile::tempdir().expect("stub directory");
            let script = dir.path().join("ssh-stub");
            Self { dir, script }
        }

        fn path_str(&self) -> &str {
            self.script.to_str().expect("stub path is utf-8")
        }

        fn log(&self) -> String {
            format!("stub {}", self.script.display())
        }

        /// Machine files in the stub's own directory: only `conf` matters to the argv, and nothing
        /// here connects for real.
        fn files(&self) -> MachineFiles {
            let machine = MachineSsh {
                name: "fx-stub".to_string(),
                hostname: "127.0.0.1".to_string(),
                username: None,
                port: None,
                identity_file: None,
                pinned_host_key: None,
                proxy_command: None,
                certificate_file: None,
            };
            write_machine_files_in(&self.dir.path().join("ssh"), &machine).expect("machine files")
        }

        fn spec(&self, name: &str) -> ForwardSpec {
            ForwardSpec::new(
                self.dir.path().join("ssh").join(format!("{name}.sock")),
                "/home/daytona/.ferryx-cloud/rpc.sock",
            )
            .expect("forward spec")
        }
    }

    const PYTHON3: &[&str] = &["/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3"];

    /// F2: the ssh child must not outlive the process that owns the forward, even on `SIGKILL`.
    ///
    /// Driven against a *separate process* rather than this test binary: the property is about the
    /// owner dying with its `Drop` never running, so the owner has to be killable without taking
    /// the test harness with it. The child process opens a forward to the fixture sshd, reports its
    /// pids, then is `SIGKILL`ed; the ssh it spawned must be gone afterwards.
    #[tokio::test]
    async fn the_ssh_child_dies_with_a_sigkilled_owner() {
        let Some(fixture) = Fixture::start() else {
            eprintln!("SKIPPED: no sshd");
            return;
        };
        let echo = Echo::start();
        let files = fixture.files("fx-t6-sigkill");
        let spec = fixture.spec("fx-t6-sigkill", &echo.path);

        // The owner half is this same test binary, in a mode that only starts a forward and waits.
        let mut owner = StdCommand::new(std::env::current_exe().expect("test binary"))
            .args([
                "ssh::socket_forward::tests::sigkill_owner_child",
                "--exact",
                "--nocapture",
            ])
            .env(ENV_OWNER_FILES, files.conf.to_string_lossy().into_owned())
            .env(ENV_OWNER_LOCAL, spec.local_socket.to_string_lossy().into_owned())
            .env(ENV_OWNER_CONF, files.conf.to_string_lossy().into_owned())
            .env(ENV_OWNER_KNOWN, files.known_hosts.to_string_lossy().into_owned())
            .env(ENV_OWNER_ALIAS, files.alias.clone())
            .env(ENV_OWNER_REMOTE, spec.remote_socket.clone())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("owner process");
        let owner_pid = owner.id();
        // The owner prints the ssh pid it owns once the forward is ready.
        let ssh_pid = read_reported_ssh_pid(&mut owner).await;
        let Some(ssh_pid) = ssh_pid else {
            let _ = owner.kill();
            let output = owner.wait_with_output().expect("owner output");
            panic!(
                "the owner forward never became ready: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        assert!(alive(ssh_pid), "the ssh child is running before the kill");
        assert!(
            spec.local_socket.exists(),
            "the owner published the socket before the kill"
        );

        // The whole point: no `Drop`, no unwinding, no chance to run teardown.
        unsafe { libc::kill(owner_pid as libc::pid_t, libc::SIGKILL) };
        let _ = owner.wait();
        assert!(
            await_dead(ssh_pid).await,
            "a SIGKILLed owner must not leave ssh serving its socket (pid {ssh_pid})"
        );
    }

    const ENV_OWNER_FILES: &str = "FERRYX_T6_OWNER_CONF";
    const ENV_OWNER_LOCAL: &str = "FERRYX_T6_OWNER_LOCAL";
    const ENV_OWNER_CONF: &str = "FERRYX_T6_OWNER_CONF_PATH";
    const ENV_OWNER_KNOWN: &str = "FERRYX_T6_OWNER_KNOWN";
    const ENV_OWNER_ALIAS: &str = "FERRYX_T6_OWNER_ALIAS";
    const ENV_OWNER_REMOTE: &str = "FERRYX_T6_OWNER_REMOTE";
    /// Marker that this process was started as the `SIGKILL` test's owner half.
    const OWNER_READY: &str = "FERRYX_T6_OWNER_READY";

    /// Reads the owner's report line, bounded: a start that never becomes ready is a failure of the
    /// test's own premise, not something to hang on.
    async fn read_reported_ssh_pid(owner: &mut std::process::Child) -> Option<u32> {
        use std::io::BufRead;
        let stdout = owner.stdout.take()?;
        let reader = std::io::BufReader::new(stdout);
        let read = tokio::task::spawn_blocking(move || {
            let mut lines = reader.lines();
            while let Some(Ok(line)) = lines.next() {
                if let Some(rest) = line.strip_prefix(OWNER_READY) {
                    return rest.trim().parse::<u32>().ok();
                }
            }
            None
        });
        match tokio::time::timeout(Duration::from_secs(60), read).await {
            Ok(Ok(pid)) => pid,
            _ => None,
        }
    }

    /// The owner half of [`the_ssh_child_dies_with_a_sigkilled_owner`].
    ///
    /// Only runs when the marker is present, so a plain `cargo test` skips it instead of hanging.
    #[tokio::test]
    async fn sigkill_owner_child() {
        let (Ok(conf), Ok(local), Ok(known), Ok(alias), Ok(remote)) = (
            std::env::var(ENV_OWNER_CONF),
            std::env::var(ENV_OWNER_LOCAL),
            std::env::var(ENV_OWNER_KNOWN),
            std::env::var(ENV_OWNER_ALIAS),
            std::env::var(ENV_OWNER_REMOTE),
        ) else {
            eprintln!("SKIPPED: not the SIGKILL owner process");
            return;
        };
        let files = MachineFiles {
            alias,
            conf: PathBuf::from(conf),
            known_hosts: PathBuf::from(known),
        };
        let spec = ForwardSpec::new(PathBuf::from(local), remote).expect("forward spec");
        let forward = start_with_files(&files, &spec)
            .await
            .expect("owner forward must start");
        println!("{OWNER_READY} {}", forward.pid().expect("forward pid"));
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        // Wait to be killed: the parent's SIGKILL is the whole experiment.
        std::future::pending::<()>().await;
    }
}
