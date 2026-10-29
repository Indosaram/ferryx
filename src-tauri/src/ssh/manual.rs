//! Upload adapter for an `ssh` the user typed by hand in a local pane.
//!
//! The user's own argv is the connection contract: config file, jump hosts, identity, port,
//! `-o` settings and ControlPath all survive. Forced options are placed first because ssh keeps
//! the first value it sees; ControlPath is never overridden so a live master is reused, and
//! ControlMaster=no stops this probe from becoming a new master.
use super::runtime::{parse_fields, RemoteEnvironment, RemoteExecutor, RemotePlatform};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::terminal::shell::ShellCommandPlan;
use std::time::Duration;

fn unsupported(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::Unsupported, message)
        .with_details(serde_json::json!({ "code": crate::terminal::manual_ssh::CODE_UNSUPPORTED }))
}

/// The user's connection, reduced to what a non-interactive upload may reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualSshTarget {
    pub program: String,
    pub options: Vec<String>,
    pub destination: String,
}

const FLAGS_KEPT: &[char] = &['4', '6', 'A', 'a', 'C', 'K', 'k', 'X', 'x', 'Y', 'q', 'v'];
const FLAGS_IGNORED: &[char] = &['t', 'T', 'n', 'e'];
const FLAGS_REJECTED: &[char] = &['N', 'f', 's', 'G', 'V', 'M', 'W', 'O', 'Q'];
const VALUES_KEPT: &[char] = &['b', 'B', 'c', 'F', 'i', 'I', 'J', 'l', 'm', 'o', 'p', 'P', 'S'];
const VALUES_DROPPED: &[char] = &['D', 'E', 'L', 'R', 'w', 'e'];
/// Forwarding and log flags must not replay onto a probe; paths get resolved against cwd.
const PATH_VALUES: &[char] = &['F', 'i', 'S'];

fn rejected_option(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let key = lower.split(['=', ' ']).next().unwrap_or("");
    let rest = lower[key.len()..].trim_start_matches(['=', ' ']);
    match key {
        "remotecommand" => rest != "none",
        "sessiontype" => rest != "default",
        "forkafterauthentication" | "stdinnull" => rest == "yes",
        _ => false,
    }
}

/// Parses real argv with OpenSSH's rules (options may follow the destination; `--` ends them).
/// A trailing remote command means the pane is not an interactive login shell.
pub fn parse_argv(argv: &[String], cwd: Option<&str>) -> Result<ManualSshTarget, IpcError> {
    let program = argv.first().cloned().ok_or_else(|| unsupported("Empty SSH argv"))?;
    let mut options = Vec::new();
    let mut destination: Option<String> = None;
    let mut index = 1;
    let mut options_done = false;
    while index < argv.len() {
        let arg = &argv[index];
        index += 1;
        if !options_done && arg == "--" {
            options_done = true;
            continue;
        }
        if options_done || !arg.starts_with('-') || arg == "-" {
            if destination.is_none() {
                destination = Some(arg.clone());
                continue;
            }
            return Err(unsupported("SSH was started with a remote command, not an interactive shell"));
        }
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut at = 0;
        while at < chars.len() {
            let flag = chars[at];
            at += 1;
            if FLAGS_REJECTED.contains(&flag) {
                return Err(unsupported(format!("SSH option -{flag} is not an interactive session")));
            }
            if FLAGS_KEPT.contains(&flag) {
                options.push(format!("-{flag}"));
                continue;
            }
            if FLAGS_IGNORED.contains(&flag) && !VALUES_DROPPED.contains(&flag) {
                continue;
            }
            let takes_value = VALUES_KEPT.contains(&flag) || VALUES_DROPPED.contains(&flag);
            if !takes_value {
                if flag == 'y' || flag == 'g' {
                    continue;
                }
                return Err(unsupported(format!("Unknown SSH option -{flag}")));
            }
            let value = if at < chars.len() {
                let v: String = chars[at..].iter().collect();
                at = chars.len();
                v
            } else {
                let v = argv.get(index).cloned().ok_or_else(|| unsupported(format!("SSH option -{flag} is missing its value")))?;
                index += 1;
                v
            };
            if VALUES_DROPPED.contains(&flag) {
                continue;
            }
            if flag == 'o' && rejected_option(&value) {
                return Err(unsupported(format!("SSH option {value} is not an interactive session")));
            }
            let value = if PATH_VALUES.contains(&flag) {
                absolute(&value, cwd)?
            } else if flag == 'o' {
                resolve_path_option(&value, cwd)?
            } else {
                value
            };
            options.push(format!("-{flag}"));
            options.push(value);
        }
    }
    let destination = destination.ok_or_else(|| unsupported("SSH argv has no destination"))?;
    Ok(ManualSshTarget { program, options, destination })
}

/// Builds the upload target for a detected process. The kernel-reported executable wins over
/// argv[0], which the GUI's own PATH could resolve to a different ssh binary. Either may be a
/// relative exec path, which only means something against the ssh's own cwd.
pub fn target_for(process: &crate::terminal::manual_ssh::ManualSshProcess) -> Result<ManualSshTarget, IpcError> {
    let cwd = process.cwd.as_deref();
    let mut target = parse_argv(&process.argv, cwd)?;
    let program = process.executable.clone().unwrap_or_else(|| target.program.clone());
    target.program = if program.contains(['/', '\\']) { absolute(&program, cwd)? } else { program };
    Ok(target)
}

fn resolve_path_option(option: &str, cwd: Option<&str>) -> Result<String, IpcError> {
    let option = option.trim_start();
    let Some(separator) = option.find(|c: char| c == '=' || c.is_whitespace()) else {
        return Ok(option.to_owned());
    };
    let key = &option[..separator];
    if !matches!(key.to_ascii_lowercase().as_str(),
        "identityfile" | "certificatefile" | "userknownhostsfile" | "globalknownhostsfile" |
        "controlpath" | "identityagent") {
        return Ok(option.to_owned());
    }
    let value = option[separator..].trim_start_matches(|c: char| c == '=' || c.is_whitespace());
    if std::path::Path::new(value).is_absolute() || value.starts_with('~') || value.eq_ignore_ascii_case("none") {
        return Ok(option.to_owned());
    }
    if value.contains(['\'', '"']) || value.chars().any(char::is_whitespace) {
        return Err(unsupported("SSH path options with quoted or multiple paths are not supported for image upload"));
    }
    if value.is_empty() {
        return Err(unsupported("SSH path option is empty"));
    }
    if value.starts_with('$') || value.starts_with('%') {
        return Ok(option.to_owned());
    }
    let resolved = absolute(value, cwd)?;
    if resolved.contains(['"', '\n', '\r']) {
        return Err(unsupported("SSH path cannot be represented as an option"));
    }
    Ok(format!("{key}=\"{resolved}\""))
}

fn absolute(value: &str, cwd: Option<&str>) -> Result<String, IpcError> {
    let path = std::path::Path::new(value);
    if path.is_absolute() || value.starts_with('~') || value.eq_ignore_ascii_case("none") || value.starts_with('%') {
        return Ok(value.to_string());
    }
    let cwd = cwd.ok_or_else(|| unsupported("SSH uses a relative path but its working directory is unknown"))?;
    Ok(std::path::Path::new(cwd).join(path).to_string_lossy().into_owned())
}

impl ManualSshTarget {
    pub fn plan(&self, command: String) -> ShellCommandPlan {
        let mut args: Vec<String> = [
            "-T",
            "-o", "BatchMode=yes",
            "-o", "ControlMaster=no",
            "-o", "ConnectTimeout=10",
            "-o", "RemoteCommand=none",
            "-o", "RequestTTY=no",
            "-o", "SessionType=default",
            "-o", "PermitLocalCommand=no",
            "-o", "ClearAllForwardings=yes",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        args.extend(self.options.iter().cloned());
        args.push("--".into());
        args.push(self.destination.clone());
        args.push(command);
        ShellCommandPlan { program: self.program.clone(), args }
    }
}

/// Minimal platform probe over the manual plan (mirrors `runtime::detect`'s scripts).
pub async fn detect(target: &ManualSshTarget) -> Result<RemoteEnvironment, IpcError> {
    let marker = format!("FERRYX_ENV_V1_{}", uuid::Uuid::new_v4().simple());
    let mut last = None;
    for executor in [RemoteExecutor::Powershell, RemoteExecutor::Sh] {
        let script = match executor {
            RemoteExecutor::Sh => format!(
                "os=$(uname -s) || exit; case \"$os\" in Linux|Darwin|FreeBSD|OpenBSD|NetBSD) ;; *) exit 2;; esac; \
                 printf '{marker}\\000posix\\000%s\\000%s\\000%s\\000' \"$os\" \"$HOME\" \"${{TMPDIR:-/tmp}}\""
            ),
            _ => format!(
                "if ([Environment]::OSVersion.Platform -ne 'Win32NT') {{ exit 2 }}; \
                 [Console]::Write(('{marker}','windows',$PSVersionTable.PSVersion.ToString(),$HOME,[IO.Path]::GetTempPath(),'' -join [char]0))"
            ),
        };
        match super::direct::bounded_output(&target.plan(executor.command(&script)), Duration::from_secs(8)).await {
            Ok(output) => return environment_from(executor, &output, &marker),
            // 255 is a transport failure: every executor would fail the same way.
            Err(err) if err.details.as_ref().and_then(|d| d.get("exitCode")).and_then(|v| v.as_i64()) == Some(255) => return Err(err),
            Err(err) => last = Some(err),
        }
    }
    Err(last.unwrap_or_else(|| unsupported("No supported remote executor")))
}

fn environment_from(executor: RemoteExecutor, output: &[u8], marker: &str) -> Result<RemoteEnvironment, IpcError> {
    let fields = parse_fields(output, marker, 4)?;
    let platform = match (executor, fields[0]) {
        (RemoteExecutor::Sh, "posix") => RemotePlatform::Posix,
        (RemoteExecutor::Powershell, "windows") => RemotePlatform::Windows,
        _ => return Err(unsupported("SSH environment does not match its executor")),
    };
    Ok(RemoteEnvironment {
        platform,
        executor,
        version: fields[1].into(),
        home: fields[2].into(),
        temp: fields[3].into(),
        git: false,
        runtime_dir: None,
        os: String::new(),
        arch: String::new(),
    })
}

pub async fn upload(target: &ManualSshTarget, file_name: &str, bytes: Vec<u8>) -> Result<String, IpcError> {
    let environment = detect(target).await?;
    super::operations::upload_with(&environment, file_name, bytes, |command| Ok(target.plan(command))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(marker: &str, fields: [&str; 4]) -> Vec<u8> {
        let mut out = b"login banner\n".to_vec();
        for field in std::iter::once(marker).chain(fields) {
            out.extend_from_slice(field.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn probe_output_identifies_windows_linux_and_macos_targets() {
        let m = "FERRYX_ENV_V1_x";
        let win = environment_from(RemoteExecutor::Powershell, &probe(m, ["windows", "5.1", "C:\\Users\\u", "C:\\Temp\\"]), m).unwrap();
        assert!(matches!(win.platform, RemotePlatform::Windows));
        assert_eq!(win.temp, "C:\\Temp\\");
        for os in ["Linux", "Darwin"] {
            let env = environment_from(RemoteExecutor::Sh, &probe(m, ["posix", os, "/home/u", "/tmp"]), m).unwrap();
            assert!(matches!(env.platform, RemotePlatform::Posix));
            assert_eq!(env.version, os);
        }
        // A shell answering for the wrong executor is refused, never guessed.
        assert!(environment_from(RemoteExecutor::Sh, &probe(m, ["windows", "5.1", "h", "t"]), m).is_err());
    }

    /// Live Mac-to-target proof, run on a remote verify host only:
    /// FERRYX_MANUAL_SSH_E2E="win-host linux-host mac-host" cargo test --lib -- --ignored manual_ssh_live
    #[tokio::test]
    #[ignore]
    async fn manual_ssh_live_upload_to_each_target() {
        let hosts = std::env::var("FERRYX_MANUAL_SSH_E2E").expect("set FERRYX_MANUAL_SSH_E2E");
        for host in hosts.split_whitespace() {
            let mut args = argv(&["ssh"]);
            if let Ok(config) = std::env::var("FERRYX_MANUAL_SSH_CONFIG") {
                args.extend(["-F".into(), config]);
            }
            args.push(host.into());
            let target = parse_argv(&args, None).unwrap();
            let env = detect(&target).await.unwrap_or_else(|e| panic!("{host}: {e:?}"));
            let name = format!("{}.png", uuid::Uuid::new_v4());
            let path = upload(&target, &name, vec![0x89, b'P', b'N', b'G']).await.unwrap_or_else(|e| panic!("{host}: {e:?}"));
            assert!(path.ends_with(&name), "{host}: {path}");
            let command = match env.platform {
                RemotePlatform::Posix => format!("cat -- {}", super::super::direct::quote_posix(&path)),
                RemotePlatform::Windows => format!(
                    "[Console]::OpenStandardOutput().Write([IO.File]::ReadAllBytes({}),0,4)",
                    super::super::runtime::powershell_data(&path)
                ),
            };
            let bytes = super::super::direct::bounded_output(
                &target.plan(env.executor.command(&command)), Duration::from_secs(15),
            ).await.unwrap();
            assert_eq!(bytes, [0x89, b'P', b'N', b'G']);
            println!("MANUAL_SSH_LIVE host={host} platform={:?} path={path}", env.platform);
        }
    }

    pub(super) fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn process(argv: &[&str], executable: Option<&str>, cwd: Option<&str>) -> crate::terminal::manual_ssh::ManualSshProcess {
        crate::terminal::manual_ssh::ManualSshProcess {
            pid: 7,
            foreground_group: 7,
            argv: super::tests::argv(argv),
            executable: executable.map(String::from),
            cwd: cwd.map(String::from),
        }
    }

    #[test]
    fn kernel_executable_replaces_argv0() {
        let executable = std::env::temp_dir().join("ssh");
        let executable = executable.to_str().unwrap();
        let t = target_for(&process(&["ssh", "box"], Some(executable), None)).unwrap();
        assert_eq!(t.program, executable);
        let bare = target_for(&process(&["ssh", "box"], None, None)).unwrap();
        assert_eq!(bare.program, "ssh");
    }

    // Detection only yields a process on unix hosts, so path semantics are unix-only.
    #[cfg(unix)]
    #[test]
    fn relative_argv0_without_executable_resolves_against_ssh_cwd() {
        let t = target_for(&process(&["./bin/ssh", "box"], None, Some("/home/u"))).unwrap();
        assert_eq!(t.program, "/home/u/./bin/ssh");
        assert!(target_for(&process(&["./bin/ssh", "box"], None, None)).is_err());
        let exec = target_for(&process(&["ssh", "box"], Some("./ssh"), Some("/w"))).unwrap();
        assert_eq!(exec.program, "/w/./ssh");
    }

    #[cfg(unix)]
    #[test]
    fn keeps_connection_options_and_spaces_verbatim() {
        let t = parse_argv(&argv(&["ssh", "-F", "/cfg dir/c", "-J", "jump", "-p2222", "-i", "/k y/id", "-l", "me", "-o", "ControlPath=/tmp/cm %C", "box"]), None).unwrap();
        assert_eq!(t.destination, "box");
        assert_eq!(t.options, argv(&["-F", "/cfg dir/c", "-J", "jump", "-p", "2222", "-i", "/k y/id", "-l", "me", "-o", "ControlPath=/tmp/cm %C"]));
    }

    #[cfg(unix)]
    #[test]
    fn options_after_destination_and_combined_flags_parse() {
        let t = parse_argv(&argv(&["ssh", "host", "-4Ap", "22", "-S", "/s"]), None).unwrap();
        assert_eq!(t.destination, "host");
        assert_eq!(t.options, argv(&["-4", "-A", "-p", "22", "-S", "/s"]));
    }

    #[cfg(unix)]
    #[test]
    fn relative_paths_resolve_against_ssh_cwd() {
        let t = parse_argv(&argv(&["ssh", "-i", "keys/id", "h"]), Some("/home/u")).unwrap();
        assert_eq!(t.options, argv(&["-i", "/home/u/keys/id"]));
        assert!(parse_argv(&argv(&["ssh", "-i", "keys/id", "h"]), None).is_err());
    }

    #[test]
    fn relative_config_paths_use_the_session_directory() {
        let cwd = std::env::temp_dir().join("ssh session");
        let cwd = cwd.to_str().unwrap();
        for key in ["IdentityFile", "CertificateFile", "ControlPath", "IdentityAgent", "UserKnownHostsFile"] {
            let option = format!("{key}=relative-file");
            let target = parse_argv(&argv(&["ssh", "-o", &option, "host"]), Some(cwd)).unwrap();
            let expected = std::path::Path::new(cwd).join("relative-file");
            assert_eq!(target.options, argv(&["-o", &format!("{key}=\"{}\"", expected.display())]));
            assert!(parse_argv(&argv(&["ssh", "-o", &option, "host"]), None).is_err());
        }
    }

    #[test]
    fn ambiguous_relative_path_lists_are_not_rebased_as_one_file() {
        assert!(resolve_path_option("UserKnownHostsFile=first second", Some("/work")).is_err());
        assert!(resolve_path_option("IdentityFile=\"key file\"", Some("/work")).is_err());
        for option in ["IdentityAgent=none", "ControlPath=~/.ssh/%C", "IdentityAgent=$SSH_AUTH_SOCK"] {
            assert_eq!(resolve_path_option(option, None).unwrap(), option);
        }
    }

    #[test]
    fn non_interactive_invocations_are_rejected() {
        for bad in [
            &["ssh", "-N", "h"][..],
            &["ssh", "-W", "x:22", "h"],
            &["ssh", "h", "uptime"],
            &["ssh", "-o", "RemoteCommand=top", "h"],
            &["ssh", "-oSessionType=none", "h"],
            &["ssh", "-G", "h"],
            &["ssh"],
            &["ssh", "-p"],
        ] {
            assert!(parse_argv(&argv(bad), None).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn forwarding_flags_are_not_replayed() {
        let t = parse_argv(&argv(&["ssh", "-L", "8080:x:80", "-D1080", "-t", "h"]), None).unwrap();
        assert!(t.options.is_empty());
    }

    #[test]
    fn plan_keeps_control_path_and_never_becomes_a_master() {
        let t = parse_argv(&argv(&["/usr/bin/ssh", "-o", "ControlPath=~/.ssh/cm-%C", "box"]), None).unwrap();
        let plan = t.plan("sh -c true".into());
        assert_eq!(plan.program, "/usr/bin/ssh");
        assert!(plan.args.windows(2).any(|w| w == ["-o", "ControlMaster=no"]));
        assert!(plan.args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
        assert!(!plan.args.iter().any(|a| a == "ControlPath=none"));
        let forced = plan.args.iter().position(|a| a == "ControlMaster=no").unwrap();
        let user = plan.args.iter().position(|a| a == "ControlPath=~/.ssh/cm-%C").unwrap();
        assert!(forced < user);
        assert_eq!(&plan.args[plan.args.len() - 3..], &argv(&["--", "box", "sh -c true"])[..]);
    }
}
