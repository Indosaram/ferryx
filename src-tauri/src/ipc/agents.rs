use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    pub name: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[tauri::command]
pub async fn cmd_agents_detect(names: Vec<String>) -> Result<Vec<AgentDetection>, super::IpcError> {
    super::run_blocking(move || Ok(detect_agents(&names))).await
}

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

pub(crate) fn discover_agent_session_id(root_pid: u32, agent_type: &str) -> Option<String> {
    let requested = agent_type.trim().to_ascii_lowercase();
    let normalized = if requested == "cursor-agent" {
        "cursor"
    } else {
        requested.as_str()
    };
    if !matches!(
        normalized,
        "claude" | "codex" | "copilot" | "cursor" | "kimi" | "omo" | "gjc"
    ) {
        return None;
    }

    let agent_pid = process_table_entries()
        .and_then(|entries| descendant_agent_pid(&entries, root_pid, normalized));
    match agent_pid {
        Some(pid) if normalized == "omo" => omo_session_id_from_environment(pid, normalized),
        Some(pid) => lsof_session_id(pid, normalized),
        None => lsof_session_id(root_pid, normalized),
    }
}

fn process_table_entries() -> Option<Vec<(u32, u32, String)>> {
    let output = crate::util::no_window_command("/bin/ps")
        .args(["-axwwo", "pid=,ppid=,args="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|stdout| parse_ps_entries(&stdout))
}

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

fn args_match_agent(agent_type: &str, args: &str) -> bool {
    let requested = agent_type.trim().to_ascii_lowercase();
    let normalized_agent = if requested == "cursor-agent" {
        "cursor"
    } else {
        requested.as_str()
    };
    if normalized_agent == "omo" {
        let normalized_args = args.to_ascii_lowercase();
        return normalized_args.contains("/omo.js")
            || normalized_args.contains("/senpi/dist/cli.js");
    }

    let Some(command) = args.split_whitespace().next() else {
        return false;
    };
    let basename = command
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(command)
        .to_ascii_lowercase();
    match normalized_agent {
        "cursor" => matches!(basename.as_str(), "cursor-agent" | "cursor"),
        "claude" | "codex" | "copilot" | "kimi" => basename == normalized_agent,
        "gjc" => basename == "gjc" || args.to_ascii_lowercase().contains("/gjc.js"),
        _ => false,
    }
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
    let output = crate::util::no_window_command("/bin/ps")
        .args(["-E", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .and_then(|stdout| session_id_from_env_output(agent_type, &stdout))
}

fn session_id_from_env_output(agent_type: &str, stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .filter_map(|part| part.strip_prefix("PI_SESSION_FILE="))
        .find_map(|path| extract_session_id_from_path(agent_type, path))
}

fn lsof_session_id(pid: u32, agent_type: &str) -> Option<String> {
    let output = crate::util::no_window_command("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-Fn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .filter_map(|line| line.strip_prefix('n'))
        .find_map(|path| extract_session_id_from_path(agent_type, path))
}

fn extract_session_id_from_path(agent_type: &str, path: &str) -> Option<String> {
    let agent_type = if agent_type == "cursor-agent" {
        "cursor"
    } else {
        agent_type
    };
    let normalized = path.replace('\\', "/");
    if agent_type.eq_ignore_ascii_case("gjc") {
        return gjc_session_id_from_path(&normalized);
    }
    let marker = match agent_type {
        "claude" => "/.claude/projects/",
        "codex" => "/.codex/sessions/",
        "copilot" => "/.copilot/session-state/",
        "cursor" => "/.cursor/chats/",
        "kimi" => "/.kimi/sessions/",
        "omo" => "/.omo/sessions/",
        _ => return None,
    };
    if !normalized.contains(marker) {
        return None;
    }
    uuid_from_session_path(&normalized)
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

// GJC (gajae-code) stores managed sessions at
// ~/.gjc/agent/sessions/<v2-scope>/<timestamp>_<sessionId>.jsonl; XDG data homes
// flatten the `agent/` segment (optionally under profiles/<profile>/). Session
// IDs are short hex strings, NOT UUIDs, so the ID is read from the file name.
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
    let search_paths = search_paths();
    names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| {
            let path = resolve_binary(name, &search_paths);
            AgentDetection {
                name: name.to_string(),
                available: path.is_some(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            }
        })
        .collect()
}

fn search_paths() -> Vec<PathBuf> {
    static PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    PATHS
        .get_or_init(|| {
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
            if let Some(value) = login_shell_path() {
                push_all(value);
            }
            dirs
        })
        .clone()
}

// Terminals spawn through the user's login shell, so its PATH (Homebrew, nvm,
// mise, ~/.local/bin) is the search space that matches what a launched agent
// would actually resolve against - the GUI process PATH alone misses most of it.
fn login_shell_path() -> Option<OsString> {
    let shell = env::var("SHELL").ok().filter(|s| !s.trim().is_empty())?;
    let output = crate::util::no_window_command(&shell)
        .args(["-lic", "printf %s \"$PATH\""])
        .env("PROMPT_EOL_MARK", "")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        return None;
    }
    Some(OsString::from(value))
}

fn resolve_binary(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') || name.contains('\\') {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_detect_existing_and_nonexistent_binaries() {
        let results = detect_agents(&[
            "sh".into(),
            "ls".into(),
            "definitely-not-a-real-binary-xyz".into(),
            "".into(),
            "   ".into(),
        ]);

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].name, "sh");
        assert!(results[0].available);
        assert!(results[0].path.is_some());
        assert_eq!(results[1].name, "ls");
        assert!(results[1].available);
        assert_eq!(results[2].name, "definitely-not-a-real-binary-xyz");
        assert!(!results[2].available);
        assert!(results[2].path.is_none());
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
        assert!(args_match_agent("gjc", "/usr/local/bin/gjc"));
        assert!(args_match_agent(
            "gjc",
            "node /Users/x/.npm-global/bin/gjc.js"
        ));
        assert!(!args_match_agent("gjc", "/usr/local/bin/gjc-helper"));
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
}
