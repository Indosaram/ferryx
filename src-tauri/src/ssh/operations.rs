use super::runtime::{
    error, parse_fields, powershell_data, RemoteEnvironment, RemotePlatform, POWERSHELL_GIT,
};
use super::{direct, SshHost};
use crate::daemon::agent_extension::{extension_source_for, EXTENSION_AGENTS, EXTENSION_FILE_NAME};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::terminal::shell::ShellCommandPlan;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectProbe {
    pub repo_root: String,
    pub git_root: Option<String>,
    pub git_remote: Option<String>,
    pub git_common_dir: Option<String>,
    pub git_branch: Option<String>,
    pub git_head: Option<String>,
}

pub async fn probe(
    host: &SshHost,
    environment: &RemoteEnvironment,
    path: &str,
) -> Result<ProjectProbe, IpcError> {
    environment.platform.validate_path(path)?;
    let marker = format!("FERRYX_DIR_V2_{}", uuid::Uuid::new_v4().simple());
    let script = project_probe_script(environment.platform, path, &marker);
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    let output = direct::bounded_output(&plan, Duration::from_secs(12))
        .await
        .map_err(|mut err| {
            if let Some(details) = err.details.as_mut() {
                details["stage"] = "directory".into();
            }
            err
        })?;
    let fields = parse_fields(&output, &marker, 6)?;
    environment.platform.validate_path(fields[0])?;
    for path in [fields[1], fields[3]] {
        if !path.is_empty() {
            environment.platform.validate_path(path)?;
        }
    }
    let git_branch = (!fields[4].is_empty()).then(|| fields[4].trim().to_string());
    let git_head = (!fields[5].is_empty()).then(|| fields[5].trim().to_string());
    Ok(ProjectProbe {
        repo_root: fields[0].into(),
        git_root: (!fields[1].is_empty()).then(|| fields[1].into()),
        git_remote: crate::worktree::git::select_project_remote(fields[2]),
        git_common_dir: (!fields[3].is_empty()).then(|| fields[3].into()),
        git_branch,
        git_head,
    })
}

fn project_probe_script(platform: RemotePlatform, path: &str, marker: &str) -> String {
    match platform {
        RemotePlatform::Posix => format!(
            "cd {} || exit; root=$(pwd -P) || exit; \
             gitroot=$(git rev-parse --show-toplevel 2>/dev/null) || gitroot=; \
             remotes=$(git remote -v 2>/dev/null) || remotes=; \
             common=$(git rev-parse --git-common-dir 2>/dev/null) || common=; \
             if [ -n \"$common\" ]; then common=$(cd \"$common\" && pwd -P) || common=; fi; \
             branch=$(git branch --show-current 2>/dev/null) || branch=; \
             if [ -z \"$branch\" ]; then branch=$(git symbolic-ref --quiet --short HEAD 2>/dev/null) || branch=; fi; \
             head=$(git rev-parse --verify HEAD 2>/dev/null) || head=; \
             printf '{marker}\\000%s\\000%s\\000%s\\000%s\\000%s\\000%s\\000' \"$root\" \"$gitroot\" \"$remotes\" \"$common\" \"$branch\" \"$head\"",
            direct::quote_posix(path)
        ),
        RemotePlatform::Windows => format!(
            "{POWERSHELL_GIT}\n$p={}; $item=Get-Item -LiteralPath $p -Force; \
             if (!$item.PSIsContainer) {{ throw 'Remote path is not a directory' }}; \
             $root=$item.FullName; $gitroot=''; $remotes=''; $common=''; $branch=''; $head=''; \
             if (Get-Command git -ErrorAction SilentlyContinue) {{ \
                 $g=Invoke-FerryxGit @('-C',$root,'rev-parse','--show-toplevel'); \
                 if ($g.Code -eq 0) {{ $gitroot=$g.Output.TrimEnd([char[]]\"`r`n\"); \
                     $g=Invoke-FerryxGit @('-C',$root,'remote','-v'); \
                     if ($g.Code -eq 0) {{ $remotes=$g.Output.TrimEnd([char[]]\"`r`n\") }}; \
                     $g=Invoke-FerryxGit @('-C',$root,'rev-parse','--git-common-dir'); \
                     if ($g.Code -eq 0) {{ $c=$g.Output.TrimEnd([char[]]\"`r`n\"); \
                         if (![IO.Path]::IsPathRooted($c)) {{ $c=Join-Path $root $c }}; \
                         $common=(Get-Item -LiteralPath $c -Force).FullName \
                     }}; \
                     $g=Invoke-FerryxGit @('-C',$root,'branch','--show-current'); \
                     if ($g.Code -eq 0) {{ $branch=$g.Output.TrimEnd([char[]]\"`r`n\") }}; \
                     if (!$branch) {{ \
                         $g=Invoke-FerryxGit @('-C',$root,'symbolic-ref','--quiet','--short','HEAD'); \
                         if ($g.Code -eq 0) {{ $branch=$g.Output.TrimEnd([char[]]\"`r`n\") }} \
                     }}; \
                     $g=Invoke-FerryxGit @('-C',$root,'rev-parse','--verify','HEAD'); \
                     if ($g.Code -eq 0) {{ $head=$g.Output.TrimEnd([char[]]\"`r`n\") }} \
                 }} \
             }}; [Console]::Write(('{marker}',$root,$gitroot,$remotes,$common,$branch,$head,'' -join [char]0))",
            powershell_data(path)
        ),
    }
}

pub fn shell_plan(
    host: &SshHost,
    environment: &RemoteEnvironment,
    root: &str,
    session_id: &str,
    agent_socket: Option<&str>,
    state_endpoint: Option<&super::state_bridge::StateEndpoint>,
) -> Result<ShellCommandPlan, IpcError> {
    environment.platform.validate_path(root)?;
    match environment.platform {
        RemotePlatform::Posix => {
            let mut plan =
                direct::shell_plan_with_session(host, root, Some(session_id), agent_socket)?;
            if let Some(script) = plan.args.pop() {
                plan.args.push(environment.executor.command(&script));
            }
            Ok(plan)
        }
        RemotePlatform::Windows => {
            let state = match state_endpoint {
                Some(endpoint) => format!(
                    "$env:FERRYX_AGENT_STATE_PORT='{}'; $env:FERRYX_AGENT_STATE_TOKEN={};",
                    endpoint.port,
                    powershell_data(&endpoint.token)
                ),
                None => "$env:FERRYX_AGENT_STATE_PORT=$null; $env:FERRYX_AGENT_STATE_TOKEN=$null;"
                    .into(),
            };
            let script = format!(
                "{state} $env:FERRYX_AGENT_STATE_SOCKET=$null; $env:FERRYX_SESSION_ID={}; Set-Location -LiteralPath {}",
                powershell_data(session_id), powershell_data(root)
            );
            direct::ssh_plan(host, environment.executor.command_mode(&script, true), true)
        }
    }
}

pub fn upload_deadline_for_bytes(byte_count: usize) -> Duration {
    let base = 60u64;
    let additional = (byte_count as u64) / (200 * 1024);
    Duration::from_secs((base + additional).min(600))
}

pub fn posix_drop_upload_script(upload_id: &str, file_name: &str, marker: &str) -> String {
    let quoted_name = direct::quote_posix(file_name);
    format!(
        "set -e; \
         d=\"${{TMPDIR:-/tmp}}\"; d=\"${{d%/}}/ferryx-paste\"; mkdir -p \"$d\"; chmod 700 \"$d\"; \
         find \"$d\" -mindepth 1 -maxdepth 1 -mtime +1 -exec rm -rf {{}} + 2>/dev/null || true; \
         sub=\"$d/{upload_id}\"; mkdir -p \"$sub\"; chmod 700 \"$sub\"; \
         umask 077; target=\"$sub/\"{quoted_name}; \
         cat > \"$target\"; \
         printf '{marker}\\000%s\\000' \"$target\""
    )
}

pub fn windows_drop_upload_script(upload_id: &str, file_name: &str, marker: &str) -> String {
    let sub_id = powershell_data(upload_id);
    let name_data = powershell_data(file_name);
    format!(
        "$d=Join-Path ([IO.Path]::GetTempPath()) 'ferryx-paste'; \
         [void][IO.Directory]::CreateDirectory($d); \
         $acl=New-Object Security.AccessControl.DirectorySecurity; \
         $acl.SetAccessRuleProtection($true,$false); \
         $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
         $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'); \
         $acl.AddAccessRule($rule); Set-Acl -LiteralPath $d -AclObject $acl; \
         Get-ChildItem -LiteralPath $d | Where-Object {{ $_.LastWriteTimeUtc -lt [DateTime]::UtcNow.AddDays(-1) }} | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue; \
         $sub=Join-Path $d {sub_id}; \
         [void][IO.Directory]::CreateDirectory($sub); \
         $p=Join-Path $sub {name_data}; \
         $f=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None); \
         try {{ $f.Write($bytes,0,$bytes.Length) }} finally {{ $f.Dispose() }}; \
         [Console]::Write(('{marker}',$p,'' -join [char]0))"
    )
}

pub async fn upload_dropped(
    host: &SshHost,
    environment: &RemoteEnvironment,
    upload_id: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<String, IpcError> {
    crate::clipboard_image::validate_drop_file_name(file_name)?;
    let marker = format!("FERRYX_DROP_V1_{}", uuid::Uuid::new_v4().simple());
    let deadline = upload_deadline_for_bytes(bytes.len());

    let script = match environment.platform {
        RemotePlatform::Posix => posix_drop_upload_script(upload_id, file_name, &marker),
        RemotePlatform::Windows => windows_drop_upload_script(upload_id, file_name, &marker),
    };

    let plan = |command| direct::ssh_plan(host, command, false);
    let output = data_output_with(environment, &script, bytes, deadline, plan)
        .await
        .map_err(|mut err| {
            if let Some(details) = err.details.as_mut() {
                details["stage"] = "upload_dropped".into();
            }
            err
        })?;
    let fields = parse_fields(&output, &marker, 1)?;
    environment.platform.validate_path(fields[0])?;
    Ok(fields[0].into())
}

pub async fn upload(
    host: &SshHost,
    environment: &RemoteEnvironment,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<String, IpcError> {
    upload_with(environment, file_name, bytes, |command| {
        direct::ssh_plan(host, command, false)
    })
    .await
}

/// Same upload script over a caller-built SSH plan (manual terminal SSH keeps its own options).
pub(crate) async fn upload_with(
    environment: &RemoteEnvironment,
    file_name: &str,
    bytes: Vec<u8>,
    plan: impl FnOnce(String) -> Result<ShellCommandPlan, IpcError>,
) -> Result<String, IpcError> {
    let posix = direct::upload_temp_command(file_name)?;
    let marker = format!("FERRYX_UPLOAD_V1_{}", uuid::Uuid::new_v4().simple());
    let script = match environment.platform {
        RemotePlatform::Posix => format!(
            "p=$({posix}) || exit; printf '{marker}\\000%s\\000' \"$p\""
        ),
        RemotePlatform::Windows => format!(
            "$d=Join-Path ([IO.Path]::GetTempPath()) 'ferryx-paste'; \
             [void][IO.Directory]::CreateDirectory($d); \
             $acl=New-Object Security.AccessControl.DirectorySecurity; \
             $acl.SetAccessRuleProtection($true,$false); \
             $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'); \
             $acl.AddAccessRule($rule); Set-Acl -LiteralPath $d -AclObject $acl; \
             Get-ChildItem -LiteralPath $d -File | Where-Object {{ $_.LastWriteTimeUtc -lt [DateTime]::UtcNow.AddDays(-1) }} | Remove-Item -Force; \
             $p=Join-Path $d '{file_name}'; \
             $f=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None); \
             try {{ $f.Write($bytes,0,$bytes.Length) }} finally {{ $f.Dispose() }}; \
             [Console]::Write(('{marker}',$p,'' -join [char]0))"
        ),
    };
    let deadline = upload_deadline_for_bytes(bytes.len());
    let output = data_output_with(environment, &script, bytes, deadline, plan)
        .await
        .map_err(|mut err| {
            if let Some(details) = err.details.as_mut() {
                details["stage"] = "upload".into();
            }
            err
        })?;
    let fields = parse_fields(&output, &marker, 1)?;
    environment.platform.validate_path(fields[0])?;
    Ok(fields[0].into())
}

/// Installs the extension for one agent directory (`.omo`, `.pi` or `.omp`), whose own payload
/// arrives on stdin. A missing agent directory is left alone: Ferryx never creates one.
pub fn posix_install_script(dir: &str) -> String {
    format!(
        "set -e; b=\"$HOME/\"{dir}; \
         if [ ! -d \"$b\" ]; then cat > /dev/null; exit 0; fi; \
         d=\"$b/agent/extensions\"; mkdir -p \"$d\"; \
         stage=$(mktemp \"$d/.ferryx-agent-state.XXXXXX\"); \
         trap '[ -z \"$stage\" ] || rm -f \"$stage\"' EXIT; \
         cat > \"$stage\"; mv -f \"$stage\" \"$d/{EXTENSION_FILE_NAME}\"; stage=",
        dir = direct::quote_posix(dir)
    )
}

fn windows_install_script(dir: &str) -> String {
    format!(
        "$b=Join-Path $homeRoot {dir}; \
         if ([IO.Directory]::Exists($b)) {{ \
             $d=Join-Path $b 'agent/extensions'; [void][IO.Directory]::CreateDirectory($d); \
             $p=Join-Path $d '{EXTENSION_FILE_NAME}'; $tmp=$p+'.'+[Guid]::NewGuid().ToString()+'.tmp'; \
             try {{ [IO.File]::WriteAllBytes($tmp,$bytes); \
                 if ([IO.File]::Exists($p)) {{ [IO.File]::Replace($tmp,$p,[NullString]::Value) }} else {{ [IO.File]::Move($tmp,$p) }} \
             }} finally {{ if ([IO.File]::Exists($tmp)) {{ [IO.File]::Delete($tmp) }} }} \
         }}",
        dir = powershell_data(dir)
    )
}

/// Removes one agent directory's extension file when it is byte-for-byte one of the candidate
/// payloads, which arrive concatenated on stdin with their byte `lengths` in order. Prints one
/// line: `removed|kept_modified|absent <agent>`. Only the file is ever deleted.
pub fn posix_removal_script(dir: &str, agent: &str, lengths: &[usize]) -> String {
    let lengths = lengths
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "set -e; agent={agent}; t=\"$HOME/\"{dir}\"/agent/extensions/{EXTENSION_FILE_NAME}\"; \
         base=\"${{TMPDIR:-/tmp}}\"; base=\"${{base%/}}\"; c=; \
         all=$(mktemp \"$base/.ferryx-remove.XXXXXX\"); \
         trap 'rm -f \"$all\"; [ -z \"$c\" ] || rm -f \"$c\"' EXIT; cat > \"$all\"; \
         if [ -L \"$t\" ]; then printf 'kept_modified %s\\n' \"$agent\"; exit 0; fi; \
         if [ ! -e \"$t\" ]; then printf 'absent %s\\n' \"$agent\"; exit 0; fi; \
         if [ ! -f \"$t\" ]; then printf 'kept_modified %s\\n' \"$agent\"; exit 0; fi; \
         c=$(mktemp \"$base/.ferryx-candidate.XXXXXX\"); off=0; \
         for n in {lengths}; do \
             dd if=\"$all\" of=\"$c\" bs=1 skip=\"$off\" count=\"$n\" 2>/dev/null; \
             if cmp -s \"$t\" \"$c\"; then rm -f \"$t\"; printf 'removed %s\\n' \"$agent\"; exit 0; fi; \
             off=$((off + n)); \
         done; \
         printf 'kept_modified %s\\n' \"$agent\"",
        agent = direct::quote_posix(agent),
        dir = direct::quote_posix(dir),
    )
}

fn windows_removal_script(dir: &str, agent: &str, lengths: &[usize]) -> String {
    let lengths = lengths
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "$agent={agent}; $t=Join-Path (Join-Path $homeRoot {dir}) 'agent/extensions/{EXTENSION_FILE_NAME}'; \
         $item=Get-Item -LiteralPath $t -Force -ErrorAction SilentlyContinue; \
         if (!$item) {{ $state='absent' }} \
         elseif ($item.PSIsContainer -or $item.LinkType) {{ $state='kept_modified' }} \
         else {{ $file=[IO.File]::ReadAllBytes($t); $state='kept_modified'; $off=0; \
             foreach ($n in @({lengths})) {{ \
                 if ($file.Length -eq $n) {{ $same=$true; \
                     for ($i=0; $i -lt $n; $i++) {{ if ($file[$i] -ne $bytes[$off+$i]) {{ $same=$false; break }} }}; \
                     if ($same) {{ [IO.File]::Delete($t); $state='removed'; break }} \
                 }}; \
                 $off+=$n \
             }} \
         }}; \
         [Console]::Write($state+' '+$agent+\"`n\")",
        agent = powershell_data(agent),
        dir = powershell_data(dir),
    )
}

/// What a remote removal did, by agent id.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRemovalReport {
    pub removed: Vec<String>,
    /// Present but not a byte-exact Ferryx payload (user-modified, a directory or a symlink).
    pub kept_modified: Vec<String>,
    pub absent: Vec<String>,
}

impl RemoteRemovalReport {
    fn record(&mut self, agent: &str, output: &[u8]) -> Result<(), IpcError> {
        let text = String::from_utf8_lossy(output);
        let line = text.lines().map(str::trim).rfind(|line| !line.is_empty());
        let slot = match line.and_then(|line| line.split_once(' ')) {
            Some(("removed", reported)) if reported == agent => &mut self.removed,
            Some(("kept_modified", reported)) if reported == agent => &mut self.kept_modified,
            Some(("absent", reported)) if reported == agent => &mut self.absent,
            _ => {
                return Err(error(
                    IpcErrorCode::ParseError,
                    "integration_removal",
                    "The remote host returned an unexpected removal result",
                ))
            }
        };
        slot.push(agent.to_string());
        Ok(())
    }
}

fn agent_payload(agent: &str) -> Result<String, IpcError> {
    extension_source_for(agent).ok_or_else(|| {
        error(
            IpcErrorCode::InternalError,
            "integration",
            "The bundled extension no longer declares its agent identity",
        )
    })
}

/// Every payload Ferryx may have written, `agent`'s own first, concatenated, with their lengths.
fn removal_candidates(agent: &str) -> Result<(Vec<u8>, Vec<usize>), IpcError> {
    let mut order = vec![agent];
    order.extend(
        EXTENSION_AGENTS
            .iter()
            .map(|(_, other)| *other)
            .filter(|other| *other != agent),
    );
    let mut bytes = Vec::new();
    let mut lengths = Vec::new();
    for candidate in order {
        let payload = agent_payload(candidate)?;
        lengths.push(payload.len());
        bytes.extend_from_slice(payload.as_bytes());
    }
    Ok((bytes, lengths))
}

/// The agent-state forwarding a new SSH pane gets: none at all while the integration is off.
pub fn integration_forwarding<'a>(
    enabled: bool,
    agent_socket: Option<&'a str>,
    state_endpoint: Option<&'a super::state_bridge::StateEndpoint>,
) -> (
    Option<&'a str>,
    Option<&'a super::state_bridge::StateEndpoint>,
) {
    if enabled {
        (agent_socket, state_endpoint)
    } else {
        (None, None)
    }
}

async fn run_integration_script(
    host: &SshHost,
    environment: &RemoteEnvironment,
    script: &str,
    bytes: Vec<u8>,
    stage: &str,
) -> Result<Vec<u8>, IpcError> {
    environment.platform.validate_path(&environment.home)?;
    let script = match environment.platform {
        RemotePlatform::Posix => {
            format!("HOME={}; {script}", direct::quote_posix(&environment.home))
        }
        RemotePlatform::Windows => {
            format!("$homeRoot={}; {script}", powershell_data(&environment.home))
        }
    };
    data_output(host, environment, &script, bytes, Duration::from_secs(20))
        .await
        .map_err(|mut err| {
            if let Some(details) = err.details.as_mut() {
                details["stage"] = stage.into();
            }
            err
        })
}

/// Installs each agent's own payload into each agent directory that already exists on the host.
pub async fn prepare_integration(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> Result<(), IpcError> {
    for (dir, agent) in EXTENSION_AGENTS {
        let payload = agent_payload(agent)?;
        let script = match environment.platform {
            RemotePlatform::Posix => posix_install_script(dir),
            RemotePlatform::Windows => windows_install_script(dir),
        };
        run_integration_script(host, environment, &script, payload.into_bytes(), "integration")
            .await?;
    }
    Ok(())
}

/// Deletes the extension from each agent directory where it is still exactly a Ferryx payload.
pub async fn remove_integration(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> Result<RemoteRemovalReport, IpcError> {
    let mut report = RemoteRemovalReport::default();
    for (dir, agent) in EXTENSION_AGENTS {
        let (bytes, lengths) = removal_candidates(agent)?;
        let script = match environment.platform {
            RemotePlatform::Posix => posix_removal_script(dir, agent, &lengths),
            RemotePlatform::Windows => windows_removal_script(dir, agent, &lengths),
        };
        let output =
            run_integration_script(host, environment, &script, bytes, "integration_removal")
                .await?;
        report.record(agent, &output)?;
    }
    Ok(report)
}

#[cfg(test)]
mod drop_upload_tests {
    use super::*;

    #[test]
    fn upload_deadline_scales_with_byte_count() {
        assert_eq!(upload_deadline_for_bytes(0), Duration::from_secs(60));
        assert_eq!(upload_deadline_for_bytes(1024 * 1024), Duration::from_secs(65));
        assert_eq!(upload_deadline_for_bytes(30 * 1024 * 1024), Duration::from_secs(213));
        assert_eq!(upload_deadline_for_bytes(500 * 1024 * 1024), Duration::from_secs(600));
    }

    #[test]
    fn posix_drop_upload_script_contains_subfolder_and_quoted_name() {
        let script = posix_drop_upload_script("u-123", "report 2026.pdf", "FERRYX_DROP_V1_abc");
        assert!(script.contains("sub=\"$d/u-123\""));
        assert!(script.contains("target=\"$sub/\"'report 2026.pdf'"));
        assert!(script.contains("find \"$d\" -mindepth 1"));
        assert!(script.contains("printf 'FERRYX_DROP_V1_abc\\000%s\\000' \"$target\""));
    }

    #[test]
    fn windows_drop_upload_script_writes_under_temp_and_reports_the_marked_path() {
        let script = windows_drop_upload_script("u-123", "report 2026.pdf", "FERRYX_DROP_V1_abc");
        assert!(script.contains("'ferryx-paste'"));
        assert!(script.contains("[Convert]::FromBase64String('dS0xMjM=')"));
        assert!(script.contains("[Convert]::FromBase64String('cmVwb3J0IDIwMjYucGRm')"));
        assert!(!script.contains("u-123"), "the upload id must never reach PowerShell unescaped");
        assert!(!script.contains("report 2026.pdf"), "the file name must never reach PowerShell unescaped");
        assert!(script.contains("[Console]::Write(('FERRYX_DROP_V1_abc',$p,'' -join [char]0))"));
    }
}

#[cfg(test)]
mod forwarding_tests {
    use super::*;

    fn host() -> SshHost {
        SshHost {
            id: "host-one".into(),
            label: "Test host".into(),
            hostname: "example.test".into(),
            username: Some("user".into()),
            port: Some(2200),
            identity_file: None,
            jump_host: None,
            source: crate::ssh::SshHostSource::Manual,
            auth_method: crate::ssh::SshAuthMethod::Key,
            disabled: None,
        }
    }

    fn environment(platform: RemotePlatform) -> RemoteEnvironment {
        let (executor, home) = match platform {
            RemotePlatform::Posix => (super::super::runtime::RemoteExecutor::Sh, "/home/user"),
            RemotePlatform::Windows => {
                (super::super::runtime::RemoteExecutor::Powershell, r"C:\Users\user")
            }
        };
        RemoteEnvironment {
            platform,
            executor,
            version: "test".into(),
            home: home.into(),
            temp: "/tmp".into(),
            git: true,
        }
    }

    fn decoded_powershell(command: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let encoded = command
            .split_whitespace()
            .skip_while(|part| *part != "-EncodedCommand")
            .nth(1)
            .expect("an encoded PowerShell command");
        let bytes = STANDARD.decode(encoded).unwrap();
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn disabled_integration_omits_socket_forwarding() {
        let endpoint = super::super::state_bridge::StateEndpoint {
            port: 41_234,
            token: "token-abc".into(),
            process_id: 7,
        };
        let (socket, state) =
            integration_forwarding(false, Some("/tmp/local-agent.sock"), Some(&endpoint));
        assert!(socket.is_none() && state.is_none());

        let posix = environment(RemotePlatform::Posix);
        let plan = shell_plan(&host(), &posix, "/srv/repo", "session-123", socket, state).unwrap();
        assert!(!plan.args.iter().any(|arg| arg == "-R"), "{:?}", plan.args);
        assert!(!plan.args.iter().any(|arg| arg.contains("StreamLocalBindUnlink")));
        let remote = plan.args.last().unwrap();
        assert!(!remote.contains("FERRYX_AGENT_STATE_SOCKET"), "{remote}");
        assert!(remote.contains("FERRYX_SESSION_ID"), "{remote}");

        let windows = environment(RemotePlatform::Windows);
        let plan =
            shell_plan(&host(), &windows, r"C:\repo", "session-123", socket, state).unwrap();
        let script = decoded_powershell(plan.args.last().unwrap());
        assert!(script.contains("$env:FERRYX_AGENT_STATE_PORT=$null"), "{script}");
        assert!(script.contains("$env:FERRYX_AGENT_STATE_TOKEN=$null"), "{script}");
        assert!(!script.contains("41234") && !script.contains("token-abc"), "{script}");

        let (socket, state) =
            integration_forwarding(true, Some("/tmp/local-agent.sock"), Some(&endpoint));
        assert_eq!(socket, Some("/tmp/local-agent.sock"));
        assert_eq!(state.map(|endpoint| endpoint.port), Some(41_234));
        let plan = shell_plan(&host(), &posix, "/srv/repo", "session-123", socket, state).unwrap();
        assert!(plan.args.iter().any(|arg| arg == "-R"), "{:?}", plan.args);
    }

    #[test]
    fn removal_report_rejects_a_result_for_another_agent() {
        let mut report = RemoteRemovalReport::default();
        report.record("pi", b"removed pi\n").unwrap();
        report.record("omp", b"absent omp\n").unwrap();
        report.record("omo", b"kept_modified omo\r\n").unwrap();
        assert_eq!(report.removed, ["pi"]);
        assert_eq!(report.absent, ["omp"]);
        assert_eq!(report.kept_modified, ["omo"]);
        assert!(report.record("omo", b"removed pi\n").is_err());
        assert!(report.record("omo", b"").is_err());
    }
}

#[cfg(all(test, unix))]
mod installer_tests {
    use super::{posix_install_script, posix_removal_script, removal_candidates};
    use crate::daemon::agent_extension::{
        extension_source_for, EXTENSION_AGENTS, EXTENSION_FILE_NAME,
    };
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::process::{Command, Output, Stdio};

    fn run(script: &str, home: &Path, path: Option<String>, input: &[u8]) -> Output {
        let mut command = Command::new("sh");
        command
            .args(["-c", script])
            .env("HOME", home)
            .env("TMPDIR", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }

    fn install(home: &Path, dir: &str, agent: &str) -> Output {
        let payload = extension_source_for(agent).unwrap();
        run(&posix_install_script(dir), home, None, payload.as_bytes())
    }

    fn remove(home: &Path, dir: &str, agent: &str) -> String {
        let (bytes, lengths) = removal_candidates(agent).unwrap();
        let output = run(&posix_removal_script(dir, agent, &lengths), home, None, &bytes);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap()
    }

    fn extension_file(home: &Path, dir: &str) -> std::path::PathBuf {
        home.join(dir).join("agent/extensions").join(EXTENSION_FILE_NAME)
    }

    fn assert_no_staging_left(home: &Path) {
        for entry in std::fs::read_dir(home).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.starts_with(".ferryx-"), "leftover {name}");
        }
    }

    #[test]
    fn failed_copy_stops_installation_and_removes_staging_files() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let bin = fixture.path().join("bin");
        let extensions = home.join(".omo/agent/extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let target = extensions.join(EXTENSION_FILE_NAME);
        std::fs::write(&target, b"original").unwrap();
        let mv = bin.join("mv");
        std::fs::write(&mv, "#!/bin/sh\nexit 17\n").unwrap();
        std::fs::set_permissions(&mv, std::fs::Permissions::from_mode(0o755)).unwrap();
        let output = run(
            &posix_install_script(".omo"),
            &home,
            Some(format!("{}:/usr/bin:/bin", bin.display())),
            b"replacement",
        );
        assert!(
            !output.status.success(),
            "a failed copy must not report success"
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&extensions).unwrap().count(), 1);
    }

    #[test]
    fn successful_install_updates_all_present_agent_directories() {
        let fixture = tempfile::tempdir().unwrap();
        for name in [".omo", ".pi", ".omp"] {
            std::fs::create_dir(fixture.path().join(name)).unwrap();
        }
        for name in [".omo", ".pi", ".omp"] {
            let output = run(&posix_install_script(name), fixture.path(), None, b"extension");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        for name in [".omo", ".pi", ".omp"] {
            let directory = fixture.path().join(name).join("agent/extensions");
            assert_eq!(
                std::fs::read(directory.join(EXTENSION_FILE_NAME)).unwrap(),
                b"extension"
            );
            assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
        }
    }

    #[test]
    fn install_writes_each_agents_own_identity() {
        let fixture = tempfile::tempdir().unwrap();
        for (dir, _) in EXTENSION_AGENTS {
            std::fs::create_dir(fixture.path().join(dir)).unwrap();
        }
        for (dir, agent) in EXTENSION_AGENTS {
            let output = install(fixture.path(), dir, agent);
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        for (dir, agent) in EXTENSION_AGENTS {
            let written = std::fs::read_to_string(extension_file(fixture.path(), dir)).unwrap();
            assert_eq!(written, extension_source_for(agent).unwrap());
            assert!(
                written.contains(&format!("// FERRYX_INTEGRATION_ID={agent}\n")),
                "{dir} must declare {agent}"
            );
            assert!(written.contains(&format!("const AGENT_ID = \"{agent}\";")));
            let directory = fixture.path().join(dir).join("agent/extensions");
            assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
        }
        assert_no_staging_left(fixture.path());
    }

    #[test]
    fn install_does_not_create_missing_omo_dir() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join(".pi")).unwrap();
        for (dir, agent) in EXTENSION_AGENTS {
            let output = install(fixture.path(), dir, agent);
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        }
        assert!(!fixture.path().join(".omo").exists(), "a missing .omo must stay missing");
        assert!(!fixture.path().join(".omp").exists());
        assert_eq!(
            std::fs::read_to_string(extension_file(fixture.path(), ".pi")).unwrap(),
            extension_source_for("pi").unwrap()
        );
    }

    #[test]
    fn removal_deletes_matching_files_only() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        for (dir, _) in EXTENSION_AGENTS {
            std::fs::create_dir_all(home.join(dir).join("agent/extensions")).unwrap();
        }
        std::fs::write(extension_file(home, ".omo"), extension_source_for("omo").unwrap()).unwrap();
        // A file written with another agent's identity is still a Ferryx payload.
        std::fs::write(extension_file(home, ".pi"), extension_source_for("omo").unwrap()).unwrap();
        std::fs::write(extension_file(home, ".omp"), extension_source_for("omp").unwrap()).unwrap();
        let neighbour = home.join(".omp/agent/extensions/user-extension.ts");
        std::fs::write(&neighbour, extension_source_for("omp").unwrap()).unwrap();

        for (dir, agent) in EXTENSION_AGENTS {
            assert_eq!(remove(home, dir, agent), format!("removed {agent}\n"));
            assert!(!extension_file(home, dir).exists(), "{dir}");
            assert!(home.join(dir).join("agent/extensions").is_dir(), "directories stay");
        }
        assert!(neighbour.is_file(), "only the Ferryx file name is ever deleted");
        assert_no_staging_left(home);
    }

    #[test]
    fn removal_reports_kept_modified() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        std::fs::create_dir_all(home.join(".omo/agent/extensions")).unwrap();
        let edited = format!("{}\n// user edit\n", extension_source_for("omo").unwrap());
        std::fs::write(extension_file(home, ".omo"), &edited).unwrap();
        let pi = extension_source_for("pi").unwrap();
        let truncated = &pi.as_bytes()[..64];
        std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
        std::fs::write(extension_file(home, ".pi"), truncated).unwrap();
        std::fs::create_dir_all(extension_file(home, ".omp")).unwrap();

        assert_eq!(remove(home, ".omo", "omo"), "kept_modified omo\n");
        assert_eq!(std::fs::read_to_string(extension_file(home, ".omo")).unwrap(), edited);
        assert_eq!(remove(home, ".pi", "pi"), "kept_modified pi\n");
        assert_eq!(std::fs::read(extension_file(home, ".pi")).unwrap(), truncated);
        assert_eq!(remove(home, ".omp", "omp"), "kept_modified omp\n");
        assert!(extension_file(home, ".omp").is_dir(), "a directory is never deleted");
        assert_no_staging_left(home);
    }

    #[test]
    fn removal_reports_absent() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path();
        std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
        assert_eq!(remove(home, ".omo", "omo"), "absent omo\n");
        assert_eq!(remove(home, ".pi", "pi"), "absent pi\n");
        assert!(!home.join(".omo").exists(), "removal never creates directories");
        assert_no_staging_left(home);
    }
}

async fn data_output(
    host: &SshHost,
    environment: &RemoteEnvironment,
    script: &str,
    bytes: Vec<u8>,
    deadline: Duration,
) -> Result<Vec<u8>, IpcError> {
    data_output_with(environment, script, bytes, deadline, |command| {
        direct::ssh_plan(host, command, false)
    })
    .await
}

async fn data_output_with(
    environment: &RemoteEnvironment,
    script: &str,
    bytes: Vec<u8>,
    deadline: Duration,
    plan: impl FnOnce(String) -> Result<ShellCommandPlan, IpcError>,
) -> Result<Vec<u8>, IpcError> {
    let (command, input) = match environment.platform {
        RemotePlatform::Posix => (environment.executor.command(script), bytes),
        RemotePlatform::Windows => {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let input = format!(
                "$ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'; \
                 [Console]::OutputEncoding=New-Object Text.UTF8Encoding($false); \
                 try {{ $bytes=[Convert]::FromBase64String('{}'); {script} }} \
                 catch {{ [Console]::Error.Write($_.Exception.Message); exit 1 }}\n",
                STANDARD.encode(&bytes)
            );
            (
                format!(
                    "{} -NoLogo -NoProfile -NonInteractive -Command -",
                    environment.executor.program()
                ),
                input.into_bytes(),
            )
        }
    };
    let plan = plan(command)?;
    direct::bounded_output_with_stdin(&plan, deadline, input).await
}

pub async fn git(
    host: &SshHost,
    environment: &RemoteEnvironment,
    root: &str,
    args: &[&str],
) -> Result<Vec<u8>, IpcError> {
    environment.platform.validate_path(root)?;
    if !environment.git {
        return Err(error(
            IpcErrorCode::Unsupported,
            "git",
            "Git is not available on the remote host",
        ));
    }
    let script = match environment.platform {
        RemotePlatform::Posix => format!(
            "git -C {} {}",
            direct::quote_posix(root),
            args.iter()
                .map(|arg| direct::quote_posix(arg))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        RemotePlatform::Windows => format!(
            "{POWERSHELL_GIT}\n$g=Invoke-FerryxGit @('-C',{},{}); \
             [Console]::Write($g.Output); [Console]::Error.Write($g.Error); exit $g.Code",
            powershell_data(root),
            args.iter()
                .map(|arg| powershell_data(arg))
                .collect::<Vec<_>>()
                .join(",")
        ),
    };
    let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
    direct::bounded_output(&plan, Duration::from_secs(30))
        .await
        .map_err(|mut err| {
            err.code = IpcErrorCode::GitError;
            if let Some(details) = err.details.as_mut() {
                details["stage"] = "git".into();
            }
            err
        })
}
