use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetection {
    pub name: String,
    pub available: bool,
}

/// Detect availability of CLI coding agents by checking if their binaries exist on PATH.
#[tauri::command]
pub fn cmd_agents_detect(names: Vec<String>) -> Vec<AgentDetection> {
    names
        .into_iter()
        .filter(|name| !name.trim().is_empty())
        .map(|name| {
            let available = check_binary_available(&name);
            AgentDetection { name, available }
        })
        .collect()
}

fn check_binary_available(name: &str) -> bool {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        // reject path-shaped names
        return false;
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_detect_existing_and_nonexistent_binaries() {
        let results = cmd_agents_detect(vec![
            "sh".into(),
            "ls".into(),
            "definitely-not-a-real-binary-xyz".into(),
            "".into(),
            "   ".into(),
        ]);

        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0],
            AgentDetection {
                name: "sh".into(),
                available: true,
            }
        );
        assert_eq!(
            results[1],
            AgentDetection {
                name: "ls".into(),
                available: true,
            }
        );
        assert_eq!(
            results[2],
            AgentDetection {
                name: "definitely-not-a-real-binary-xyz".into(),
                available: false,
            }
        );
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
        };
        let json = serde_json::to_string(&detection).expect("serialize");
        assert_eq!(json, r#"{"name":"claude","available":true}"#);

        let decoded: AgentDetection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, detection);
    }
}
