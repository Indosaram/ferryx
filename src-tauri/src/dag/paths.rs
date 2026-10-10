use std::path::{Path, PathBuf};

pub fn resolve_dag_runs_dir(project_path: &Path) -> PathBuf {
    resolve_dag_runs_dir_with_env(project_path, |key| std::env::var_os(key))
}

fn resolve_dag_runs_dir_with_env(
    project_path: &Path,
    get_env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> PathBuf {
    let nested = project_path.join(".omo/senpi-task/dag");
    if nested.join("runs").is_dir() {
        nested.join("runs")
    } else if nested.is_dir() {
        nested
    } else if project_path.join("runs").is_dir() {
        project_path.join("runs")
    } else if project_path.ends_with(".omo/senpi-task/dag") || project_path.ends_with("dag") {
        project_path.join("runs")
    } else if has_records(&project_path.join(".omo/senpi-task")) {
        nested.join("runs")
    } else {
        match agent_project_state_dir(project_path, get_env) {
            Some(state_dir) => state_dir.join("dag/runs"),
            None => nested.join("runs"),
        }
    }
}

/// Mirrors where omo writes task state: `<project>/.omo/senpi-task` once that dir holds records,
/// otherwise `<agent dir>/projects/<name>-<sha256(realpath)[..12]>/senpi-task`.
fn agent_project_state_dir(
    project_path: &Path,
    get_env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let agent_dir = ["OMO_CODING_AGENT_DIR", "SENPI_CODING_AGENT_DIR", "PI_CODING_AGENT_DIR"]
        .iter()
        .find_map(|key| {
            get_env(key)
                .and_then(|value| value.into_string().ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            get_env("HOME")
                .filter(|value| !value.is_empty())
                .or_else(|| get_env("USERPROFILE").filter(|value| !value.is_empty()))
                .map(|home| PathBuf::from(home).join(".omo").join("agent"))
        })?;
    let real = real_project_path(project_path);
    let real_text = real.to_string_lossy();
    let digest = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(real_text.as_bytes());
        hash.iter()
            .take(6)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let name: String = real
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = if name.is_empty() { "root".to_string() } else { name };
    Some(
        agent_dir
            .join("projects")
            .join(format!("{name}-{digest}"))
            .join("senpi-task"),
    )
}

/// Matches Node's `realpathSync.native`, which omo hashes: no `\\?\` verbatim prefix.
fn real_project_path(project_path: &Path) -> PathBuf {
    let Ok(real) = std::fs::canonicalize(project_path) else {
        return project_path.to_path_buf();
    };
    let text = real.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        real
    }
}

fn has_records(dir: &Path) -> bool {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if !meta.is_dir() => true,
        Ok(_) => dir_has_records(dir),
        Err(_) => false,
    }
}

fn dir_has_records(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let bare_dir = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && !entry.file_name().to_string_lossy().contains('.');
        !bare_dir || dir_has_records(&entry.path())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_home(home: &Path) -> impl Fn(&str) -> Option<std::ffi::OsString> + '_ {
        move |key| (key == "HOME").then(|| home.as_os_str().to_os_string())
    }

    #[test]
    fn journal_outside_the_project_resolves_to_the_agent_state_dir() {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = temp.path().join("home");
        let project = temp.path().join("my repo");
        std::fs::create_dir_all(&project).expect("project");
        std::fs::create_dir_all(project.join(".omo")).expect("bare .omo");

        let real = real_project_path(&project);
        let expected_hash: String = {
            use sha2::{Digest, Sha256};
            Sha256::digest(real.to_string_lossy().as_bytes())
                .iter()
                .take(6)
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        assert_eq!(
            resolve_dag_runs_dir_with_env(&project, env_with_home(&home)),
            home.join(".omo/agent/projects")
                .join(format!("my_repo-{expected_hash}"))
                .join("senpi-task/dag/runs")
        );
    }

    #[test]
    fn agent_dir_override_wins_over_home() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = temp.path().join("p");
        std::fs::create_dir_all(&project).expect("project");
        let override_dir = temp.path().join("agent");
        let env = |key: &str| match key {
            "OMO_CODING_AGENT_DIR" => Some(override_dir.as_os_str().to_os_string()),
            "HOME" => Some(temp.path().join("home").into_os_string()),
            _ => None,
        };
        assert!(resolve_dag_runs_dir_with_env(&project, env).starts_with(override_dir.join("projects")));
    }

    #[test]
    fn adopted_in_project_state_keeps_the_in_project_journal() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = temp.path().join("p");
        let locks = project.join(".omo/senpi-task/locks");
        std::fs::create_dir_all(&locks).expect("locks");
        std::fs::write(locks.join("lock.json"), "{}").expect("record");
        assert_eq!(
            resolve_dag_runs_dir_with_env(&project, env_with_home(&temp.path().join("home"))),
            project.join(".omo/senpi-task/dag/runs")
        );
    }

    #[test]
    fn empty_in_project_state_dir_is_not_adopted() {
        let temp = tempfile::tempdir().expect("temp dir");
        let project = temp.path().join("p");
        std::fs::create_dir_all(project.join(".omo/senpi-task/tasks")).expect("bare dirs");
        assert!(!has_records(&project.join(".omo/senpi-task")));
        assert!(resolve_dag_runs_dir_with_env(&project, env_with_home(&temp.path().join("home")))
            .starts_with(temp.path().join("home/.omo/agent/projects")));
    }

    #[test]
    fn known_omo_hash_matches_node_for_an_absent_path() {
        let resolved = resolve_dag_runs_dir_with_env(
            Path::new("/home/projects/mhc"),
            |key| (key == "HOME").then(|| std::ffi::OsString::from("/home/indo")),
        );
        assert_eq!(
            resolved,
            PathBuf::from("/home/indo/.omo/agent/projects/mhc-e1e16759b81f/senpi-task/dag/runs")
        );
    }
}
