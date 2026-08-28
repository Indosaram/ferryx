use serde::{Deserialize, Serialize};
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
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
    let output = Command::new(&shell)
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
}
