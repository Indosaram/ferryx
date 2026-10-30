//! G4: the hook-style agents' state hooks on an SSH host.
//!
//! The local install (`daemon/agent_hook_install.rs`) uses these files, shapes, merge and strip,
//! and so does this one, applied to the host's own files. The desktop reads each file over SSH,
//! merges or strips it with the shared functions, and writes the result back only if the file
//! still holds exactly the bytes it read. A concurrent edit on the host is therefore never
//! overwritten. The command written is the staged remote helper's `agent-hook <agent> <event>`
//! (`ferryx_scope/ssh/agent_hook_core.rs`). It sends the same `agentState` line as the desktop
//! reporter, either to the pane's forwarded socket (`FERRYX_AGENT_STATE_SOCKET`) or to the
//! helper's loopback TCP ingress (`FERRYX_AGENT_STATE_PORT` plus `FERRYX_AGENT_STATE_TOKEN`). On a
//! Windows host the TCP ingress is the only channel.
//!
//! The Ferryx agent skill (`SKILL_FILES`) rides the same read-compare-write path: it is written
//! where the agent's config directory exists, and only a skill whose frontmatter still says
//! `ferryx-managed: true` is replaced or deleted.

use super::helper_setup::{self, HelperProbeState};
use super::operations::RemoteRemovalReport;
use super::runtime::{powershell_data, RemoteEnvironment, RemotePlatform};
use super::{direct, SshHost};
use crate::cli::agent_hook::HOOK_AGENTS;
use crate::daemon::agent_hook_install::{
    ferryx_script_event, hook_script_for, is_ferryx_skill, merged_settings_text,
    script_file_name_for, stripped_settings_text, HookShape, Reporter, HOOK_FILES, SKILL_FILES,
    SKILL_SOURCE,
};
use crate::ipc::IpcError;
use std::future::Future;
use std::time::Duration;

/// Settings files are read whole; this bounds one read.
const MAX_SETTINGS_BYTES: usize = 4 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(20);

/// One file Ferryx writes on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RemoteHookFile {
    pub agent: &'static str,
    /// The agent's config directory as in [`HOOK_FILES`]; `.config/` marks the XDG-rooted one.
    pub dir: &'static str,
    /// The path inside that directory, `/`-separated; for [`Kind::Skill`] the path inside HOME.
    pub rel: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Settings(HookShape, &'static [&'static str]),
    /// One of cline's per-event scripts.
    Script(&'static str),
    /// The Ferryx agent skill, gated on the agent's directory but located from HOME.
    Skill,
}

pub(super) fn hook_files(platform: RemotePlatform) -> Vec<RemoteHookFile> {
    let windows = platform == RemotePlatform::Windows;
    let mut files = Vec::new();
    for (agent, dir, file, shape) in HOOK_FILES {
        let Some(&(_, events)) = HOOK_AGENTS.iter().find(|(name, _)| *name == agent) else {
            continue;
        };
        if shape == HookShape::EventScripts {
            for &event in events {
                files.push(RemoteHookFile {
                    agent,
                    dir,
                    rel: format!("{file}/{}", script_file_name_for(event, windows)),
                    kind: Kind::Script(event),
                });
            }
        } else {
            files.push(RemoteHookFile {
                agent,
                dir,
                rel: file.to_string(),
                kind: Kind::Settings(shape, events),
            });
        }
        if let Some((_, rel)) = SKILL_FILES.iter().find(|(name, _)| *name == agent) {
            files.push(RemoteHookFile { agent, dir, rel: rel.to_string(), kind: Kind::Skill });
        }
    }
    files
}

/// What the read script found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Found {
    /// The agent is not installed on the host: nothing is written.
    NoDir,
    /// A directory or a dangling link under the file's name: never ours to touch.
    NotFile,
    Absent,
    File(Vec<u8>),
}

pub(super) fn parse_found(output: &[u8]) -> Result<Found, IpcError> {
    let split = output.iter().position(|byte| *byte == b'\n').unwrap_or(output.len());
    let (head, rest) = output.split_at(split);
    let rest = rest.get(1..).unwrap_or_default();
    match String::from_utf8_lossy(head).trim() {
        "nodir" => Ok(Found::NoDir),
        "notfile" => Ok(Found::NotFile),
        "absent" => Ok(Found::Absent),
        "file" => Ok(Found::File(rest.to_vec())),
        _ => Err(unexpected()),
    }
}

fn unexpected() -> IpcError {
    super::runtime::error(
        crate::ipc::IpcErrorCode::ParseError,
        "agent_hooks",
        "The remote host returned an unexpected hook-file result",
    )
}

/// The shell assignments locating the agent directory `d` and the file `f`.
fn posix_locate(file: &RemoteHookFile) -> String {
    let dir = match file.dir.strip_prefix(".config/") {
        Some(rest) => format!(
            "\"${{XDG_CONFIG_HOME:-$HOME/.config}}\"/{}",
            direct::quote_posix(rest)
        ),
        None => format!("\"$HOME\"/{}", direct::quote_posix(file.dir)),
    };
    let root = if file.kind == Kind::Skill { "\"$HOME\"" } else { "\"$d\"" };
    format!("d={dir}; f={root}/{};", direct::quote_posix(&file.rel))
}

fn windows_locate(file: &RemoteHookFile) -> String {
    let dir = match file.dir.strip_prefix(".config/") {
        Some(rest) => format!(
            "$c=$env:XDG_CONFIG_HOME; if (!$c) {{ $c=Join-Path $homeRoot '.config' }}; $d=Join-Path $c {};",
            powershell_data(rest)
        ),
        None => format!("$d=Join-Path $homeRoot {};", powershell_data(file.dir)),
    };
    let root = if file.kind == Kind::Skill { "$homeRoot" } else { "$d" };
    format!(
        "{dir} $f=Join-Path {root} {};",
        powershell_data(&file.rel.replace('/', "\\"))
    )
}

/// Prints `nodir`, `notfile`, `absent`, or `file` followed by the file's bytes.
pub(super) fn read_script(platform: RemotePlatform, file: &RemoteHookFile) -> String {
    match platform {
        RemotePlatform::Posix => format!(
            "{} if [ ! -d \"$d\" ]; then printf 'nodir\\n'; \
             elif [ -d \"$f\" ] || {{ [ -L \"$f\" ] && [ ! -e \"$f\" ]; }}; then printf 'notfile\\n'; \
             elif [ ! -e \"$f\" ]; then printf 'absent\\n'; \
             else printf 'file\\n'; cat \"$f\"; fi",
            posix_locate(file)
        ),
        RemotePlatform::Windows => format!(
            "{} $o=[Console]::OpenStandardOutput(); \
             if (![IO.Directory]::Exists($d)) {{ $h='nodir' }} \
             else {{ $i=Get-Item -LiteralPath $f -Force -ErrorAction SilentlyContinue; \
                 if (!$i) {{ $h='absent' }} elseif ($i.PSIsContainer) {{ $h='notfile' }} else {{ $h='file' }} }}; \
             $b=[Text.Encoding]::ASCII.GetBytes($h+\"`n\"); $o.Write($b,0,$b.Length); \
             if ($h -eq 'file') {{ $b=[IO.File]::ReadAllBytes($f); $o.Write($b,0,$b.Length) }}; $o.Flush()",
            windows_locate(file)
        ),
    }
}

/// What a write replaces: the file must still be absent, or still hold exactly these bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Expect {
    Absent,
    Bytes(Vec<u8>),
}

/// What a write does once the expectation holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Change {
    Write { bytes: Vec<u8>, executable: bool },
    Delete,
}

/// stdin carries the expected bytes followed by the new ones. The script prints one of `nodir`,
/// `changed`, `written` or `removed`. `changed` means the file no longer matches what was read, so
/// it is left alone. A symlinked file is written through at its target, as the local install does.
pub(super) fn write_script(
    platform: RemotePlatform,
    file: &RemoteHookFile,
    expect: &Expect,
    change: &Change,
) -> String {
    let old = match expect {
        Expect::Absent => 0,
        Expect::Bytes(bytes) => bytes.len(),
    };
    let new = match change {
        Change::Write { bytes, .. } => bytes.len(),
        Change::Delete => 0,
    };
    match platform {
        RemotePlatform::Posix => {
            let check = match expect {
                Expect::Absent => {
                    "if [ -e \"$f\" ] || [ -L \"$f\" ]; then printf 'changed\\n'; exit 0; fi;"
                }
                Expect::Bytes(_) => {
                    "if [ ! -f \"$f\" ] || ! cmp -s \"$f\" \"$old\"; then printf 'changed\\n'; exit 0; fi;"
                }
            };
            let act = match change {
                Change::Delete => "rm -f \"$f\"; printf 'removed\\n'".to_string(),
                Change::Write { executable, .. } => format!(
                    "mkdir -p \"$(dirname \"$f\")\"; \
                     if [ -L \"$f\" ]; then cat \"$new\" > \"$f\"; \
                     else tmp=\"$f.ferryx.tmp.$$\"; if [ -e \"$f\" ]; then cp -p \"$f\" \"$tmp\"; fi; \
                         cat \"$new\" > \"$tmp\"; {}mv -f \"$tmp\" \"$f\"; tmp=; fi; \
                     printf 'written\\n'",
                    if *executable { "chmod 755 \"$tmp\"; " } else { "" }
                ),
            };
            format!(
                "set -e; {locate} base=\"${{TMPDIR:-/tmp}}\"; base=\"${{base%/}}\"; old=; new=; tmp=; \
                 all=$(mktemp \"$base/.ferryx-hook.XXXXXX\"); \
                 trap 'rm -f \"$all\"; for x in \"$old\" \"$new\" \"$tmp\"; do [ -z \"$x\" ] || rm -f \"$x\"; done' EXIT; \
                 cat > \"$all\"; \
                 if [ ! -d \"$d\" ]; then printf 'nodir\\n'; exit 0; fi; \
                 old=$(mktemp \"$base/.ferryx-hook.XXXXXX\"); new=$(mktemp \"$base/.ferryx-hook.XXXXXX\"); \
                 dd if=\"$all\" of=\"$old\" bs=1 count={old} 2>/dev/null; \
                 dd if=\"$all\" of=\"$new\" bs=1 skip={old} count={new} 2>/dev/null; \
                 {check} {act}",
                locate = posix_locate(file),
            )
        }
        RemotePlatform::Windows => {
            let check = match expect {
                Expect::Absent => "$i",
                Expect::Bytes(_) => {
                    "(!$i -or $i.PSIsContainer -or ([Convert]::ToBase64String([IO.File]::ReadAllBytes($f)) -ne [Convert]::ToBase64String($old)))"
                }
            };
            let act = match change {
                Change::Delete => "[IO.File]::Delete($f); $s='removed'",
                Change::Write { .. } => {
                    "[void][IO.Directory]::CreateDirectory((Split-Path -Parent $f)); \
                     if ($i -and $i.LinkType) { [IO.File]::WriteAllBytes($f,$new) } \
                     else { $t=$f+'.ferryx.tmp'; [IO.File]::WriteAllBytes($t,$new); Move-Item -LiteralPath $t -Destination $f -Force }; \
                     $s='written'"
                }
            };
            format!(
                "{locate} $n={old}; $old=[byte[]]@(); if ($n -gt 0) {{ $old=[byte[]]$bytes[0..($n-1)] }}; \
                 $new=[byte[]]@(); if ($bytes.Length -gt $n) {{ $new=[byte[]]$bytes[$n..($bytes.Length-1)] }}; \
                 $i=Get-Item -LiteralPath $f -Force -ErrorAction SilentlyContinue; \
                 if (![IO.Directory]::Exists($d)) {{ $s='nodir' }} elseif ({check}) {{ $s='changed' }} else {{ {act} }}; \
                 [Console]::Write($s+\"`n\")",
                locate = windows_locate(file),
            )
        }
    }
}

/// One remote round trip the install or removal needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// A script whose output may be a whole settings file.
    Read(String),
    /// A script fed these bytes on stdin, answering one short line.
    Write(String, Vec<u8>),
}

fn write_step(platform: RemotePlatform, file: &RemoteHookFile, expect: Expect, change: Change) -> Step {
    let script = write_script(platform, file, &expect, &change);
    let mut bytes = match expect {
        Expect::Absent => Vec::new(),
        Expect::Bytes(bytes) => bytes,
    };
    if let Change::Write { bytes: new, .. } = change {
        bytes.extend_from_slice(&new);
    }
    Step::Write(script, bytes)
}

fn write_result(output: &[u8]) -> Result<&'static str, IpcError> {
    let text = String::from_utf8_lossy(output);
    match text.lines().map(str::trim).rfind(|line| !line.is_empty()) {
        Some("written") => Ok("written"),
        Some("removed") => Ok("removed"),
        Some("changed") => Ok("changed"),
        Some("nodir") => Ok("nodir"),
        _ => Err(unexpected()),
    }
}

/// The change an install makes to one file it found, or `None` to leave it.
pub(super) fn install_change(
    file: &RemoteHookFile,
    reporter: &Reporter,
    found: Found,
) -> Option<(Expect, Change)> {
    let (expect, existing) = match found {
        Found::NoDir | Found::NotFile => return None,
        Found::Absent => (Expect::Absent, None),
        Found::File(bytes) => {
            let text = String::from_utf8(bytes.clone()).ok()?;
            (Expect::Bytes(bytes), Some(text))
        }
    };
    match file.kind {
        Kind::Settings(shape, events) => {
            match merged_settings_text(shape, file.agent, events, reporter, existing.as_deref()) {
                Ok(Some(text)) => Some((
                    expect,
                    Change::Write { bytes: text.into_bytes(), executable: false },
                )),
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!(agent = file.agent, file = %file.rel, %error, "Remote agent hook file left as is");
                    None
                }
            }
        }
        Kind::Script(event) => {
            let body = hook_script_for(reporter, file.agent, event);
            match existing.as_deref() {
                Some(text) if text == body => None,
                // Cline runs one script per event: a script Ferryx did not write is the user's own.
                Some(text) if ferryx_script_event(text, file.agent).is_none() => None,
                _ => Some((expect, Change::Write { bytes: body.into_bytes(), executable: true })),
            }
        }
        Kind::Skill => match existing.as_deref() {
            Some(text) if text == SKILL_SOURCE || !is_ferryx_skill(text) => None,
            _ => Some((
                expect,
                Change::Write { bytes: SKILL_SOURCE.as_bytes().to_vec(), executable: false },
            )),
        },
    }
}

/// The change a removal makes to one file it found. `Err(())` marks a file that cannot be read
/// as one Ferryx wrote; it is reported as kept.
pub(super) fn removal_change(
    file: &RemoteHookFile,
    found: Found,
) -> Result<Option<(Expect, Change)>, ()> {
    let Found::File(bytes) = found else { return Ok(None) };
    let text = String::from_utf8(bytes.clone()).map_err(|_| ())?;
    match file.kind {
        Kind::Settings(shape, _) => match stripped_settings_text(shape, file.agent, &text) {
            Ok(Some(rest)) => Ok(Some((
                Expect::Bytes(bytes),
                Change::Write { bytes: rest.into_bytes(), executable: false },
            ))),
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        },
        Kind::Script(_) if ferryx_script_event(&text, file.agent).is_some() => {
            Ok(Some((Expect::Bytes(bytes), Change::Delete)))
        }
        Kind::Script(_) => Ok(None),
        Kind::Skill if is_ferryx_skill(&text) => Ok(Some((Expect::Bytes(bytes), Change::Delete))),
        Kind::Skill => Ok(None),
    }
}

/// Installs every agent's hooks through `exec`. Returns how many files were written.
pub(super) async fn install_with<F, Fut>(
    platform: RemotePlatform,
    reporter: &Reporter,
    mut exec: F,
) -> Result<usize, IpcError>
where
    F: FnMut(Step) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, IpcError>>,
{
    let mut written = 0;
    for file in hook_files(platform) {
        let found = parse_found(&exec(Step::Read(read_script(platform, &file))).await?)?;
        let Some((expect, change)) = install_change(&file, reporter, found) else { continue };
        match write_result(&exec(write_step(platform, &file, expect, change)).await?)? {
            "written" => written += 1,
            "changed" => tracing::warn!(agent = file.agent, file = %file.rel, "Remote agent hook file changed while installing; left as is"),
            _ => {}
        }
    }
    Ok(written)
}

/// The label one agent's hooks are reported under in a [`RemoteRemovalReport`].
pub(super) fn removal_label(agent: &str) -> String {
    format!("{agent}-hooks")
}

/// Deletes every Ferryx hook entry through `exec` and records one result per agent:
/// - `removed` when any entry went;
/// - `kept_modified` when an entry was left because the file changed or could not be read;
/// - `absent` otherwise.
pub(super) async fn remove_with<F, Fut>(
    platform: RemotePlatform,
    report: &mut RemoteRemovalReport,
    mut exec: F,
) -> Result<(), IpcError>
where
    F: FnMut(Step) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, IpcError>>,
{
    for (agent, _) in HOOK_AGENTS {
        let (mut removed, mut kept) = (false, false);
        for file in hook_files(platform).into_iter().filter(|file| file.agent == agent) {
            let found = parse_found(&exec(Step::Read(read_script(platform, &file))).await?)?;
            match removal_change(&file, found) {
                Ok(None) => {}
                Err(()) => kept = true,
                Ok(Some((expect, change))) => {
                    match write_result(&exec(write_step(platform, &file, expect, change)).await?)? {
                        "written" | "removed" => removed = true,
                        "changed" => kept = true,
                        _ => {}
                    }
                }
            }
        }
        let slot = if removed {
            &mut report.removed
        } else if kept {
            &mut report.kept_modified
        } else {
            &mut report.absent
        };
        slot.push(removal_label(agent));
    }
    Ok(())
}

/// The staged helper that the hook commands name. The version-qualified copy for this desktop's
/// helper version is tried first, then the legacy unversioned one. Returns `None` when neither is
/// staged, or when the staged one does not advertise `agentHookV1` in `--capabilities`: a hook
/// naming a missing executable, or one without the subcommand, would fail in the agent's UI.
async fn staged_reporter(host: &SshHost, environment: &RemoteEnvironment) -> Option<Reporter> {
    for location in helper_setup::probe_locations(host, environment).ok()? {
        if helper_setup::probe_ready_at(host, environment, &location).await
            == HelperProbeState::Installed
            && answers_agent_hook(host, environment, &location.executable).await
        {
            return Some(Reporter {
                exe: location.executable,
                windows: environment.platform == RemotePlatform::Windows,
            });
        }
    }
    None
}

/// Runs the staged helper's `--capabilities` and checks it lists `agentHookV1`.
async fn answers_agent_hook(host: &SshHost, environment: &RemoteEnvironment, exe: &str) -> bool {
    let script = match environment.platform {
        RemotePlatform::Posix => format!("{} --capabilities", direct::quote_posix(exe)),
        RemotePlatform::Windows => format!("& {} --capabilities", powershell_data(exe)),
    };
    let output = async {
        let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
        direct::bounded_output(&plan, Duration::from_secs(10)).await
    }
    .await;
    output.is_ok_and(|stdout| advertises_agent_hook(&stdout))
}

/// True when a `--capabilities` report lists `agentHookV1`.
pub(super) fn advertises_agent_hook(stdout: &[u8]) -> bool {
    std::str::from_utf8(stdout)
        .ok()
        .and_then(|text| text.lines().find_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok()))
        .and_then(|report| report.get("capabilities")?.as_array().cloned())
        .is_some_and(|caps| {
            caps.iter().any(|cap| {
                cap.as_str() == Some(crate::ssh::helper_runtime::process::AGENT_HOOK_CAPABILITY)
            })
        })
}

async fn exec(
    host: &SshHost,
    environment: &RemoteEnvironment,
    step: Step,
) -> Result<Vec<u8>, IpcError> {
    match step {
        Step::Write(script, bytes) => {
            super::operations::run_integration_script(host, environment, &script, bytes, "agent_hooks")
                .await
        }
        Step::Read(script) => {
            environment.platform.validate_path(&environment.home)?;
            let script = match environment.platform {
                RemotePlatform::Posix => {
                    format!("HOME={}; {script}", direct::quote_posix(&environment.home))
                }
                RemotePlatform::Windows => {
                    format!("$homeRoot={}; {script}", powershell_data(&environment.home))
                }
            };
            let plan = direct::ssh_plan(host, environment.executor.command(&script), false)?;
            direct::bounded_output_with_limit(&plan, DEADLINE, MAX_SETTINGS_BYTES).await
        }
    }
}

/// Installs the hook-style agents' hooks on the host, pointing at its staged remote helper.
pub async fn install(host: &SshHost, environment: &RemoteEnvironment) -> Result<(), IpcError> {
    let Some(reporter) = staged_reporter(host, environment).await else {
        tracing::info!(host = %host.id, "No staged remote helper; remote agent hooks not installed");
        return Ok(());
    };
    install_with(environment.platform, &reporter, |step| exec(host, environment, step)).await?;
    Ok(())
}

/// Deletes the Ferryx hook entries from the host's agent files, recording each agent in `report`.
pub async fn remove(
    host: &SshHost,
    environment: &RemoteEnvironment,
    report: &mut RemoteRemovalReport,
) -> Result<(), IpcError> {
    remove_with(environment.platform, report, |step| exec(host, environment, step)).await
}

#[cfg(test)]
#[path = "remote_hooks_tests.rs"]
mod tests;
