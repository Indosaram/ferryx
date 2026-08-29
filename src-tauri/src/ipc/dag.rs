use crate::dag::journal::{list_run_summaries, parse_run_checkpoint, DagRunSnapshot, DagRunSummary};
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use std::path::{Path, PathBuf};

fn resolve_dag_runs_dir(project_path: &Path) -> PathBuf {
    let nested = project_path.join(".omo/senpi-task/dag");
    if nested.join("runs").is_dir() {
        nested.join("runs")
    } else if nested.is_dir() {
        nested
    } else if project_path.join("runs").is_dir() {
        project_path.join("runs")
    } else if project_path.ends_with(".omo/senpi-task/dag") || project_path.ends_with("dag") {
        project_path.join("runs")
    } else {
        nested.join("runs")
    }
}

#[tauri::command]
pub async fn dag_list_runs(project_path: String) -> Result<Vec<DagRunSummary>, IpcError> {
    run_blocking(move || {
        let path = PathBuf::from(&project_path);
        let runs_dir = resolve_dag_runs_dir(&path);
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }
        list_run_summaries(&runs_dir).map_err(|err| match err {
            crate::dag::journal::DagJournalError::Json(_) => {
                IpcError::new(IpcErrorCode::ParseError, err.to_string())
            }
            crate::dag::journal::DagJournalError::Io(_) => {
                IpcError::new(IpcErrorCode::IoError, err.to_string())
            }
            crate::dag::journal::DagJournalError::UnknownWaveNode { .. } => {
                IpcError::new(IpcErrorCode::ParseError, err.to_string())
            }
        })
    })
    .await
}

#[tauri::command]
pub async fn dag_get_run(
    project_path: String,
    run_id: String,
) -> Result<Option<DagRunSnapshot>, IpcError> {
    run_blocking(move || {
        let path = PathBuf::from(&project_path);
        let runs_dir = resolve_dag_runs_dir(&path);
        if !runs_dir.exists() {
            return Ok(None);
        }

        let clean_id = run_id.strip_prefix("dag_").unwrap_or(&run_id);
        let candidates = [
            runs_dir.join(format!("dag_{clean_id}.json")),
            runs_dir.join(format!("{run_id}.json")),
            runs_dir.join(&run_id),
        ];

        for candidate in &candidates {
            if candidate.is_file() {
                let content = std::fs::read_to_string(candidate).map_err(|e| {
                    IpcError::new(IpcErrorCode::IoError, format!("failed to read file: {e}"))
                })?;
                let snapshot = parse_run_checkpoint(&content).map_err(|e| {
                    IpcError::new(IpcErrorCode::ParseError, format!("failed to parse checkpoint: {e}"))
                })?;
                return Ok(Some(snapshot));
            }
        }

        if let Ok(entries) = std::fs::read_dir(&runs_dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "json") {
                    if let Ok(content) = std::fs::read_to_string(&entry_path) {
                        if let Ok(snapshot) = parse_run_checkpoint(&content) {
                            if snapshot.run_id == run_id || snapshot.run_id == clean_id {
                                return Ok(Some(snapshot));
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_F107: &str =
        include_str!("../dag/testdata/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");

    #[tokio::test]
    async fn test_dag_list_runs_empty_for_missing_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = dag_list_runs(temp.path().to_string_lossy().to_string())
            .await
            .expect("runs list");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_dag_list_and_get_runs_success() {
        let temp = tempfile::tempdir().expect("tempdir");
        let runs_dir = temp.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");
        let file_path = runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");
        std::fs::write(&file_path, FIXTURE_F107).expect("write checkpoint");

        let project_str = temp.path().to_string_lossy().to_string();
        let list = dag_list_runs(project_str.clone()).await.expect("list runs");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].run_id, "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7");

        let run_direct = dag_get_run(project_str.clone(), "f107f318-ac78-46a2-b8c6-584b4e10eaa7".into())
            .await
            .expect("get run");
        assert!(run_direct.is_some());
        assert_eq!(run_direct.unwrap().run_id, "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7");

        let run_with_prefix = dag_get_run(project_str.clone(), "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7".into())
            .await
            .expect("get run with prefix");
        assert!(run_with_prefix.is_some());

        let missing = dag_get_run(project_str, "nonexistent".into())
            .await
            .expect("get missing run");
        assert!(missing.is_none());
    }
}
