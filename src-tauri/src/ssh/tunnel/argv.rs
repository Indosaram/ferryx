//! Isolated forwarding argv and fail-closed configuration equivalence.
//!
//! The zero-byte config is a trust root inside the daemon's owner-only 0700 runtime
//! directory (`daemon::server::{get_runtime_dir, ensure_runtime_directory}`). Checking the file cannot
//! eliminate ssh's later open-by-name TOCTOU; the private directory mitigates it.
//! The caller creates/replaces the file and revalidates it immediately before spawn.
//!
//! A jump child inherits -F, but none of our destination -o options. Consequently
//! its default StrictHostKeyChecking=ask fails closed without a tty for unknown
//! bastions, and its TCP timeout is the OS timeout (about 75s on macOS), not 10s.
//! It may append bastion key types unhashed: losing HashKnownHosts while retaining
//! default UpdateHostKeys is an accepted V1 privacy residual. UseKeychain is inert
//! on a hop with no explicit identity; a hop requiring its own identity is refused.
//! Match blocks (including Fedora/RHEL's Match final all) are refused before -G.

use super::{validate_spec, TunnelSpec, UNSUPPORTED_CONFIG_PIECES};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::{SshAuthMethod, SshHost, SshHostSource};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

type Settings = BTreeMap<String, Vec<String>>;

/// Fixture system text is scanned too, but never supplied to ssh -F: fixture -G
/// proves user-config equivalence only. Real -G has no -F and sees system files.
#[derive(Debug, Clone, Copy)]
pub enum ConfigSource<'a> {
    Real,
    Fixture { user: &'a str, system: &'a str },
}

const REPRODUCED: &[&str] = &[
    "hostname", "user", "port", "identityfile", "proxyjump", "identityagent",
    "identitiesonly", "usekeychain",
];
// Authentication negotiation drops can remove/reorder methods, not weaken host
// verification; key hosts remain BatchMode=yes. Agent enrollment, compression,
// environment and liveness are bookkeeping. HashKnownHosts/UpdateHostKeys cannot
// write destination keys: we force StrictHostKeyChecking=yes + UpdateHostKeys=no.
// X11/agent forwarding has no requested channel in this -N -T connection.
const INERT: &[&str] = &[
    "addkeystoagent", "serveraliveinterval", "serveralivecountmax", "compression",
    "sendenv", "hashknownhosts", "gssapiauthentication", "gssapikeyexchange",
    "pubkeyacceptedkeytypes", "pubkeyacceptedalgorithms", "preferredauthentications",
    "updatehostkeys", "controlpersist", "xauthlocation", "forwardx11",
    "forwardx11trusted", "forwardagent", "ignoreunknown",
];
const FORWARDING: &[&str] = &[
    "localforward", "remoteforward", "dynamicforward", "clearallforwardings",
];
// The builder AND oracle derive their forced keys from this actual option list.
const FORCED_OPTIONS: &[&str] = &[
    "ExitOnForwardFailure=yes", "ControlMaster=no", "ControlPath=none",
    "StrictHostKeyChecking=yes", "UpdateHostKeys=no", "PermitLocalCommand=no",
    "RemoteCommand=none", "ServerAliveInterval=15", "ServerAliveCountMax=2",
    "ConnectionAttempts=1",
];
const PASSWORD_OPTIONS: &[&str] = &[
    "PreferredAuthentications=password", "PasswordAuthentication=yes",
    "PubkeyAuthentication=no", "KbdInteractiveAuthentication=no", "NumberOfPasswordPrompts=1",
];

fn unsupported(piece: &str) -> IpcError {
    debug_assert!(UNSUPPORTED_CONFIG_PIECES.contains(&piece));
    IpcError::new(IpcErrorCode::InvalidArgument, "SSH configuration cannot be reproduced safely")
        .with_details(serde_json::json!({"reason": "unsupported_config", "piece": piece}))
}

fn option(args: &mut Vec<String>, value: impl Into<String>) {
    args.extend(["-o".into(), value.into()]);
}

fn forced_options(host: &SshHost) -> Vec<String> {
    let mut options: Vec<String> = FORCED_OPTIONS.iter().map(|s| (*s).into()).collect();
    options.push(format!("BatchMode={}", if host.auth_method == SshAuthMethod::Password { "no" } else { "yes" }));
    options.push(format!("ConnectTimeout={}", if host.jump_host.is_some() { 10 } else { 3 }));
    if host.auth_method == SshAuthMethod::Password {
        options.extend(PASSWORD_OPTIONS.iter().map(|s| (*s).to_string()));
    }
    options
}

fn forwarding_directive(key: &str) -> bool { FORWARDING.contains(&key) }

fn forced_values(host: &SshHost) -> BTreeMap<String, String> {
    let mut result: BTreeMap<_, _> = forced_options(host).into_iter().map(|s| {
        let (key, value) = s.split_once('=').expect("constant option");
        (key.to_ascii_lowercase(), value.to_string())
    }).collect();
    // Forced by flags, not -o options.
    result.insert("sessiontype".into(), "none".into());
    result.insert("requesttty".into(), "no".into());
    result
}

/// Return options only, destination appended separately. The existing private
/// append_options is reused through interactive_argv, without changing any shared
/// builder. Its program, -tt and destination are not part of this options slice.
fn host_options(host: &SshHost) -> Result<Vec<String>, IpcError> {
    let args = crate::ssh::exec::interactive_argv(host)?;
    Ok(args[2..args.len() - 1].to_vec())
}

fn safe_token(value: &str, extra: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

#[derive(Debug)]
struct Hop { name: String, user: Option<String>, port: Option<u16> }

fn parse_hop(raw: &str) -> Result<Hop, IpcError> {
    let (user, address) = match raw.split_once('@') {
        Some((u, h)) if safe_token(u, "._-") => (Some(u.to_string()), h),
        Some(_) => return Err(unsupported("jump_config")),
        None => (None, raw),
    };
    let (name, port) = if let Some(rest) = address.strip_prefix('[') {
        let (ip, suffix) = rest.split_once(']').ok_or_else(|| unsupported("jump_config"))?;
        ip.parse::<std::net::Ipv6Addr>().map_err(|_| unsupported("jump_config"))?;
        let port = if suffix.is_empty() { None } else {
            Some(suffix.strip_prefix(':').ok_or_else(|| unsupported("jump_config"))?
                .parse::<u16>().map_err(|_| unsupported("jump_config"))?)
        };
        (ip.to_string(), port)
    } else {
        let (name, port) = match address.split_once(':') {
            Some((h, p)) => (h, Some(p.parse::<u16>().map_err(|_| unsupported("jump_config"))?)),
            None => (address, None),
        };
        if !safe_token(name, "._-") { return Err(unsupported("jump_config")); }
        (name.to_string(), port)
    };
    if port == Some(0) { return Err(unsupported("jump_config")); }
    Ok(Hop { name, user, port })
}

fn validate_inputs(host: &SshHost) -> Result<SshHost, IpcError> {
    let mut host = host.clone();
    // Importers may enclose IPv6 in exactly one bracket pair; normalize storage.
    if let Some(ip) = host.hostname.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        ip.parse::<std::net::Ipv6Addr>().map_err(|_| unsupported("config_only_endpoint"))?;
        host.hostname = ip.to_string();
    }
    if !(safe_token(&host.hostname, "._-") || host.hostname.parse::<std::net::Ipv6Addr>().is_ok())
        || host.username.as_deref().is_some_and(|u| !safe_token(u, "._-"))
        || host.port == Some(0)
    { return Err(unsupported("config_only_endpoint")); }
    if let Some(jump) = &host.jump_host {
        parse_hop(jump)?;
        if host.auth_method == SshAuthMethod::Password { return Err(unsupported("jump_password")); }
    }
    if let Some(identity) = &host.identity_file {
        normalize_identity(identity)?;
    }
    Ok(host)
}

/// Validate, but do not repair, a caller-owned zero-byte trust root. The registry
/// recreates suspect inodes atomically; argv never mutates the filesystem.
pub fn validate_empty_config(path: &Path) -> Result<(), IpcError> {
    let parent = path.parent().ok_or_else(|| unsupported("config_path"))?;
    let dir = std::fs::symlink_metadata(parent).map_err(|_| unsupported("config_path"))?;
    if !dir.is_dir() { return Err(unsupported("config_path")); }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        if dir.uid() != unsafe { libc::geteuid() } || dir.mode() & 0o777 != 0o700 {
            return Err(unsupported("config_path"));
        }
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(path).map_err(|_| unsupported("config_path"))?.file_type().is_symlink() {
        return Err(unsupported("config_path"));
    }
    let file = options.open(path).map_err(|_| unsupported("config_path"))?;
    let meta = file.metadata().map_err(|_| unsupported("config_path"))?;
    if !meta.is_file() || meta.len() != 0 { return Err(unsupported("config_path")); }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o777 != 0o600 {
            return Err(unsupported("config_path"));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Directive { key: String, values: Vec<String> }

// Shell-like ssh_config words, with quotes, comments, escapes and Key=Value.
fn words(line: &str) -> Result<Vec<String>, IpcError> {
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut separator = false;
    for c in line.trim_start().chars() {
        if escaped { word.push(c); escaped = false; continue; }
        if c == '\\' { escaped = true; continue; }
        if let Some(q) = quote {
            if c == q { quote = None; } else { word.push(c); }
        } else if c == '\'' || c == '"' { quote = Some(c); }
        else if c == '#' { break; }
        else if c.is_whitespace() || (c == '=' && !separator && (result.is_empty() || (result.len() == 1 && word.is_empty()))) {
            if c == '=' { separator = true; }
            if !word.is_empty() { result.push(std::mem::take(&mut word)); }
        } else { word.push(c); }
    }
    if quote.is_some() || escaped { return Err(unsupported("bad_option")); }
    if !word.is_empty() { result.push(word); }
    Ok(result)
}

fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let (p, n) = (pattern.as_bytes(), name.as_bytes());
    let mut row = vec![false; n.len() + 1]; row[0] = true;
    for c in p {
        let mut next = vec![false; n.len() + 1];
        if *c == b'*' { next[0] = row[0]; }
        for j in 1..=n.len() {
            next[j] = if *c == b'*' { row[j] || next[j - 1] }
                else { row[j - 1] && (*c == b'?' || *c == n[j - 1]) };
        }
        row = next;
    }
    row[n.len()]
}

fn expand_home(value: &str) -> String {
    let home = dirs::home_dir().unwrap_or_default().to_string_lossy().into_owned();
    value.strip_prefix("~/").map(|v| format!("{home}/{v}"))
        .unwrap_or_else(|| value.replace("%d", &home))
}

fn include_paths(pattern: &str, base: &Path) -> Result<Vec<PathBuf>, IpcError> {
    let expanded = expand_home(pattern);
    let path = if Path::new(&expanded).is_absolute() { PathBuf::from(expanded) } else { base.join(expanded) };
    let mut paths = vec![PathBuf::new()];
    for component in path.components() {
        let part = component.as_os_str().to_string_lossy();
        let mut next = Vec::new();
        for path in paths {
            if part.contains('*') || part.contains('?') || part.contains('[') {
                // Bracket glob classes aren't implemented: refusing is safer than
                // executing -G over an include graph we did not inspect.
                if part.contains('[') { return Err(unsupported("unsupported_directive")); }
                match std::fs::read_dir(&path) {
                    Ok(entries) => for entry in entries {
                        let entry = entry.map_err(|_| unsupported("bad_option"))?;
                        if wildcard(&part, &entry.file_name().to_string_lossy()) { next.push(entry.path()); }
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                    Err(_) => return Err(unsupported("bad_option")),
                }
            } else { next.push(path.join(component.as_os_str())); }
        }
        paths = next;
    }
    paths.sort();
    Ok(paths)
}

fn scan_text(text: &str, base: &Path, depth: usize, seen: &mut BTreeSet<PathBuf>, out: &mut Vec<Directive>) -> Result<(), IpcError> {
    if depth > 32 { return Err(unsupported("bad_option")); }
    for line in text.lines() {
        let mut fields = words(line)?;
        if fields.is_empty() { continue; }
        let key = fields.remove(0).to_ascii_lowercase();
        // refuse_any_match_block: -G executes Match exec, even when only resolving.
        if key == "match" { return Err(unsupported("match_block")); }
        if key == "include" {
            for pattern in fields {
                for path in include_paths(&pattern, base)? {
                    if !seen.insert(path.clone()) { return Err(unsupported("bad_option")); }
                    match std::fs::read_to_string(&path) {
                        Ok(text) => scan_text(&text, base, depth + 1, seen, out)?,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                        Err(_) => return Err(unsupported("bad_option")),
                    }
                    seen.remove(&path);
                }
            }
        } else { out.push(Directive { key, values: fields }); }
    }
    Ok(())
}

struct Config {
    directives: Vec<Directive>,
    // -F fixture only; production deliberately has NO left-side -F.
    fixture: Option<tempfile::NamedTempFile>,
}

impl Config {
    fn load(source: &ConfigSource<'_>) -> Result<Self, IpcError> {
        let mut directives = Vec::new();
        let user_base = dirs::home_dir().unwrap_or_default().join(".ssh");
        let mut seen = BTreeSet::new();
        let fixture = match source {
            ConfigSource::Fixture { user, system } => {
                // Fixture includes must use absolute paths, never operator files.
                let dir = tempfile::tempdir().map_err(|_| unsupported("bad_option"))?;
                scan_text(user, dir.path(), 0, &mut seen, &mut directives)?;
                let fixture_text = directives.iter().map(|d| {
                    let values = d.values.iter().map(|v| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))).collect::<Vec<_>>().join(" ");
                    format!("{} {values}\n", d.key)
                }).collect::<String>();
                directives.push(Directive { key: "host".into(), values: vec!["*".into()] });
                scan_text(system, dir.path(), 0, &mut seen, &mut directives)?;
                let file = tempfile::NamedTempFile::new().map_err(|_| unsupported("bad_option"))?;
                std::fs::write(file.path(), fixture_text).map_err(|_| unsupported("bad_option"))?;
                Some(file)
            }
            ConfigSource::Real => {
                for (path, base) in [(user_base.join("config"), user_base),
                    (PathBuf::from("/etc/ssh/ssh_config"), PathBuf::from("/etc/ssh"))] {
                    match std::fs::read_to_string(path) {
                        Ok(text) => {
                            directives.push(Directive { key: "host".into(), values: vec!["*".into()] });
                            scan_text(&text, &base, 0, &mut seen, &mut directives)?;
                        },
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                        Err(_) => return Err(unsupported("bad_option")),
                    }
                }
                None
            }
        };
        Ok(Self { directives, fixture })
    }

    fn left(&self) -> Vec<String> {
        let mut args = vec!["-o".into(), "IgnoreUnknown=UseKeychain".into()];
        if let Some(file) = &self.fixture {
            args.extend(["-F".into(), file.path().to_string_lossy().into_owned()]);
        }
        args
    }

    fn scan(&self, names: &[&str], hop: bool) -> Result<BTreeMap<String, String>, IpcError> {
        let mut active = true;
        let mut supplied = BTreeMap::new();
        for d in &self.directives {
            if d.key == "host" {
                let patterns: Vec<_> = d.values.iter().flat_map(|v| v.split(',')).collect();
                active = names.iter().any(|name| {
                    !patterns.iter().any(|p| p.strip_prefix('!').is_some_and(|p| wildcard(p, name)))
                        && patterns.iter().any(|p| !p.starts_with('!') && wildcard(p, name))
                });
                continue;
            }
            if !active { continue; }
            let key = d.key.as_str();
            if key == "identityfile" && !hop {
                for value in &d.values { normalize_identity(value)?; }
            }
            if INERT.contains(&key) || forwarding_directive(key) || (hop && key == "usekeychain") { continue; }
            if hop {
                if ["identityfile", "certificatefile", "identityagent", "identitiesonly"].contains(&key) {
                    return Err(unsupported("jump_identity"));
                }
                if !["hostname", "user", "port"].contains(&key) { return Err(unsupported("jump_config")); }
            } else if !REPRODUCED.contains(&key) && !FORCED_OPTIONS.iter().any(|o| o.split_once('=').unwrap().0.eq_ignore_ascii_case(key))
                && !["batchmode", "connecttimeout"].contains(&key) {
                tracing::debug!(directive = key, "Tunnel config directive cannot be reproduced");
                return Err(unsupported(if key == "certificatefile" { "certificate_file" } else { "unsupported_directive" }));
            }
            supplied.entry(d.key.clone()).or_insert_with(|| d.values.join(" "));
        }
        Ok(supplied)
    }
}

// -G values may contain spaces. Never split the rest of the line into words.
fn parse_g(text: &str) -> Settings {
    let mut settings = Settings::new();
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(index) = line.find(char::is_whitespace) {
            let (key, rest) = line.split_at(index);
            settings.entry(key.to_ascii_lowercase()).or_default().push(rest.trim_start().to_string());
        }
    }
    settings
}

#[cfg(test)]
thread_local! { static G_SPAWNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn read_g(args: &[String]) -> Result<Settings, IpcError> {
    #[cfg(test)]
    G_SPAWNS.with(|count| count.set(count.get() + 1));
    let output = Command::new(crate::ssh::exec::resolve_ssh_program()?).arg("-G").args(args)
        .output().map_err(|_| unsupported("bad_option"))?;
    if !output.status.success() || output.stdout.is_empty() { return Err(unsupported("bad_option")); }
    let settings = parse_g(&String::from_utf8_lossy(&output.stdout));
    if settings.is_empty() { return Err(unsupported("bad_option")); }
    Ok(settings)
}

fn value<'a>(settings: &'a Settings, key: &str) -> Option<&'a str> {
    settings.get(key).and_then(|v| v.first()).map(String::as_str)
}

fn normalize_identity(raw: &str) -> Result<String, IpcError> {
    let mut path = expand_home(raw);
    while let Some(start) = path.find("${") {
        let end = path[start + 2..].find('}').map(|i| start + 2 + i).ok_or_else(|| unsupported("identity_file_token"))?;
        let name = &path[start + 2..end];
        let expanded = std::env::var(name).map_err(|_| unsupported("identity_file_token"))?;
        if expanded.contains("${") { return Err(unsupported("identity_file_token")); }
        path.replace_range(start..=end, &expanded);
    }
    if path.contains('%') { return Err(unsupported("identity_file_token")); }
    let path = PathBuf::from(path);
    Ok(if path.is_absolute() { path } else {
        std::env::current_dir().map_err(|_| unsupported("identity_file_token"))?.join(path)
    }.to_string_lossy().into_owned())
}

fn identities(settings: &Settings) -> Result<Vec<String>, IpcError> {
    let mut result = Vec::new();
    for raw in settings.get("identityfile").into_iter().flatten() {
        let path = normalize_identity(raw)?;
        if !result.contains(&path) { result.push(path); }
    }
    Ok(result)
}

fn compare(left: &Settings, right: &Settings, host: &SshHost, hop: bool) -> Result<(), IpcError> {
    let forced = forced_values(host);
    for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
        let k = key.as_str();
        // `host` is -G's original command-line lookup name, not HostName.
        // An alias on the left and its resolved address on the right necessarily
        // differ here; `hostname` below still verifies the actual endpoint.
        if k == "host" { continue; }
        let a = value(left, k); let b = value(right, k);
        let equal = if k == "identityfile" { identities(left)? == identities(right)? }
            else if k == "hostname" { a.map(str::to_ascii_lowercase) == b.map(str::to_ascii_lowercase) }
            else if k == "proxyjump" { a.filter(|s| *s != "none").is_some() == b.filter(|s| *s != "none").is_some() }
            else { left.get(key) == right.get(key) };
        if hop && ["identityfile", "certificatefile", "identityagent", "identitiesonly"].contains(&k) && !equal {
            return Err(unsupported("jump_identity"));
        }
        if equal { continue; }
        if INERT.contains(&k) || forwarding_directive(k) || (hop && k == "usekeychain") { continue; }
        if hop {
            if ["hostname", "user", "port"].contains(&k) { continue; }
            return Err(unsupported(if ["identityfile", "certificatefile", "identityagent", "identitiesonly"].contains(&k) {
                "jump_identity"
            } else { "jump_config" }));
        }
        if forced.get(k).is_some_and(|v| {
            // OpenSSH renders ControlMaster=no as false, and omits some none
            // values entirely. Compare their canonical effective spelling.
            let actual = match b { Some("false") => Some("no"), Some("true") => Some("yes"), other => other };
            actual == Some(v.as_str()) || (v == "none" && actual.is_none())
        }) { continue; }
        if ["identityagent", "identitiesonly", "usekeychain"].contains(&k) {
            // Reproduction still requires exact equality; IgnoreUnknown may remove
            // UseKeychain entirely on non-Apple binaries (both sides absent).
            return Err(unsupported(if k == "identityagent" { "identity_agent" } else { "unsupported_directive" }));
        }
        if REPRODUCED.contains(&k) {
            return Err(unsupported(if k == "proxyjump" { "config_only_jump" } else { "config_only_endpoint" }));
        }
        tracing::debug!(directive = k, "Tunnel -G equivalence refused differing key");
        return Err(unsupported("unsupported_directive"));
    }
    Ok(())
}

fn quote_agent(value: &str) -> Result<String, IpcError> {
    if value == "SSH_AUTH_SOCK" || value.contains('%') || value.contains("${") || value.chars().any(char::is_control) {
        return Err(unsupported("identity_agent"));
    }
    Ok(format!("IdentityAgent=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

fn aliases(config: &Config, host: &SshHost) -> Result<Vec<String>, IpcError> {
    if host.source != SshHostSource::Config { return Ok(vec![host.hostname.clone()]); }
    let mut candidates = BTreeSet::new();
    for d in &config.directives {
        if d.key != "host" { continue; }
        for alias in d.values.iter().flat_map(|v| v.split(',')) {
            if !safe_token(alias, "._-") { continue; }
            let mut raw = SshHost { hostname: alias.into(), port: None, username: None, identity_file: None, jump_host: None, ..host.clone() };
            raw.source = SshHostSource::Manual;
            let mut args = config.left(); args.push(raw.hostname.clone());
            let effective = read_g(&args)?;
            let same_identity = match &host.identity_file {
                Some(path) => identities(&effective)?.contains(&normalize_identity(path)?),
                None => true, // The full alias scan and -G comparison below still run.
            };
            if value(&effective, "hostname").is_some_and(|v| v.eq_ignore_ascii_case(&host.hostname))
                && value(&effective, "port").and_then(|v| v.parse::<u16>().ok()).unwrap_or(22) == host.port.unwrap_or(22)
                && host.username.as_deref().map_or(true, |u| value(&effective, "user") == Some(u))
                && same_identity { candidates.insert(alias.to_string()); }
        }
    }
    if candidates.is_empty() { return Err(unsupported("config_alias")); }
    Ok(candidates.into_iter().collect())
}

fn resolve_hop(config: &Config, host: &SshHost, empty: &Path) -> Result<Option<String>, IpcError> {
    let Some(raw) = &host.jump_host else { return Ok(None); };
    let hop = parse_hop(raw)?;
    config.scan(&[&hop.name], true)?;
    let mut args = config.left();
    if let Some(user) = &hop.user { args.extend(["-l".into(), user.clone()]); }
    if let Some(port) = hop.port { args.extend(["-p".into(), port.to_string()]); }
    args.push(hop.name.clone()); // raw alias, NEVER resolved HostName, on the left.
    let left = read_g(&args)?;
    let name = value(&left, "hostname").ok_or_else(|| unsupported("jump_config"))?;
    let user = value(&left, "user").ok_or_else(|| unsupported("jump_config"))?;
    let port = value(&left, "port").and_then(|v| v.parse::<u16>().ok()).filter(|v| *v != 0).ok_or_else(|| unsupported("jump_config"))?;
    let address = if name.parse::<std::net::Ipv6Addr>().is_ok() { format!("[{name}]") } else { name.to_string() };
    let resolved = format!("{user}@{address}:{port}");
    parse_hop(&resolved)?; // Validate config-derived data BEFORE the right-side -G.
    config.scan(&[&hop.name, name], true)?;
    let right = read_g(&["-o".into(), "IgnoreUnknown=UseKeychain".into(), "-F".into(), empty.to_string_lossy().into_owned(),
        "-l".into(), user.into(), "-p".into(), port.to_string(), name.into()])?;
    compare(&left, &right, host, true)?;
    Ok(Some(resolved))
}

/// Build isolated options; never spawn a tunnel or install credentials. Config
/// inspection runs ssh -G only after all untrusted option tokens are validated.
/// Real left-side -G includes system config; fixture -F omits it, so fixture system
/// text is checked by the identical scan but not by the user-config -G oracle.
pub fn tunnel_argv(host: &SshHost, spec: &TunnelSpec, local_port: u16, empty_config: &Path, config_source: &ConfigSource<'_>) -> Result<Vec<String>, IpcError> {
    if local_port == 0 { return Err(IpcError::new(IpcErrorCode::InvalidArgument, "Tunnel local port must be nonzero")); }
    validate_spec(spec)?;
    let host = validate_inputs(host)?;
    validate_empty_config(empty_config)?;
    let config = Config::load(config_source)?;
    let aliases = aliases(&config, &host)?;
    let multiple = aliases.len() > 1;
    let build = || -> Result<Vec<String>, IpcError> {
        let mut resolved = host.clone();
        resolved.jump_host = resolve_hop(&config, &host, empty_config)?;
        let mut result = None;
        for alias in &aliases {
            let supplied = config.scan(&[alias, &host.hostname], false)?;
            let mut left_args = config.left();
            left_args.extend(host_options(&host)?);
            if let Some(user) = &host.username { left_args.extend(["-l".into(), user.clone()]); }
            left_args.push(alias.clone());
            let left = read_g(&left_args)?;
            let mut args = vec![crate::ssh::exec::resolve_ssh_program()?, "-o".into(), "IgnoreUnknown=UseKeychain".into()];
            args.extend(host_options(&resolved)?);
            for key in ["identityagent", "identitiesonly", "usekeychain"] {
                if !supplied.contains_key(key) { continue; }
                let val = value(&left, key).or_else(|| supplied.get(key).map(String::as_str)).ok_or_else(|| unsupported("bad_option"))?;
                option(&mut args, match key {
                    "identityagent" => quote_agent(val)?,
                    "identitiesonly" => format!("IdentitiesOnly={val}"),
                    _ => format!("UseKeychain={val}"),
                });
            }
            args.extend(["-N".into(), "-T".into()]);
            for value in forced_options(&host) { option(&mut args, value); }
            args.extend(["-F".into(), empty_config.to_string_lossy().into_owned()]);
            let mut right_args = args[1..].to_vec();
            right_args.push(resolved.target());
            let right = read_g(&right_args)?;
            compare(&left, &right, &host, false)?;
            args.push(resolved.target());
            let remote = if spec.remote_host.contains(':') && !spec.remote_host.starts_with('[') {
                format!("[{}]", spec.remote_host)
            } else { spec.remote_host.clone() };
            args.push(format!("-L127.0.0.1:{local_port}:{remote}:{}", spec.remote_port));
            result = Some(args);
        }
        result.ok_or_else(|| unsupported("config_alias"))
    };
    build().map_err(|e| if multiple { unsupported("config_alias") } else { e })
}

#[cfg(test)]
mod tests {
use super::*;
use crate::ssh::tunnel::TunnelSpecId;

#[cfg(unix)]
mod live {
// Unix-only live endpoint exercises. Run the entire argv filter serially.
use super::*;
use std::process::{Child, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Every fixture child leads its own group. Reap proxy ssh/nc children too,
        // rather than leaving a hop alive after the destination ssh is killed.
        let result = unsafe { libc::kill(-(self.0.id() as libc::pid_t), libc::SIGKILL) };
        if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH) {
            eprintln!("kill fixture process group: {}", std::io::Error::last_os_error());
        }
        if let Err(error) = self.0.kill() {
            if error.kind() != std::io::ErrorKind::InvalidInput { eprintln!("kill fixture child: {error}"); }
        }
        if let Err(error) = self.0.wait() { eprintln!("reap fixture child: {error}"); }
    }
}
struct Live {
    fixture: Fixture,
    host: SshHost,
    server: tokio::task::JoinHandle<()>,
    echo: tokio::task::JoinHandle<()>,
    sink: u16,
    launcher: PathBuf,
}
impl Drop for Live {
    fn drop(&mut self) { self.server.abort(); self.echo.abort(); }
}
impl Live {
    async fn new() -> Self {
        use std::os::fd::OwnedFd;
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new();
        for name in ["host_key", "user_key"] {
            let output = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(fixture.dir.path().join(name)).output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let root = fixture.dir.path();
        let config = root.join("sshd_config");
        std::fs::write(&config, format!("HostKey {}\nAuthorizedKeysFile {}\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nLogLevel VERBOSE\n", root.join("host_key").display(), root.join("user_key.pub").display())).unwrap();
        let known = root.join("known_hosts");
        std::fs::write(&known, format!("[127.0.0.1]:{port} {}", std::fs::read_to_string(root.join("host_key.pub")).unwrap())).unwrap();
        // Only the fixture's key trust is injected by this launcher. The builder's
        // -F, -L and all forwarding options reach real ssh unchanged in both arms.
        // This is not a production known_hosts override.
        let launcher = root.join("ssh-fixture");
        std::fs::write(&launcher, format!("#!/bin/sh\nexec /usr/bin/ssh -o UserKnownHostsFile={} -o GlobalKnownHostsFile=/dev/null \"$@\"\n", crate::ssh::direct::quote_posix(known.to_str().unwrap()))).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        let log = root.join("sshd.log");
        let server = tokio::spawn(async move {
            let mut children = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let stream = stream.into_std().unwrap(); stream.set_nonblocking(false).unwrap();
                        let text = std::fs::read_to_string(&config).unwrap();
                        let password_fixture = text.contains("PasswordAuthentication yes");
                        let mut command = if password_fixture && unsafe { libc::geteuid() } != 0 {
                            let mut command = tokio::process::Command::new("sudo");
                            command.args(["-n", "/usr/sbin/sshd"]); command
                        } else { tokio::process::Command::new("/usr/sbin/sshd") };
                        let mut child = command.args(["-i", "-e", "-f"]).arg(&config)
                            .stdin(Stdio::from(OwnedFd::from(stream.try_clone().unwrap())))
                            .stdout(Stdio::from(OwnedFd::from(stream)))
                            .stderr(std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap())
                            .kill_on_drop(true).spawn().expect("private loopback sshd prerequisite");
                        children.spawn(async move { child.wait().await.expect("reap sshd"); });
                    }
                    Some(result) = children.join_next(), if !children.is_empty() => { result.unwrap(); }
                }
            }
        });
        let sink = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sink_port = sink.local_addr().unwrap().port();
        let echo = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = sink.accept() => {
                        let (mut stream, _) = accepted.unwrap();
                        clients.spawn(async move {
                            let mut byte = [0u8; 1];
                            if stream.read_exact(&mut byte).await.is_ok() { stream.write_all(&byte).await.expect("echo payload"); }
                        });
                    }
                    Some(result) = clients.join_next(), if !clients.is_empty() => { result.unwrap(); }
                }
            }
        });
        let mut host = host(); host.hostname = "127.0.0.1".into(); host.port = Some(port);
        host.username = Some(std::env::var("USER").expect("fixture login user"));
        host.identity_file = Some(root.join("user_key").to_string_lossy().into_owned());
        Self { fixture, host, server, echo, sink: sink_port, launcher }
    }
    fn spawn(&self, args: &[String]) -> ChildGuard {
        let log = self.fixture.dir.path().join("client.log");
        use std::os::unix::process::CommandExt;
        let child = Command::new(&self.launcher).args(&args[1..]).process_group(0).stdin(Stdio::null())
            .stdout(Stdio::null()).stderr(std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap())
            .spawn().unwrap();
        eprintln!("live tunnel pid={}", child.id());
        ChildGuard(child)
    }
    fn argv(&self, port: u16, text: &str) -> Vec<String> {
        let mut spec = spec(); spec.remote_port = self.sink;
        tunnel_argv(&self.host, &spec, port, &self.fixture.empty, &ConfigSource::Fixture { user: text, system: "" }).unwrap()
    }
}
fn reserve() -> std::net::TcpListener { std::net::TcpListener::bind("127.0.0.1:0").unwrap() }
async fn connect(port: u16) -> tokio::net::TcpStream {
    // Connection completion itself is the observable readiness event. No fixed
    // sleeps: each refused connect yields before subscribing to the next attempt.
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await {
                Ok(stream) => break stream,
                Err(_) => tokio::task::yield_now().await,
            }
        }
    }).await.expect("bounded tunnel connectability")
}
async fn payload(port: u16) {
    let mut stream = connect(port).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        stream.write_all(b"x").await.unwrap();
        let mut byte = [0]; stream.read_exact(&mut byte).await.unwrap(); assert_eq!(&byte, b"x");
    }).await.expect("payload echo on this connection");
}
async fn contrast(isolated: bool) {
    let live = Live::new().await;
    let reservations: Vec<_> = (0..4).map(|_| reserve()).collect();
    let ports: Vec<_> = reservations.iter().map(|p| p.local_addr().unwrap().port()).collect();
    let text = format!("Host *\n LocalForward 127.0.0.1:{} 127.0.0.1:{}\n RemoteForward 127.0.0.1:{} 127.0.0.1:{}\n DynamicForward 127.0.0.1:{}\n", ports[1], live.sink, ports[2], live.sink, ports[3]);
    let mut args = live.argv(ports[0], &text);
    let probe = live.fixture.dir.path().join("probe"); std::fs::write(&probe, &text).unwrap();
    if !isolated {
        let index = args.iter().position(|s| s == "-F").unwrap();
        args[index + 1] = probe.to_string_lossy().into_owned();
    }
    drop(reservations);
    let _child = live.spawn(&args);
    payload(ports[0]).await;
    if isolated {
        for port in &ports[1..] {
            assert!(tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, *port)).await.is_err(), "inherited listener {port}");
        }
    } else {
        payload(ports[1]).await; payload(ports[2]).await; drop(connect(ports[3]).await);
    }
    eprintln!("Arm {} allocated={} localforward={} remoteforward={} dynamicforward={} inherited_listeners={}", if isolated { "A" } else { "B" }, ports[0], ports[1], ports[2], ports[3], !isolated);
}
#[tokio::test]
async fn password_non_jump_tunnel_authenticates() {
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};
    // Provision a disposable Linux account, never change the operator's password.
    // Lack of account-management privileges is a prerequisite failure, not a pass.
    // macOS's system account database is not modified by this Unix fixture.
    #[derive(Debug)]
    enum SkipReason { PasswordAccountProvisioningUnsupportedPlatform }
    if !cfg!(target_os = "linux") {
        eprintln!("EXPLICIT_TEST_SKIP {:?}", SkipReason::PasswordAccountProvisioningUnsupportedPlatform);
        return;
    }
    fn account_command(program: &str) -> Command {
        if unsafe { libc::geteuid() } == 0 { Command::new(program) }
        else {
            let mut command = Command::new("sudo"); command.args(["-n", program]); command
        }
    }
    struct Account(String);
    impl Drop for Account {
        fn drop(&mut self) {
            let output = account_command("userdel").arg(&self.0).output().expect("remove fixture account");
            assert!(output.status.success(), "userdel: {}", String::from_utf8_lossy(&output.stderr));
        }
    }
    let username = format!("fxt{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let output = account_command("useradd").args(["--no-create-home", "--shell", "/bin/sh", &username]).output().expect("useradd prerequisite");
    assert!(output.status.success(), "password fixture needs root or sudo -n useradd: {}", String::from_utf8_lossy(&output.stderr));
    let _account = Account(username.clone());
    let password = uuid::Uuid::new_v4().to_string();
    let mut chpasswd = account_command("chpasswd").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("chpasswd prerequisite");
    {
        use std::io::Write;
        chpasswd.stdin.take().unwrap().write_all(format!("{username}:{password}\n").as_bytes()).unwrap();
    }
    let output = chpasswd.wait_with_output().unwrap();
    assert!(output.status.success(), "chpasswd: {}", String::from_utf8_lossy(&output.stderr));
    let live = Live::new().await;
    let config = live.fixture.dir.path().join("sshd_config");
    let text = std::fs::read_to_string(&config).unwrap()
        .replace("PasswordAuthentication no", "PasswordAuthentication yes")
        .replace("UsePAM no", "UsePAM yes");
    std::fs::write(&config, text).unwrap();
    let mut host = live.host.clone();
    host.username = Some(username);
    host.identity_file = None;
    host.auth_method = SshAuthMethod::Password;
    assert!(host.jump_host.is_none());
    crate::ssh::password::set(&host, crate::ssh::password::Password::new(password)).unwrap();
    struct CredentialGuard(SshHost);
    impl Drop for CredentialGuard {
        fn drop(&mut self) { crate::ssh::password::clear(&self.0).expect("clear password fixture credential"); }
    }
    let _credential = CredentialGuard(host.clone());
    let mut spec = spec(); spec.remote_port = live.sink;
    let reservation = reserve(); let port = reservation.local_addr().unwrap().port(); drop(reservation);
    let args = tunnel_argv(&host, &spec, port, &live.fixture.empty, &ConfigSource::Fixture { user: "", system: "" }).unwrap();
    let environment = crate::ssh::password::environment(&args).unwrap();
    // The libtest executable does not dispatch run_askpass. This secret-free
    // adapter exercises the real broker's capability protocol instead; only the
    // random token and broker port are inherited, never the password.
    let helper = live.fixture.dir.path().join("askpass");
    std::fs::write(&helper, "#!/usr/bin/env python3\nimport os, socket, sys\nwith socket.create_connection(('127.0.0.1', int(os.environ['FERRYX_SSH_ASKPASS_PORT'])), timeout=5) as s:\n s.sendall(os.environ['FERRYX_SSH_ASKPASS_TOKEN'].encode())\n while True:\n  data = s.recv(4096)\n  if not data: break\n  sys.stdout.buffer.write(data)\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new(&live.launcher);
    command.args(&args[1..]).envs(environment).env("SSH_ASKPASS", helper).process_group(0)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let child = command.spawn().unwrap(); eprintln!("password tunnel pid={}", child.id());
    let _child = ChildGuard(child);
    payload(port).await;
    let log = std::fs::read_to_string(live.fixture.dir.path().join("sshd.log")).unwrap();
    assert!(log.contains(&format!("Accepted password for {}", host.username.as_deref().unwrap())), "real password authentication required: {log}");
}
#[tokio::test]
async fn arm_a_isolates_inherited_forwardings() { contrast(true).await; }
#[tokio::test]
async fn arm_b_probe_forwardings_are_live() { contrast(false).await; }
#[tokio::test]
async fn jump_alias_connects() {
    let live = Live::new().await;
    let reservation = reserve(); let port = reservation.local_addr().unwrap().port(); drop(reservation);
    // This fixture intentionally supplies hop identity/trust in A. It measures
    // OpenSSH -F inheritance, NOT the product's zero-byte trust root. Product
    // hop identities are refused by jump_identity_is_refused.
    let config = live.fixture.dir.path().join("hop-config");
    let user = live.host.username.as_deref().unwrap(); let ssh_port = live.host.port.unwrap();
    let marker = live.fixture.dir.path().join("proxy-executed");
    let base = format!("Host bastion\n HostName 127.0.0.1\n User {user}\n Port {ssh_port}\n IdentityFile {}\n StrictHostKeyChecking no\n UserKnownHostsFile /dev/null\n", live.host.identity_file.as_deref().unwrap());
    std::fs::write(&config, &base).unwrap();
    let mut args = live.argv(port, "");
    let f = args.iter().position(|s| s == "-F").unwrap(); args[f + 1] = config.to_string_lossy().into_owned();
    args.splice(3..3, ["-J".into(), "bastion".into()]);
    { let _child = live.spawn(&args); payload(port).await; }
    let log = std::fs::read_to_string(live.fixture.dir.path().join("sshd.log")).unwrap();
    assert!(log.contains(&format!("Accepted publickey for {user}")), "bastion auth log: {log}");
    // OpenSSH prepends `exec` to ProxyCommand. A bare `echo ...; nc ...`
    // therefore execs echo and never reaches nc. Make the whole probe one program.
    use std::os::unix::fs::PermissionsExt;
    let proxy = live.fixture.dir.path().join("probe-proxy");
    std::fs::write(&proxy, format!("#!/bin/sh\nprintf 'inherited\\n' > {}\nexec /usr/bin/nc 127.0.0.1 {ssh_port}\n", crate::ssh::direct::quote_posix(marker.to_str().unwrap()))).unwrap();
    std::fs::set_permissions(&proxy, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&config, format!("{base} ProxyCommand {}\n", proxy.display())).unwrap();
    let second_reservation = reserve();
    let second_port = second_reservation.local_addr().unwrap().port();
    *args.last_mut().unwrap() = format!("-L127.0.0.1:{second_port}:127.0.0.1:{}", live.sink);
    drop(second_reservation);
    { let _child = live.spawn(&args); payload(second_port).await; }
    assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), "inherited");
    eprintln!("hop -F inheritance: authenticated user={user} port={ssh_port}; ProxyCommand marker=inherited");
}

}

struct Fixture {
    dir: tempfile::TempDir,
    empty: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty");
        std::fs::write(&empty, "").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        Self { dir, empty }
    }
    fn argv(&self, host: &SshHost, user: &str, system: &str) -> Result<Vec<String>, IpcError> {
        tunnel_argv(host, &spec(), 23456, &self.empty, &ConfigSource::Fixture { user, system })
    }
}
fn host() -> SshHost {
    SshHost {
        id: "argv-host".into(), label: "test".into(), hostname: "genericbox.local".into(),
        username: Some("ops".into()), port: Some(2222), identity_file: None,
        jump_host: None, source: SshHostSource::Manual, auth_method: SshAuthMethod::Agent,
        disabled: None, shell: None,
    }
}
fn spec() -> TunnelSpec {
    TunnelSpec { id: TunnelSpecId("argv-spec".into()), label: "test".into(),
        local_port: None, remote_host: "127.0.0.1".into(), remote_port: 45678, enabled: true }
}
fn piece(error: IpcError) -> String {
    assert_eq!(error.code, IpcErrorCode::InvalidArgument);
    let details = error.details.expect("typed details");
    assert_eq!(details["reason"], "unsupported_config");
    let piece = details["piece"].as_str().unwrap().to_string();
    assert!(UNSUPPORTED_CONFIG_PIECES.contains(&piece.as_str()));
    piece
}
fn has(args: &[String], value: &str) -> bool { args.iter().any(|s| s == value) }

#[test]
fn destination_precedes_joined_forward() {
    let f = Fixture::new();
    let mut h = host(); h.jump_host = Some("bastion".into());
    let args = f.argv(&h, "Host bastion\n HostName 127.0.0.1\n User relay\n Port 2200\n", "").unwrap();
    assert_eq!(args[args.len() - 2], "ops@genericbox.local");
    assert_eq!(args.last().unwrap(), "-L127.0.0.1:23456:127.0.0.1:45678");
    let j = args.iter().position(|s| s == "-J").unwrap();
    assert_eq!(args[j + 1], "relay@127.0.0.1:2200");
    assert!(!args[j + 1].contains("bastion"));
    let left = parse_g("host bastion\nhostname 127.0.0.1\nuser relay\nport 2200\n");
    let right = parse_g("host 127.0.0.1\nhostname 127.0.0.1\nuser relay\nport 2200\n");
    assert!(compare(&left, &right, &h, true).is_ok(), "lookup alias is not a connection setting");
    let changed = parse_g("host 127.0.0.1\nhostname 127.0.0.1\nuser relay\nport 2200\nidentityagent /tmp/different-agent\n");
    assert_eq!(piece(compare(&left, &changed, &h, true).unwrap_err()), "jump_identity");
}
#[test]
fn empty_config_is_the_only_forwarding_isolation() {
    let f = Fixture::new(); let args = f.argv(&host(), "", "").unwrap();
    assert!(!args.iter().any(|s| s.to_ascii_lowercase().contains("clearallforwardings")));
    let flags: Vec<_> = args.iter().enumerate().filter(|(_, s)| *s == "-F").collect();
    assert_eq!(flags.len(), 1);
    assert_eq!(Path::new(&args[flags[0].0 + 1]), f.empty);
}
#[test]
fn tunnel_lifetime_options_are_explicit() {
    let f = Fixture::new(); let args = f.argv(&host(), "", "").unwrap();
    for expected in ["-N", "-T", "ExitOnForwardFailure=yes", "ControlMaster=no", "ControlPath=none",
        "StrictHostKeyChecking=yes", "UpdateHostKeys=no", "PermitLocalCommand=no", "RemoteCommand=none",
        "ServerAliveInterval=15", "ServerAliveCountMax=2", "BatchMode=yes"] { assert!(has(&args, expected), "{expected}"); }
    let mut h = host(); h.auth_method = SshAuthMethod::Password;
    let args = f.argv(&h, "", "").unwrap();
    assert!(has(&args, "BatchMode=no")); assert!(!has(&args, "BatchMode=yes"));
}
#[test]
fn jump_and_direct_connect_timeouts() {
    let f = Fixture::new();
    assert!(has(&f.argv(&host(), "", "").unwrap(), "ConnectTimeout=3"));
    let mut h = host(); h.jump_host = Some("relay@127.0.0.1:2200".into());
    assert!(has(&f.argv(&h, "", "").unwrap(), "ConnectTimeout=10"));
}
#[test]
fn ssh_plan_still_clears_forwardings() {
    let plan = crate::ssh::direct::ssh_plan(&host(), "true".into(), false).unwrap();
    assert!(has(&plan.args, "ClearAllForwardings=yes"));
    assert!(!plan.args.iter().any(|s| s.starts_with("-L")));
}
#[test]
fn zero_local_port_has_no_argv() {
    let f = Fixture::new();
    let error = tunnel_argv(&host(), &spec(), 0, &f.empty, &ConfigSource::Fixture { user: "", system: "" }).unwrap_err();
    assert_eq!(error.code, IpcErrorCode::InvalidArgument);
}
#[test]
fn jump_identity_is_refused() {
    let f = Fixture::new(); let mut h = host(); h.jump_host = Some("bastion".into());
    assert_eq!(piece(f.argv(&h, "Host bastion\n HostName 127.0.0.1\n IdentityFile /tmp/hop-key\n", "").unwrap_err()), "jump_identity");
}
#[test]
fn jump_password_is_refused() {
    let f = Fixture::new(); let mut h = host(); h.auth_method = SshAuthMethod::Password; h.jump_host = Some("bastion".into());
    assert_eq!(piece(f.argv(&h, "", "").unwrap_err()), "jump_password");
}
#[test]
fn jump_shell_metacharacters_are_rejected() {
    let f = Fixture::new();
    for ch in "`$();|&<>*?{}~!#\\\"'% ,/\t\n".chars() {
        let mut h = host(); h.jump_host = Some(format!("bast{ch}ion"));
        assert_eq!(piece(f.argv(&h, "", "").unwrap_err()), "jump_config", "{ch:?}");
    }
    for raw in ["ssh://ops@bastion:2222", "[broken]:22", "[fd00::1]:0", "[fd00::1];true", "-bastion"] {
        let mut h = host(); h.jump_host = Some(raw.into());
        assert_eq!(piece(f.argv(&h, "", "").unwrap_err()), "jump_config", "{raw}");
    }
    for raw in ["[fd00::1]", "ops@[fd00::1]:2222"] { assert!(parse_hop(raw).is_ok()); }
    let mut h = host(); h.jump_host = Some("bastion".into());
    assert_eq!(piece(f.argv(&h, "Host bastion\n User bad;user\n", "").unwrap_err()), "jump_config");
    h.hostname = "bad'host".into();
    assert_eq!(piece(f.argv(&h, "", "").unwrap_err()), "config_only_endpoint");
}
#[test]
fn refuse_match_exec() {
    let f = Fixture::new(); let marker = f.dir.path().join("must-not-exist");
    for keyword in ["Match exec", "mAtCh=exec"] {
        let text = format!("{keyword} \"touch {}\"\n", marker.display());
        assert_eq!(piece(f.argv(&host(), &text, "").unwrap_err()), "match_block");
        assert!(!marker.exists(), "-G must not execute Match exec");
    }
    let include = f.dir.path().join("included.conf");
    std::fs::write(&include, "Match exec true\n").unwrap();
    assert_eq!(piece(f.argv(&host(), &format!("Include {}\n", include.display()), "").unwrap_err()), "match_block");
}
#[test]
fn refuse_match_host() {
    let f = Fixture::new();
    for text in ["Match host genericbox.local\n Port 2222\n", "Match=final all\n"] {
        assert_eq!(piece(f.argv(&host(), "", text).unwrap_err()), "match_block");
    }
}
#[test]
fn stock_system_config_connects() {
    let f = Fixture::new();
    let stock = "Host *\n UseKeychain yes\n IdentityAgent /tmp/agent.sock\n IdentitiesOnly yes\n";
    let args = f.argv(&host(), stock, "").unwrap();
    assert_eq!(args.iter().filter(|s| *s == "IgnoreUnknown=UseKeychain").count(), 1);
    let usekeychain = args.iter().position(|s| s.eq_ignore_ascii_case("UseKeychain=yes")).unwrap();
    assert!(args.iter().position(|s| s == "IgnoreUnknown=UseKeychain").unwrap() < usekeychain);
    assert_eq!(args.iter().filter(|s| s.eq_ignore_ascii_case("UseKeychain=yes")).count(), 1);
    assert!(has(&args, "UpdateHostKeys=no"));
    assert_eq!(args, f.argv(&host(), &format!("IgnoreUnknown UseKeychain\n{stock}"), "").unwrap());
    for system in ["Host *\n SendEnv LANG LC_*\n HashKnownHosts yes\n GSSAPIAuthentication yes\n",
        "Host *\n XAuthLocation /opt/X11/bin/xauth\n ForwardX11 yes\n ForwardX11Trusted yes\n"] {
        assert!(f.argv(&host(), "", system).is_ok());
    }
}
#[test]
fn forwarding_directive_is_overridden_not_refused() {
    for key in FORWARDING { assert!(forwarding_directive(key)); }
    assert!(!forwarding_directive("proxycommand"));
    let f = Fixture::new();
    assert!(f.argv(&host(), "Host *\n LocalForward 127.0.0.1:23457 127.0.0.1:23458\n RemoteForward 23459 127.0.0.1:23458\n DynamicForward 23460\n", "").is_ok());
}
#[test]
fn already_equivalent_config_passes_the_g_diff() {
    let f = Fixture::new();
    assert!(f.argv(&host(), "", "").is_ok(), "no-config -N/-T and forced defaults must pass");
    assert!(f.argv(&host(), "Host genericbox.local\n User ignored\n Port 3333\n", "").is_ok(), "record user/port override both sides");
}
#[test]
fn forwarding_only_diff_connects() {
    let f = Fixture::new(); let args = f.argv(&host(), "Host *\n LocalForward 23457 localhost:22\n", "").unwrap();
    assert_eq!(args.iter().filter(|s| s.starts_with("-L")).count(), 1);
    assert_eq!(args.last().unwrap(), "-L127.0.0.1:23456:127.0.0.1:45678");
}
#[test]
#[cfg(unix)]
fn empty_config_trust_root_is_enforced() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new(); assert!(validate_empty_config(&f.empty).is_ok());
    std::fs::write(&f.empty, "ProxyCommand false\n").unwrap();
    assert_eq!(piece(validate_empty_config(&f.empty).unwrap_err()), "config_path");
    std::fs::remove_file(&f.empty).unwrap();
    let target = f.dir.path().join("target"); std::fs::write(&target, "").unwrap();
    symlink(&target, &f.empty).unwrap();
    assert_eq!(piece(validate_empty_config(&f.empty).unwrap_err()), "config_path");
    std::fs::remove_file(&f.empty).unwrap(); std::fs::write(&f.empty, "").unwrap();
    std::fs::set_permissions(&f.empty, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(piece(validate_empty_config(&f.empty).unwrap_err()), "config_path");
    std::fs::set_permissions(&f.empty, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(f.dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(piece(validate_empty_config(&f.empty).unwrap_err()), "config_path");
}
#[test]
fn per_setting_equivalence_refusals() {
    let f = Fixture::new();
    let rows = [
        ("Host *\n KexAlgorithms curve25519-sha256\n", "unsupported_directive"),
        ("Host *\n LogLevel ERROR\n", "unsupported_directive"),
        ("Host *\n CertificateFile /tmp/cert\n", "certificate_file"),
        ("Host *\n IdentityFile ~/.ssh/%h.key\n", "identity_file_token"),
        ("Host *\n IdentityFile /tmp/other-key\n", "config_only_endpoint"),
        ("Host *\n IdentityFile /tmp/key-a\n IdentityFile /tmp/key-b\n", "config_only_endpoint"),
        ("Host *\n ProxyJump bastion\n", "config_only_jump"),
        ("Host genericbox.local\n HostName 10.0.0.5\n", "config_only_endpoint"),
        ("Host *\n DefinitelyNotAnSshOption yes\n", "unsupported_directive"),
    ];
    for (text, expected) in rows { assert_eq!(piece(f.argv(&host(), text, "").unwrap_err()), expected, "{text}"); }
    let mut no_port = host(); no_port.port = None; no_port.username = None;
    for text in ["Host *\n Port 3333\n", "Host *\n User anotheruser\n"] {
        assert_eq!(piece(f.argv(&no_port, text, "").unwrap_err()), "config_only_endpoint");
    }
    let mut imported = host(); imported.source = SshHostSource::Config;
    assert_eq!(piece(f.argv(&imported, "", "").unwrap_err()), "config_alias");
    let text = "Host prod\n HostName genericbox.local\n User ops\n Port 2222\nHost prod-tunnel\n HostName genericbox.local\n User ops\n Port 2222\n HostKeyAlias private\n";
    assert_eq!(piece(f.argv(&imported, text, "").unwrap_err()), "config_alias");
    let mut mixed = host(); mixed.hostname = "GenericBox.local".into();
    for name in ["GenericBox.local", "genericbox.local"] {
        assert!(f.argv(&mixed, &format!("Host *\n HostName {name}\n"), "").is_ok());
    }
    // ssh ignores a nonexistent command-line -i before -G, whereas a config
    // IdentityFile remains in its list. Supply a real file so this row measures
    // tilde normalization and de-duplication, not missing-key CLI semantics.
    let home = dirs::home_dir().unwrap();
    let key_dir = tempfile::Builder::new().prefix("fx-argv-key-").tempdir_in(&home).unwrap();
    std::fs::write(key_dir.path().join("id_x"), "fixture identity\n").unwrap();
    let relative = key_dir.path().strip_prefix(&home).unwrap().join("id_x");
    let tilde_key = format!("~/{}", relative.to_string_lossy());
    let mut key = host(); key.identity_file = Some(tilde_key.clone());
    for indentation in ["", " ", "\t"] {
        assert_eq!(words(&format!("{indentation}IdentityFile {tilde_key}")).unwrap(), vec!["IdentityFile".to_string(), tilde_key.clone()]);
        assert!(f.argv(&key, &format!("Host *\n{indentation}IdentityFile {tilde_key}\n"), "").is_ok());
    }
    let parsed = parse_g(&format!(" identityfile {tilde_key}\n\tidentityfile {tilde_key}\n"));
    assert_eq!(identities(&parsed).unwrap(), vec![normalize_identity(&tilde_key).unwrap()]);
    let args = f.argv(&host(), "Host *\n IdentitiesOnly yes\n", "").unwrap();
    assert!(args.iter().any(|v| v.eq_ignore_ascii_case("IdentitiesOnly=yes")));
    let mut jump = host(); jump.jump_host = Some("bastion".into());
    let debian = "Host *\n SendEnv LANG LC_*\n HashKnownHosts yes\n GSSAPIAuthentication yes\n";
    assert!(f.argv(&jump, "Host bastion\n HostName 127.0.0.1\n User relay\n Port 2200\n", debian).is_ok());
    assert!(f.argv(&jump, "Host *\n UseKeychain yes\n", "").is_ok());
    for text in ["Host *\n IdentityAgent /tmp/agent\n", "Host bastion\n HostName 127.0.0.1\nHost 127.0.0.1\n IdentityAgent /tmp/agent\n"] {
        assert_eq!(piece(f.argv(&jump, text, "").unwrap_err()), "jump_identity");
    }
    for text in ["Host bastion\n ProxyJump nested\n", "Host bastion\n StrictHostKeyChecking yes\n"] {
        assert_eq!(piece(f.argv(&jump, text, "").unwrap_err()), "jump_config");
    }
    let mut tailscale = host(); tailscale.source = SshHostSource::Tailscale;
    assert_eq!(piece(f.argv(&tailscale, "Host *\n KexAlgorithms curve25519-sha256\n", "").unwrap_err()), "unsupported_directive");
    assert!(f.argv(&tailscale, "", debian).is_ok());
    assert_eq!(piece(f.argv(&host(), "Host *\n Port not-a-number\n", "").unwrap_err()), "bad_option");
}
#[test]
fn identity_agent_spaced_path_is_quoted() {
    let f = Fixture::new();
    let path = "/tmp/Library/Group Containers/agent.sock";
    let args = f.argv(&host(), &format!("Host *\n IdentityAgent \"{path}\"\n"), "").unwrap();
    assert!(has(&args, &format!("IdentityAgent=\"{path}\"")), "value sourced through -G parsing");
    assert_eq!(value(&parse_g(&format!("identityagent {path}\n")), "identityagent"), Some(path));
    for value in ["SSH_AUTH_SOCK", "/tmp/literal%agent", "/tmp/${AGENT}"] {
        let parsed = parse_g(&format!("identityagent {value}\n"));
        assert_eq!(piece(quote_agent(super::value(&parsed, "identityagent").unwrap()).unwrap_err()), "identity_agent");
    }
}
#[test]
fn option_injection_is_refused() {
    // Deliberately nonexistent config path proves input validation precedes even
    // the config load, hence there can be no -G or tunnel spawn on these rows.
    for field in ["hostname", "jump_host", "username"] {
        let mut h = host();
        match field { "hostname" => h.hostname = "-F/tmp/x".into(), "jump_host" => h.jump_host = Some("-F/tmp/x".into()), _ => h.username = Some("-oProxyCommand=touch".into()) }
        G_SPAWNS.with(|count| count.set(0));
        let error = tunnel_argv(&h, &spec(), 23456, Path::new("/does-not-exist/empty"), &ConfigSource::Fixture { user: "Match exec false", system: "" }).unwrap_err();
        assert_eq!(piece(error), if field == "jump_host" { "jump_config" } else { "config_only_endpoint" });
        G_SPAWNS.with(|count| assert_eq!(count.get(), 0, "injected input must never spawn -G"));
        // tunnel_argv never spawns a tunnel; Err also yields no spawnable argv.
    }
    let f = Fixture::new(); let mut h = host(); h.hostname = "fd00::5".into(); h.jump_host = Some("ops@[fd00::1]".into());
    assert!(f.argv(&h, "", "").is_ok());
    h.hostname = "[fd00::5]".into(); assert!(f.argv(&h, "", "").is_ok());
}

}
