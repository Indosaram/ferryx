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

#[derive(Clone, Default)]
struct WatcherHooks {
    #[cfg(test)]
    events: Option<tokio::sync::mpsc::UnboundedSender<&'static str>>,
    #[cfg(test)]
    scan: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl WatcherHooks {
    fn event(&self, _event: &'static str) {
        #[cfg(test)]
        if let Some(events) = &self.events {
            let _ = events.send(_event);
        }
    }

    fn scanning(&self) {
        #[cfg(test)]
        if let Some(scan) = &self.scan {
            scan();
        }
    }
}

async fn scan_and_emit(
    project_path: &str,
    root: &Path,
    cache: &mut HashMap<String, DagRunSnapshot>,
    sink: &TaggedSink,
    hooks: &WatcherHooks,
) -> bool {
    let runs_dir = resolve_dag_runs_dir(root);
    let target_dir = if runs_dir.is_dir() {
        &runs_dir
    } else if root.is_dir()
        && (root.join("runs").is_dir()
            || root.ends_with(".omo/senpi-task/dag")
            || root.ends_with("dag"))
    {
        root
    } else {
        // Even the "nothing to scan" determination is filesystem work, so it must be
        // observed off the async worker like any other scan.
        let probe_hooks = hooks.clone();
        let _ = tokio::task::spawn_blocking(move || probe_hooks.scanning()).await;
        return true;
    };

    // The directory walk, the file reads and the JSON parses all run on a blocking
    // thread. Doing them inline occupied a Tokio worker for the whole scan -- a
    // directory walk plus N synchronous reads and parses -- which stalls other tasks
    // on that worker and stalls the entire runtime on a current-thread executor.
    // Only the sink sends stay on the async side.
    let scan_dir = target_dir.to_path_buf();
    let scan_hooks = hooks.clone();
    let snapshots = match tokio::task::spawn_blocking(move || {
        // The scan observation point belongs INSIDE the blocking closure: the whole
        // point is that the walk, reads and parses happen off the async worker, so a
        // hook that fired on the async side would report the wrong thread.
        scan_hooks.scanning();
        let mut collected: Vec<DagRunSnapshot> = Vec::new();
        let entries = match std::fs::read_dir(&scan_dir) {
            Ok(e) => e,
            Err(_) => return collected,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|ext| ext == "json") {
                continue;
            }
            // A checkpoint can be observed mid-write; retry briefly before giving up.
            for attempt in 0..3 {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(snapshot) = parse_run_checkpoint(&content) {
                        collected.push(snapshot);
                        break;
                    }
                }
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
        collected
    })
    .await
    {
        Ok(snapshots) => snapshots,
        Err(_) => return true,
    };

    for snapshot in snapshots {
        let is_updated = match cache.get(&snapshot.run_id) {
            Some(prev) => prev != &snapshot,
            None => true,
        };
        if is_updated {
            cache.insert(snapshot.run_id.clone(), snapshot.clone());
            if sink
                .send((project_path.to_string(), snapshot))
                .await
                .is_err()
            {
                return false;
            }
        }
    }
    true
}

async fn run_watcher_loop(project_path: String, root: PathBuf, sink: TaggedSink) {
    run_watcher_loop_observed(project_path, root, sink, WatcherHooks::default(), None).await;
}

async fn run_watcher_loop_observed(
    project_path: String,
    root: PathBuf,
    sink: TaggedSink,
    hooks: WatcherHooks,
    mut lose_watch: Option<tokio::sync::oneshot::Receiver<()>>,
) {
    let mut cache = HashMap::new();
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel(64);

    // Only journal directories are watched. Falling back to the project root would arm a
    // recursive watch over an entire source tree for every session root we track.
    let watch_target = if root.join(".omo/senpi-task/dag").exists() {
        root.join(".omo/senpi-task/dag")
    } else if root.join("runs").exists()
        || root.ends_with(".omo/senpi-task/dag")
        || root.ends_with("dag")
    {
        root.clone()
    } else {
        root.join(".omo/senpi-task/dag")
    };

    // Hydrate before arming: starting a filesystem watch can stall for seconds on a loaded
    // host, and the current inventory must not wait on it.
    if !scan_and_emit(&project_path, &root, &mut cache, &sink, &hooks).await {
        return;
    }

    let tx_clone = notify_tx.clone();
    let watcher_res = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let is_noisy = event.paths.iter().any(|p| {
                    p.components().any(|c| {
                        let s = c.as_os_str();
                        s == "node_modules"
                            || s == "target"
                            || s == ".git"
                            || s == "dist"
                            || s == ".cache"
                    })
                });
                if !is_noisy {
                    let _ = tx_clone.try_send(());
                }
            }
        },
        Config::default(),
    );

    let (mut polling_mode, mut _watcher_guard) = match watcher_res {
        Ok(mut watcher) if watch_target.exists() => {
            match watcher.watch(&watch_target, RecursiveMode::Recursive) {
                Ok(()) => (false, Some(watcher)),
                Err(_) => (true, None),
            }
        }
        _ => (true, None),
    };

    hooks.event(if polling_mode { "polling" } else { "armed" });
    // Second pass: anything written while the watch was arming produced no event.
    if !scan_and_emit(&project_path, &root, &mut cache, &sink, &hooks).await {
        return;
    }
    hooks.event("scanned");

    let mut debounce_sleep: Option<std::pin::Pin<Box<tokio::time::Sleep>>> = None;
    let mut poll_interval = interval(Duration::from_secs(1));
    poll_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            _ = async {
                match lose_watch.as_mut() {
                    Some(signal) => { let _ = signal.await; }
                    None => std::future::pending::<()>().await,
                }
            } => {
                // Models the Windows backend silently losing its native watch.
                lose_watch = None;
                drop(_watcher_guard.take());
                notify_rx.close();
                while notify_rx.try_recv().is_ok() {}
                debounce_sleep = None;
                // Fall back to interval reconciliation. Without this every arm of the
                // select is disabled once the native watch is gone -- the notify arm is
                // gated off by `!notify_rx.is_closed()`, the poll arm by `polling_mode`,
                // `lose_watch` is now None and `debounce_sleep` is None -- so the task
                // parks forever and DAG progress silently freezes until the app restarts.
                polling_mode = true;
                if !scan_and_emit(&project_path, &root, &mut cache, &sink, &hooks).await {
                    break;
                }
                hooks.event("lost-scanned");
            }
            recv_res = notify_rx.recv(), if !notify_rx.is_closed() => {
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
                if !scan_and_emit(&project_path, &root, &mut cache, &sink, &hooks).await {
                    break;
                }
                hooks.event("scanned");
            }
            _ = poll_interval.tick(), if polling_mode => {
                if !scan_and_emit(&project_path, &root, &mut cache, &sink, &hooks).await {
                    break;
                }
                hooks.event("scanned");
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

    async fn next_event(
        events: &mut tokio::sync::mpsc::UnboundedReceiver<&'static str>,
        expected: &'static str,
    ) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if events.recv().await.expect("watcher event stream closed") == expected {
                    break;
                }
            }
        })
        .await
        .expect("watcher state deadline");
    }

    #[tokio::test]
    async fn test_dag_watcher_recovers_after_silent_watch_loss() {
        // Given an armed real native watch, with event subscriptions installed first.
        let temp = tempfile::tempdir().expect("owned journal root");
        let root = temp.path().to_path_buf();
        let dag = root.join(".omo/senpi-task/dag");
        let runs = dag.join("runs");
        std::fs::create_dir_all(&runs).expect("create watched directory");
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let (events_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
        let (lose_tx, lose_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(run_watcher_loop_observed(
            root.to_string_lossy().into_owned(),
            root.clone(),
            tx,
            WatcherHooks {
                events: Some(events_tx),
                scan: None,
            },
            Some(lose_rx),
        ));
        let outcome = async {
            next_event(&mut events, "armed").await;
            next_event(&mut events, "scanned").await;
            // When the entire watched target is removed and the backend loses its handle.
            std::fs::remove_dir_all(&dag).expect("delete entire watch target");
            lose_tx.send(()).expect("inject silent backend loss");
            next_event(&mut events, "lost-scanned").await;
            std::fs::create_dir_all(&runs).expect("recreate journal");
            let checkpoint = runs.join("checkpoint.json");
            std::fs::write(
                &checkpoint,
                FIXTURE_F107_JSON.replace("\"status\":\"cancelled\"", "\"status\":\"running\""),
            )
            .expect("write recreated checkpoint");
            // Then periodic reconciliation must recover without any native notification.
            let first = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
            if let Ok(Some((_, snapshot))) = &first {
                assert_eq!(snapshot.status, DagRunStatus::Running);
            }
            if !matches!(first, Ok(Some(_))) {
                return (first, None);
            }
            std::fs::write(
                &checkpoint,
                FIXTURE_F107_JSON.replace("\"status\":\"cancelled\"", "\"status\":\"completed\""),
            )
            .expect("write subsequent update");
            let second = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
            (first, Some(second))
        }
        .await;
        task.abort();
        assert!(task.await.expect_err("watcher cancelled").is_cancelled());
        let first = outcome
            .0
            .expect("recreated journal must emit after silent watch loss")
            .expect("snapshot sink remains open");
        assert_eq!(first.0, root.to_string_lossy());
        let second = outcome
            .1
            .expect("subsequent update attempted")
            .expect("subsequent update deadline")
            .expect("subsequent snapshot");
        assert_eq!(second.1.status, DagRunStatus::Completed);
    }

    #[test]
    fn test_dag_scan_runs_off_async_worker() {
        // Given a current-thread runtime and externally controlled blocking scan boundary.
        let temp = tempfile::tempdir().expect("owned scan root");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime");
        let async_thread = std::thread::current().id();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (sentinel_tx, sentinel_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = std::sync::Mutex::new(release_rx);
        let hooks = WatcherHooks {
            events: None,
            scan: Some(std::sync::Arc::new(move || {
                entered_tx
                    .send(std::thread::current().id())
                    .expect("record scan identity");
                release_rx
                    .lock()
                    .expect("release receiver")
                    .recv()
                    .expect("scan released");
            })),
        };
        let controller = std::thread::spawn(move || {
            let worker = entered_rx.recv_timeout(Duration::from_secs(5));
            // RED must release the scan too: a bounded deadline is not success evidence.
            let sentinel = sentinel_rx.recv_timeout(Duration::from_secs(2));
            release_tx.send(()).expect("release owned scan");
            (worker, sentinel)
        });
        // When the real scan starts, another task must run before its barrier is released.
        runtime.block_on(async {
            let (tx, _rx) = tokio::sync::mpsc::channel(10);
            let mut cache = HashMap::new();
            let scan = scan_and_emit("owned", temp.path(), &mut cache, &tx, &hooks);
            let sentinel = async {
                sentinel_tx.send(()).expect("async sentinel");
            };
            let (open, ()) = tokio::join!(biased; scan, sentinel);
            assert!(open);
        });
        let (worker, sentinel) = controller.join().expect("controller joined");
        // Then neither metadata, file reads nor parsing may occupy the async worker.
        assert_ne!(
            worker.expect("scan entered"),
            async_thread,
            "scan must run off async worker"
        );
        sentinel.expect("async sentinel must execute before scan release");
    }

    #[tokio::test]
    async fn test_dag_watcher_detects_checkpoint_changes() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let runs_dir = temp_dir.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");

        let file_path = runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json");
        let initial_json =
            FIXTURE_F107_JSON.replace("\"status\":\"cancelled\"", "\"status\":\"running\"");
        std::fs::write(&file_path, &initial_json).expect("write initial json");

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let _handle = spawn_dag_watcher(temp_dir.path().to_path_buf(), tx);

        let first = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("first snapshot receive must not time out")
            .expect("first snapshot must be received");
        let (tagged_project, first) = first;
        assert_eq!(
            tagged_project,
            temp_dir.path().to_string_lossy().to_string()
        );
        assert_eq!(first.status, DagRunStatus::Running);

        let updated_json =
            FIXTURE_F107_JSON.replace("\"status\":\"cancelled\"", "\"status\":\"completed\"");
        std::fs::write(&file_path, &updated_json).expect("write updated json");

        let second = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("second snapshot receive must not time out")
            .expect("second snapshot must be received");
        let (tagged_project_second, second) = second;
        assert_eq!(tagged_project_second, tagged_project);
        assert_eq!(second.status, DagRunStatus::Completed);
        assert_ne!((tagged_project, first), (tagged_project_second, second));
    }

    #[tokio::test]
    async fn test_dag_watcher_reports_journal_created_after_start() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let _handle = spawn_dag_watcher(temp_dir.path().to_path_buf(), tx);

        let runs_dir = temp_dir.path().join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs_dir).expect("create runs dir");
        std::fs::write(
            runs_dir.join("dag_f107f318-ac78-46a2-b8c6-584b4e10eaa7.json"),
            FIXTURE_F107_JSON.replace("\"status\":\"cancelled\"", "\"status\":\"running\""),
        )
        .expect("write journal");

        let (tagged_project, snapshot) = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("snapshot receive must not time out")
            .expect("snapshot must be received");
        assert_eq!(
            tagged_project,
            temp_dir.path().to_string_lossy().to_string()
        );
        assert_eq!(snapshot.status, DagRunStatus::Running);
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
