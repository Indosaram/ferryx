use super::runtime::{
    error, parse_fields, powershell_data, RemoteEnvironment, RemotePlatform, POWERSHELL_GIT,
};
use super::{direct, SshHost};
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

pub async fn upload(
    host: &SshHost,
    environment: &RemoteEnvironment,
    file_name: &str,
    bytes: Vec<u8>,
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
    let output = data_output(host, environment, &script, bytes, Duration::from_secs(60))
        .await.map_err(|mut err| {
            if let Some(details) = err.details.as_mut() { details["stage"] = "upload".into(); }
            err
        })?;
    let fields = parse_fields(&output, &marker, 1)?;
    environment.platform.validate_path(fields[0])?;
    Ok(fields[0].into())
}

const POSIX_INTEGRATION_SCRIPT: &str =
    "set -e; d=\"$HOME/.omo/agent/extensions\"; mkdir -p \"$d\"; \
     tmp=$(mktemp \"$d/.ferryx-agent-state.XXXXXX\"); \
     stage=; trap 'rm -f \"$tmp\"; [ -z \"$stage\" ] || rm -f \"$stage\"' EXIT; cat > \"$tmp\"; \
     for b in \"$HOME/.omo\" \"$HOME/.pi\" \"$HOME/.omp\"; do \
         if [ -d \"$b\" ]; then mkdir -p \"$b/agent/extensions\"; \
             target=\"$b/agent/extensions/ferryx-agent-state.ts\"; \
             stage=$(mktemp \"$b/agent/extensions/.ferryx-agent-state.XXXXXX\"); \
             cp \"$tmp\" \"$stage\"; mv -f \"$stage\" \"$target\"; stage=; fi; done";

pub async fn prepare_integration(
    host: &SshHost,
    environment: &RemoteEnvironment,
) -> Result<(), IpcError> {
    environment.platform.validate_path(&environment.home)?;
    let script = match environment.platform {
        RemotePlatform::Posix => POSIX_INTEGRATION_SCRIPT.into(),
        RemotePlatform::Windows => String::from(
            "foreach ($name in @('.omo','.pi','.omp')) { \
                 $b=Join-Path $homeRoot $name; \
                 if ($name -eq '.omo' -or [IO.Directory]::Exists($b)) { \
                     $d=Join-Path $b 'agent/extensions'; [void][IO.Directory]::CreateDirectory($d); \
                     $p=Join-Path $d 'ferryx-agent-state.ts'; $tmp=$p+'.'+[Guid]::NewGuid().ToString()+'.tmp'; \
                     try { [IO.File]::WriteAllBytes($tmp,$bytes); \
                         if ([IO.File]::Exists($p)) { [IO.File]::Replace($tmp,$p,[NullString]::Value) } else { [IO.File]::Move($tmp,$p) } \
                     } finally { if ([IO.File]::Exists($tmp)) { [IO.File]::Delete($tmp) } } \
                 } \
             }"
        ),
    };
    let script = match environment.platform {
        RemotePlatform::Posix => {
            format!("HOME={}; {script}", direct::quote_posix(&environment.home))
        }
        RemotePlatform::Windows => {
            format!("$homeRoot={}; {script}", powershell_data(&environment.home))
        }
    };
    data_output(
        host,
        environment,
        &script,
        crate::daemon::agent_extension::EXTENSION_SOURCE
            .as_bytes()
            .to_vec(),
        Duration::from_secs(20),
    )
    .await.map_err(|mut err| {
        if let Some(details) = err.details.as_mut() { details["stage"] = "integration".into(); }
        err
    })?;
    Ok(())
}

#[cfg(all(test, unix))]
mod installer_tests {
    use super::POSIX_INTEGRATION_SCRIPT;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    #[test]
    fn failed_copy_stops_installation_and_removes_staging_files() {
        let fixture = tempfile::tempdir().unwrap();
        let home = fixture.path().join("home");
        let bin = fixture.path().join("bin");
        let extensions = home.join(".omo/agent/extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        std::fs::create_dir_all(home.join(".pi")).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let target = extensions.join("ferryx-agent-state.ts");
        std::fs::write(&target, b"original").unwrap();
        let cp = bin.join("cp");
        std::fs::write(&cp, "#!/bin/sh\ncase \"$2\" in \"$HOME\"/.omo/*) exit 17;; esac\nexec /bin/cp \"$@\"\n").unwrap();
        std::fs::set_permissions(&cp, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = Command::new("sh")
            .args(["-c", POSIX_INTEGRATION_SCRIPT])
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"replacement").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success(), "a failed copy must not report success");
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&extensions).unwrap().count(), 1);
        assert!(!home.join(".pi/agent/extensions/ferryx-agent-state.ts").exists());
    }

    #[test]
    fn successful_install_updates_all_present_agent_directories() {
        let fixture = tempfile::tempdir().unwrap();
        for name in [".omo", ".pi", ".omp"] {
            std::fs::create_dir(fixture.path().join(name)).unwrap();
        }
        let mut child = Command::new("sh")
            .args(["-c", POSIX_INTEGRATION_SCRIPT])
            .env("HOME", fixture.path())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"extension").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        for name in [".omo", ".pi", ".omp"] {
            let directory = fixture.path().join(name).join("agent/extensions");
            assert_eq!(std::fs::read(directory.join("ferryx-agent-state.ts")).unwrap(), b"extension");
            assert_eq!(std::fs::read_dir(directory).unwrap().count(), 1);
        }
    }
}

async fn data_output(
    host: &SshHost,
    environment: &RemoteEnvironment,
    script: &str,
    bytes: Vec<u8>,
    deadline: Duration,
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
            (format!("{} -NoLogo -NoProfile -NonInteractive -Command -", environment.executor.program()),
                input.into_bytes())
        }
    };
    let plan = direct::ssh_plan(host, command, false)?;
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
