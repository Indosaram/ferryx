use crate::dag::journal::{parse_run_checkpoint, DagRunSnapshot};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc::Sender;

type TaggedSink = Sender<(String, DagRunSnapshot)>;
use tokio::time::{interval, sleep, MissedTickBehavior};

fn resolve_dag_runs_dir(root: &Path) -> PathBuf {
    let nested = root.join(".omo/senpi-task/dag");
    if nested.join("runs").is_dir() {
        nested.join("runs")
    } else if nested.is_dir() {
        nested
    } else if root.join("runs").is_dir() {
        root.join("runs")
    } else if root.ends_with(".omo/senpi-task/dag") || root.ends_with("dag") {
        root.join("runs")
    } else {
        nested.join("runs")
    }
}

async fn scan_and_emit(
    project_path: &str,
    root: &Path,
    cache: &mut HashMap<String, DagRunSnapshot>,
    sink: &TaggedSink,
) -> bool {
    let runs_dir = resolve_dag_runs_dir(root);
    let target_dir = if runs_dir.is_dir() {
        &runs_dir
    } else if root.is_dir() {
        root
    } else {
        return true;
    };

    let entries = match std::fs::read_dir(target_dir) {
        Ok(e) => e,
        Err(_) => return true,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(snapshot) = parse_run_checkpoint(&content) {
                    let is_updated = match cache.get(&snapshot.run_id) {
                        Some(prev) => prev != &snapshot,
                        None => true,
                    };
                    if is_updated {
                        cache.insert(snapshot.run_id.clone(), snapshot.clone());
                        if sink.send((project_path.to_string(), snapshot)).await.is_err() {
                            return false;
                        }
                    }
                }
            }
        }
    }
    true
}

async fn run_watcher_loop(project_path: String, root: PathBuf, sink: TaggedSink) {
    let mut cache = HashMap::new();
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel(64);

    let watch_target = if root.join(".omo/senpi-task/dag").exists() {
        root.join(".omo/senpi-task/dag")
    } else if root.join("runs").exists()
        || root.ends_with(".omo/senpi-task/dag")
        || root.ends_with("dag")
        || root.exists()
    {
        root.clone()
    } else {
        root.join(".omo/senpi-task/dag")
    };

    let tx_clone = notify_tx.clone();
    let watcher_res = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if res.is_ok() {
                let _ = tx_clone.try_send(());
            }
        },
        Config::default(),
    );

    let (polling_mode, mut _watcher_guard) = match watcher_res {
        Ok(mut watcher) if watch_target.exists() => match watcher.watch(&watch_target, RecursiveMode::Recursive) {
            Ok(()) => (false, Some(watcher)),
            Err(_) => (true, None),
        },
        _ => (true, None),
    };

    if !scan_and_emit(&project_path, &root, &mut cache, &sink).await {
        return;
    }

    let mut debounce_sleep: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
    let mut poll_interval = interval(Duration::from_secs(1));
    poll_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            recv_res = notify_rx.recv() => {
                match recv_res {
                    Some(()) => {
                        debounce_sleep = Some(Box::pin(sleep(Duration::from_millis(250))));
                    }
                    None => {
                        if !polling_mode {
                            break;
                        }
                    }
                }
            }
            _ = async {
                match debounce_sleep.as_mut() {
                    Some(s) => s.as_mut().await,
                    None => std::future::pending().await,
                }
            }, if debounce_sleep.is_some() => {
                debounce_sleep = None;
                if !scan_and_emit(&project_path, &root, &mut cache, &sink).await {
                    break;
                }
            }
            _ = poll_interval.tick(), if polling_mode => {
                if !scan_and_emit(&project_path, &root, &mut cache, &sink).await {
                    break;
                }
            }
        }
    }
}

pub fn spawn_dag_watcher(
    project_path: PathBuf,
    sink: TaggedSink,
) -> tauri::async_runtime::JoinHandle<()> {
    let tagged = project_path.to_string_lossy().to_string();
    tauri::async_runtime::spawn(async move {
        run_watcher_loop(tagged, project_path, sink).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::journal::DagRunStatus;
    use tokio::time::Instant;

    const FIXTURE_F107_JSON: &str =
        include_str!("testdata/dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");

    #[tokio::test]
    async fn test_dag_watcher_detects_checkpoint_changes() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let runs_dir = temp_dir.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");

        let file_path = runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");
        let initial_json = FIXTURE_F107_JSON.replace(
            "\"status\":\"cancelled\"",
            "\"status\":\"running\"",
        );
        std::fs::write(&file_path, &initial_json).expect("write initial json");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let _handle = spawn_dag_watcher(temp_dir.path().to_path_buf(), tx);

        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("first snapshot receive must not time out")
            .expect("first snapshot must be received");
        let (tagged_project, first) = first;
        assert_eq!(tagged_project, temp_dir.path().to_string_lossy().to_string());
        assert_eq!(first.status, DagRunStatus::Running);

        let updated_json = FIXTURE_F107_JSON.replace(
            "\"status\":\"cancelled\"",
            "\"status\":\"completed\"",
        );
        std::fs::write(&file_path, &updated_json).expect("write updated json");

        let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("second snapshot receive must not time out")
            .expect("second snapshot must be received");
        let (tagged_project_second, second) = second;
        assert_eq!(tagged_project_second, tagged_project);
        assert_eq!(second.status, DagRunStatus::Completed);
        assert_ne!((tagged_project, first), (tagged_project_second, second));
    }

    #[tokio::test]
    #[ignore]
    async fn dag_live_tail() {
        let dir = std::env::var("FERRYX_DAG_TAIL_DIR").unwrap_or_else(|_| ".".to_string());
        let path = PathBuf::from(dir);
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let _handle = spawn_dag_watcher(path, tx);
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(snapshot)) => {
                    println!(
                        "runId: {}, status: {:?}, counts: total={}, completed={}, failed={}, cancelled={}, running={}, skipped={}",
                        snapshot.1.run_id,
                        snapshot.1.status,
                        snapshot.1.counts.total,
                        snapshot.1.counts.completed,
                        snapshot.1.counts.failed,
                        snapshot.1.counts.cancelled,
                        snapshot.1.counts.running,
                        snapshot.1.counts.skipped,
                    );
                }
                _ => break,
            }
        }
    }
}
