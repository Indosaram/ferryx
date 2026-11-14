use super::SshHost;
use crate::ipc::{IpcError, IpcErrorCode};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub fn resolve_ssh_program() -> Result<String, IpcError> {
    resolve_ssh_program_platform(
        cfg!(windows),
        std::env::var_os("PATH"),
        std::env::var("SystemRoot").ok(),
        std::env::var("ProgramFiles").ok(),
        |p| p.is_file(),
    )
}

fn split_path_list(value: &std::ffi::OsStr, is_windows: bool) -> Vec<PathBuf> {
    if !is_windows {
        return std::env::split_paths(value).collect();
    }
    value
        .to_string_lossy()
        .split(';')
        .map(|entry| entry.trim().trim_matches('"'))
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn resolve_ssh_program_platform<F>(
    is_windows: bool,
    path_env: Option<OsString>,
    system_root: Option<String>,
    program_files: Option<String>,
    file_exists: F,
) -> Result<String, IpcError>
where
    F: Fn(&Path) -> bool,
{
    if !is_windows {
        return Ok("ssh".to_string());
    }

    if let Some(paths) = path_env {
        for dir in split_path_list(&paths, is_windows) {
            for candidate in ["ssh.exe", "ssh.cmd", "ssh"] {
                let candidate_path = dir.join(candidate);
                if file_exists(&candidate_path) {
                    return Ok(candidate_path.to_string_lossy().into_owned());
                }
            }
        }
    }

    let sys_root = system_root.as_deref().unwrap_or(r"C:\Windows");
    let sys32_ssh = PathBuf::from(sys_root)
        .join("System32")
        .join("OpenSSH")
        .join("ssh.exe");
    if file_exists(&sys32_ssh) {
        return Ok(sys32_ssh.to_string_lossy().into_owned());
    }

    if let Some(prog_files) = program_files.as_deref() {
        let prog_ssh = PathBuf::from(prog_files)
            .join("OpenSSH")
            .join("ssh.exe");
        if file_exists(&prog_ssh) {
            return Ok(prog_ssh.to_string_lossy().into_owned());
        }
    }

    Err(IpcError::new(
        IpcErrorCode::NotFound,
        format!(
            "OpenSSH client binary not found. Expected at {} or in PATH",
            sys32_ssh.display()
        ),
    ))
}

/// The `-J` target for `host`, or `None` when none is configured.
///
/// A whitespace-only saved value is treated as absent: `-J ""` is a malformed
/// destination that ssh rejects at connect time, so a blank field must never reach
/// the argument list. The character set itself is validated by
/// `ssh::direct::validate_host` before any argv is built.
pub fn jump_target(host: &SshHost) -> Result<Option<String>, IpcError> {
    let Some(raw) = host.jump_host.as_deref() else {
        return Ok(None);
    };
    let jump = raw.trim();
    if jump.is_empty() {
        return Ok(None);
    }
    // Validation trims each comma part, so the argv must carry the same trimmed parts: passing a
    // part with its interior padding through would hand ssh a different value than the one that
    // was validated.
    let jump = jump
        .split(',')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(",");
    // This is the last point before `-J` reaches ssh's own argument parser, so the value is
    // validated here as well as at registration. A jump host that carries option syntax would
    // otherwise change ssh's semantics (a ProxyCommand, a config file, another destination).
    // Registration rejects these values already, so this is a last line of defence - and it FAILS
    // rather than skipping the option: dropping `-J` would connect straight to the target and
    // silently skip the bastion, which is the opposite of what a jump host means.
    crate::ssh::direct::validate_jump_host(&jump)?;
    Ok(Some(jump))
}

pub fn probe_argv(host: &SshHost) -> Result<Vec<String>, IpcError> {
    let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    let mut argv = vec![
        program,
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        // A jump host pays two handshakes; 2.5 s is a single-hop bound.
        if host.jump_host.is_some() {
            "ConnectTimeout=10".to_string()
        } else {
            "ConnectTimeout=2.5".to_string()
        },
    ];
    append_options(&mut argv, host)?;
    append_target(&mut argv, host);
    Ok(argv)
}

pub fn interactive_argv(host: &SshHost) -> Result<Vec<String>, IpcError> {
    let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    let mut argv = vec![program, "-tt".to_string()];
    append_options(&mut argv, host)?;
    append_target(&mut argv, host);
    Ok(argv)
}

fn append_options(argv: &mut Vec<String>, host: &SshHost) -> Result<(), IpcError> {
    if let Some(port) = host.port {
        argv.push("-p".to_string());
        argv.push(port.to_string());
    }
    if let Some(identity) = &host.identity_file {
        argv.push("-i".to_string());
        argv.push(identity.clone());
    }
    if let Some(jump) = jump_target(host)? {
        argv.push("-J".to_string());
        argv.push(jump);
    }
    Ok(())
}

fn append_target(argv: &mut Vec<String>, host: &SshHost) {
    argv.push(host.target());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::SshAuthMethod;
    use crate::ssh::SshHostSource;

    fn host(
        port: Option<u16>,
        identity: Option<&str>,
        jump: Option<&str>,
        user: Option<&str>,
    ) -> SshHost {
        SshHost {
            id: "h".into(),
            label: "l".into(),
            hostname: "maho-win".into(),
            username: user.map(Into::into),
            port,
            identity_file: identity.map(Into::into),
            jump_host: jump.map(Into::into),
            source: SshHostSource::Config,
            auth_method: SshAuthMethod::Agent,
            disabled: None,
            shell: None,
        }
    }

    #[test]
    fn a_blank_jump_host_never_becomes_a_j_argument() {
        // A saved field of spaces is not a destination: `-J "  "` fails at connect time with
        // an opaque error, so it must be dropped before the argv is built.
        assert_eq!(jump_target(&host(None, None, Some("  "), None)).unwrap(), None);
        assert_eq!(jump_target(&host(None, None, Some(""), None)).unwrap(), None);
        assert!(!probe_argv(&host(None, None, Some("   "), None)).unwrap()
            .iter()
            .any(|arg| arg == "-J"));
    }

    #[test]
    fn a_real_jump_host_is_trimmed_and_forwarded() {
        assert_eq!(
            jump_target(&host(None, None, Some(" bastion "), None))
                .unwrap()
                .as_deref(),
            Some("bastion")
        );
        assert!(probe_argv(&host(None, None, Some("bastion"), None)).unwrap()
            .windows(2)
            .any(|args| args == ["-J", "bastion"]));
    }

    #[test]
    fn an_injected_jump_host_is_rejected_before_argv_building() {
        // validate_host is the gate that keeps a jump host from changing ssh's argument
        // semantics; exec only forwards what survives it.
        for injected in ["bastion -o ProxyCommand=sh", "bastion;reboot", "-oProxyCommand=x"] {
            assert!(
                crate::ssh::direct::validate_host(&host(None, None, Some(injected), None)).is_err(),
                "{injected:?} must not pass the host validator"
            );
        }
        assert!(crate::ssh::direct::validate_host(&host(None, None, Some("user@bastion:22"), None)).is_ok());
    }

    /// The gate has to sit in the ARGV BUILDER, not only in the host validator: this calls the
    /// function that actually assembles `-J` and proves an injected value stops argv building
    /// outright instead of quietly producing a direct connection.
    #[test]
    fn the_argv_builder_refuses_an_injected_jump_host() {
        for injected in ["bastion -o ProxyCommand=sh", "bastion;reboot", "-oProxyCommand=x"] {
            let mut argv = Vec::new();
            let mut host = host(None, None, Some(injected), None);
            host.jump_host = Some(injected.to_string());
            assert!(
                append_options(&mut argv, &host).is_err(),
                "{injected:?} must be refused, not dropped: {argv:?}"
            );
            assert!(
                argv.is_empty(),
                "{injected:?} must not reach argv at all: {argv:?}"
            );
        }
    }

    #[test]
    fn probe_honors_saved_connection_options() {
        let argv = probe_argv(&host(
            Some(2200),
            Some("/keys/key with space"),
            Some("jump"),
            Some("user"),
        )).unwrap();
        for pair in [
            ["-p", "2200"],
            ["-i", "/keys/key with space"],
            ["-J", "jump"],
        ] {
            assert!(
                argv.windows(2).any(|args| args == pair),
                "missing {pair:?} in {argv:?}"
            );
        }
    }

    #[test]
    fn red_probe_argv_shape() {
        let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
        assert_eq!(
            probe_argv(&host(None, None, None, None)).unwrap(),
            vec![
                program.as_str(),
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=2.5",
                "maho-win"
            ]
        );
        assert_eq!(
            probe_argv(&host(None, None, None, Some("sook"))).unwrap()
                .last()
                .map(String::as_str),
            Some("sook@maho-win")
        );
    }

    #[test]
    fn red_interactive_argv_flags() {
        let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
        let bare = interactive_argv(&host(None, None, None, None)).unwrap();
        assert_eq!(bare, vec![program.as_str(), "-tt", "maho-win"]);

        let full = interactive_argv(&host(
            Some(2200),
            Some("~/.ssh/id_ed25519"),
            Some("bastion"),
            Some("sook"),
        )).unwrap();
        assert_eq!(
            full,
            vec![
                program.as_str(),
                "-tt",
                "-p",
                "2200",
                "-i",
                "~/.ssh/id_ed25519",
                "-J",
                "bastion",
                "sook@maho-win"
            ]
        );
    }

    #[test]
    fn red_windows_ssh_resolution() {
        use std::ffi::OsString;
        use std::path::Path;

        let in_path = Path::new(r"C:\fake\bin").join("ssh.exe");
        let path_env = Some(OsString::from(r"C:\fake\bin;C:\other\bin"));
        let resolved = resolve_ssh_program_platform(
            true,
            path_env,
            Some(r"C:\Windows".into()),
            Some(r"C:\Program Files".into()),
            |p| p == in_path,
        )
        .expect("should find in PATH");
        assert_eq!(Path::new(&resolved), in_path);

        let sys32 = Path::new(r"C:\Windows")
            .join("System32")
            .join("OpenSSH")
            .join("ssh.exe");
        let resolved_sys = resolve_ssh_program_platform(
            true,
            None,
            Some(r"C:\Windows".into()),
            Some(r"C:\Program Files".into()),
            |p| p == sys32,
        )
        .expect("should fall back to System32");
        assert_eq!(Path::new(&resolved_sys), sys32);

        let program_files = Path::new(r"C:\Program Files")
            .join("OpenSSH")
            .join("ssh.exe");
        let resolved_prog = resolve_ssh_program_platform(
            true,
            None,
            Some(r"C:\Windows".into()),
            Some(r"C:\Program Files".into()),
            |p| p == program_files,
        )
        .expect("should fall back to ProgramFiles");
        assert_eq!(Path::new(&resolved_prog), program_files);

        let err = resolve_ssh_program_platform(
            true,
            None,
            Some(r"C:\Windows".into()),
            Some(r"C:\Program Files".into()),
            |_| false,
        )
        .expect_err("should return error when binary missing");
        assert_eq!(err.code, IpcErrorCode::NotFound);
        assert!(err.message.contains("OpenSSH"));
        assert!(err
            .message
            .replace('\\', "/")
            .contains("System32/OpenSSH/ssh.exe"));

        let unix_res = resolve_ssh_program_platform(
            false,
            None,
            None,
            None,
            |_| false,
        )
        .expect("unix always returns ssh");
        assert_eq!(unix_res, "ssh");
    }
}
