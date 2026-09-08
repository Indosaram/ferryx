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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let marker = format!("FERRYX_ENV_V1_{nonce}");
    for executor in [
        RemoteExecutor::Powershell,
        RemoteExecutor::Pwsh,
        RemoteExecutor::Sh,
    ] {
        let script = match executor {
            RemoteExecutor::Sh => format!(
                "os=$(uname -s) || exit; case \"$os\" in Linux|Darwin|FreeBSD|OpenBSD|NetBSD) ;; *) exit 2;; esac; \
                 g=0; command -v git >/dev/null 2>&1 && g=1; printf '{marker}\\000%s\\000%s\\000%s\\000%s\\000%s\\000' \
                 posix \"$os\" \"$HOME\" \"${{TMPDIR:-/tmp}}\" \"$g\""
            ),
            RemoteExecutor::Powershell | RemoteExecutor::Pwsh => format!(
                "if ([Environment]::OSVersion.Platform -ne 'Win32NT') {{ exit 2 }}; \
                 $g='0'; if (Get-Command git -ErrorAction SilentlyContinue) {{ $g='1' }}; \
                 [Console]::Write(('{marker}','windows',$PSVersionTable.PSVersion.ToString(),$HOME,[IO.Path]::GetTempPath(),$g,'' -join [char]0))"
            ),
        };
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(error(
                IpcErrorCode::IoError,
                "environment",
                "SSH environment detection timed out",
            ));
        }
        let plan = direct::ssh_plan(host, executor.command(&script), false)?;
        match direct::bounded_output(&plan, remaining).await {
            Ok(output) => {
                let fields = parse_fields(&output, &marker, 5)?;
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
                return Ok(RemoteEnvironment {
                    platform,
                    executor,
                    version: fields[1].into(),
                    home: fields[2].into(),
                    temp: fields[3].into(),
                    git,
                });
            }
            Err(mut err) => {
                if err
                    .details
                    .as_ref()
                    .and_then(|v| v.get("exitCode"))
                    .and_then(|v| v.as_i64())
                    .is_none_or(|code| code == 255)
                {
                    if let Some(details) = err.details.as_mut() {
                        details["stage"] = "environment".into();
                        details["executor"] = executor.program().into();
                    }
                    return Err(err);
                }
            }
        }
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
}
