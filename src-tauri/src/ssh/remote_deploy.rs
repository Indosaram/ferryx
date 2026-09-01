//! Remote resident-daemon deployment command construction and execution.
//!
//! The builders in this module are intentionally pure. The blocking execution
//! helpers are explicit about their contract so async callers can dispatch them
//! through `crate::ipc::run_blocking`.

use super::exec::{validate_host_argv_fields, SshArgvError};
use super::SshHost;
use std::io::{self, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub const UPLOAD_CHUNK_SIZE: usize = 256 * 1024;
pub const READINESS_TIMEOUT: Duration = Duration::from_secs(90);
pub const READINESS_INTERVAL: Duration = Duration::from_millis(500);
pub const LIVENESS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemotePlatform {
    Macos,
    Linux,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    pub platform: RemotePlatform,
    pub arch: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteDeployError {
    #[error(transparent)]
    UnsafeHost(#[from] SshArgvError),
    #[error("remote deploy field '{field}' must not be empty or start with '-': '{value}'")]
    UnsafePath { field: &'static str, value: String },
    #[error("failed to execute ssh: {0}")]
    Io(#[from] io::Error),
    #[error("remote command failed with status {0}")]
    RemoteCommand(ExitStatus),
    #[error("remote daemon did not become ready within 90 seconds")]
    ReadinessTimeout,
}

/// POSIX detection is attempted first. If ssh reports that `uname` is absent,
/// callers can retry with [`windows_detect_argv`].
pub fn posix_detect_argv(host: &SshHost) -> Result<Vec<String>, RemoteDeployError> {
    ssh_argv(host, "uname -s -m")
}

pub fn windows_detect_argv(host: &SshHost) -> Result<Vec<String>, RemoteDeployError> {
    ssh_argv(
        host,
        "powershell -NoProfile -NonInteractive -Command \"Write-Output ($env:OS + ' ' + $env:PROCESSOR_ARCHITECTURE)\"",
    )
}

pub fn parse_detect_output(output: &str) -> Option<RemoteTarget> {
    let mut fields = output.split_whitespace();
    let os = fields.next()?;
    let arch = fields.next()?.to_string();
    let platform = match os.to_ascii_lowercase().as_str() {
        "darwin" => RemotePlatform::Macos,
        "linux" => RemotePlatform::Linux,
        "windows_nt" => RemotePlatform::Windows,
        _ => return None,
    };
    Some(RemoteTarget { platform, arch })
}

pub fn install_argv(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<Vec<String>, RemoteDeployError> {
    validate_component("version", version)?;
    ssh_argv(host, &install_command(platform, version))
}

pub fn launch_argv(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<Vec<String>, RemoteDeployError> {
    validate_component("version", version)?;
    ssh_argv(host, &launch_command(platform, version))
}

pub fn readiness_argv(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<Vec<String>, RemoteDeployError> {
    validate_component("version", version)?;
    ssh_argv(host, &readiness_command(platform, version))
}

pub fn resident_probe_argv(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<Vec<String>, RemoteDeployError> {
    validate_component("version", version)?;
    ssh_argv(host, &resident_probe_command(platform, version))
}

pub fn liveness_argv(
    host: &SshHost,
    platform: RemotePlatform,
    pid: u32,
) -> Result<Vec<String>, RemoteDeployError> {
    ssh_argv(host, &liveness_command(platform, pid))
}

pub fn gc_argv(
    host: &SshHost,
    platform: RemotePlatform,
    current: &str,
    previous: Option<&str>,
) -> Result<Vec<String>, RemoteDeployError> {
    validate_component("current version", current)?;
    if let Some(previous) = previous {
        validate_component("previous version", previous)?;
    }
    ssh_argv(host, &gc_command(platform, current, previous))
}

pub fn install_command(platform: RemotePlatform, version: &str) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => {
            let dir = posix_version_dir(version);
            let bin = format!("{dir}/ferryx");
            format!(
                "mkdir -p {} && cat > {} && chmod 755 {}",
                sh_quote(&dir),
                sh_quote(&bin),
                sh_quote(&bin)
            )
        }
        RemotePlatform::Windows => {
            let dir = windows_version_dir(version);
            let bin = format!(r"{}\ferryx.exe", dir);
            format!(
                "powershell -NoProfile -NonInteractive -Command \"$dir={}; New-Item -ItemType Directory -Force -Path $dir | Out-Null; $out=[IO.File]::Open({},[IO.FileMode]::Create,[IO.FileAccess]::Write); try {{ [Console]::OpenStandardInput().CopyTo($out) }} finally {{ $out.Dispose() }}\"",
                ps_quote(&dir),
                ps_quote(&bin)
            )
        }
    }
}

pub fn launch_command(platform: RemotePlatform, version: &str) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => {
            let dir = posix_version_dir(version);
            let bin = format!("{dir}/ferryx");
            format!(
                "rm -f {ready}; nohup setsid {bin} --daemon >> {log} 2>&1 < /dev/null & echo $! > {pid}",
                ready = sh_quote(&format!("{dir}/ready")),
                bin = sh_quote(&bin),
                log = sh_quote(&format!("{dir}/daemon.log")),
                pid = sh_quote(&format!("{dir}/daemon.pid")),
            )
        }
        RemotePlatform::Windows => {
            let dir = windows_version_dir(version);
            let bin = format!(r"{}\ferryx.exe", dir);
            format!(
                "powershell -NoProfile -NonInteractive -Command \"$p=Start-Process -WindowStyle Hidden -FilePath {} -ArgumentList '--daemon' -RedirectStandardOutput {} -RedirectStandardError {} -PassThru; Set-Content -NoNewline -Path {} -Value $p.Id; Write-Output $p.Id\"",
                ps_quote(&bin),
                ps_quote(&format!(r"{}\daemon.stdout.log", dir)),
                ps_quote(&format!(r"{}\daemon.stderr.log", dir)),
                ps_quote(&format!(r"{}\daemon.pid", dir)),
            )
        }
    }
}

pub fn readiness_command(platform: RemotePlatform, version: &str) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => {
            let ready = format!("{}/ready", posix_version_dir(version));
            // This endpoint is the remote equivalent of daemon::server::get_socket_path().
            format!(
                "test -S /tmp/rorca-$(id -u)/daemon.sock || test -f {}",
                sh_quote(&ready)
            )
        }
        RemotePlatform::Windows => {
            let ready = format!(r"{}\ready", windows_version_dir(version));
            // Mirrors client.rs resolution: daemon.port contains the loopback TCP port.
            format!(
                "powershell -NoProfile -NonInteractive -Command \"$pf=Join-Path $env:LOCALAPPDATA 'Ferryx\\runtime\\daemon.port'; if (Test-Path $pf) {{ try {{ $port=[int](Get-Content -Raw $pf); $c=[Net.Sockets.TcpClient]::new(); $c.Connect('127.0.0.1',$port); $c.Dispose(); exit 0 }} catch {{}} }}; if (Test-Path {}) {{ exit 0 }}; exit 1\"",
                ps_quote(&ready)
            )
        }
    }
}

pub fn resident_probe_command(platform: RemotePlatform, version: &str) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => {
            let dir = posix_version_dir(version);
            format!(
                "pid=$(cat {pid} 2>/dev/null) && kill -0 \"$pid\" 2>/dev/null && test -S /tmp/rorca-$(id -u)/daemon.sock",
                pid = sh_quote(&format!("{dir}/daemon.pid")),
            )
        }
        RemotePlatform::Windows => {
            let dir = windows_version_dir(version);
            format!(
                "powershell -NoProfile -NonInteractive -Command \"$pid=[int](Get-Content -Raw {} -ErrorAction Stop); if (-not (Get-Process -Id $pid -ErrorAction SilentlyContinue)) {{ exit 1 }}; $pf=Join-Path $env:LOCALAPPDATA 'Ferryx\\runtime\\daemon.port'; $port=[int](Get-Content -Raw $pf -ErrorAction Stop); $c=[Net.Sockets.TcpClient]::new(); try {{ $c.Connect('127.0.0.1',$port); exit 0 }} catch {{ exit 1 }} finally {{ $c.Dispose() }}\"",
                ps_quote(&format!(r"{}\daemon.pid", dir)),
            )
        }
    }
}

pub fn remote_binary_path(platform: RemotePlatform, version: &str) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => format!("{}/ferryx", posix_version_dir(version)),
        RemotePlatform::Windows => format!(r"{}\ferryx.exe", windows_version_dir(version)),
    }
}

pub fn liveness_command(platform: RemotePlatform, pid: u32) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => format!("kill -0 {pid}"),
        RemotePlatform::Windows => format!(
            "powershell -NoProfile -NonInteractive -Command \"if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}\""
        ),
    }
}

pub fn gc_command(platform: RemotePlatform, current: &str, previous: Option<&str>) -> String {
    match platform {
        RemotePlatform::Macos | RemotePlatform::Linux => {
            let current = format!("daemon-{current}");
            let previous = previous.map(|value| format!("daemon-{value}"));
            let mut command = format!(
                "for d in \"$HOME\"/.ferryx-remote/daemon-*; do [ -d \"$d\" ] || continue; [ -L \"$d\" ] && continue; n=${{d##*/}}; [ \"$n\" = {} ]",
                sh_quote(&current)
            );
            if let Some(previous) = previous {
                command.push_str(&format!(" || [ \"$n\" = {} ]", sh_quote(&previous)));
            }
            command.push_str(" || rm -rf -- \"$d\"; done");
            command
        }
        RemotePlatform::Windows => {
            let mut keep = vec![ps_quote(&format!("daemon-{current}"))];
            if let Some(previous) = previous {
                keep.push(ps_quote(&format!("daemon-{previous}")));
            }
            format!(
                "powershell -NoProfile -NonInteractive -Command \"$keep=@({}); $root=Join-Path $HOME '.ferryx-remote'; Get-ChildItem -LiteralPath $root -Directory -Filter 'daemon-*' -ErrorAction SilentlyContinue | Where-Object {{ -not $_.LinkType -and $keep -notcontains $_.Name }} | Remove-Item -Recurse -Force\"",
                keep.join(",")
            )
        }
    }
}

/// Streams a binary to the install command without buffering another full copy.
/// Async callers must invoke this through `crate::ipc::run_blocking`.
pub fn upload_binary_blocking(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
    binary: &[u8],
) -> Result<(), RemoteDeployError> {
    let argv = install_argv(host, platform, version)?;
    let mut child = command_from_argv(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    {
        let mut stdin = child.stdin.take().expect("piped ssh stdin");
        for chunk in binary.chunks(UPLOAD_CHUNK_SIZE) {
            stdin.write_all(chunk)?;
        }
    }
    let status = child.wait()?;
    status
        .success()
        .then_some(())
        .ok_or(RemoteDeployError::RemoteCommand(status))
}

pub fn run_remote_blocking(argv: &[String]) -> Result<bool, RemoteDeployError> {
    Ok(command_from_argv(argv).status()?.success())
}

/// Runs the platform-specific PID probe with a hard two-second process bound.
/// Async callers must invoke this through `crate::ipc::run_blocking`.
pub fn probe_resident_blocking(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<bool, RemoteDeployError> {
    let argv = resident_probe_argv(host, platform, version)?;
    run_bounded_status(&argv, LIVENESS_TIMEOUT)
}

pub fn probe_liveness_blocking(
    host: &SshHost,
    platform: RemotePlatform,
    pid: u32,
) -> Result<bool, RemoteDeployError> {
    let argv = liveness_argv(host, platform, pid)?;
    run_bounded_status(&argv, LIVENESS_TIMEOUT)
}

fn run_bounded_status(argv: &[String], timeout: Duration) -> Result<bool, RemoteDeployError> {
    let mut child = command_from_argv(argv)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Uploads the binary, launches it detached, then waits for its endpoint to
/// accept connections. Async callers must invoke this through `run_blocking`.
pub fn deploy_then_ready_blocking(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
    binary: &[u8],
) -> Result<(), RemoteDeployError> {
    upload_binary_blocking(host, platform, version, binary)?;
    let launch = launch_argv(host, platform, version)?;
    let status = command_from_argv(&launch).status()?;
    if !status.success() {
        return Err(RemoteDeployError::RemoteCommand(status));
    }
    wait_until_ready_blocking(host, platform, version)
}

pub fn wait_until_ready_blocking(
    host: &SshHost,
    platform: RemotePlatform,
    version: &str,
) -> Result<(), RemoteDeployError> {
    let argv = readiness_argv(host, platform, version)?;
    let deadline = Instant::now() + READINESS_TIMEOUT;
    loop {
        if run_remote_blocking(&argv)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(RemoteDeployError::ReadinessTimeout);
        }
        std::thread::sleep(READINESS_INTERVAL);
    }
}

fn ssh_argv(host: &SshHost, remote_command: &str) -> Result<Vec<String>, RemoteDeployError> {
    validate_host_argv_fields(host)?;
    let mut argv = vec!["ssh".to_string()];
    if let Some(port) = host.port {
        argv.extend(["-p".to_string(), port.to_string()]);
    }
    if let Some(identity) = &host.identity_file {
        argv.extend(["-i".to_string(), identity.clone()]);
    }
    if let Some(jump) = &host.jump_host {
        argv.extend(["-J".to_string(), jump.clone()]);
    }
    argv.push(host.target());
    argv.push(remote_command.to_string());
    Ok(argv)
}

fn command_from_argv(argv: &[String]) -> Command {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

fn validate_component(field: &'static str, value: &str) -> Result<(), RemoteDeployError> {
    if value.is_empty()
        || value.starts_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(RemoteDeployError::UnsafePath {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn posix_version_dir(version: &str) -> String {
    format!("$HOME/.ferryx-remote/daemon-{version}")
}

fn windows_version_dir(version: &str) -> String {
    format!(r"$HOME\.ferryx-remote\daemon-{version}")
}

fn sh_quote(value: &str) -> String {
    if let Some(home_relative) = value.strip_prefix("$HOME/") {
        format!("\"$HOME/{}\"", home_relative.replace('"', "\\\""))
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn ps_quote(value: &str) -> String {
    if let Some(home_relative) = value.strip_prefix(r"$HOME\") {
        format!("(Join-Path $HOME '{}')", home_relative.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::{SshAuthMethod, SshHostSource};

    fn host() -> SshHost {
        SshHost {
            id: "host".into(),
            label: "Remote".into(),
            hostname: "remote.example".into(),
            username: Some("deploy".into()),
            port: Some(2200),
            identity_file: Some("/keys/id_ed25519".into()),
            jump_host: None,
            source: SshHostSource::Manual,
            auth_method: SshAuthMethod::Key,
            disabled: None,
            repo_root: None,
            remote_continuity: crate::ssh::RemoteContinuity::Auto,
        }
    }

    fn remote(argv: Vec<String>) -> String {
        argv.last().expect("remote command").clone()
    }

    #[test]
    fn detect_commands_and_output_cover_all_platforms() {
        assert_eq!(remote(posix_detect_argv(&host()).unwrap()), "uname -s -m");
        assert!(remote(windows_detect_argv(&host()).unwrap()).contains("PROCESSOR_ARCHITECTURE"));
        assert_eq!(
            parse_detect_output("Darwin arm64\n").unwrap().platform,
            RemotePlatform::Macos
        );
        assert_eq!(
            parse_detect_output("Linux x86_64\n").unwrap().platform,
            RemotePlatform::Linux
        );
        assert_eq!(
            parse_detect_output("Windows_NT AMD64\r\n")
                .unwrap()
                .platform,
            RemotePlatform::Windows
        );
    }

    #[test]
    fn macos_and_linux_command_snapshots() {
        for platform in [RemotePlatform::Macos, RemotePlatform::Linux] {
            assert_eq!(
                remote(install_argv(&host(), platform, "1.2.3").unwrap()),
                "mkdir -p \"$HOME/.ferryx-remote/daemon-1.2.3\" && cat > \"$HOME/.ferryx-remote/daemon-1.2.3/ferryx\" && chmod 755 \"$HOME/.ferryx-remote/daemon-1.2.3/ferryx\""
            );
            assert!(remote(launch_argv(&host(), platform, "1.2.3").unwrap())
                .contains("nohup setsid \"$HOME/.ferryx-remote/daemon-1.2.3/ferryx\" --daemon"));
            assert_eq!(
                remote(readiness_argv(&host(), platform, "1.2.3").unwrap()),
                "test -S /tmp/rorca-$(id -u)/daemon.sock || test -f \"$HOME/.ferryx-remote/daemon-1.2.3/ready\""
            );
            assert_eq!(
                remote(liveness_argv(&host(), platform, 42).unwrap()),
                "kill -0 42"
            );
            let gc = remote(gc_argv(&host(), platform, "1.2.3", Some("1.2.2")).unwrap());
            assert!(gc.contains("'daemon-1.2.3'"));
            assert!(gc.contains("'daemon-1.2.2'"));
            assert!(gc.contains("[ -L \"$d\" ] && continue"));
            assert!(gc.ends_with("rm -rf -- \"$d\"; done"));
        }
    }

    #[test]
    fn windows_command_snapshots() {
        let install = remote(install_argv(&host(), RemotePlatform::Windows, "1.2.3").unwrap());
        assert!(install.contains("OpenStandardInput().CopyTo($out)"));
        assert!(install.contains("daemon-1.2.3\\ferryx.exe"));
        let launch = remote(launch_argv(&host(), RemotePlatform::Windows, "1.2.3").unwrap());
        assert!(launch.contains("Start-Process -WindowStyle Hidden"));
        assert!(launch.contains("-ArgumentList '--daemon'"));
        let ready = remote(readiness_argv(&host(), RemotePlatform::Windows, "1.2.3").unwrap());
        assert!(ready.contains("TcpClient"));
        assert!(ready.contains("daemon.port"));
        assert!(
            remote(liveness_argv(&host(), RemotePlatform::Windows, 42).unwrap())
                .contains("Get-Process -Id 42")
        );
        let gc = remote(gc_argv(&host(), RemotePlatform::Windows, "1.2.3", Some("1.2.2")).unwrap());
        assert!(gc.contains("$keep=@('daemon-1.2.3','daemon-1.2.2')"));
        assert!(gc.contains("-not $_.LinkType"));
        assert!(gc.contains("Remove-Item -Recurse -Force"));
    }

    #[test]
    fn ssh_argv_preserves_connection_options() {
        let argv = install_argv(&host(), RemotePlatform::Linux, "1").unwrap();
        assert_eq!(&argv[..7], ["ssh", "-p", "2200", "-i", "/keys/id_ed25519", "deploy@remote.example", "mkdir -p \"$HOME/.ferryx-remote/daemon-1\" && cat > \"$HOME/.ferryx-remote/daemon-1/ferryx\" && chmod 755 \"$HOME/.ferryx-remote/daemon-1/ferryx\""]);
    }

    #[test]
    fn rejects_leading_dash_injection_paths_and_hosts() {
        for platform in [
            RemotePlatform::Macos,
            RemotePlatform::Linux,
            RemotePlatform::Windows,
        ] {
            assert!(matches!(
                install_argv(&host(), platform, "--upload-command=evil"),
                Err(RemoteDeployError::UnsafePath { .. })
            ));
            assert!(matches!(
                gc_argv(&host(), platform, "1", Some("-rf")),
                Err(RemoteDeployError::UnsafePath { .. })
            ));
        }
        let mut unsafe_host = host();
        unsafe_host.hostname = "-oProxyCommand=evil".into();
        assert!(matches!(
            launch_argv(&unsafe_host, RemotePlatform::Linux, "1"),
            Err(RemoteDeployError::UnsafeHost(_))
        ));
    }

    #[test]
    fn rejects_shell_metacharacters_in_versions() {
        for value in ["1'; touch /tmp/pwn; echo '", "1;rm", "1/../../tmp"] {
            assert!(matches!(
                install_argv(&host(), RemotePlatform::Linux, value),
                Err(RemoteDeployError::UnsafePath { .. })
            ));
            assert!(matches!(
                install_argv(&host(), RemotePlatform::Windows, value),
                Err(RemoteDeployError::UnsafePath { .. })
            ));
        }
    }
}
