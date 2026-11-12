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
}

pub fn error(code: IpcErrorCode, stage: &str, message: &str) -> IpcError {
    IpcError::new(code, message).with_details(serde_json::json!({"stage": stage}))
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

/// Exit code 255 is ssh's own failure code, but a remote command may also exit 255.
/// The two are indistinguishable by code alone, so the text decides: every ssh client
/// diagnostic is recognisable, while a remote shell's output never is. Treating a remote
/// exit 255 as a transport failure aborts detection with the remote stderr, which is what
/// made a healthy Windows host look like a broken connection.
fn is_ssh_transport_failure(err: &IpcError) -> bool {
    const DIAGNOSTICS: [&str; 15] = [
        "ssh: ",
        "kex_exchange_identification",
        "banner exchange",
        "Permission denied (",
        "Authentication failed",
        "Too many authentication failures",
        "Host key verification failed",
        "REMOTE HOST IDENTIFICATION HAS CHANGED",
        "Connection closed by ",
        "Connection reset by peer",
        "Connection refused",
        "Connection timed out",
        "Could not resolve hostname",
        "Network is unreachable",
        "No route to host",
    ];
    err.details
        .as_ref()
        .and_then(|value| value.get("stderr"))
        .and_then(|value| value.as_str())
        .is_some_and(|text| DIAGNOSTICS.iter().any(|marker| text.contains(marker)))
}

/// The handshake died before a session existed: the proxy hop answered but the target's
/// banner never arrived. Nothing on the remote side ran, so a retry cannot repeat an effect.
fn is_transient_handshake_timeout(err: &IpcError) -> bool {
    const TRANSIENT: [&str; 4] = [
        "banner exchange",
        "kex_exchange_identification",
        "Connection timed out",
        "Operation timed out",
    ];
    err.details
        .as_ref()
        .and_then(|value| value.get("stderr"))
        .and_then(|value| value.as_str())
        .is_some_and(|text| TRANSIENT.iter().any(|marker| text.contains(marker)))
}

/// How long a whole detection may take. An unknown platform has to pay for a Windows shell
/// start (measured 6.7-12.0 s) plus a POSIX fallback, and a Windows probe pays for the same
/// slow start. A jump host puts a second handshake in front of every probe, and the connect
/// timeout alone already allows 10 s there (see `direct::ssh_plan`), so the deadline has to
/// stay above it and still leave room for the probe command.
fn detection_budget(platform: Option<RemotePlatform>, jump_host: bool) -> Duration {
    let secs = match (platform, jump_host) {
        (Some(RemotePlatform::Posix), false) => 12,
        (Some(RemotePlatform::Posix), true) => 24,
        (_, false) => 30,
        (_, true) => 40,
    };
    Duration::from_secs(secs)
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
    let deadline =
        tokio::time::Instant::now() + detection_budget(platform, host.jump_host.is_some());
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let marker = format!("FERRYX_ENV_V1_{nonce}");
    let probe_script = |executor: RemoteExecutor| -> String {
        match executor {
            RemoteExecutor::Sh => format!(
                "os=$(uname -s) || exit; case \"$os\" in Linux|Darwin|FreeBSD|OpenBSD|NetBSD) ;; *) exit 2;; esac; \
                 g=0; command -v git >/dev/null 2>&1 && g=1; printf '{marker}\\000%s\\000%s\\000%s\\000%s\\000%s\\000' \
                 posix \"$os\" \"$HOME\" \"${{TMPDIR:-/tmp}}\" \"$g\""
            ),
            RemoteExecutor::Powershell | RemoteExecutor::Pwsh => format!(
                "if ([Environment]::OSVersion.Platform -ne 'Win32NT') {{ exit 2 }}; \
                 $g='0'; if (Get-Command git -ErrorAction SilentlyContinue) {{ $g='1' }}; \
                 $ds=''; $k=Get-ItemProperty 'HKLM:\\SOFTWARE\\OpenSSH' -ErrorAction SilentlyContinue; \
                 if ($k -and $k.DefaultShell) {{ $ds=[string]$k.DefaultShell }}; \
                 [Console]::Write(('{marker}','windows',$PSVersionTable.PSVersion.ToString(),$HOME,[IO.Path]::GetTempPath(),$g,$ds,'' -join [char]0))"
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
        let step_timeout = match executor {
            RemoteExecutor::Powershell | RemoteExecutor::Pwsh => remaining / 2,
            RemoteExecutor::Sh => remaining / probes_left,
        };

        // A jump host's second hop can lose the banner race once and win it immediately
        // after: measured 2 of 60 identical probes failing at 3.2 s, then 40 of 40 passing
        // with a longer `ConnectTimeout`. Retry only that transient shape - a handshake
        // that timed out before a session existed - so a genuinely unreachable host still
        // reports its real failure instead of paying for a second attempt.
        let mut outcome = run(executor, script.clone(), step_timeout).await;
        if let Err(err) = &outcome {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if !remaining.is_zero() && is_transient_handshake_timeout(err) {
                outcome = run(executor, script, remaining.min(step_timeout * 2)).await;
            }
        }

        match outcome {
            Ok(output) => {
                let fields = parse_fields(
                    &output,
                    &marker,
                    if executor == RemoteExecutor::Sh { 5 } else { 6 },
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
                let mut environment = RemoteEnvironment {
                    platform,
                    executor,
                    version: fields[1].into(),
                    home: fields[2].into(),
                    temp: fields[3].into(),
                    git,
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
                                    environment = parse_fields(&output2, &marker, 6)
                                        .ok()
                                        .filter(|fields| fields[0] == "windows")
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
                // executor probes will also fail identically, so fail immediately. A remote command
                // that merely exited 255 is not a transport failure: keep probing other executors.
                if exit_code == Some(255) && is_ssh_transport_failure(&err) {
                    let authentication_failed = host.auth_method == super::SshAuthMethod::Password
                        && err
                            .details
                            .as_ref()
                            .and_then(|v| v.get("stderr"))
                            .and_then(|v| v.as_str())
                            .is_some_and(|text| text.contains("Permission denied"));
                    if authentication_failed {
                        if let Some(generation) = credential_generation.as_deref() {
                            super::password::clear_generation(host, generation)?;
                        }
                    }
                    if let Some(details) = err.details.as_mut() {
                        details["stage"] = if authentication_failed {
                            "authentication"
                        } else {
                            "environment"
                        }
                        .into();
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

    #[test]
    fn a_jump_host_extends_the_detection_budget() {
        assert_eq!(
            detection_budget(Some(RemotePlatform::Posix), false),
            Duration::from_secs(12)
        );
        assert_eq!(
            detection_budget(Some(RemotePlatform::Posix), true),
            Duration::from_secs(24)
        );
        assert_eq!(detection_budget(None, true), Duration::from_secs(40));
    }

    #[tokio::test]
    async fn a_lost_banner_race_is_retried_once_then_reported() {
        let mut calls = 0;
        let environment = detect_with(
            &probe_host(),
            Some(RemotePlatform::Posix),
            |_, script, _| {
                calls += 1;
                // The POSIX probe writes the marker straight into `printf`, so the nonce ends at
                // the first backslash (`\000`), not at a quote like the PowerShell script does.
                let marker = script.split("FERRYX_ENV_V1_").nth(1).unwrap().split('\\').next().unwrap();
                std::future::ready(if calls == 1 {
                    Err(error(IpcErrorCode::IoError, "transport", "SSH command failed (exit 255): Connection timed out during banner exchange")
                        .with_details(serde_json::json!({
                            "exitCode": 255,
                            "stderr": "Connection timed out during banner exchange\nConnection to UNKNOWN port 65535 timed out\n",
                        })))
                } else {
                    Ok(format!("FERRYX_ENV_V1_{marker}\0posix\0Linux\0/home/u\0/tmp\01\0").into_bytes())
                })
            },
        )
        .await
        .unwrap();
        assert_eq!(environment.platform, RemotePlatform::Posix);
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn a_dead_host_is_not_retried() {
        let mut calls = 0;
        let failure = detect_with(
            &probe_host(),
            Some(RemotePlatform::Posix),
            |_, _, _| {
                calls += 1;
                std::future::ready(Err(error(IpcErrorCode::IoError, "transport", "SSH command failed (exit 255): Connection refused")
                    .with_details(serde_json::json!({"exitCode": 255, "stderr": "ssh: connect to host box port 22: Connection refused\n"}))))
            },
        )
        .await
        .unwrap_err();
        assert_eq!(calls, 1, "a refused connection must report its real failure at once");
        assert_eq!(failure.details.unwrap()["exitCode"], 255);
    }

    #[tokio::test(start_paused = true)]
    async fn windows_probe_can_finish_after_seven_seconds() {
        let environment = detect_with(
            &probe_host(),
            Some(RemotePlatform::Windows),
            |executor, script, budget| async move {
                assert_eq!(executor, RemoteExecutor::Powershell);
                let marker = script.split("FERRYX_ENV_V1_").nth(1).unwrap().split('\'').next().unwrap();
                tokio::time::timeout(budget, async {
                    tokio::time::sleep(Duration::from_millis(7000)).await;
                    format!("FERRYX_ENV_V1_{marker}\0windows\05.1\0C:\\Users\\test\0C:\\Temp\01\0\0").into_bytes()
                })
                .await
                .map_err(|_| error(IpcErrorCode::IoError, "transport", "probe deadline"))
            },
        )
        .await
        .unwrap();
        assert_eq!(environment.platform, RemotePlatform::Windows);
        assert!(environment.git);
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
                .with_details(serde_json::json!({
                    "exitCode": 255,
                    "stderr": "ssh: connect to host example.test port 22: Connection refused",
                }))))
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
        assert_eq!(details["stage"], "environment");
    }

    #[tokio::test]
    async fn remote_exit_code_255_falls_through_instead_of_aborting_detection() {
        // A Windows host whose DefaultShell is cmd.exe answers the POSIX probe with
        // "'sh' is not recognized"; cmd.exe returns 255 for that line because the probe
        // script contains `|`. That is a remote failure, not a transport failure.
        let mut probed = Vec::new();
        let failure = detect_with(
            &probe_host(),
            None,
            |executor, _, _| {
                probed.push(executor);
                std::future::ready(match executor {
                    RemoteExecutor::Powershell | RemoteExecutor::Pwsh => Err(error(
                        IpcErrorCode::IoError,
                        "execution",
                        "missing executor",
                    )
                    .with_details(serde_json::json!({ "exitCode": 1 }))),
                    RemoteExecutor::Sh => {
                        // Non-UTF-8 remote stderr, as a Korean cmd.exe emits.
                        Err(error(
                            IpcErrorCode::IoError,
                            "execution",
                            "Remote stderr contained non-UTF-8 data; raw bytes are available in details",
                        )
                        .with_details(serde_json::json!({ "exitCode": 255, "stderr": null })))
                    }
                })
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
            "a remote exit 255 must not be mistaken for a dead transport"
        );
        let details = failure.details.unwrap();
        assert_eq!(
            details["attempts"].as_array().unwrap().len(),
            3,
            "every executor attempt must be reported"
        );
    }

    #[tokio::test]
    async fn ssh_client_diagnostic_without_exit_code_is_not_transport_failure() {
        // The diagnostic alone is not enough: ssh exits 255 for it, and the caller keys on
        // the code, so a message-only match must not abort detection.
        let mut probed = Vec::new();
        let _ = detect_with(&probe_host(), None, |executor, _, _| {
            probed.push(executor);
            std::future::ready(Err(error(
                IpcErrorCode::IoError,
                "transport",
                "Connection refused",
            )))
        })
        .await
        .unwrap_err();
        assert_eq!(probed.len(), 3);
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
