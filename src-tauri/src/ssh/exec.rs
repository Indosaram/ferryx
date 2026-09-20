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

pub fn probe_argv(host: &SshHost) -> Vec<String> {
    let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    let mut argv = vec![
        program,
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=2.5".to_string(),
    ];
    append_options(&mut argv, host);
    append_target(&mut argv, host);
    argv
}

pub fn interactive_argv(host: &SshHost) -> Vec<String> {
    let program = resolve_ssh_program().unwrap_or_else(|_| "ssh".to_string());
    let mut argv = vec![program, "-tt".to_string()];
    append_options(&mut argv, host);
    append_target(&mut argv, host);
    argv
}

fn append_options(argv: &mut Vec<String>, host: &SshHost) {
    if let Some(port) = host.port {
        argv.push("-p".to_string());
        argv.push(port.to_string());
    }
    if let Some(identity) = &host.identity_file {
        argv.push("-i".to_string());
        argv.push(identity.clone());
    }
    if let Some(jump) = &host.jump_host {
        argv.push("-J".to_string());
        argv.push(jump.clone());
    }
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
        }
    }

    #[test]
    fn probe_honors_saved_connection_options() {
        let argv = probe_argv(&host(
            Some(2200),
            Some("/keys/key with space"),
            Some("jump"),
            Some("user"),
        ));
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
        assert_eq!(
            probe_argv(&host(None, None, None, None)),
            vec![
                "ssh",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=2.5",
                "maho-win"
            ]
        );
        assert_eq!(
            probe_argv(&host(None, None, None, Some("sook")))
                .last()
                .map(String::as_str),
            Some("sook@maho-win")
        );
    }

    #[test]
    fn red_interactive_argv_flags() {
        let bare = interactive_argv(&host(None, None, None, None));
        assert_eq!(bare, vec!["ssh", "-tt", "maho-win"]);

        let full = interactive_argv(&host(
            Some(2200),
            Some("~/.ssh/id_ed25519"),
            Some("bastion"),
            Some("sook"),
        ));
        assert_eq!(
            full,
            vec![
                "ssh",
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
