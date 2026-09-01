use crate::dag::journal::{
    list_run_summaries, parse_run_checkpoint, DagJournalError,
    DagRunSnapshot as JournalDagRunSnapshot, DagRunSummary,
};
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunSnapshot {
    #[serde(flatten)]
    snapshot: JournalDagRunSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
}

impl Deref for DagRunSnapshot {
    type Target = JournalDagRunSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DagCheckpointSessionIds {
    #[serde(default)]
    root_session_id: Option<String>,
    #[serde(default)]
    parent_session_id: Option<String>,
}

fn parse_ipc_run_checkpoint(json: &str) -> Result<DagRunSnapshot, DagJournalError> {
    let session_ids: DagCheckpointSessionIds = serde_json::from_str(json)?;
    let snapshot = parse_run_checkpoint(json)?;
    Ok(DagRunSnapshot {
        snapshot,
        root_session_id: session_ids.root_session_id,
        parent_session_id: session_ids.parent_session_id,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunUpdatedPayload {
    pub project_path: String,
    pub snapshot: DagRunSnapshot,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DagWatchProjectResponse {
    pub project_path: String,
    pub runs: Vec<DagRunSnapshot>,
}

fn dag_watched_roots() -> &'static Mutex<HashSet<String>> {
    static DAG_WATCHED_ROOTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    DAG_WATCHED_ROOTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn canonical_project_key(project_path: &str) -> String {
    std::fs::canonicalize(project_path)
        .map(|canonical| canonical.to_string_lossy().to_string())
        .unwrap_or_else(|_| project_path.to_string())
}

fn load_current_snapshots(project_path: &str) -> Vec<DagRunSnapshot> {
    let runs_dir = resolve_dag_runs_dir(Path::new(project_path));
    if !runs_dir.is_dir() {
        return Vec::new();
    }
    let mut snapshots = Vec::new();
    let Ok(entries) = std::fs::read_dir(&runs_dir) else {
        return snapshots;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snapshot) = parse_ipc_run_checkpoint(&content) {
                    snapshots.push(snapshot);
                }
            }
        }
    }
    snapshots.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    snapshots
}

/// Registers a per-project dag journal watcher (idempotent per canonical path) and
/// returns the current run inventory and canonical project path so the UI can
/// hydrate immediately.
#[tauri::command]
pub async fn dag_watch_project<R: tauri::Runtime>(
    project_path: String,
    app: AppHandle<R>,
) -> Result<DagWatchProjectResponse, IpcError> {
    let canonical = run_blocking(move || Ok(canonical_project_key(&project_path))).await?;
    let is_new_root = {
        let mut watched = dag_watched_roots()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        watched.insert(canonical.clone())
    };
    if is_new_root {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, JournalDagRunSnapshot)>(100);
        crate::dag::watcher::spawn_dag_watcher(PathBuf::from(&canonical), tx);
        tauri::async_runtime::spawn(async move {
            while let Some((tagged, journal_snapshot)) = rx.recv().await {
                let snapshot = load_current_snapshots(&tagged)
                    .into_iter()
                    .find(|snapshot| snapshot.run_id == journal_snapshot.run_id)
                    .unwrap_or(DagRunSnapshot {
                        snapshot: journal_snapshot,
                        root_session_id: None,
                        parent_session_id: None,
                    });
                let payload = DagRunUpdatedPayload {
                    project_path: tagged,
                    snapshot,
                };
                if let Err(error) = app.emit("dag-run-updated", &payload) {
                    tracing::debug!("Failed to emit dag-run-updated event: {error}");
                }
            }
        });
    }
    let snapshot_project = canonical.clone();
    let runs = run_blocking(move || Ok(load_current_snapshots(&snapshot_project))).await?;
    Ok(DagWatchProjectResponse {
        project_path: canonical,
        runs,
    })
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
                let snapshot = parse_ipc_run_checkpoint(&content).map_err(|e| {
                    IpcError::new(
                        IpcErrorCode::ParseError,
                        format!("failed to parse checkpoint: {e}"),
                    )
                })?;
                return Ok(Some(snapshot));
            }
        }

        if let Ok(entries) = std::fs::read_dir(&runs_dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.extension().is_some_and(|ext| ext == "json") {
                    if let Ok(content) = std::fs::read_to_string(&entry_path) {
                        if let Ok(snapshot) = parse_ipc_run_checkpoint(&content) {
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

    #[test]
    fn dag_run_snapshot_forwards_checkpoint_session_ids() {
        let with_session_ids = r#"{
            "runId": "dag-session-ids",
            "runKey": "session-ids",
            "name": "Session ids",
            "status": "running",
            "rootSessionId": "01a055f9-a8de-7619-a1f5-81ca62e3d3b1",
            "parentSessionId": "01a055f9-a8de-7619-a1f5-81ca62e3d3b2",
            "nodes": [],
            "edges": [],
            "waves": [],
            "criticalPath": [],
            "bottlenecks": []
        }"#;
        let without_session_ids = r#"{
            "runId": "dag-no-session-ids",
            "runKey": "no-session-ids",
            "name": "No session ids",
            "status": "completed",
            "nodes": [],
            "edges": [],
            "waves": [],
            "criticalPath": [],
            "bottlenecks": []
        }"#;

        let with_ids =
            parse_ipc_run_checkpoint(with_session_ids).expect("parse checkpoint with ids");
        assert_eq!(
            with_ids.root_session_id.as_deref(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1")
        );
        assert_eq!(
            with_ids.parent_session_id.as_deref(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b2")
        );
        let serialized = serde_json::to_value(&with_ids).expect("serialize checkpoint with ids");
        assert_eq!(
            serialized["rootSessionId"].as_str(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b1")
        );
        assert_eq!(
            serialized["parentSessionId"].as_str(),
            Some("01a055f9-a8de-7619-a1f5-81ca62e3d3b2")
        );

        let without_ids =
            parse_ipc_run_checkpoint(without_session_ids).expect("parse checkpoint without ids");
        assert_eq!(without_ids.root_session_id, None);
        assert_eq!(without_ids.parent_session_id, None);
        let serialized =
            serde_json::to_value(&without_ids).expect("serialize checkpoint without ids");
        assert!(serialized.get("rootSessionId").is_none());
        assert!(serialized.get("parentSessionId").is_none());
    }

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

        let run_direct = dag_get_run(
            project_str.clone(),
            "f107f318-ac78-46a2-b8c6-584b4e10eaa7".into(),
        )
        .await
        .expect("get run");
        assert!(run_direct.is_some());
        assert_eq!(
            run_direct.unwrap().run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );

        let run_with_prefix = dag_get_run(
            project_str.clone(),
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7".into(),
        )
        .await
        .expect("get run with prefix");
        assert!(run_with_prefix.is_some());

        let missing = dag_get_run(project_str, "nonexistent".into())
            .await
            .expect("get missing run");
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_dag_watch_project_canonical_key_and_dedup() {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app");
        let (_temp, canonical_path, relative_path) = run_blocking(|| {
            let temp = tempfile::tempdir().expect("tempdir");
            let runs_dir = temp.path().join(".omo/senpi-task/dag/runs");
            std::fs::create_dir_all(&runs_dir).expect("create runs dir");
            std::fs::write(
                runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json"),
                FIXTURE_F107,
            )
            .expect("write checkpoint");

            let canonical_path = std::fs::canonicalize(temp.path())
                .expect("canonicalize")
                .to_string_lossy()
                .to_string();
            std::fs::create_dir_all(temp.path().join("sub")).expect("create sub");
            let relative_path = format!("{}/sub/..", temp.path().display());

            Ok((temp, canonical_path, relative_path))
        })
        .await
        .expect("set up project");

        let res1 = dag_watch_project(relative_path.clone(), app.handle().clone())
            .await
            .expect("watch project relative");

        assert_eq!(res1.project_path, canonical_path);
        assert_eq!(res1.runs.len(), 1);
        assert_eq!(
            res1.runs[0].run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );
        let serialized = serde_json::to_value(&res1).expect("serialize response");
        assert_eq!(
            serialized["projectPath"].as_str(),
            Some(canonical_path.as_str())
        );
        assert!(serialized.get("project_path").is_none());

        let (has_canonical_root, has_relative_root) = {
            let watched = dag_watched_roots()
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            (
                watched.contains(&canonical_path),
                watched.contains(&relative_path),
            )
        };
        assert!(has_canonical_root);
        assert!(!has_relative_root);

        let res2 = dag_watch_project(canonical_path.clone(), app.handle().clone())
            .await
            .expect("watch project canonical");

        assert_eq!(res2.project_path, canonical_path);
        assert_eq!(res2.runs.len(), 1);
        assert_eq!(
            res2.runs[0].run_id,
            "dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7"
        );
    }
}
