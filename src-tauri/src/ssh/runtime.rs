use super::{direct, SshHost};
use crate::ipc::{IpcError, IpcErrorCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemotePlatform {
    Posix,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteExecutor {
    Sh,
    Powershell,
    Pwsh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEnvironment {
    pub platform: RemotePlatform,
    pub executor: RemoteExecutor,
    pub version: String,
    pub home: String,
    pub temp: String,
    pub git: bool,
    /// The remote `$XDG_RUNTIME_DIR`, reported only when logind lingers the user: a short,
    /// per-user 0700 directory for the helper socket when the home path is too long for
    /// `sun_path`. Without linger logind removes it at logout, taking the socket with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_dir: Option<String>,
    /// Operating system the probe reported (`uname -s`, or `Windows`). Empty when unknown,
    /// for example in an environment serialized by an older build.
    #[serde(default)]
    pub os: String,
    /// CPU architecture the probe reported (`uname -m`, or the native Windows
    /// `PROCESSOR_ARCHITECTURE`). Empty when unknown; helper selection then fails.
    #[serde(default)]
    pub arch: String,
}

pub fn error(code: IpcErrorCode, stage: &str, message: &str) -> IpcError {
    IpcError::new(code, message).with_details(serde_json::json!({"stage": stage}))
}

/// Whether OpenSSH reported the server as new: no key of the presented type is known for it in any
/// known_hosts file it consulted ("No <type> host key is known for <host> ...").
pub fn host_key_is_new(stderr: &str) -> bool {
    stderr
        .to_ascii_lowercase()
        .lines()
        .any(|line| line.contains("no ") && line.contains(" host key is known for "))
}

/// Classifies OpenSSH stderr that reports a host-key verification failure. Only OpenSSH's explicit
/// "No <type> host key is known" line is an unknown key the user may trust. A bare "Host key
/// verification failed" is also what a changed key prints under `LogLevel FATAL` and what an
/// untrusted ProxyJump hop prints, so it is reported like a changed key and never offered trust.
pub fn host_key_failure(stderr: &str) -> Option<IpcErrorCode> {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("remote host identification has changed")
        || (lower.contains("host key for ") && lower.contains(" has changed"))
    {
        return Some(IpcErrorCode::HostKeyChanged);
    }
    if host_key_is_new(stderr) {
        return Some(IpcErrorCode::HostKeyUnknown);
    }
    if lower.contains("host key verification failed") {
        return Some(IpcErrorCode::HostKeyChanged);
    }
    None
}

pub fn powershell_data(value: &str) -> String {
    format!(
        "([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{}')))",
        STANDARD.encode(value.as_bytes())
    )
}

/// Maps the Windows OpenSSH `DefaultShell` registry value (HKLM\SOFTWARE\OpenSSH)
/// to a supported executor so SSH terminals open the same shell the host configured
/// for interactive logins. Returns `None` for cmd.exe and anything else, leaving the
/// guaranteed-available executor probe fallback in charge.
pub fn executor_from_default_shell(value: &str) -> Option<RemoteExecutor> {
    let trimmed = value.trim().trim_matches('"');
    let program = trimmed.rsplit(['\\', '/']).next()?;
    match program.to_ascii_lowercase().as_str() {
        "pwsh.exe" | "pwsh" => Some(RemoteExecutor::Pwsh),
        "powershell.exe" | "powershell" => Some(RemoteExecutor::Powershell),
        _ => None,
    }
}

impl RemoteExecutor {
    pub const fn program(self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Powershell => "powershell.exe",
            Self::Pwsh => "pwsh",
        }
    }

    pub fn command(self, script: &str) -> String {
        self.command_mode(script, false)
    }

    pub fn command_mode(self, script: &str, interactive: bool) -> String {
        match self {
            Self::Sh => format!("sh -c {}", direct::quote_posix(script)),
            Self::Powershell | Self::Pwsh => {
                let script = format!(
                    "$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; \
                     [Console]::OutputEncoding=New-Object Text.UTF8Encoding($false); \
                     try {{ {script} }} catch {{ [Console]::Error.Write($_.Exception.Message); exit 1 }}"
                );
                let script = if interactive {
                    format!("& {{ {script} }}")
                } else {
                    script
                };
                let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
                format!(
                    "{} -NoLogo {} -EncodedCommand {}",
                    self.program(),
                    if interactive {
                        "-NoExit"
                    } else {
                        "-NoProfile -NonInteractive"
                    },
                    STANDARD.encode(bytes)
                )
            }
        }
    }
}

impl RemotePlatform {
    pub fn validate_path(self, path: &str) -> Result<(), IpcError> {
        let absolute = match self {
            Self::Posix => path.starts_with('/'),
            Self::Windows => {
                let b = path.as_bytes();
                let drive = b.len() >= 3
                    && b[0].is_ascii_alphabetic()
                    && b[1] == b':'
                    && matches!(b[2], b'\\' | b'/');
                let unc = path.starts_with("\\\\")
                    && !path.starts_with("\\\\?\\")
                    && !path.starts_with("\\\\.\\")
                    && path[2..].split('\\').filter(|s| !s.is_empty()).count() >= 2;
                drive || unc
            }
        };
        if !absolute || path.len() > 4096 || path.chars().any(char::is_control) {
            return Err(error(
                IpcErrorCode::InvalidPath,
                "directory",
                "Remote path must be absolute in the detected remote filesystem",
            ));
        }
        Ok(())
    }
}

pub fn parse_fields<'a>(
    bytes: &'a [u8],
    marker: &str,
    count: usize,
) -> Result<Vec<&'a str>, IpcError> {
    let prefix = format!("{marker}\0");
    let start = bytes
        .windows(prefix.len())
        .position(|part| part == prefix.as_bytes())
        .ok_or_else(|| {
            error(
                IpcErrorCode::ParseError,
                "response",
                "Missing SSH response frame",
            )
        })?;
    let fields = bytes[start + prefix.len()..]
        .split(|b| *b == 0)
        .collect::<Vec<_>>();
    if fields.len() != count + 1 || fields.last() != Some(&b"".as_slice()) {
        return Err(error(
            IpcErrorCode::ParseError,
            "response",
            "Malformed SSH response frame",
        ));
    }
    fields[..count]
        .iter()
        .map(|field| {
            std::str::from_utf8(field).map_err(|_| {
                error(
                    IpcErrorCode::ParseError,
                    "response",
                    "SSH response is not UTF-8",
                )
            })
        })
        .collect()
}

pub async fn detect(host: &SshHost) -> Result<RemoteEnvironment, IpcError> {
    detect_for(host, None).await
}

pub async fn detect_for(
    host: &SshHost,
    platform: Option<RemotePlatform>,
) -> Result<RemoteEnvironment, IpcError> {
    detect_with(host, platform, |executor, script, timeout| async move {
        let plan = direct::ssh_plan(host, executor.command(&script), false)?;
        direct::bounded_output(&plan, timeout).await.map_err(|mut err| {
            let details = err.details.get_or_insert_with(|| serde_json::json!({}));
            details["operation"] = "environment_detection_probe".into();
            details["executor"] = executor.program().into();
            err
        })
    })
    .await
}

async fn detect_with<F, Fut>(
    host: &SshHost,
    platform: Option<RemotePlatform>,
    mut run: F,
) -> Result<RemoteEnvironment, IpcError>
where
    F: FnMut(RemoteExecutor, String, Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, IpcError>>,
{
    let credential_generation = super::password::generation(host)?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let marker = format!("FERRYX_ENV_V1_{nonce}");
    let probe_script = |executor: RemoteExecutor| -> String {
        match executor {
            RemoteExecutor::Sh => format!(
                "os=$(uname -s) || exit; case \"$os\" in Linux|Darwin|FreeBSD|OpenBSD|NetBSD) ;; *) exit 2;; esac; \
                 arch=$(uname -m) || exit; \
                 {agent_dir}g=0; command -v git >/dev/null 2>&1 && g=1; \
                 rd=; [ -n \"${{XDG_RUNTIME_DIR:-}}\" ] && [ \"$(loginctl show-user \"$(id -u)\" -p Linger --value 2>/dev/null)\" = yes ] && rd=$XDG_RUNTIME_DIR; \
                 printf '{marker}\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000' \
                 posix \"$os\" \"$HOME\" \"${{TMPDIR:-/tmp}}\" \"$g\" \"$arch\" \"$rd\"",
                agent_dir = direct::REMOTE_AGENT_DIR_PREFLIGHT,
            ),
            RemoteExecutor::Powershell | RemoteExecutor::Pwsh => format!(
                "if ([Environment]::OSVersion.Platform -ne 'Win32NT') {{ exit 2 }}; \
                 $g='0'; if (Get-Command git -ErrorAction SilentlyContinue) {{ $g='1' }}; \
                 $ds=''; $k=Get-ItemProperty 'HKLM:\\SOFTWARE\\OpenSSH' -ErrorAction SilentlyContinue; \
                 if ($k -and $k.DefaultShell) {{ $ds=[string]$k.DefaultShell }}; \
                 $a=$env:PROCESSOR_ARCHITEW6432; if (-not $a) {{ $a=$env:PROCESSOR_ARCHITECTURE }}; \
                 [Console]::Write(('{marker}','windows',$PSVersionTable.PSVersion.ToString(),$HOME,[IO.Path]::GetTempPath(),$g,$ds,[string]$a,'' -join [char]0))"
            ),
        }
    };
    let executors: &[RemoteExecutor] = match platform {
        Some(RemotePlatform::Windows) => &[RemoteExecutor::Powershell, RemoteExecutor::Pwsh],
        Some(RemotePlatform::Posix) => &[RemoteExecutor::Sh],
        None => &[RemoteExecutor::Powershell, RemoteExecutor::Pwsh, RemoteExecutor::Sh],
    };
    let mut last_error: Option<IpcError> = None;
    let mut attempts = Vec::new();

    for (index, &executor) in executors.iter().enumerate() {
        let script = probe_script(executor);
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            if let Some(err) = last_error {
                return Err(err);
            }
            return Err(error(
                IpcErrorCode::IoError,
                "environment",
                "SSH environment detection timed out",
            ));
        }

        // Divide the remaining deadline among compatible executors
        // so that a hanging/non-responsive executor does not starve subsequent executors.
        let probes_left = (executors.len() - index) as u32;
        let step_timeout = remaining / probes_left;

        match run(executor, script, step_timeout).await {
            Ok(output) => {
                let fields = parse_fields(
                    &output,
                    &marker,
                    7,
                )?;
                let platform = match (executor, fields[0]) {
                    (RemoteExecutor::Sh, "posix") => RemotePlatform::Posix,
                    (RemoteExecutor::Powershell | RemoteExecutor::Pwsh, "windows") => {
                        RemotePlatform::Windows
                    }
                    _ => {
                        return Err(error(
                            IpcErrorCode::ParseError,
                            "environment",
                            "SSH environment does not match its executor",
                        ))
                    }
                };
                platform.validate_path(fields[2])?;
                platform.validate_path(fields[3])?;
                let git = match fields[4] {
                    "0" => false,
                    "1" => true,
                    _ => {
                        return Err(error(
                            IpcErrorCode::ParseError,
                            "environment",
                            "Invalid Git capability response",
                        ))
                    }
                };
                let (os, arch) = match platform {
                    RemotePlatform::Posix => (fields[1], fields[5]),
                    RemotePlatform::Windows => ("Windows", fields[6]),
                };
                // Refuse an OS without a helper here, so a connection test reports it instead
                // of passing and failing later at helper selection.
                super::helper_assets::require_helper_os(os)?;
                if arch.trim().is_empty() {
                    return Err(error(
                        IpcErrorCode::ParseError,
                        "environment",
                        "SSH environment probe did not report the CPU architecture",
                    ));
                }
                let mut environment = RemoteEnvironment {
                    platform,
                    executor,
                    version: fields[1].into(),
                    home: fields[2].into(),
                    temp: fields[3].into(),
                    git,
                    // Only an absolute, control-free POSIX path is usable as a socket root.
                    runtime_dir: (platform == RemotePlatform::Posix
                        && fields[6].starts_with('/')
                        && platform.validate_path(fields[6]).is_ok())
                    .then(|| fields[6].trim_end_matches('/').to_string())
                    .filter(|dir| !dir.is_empty()),
                    os: os.into(),
                    arch: arch.into(),
                };

                // Windows terminals must open the shell the host configured via the
                // OpenSSH DefaultShell registry value (mirrors the POSIX branch, which
                // execs the account login shell). When the probed executor differs from
                // the host decision, re-probe under it to validate it runs and report
                // its true engine version; on any failure fall back to the probe result.
                if platform == RemotePlatform::Windows {
                    if let Some(preferred) = executor_from_default_shell(fields[5]) {
                        if preferred != executor {
                            let remaining =
                                deadline.saturating_duration_since(tokio::time::Instant::now());
                            if !remaining.is_zero() {
                                let script2 = probe_script(preferred);
                                if let Ok(output2) = run(preferred, script2, remaining).await {
                                    environment = parse_fields(&output2, &marker, 7)
                                        .ok()
                                        .filter(|fields| {
                                            fields[0] == "windows" && !fields[6].trim().is_empty()
                                        })
                                        .and_then(|fields| {
                                            RemotePlatform::Windows
                                                .validate_path(fields[2])
                                                .ok()?;
                                            RemotePlatform::Windows
                                                .validate_path(fields[3])
                                                .ok()?;
                                            let git = match fields[4] {
                                                "0" => false,
                                                "1" => true,
                                                _ => return None,
                                            };
                                            Some(RemoteEnvironment {
                                                platform: RemotePlatform::Windows,
                                                executor: preferred,
                                                version: fields[1].into(),
                                                home: fields[2].into(),
                                                temp: fields[3].into(),
                                                git,
                                                runtime_dir: None,
                                                os: "Windows".into(),
                                                arch: fields[6].into(),
                                            })
                                        })
                                        .unwrap_or(environment);
                                }
                            }
                        }
                    }
                }

                return Ok(environment);
            }
            Err(mut err) => {
                let exit_code = err
                    .details
                    .as_ref()
                    .and_then(|v| v.get("exitCode"))
                    .and_then(|v| v.as_i64());

                // Exit code 255 indicates a fatal SSH transport failure (e.g. connection refused,
                // host key verification failure, authentication failure). In that case, subsequent
                // executor probes will also fail identically, so fail immediately.
                if exit_code == Some(255) {
                    let stderr = err
                        .details
                        .as_ref()
                        .and_then(|v| v.get("stderr"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    let host_key = host_key_failure(stderr);
                    let authentication_failed = host_key.is_none()
                        && host.auth_method == super::SshAuthMethod::Password
                        && stderr.contains("Permission denied");
                    if authentication_failed {
                        if let Some(generation) = credential_generation.as_deref() {
                            super::password::clear_generation(host, generation)?;
                        }
                    }
                    let stage = if host_key.is_some() {
                        "host_key"
                    } else if authentication_failed {
                        "authentication"
                    } else {
                        "environment"
                    };
                    if let Some(code) = host_key {
                        err.code = code;
                    }
                    if let Some(details) = err.details.as_mut() {
                        details["stage"] = stage.into();
                        details["executor"] = executor.program().into();
                    }
                    return Err(err);
                }

                // For non-255 errors (e.g. exit code 127 command not found, exit code 2 not supported,
                // or individual step timeout), record error and fall through to next executor.
                if let Some(details) = err.details.as_mut() {
                    details["stage"] = "environment".into();
                    details["executor"] = executor.program().into();
                }
                attempts.push(serde_json::json!({
                    "executor": executor.program(),
                    "message": err.message,
                    "details": err.details,
                }));
                // A failed fallback must not hide a preceding timeout.
                if last_error.is_none() || exit_code.is_none() {
                    last_error = Some(err);
                }
                if let Some(selected) = last_error.as_mut() {
                    let details = selected.details.get_or_insert_with(|| serde_json::json!({}));
                    details["attempts"] = serde_json::json!(attempts);
                }
            }
        }
    }

    if let Some(err) = last_error {
        return Err(err);
    }
    Err(error(
        IpcErrorCode::Unsupported,
        "environment",
        "SSH connected, but no supported POSIX or Windows executor was found",
    ))
}

pub const POWERSHELL_GIT: &str = r#"
function Invoke-FerryxGit([string[]]$GitArgs) {
    $p=New-Object Diagnostics.Process
    $p.StartInfo.FileName='git'
    $p.StartInfo.UseShellExecute=$false
    $p.StartInfo.CreateNoWindow=$true
    $p.StartInfo.RedirectStandardOutput=$true
    $p.StartInfo.RedirectStandardError=$true
    $p.StartInfo.StandardOutputEncoding=New-Object Text.UTF8Encoding($false)
    $p.StartInfo.StandardErrorEncoding=New-Object Text.UTF8Encoding($false)
    $p.StartInfo.Arguments=($GitArgs | ForEach-Object { '"' + (($_ -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"' }) -join ' '
    try {
        [void]$p.Start()
        $o=$p.StandardOutput.ReadToEndAsync()
        $e=$p.StandardError.ReadToEndAsync()
        $p.WaitForExit()
        return @{ Code=$p.ExitCode; Output=$o.Result; Error=$e.Result }
    } finally { $p.Dispose() }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_host() -> SshHost {
        SshHost {
            id: "environment-probe-test".into(),
            label: "Probe".into(),
            hostname: "example.test".into(),
            username: None,
            port: None,
            identity_file: None,
            jump_host: None,
            source: super::super::SshHostSource::Manual,
            auth_method: super::super::SshAuthMethod::Agent,
            disabled: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn windows_probe_can_finish_after_four_seconds() {
        let environment = detect_with(
            &probe_host(),
            Some(RemotePlatform::Windows),
            |executor, script, budget| async move {
                assert_eq!(executor, RemoteExecutor::Powershell);
                let marker = script.split("FERRYX_ENV_V1_").nth(1).unwrap().split('\'').next().unwrap();
                tokio::time::timeout(budget, async {
                    tokio::time::sleep(Duration::from_millis(4300)).await;
                    format!("FERRYX_ENV_V1_{marker}\0windows\05.1\0C:\\Users\\test\0C:\\Temp\01\0\0AMD64\0").into_bytes()
                })
                .await
                .map_err(|_| error(IpcErrorCode::IoError, "transport", "probe deadline"))
            },
        )
        .await
        .unwrap();
        assert_eq!(environment.platform, RemotePlatform::Windows);
        assert!(environment.git);
        assert_eq!((environment.os.as_str(), environment.arch.as_str()), ("Windows", "AMD64"));
    }

    fn sh_reply(script: &str, fields: &[&str]) -> Vec<u8> {
        let marker = script.split("FERRYX_ENV_V1_").nth(1).unwrap().split('\\').next().unwrap();
        // Login rc files may print anything before the frame.
        let mut out = b"Welcome!\nlast login: today\n\0stray\0FERRYX_ENV_V1_bogus\0".to_vec();
        for field in std::iter::once(format!("FERRYX_ENV_V1_{marker}").as_str()).chain(fields.iter().copied()) {
            out.extend_from_slice(field.as_bytes());
            out.push(0);
        }
        out
    }

    async fn detect_sh_reply(fields: &'static [&'static str]) -> Result<RemoteEnvironment, IpcError> {
        detect_with(&probe_host(), Some(RemotePlatform::Posix), |executor, script, _| {
            assert_eq!(executor, RemoteExecutor::Sh);
            assert!(script.contains("uname -m"));
            std::future::ready(Ok(sh_reply(&script, fields)))
        })
        .await
    }

    #[tokio::test]
    async fn posix_probe_reports_os_and_arch_through_rc_file_noise() {
        let environment =
            detect_sh_reply(&["posix", "Linux", "/home/u", "/tmp", "1", "aarch64", ""]).await.unwrap();
        assert_eq!((environment.os.as_str(), environment.arch.as_str()), ("Linux", "aarch64"));
        assert_eq!(environment.version, "Linux");
        assert_eq!(
            crate::ipc::ssh::resolve_remote_target_triple(&environment).unwrap(),
            "aarch64-unknown-linux-musl"
        );
    }

    #[tokio::test]
    async fn probe_reply_without_arch_errors_instead_of_defaulting() {
        // The pre-arch frame carries five fields; it must not parse as a valid environment.
        let failure = detect_sh_reply(&["posix", "Linux", "/home/u", "/tmp", "1"]).await.unwrap_err();
        assert_eq!(failure.code, IpcErrorCode::ParseError);
        let failure =
            detect_sh_reply(&["posix", "Linux", "/home/u", "/tmp", "1", "", ""]).await.unwrap_err();
        assert_eq!(failure.code, IpcErrorCode::ParseError);
    }

    #[tokio::test]
    async fn bsd_is_refused_at_detection_with_the_helper_reason() {
        let failure =
            detect_sh_reply(&["posix", "FreeBSD", "/home/u", "/tmp", "1", "amd64", ""]).await.unwrap_err();
        assert_eq!(failure.code, IpcErrorCode::Unsupported);
        let details = failure.details.unwrap();
        assert_eq!(details["reason"], "no_helper_for_os");
        assert_eq!(details["stage"], "environment");
        assert!(failure.message.contains("no remote helper for this OS"));
    }

    #[tokio::test]
    async fn posix_probe_reports_runtime_dir_only_when_absolute() {
        for (value, expected) in [
            ("/run/user/1000", Some("/run/user/1000")),
            ("/run/user/1000/", Some("/run/user/1000")),
            ("", None),
            ("relative/dir", None),
        ] {
            let environment = detect_with(
                &probe_host(),
                Some(RemotePlatform::Posix),
                |executor, script, _| {
                    assert_eq!(executor, RemoteExecutor::Sh);
                    assert!(script.contains("-p Linger --value"), "{script}");
                    let marker = script
                        .split("FERRYX_ENV_V1_")
                        .nth(1)
                        .unwrap()
                        .split('\\')
                        .next()
                        .unwrap()
                        .to_string();
                    std::future::ready(Ok(format!(
                        "FERRYX_ENV_V1_{marker}\0posix\0Linux\0/home/u\0/tmp\01\0x86_64\0{value}\0"
                    )
                    .into_bytes()))
                },
            )
            .await
            .unwrap();
            assert_eq!(environment.runtime_dir.as_deref(), expected, "{value:?}");
        }
    }

    #[tokio::test]
    async fn windows_failures_preserve_timeout_without_probing_sh() {
        let mut executors = Vec::new();
        let failure = detect_with(
            &probe_host(),
            Some(RemotePlatform::Windows),
            |executor, _, _| {
                executors.push(executor);
                std::future::ready(Err(match executor {
                    RemoteExecutor::Powershell => error(IpcErrorCode::IoError, "transport", "probe deadline"),
                    RemoteExecutor::Pwsh => error(IpcErrorCode::IoError, "execution", "missing executor")
                        .with_details(serde_json::json!({"exitCode": 1})),
                    RemoteExecutor::Sh => panic!("Windows must not probe sh"),
                }))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(executors, [RemoteExecutor::Powershell, RemoteExecutor::Pwsh]);
        let details = failure.details.unwrap();
        assert_eq!(details["executor"], "powershell.exe");
        assert_eq!(details["attempts"].as_array().unwrap().len(), 2);
        assert!(details.get("exitCode").is_none());
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires FERRYX_SSH_WORKTREE_PROBE_ROOT pointing to a Windows Git repository"]
    async fn live_windows_environment_and_worktree_listing() {
        let root = std::env::var("FERRYX_SSH_WORKTREE_PROBE_ROOT").unwrap();
        let environment = detect_with(
            &probe_host(),
            Some(RemotePlatform::Windows),
            |executor, script, budget| async move {
                let command = executor.command(&script);
                let mut args = command.split_whitespace();
                let plan = crate::terminal::shell::ShellCommandPlan {
                    program: args.next().unwrap().into(),
                    args: args.map(String::from).collect(),
                };
                direct::bounded_output(&plan, budget).await
            },
        )
        .await
        .unwrap();
        assert_eq!(environment.platform, RemotePlatform::Windows);
        let marker = "FERRYX_LIVE_WORKTREE_PROBE";
        let script = super::super::worktree::worktree_list_script(environment.platform, &root, marker);
        let command = environment.executor.command(&script);
        let mut args = command.split_whitespace();
        let plan = crate::terminal::shell::ShellCommandPlan {
            program: args.next().unwrap().into(),
            args: args.map(String::from).collect(),
        };
        let output = direct::bounded_output(&plan, Duration::from_secs(30)).await.unwrap();
        let fields = parse_fields(&output, marker, 1).unwrap();
        let worktrees = super::super::worktree::parse_worktree_porcelain(fields[0]);
        assert!(worktrees.iter().any(|worktree| worktree.path.replace('\\', "/") == root.replace('\\', "/")));
    }

    #[test]
    fn paths_are_interpreted_by_the_remote_platform() {
        for path in [r"C:\Users\sook", "C:/Users/sook", r"\\server\share\repo"] {
            assert!(RemotePlatform::Windows.validate_path(path).is_ok());
            assert!(RemotePlatform::Posix.validate_path(path).is_err());
        }
        for path in ["C:relative", r"\relative", r"\\?\C:\repo", "C:\\bad\0path"] {
            assert!(RemotePlatform::Windows.validate_path(path).is_err());
        }
        assert!(RemotePlatform::Posix.validate_path("/home/user").is_ok());
        assert!(RemotePlatform::Windows.validate_path("/home/user").is_err());
    }

    #[test]
    fn default_shell_registry_value_maps_to_matching_executor() {
        assert_eq!(
            executor_from_default_shell(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            Some(RemoteExecutor::Pwsh)
        );
        assert_eq!(
            executor_from_default_shell(r#""C:\Program Files\PowerShell\7\pwsh.exe""#),
            Some(RemoteExecutor::Pwsh)
        );
        assert_eq!(
            executor_from_default_shell(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
            ),
            Some(RemoteExecutor::Powershell)
        );
        assert_eq!(
            executor_from_default_shell(r"C:\Windows\System32\cmd.exe"),
            None
        );
        assert_eq!(executor_from_default_shell(""), None);
        assert_eq!(executor_from_default_shell("/bin/bash"), None);
    }

    #[test]
    fn frames_accept_banners_but_reject_truncation_and_wrong_requests() {
        assert_eq!(
            parse_fields(b"banner\nframe\0one\0two\0", "frame", 2).unwrap(),
            vec!["one", "two"]
        );
        for bytes in [
            b"frame\0one\0two".as_slice(),
            b"other\0one\0two\0",
            b"frame\0\xff\0two\0",
        ] {
            assert!(parse_fields(bytes, "frame", 2).is_err());
        }
    }

    #[test]
    fn fatal_transport_error_exit_code_255_is_identified() {
        let err = error(IpcErrorCode::IoError, "transport", "Connection refused")
            .with_details(serde_json::json!({ "exitCode": 255 }));
        let exit_code = err
            .details
            .as_ref()
            .and_then(|v| v.get("exitCode"))
            .and_then(|v| v.as_i64());
        assert_eq!(exit_code, Some(255));
    }

    #[test]
    fn non_fatal_timeout_has_no_exit_code_and_does_not_match_255() {
        let err = error(
            IpcErrorCode::IoError,
            "transport",
            "SSH operation timed out",
        );
        let exit_code = err
            .details
            .as_ref()
            .and_then(|v| v.get("exitCode"))
            .and_then(|v| v.as_i64());
        assert_ne!(exit_code, Some(255));
    }

    /// The POSIX detect probe precedes every pane, so it pre-creates the agent-forward directory
    /// before sshd binds the first pane's -R forward; the probe's fields are unchanged.
    #[cfg(unix)]
    #[tokio::test]
    async fn posix_probe_precreates_private_agent_dir() {
        use std::os::unix::fs::PermissionsExt as _;
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().to_path_buf();
        let environment = detect_with(&probe_host(), Some(RemotePlatform::Posix), |executor, script, _| {
            assert_eq!(executor, RemoteExecutor::Sh);
            let output = std::process::Command::new("sh")
                .args(["-c", &script])
                .env("HOME", &home)
                .output()
                .unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            std::future::ready(Ok(output.stdout))
        })
        .await
        .unwrap();
        assert_eq!(environment.platform, RemotePlatform::Posix);
        assert_eq!(environment.home, home.to_str().unwrap());
        let mode = std::fs::metadata(home.join(".ferryx/a")).unwrap().permissions().mode();
        assert_eq!(mode & 0o7777, 0o700);
    }

    #[tokio::test]
    async fn exit_code_255_aborts_detection_immediately_without_probing_other_executors() {
        let mut probed = Vec::new();
        let failure = detect_with(
            &probe_host(),
            None,
            |executor, _, _| {
                probed.push(executor);
                std::future::ready(Err(error(
                    IpcErrorCode::IoError,
                    "execution",
                    "fatal transport error",
                )
                .with_details(serde_json::json!({ "exitCode": 255 }))))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            probed,
            [RemoteExecutor::Powershell],
            "Exit code 255 must abort on the first executor rather than probing other shells"
        );
        let details = failure.details.unwrap();
        assert_eq!(details["exitCode"], 255);
    }

    #[tokio::test]
    async fn host_key_failures_report_their_own_code_and_stage() {
        let samples = [
            // Bare line: a changed key under LogLevel FATAL, or an untrusted ProxyJump hop.
            ("Host key verification failed.\r\n", IpcErrorCode::HostKeyChanged),
            (
                "No ED25519 host key is known for example.test and you have requested strict checking.\nHost key verification failed.\n",
                IpcErrorCode::HostKeyUnknown,
            ),
            (
                "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\nHost key for example.test has changed and you have requested strict checking.\nHost key verification failed.\n",
                IpcErrorCode::HostKeyChanged,
            ),
        ];
        for (stderr, code) in samples {
            let failure = detect_with(&probe_host(), Some(RemotePlatform::Posix), |_, _, _| {
                std::future::ready(Err(error(IpcErrorCode::IoError, "execution", "ssh failed")
                    .with_details(serde_json::json!({ "exitCode": 255, "stderr": stderr }))))
            })
            .await
            .unwrap_err();
            assert_eq!(failure.code, code, "{stderr}");
            assert_eq!(failure.details.unwrap()["stage"], "host_key", "{stderr}");
        }
    }

    #[tokio::test]
    async fn password_permission_denied_still_reports_authentication() {
        let host = SshHost {
            auth_method: super::super::SshAuthMethod::Password,
            ..probe_host()
        };
        let failure = detect_with(&host, Some(RemotePlatform::Posix), |_, _, _| {
            std::future::ready(Err(error(IpcErrorCode::IoError, "execution", "ssh failed")
                .with_details(serde_json::json!({
                    "exitCode": 255,
                    "stderr": "user@example.test: Permission denied (password)."
                }))))
        })
        .await
        .unwrap_err();
        assert_eq!(failure.code, IpcErrorCode::IoError);
        assert_eq!(failure.details.unwrap()["stage"], "authentication");
    }

    #[tokio::test]
    async fn transport_timeout_without_exit_code_falls_through_to_all_executors() {
        let mut probed = Vec::new();
        let _ = detect_with(
            &probe_host(),
            None,
            |executor, _, _| {
                probed.push(executor);
                std::future::ready(Err(error(
                    IpcErrorCode::IoError,
                    "transport",
                    "SSH operation timed out",
                )))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            probed,
            [
                RemoteExecutor::Powershell,
                RemoteExecutor::Pwsh,
                RemoteExecutor::Sh
            ],
            "Step timeout with no exit code falls through and tries all candidate executors"
        );
    }
}
