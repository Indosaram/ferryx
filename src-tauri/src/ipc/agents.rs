use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
#[cfg(feature = "desktop")]
use tauri::Manager;

#[cfg(all(test, unix))]
#[path = "agents_reset_event_tests.rs"]
mod reset_event_tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    pub name: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_agents_detect(names: Vec<String>) -> Result<Vec<AgentDetection>, super::IpcError> {
    super::run_blocking(move || Ok(detect_agents(&names))).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_agent_session_discover(
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    session_id: String,
    agent_type: String,
) -> Result<Option<String>, super::IpcError> {
    daemon_client
        .discover_agent_session(&session_id, &agent_type)
        .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_agent_state_reset<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon_client: tauri::State<'_, std::sync::Arc<crate::daemon::client::DaemonClient>>,
    session_id: String,
) -> Result<(), super::IpcError> {
    daemon_client.reset_agent_state(&session_id).await?;
    #[cfg(feature = "native-terminal")]
    if let Some(host) =
        app.try_state::<crate::native_terminal::surface_host::NativeTerminalSurfaceHostState>()
    {
        host.reset_agent_state::<R>(&session_id, Some(&app));
    }
    #[cfg(not(feature = "native-terminal"))]
    let _ = app;
    Ok(())
}

pub(crate) fn discover_agent_session_id(root_pid: u32, agent_type: &str) -> Option<String> {
    discover_agent_session(root_pid, agent_type).map(|(id, _)| id)
}

/// Where a discovered provider id came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscoverySource {
    /// A file the process holds open, or (omo) its environment: it follows the live conversation.
    Live,
    /// The launch argv only. It cannot follow a conversation the agent switched to after launch
    /// (`/clear`, `/new`, an in-TUI `/resume`), so it must not veto a different reported id.
    LaunchArgv,
}

/// [`discover_agent_session_id`] plus where the id came from.
pub(crate) fn discover_agent_session(
    root_pid: u32,
    agent_type: &str,
) -> Option<(String, DiscoverySource)> {
    let requested = agent_type.trim().to_ascii_lowercase();
    let normalized = if requested == "cursor-agent" {
        "cursor"
    } else {
        requested.as_str()
    };
    if !matches!(
        normalized,
        "claude"
            | "codex"
            | "copilot"
            | "cursor"
            | "kimi"
            | "omo"
            | "gjc"
            | "antigravity"
            | "opencode"
            | "pi"
            | "omp"
            | "grok"
            | "droid"
            | "cline"
            | "crush"
    ) {
        return None;
    }

    let entries = process_table_entries();
    let agent_pid = entries
        .as_deref()
        .and_then(|entries| descendant_agent_pid(entries, root_pid, normalized));
    // A resumed grok, droid, cline, crush, kimi, copilot or gjc names its session in its own argv, which binds the id
    // to this pane's process. The open-file walk stays first because it follows a conversation the
    // agent switched to after launch, which the launch argv cannot.
    let resumed_id = |pid: u32| {
        let (_, _, args) = entries
            .as_deref()?
            .iter()
            .find(|(entry_pid, _, _)| *entry_pid == pid)?;
        resume_id_from_args(normalized, args).map(|id| (id, DiscoverySource::LaunchArgv))
    };
    let live = |id: Option<String>| id.map(|id| (id, DiscoverySource::Live));
    match agent_pid {
        Some(pid) if normalized == "omo" => live(
            omo_session_id_from_environment(pid, normalized)
                .or_else(|| lsof_session_id(pid, normalized)),
        ),
        Some(pid) if normalized == "antigravity" => live(antigravity_session_id(Some(pid), root_pid)),
        None if normalized == "antigravity" => live(antigravity_session_id(None, root_pid)),
        Some(pid) if normalized == "opencode" => live(opencode_session_id(Some(pid), root_pid)),
        None if normalized == "opencode" => live(opencode_session_id(None, root_pid)),
        Some(pid) if normalized == "pi" => live(pi_session_id(Some(pid), root_pid)),
        None if normalized == "pi" => live(pi_session_id(None, root_pid)),
        Some(pid)
            if matches!(
                normalized,
                "claude" | "omp" | "grok" | "droid" | "cline" | "kimi" | "copilot" | "gjc"
            ) =>
        {
            live(lsof_session_id(pid, normalized)).or_else(|| resumed_id(pid))
        }
        // crush keeps every session of a project in one `.crush/crush.db`, so no open file names
        // the session; only the resume argv does.
        Some(pid) if normalized == "crush" => resumed_id(pid),
        None if normalized == "crush" => None,
        Some(pid) => live(lsof_session_id(pid, normalized)),
        None => live(lsof_session_id(root_pid, normalized)),
    }
}

#[cfg(not(windows))]
fn process_table_entries() -> Option<Vec<(u32, u32, String)>> {
    let ps = resolve_tool("ps")?;
    let stdout = probe_stdout(
        "ps",
        crate::util::no_window_command(ps).args(["-axwwo", "pid=,ppid=,args="]),
    )?;
    String::from_utf8(stdout)
        .ok()
        .map(|stdout| parse_ps_entries(&stdout))
}

// Only the unix arm reads `ps`, so this parser is unix-only outside tests.
#[cfg(any(not(windows), test))]
fn parse_ps_entries(stdout: &str) -> Vec<(u32, u32, String)> {
    stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            let pid_end = trimmed.find(char::is_whitespace)?;
            let pid = trimmed[..pid_end].parse().ok()?;
            let after_pid = trimmed[pid_end..].trim_start();
            let ppid_end = after_pid.find(char::is_whitespace)?;
            let ppid = after_pid[..ppid_end].parse().ok()?;
            let args = after_pid[ppid_end..].trim_start().to_string();
            Some((pid, ppid, args))
        })
        .collect()
}

// Windows has no `ps`, so the process table comes from PowerShell's CIM view of
// Win32_Process, which exposes the same (pid, ppid, command line) triple. A null
// CommandLine (protected or elevated processes) becomes an empty string so every
// entry keeps the tuple shape the tree walk expects.
#[cfg(windows)]
fn process_table_entries() -> Option<Vec<(u32, u32, String)>> {
    let powershell = resolve_tool("powershell")?;
    let stdout = probe_stdout(
        "powershell",
        crate::util::no_window_command(powershell).args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            POWERSHELL_PROCESS_TABLE_COMMAND,
        ]),
    )?;
    let stdout = match String::from_utf8(stdout) {
        Ok(stdout) => stdout,
        Err(error) => {
            tracing::debug!(%error, "agent session discovery: powershell output not utf-8");
            return None;
        }
    };
    let entries = parse_powershell_process_json(&stdout);
    if entries.is_none() {
        tracing::debug!("agent session discovery: powershell process table unparsable");
    }
    entries
}

// Pinning the console output encoding to UTF-8 keeps non-ASCII command lines
// intact across the pipe instead of arriving in the console code page.
#[cfg(windows)]
const POWERSHELL_PROCESS_TABLE_COMMAND: &str =
    "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Get-CimInstance Win32_Process | \
     Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress";

// ConvertTo-Json emits an array for several processes but a bare object for a
// single one, so both shapes are accepted; anything else is not a table.
#[cfg(any(windows, test))]
fn parse_powershell_process_json(json_str: &str) -> Option<Vec<(u32, u32, String)>> {
    #[derive(serde::Deserialize)]
    struct WindowsProcessEntry {
        #[serde(rename = "ProcessId")]
        process_id: u32,
        #[serde(rename = "ParentProcessId")]
        parent_process_id: u32,
        #[serde(rename = "CommandLine", default)]
        command_line: Option<String>,
    }

    let entries: Vec<WindowsProcessEntry> = match serde_json::from_str(json_str) {
        Ok(entries) => entries,
        Err(_) => vec![serde_json::from_str(json_str).ok()?],
    };

    Some(
        entries
            .into_iter()
            .map(|entry| {
                (
                    entry.process_id,
                    entry.parent_process_id,
                    entry.command_line.unwrap_or_default(),
                )
            })
            .collect(),
    )
}

fn args_match_agent(agent_type: &str, args: &str) -> bool {
    let requested = agent_type.trim().to_ascii_lowercase();
    let normalized_agent = if requested == "cursor-agent" {
        "cursor"
    } else {
        requested.as_str()
    };
    if normalized_agent == "omo" {
        let normalized_args = args.to_ascii_lowercase();
        let executable = if let Some(quoted) = args.trim_start().strip_prefix('"') {
            quoted.split('"').next().unwrap_or("")
        } else {
            args.split_whitespace().next().unwrap_or("")
        };
        let basename = executable.rsplit(['/', '\\']).next().unwrap_or(executable);
        return basename.eq_ignore_ascii_case("mhc")
            || basename.eq_ignore_ascii_case("mhc.exe")
            || normalized_args.contains("/omo.js")
            || normalized_args.contains("/senpi/dist/cli.js");
    }

    let Some(command) = args.split_whitespace().next() else {
        return false;
    };
    let basename = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(command)
        .to_ascii_lowercase();
    // kimi is a uv console script (`…/kimi-cli/bin/python ~/.local/bin/kimi`, local-2.md:212) and
    // gjc a bun-shebang symlink (`bun …/bin/gjc`, remote-gjc.md:18): argv[0] is the interpreter
    // and the agent is the entry script that follows it.
    let interpreted_entry = || {
        let interpreter = basename.trim_end_matches(".exe");
        let is_interpreter = matches!(interpreter, "bun" | "node" | "python" | "python3")
            || (interpreter.starts_with("python3.")
                && interpreter[8..].bytes().all(|byte| byte.is_ascii_digit()));
        is_interpreter
            && args.split_whitespace().nth(1).is_some_and(|entry| {
                entry
                    .rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(normalized_agent))
            })
    };
    match normalized_agent {
        "cursor" => matches!(basename.as_str(), "cursor-agent" | "cursor"),
        "claude" | "codex" | "copilot" => basename == normalized_agent,
        "kimi" => basename == "kimi" || interpreted_entry(),
        "antigravity" => basename == "agy",
        "gjc" => {
            basename == "gjc"
                || args.to_ascii_lowercase().contains("/gjc.js")
                || interpreted_entry()
        }
        "opencode" => basename == "opencode",
        "pi" => {
            basename == "pi"
                || basename == "pi.js"
                || args.to_ascii_lowercase().contains("/pi-coding-agent")
        }
        // The bun install runs `bun ~/.bun/bin/omp`, so the entry script is argv[1]; omp's own
        // worker helpers run `bun …/dist/cli.js __omp_worker_*` and must not match.
        "omp" => {
            basename == "omp"
                || (basename == "bun"
                    && args.split_whitespace().nth(1).is_some_and(|entry| {
                        entry
                            .rsplit(['/', '\\'])
                            .next()
                            .is_some_and(|name| name.eq_ignore_ascii_case("omp"))
                    }))
        }
        // The npm install runs `node …/bin/crush` with the Go binary `…/@charmland/crush/bin/crush`
        // as its child; a Go or Homebrew install is the bare binary.
        "grok" | "droid" | "crush" => basename == normalized_agent,
        // The npm shim runs `node ~/.bun/bin/cline` and the native `cline` binary is its child;
        // the detached `--cline-hub-daemon` serves sessions for other panes too.
        "cline" => basename == "cline" && !args.contains("--cline-hub-daemon"),
        _ => false,
    }
}

/// The session a resumed agent was launched with, read from that agent's own argv: grok
/// `--resume <id>`, droid `--resume <id>` or `droid resume <id>`, cline `--id <id>`, crush
/// `--session/-s <id>`. Only ids in the provider's own shape are accepted, so a grok title or a
/// missing value is not an id.
fn resume_id_from_args(agent_type: &str, args: &str) -> Option<String> {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let flags: &[&str] = match agent_type {
        "grok" => &["--resume", "-r"],
        "droid" => &["--resume", "-r", "resume"],
        "cline" => &["--id"],
        "crush" => &["--session", "-s"],
        // `claude --help`: `-r, --resume [value]` and `--session-id <uuid>`; Ferryx resumes claude
        // with `--resume <uuid>` (terminal/shell.rs). claude keeps no transcript open, so argv is
        // its only process-bound proof (re-explore-5 G1, measured on claude 2.1.308).
        "claude" => &["--resume", "-r", "--session-id"],
        // Ferryx resumes omp with `--resume <transcript-path>` or `--resume <id>`
        // (terminal/shell.rs); a path value is read through omp's own store-path reader.
        "omp" => &["--resume"],
        // Measured flags: `kimi --help` (--session/-S), `copilot --help` (--resume [sessionId]),
        // and the resume argv Ferryx itself spawns (terminal/shell.rs resolve_agent_resume_plan).
        "kimi" => &["--session", "-S"],
        "copilot" => &["--resume"],
        "gjc" => &["--resume"],
        _ => return None,
    };
    let value = tokens
        .windows(2)
        .find(|pair| flags.contains(&pair[0]))
        .map(|pair| pair[1])
        .or_else(|| {
            tokens.iter().find_map(|token| {
                let (flag, value) = token.split_once('=')?;
                flags.contains(&flag).then_some(value)
            })
        })?;
    let valid = match agent_type {
        "omp" => {
            return if is_valid_uuid(value) {
                Some(value.to_string())
            } else {
                extract_session_id_from_path("omp", value)
            };
        }
        "cline" => is_cline_session_id(value),
        "crush" => is_valid_uuid(value) || is_crush_session_hash(value),
        // gjc ids are the 16-hex suffix of its store file (gjc_session_id_from_path).
        "gjc" => is_crush_session_hash(value),
        _ => is_valid_uuid(value),
    };
    valid.then(|| value.to_string())
}

// crush prints and accepts the 16-hex XXH3 hash of the session UUID (`crush -s 7f7fbfa62dd136ec`)
// as well as the UUID itself. Hash prefixes also resolve, but only the full hash is accepted here.
fn is_crush_session_hash(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn descendant_agent_pid(
    entries: &[(u32, u32, String)],
    root_pid: u32,
    agent_type: &str,
) -> Option<u32> {
    let mut children = HashMap::<u32, Vec<u32>>::new();
    let mut args_by_pid = HashMap::<u32, &str>::new();
    for (pid, ppid, args) in entries {
        children.entry(*ppid).or_default().push(*pid);
        args_by_pid.insert(*pid, args);
    }

    let mut queue = children
        .get(&root_pid)
        .into_iter()
        .flatten()
        .copied()
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::from([root_pid]);
    while let Some(pid) = queue.pop_front() {
        if !visited.insert(pid) {
            continue;
        }
        if args_by_pid
            .get(&pid)
            .is_some_and(|args| args_match_agent(agent_type, args))
        {
            return Some(pid);
        }
        if let Some(descendants) = children.get(&pid) {
            queue.extend(descendants.iter().copied());
        }
    }
    None
}

fn omo_session_id_from_environment(pid: u32, agent_type: &str) -> Option<String> {
    process_environment_output(pid)
        .and_then(|stdout| session_id_from_env_output(agent_type, &stdout))
}

/// The omo conversation id of the agent running inside a Ferryx session, read from the process
/// table rather than from a report.
///
/// The daemon learns an agent's conversation only when the agent reports a state change, and a
/// daemon started by handover begins with none, so an agent that has been idle since keeps its
/// session unresolved. Ferryx puts `FERRYX_SESSION_ID` into every PTY environment, which makes it
/// an exact owner key; the conversation comes from the agent's `--session <id>` argument or its
/// `PI_SESSION_FILE`. Two different conversations under one session answer `None` rather than a
/// guess, since a wrong answer would show another agent's chat.
pub(crate) fn omo_session_id_for_ferryx_session(ferryx_session_id: &str) -> Option<String> {
    let table = process_environment_table()?;
    omo_session_id_in_table(&table, ferryx_session_id)
}

fn omo_session_id_in_table(table: &str, ferryx_session_id: &str) -> Option<String> {
    let owner = format!("FERRYX_SESSION_ID={ferryx_session_id}");
    let mut found: Option<String> = None;
    for line in table.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if !tokens.iter().any(|token| *token == owner) {
            continue;
        }
        let from_arg = tokens
            .windows(2)
            .find(|pair| pair[0] == "--session")
            .map(|pair| pair[1].to_string());
        let id = from_arg.or_else(|| session_id_from_env_output("omo", line));
        let Some(id) = id.filter(|id| crate::agent_transcript::is_valid_session_id(id)) else {
            continue;
        };
        match &found {
            Some(existing) if *existing != id => return None,
            _ => found = Some(id),
        }
    }
    found
}

/// The live Ferryx session whose agent is already running omo conversation `provider_id`.
///
/// `sessions` are the daemon's live local sessions as (Ferryx session id, PTY pid). The daemon
/// records a conversation claim only for agents it spawned as a resume, so an omo started by hand
/// in a shell, or carried over from an older daemon by handover, owns its conversation without a
/// claim. Reconnecting a pane to that conversation would then start a second agent on the same
/// transcript. This finds the owner from the process table: an omo process carrying one of these
/// `FERRYX_SESSION_ID`s whose `--session` argument or `PI_SESSION_FILE` names the conversation,
/// else a descendant of a session's PTY holding that conversation's transcript open. Two owners
/// answer `None` rather than a guess.
pub(crate) fn omo_conversation_owner(provider_id: &str, sessions: &[(String, u32)]) -> Option<String> {
    if sessions.is_empty() || !crate::agent_transcript::is_valid_session_id(provider_id) {
        return None;
    }
    let ids: HashSet<&str> = sessions.iter().map(|(id, _)| id.as_str()).collect();
    if let Some(owner) = process_environment_table()
        .and_then(|table| omo_conversation_owner_in_table(&table, provider_id, &ids))
    {
        return Some(owner);
    }
    omo_conversation_owner_by_open_files(provider_id, sessions)
}

fn omo_conversation_owner_in_table(
    table: &str,
    provider_id: &str,
    ids: &HashSet<&str>,
) -> Option<String> {
    let mut owner: Option<&str> = None;
    for line in table.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(session) = tokens
            .iter()
            .filter_map(|token| token.strip_prefix("FERRYX_SESSION_ID="))
            .find(|id| ids.contains(id))
        else {
            continue;
        };
        let by_arg = tokens
            .windows(2)
            .any(|pair| pair[0] == "--session" && pair[1] == provider_id);
        let by_file = tokens
            .iter()
            .filter_map(|token| token.strip_prefix("PI_SESSION_FILE="))
            .any(|path| extract_session_id_from_path("omo", path).as_deref() == Some(provider_id));
        if !(by_arg || by_file) {
            continue;
        }
        match owner {
            Some(existing) if existing != session => return None,
            _ => owner = Some(session),
        }
    }
    owner.map(str::to_string)
}

#[cfg(windows)]
fn omo_conversation_owner_by_open_files(_provider_id: &str, _sessions: &[(String, u32)]) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn omo_conversation_owner_by_open_files(provider_id: &str, sessions: &[(String, u32)]) -> Option<String> {
    let entries = process_table_entries()?;
    let owner_by_pid = session_by_descendant_pid(&entries, sessions);
    if owner_by_pid.is_empty() {
        return None;
    }
    let pids = owner_by_pid.keys().map(u32::to_string).collect::<Vec<_>>().join(",");
    let lsof = resolve_tool("lsof")?;
    // lsof exits non-zero when any listed pid is gone, so its stdout is read regardless.
    let output = crate::util::no_window_command(lsof)
        .args(["-a", "-p", &pids, "-Fpn"])
        .output()
        .ok()?;
    omo_conversation_owner_in_lsof(&String::from_utf8_lossy(&output.stdout), provider_id, &owner_by_pid)
}

#[cfg(any(not(windows), test))]
fn session_by_descendant_pid<'a>(
    entries: &[(u32, u32, String)],
    sessions: &'a [(String, u32)],
) -> HashMap<u32, &'a str> {
    let mut children = HashMap::<u32, Vec<u32>>::new();
    for (pid, ppid, _) in entries {
        children.entry(*ppid).or_default().push(*pid);
    }
    let mut owner_by_pid = HashMap::new();
    for (session, root) in sessions {
        let mut queue = VecDeque::from([*root]);
        while let Some(pid) = queue.pop_front() {
            if owner_by_pid.insert(pid, session.as_str()).is_some() {
                continue;
            }
            if let Some(descendants) = children.get(&pid) {
                queue.extend(descendants.iter().copied());
            }
        }
    }
    owner_by_pid
}

#[cfg(any(not(windows), test))]
fn omo_conversation_owner_in_lsof(
    stdout: &str,
    provider_id: &str,
    owner_by_pid: &HashMap<u32, &str>,
) -> Option<String> {
    let mut current: Option<&str> = None;
    let mut owner: Option<&str> = None;
    for line in stdout.lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current = pid.parse().ok().and_then(|pid| owner_by_pid.get(&pid).copied());
            continue;
        }
        let (Some(session), Some(path)) = (current, line.strip_prefix('n')) else {
            continue;
        };
        if extract_session_id_from_path("omo", path).as_deref() != Some(provider_id) {
            continue;
        }
        match owner {
            Some(existing) if existing != session => return None,
            _ => owner = Some(session),
        }
    }
    owner.map(str::to_string)
}

// One line per process: its arguments followed by its environment, as `ps -E` prints them.
#[cfg(all(not(target_os = "linux"), not(windows)))]
fn process_environment_table() -> Option<String> {
    let ps = resolve_tool("ps")?;
    let stdout = probe_stdout(
        "ps",
        crate::util::no_window_command(ps).args(["-axwwE", "-o", "command="]),
    )?;
    String::from_utf8(stdout).ok()
}

#[cfg(target_os = "linux")]
fn process_environment_table() -> Option<String> {
    let mut table = String::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let path = entry.path();
        let (Ok(cmdline), Ok(environ)) = (
            std::fs::read(path.join("cmdline")),
            std::fs::read(path.join("environ")),
        ) else {
            continue;
        };
        table.push_str(&String::from_utf8_lossy(&cmdline).replace('\0', " "));
        table.push(' ');
        table.push_str(&String::from_utf8_lossy(&environ).replace('\0', " "));
        table.push('\n');
    }
    Some(table)
}

#[cfg(windows)]
fn process_environment_table() -> Option<String> {
    tracing::debug!("agent session discovery: environment discovery unavailable on Windows");
    None
}

// procps-ng ps rejects BSD's `-E`, so Linux reads the target process's
// NUL-separated /proc environment block instead of listing it through ps.
#[cfg(target_os = "linux")]
fn process_environment_output(pid: u32) -> Option<String> {
    let bytes = match std::fs::read(format!("/proc/{pid}/environ")) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::debug!(pid, %error, "agent session discovery: /proc environ unreadable");
            return None;
        }
    };
    if bytes.is_empty() {
        return None;
    }
    // session_id_from_env_output tokenizes on whitespace, as it does for ps -E output.
    Some(String::from_utf8_lossy(&bytes).replace('\0', " "))
}

// Windows has neither /proc nor a BSD `ps -E`, so environment-based discovery is
// unavailable there and callers fall back to the process table and session
// directories; the None is logged rather than passed off as an empty environment.
#[cfg(windows)]
fn process_environment_output(pid: u32) -> Option<String> {
    tracing::debug!(
        pid,
        "agent session discovery: environment discovery unavailable on Windows"
    );
    None
}

#[cfg(all(not(target_os = "linux"), not(windows)))]
fn process_environment_output(pid: u32) -> Option<String> {
    let ps = resolve_tool("ps")?;
    let stdout = probe_stdout(
        "ps",
        crate::util::no_window_command(ps).args(["-Eww", "-p", &pid.to_string()]),
    )?;
    String::from_utf8(stdout).ok()
}

fn session_id_from_env_output(agent_type: &str, stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .filter_map(|part| part.strip_prefix("PI_SESSION_FILE="))
        .find_map(|path| extract_session_id_from_path(agent_type, path))
}

// lsof has no Windows counterpart, so the handle walk degrades to a logged None
// there instead of spawning a binary that cannot exist.
#[cfg(windows)]
fn lsof_session_id(pid: u32, _agent_type: &str) -> Option<String> {
    tracing::debug!(
        pid,
        "agent session discovery: lsof handle discovery unavailable on Windows"
    );
    None
}

#[cfg(not(windows))]
fn lsof_session_id(pid: u32, agent_type: &str) -> Option<String> {
    // Linux can name a process's open files without lsof, which minimal hosts (Arch, containers)
    // often lack: every /proc/<pid>/fd entry is a symlink to the open path.
    #[cfg(target_os = "linux")]
    if let Some(id) = proc_fd_session_id(std::path::Path::new(&format!("/proc/{pid}/fd")), agent_type) {
        return Some(id);
    }
    let lsof = resolve_tool("lsof")?;
    let stdout = probe_stdout(
        "lsof",
        crate::util::no_window_command(lsof).args(["-a", "-p", &pid.to_string(), "-Fn"]),
    )?;
    String::from_utf8(stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .find_map(|path| extract_session_id_from_path(agent_type, path))
}

/// The session named by any open file of a process, read from its `/proc/<pid>/fd` symlinks.
#[cfg(any(target_os = "linux", test))]
fn proc_fd_session_id(fd_dir: &std::path::Path, agent_type: &str) -> Option<String> {
    std::fs::read_dir(fd_dir)
        .ok()?
        .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
        .find_map(|target| extract_session_id_from_path(agent_type, &target.to_string_lossy()))
}

pub(crate) fn extract_session_id_from_path(agent_type: &str, path: &str) -> Option<String> {
    let agent_type = if agent_type == "cursor-agent" {
        "cursor"
    } else {
        agent_type
    };
    let normalized = path.replace('\\', "/");
    if agent_type.eq_ignore_ascii_case("gjc") {
        return gjc_session_id_from_path(&normalized);
    }
    match agent_type {
        "grok" => return grok_session_id_from_path(&normalized),
        "droid" => return droid_session_id_from_path(&normalized),
        "codex" => return codex_session_id_from_path(&normalized),
        "cline" => return cline_session_id_from_path(&normalized),
        // omp writes `<agentDir>/sessions/<cwd-slug>/<ts>_<UUIDv7>.jsonl`, agentDir defaulting to
        // `~/.omp/agent`; a named profile uses `~/.omp/profiles/<name>/agent`.
        "omp" => {
            let in_default = normalized.contains("/.omp/agent/sessions/");
            let in_profile = normalized.contains("/.omp/profiles/")
                && normalized.contains("/agent/sessions/");
            return if in_default || in_profile {
                uuid_from_session_path(&normalized)
            } else {
                None
            };
        }
        _ => {}
    }
    let markers: &[&str] = match agent_type {
        "claude" => &["/.claude/projects/"],
        "codex" => &["/.codex/sessions/"],
        "copilot" => &["/.copilot/session-state/"],
        "cursor" => &["/.cursor/chats/"],
        "kimi" => &["/.kimi/sessions/"],
        // omo writes its own store under `~/.omo/agent/sessions/` today and used the flat
        // `~/.omo/sessions/` before that. Both are this reader's own store - the transcript
        // reader and the resume-cwd search scan both - so both spellings identify an omo
        // transcript, and neither is a guess at another program's file.
        "omo" => &["/.omo/agent/sessions/", "/.omo/sessions/"],
        // The measured pi is senpi (facts/local.md:181): its store is
        // `<agentDir>/sessions/--<cwd slug>--/<ts>_<uuid>.jsonl` with agentDir
        // `$SENPI_CODING_AGENT_DIR`, else `~/.senpi/agent` (facts/local-1.md:341-344). Upstream pi
        // keeps `~/.pi/`.
        "pi" => {
            return pi_session_id_from_path(&normalized, |key| env::var_os(key));
        }
        "antigravity" => &["/.gemini/antigravity-cli/conversations/"],
        _ => return None,
    };
    // A provider may own more than one store spelling (omo does, above), so this is a set of
    // accepted roots rather than one string.
    if !markers.iter().any(|marker| normalized.contains(*marker)) {
        return None;
    }
    uuid_from_session_path(&normalized)
}

const PI_STORE_MARKERS: [&str; 2] = ["/.pi/", "/.senpi/agent/sessions/"];

fn pi_session_id_from_path(
    normalized: &str,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> Option<String> {
    let in_marker = PI_STORE_MARKERS.iter().any(|marker| normalized.contains(*marker));
    let in_env_root = senpi_agent_dir(&get_env).is_some_and(|dir| {
        let root = format!(
            "{}/sessions/",
            dir.to_string_lossy().replace('\\', "/").trim_end_matches('/')
        );
        normalized.starts_with(&root)
    });
    if in_marker || in_env_root {
        uuid_from_session_path(normalized)
    } else {
        None
    }
}

/// `SENPI_CODING_AGENT_DIR`, the agent dir senpi uses instead of `~/.senpi/agent`.
fn senpi_agent_dir(get_env: &impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    get_env("SENPI_CODING_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Every pi session root: `$SENPI_CODING_AGENT_DIR/sessions` when set, senpi's default
/// `~/.senpi/agent/sessions`, and upstream pi's `~/.pi/agent/sessions`. The override is the
/// daemon's own value, which a pane may not share, so the default stays searched beside it.
fn pi_session_roots(get_env: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let home = get_env("HOME")
        .filter(|value| !value.is_empty())
        .or_else(|| get_env("USERPROFILE").filter(|value| !value.is_empty()))
        .map(PathBuf::from);
    let mut roots: Vec<PathBuf> = Vec::new();
    let candidates = senpi_agent_dir(&get_env)
        .into_iter()
        .chain(home.iter().flat_map(|home| [home.join(".senpi/agent"), home.join(".pi/agent")]))
        .map(|dir| dir.join("sessions"));
    for root in candidates {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

fn uuid_from_session_path(path: &str) -> Option<String> {
    path.split(|ch: char| !ch.is_ascii_hexdigit() && ch != '-')
        .find(|part| {
            part.len() == 36
                && part.as_bytes().get(8) == Some(&b'-')
                && part.as_bytes().get(13) == Some(&b'-')
                && part.as_bytes().get(18) == Some(&b'-')
                && part.as_bytes().get(23) == Some(&b'-')
        })
        .map(str::to_string)
}

/// The path components below `marker`, or `None` when the path is outside that store.
fn components_after_marker<'a>(path: &'a str, marker: &str) -> Option<Vec<&'a str>> {
    let start = path.find(marker)? + marker.len();
    Some(path[start..].split('/').filter(|part| !part.is_empty()).collect())
}

// codex writes `~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-<yyyy>-<mm>-<dd>T<hh>-<mm>-<ss>-<id>.jsonl`
// (measured: cli/fixtures/agent_hook/codex-*.json). The timestamp is glued to the id with `-`, so
// the generic UUID scan reads one 45-character run and finds nothing; the id is the file stem's
// last 36 characters.
fn codex_session_id_from_path(path: &str) -> Option<String> {
    let parts = components_after_marker(path, "/.codex/sessions/")?;
    let stem = parts.last()?.strip_suffix(".jsonl")?.strip_prefix("rollout-")?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    let separated = stem.len() == 36 || stem.as_bytes().get(stem.len() - 37) == Some(&b'-');
    (separated && is_valid_uuid(id)).then(|| id.to_string())
}

// grok keeps one directory per session: `~/.grok/sessions/<url-encoded cwd>/<id>/<file>`, where
// grok-minted ids are UUIDv7. The id is the directory directly under the cwd group, so a file
// beside the groups (the global `session_search.sqlite`) carries none.
fn grok_session_id_from_path(path: &str) -> Option<String> {
    let parts = components_after_marker(path, "/.grok/sessions/")?;
    match parts.as_slice() {
        [_group, id, _file, ..] if is_valid_uuid(id) => Some((*id).to_string()),
        _ => None,
    }
}

// droid writes `~/.factory/sessions/<cwd with / replaced by ->/<uuid>.jsonl` plus a sibling
// `<uuid>.settings.json`; the cwd slug may itself contain UUID-shaped text, so only the file
// name is read.
fn droid_session_id_from_path(path: &str) -> Option<String> {
    let parts = components_after_marker(path, "/.factory/sessions/")?;
    let [_slug, file] = parts.as_slice() else {
        return None;
    };
    let id = file
        .strip_suffix(".settings.json")
        .or_else(|| file.strip_suffix(".jsonl"))?;
    is_valid_uuid(id).then(|| id.to_string())
}

// cline keeps a directory per session, `~/.cline/data/sessions/<id>/<id>.json` and
// `<id>.messages.json`, with ids shaped `<epoch-ms>_<5 [a-z0-9]>`, not UUIDs.
fn cline_session_id_from_path(path: &str) -> Option<String> {
    let parts = components_after_marker(path, "/.cline/data/sessions/")?;
    let [id, file] = parts.as_slice() else {
        return None;
    };
    let stem = file
        .strip_suffix(".messages.json")
        .or_else(|| file.strip_suffix(".json"))?;
    (stem == *id && is_cline_session_id(id)).then(|| (*id).to_string())
}

fn is_cline_session_id(id: &str) -> bool {
    let Some((millis, suffix)) = id.split_once('_') else {
        return false;
    };
    !millis.is_empty()
        && millis.bytes().all(|b| b.is_ascii_digit())
        && suffix.len() == 5
        && suffix
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn is_valid_uuid(val: &str) -> bool {
    let trimmed = val.trim();
    if trimmed.len() != 36 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
}

fn normalize_lookup_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let trimmed = replaced.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_path_prefix(prefix: &str, target: &str) -> bool {
    let p = normalize_lookup_path(prefix);
    let t = normalize_lookup_path(target);
    if t == p {
        return true;
    }
    if let Some(rest) = t.strip_prefix(&p) {
        return rest.starts_with('/');
    }
    false
}

pub(crate) fn find_antigravity_conversation_id(json_str: &str, cwd: &str) -> Option<String> {
    let map: HashMap<String, String> = serde_json::from_str(json_str).ok()?;
    let target_norm = normalize_lookup_path(cwd);

    // 1. Exact match (comparing normalized path strings)
    for (key, id) in &map {
        if normalize_lookup_path(key) == target_norm && is_valid_uuid(id) {
            return Some(id.trim().to_string());
        }
    }

    // 2. Prefix fallback: find key that is a path prefix of cwd with greatest normalized length
    let mut best_match: Option<(&String, &String, usize)> = None;
    for (key, id) in &map {
        if is_path_prefix(key, cwd) && is_valid_uuid(id) {
            let key_len = normalize_lookup_path(key).len();
            match best_match {
                None => best_match = Some((key, id, key_len)),
                Some((_, _, best_len)) => {
                    if key_len > best_len {
                        best_match = Some((key, id, key_len));
                    }
                }
            }
        }
    }

    best_match.map(|(_, id, _)| id.trim().to_string())
}

fn antigravity_session_id(agent_pid: Option<u32>, root_pid: u32) -> Option<String> {
    let target_cwd = agent_pid
        .and_then(crate::ipc::terminal::process_cwd)
        .or_else(|| crate::ipc::terminal::process_cwd(root_pid));

    if let Some(cwd) = target_cwd {
        let normalized_cwd = crate::daemon::server::normalize_process_cwd(&cwd);
        let cwd_str = normalized_cwd.to_string_lossy();
        if let Some(home) = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
        {
            let cache_path = home.join(".gemini/antigravity-cli/cache/last_conversations.json");
            if let Ok(contents) = std::fs::read_to_string(&cache_path) {
                if let Some(id) = find_antigravity_conversation_id(&contents, &cwd_str) {
                    return Some(id);
                }
            }
        }
    }

    if let Some(pid) = agent_pid {
        if let Some(id) = lsof_session_id(pid, "antigravity") {
            return Some(id);
        }
    }
    lsof_session_id(root_pid, "antigravity")
}

fn opencode_session_id(agent_pid: Option<u32>, root_pid: u32) -> Option<String> {
    let cwd = agent_pid
        .and_then(crate::ipc::terminal::process_cwd)
        .or_else(|| crate::ipc::terminal::process_cwd(root_pid))?;
    let cwd_str = cwd.to_string_lossy();

    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    {
        let db_path = home.join(".local/share/opencode/opencode.db");
        if db_path.exists() {
            if let Some(id) = opencode_session_id_from_sqlite(&db_path, &cwd_str) {
                return Some(id);
            }
        }
    }

    opencode_session_id_from_cli(&cwd_str)
}

fn is_valid_opencode_session_id(id: &str) -> bool {
    let trimmed = id.trim();
    if !trimmed.starts_with("ses_") || trimmed.len() < 10 || trimmed.len() > 128 {
        return false;
    }
    trimmed["ses_".len()..]
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn opencode_session_id_from_sqlite(db_path: &Path, cwd: &str) -> Option<String> {
    let escaped_cwd = cwd.replace('\'', "''");
    let query = format!(
        "SELECT id FROM session WHERE directory = '{}' ORDER BY time_updated DESC LIMIT 1;",
        escaped_cwd
    );
    let sqlite3 = resolve_tool("sqlite3")?;
    let stdout = probe_stdout(
        "sqlite3",
        crate::util::no_window_command(sqlite3).arg(db_path).arg(&query),
    )?;

    let stdout = String::from_utf8(stdout).ok()?;
    let id = stdout.trim().lines().next()?.trim();
    if is_valid_opencode_session_id(id) {
        Some(id.to_string())
    } else {
        None
    }
}

fn opencode_session_id_from_cli(cwd: &str) -> Option<String> {
    let opencode = resolve_tool("opencode")?;
    let stdout = probe_stdout(
        "opencode",
        crate::util::no_window_command(opencode)
            .args(["session", "list", "--format", "json", "-n", "10"]),
    )?;

    parse_opencode_session_list_json(&String::from_utf8(stdout).ok()?, cwd)
}

pub(crate) fn parse_opencode_session_list_json(json_str: &str, cwd: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct OpencodeSessionItem {
        id: Option<String>,
        directory: Option<String>,
    }

    let items: Vec<OpencodeSessionItem> = serde_json::from_str(json_str).ok()?;
    for item in items {
        if let (Some(id), Some(dir)) = (item.id, item.directory) {
            if (is_path_prefix(&dir, cwd) || is_path_prefix(cwd, &dir))
                && is_valid_opencode_session_id(&id)
            {
                return Some(id);
            }
        }
    }
    None
}

fn encode_pi_safe_path(cwd: &str) -> String {
    let trimmed = cwd.trim_matches(|c| c == '/' || c == '\\');
    let clean = trimmed.replace(['/', '\\', ':'], "-");
    format!("--{}--", clean)
}

fn pi_session_id(agent_pid: Option<u32>, root_pid: u32) -> Option<String> {
    if let Some(pid) = agent_pid {
        if let Some(id) = omo_session_id_from_environment(pid, "pi") {
            return Some(id);
        }
        if let Some(id) = lsof_session_id(pid, "pi") {
            return Some(id);
        }
    }

    if let Some(id) = lsof_session_id(root_pid, "pi") {
        return Some(id);
    }

    let cwd = agent_pid
        .and_then(crate::ipc::terminal::process_cwd)
        .or_else(|| crate::ipc::terminal::process_cwd(root_pid))?;
    let cwd_str = cwd.to_string_lossy();
    pi_session_id_from_session_dirs(&pi_session_roots(|key| env::var_os(key)), &cwd_str)
}

/// The newest transcript for `cwd` across every pi session root.
fn pi_session_id_from_session_dirs(roots: &[PathBuf], cwd: &str) -> Option<String> {
    let safe_dir = encode_pi_safe_path(cwd);
    let mut entries: Vec<_> = roots
        .iter()
        .filter_map(|root| std::fs::read_dir(root.join(&safe_dir)).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "jsonl")
                .unwrap_or(false)
        })
        .collect();

    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });

    for entry in entries.iter().rev() {
        let path = entry.path();
        let path_str = path.to_string_lossy();
        if let Some(uuid) = uuid_from_session_path(&path_str) {
            return Some(uuid);
        }
    }
    None
}

// GJC (gajae-code) stores managed sessions at
// ~/.gjc/agent/sessions/<v2-scope>/<timestamp>_<sessionId>.jsonl; XDG data homes
// flatten the `agent/` segment (optionally under profiles/<profile>/). gjc mints
// session IDs as UUIDv7 (`Bun.randomUUIDv7()`); the ID is read from the file name,
// whose `<timestamp>_` prefix the parser strips.
fn gjc_session_id_from_path(path: &str) -> Option<String> {
    let in_agent_layout = path.contains("/.gjc/agent/sessions/");
    let in_xdg_layout = match path.find("/gjc/") {
        Some(start) => path[start..].contains("/sessions/"),
        None => false,
    };
    if !in_agent_layout && !in_xdg_layout {
        return None;
    }
    let file_name = path.rsplit('/').next()?;
    let id = file_name.strip_suffix(".jsonl")?.rsplit('_').next()?;
    let len = id.len();
    if (8..=64).contains(&len) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Some(id.to_string());
    }
    None
}

pub fn detect_agents(names: &[String]) -> Vec<AgentDetection> {
    names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| {
            let path = resolve_discovered_binary(name);
            AgentDetection {
                name: name.to_string(),
                available: path.is_some(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

/// The process PATH merged with the login-shell PATH once background discovery has finished.
///
/// Never blocks: the first call starts discovery on its own thread and returns the process PATH
/// until it publishes, so a session spawn on an async path never waits for the user's rc chain.
pub(crate) fn search_paths() -> Vec<PathBuf> {
    merged_search_paths(login_path_cache().current())
}

/// Starts login-shell PATH discovery without waiting for it, so later lookups find it ready.
pub fn prewarm_login_path() {
    login_path_cache();
}

/// Resolves `name` like `resolve_binary`, but when it is missing while discovery is still
/// running, waits (bounded) for the login-shell PATH and tries again. Only for callers already
/// on a blocking thread whose answer would otherwise be a false "not installed".
pub(crate) fn resolve_discovered_binary(name: &str) -> Option<PathBuf> {
    resolve_discovered_binary_within(name, LOGIN_PATH_TIMEOUT + std::time::Duration::from_secs(1))
}

/// `resolve_discovered_binary` with a caller-chosen cap on the wait for discovery, for callers
/// inside a request budget (session spawn) that must answer before the client gives up.
pub(crate) fn resolve_discovered_binary_within(
    name: &str,
    bound: std::time::Duration,
) -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(login) = TEST_LOGIN_PATH.with(|slot| slot.borrow().clone()) {
        return resolve_with_cache(name, &login.0, bound);
    }
    resolve_with_cache(name, login_path_cache(), bound)
}

/// Test seam: a login-PATH discovery that the test publishes by hand, or never.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct PendingLoginPath(std::sync::Arc<LoginPathCache>);

#[cfg(test)]
impl PendingLoginPath {
    pub(crate) fn publish(&self, path: Option<OsString>) {
        self.0.publish(path);
    }
}

#[cfg(test)]
thread_local! {
    static TEST_LOGIN_PATH: std::cell::RefCell<Option<PendingLoginPath>> = const { std::cell::RefCell::new(None) };
}

/// Runs `work` with `resolve_discovered_binary_within` on this thread reading `login` instead of
/// the process-wide discovery.
#[cfg(test)]
pub(crate) fn with_login_path<T>(login: &PendingLoginPath, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<PendingLoginPath>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_LOGIN_PATH.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(TEST_LOGIN_PATH.with(|slot| slot.replace(Some(login.clone()))));
    work()
}

fn resolve_with_cache(
    name: &str,
    cache: &LoginPathCache,
    bound: std::time::Duration,
) -> Option<PathBuf> {
    // One snapshot decides: a publish between the lookup and a separate "settled?" read would
    // otherwise answer None for an agent the just-published PATH contains.
    let snapshot = cache.lock().clone();
    if let Some(found) = resolve_binary(name, &merged_search_paths(snapshot.clone().flatten())) {
        return Some(found);
    }
    if snapshot.is_some() {
        return None;
    }
    let login = cache.wait(bound);
    resolve_binary(name, &merged_search_paths(login))
}

fn merged_search_paths(login: Option<OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut push_all = |value: OsString| {
        for dir in env::split_paths(&value) {
            if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    };
    if let Some(value) = env::var_os("PATH") {
        push_all(value);
    }
    if let Some(value) = login {
        push_all(value);
    }
    dirs
}

// Terminals spawn through the user's login shell, so its PATH (Homebrew, nvm,
// mise, ~/.local/bin) is the search space that matches what a launched agent
// would actually resolve against - the GUI process PATH alone misses most of it.
//
// The bound exists because of a measured hang: an interactive login shell sources
// the user's whole rc chain, and on a host whose rc chain runs `pyenv rehash`
// (pyenv's 60 s lock wait) `zsh -lic true` measured 60.41 s wall. Discovery now
// runs once per process on its own thread, so this bound only caps how long that
// thread waits; no session spawn or Tokio worker waits on it.
const LOGIN_PATH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Marker printed right before the PATH, so rc-chain output (banners, `fortune`) on stdout
/// cannot be mistaken for it.
const LOGIN_PATH_MARKER: &str = "__FERRYX_LOGIN_PATH__";

#[derive(Default)]
struct LoginPathCache {
    // None while discovery runs; Some(result) once it published.
    state: std::sync::Mutex<Option<Option<OsString>>>,
    ready: std::sync::Condvar,
}

impl LoginPathCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Option<OsString>>> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn publish(&self, value: Option<OsString>) {
        *self.lock() = Some(value);
        self.ready.notify_all();
    }

    fn current(&self) -> Option<OsString> {
        self.lock().clone().flatten()
    }

    fn wait(&self, bound: std::time::Duration) -> Option<OsString> {
        let guard = self.lock();
        let (guard, _) = self
            .ready
            .wait_timeout_while(guard, bound, |state| state.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone().flatten()
    }
}

fn login_path_cache() -> &'static LoginPathCache {
    static CACHE: OnceLock<LoginPathCache> = OnceLock::new();
    static START: std::sync::Once = std::sync::Once::new();
    let cache = CACHE.get_or_init(LoginPathCache::default);
    START.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("ferryx-login-path".into())
            .spawn(move || cache.publish(discover_login_path()));
        if let Err(error) = spawned {
            tracing::debug!(%error, "login PATH discovery: could not start its thread");
            cache.publish(None);
        }
    });
    cache
}

fn discover_login_path() -> Option<OsString> {
    let Some(shell) = crate::terminal::shell::login_shell_program() else {
        tracing::debug!("login PATH discovery: no SHELL and no passwd shell");
        return None;
    };
    discover_login_path_with(&shell, LOGIN_PATH_TIMEOUT)
}

/// The shell family decides how to ask for an interactive login shell and how to print PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginShellKind {
    Posix,
    Csh,
    Fish,
    Nushell,
}

impl LoginShellKind {
    fn of(shell: &str) -> Self {
        let name = shell.rsplit('/').next().unwrap_or(shell).trim_start_matches('-');
        match name {
            "csh" | "tcsh" => Self::Csh,
            "fish" => Self::Fish,
            "nu" | "nushell" => Self::Nushell,
            _ => Self::Posix,
        }
    }

    /// Arguments for the probe. Flags stay separate: `-lic` is rejected by shells that only
    /// accept one bundled flag, and csh/tcsh honor `-l` only as the sole argument, so they get
    /// an interactive shell (which reads ~/.cshrc and ~/.tcshrc, where csh users set PATH).
    fn probe_args(self, printenv: &str) -> Vec<String> {
        let posix = format!("echo {LOGIN_PATH_MARKER}; {printenv} PATH");
        match self {
            Self::Posix => vec!["-l".into(), "-i".into(), "-c".into(), posix],
            Self::Csh => vec!["-i".into(), "-c".into(), posix],
            // fish keeps PATH as a list; join it so the output has the POSIX shape.
            Self::Fish => vec![
                "-l".into(),
                "-i".into(),
                "-c".into(),
                format!("echo {LOGIN_PATH_MARKER}; string join : $PATH"),
            ],
            Self::Nushell => vec![
                "-l".into(),
                "-i".into(),
                "-c".into(),
                format!("print \"{LOGIN_PATH_MARKER}\"; print ($env.PATH | str join \":\")"),
            ],
        }
    }
}

/// Picks the line after the last marker and accepts it only when it is a usable PATH: every
/// non-empty entry absolute and free of control characters, at least one entry present.
fn parse_login_path_output(stdout: &[u8]) -> Result<OsString, &'static str> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines().map(|line| line.trim_end_matches('\r'));
    let mut value = None;
    while let Some(line) = lines.next() {
        if line.trim() == LOGIN_PATH_MARKER {
            value = lines.next();
        }
    }
    let value = value.ok_or("marker missing from shell output")?.trim();
    let entries: Vec<&str> = value.split(':').filter(|entry| !entry.is_empty()).collect();
    if entries.is_empty() {
        return Err("PATH is empty");
    }
    if entries
        .iter()
        .any(|entry| !entry.starts_with('/') || entry.chars().any(char::is_control))
    {
        return Err("PATH has a relative or malformed entry");
    }
    Ok(OsString::from(entries.join(":")))
}

fn printenv_program() -> &'static str {
    ["/usr/bin/printenv", "/bin/printenv"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        // NixOS and similar ship printenv only through PATH, which the rc chain has set by now.
        .unwrap_or("printenv")
}

fn discover_login_path_with(shell: &str, timeout: std::time::Duration) -> Option<OsString> {
    let kind = LoginShellKind::of(shell);
    let mut command = crate::util::no_window_command(shell);
    command
        .args(kind.probe_args(printenv_program()))
        .env("PROMPT_EOL_MARK", "")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        // Never read; piping it could let a chatty rc chain fill the pipe and trip the deadline.
        .stderr(std::process::Stdio::null());
    without_controlling_terminal(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            tracing::debug!(shell, %error, "login PATH discovery: shell failed to spawn");
            return None;
        }
    };
    // Drain stdout on its own thread so a shell that writes more than a pipe buffer cannot
    // block on write until the deadline kills it.
    let Some(mut stdout_pipe) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let drain = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut stdout_pipe, &mut bytes);
        bytes
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            // Kill and reap the direct child so it cannot leak; a grandchild the rc chain
            // already spawned (e.g. `pyenv-rehash`) is orphaned and finishes on its own.
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain.join();
                tracing::debug!(
                    shell,
                    timeout_ms = timeout.as_millis() as u64,
                    unreapable = outcome.is_err(),
                    "login PATH discovery: shell did not finish in time"
                );
                return None;
            }
        }
    };
    let bytes = drain.join().unwrap_or_default();
    if !status.success() {
        tracing::debug!(shell, code = ?status.code(), "login PATH discovery: shell exited non-zero");
        return None;
    }
    match parse_login_path_output(&bytes) {
        Ok(value) => Some(value),
        Err(reason) => {
            tracing::debug!(shell, reason, "login PATH discovery: output rejected");
            None
        }
    }
}

/// Runs the child in a new session with no controlling terminal.
///
/// `-i` turns on job control, and an interactive shell that inherits a terminal makes its own
/// process group that terminal's foreground group. When this process was started from a terminal
/// (the app binary run as a command), the shell took that terminal; once the deadline above killed
/// it, the foreground stayed with a dead group and the user's shell or agent stopped on its next
/// read with "suspended (tty input)". A child with no controlling terminal cannot take one.
#[cfg(unix)]
fn without_controlling_terminal(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;
    // SAFETY:
    // Category: Foreign Function Interface (FFI).
    // Invariant: `pre_exec` runs between fork and exec, where only async-signal-safe calls are
    // legal; `setsid` qualifies - it takes no pointers, allocates nothing, and returns a value
    // instead of unwinding. A failure is returned as an `io::Error`, which fails the spawn.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Windows consoles have no foreground process group for a child to take.
#[cfg(not(unix))]
fn without_controlling_terminal(_command: &mut std::process::Command) {}

pub(crate) fn resolve_binary(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    resolve_binary_with_env(
        name,
        search_paths,
        cfg!(windows),
        env::var("PATHEXT").ok().as_deref(),
    )
}

fn resolve_binary_with_env(
    name: &str,
    search_paths: &[PathBuf],
    windows: bool,
    pathext: Option<&str>,
) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
        return None;
    }
    if windows {
        // Only extensions with a supported native or command-script launch policy.
        let extensions: Vec<String> = pathext
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .map(|ext| ext.trim().to_ascii_lowercase())
            .filter(|ext| matches!(ext.as_str(), ".com" | ".exe" | ".bat" | ".cmd"))
            .collect();
        let candidates: Vec<String> = if Path::new(name).extension().is_some() {
            if !extensions
                .iter()
                .any(|ext| name.to_ascii_lowercase().ends_with(ext))
            {
                return None;
            }
            vec![name.to_string()]
        } else {
            extensions
                .iter()
                .map(|ext| format!("{name}{ext}"))
                .collect()
        };
        for dir in search_paths {
            for candidate in &candidates {
                // Preserve the directory entry's spelling on both case-sensitive and
                // case-insensitive filesystems (the returned path is displayed in the UI).
                if let Ok(entries) = std::fs::read_dir(dir) {
                    if let Some(path) = entries
                        .filter_map(Result::ok)
                        .find(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .eq_ignore_ascii_case(candidate)
                                && entry.path().is_file()
                        })
                        .map(|entry| entry.path())
                    {
                        return Some(path);
                    }
                }
            }
        }
        return None;
    }
    search_paths
        .iter()
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

// Discovery probes system tools by bare name so PATH decides the location on
// every platform (PATHEXT covers npm .cmd shims on Windows); a missing tool
// yields None instead of a failed spawn.
fn resolve_tool(name: &str) -> Option<PathBuf> {
    let resolved = resolve_binary(name, &search_paths());
    if resolved.is_none() {
        tracing::debug!(tool = name, "agent session discovery: tool missing from PATH");
    }
    resolved
}

// Probe failures used to return None silently, which hid every platform
// difference; log them and keep the None-on-failure contract.
fn probe_stdout(tool: &str, command: &mut std::process::Command) -> Option<Vec<u8>> {
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            tracing::debug!(tool, %error, "agent session discovery: probe failed to spawn");
            return None;
        }
    };
    if !output.status.success() {
        tracing::debug!(
            tool,
            code = ?output.status.code(),
            "agent session discovery: probe exited non-zero"
        );
        return None;
    }
    Some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The login-shell probe must not be able to take the terminal of the process that ran it.
    /// A child in its own session has no controlling terminal, so `/dev/tty` cannot be opened.
    #[cfg(unix)]
    #[test]
    fn login_shell_probe_child_has_no_controlling_terminal() {
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "if (exec 3</dev/tty) 2>/dev/null; then echo has-tty; else echo no-tty; fi; ps -o pgid= -p $$"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        without_controlling_terminal(&mut command);
        let output = command.output().expect("spawn probe child");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.starts_with("no-tty"), "child still has a controlling terminal: {text}");
        let own_group = unsafe { libc::getpgrp() } as u32;
        let child_group: u32 = text
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().last())
            .and_then(|pgid| pgid.parse().ok())
            .expect("child pgid");
        assert_ne!(child_group, own_group, "child must lead its own session and group");
    }

    #[test]
    fn omo_session_id_in_table_uses_the_owning_process_only() {
        let table = concat!(
            "bun /x/senpi/dist/bundle/cli.js --session 01a0-mine FERRYX_SESSION_ID=pty-a HOME=/h\n",
            "bun /x/senpi/dist/bundle/cli.js --session 01a0-other FERRYX_SESSION_ID=pty-b\n",
            "bun /x/senpi/dist/bundle/cli.js FERRYX_SESSION_ID=pty-c PI_SESSION_FILE=/h/.omo/sessions/--p--/2026_01a0d650-db7a-7817-a21b-7be552a81e89.jsonl\n",
            "/bin/zsh -l FERRYX_SESSION_ID=pty-a\n",
        );
        assert_eq!(omo_session_id_in_table(table, "pty-a").as_deref(), Some("01a0-mine"));
        assert_eq!(omo_session_id_in_table(table, "pty-b").as_deref(), Some("01a0-other"));
        assert_eq!(
            omo_session_id_in_table(table, "pty-c").as_deref(),
            Some("01a0d650-db7a-7817-a21b-7be552a81e89")
        );
        assert_eq!(omo_session_id_in_table(table, "pty-"), None, "prefix must not match");
        assert_eq!(omo_session_id_in_table(table, "pty-z"), None);
    }

    #[test]
    fn omo_session_id_in_table_refuses_two_conversations_in_one_session() {
        let table = concat!(
            "bun cli.js --session 01a0-one FERRYX_SESSION_ID=pty-a\n",
            "bun cli.js --session 01a0-two FERRYX_SESSION_ID=pty-a\n",
            "bun cli.js --session 01a0-one FERRYX_SESSION_ID=pty-b\n",
            "bun cli.js --session 01a0-one FERRYX_SESSION_ID=pty-b\n",
        );
        assert_eq!(omo_session_id_in_table(table, "pty-a"), None);
        assert_eq!(omo_session_id_in_table(table, "pty-b").as_deref(), Some("01a0-one"));
    }

    #[test]
    fn omo_conversation_owner_in_table_finds_the_live_session_running_it() {
        let conversation = "01a17077-db89-7bd2-b57b-da531f61b34d";
        let table = format!(
            "mhc --session {conversation} FERRYX_SESSION_ID=pty-a HOME=/h\n\
             bun cli.js FERRYX_SESSION_ID=pty-b PI_SESSION_FILE=/h/.omo/agent/sessions/--p--/2026_01a170ef-9631-78d3-bc77-5e0233e39233.jsonl\n\
             mhc --session {conversation} FERRYX_SESSION_ID=pty-gone\n"
        );
        let ids = HashSet::from(["pty-a", "pty-b"]);
        assert_eq!(
            omo_conversation_owner_in_table(&table, conversation, &ids).as_deref(),
            Some("pty-a")
        );
        assert_eq!(
            omo_conversation_owner_in_table(&table, "01a170ef-9631-78d3-bc77-5e0233e39233", &ids)
                .as_deref(),
            Some("pty-b")
        );
        assert_eq!(
            omo_conversation_owner_in_table(&table, "01a0b028-8789-7683-9220-66d6ca855b24", &ids),
            None
        );
        let both = HashSet::from(["pty-a", "pty-gone"]);
        assert_eq!(omo_conversation_owner_in_table(&table, conversation, &both), None);
    }

    #[test]
    fn omo_conversation_owner_in_lsof_maps_an_open_transcript_to_its_session() {
        let entries = vec![
            (100, 1, "/bin/zsh -l".to_string()),
            (101, 100, "mhc".to_string()),
            (200, 1, "/bin/zsh -l".to_string()),
        ];
        let sessions = vec![("pty-a".to_string(), 100), ("pty-b".to_string(), 200)];
        let owners = session_by_descendant_pid(&entries, &sessions);
        assert_eq!(owners.get(&101), Some(&"pty-a"));
        let stdout = concat!(
            "p101\n",
            "n/h/.omo/sessions/--p--/2026-10-25T02-31-52-201Z_01a170e6-6e89-7900-a2a9-112ec1393d5e.jsonl.lock\n",
            "n/h/.omo/memory/sidecars/2026_01a1713f-5231-7c62-8826-108555b19c46.jsonl.lock\n",
            "p200\n",
            "n/dev/ttys001\n",
        );
        assert_eq!(
            omo_conversation_owner_in_lsof(stdout, "01a170e6-6e89-7900-a2a9-112ec1393d5e", &owners)
                .as_deref(),
            Some("pty-a")
        );
        assert_eq!(
            omo_conversation_owner_in_lsof(stdout, "01a1713f-5231-7c62-8826-108555b19c46", &owners),
            None,
            "a file outside the omo transcript store is not the conversation"
        );
    }

    #[test]
    fn test_agents_detect_existing_and_nonexistent_binaries() {
        let (existing_a, existing_b) = if cfg!(windows) {
            ("cmd", "powershell")
        } else {
            ("sh", "ls")
        };
        let results = detect_agents(&[
            existing_a.into(),
            existing_b.into(),
            "definitely-not-a-real-binary-xyz".into(),
            "".into(),
            "   ".into(),
        ]);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].name, existing_a);
        assert!(results[0].available);
        assert!(results[0].path.is_some());
        assert_eq!(results[1].name, existing_b);
        assert!(results[1].available);
        assert_eq!(results[2].name, "definitely-not-a-real-binary-xyz");
        assert!(!results[2].available);
        assert!(results[2].path.is_none());
    }

    #[test]
    fn login_path_probe_uses_per_shell_forms() {
        let posix = LoginShellKind::of("/bin/zsh").probe_args("/usr/bin/printenv");
        assert_eq!(&posix[..3], ["-l", "-i", "-c"]);
        assert!(posix[3].ends_with("/usr/bin/printenv PATH"));
        assert_eq!(LoginShellKind::of("-bash"), LoginShellKind::Posix);
        let csh = LoginShellKind::of("/bin/tcsh").probe_args("/usr/bin/printenv");
        assert_eq!(&csh[..2], ["-i", "-c"]);
        assert_eq!(LoginShellKind::of("/usr/bin/csh"), LoginShellKind::Csh);
        let fish = LoginShellKind::of("/opt/homebrew/bin/fish").probe_args("printenv");
        assert!(fish[3].contains("string join : $PATH"));
        let nu = LoginShellKind::of("/usr/local/bin/nu").probe_args("printenv");
        assert!(nu[3].contains("str join"));
    }

    #[test]
    fn login_path_output_shapes_parse_per_shell() {
        let marker = LOGIN_PATH_MARKER;
        // zsh/bash: rc-chain chatter before the marker, printenv output after it.
        let zsh = format!("Welcome back\n{marker}\n/opt/homebrew/bin:/usr/bin:/bin\n");
        assert_eq!(
            parse_login_path_output(zsh.as_bytes()).unwrap(),
            "/opt/homebrew/bin:/usr/bin:/bin"
        );
        let bash = format!("{marker}\r\n/home/u/.local/bin:/usr/bin\r\n");
        assert_eq!(parse_login_path_output(bash.as_bytes()).unwrap(), "/home/u/.local/bin:/usr/bin");
        // fish: the joined list; tcsh: printenv output identical to POSIX.
        let fish = format!("{marker}\n/home/u/.cargo/bin:/usr/bin\n");
        assert_eq!(parse_login_path_output(fish.as_bytes()).unwrap(), "/home/u/.cargo/bin:/usr/bin");
        let tcsh = format!("{marker}\n/usr/local/bin:/usr/bin:\n");
        assert_eq!(parse_login_path_output(tcsh.as_bytes()).unwrap(), "/usr/local/bin:/usr/bin");
        // nushell prints after its banner; an echoed marker inside the banner is superseded.
        let nu = format!("{marker} banner\n{marker}\n/home/u/.nix-profile/bin:/usr/bin\n");
        assert_eq!(
            parse_login_path_output(nu.as_bytes()).unwrap(),
            "/home/u/.nix-profile/bin:/usr/bin"
        );
    }

    #[test]
    fn login_path_output_rejects_malformed_values() {
        let marker = LOGIN_PATH_MARKER;
        assert!(parse_login_path_output(b"/usr/bin:/bin\n").is_err(), "no marker");
        assert!(parse_login_path_output(format!("{marker}\n").as_bytes()).is_err(), "empty");
        let relative = format!("{marker}\nbin:/usr/bin\n");
        assert!(parse_login_path_output(relative.as_bytes()).is_err());
        let garbage = format!("{marker}\nzsh: command not found: printenv\n");
        assert!(parse_login_path_output(garbage.as_bytes()).is_err());
        let control = format!("{marker}\n/usr/bin:/b\u{1b}[0min\n");
        assert!(parse_login_path_output(control.as_bytes()).is_err());
    }

    /// The probe runs a real POSIX shell end to end and returns the PATH it printed.
    #[cfg(unix)]
    #[test]
    fn login_path_discovery_reads_a_real_posix_shell() {
        let path = discover_login_path_with("/bin/sh", std::time::Duration::from_secs(10))
            .expect("/bin/sh reports a PATH");
        assert!(env::split_paths(&path).all(|dir| dir.is_absolute()), "{path:?}");
    }

    /// A shell that prints garbage yields no login PATH, so callers keep the process PATH.
    #[cfg(unix)]
    #[test]
    fn login_path_discovery_rejects_a_shell_printing_garbage() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let shell = root.path().join("garbage-sh");
        std::fs::write(&shell, format!("#!/bin/sh\necho {LOGIN_PATH_MARKER}\necho 'not a path'\n")).unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        let shell = shell.to_string_lossy().into_owned();
        assert_eq!(discover_login_path_with(&shell, std::time::Duration::from_secs(10)), None);
    }

    /// An agent only on the login PATH resolves once discovery publishes, even when the lookup
    /// started before it did (the daemon-start race of a machine agent resume).
    #[cfg(unix)]
    #[test]
    fn discovered_binary_waits_for_a_login_path_published_later() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let name = format!("ferryx-login-only-agent-{}", std::process::id());
        let agent = root.path().join(&name);
        std::fs::write(&agent, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cache = std::sync::Arc::new(LoginPathCache::default());
        let publisher = {
            let cache = std::sync::Arc::clone(&cache);
            let dir = root.path().as_os_str().to_owned();
            std::thread::spawn(move || cache.publish(Some(dir)))
        };
        let found = resolve_with_cache(&name, &cache, std::time::Duration::from_secs(30));
        publisher.join().unwrap();
        assert_eq!(found, Some(agent));
    }

    /// Discovery that never publishes cannot hold the caller past its bound.
    #[test]
    fn discovered_binary_wait_is_bounded() {
        let cache = LoginPathCache::default();
        let started = std::time::Instant::now();
        let found = resolve_with_cache(
            "ferryx-agent-that-does-not-exist",
            &cache,
            std::time::Duration::from_millis(200),
        );
        assert_eq!(found, None);
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    }

    #[test]
    fn test_agents_detect_rejects_path_shaped_names() {
        let results = detect_agents(&["/bin/sh".into(), "../sh".into()]);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| !r.available));
    }

    #[test]
    fn test_agents_detect_does_not_spawn_which() {
        let src = include_str!("agents.rs");
        assert!(
            !src.contains("Command::new(\"which\")"),
            "agent detection must scan PATH in-process"
        );
    }

    #[test]
    fn test_agent_detection_serde_camel_case() {
        let detection = AgentDetection {
            name: "claude".into(),
            available: true,
            path: None,
        };
        let json = serde_json::to_string(&detection).expect("serialize");
        assert_eq!(json, r#"{"name":"claude","available":true}"#);

        let decoded: AgentDetection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, detection);
    }

    #[test]
    fn session_discovery_extracts_supported_paths_only() {
        assert_eq!(
            extract_session_id_from_path(
                "claude",
                "/Users/me/.claude/projects/repo/11111111-2222-3333-4444-555555555555.jsonl",
            ),
            Some("11111111-2222-3333-4444-555555555555".to_string())
        );
        assert_eq!(extract_session_id_from_path("opencode", "/tmp/id"), None);
    }

    #[test]
    fn session_discovery_accepts_both_omo_store_spellings() {
        let id = "01a112e9-02b1-7857-b11d-967060362583";
        // The current layout: the agent dir carries a `sessions` segment.
        assert_eq!(
            extract_session_id_from_path(
                "omo",
                &format!("/Users/me/.omo/agent/sessions/--repo--/2026-10-07T09-00-00.000Z_{id}.jsonl"),
            ),
            Some(id.to_string())
        );
        // The legacy flat layout, which the same reader and the resume-cwd search both scan.
        assert_eq!(
            extract_session_id_from_path(
                "omo",
                &format!("/Users/me/.omo/sessions/--repo--/2026-10-07T09-00-00.000Z_{id}.jsonl"),
            ),
            Some(id.to_string())
        );
        // A Windows spelling normalizes its separators before the marker test.
        assert_eq!(
            extract_session_id_from_path(
                "omo",
                &format!(r"C:\Users\me\.omo\agent\sessions\--repo--\2026-10-07T09-00-00.000Z_{id}.jsonl"),
            ),
            Some(id.to_string())
        );
        // The reader's own store root is still required: an unrelated .omo path is not a transcript.
        assert_eq!(
            extract_session_id_from_path(
                "omo",
                &format!("/Users/me/.omo/cache/{id}.jsonl"),
            ),
            None
        );
        // And a file the omo store holds but whose name carries no UUID is not one either.
        assert_eq!(
            extract_session_id_from_path(
                "omo",
                "/Users/me/.omo/agent/sessions/--repo--/2026-10-07T09-00-00.000Z_not-a-uuid.jsonl",
            ),
            None
        );
    }

    #[test]
    fn session_discovery_extracts_gjc_short_hex_ids() {
        assert_eq!(
            extract_session_id_from_path(
                "gjc",
                "/Users/me/.gjc/agent/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_1f9d2a6b9c0d1234.jsonl",
            ),
            Some("1f9d2a6b9c0d1234".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "gjc",
                "/Users/me/.local/share/gjc/profiles/work/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_1f9d2a6b9c0d1234.jsonl",
            ),
            Some("1f9d2a6b9c0d1234".to_string())
        );
        assert_eq!(
            extract_session_id_from_path("gjc", "/tmp/unrelated_1f9d2a6b9c0d1234.jsonl",),
            None
        );
        assert_eq!(
            extract_session_id_from_path(
                "gjc",
                "/Users/me/.gjc/agent/sessions/v2-scopeid/2026-02-16T10-20-30.000Z_abcd.jsonl",
            ),
            None
        );
    }

    // Store paths and ids below are measured (.omo-parity/facts remote-omp.md, remote-grok.md,
    // remote-droid.md, local-3.md, remote-gjc.md).
    #[test]
    fn session_discovery_extracts_measured_omp_grok_droid_cline_store_paths() {
        // codex: the measured rollout name glues the timestamp to the id.
        assert_eq!(
            extract_session_id_from_path(
                "codex",
                "/Users/indo/.codex/sessions/2026/10/22/rollout-2026-10-22T16-07-46-01a1626f-f0db-75d0-b73c-2372a21d4cae.jsonl",
            ),
            Some("01a1626f-f0db-75d0-b73c-2372a21d4cae".to_string())
        );
        assert_eq!(
            extract_session_id_from_path("codex", "/Users/indo/.codex/sessions/2026/10/22/notes.jsonl"),
            None
        );
        assert_eq!(
            extract_session_id_from_path(
                "codex",
                "/Users/indo/.claude/projects/x/rollout-2026-10-22T16-07-46-01a1626f-f0db-75d0-b73c-2372a21d4cae.jsonl",
            ),
            None
        );
        assert_eq!(
            extract_session_id_from_path(
                "omp",
                "/home/indo/.omp/agent/sessions/-agent-parity-facts-work/2026-10-22T07-08-47-832Z_01a16270-e358-7635-88e4-1b2f84460057.jsonl",
            ),
            Some("01a16270-e358-7635-88e4-1b2f84460057".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "omp",
                "/home/indo/.omp/profiles/work/agent/sessions/-agent-parity-facts-work/2026-10-22T07-08-47-832Z_01a16270-e358-7635-88e4-1b2f84460057.jsonl",
            ),
            Some("01a16270-e358-7635-88e4-1b2f84460057".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "omp",
                "/home/indo/.pi/agent/sessions/-x/2026-10-22T07-08-47-832Z_01a16270-e358-7635-88e4-1b2f84460057.jsonl",
            ),
            None
        );

        assert_eq!(
            extract_session_id_from_path(
                "grok",
                "/home/indo/.grok/sessions/%2Fhome%2Findo%2Fagent-parity-facts%2Fgrok%2Fproj/01a16274-a688-7e73-b6fc-434bc7781be4/updates.jsonl",
            ),
            Some("01a16274-a688-7e73-b6fc-434bc7781be4".to_string())
        );
        assert_eq!(
            extract_session_id_from_path("grok", "/home/indo/.grok/sessions/session_search.sqlite"),
            None
        );

        assert_eq!(
            extract_session_id_from_path(
                "droid",
                "/home/indo/.factory/sessions/-home-indo-agent-parity-facts-droid-work/d08d895d-058f-4f86-aa21-48d7dbdcaa9f.jsonl",
            ),
            Some("d08d895d-058f-4f86-aa21-48d7dbdcaa9f".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "droid",
                "/home/indo/.factory/sessions/-home-indo-agent-parity-facts-droid-work/d08d895d-058f-4f86-aa21-48d7dbdcaa9f.settings.json",
            ),
            Some("d08d895d-058f-4f86-aa21-48d7dbdcaa9f".to_string())
        );
        assert_eq!(
            extract_session_id_from_path("droid", "/home/indo/.factory/host.json"),
            None
        );

        assert_eq!(
            extract_session_id_from_path(
                "cline",
                "/Users/indo/.cline/data/sessions/1792653340197_ys4yu/1792653340197_ys4yu.json",
            ),
            Some("1792653340197_ys4yu".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "cline",
                "/Users/indo/.cline/data/sessions/1792653340197_ys4yu/1792653340197_ys4yu.messages.json",
            ),
            Some("1792653340197_ys4yu".to_string())
        );
        assert_eq!(
            extract_session_id_from_path(
                "cline",
                "/Users/indo/.cline/data/sessions/1792653340197_ys4yu/other_ys4yu.json",
            ),
            None
        );
        // gjc mints UUIDv7 ids; the measured file name still parses.
        assert_eq!(
            extract_session_id_from_path(
                "gjc",
                "/home/indo/.gjc/agent/sessions/v2-scope/2026-10-22T07-12-55-764Z_01a16274-abd4-7581-8f9c-22214661ef6a.jsonl",
            ),
            Some("01a16274-abd4-7581-8f9c-22214661ef6a".to_string())
        );
    }

    // Measured on omarchy (grok 1.0.67, no lsof installed): a freshly launched grok holds
    // `<group>/<id>/events.jsonl` open, so the /proc fd walk is how a fresh pane gets its id.
    #[cfg(unix)]
    #[test]
    fn proc_fd_walk_captures_a_fresh_grok_session_from_its_open_events_file() {
        let fd_dir = tempfile::tempdir().expect("fd dir");
        let open = "/home/indo/.grok/sessions/%2Ftmp%2Ffx-grok-tkCR/01a16459-7484-7793-9e7d-90f67df3379f/events.jsonl";
        std::os::unix::fs::symlink("/dev/null", fd_dir.path().join("0")).expect("stdin link");
        std::os::unix::fs::symlink(open, fd_dir.path().join("7")).expect("session link");
        assert_eq!(
            proc_fd_session_id(fd_dir.path(), "grok"),
            Some("01a16459-7484-7793-9e7d-90f67df3379f".to_string())
        );
        assert_eq!(proc_fd_session_id(fd_dir.path(), "droid"), None);
    }

    #[test]
    fn args_match_agent_recognizes_measured_omp_grok_droid_cline_argv() {
        assert!(args_match_agent("omp", "bun /home/indo/.bun/bin/omp"));
        assert!(args_match_agent(
            "omp",
            "bun /home/indo/.bun/bin/omp --model parity-mock/mock-model --resume 01a16270"
        ));
        assert!(!args_match_agent(
            "omp",
            "bun /home/indo/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent/dist/cli.js __omp_worker_daemon_broker"
        ));
        assert!(args_match_agent("grok", "/home/indo/.grok/bin/grok"));
        assert!(!args_match_agent("grok", "/home/indo/.grok/bin/agent"));
        assert!(args_match_agent("droid", "droid"));
        assert!(args_match_agent(
            "cline",
            "/Users/indo/.bun/install/global/node_modules/@cline/cli-darwin-arm64/bin/cline --hooks-dir /tmp/parity-cline-hooks.2BXn"
        ));
        assert!(!args_match_agent(
            "cline",
            "/x/@cline/cli-darwin-arm64/bin/cline --cline-hub-daemon --cwd /private/tmp/parity-cline.HDLw --host 127.0.0.1 --port 25463 --pathname /hub"
        ));
    }

    #[test]
    fn resume_id_from_args_reads_measured_resume_argv() {
        assert_eq!(
            resume_id_from_args(
                "grok",
                "/home/indo/.grok/bin/grok --resume 01a16274-a688-7e73-b6fc-434bc7781be4"
            )
            .as_deref(),
            Some("01a16274-a688-7e73-b6fc-434bc7781be4")
        );
        // A grok title is a valid --resume value but not a session id.
        assert_eq!(resume_id_from_args("grok", "grok --resume PARITY_TOOL"), None);
        assert_eq!(
            resume_id_from_args("droid", "droid --resume d08d895d-058f-4f86-aa21-48d7dbdcaa9f")
                .as_deref(),
            Some("d08d895d-058f-4f86-aa21-48d7dbdcaa9f")
        );
        assert_eq!(
            resume_id_from_args(
                "cline",
                "/Users/indo/.bun/install/global/node_modules/@cline/cli-darwin-arm64/bin/cline --hooks-dir /tmp/parity-cline-hooks.2BXn --id 1792653340197_ys4yu"
            )
            .as_deref(),
            Some("1792653340197_ys4yu")
        );
        assert_eq!(resume_id_from_args("cline", "cline --hooks-dir /tmp/h"), None);
        // kimi/copilot UUIDs are measured store ids (~/.kimi/sessions/<md5>/<uuid>,
        // ~/.copilot/session-state/<uuid>.jsonl); gjc ids are its 16-hex store suffix.
        assert_eq!(
            resume_id_from_args("kimi", "/usr/bin/python3 /x/kimi --session cb340356-2adc-4cbf-acd3-35e6fb6a5a32")
                .as_deref(),
            Some("cb340356-2adc-4cbf-acd3-35e6fb6a5a32")
        );
        assert_eq!(
            resume_id_from_args("kimi", "kimi -S cb340356-2adc-4cbf-acd3-35e6fb6a5a32").as_deref(),
            Some("cb340356-2adc-4cbf-acd3-35e6fb6a5a32")
        );
        assert_eq!(
            resume_id_from_args("copilot", "copilot --resume 0538fdd2-a63e-42b6-8405-4588bd776c6c")
                .as_deref(),
            Some("0538fdd2-a63e-42b6-8405-4588bd776c6c")
        );
        // `copilot --resume` alone opens a picker; no id is claimed.
        assert_eq!(resume_id_from_args("copilot", "copilot --resume"), None);
        assert_eq!(
            resume_id_from_args("gjc", "bun /x/gjc --resume 1f9d2a6b9c0d1234").as_deref(),
            Some("1f9d2a6b9c0d1234")
        );
        assert_eq!(resume_id_from_args("gjc", "gjc --resume abcd"), None);
        // remote-crush.md section 1: the npm wrapper and its Go child both carry the resume flag.
        assert_eq!(
            resume_id_from_args(
                "crush",
                "/home/indo/.local/share/mise/installs/node/26.8.1/lib/node_modules/@charmland/crush/bin/crush --session 7f7fbfa62dd136ec"
            )
            .as_deref(),
            Some("7f7fbfa62dd136ec")
        );
        assert_eq!(
            resume_id_from_args("crush", "crush -s 3df112d9-e56f-4f69-b8bb-c58b5d580e89")
                .as_deref(),
            Some("3df112d9-e56f-4f69-b8bb-c58b5d580e89")
        );
        assert_eq!(resume_id_from_args("crush", "crush --session 7f7f"), None);
        assert_eq!(resume_id_from_args("crush", "crush --continue"), None);
        // A bare or non-UUID resume value (the picker, or a search term) names no session.
        assert_eq!(resume_id_from_args("claude", "claude --resume id"), None);
        assert_eq!(resume_id_from_args("claude", "claude --resume"), None);
        // omp, measured on omarchy (bun-launched); Ferryx resumes it by path or by id.
        assert_eq!(
            resume_id_from_args(
                "omp",
                "bun /home/indo/.bun/bin/omp --resume /home/indo/.omp/agent/sessions/-tmp-x/2026-10-23T00-58-57-948Z_01a16644-a81c-76a4-879e-e9857adb0145.jsonl"
            )
            .as_deref(),
            Some("01a16644-a81c-76a4-879e-e9857adb0145")
        );
        assert_eq!(
            resume_id_from_args("omp", "bun /home/indo/.bun/bin/omp --resume 01a16644-a81c-76a4-879e-e9857adb0145")
                .as_deref(),
            Some("01a16644-a81c-76a4-879e-e9857adb0145")
        );
        assert_eq!(resume_id_from_args("omp", "omp --resume /tmp/elsewhere.jsonl"), None);
        assert_eq!(
            resume_id_from_args("claude", "claude --resume 0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80")
                .as_deref(),
            Some("0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80")
        );
        assert_eq!(
            resume_id_from_args("claude", "node /x/claude -r 0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80")
                .as_deref(),
            Some("0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80")
        );
        assert_eq!(
            resume_id_from_args(
                "claude",
                "claude --session-id 0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80"
            )
            .as_deref(),
            Some("0f3c9a52-6b1e-4d7a-9c2e-8a4b5d6e7f80")
        );
    }

    #[test]
    fn crush_discovery_binds_the_go_binary_under_its_npm_wrapper() {
        // Measured process tree, remote-crush.md section 1.
        let entries = vec![
            (557000, 1, "/bin/zsh".to_string()),
            (
                557321,
                557000,
                "/home/indo/.local/share/mise/installs/node/26.8.1/bin/node /home/indo/.local/share/mise/installs/node/26.8.1/bin/crush --session 7f7fbfa62dd136ec".to_string(),
            ),
            (
                557375,
                557321,
                "/home/indo/.local/share/mise/installs/node/26.8.1/lib/node_modules/@charmland/crush/bin/crush --session 7f7fbfa62dd136ec".to_string(),
            ),
        ];
        assert_eq!(descendant_agent_pid(&entries, 557000, "crush"), Some(557375));
        assert!(!args_match_agent(
            "crush",
            "/home/indo/.local/share/mise/installs/node/26.8.1/bin/node /home/indo/.local/share/mise/installs/node/26.8.1/bin/crush"
        ));
    }

    #[test]
    fn parse_ps_entries_preserves_long_args() {
        let stdout = concat!(
            "  100     1 /bin/zsh -l\n",
            "  102   101 bun /Users/x/.bun/install/global/node_modules/omo-ai/bin/omo.js serve --very-long-argument=value\n",
            "invalid line\n",
        );

        assert_eq!(
            parse_ps_entries(stdout),
            vec![
                (100, 1, "/bin/zsh -l".to_string()),
                (
                    102,
                    101,
                    "bun /Users/x/.bun/install/global/node_modules/omo-ai/bin/omo.js serve --very-long-argument=value".to_string(),
                ),
            ]
        );
    }

    #[test]
    fn parse_powershell_process_json_accepts_array_and_single_object() {
        let array = concat!(
            "[",
            r#"{"ProcessId":100,"ParentProcessId":1,"Name":"powershell.exe","CommandLine":"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -NoProfile"},"#,
            r#"{"ProcessId":102,"ParentProcessId":101,"Name":"node.exe","CommandLine":"node C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\omo-ai\\bin\\omo.js serve"},"#,
            r#"{"ProcessId":103,"ParentProcessId":101,"Name":"System","CommandLine":null}"#,
            "]",
        );

        assert_eq!(
            parse_powershell_process_json(array),
            Some(vec![
                (
                    100,
                    1,
                    r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile"
                        .to_string(),
                ),
                (
                    102,
                    101,
                    r"node C:\Users\x\AppData\Roaming\npm\node_modules\omo-ai\bin\omo.js serve"
                        .to_string(),
                ),
                (103, 101, String::new()),
            ])
        );

        // A one-process table collapses to a bare object instead of an array.
        assert_eq!(
            parse_powershell_process_json(
                r#"{"ProcessId":7,"ParentProcessId":1,"Name":"claude.exe","CommandLine":"claude --resume id"}"#
            ),
            Some(vec![(7, 1, "claude --resume id".to_string())])
        );
    }

    #[test]
    fn parse_powershell_process_json_rejects_non_tables() {
        assert_eq!(parse_powershell_process_json(""), None);
        assert_eq!(parse_powershell_process_json("not json"), None);
        assert_eq!(parse_powershell_process_json(r#"[{"ProcessId":1"#), None);
        assert_eq!(
            parse_powershell_process_json(r#"{"ParentProcessId":1,"CommandLine":"x"}"#),
            None
        );
    }

    #[test]
    fn args_match_agent_recognizes_supported_commands() {
        assert!(args_match_agent(
            "omo",
            "bun /Users/x/.bun/install/global/node_modules/omo-ai/bin/omo.js serve"
        ));
        assert!(args_match_agent(
            "omo",
            "bun /Users/x/senpi/dist/cli.js run"
        ));
        assert!(!args_match_agent("omo", "/usr/local/bin/omo-helper serve"));
        assert!(args_match_agent("omo", "/home/user/.local/bin/mhc --session id"));
        assert!(args_match_agent("omo", "C:\\bin\\mhc.exe --session id"));
        assert!(args_match_agent("omo", "\"C:\\Program Files\\mhc.exe\" --session id"));
        assert!(!args_match_agent("omo", "/bin/echo mhc --session id"));
        assert!(!args_match_agent("omo", "/bin/mhc-helper --session id"));

        assert!(args_match_agent(
            "claude",
            "/Users/x/.local/bin/claude --resume id"
        ));
        assert!(!args_match_agent("claude", "/bin/sh claude --resume id"));
        assert!(args_match_agent("codex", "/opt/bin/codex --session id"));
        assert!(!args_match_agent("codex", "/opt/bin/codex-helper"));
        assert!(args_match_agent("copilot", "copilot --resume id"));
        assert!(!args_match_agent("copilot", "copilot-agent --resume id"));
        assert!(args_match_agent("kimi", "/usr/local/bin/kimi --session id"));
        assert!(!args_match_agent("kimi", "/usr/local/bin/kimi-cli"));
        assert!(args_match_agent("antigravity", "/Users/x/.local/bin/agy"));
        assert!(args_match_agent("antigravity", "agy --conversation id"));
        assert!(!args_match_agent(
            "antigravity",
            "/usr/local/bin/agy-helper"
        ));
        assert!(!args_match_agent("antigravity", "antigravity"));
        assert!(args_match_agent("opencode", "/usr/local/bin/opencode"));
        assert!(args_match_agent("opencode", "opencode --session id"));
        assert!(args_match_agent("pi", "/Users/x/.bun/bin/pi"));
        assert!(args_match_agent(
            "pi",
            "node /path/@mariozechner/pi-coding-agent/dist/main.js"
        ));
        assert!(args_match_agent("gjc", "/usr/local/bin/gjc"));
        assert!(args_match_agent(
            "gjc",
            "node /Users/x/.npm-global/bin/gjc.js"
        ));
        assert!(!args_match_agent("gjc", "/usr/local/bin/gjc-helper"));
        // Re-explore-4 G2: machine-pane discovery must find the interpreter-launched kimi and gjc
        // processes measured live, or their panes never get an id.
        assert!(args_match_agent(
            "kimi",
            "/Users/indo/.local/share/uv/tools/kimi-cli/bin/python /Users/indo/.local/bin/kimi"
        ));
        assert!(args_match_agent(
            "kimi",
            "/usr/bin/python3.12 /home/x/.local/bin/kimi -S eca66b4f-a89b-4979-9f13-cbe3f35b5991"
        ));
        assert!(!args_match_agent("kimi", "/usr/bin/python3 /x/manage.py kimi"));
        assert!(args_match_agent(
            "gjc",
            "bun /home/indo/.local/share/mise/installs/node/26.8.1/bin/gjc --model fakelocal/fake-model"
        ));
        assert!(!args_match_agent("gjc", "bun /x/server.ts gjc"));
        assert!(!args_match_agent("copilot", "node /x/copilot-helper.js"));
        assert!(args_match_agent(
            "cursor",
            "/Applications/Cursor.app/Contents/Resources/app/bin/cursor-agent --resume id"
        ));
        assert!(args_match_agent(
            "cursor-agent",
            "/Applications/Cursor.app/Contents/Resources/app/bin/cursor-agent --resume id"
        ));
        assert!(args_match_agent("cursor", "cursor --resume id"));
        assert!(!args_match_agent("cursor", "cursor-helper --resume id"));
        assert!(!args_match_agent("unknown", "unknown"));
    }

    #[test]
    fn cursor_agent_alias_extracts_cursor_chat_session_id() {
        let id = "123e4567-e89b-12d3-a456-426614174000";
        assert_eq!(
            extract_session_id_from_path(
                "cursor-agent",
                &format!("/Users/x/.cursor/chats/{id}/chat.json")
            ),
            Some(id.to_string())
        );
    }

    #[test]
    fn descendant_agent_pid_finds_omo_beneath_shell_chain() {
        let entries = vec![
            (100, 1, "/bin/zsh -l".to_string()),
            (101, 100, "/bin/zsh -l".to_string()),
            (200, 1, "/bin/zsh -l".to_string()),
            (
                201,
                200,
                "bun /Users/other/omo-ai/bin/omo.js serve".to_string(),
            ),
            (
                102,
                101,
                "bun /Users/x/.bun/install/global/node_modules/omo-ai/bin/omo.js serve".to_string(),
            ),
        ];

        assert_eq!(descendant_agent_pid(&entries, 100, "omo"), Some(102));
        assert_eq!(
            descendant_agent_pid(
                &[(100, 1, "/bin/zsh -l".into()), (101, 100, "/bin/zsh /Users/x/home/omo.js".into())],
                100,
                "omo"
            ),
            Some(101)
        );
    }

    #[test]
    fn uuid_from_session_path_extracts_timestamped_omo_session_id() {
        assert_eq!(
            uuid_from_session_path(
                "/Users/indo/.omo/sessions/project/2026-08-31T04-00-20-190Z_01a055f9-a8de-7619-a1f5-81ca62e3d3b1.jsonl",
            ),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1".to_string())
        );
    }

    #[test]
    fn session_id_from_env_output_reads_pi_session_file() {
        let stdout = concat!(
            "12345 ?? S 0:00.01 bun /Users/x/omo-ai/bin/omo.js serve ",
            "HOME=/Users/indo PATH=/usr/bin:/bin ",
            "PI_SESSION_FILE=/Users/indo/.omo/sessions/orca-lite/2026-08-31T04-00-20-190Z_01a055f9-a8de-7619-a1f5-81ca62e3d3b1.jsonl ",
            "SHELL=/bin/zsh\n",
        );

        assert_eq!(
            session_id_from_env_output("omo", stdout),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1".to_string())
        );
        assert_eq!(
            session_id_from_env_output("omo", "HOME=/Users/indo PATH=/usr/bin:/bin"),
            None
        );
    }

    #[test]
    fn test_antigravity_last_conversations_parser() {
        let fixture = r#"{
            "/Users/alice/projects/app": "aaaaaaaa-1111-2222-3333-444444444444",
            "/Users/alice/projects": "bbbbbbbb-1111-2222-3333-444444444444",
            "/Users/alice/other": "not-a-uuid",
            "/Users/alice/proj": "cccccccc-1111-2222-3333-444444444444"
        }"#;

        // Exact hit
        assert_eq!(
            find_antigravity_conversation_id(fixture, "/Users/alice/projects/app"),
            Some("aaaaaaaa-1111-2222-3333-444444444444".to_string())
        );

        // Prefix fallback (longest path prefix matches /Users/alice/projects, NOT /Users/alice/proj)
        assert_eq!(
            find_antigravity_conversation_id(fixture, "/Users/alice/projects/app/sub/dir"),
            Some("aaaaaaaa-1111-2222-3333-444444444444".to_string())
        );
        assert_eq!(
            find_antigravity_conversation_id(fixture, "/Users/alice/projects/other-dir"),
            Some("bbbbbbbb-1111-2222-3333-444444444444".to_string())
        );

        // Non-path prefix should not match (/Users/alice/project-foo should not match /Users/alice/projects or /Users/alice/proj)
        assert_eq!(
            find_antigravity_conversation_id(fixture, "/Users/alice/project-foo"),
            None
        );

        // Miss
        assert_eq!(find_antigravity_conversation_id(fixture, "/var/log"), None);

        // Non-UUID value is rejected
        assert_eq!(
            find_antigravity_conversation_id(fixture, "/Users/alice/other"),
            None
        );
    }

    #[test]
    fn session_discovery_extracts_pi_session_path() {
        let id = "12345678-1234-1234-1234-123456789abc";
        assert_eq!(
            extract_session_id_from_path(
                "pi",
                &format!("/Users/me/.pi/agent/sessions/--some-path--/2026-09-02T16-00-00-000Z_{id}.jsonl")
            ),
            Some(id.to_string())
        );
        assert_eq!(
            extract_session_id_from_path("pi", "/tmp/unrelated.jsonl"),
            None
        );
    }

    /// The measured senpi transcript (facts/local-1.md:341-344, facts/local.md:34).
    const SENPI_MEASURED_PATH: &str = "/Users/indo/.senpi/agent/sessions/--private-tmp-parity-pi.tqp0--/2026-10-22T07-24-18-316Z_01a1627f-160c-731c-9a7c-2069a9dd8b54.jsonl";

    #[test]
    fn pi_discovery_reads_the_measured_senpi_store() {
        let id = "01a1627f-160c-731c-9a7c-2069a9dd8b54";
        let no_env = |_: &str| None;
        assert_eq!(pi_session_id_from_path(SENPI_MEASURED_PATH, no_env), Some(id.to_string()));
        // Windows separators normalise to the same marker.
        let windows = SENPI_MEASURED_PATH.replace("/Users/indo", r"C:\Users\indo").replace('/', r"\");
        assert_eq!(extract_session_id_from_path("pi", &windows), Some(id.to_string()));
        // A senpi sibling that is not a session store carries no pi session.
        assert_eq!(
            pi_session_id_from_path(&format!("/Users/indo/.senpi/agent/runtime/{id}.jsonl"), no_env),
            None
        );
    }

    #[test]
    fn pi_discovery_honours_senpi_coding_agent_dir() {
        // The functional measured run used SENPI_CODING_AGENT_DIR=$HOME/.omo/agent
        // (facts/local-1.md:341-344): the transcript sits under that root, not ~/.senpi.
        let id = "01a16281-1b66-764d-87a3-4ea9321c5bd8";
        let path = format!("/srv/agent-root/sessions/--private-tmp-parity-pi.tqp0--/2026-10-22T07-26-30-758Z_{id}.jsonl");
        let root = OsString::from("/srv/agent-root/");
        let with_env = |key: &str| (key == "SENPI_CODING_AGENT_DIR").then(|| root.clone());
        assert_eq!(pi_session_id_from_path(&path, with_env), Some(id.to_string()));
        assert_eq!(pi_session_id_from_path(&path, |_: &str| None), None, "no marker, no override");
        let other = OsString::from("/srv/agent-rootless");
        assert_eq!(
            pi_session_id_from_path(&path, |key: &str| (key == "SENPI_CODING_AGENT_DIR").then(|| other.clone())),
            None
        );
    }

    #[test]
    fn pi_cwd_fallback_searches_senpi_then_upstream_roots() {
        let home = tempfile::tempdir().unwrap();
        let home_os = home.path().as_os_str().to_os_string();
        let roots = pi_session_roots(|key: &str| (key == "HOME").then(|| home_os.clone()));
        assert_eq!(
            roots,
            vec![home.path().join(".senpi/agent/sessions"), home.path().join(".pi/agent/sessions")]
        );
        let custom = home.path().join("custom");
        let custom_os = custom.as_os_str().to_os_string();
        let overridden = pi_session_roots(|key: &str| match key {
            "HOME" => Some(home_os.clone()),
            "SENPI_CODING_AGENT_DIR" => Some(custom_os.clone()),
            _ => None,
        });
        assert_eq!(overridden.first(), Some(&custom.join("sessions")));
        assert!(overridden.contains(&home.path().join(".senpi/agent/sessions")));

        let cwd = "/private/tmp/parity-pi.tqp0";
        let id = "01a1627f-160c-731c-9a7c-2069a9dd8b54";
        let dir = home.path().join(".senpi/agent/sessions").join(encode_pi_safe_path(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("2026-10-22T07-24-18-316Z_{id}.jsonl")), "{}\n").unwrap();
        assert_eq!(pi_session_id_from_session_dirs(&roots, cwd), Some(id.to_string()));
        // Upstream pi alone (no senpi root) still resolves.
        let upstream_id = "12345678-1234-1234-1234-123456789abc";
        let upstream = home.path().join(".pi/agent/sessions").join(encode_pi_safe_path(cwd));
        std::fs::create_dir_all(&upstream).unwrap();
        std::fs::write(upstream.join(format!("2026-09-02T16-00-00-000Z_{upstream_id}.jsonl")), "{}\n").unwrap();
        assert_eq!(pi_session_id_from_session_dirs(&roots[1..], cwd), Some(upstream_id.to_string()));
    }

    #[test]
    fn opencode_session_list_json_parser_matches_cwd() {
        let fixture = r#"[
            {
                "id": "ses_f9d2d353affewnpwvJ0rOcNZls",
                "title": "New session",
                "updated": 1788364377094,
                "created": 1788364376773,
                "directory": "/Users/indo/code/project/orca-lite"
            },
            {
                "id": "ses_other1234567890abcdef",
                "title": "Other",
                "updated": 1788360000000,
                "created": 1788360000000,
                "directory": "/Users/indo/code/other"
            }
        ]"#;

        assert_eq!(
            parse_opencode_session_list_json(fixture, "/Users/indo/code/project/orca-lite"),
            Some("ses_f9d2d353affewnpwvJ0rOcNZls".to_string())
        );
        assert_eq!(
            parse_opencode_session_list_json(fixture, "/Users/indo/code/other/sub"),
            Some("ses_other1234567890abcdef".to_string())
        );
        assert_eq!(
            parse_opencode_session_list_json(fixture, "/var/empty"),
            None
        );
    }

    #[test]
    fn test_encode_pi_safe_path() {
        assert_eq!(
            encode_pi_safe_path("/Users/indo/code/project/orca-lite"),
            "--Users-indo-code-project-orca-lite--"
        );
        assert_eq!(
            encode_pi_safe_path("/Users/indo/code/project/orca-lite/"),
            "--Users-indo-code-project-orca-lite--"
        );
        assert_eq!(
            encode_pi_safe_path(r"C:\Users\indo\project"),
            "--C--Users-indo-project--"
        );
    }

    #[test]
    fn opencode_session_id_validation() {
        assert!(is_valid_opencode_session_id(
            "ses_f9d2d353affewnpwvJ0rOcNZls"
        ));
        assert!(is_valid_opencode_session_id("ses_1234567"));
        assert!(!is_valid_opencode_session_id("invalid_id"));
        assert!(!is_valid_opencode_session_id("ses_"));
        assert!(!is_valid_opencode_session_id("ses_has space"));
        assert!(!is_valid_opencode_session_id("ses_has;semi"));
    }
}

#[cfg(test)]
mod p09_tests {
    use super::*;

    #[test]
    fn windows_pathext_order_case_and_explicit_extension() {
        let dir = std::env::temp_dir().join(format!("p09-path-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let result = std::panic::catch_unwind(|| {
            for ext in ["EXE", "COM", "CmD", "bat"] {
                std::fs::write(dir.join(format!("fixture.{ext}")), b"fixture").unwrap();
            }
            for (extensions, expected) in [
                (".EXE;.COM;.CMD;.BAT", "fixture.EXE"),
                (".COM;.EXE;.CMD;.BAT", "fixture.COM"),
                (".CMD;.BAT;.EXE", "fixture.CmD"),
                (".BAT;.CMD", "fixture.bat"),
            ] {
                assert_eq!(
                    resolve_binary_with_env("fixture", std::slice::from_ref(&dir), true, Some(extensions)),
                    Some(dir.join(expected))
                );
            }
            assert_eq!(
                resolve_binary_with_env("FIXTURE.cmd", std::slice::from_ref(&dir), true, Some(".CMD")),
                Some(dir.join("fixture.CmD"))
            );
            std::fs::write(dir.join("data.txt"), b"data").unwrap();
            assert_eq!(
                resolve_binary_with_env("data.txt", std::slice::from_ref(&dir), true, Some(".TXT;.CMD")),
                None
            );
            assert_eq!(
                resolve_binary_with_env("../fixture", std::slice::from_ref(&dir), true, Some(".EXE")),
                None
            );
        });
        std::fs::remove_dir_all(&dir).unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_requires_executable_permission() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("p09-unix-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let result = std::panic::catch_unwind(|| {
            let file = dir.join("fixture");
            std::fs::write(&file, b"fixture").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(
                resolve_binary_with_env("fixture", std::slice::from_ref(&dir), false, None),
                None
            );
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                resolve_binary_with_env("fixture", std::slice::from_ref(&dir), false, None),
                Some(file)
            );
        });
        std::fs::remove_dir_all(&dir).unwrap();
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
