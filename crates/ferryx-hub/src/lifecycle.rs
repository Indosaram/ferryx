//! Who runs the hub when two daemons share one `~/.ferryx/hub`.
//!
//! A v5 handover commits while the predecessor keeps serving retained SSH/paired sessions,
//! sometimes for days. Before 2026.1022.7 the predecessor's hub kept running through that
//! drain: it held `127.0.0.1:9119`, the cron sockets and `control.sock`, kept its own Slack
//! Socket Mode connection and its own cron scheduler, and the successor's binds failed once
//! and were never retried. This module holds the pieces that fix it:
//!
//! * [`TaskRegistry`]: every hub task's abort handle, so a handover commit callback (which is
//!   synchronous) ends Slack, cron, HTTP, the watchers and the control listener at commit.
//!   A retired registry refuses to start again; the persisted `hub_stopped` flag is not
//!   touched, so the successor still opens the hub.
//! * [`preempt_foreign_hub`]: a successor whose predecessor predates the fix finds that
//!   predecessor still answering `control.sock` and sends it `stop` itself.
//! * [`bind_tcp_with_retry`] and [`serve_native_socket_with_retry`]: the successor retries a
//!   bind that is still held, with backoff, until the predecessor lets go.
//! * [`bind_control_socket`]: the control socket is replaced by the daemon that owns the hub,
//!   so `ferryx hub start|stop` reaches the current daemon, not a draining one.

#[cfg(unix)]
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(unix)]
use crate::{HubError, Result};

/// Abort handles of the hub tasks running in this process.
#[derive(Default)]
pub struct TaskRegistry {
    handles: Mutex<Vec<tokio::task::AbortHandle>>,
    retired: AtomicBool,
}

impl TaskRegistry {
    pub const fn new() -> Self {
        Self {
            handles: Mutex::new(Vec::new()),
            retired: AtomicBool::new(false),
        }
    }

    /// Records `handle`. A task tracked after retirement is aborted at once.
    pub fn track<T>(&self, handle: &tokio::task::JoinHandle<T>) {
        if self.is_retired() {
            handle.abort();
            return;
        }
        let mut handles = self.handles.lock().unwrap_or_else(|e| e.into_inner());
        handles.retain(|handle| !handle.is_finished());
        handles.push(handle.abort_handle());
    }

    /// Aborts every tracked task and returns how many were still running.
    pub fn abort_all(&self) -> usize {
        let handles = std::mem::take(&mut *self.handles.lock().unwrap_or_else(|e| e.into_inner()));
        let live = handles
            .iter()
            .filter(|handle| !handle.is_finished())
            .count();
        for handle in handles {
            handle.abort();
        }
        live
    }

    /// Ends the hub in this process for good (handover commit). Safe to call from a
    /// synchronous callback and more than once; only the first call aborts.
    pub fn retire(&self) -> Option<usize> {
        if self.retired.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(self.abort_all())
    }

    pub fn is_retired(&self) -> bool {
        self.retired.load(Ordering::SeqCst)
    }

    pub fn live(&self) -> usize {
        self.handles
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|handle| !handle.is_finished())
            .count()
    }
}

/// Exponential backoff for binds held by a draining predecessor.
#[derive(Debug, Clone)]
pub struct BindBackoff {
    next: Duration,
    max: Duration,
}

impl Default for BindBackoff {
    fn default() -> Self {
        Self::new(Duration::from_millis(250), Duration::from_secs(30))
    }
}

impl BindBackoff {
    pub fn new(first: Duration, max: Duration) -> Self {
        Self { next: first, max }
    }

    pub fn step(&mut self) -> Duration {
        let wait = self.next;
        self.next = (self.next * 2).min(self.max);
        wait
    }
}

/// Binds `addr`, retrying with `backoff` while it is in use. The first failure is logged as a
/// warning and later ones at debug, so a long drain does not flood the log.
pub async fn bind_tcp_with_retry(addr: &str, mut backoff: BindBackoff) -> tokio::net::TcpListener {
    let mut attempts = 0_u32;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                if attempts > 0 {
                    tracing::info!(%addr, attempts, "hub http bound after waiting for the previous owner");
                }
                return listener;
            }
            Err(error) => {
                if attempts == 0 {
                    tracing::warn!(%error, %addr, "hub http is still held; retrying until the previous owner lets go");
                } else {
                    tracing::debug!(%error, %addr, attempts, "hub http bind retry");
                }
                attempts += 1;
                tokio::time::sleep(backoff.step()).await;
            }
        }
    }
}

/// [`crate::schedule::serve_native_socket`] that keeps retrying the bind while another live
/// process owns the socket.
#[cfg(unix)]
pub async fn serve_native_socket_with_retry(
    path: std::path::PathBuf,
    pool: sqlx::SqlitePool,
    mut backoff: BindBackoff,
) {
    let mut attempts = 0_u32;
    loop {
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        match crate::schedule::serve_native_socket(path.clone(), pool.clone(), stop_rx).await {
            Ok(()) => return,
            Err(error) => {
                if attempts == 0 {
                    tracing::warn!(%error, path = %path.display(), "cron socket is still held; retrying until the previous owner lets go");
                } else {
                    tracing::debug!(%error, path = %path.display(), attempts, "cron socket bind retry");
                }
                attempts += 1;
                tokio::time::sleep(backoff.step()).await;
            }
        }
    }
}

/// If another live process answers `control.sock`, it is a predecessor hub from before the
/// commit-time stop. Sends it `stop` and returns its reply. Its daemon and sessions stay up.
/// The old code persists `hub_stopped = 1` on stop; the caller restores the flag it read.
#[cfg(unix)]
pub async fn preempt_foreign_hub(control: &Path) -> Option<String> {
    if !control.exists() {
        return None;
    }
    match crate::control::send_control(control, crate::control::ControlOp::Stop).await {
        Ok(reply) => {
            tracing::info!(reply = %reply.trim(), "stopped the previous daemon's hub over control.sock");
            Some(reply)
        }
        Err(error) => {
            // A stale path (connection refused) is the normal case after a fixed predecessor
            // retired its listener at commit.
            tracing::debug!(%error, "no previous hub answered control.sock");
            None
        }
    }
}

/// Binds `control.sock` for the daemon that owns the hub. A live listener at the path belongs
/// to a draining predecessor (preempted above); its path is replaced atomically, so later
/// `ferryx hub start|stop` calls reach this daemon. The old listener keeps its unlinked inode
/// and nobody new can reach it.
#[cfg(unix)]
pub async fn bind_control_socket(path: &Path) -> Result<tokio::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let compact = uuid::Uuid::new_v4().to_string().replace('-', "");
    let staging = parent.join(format!(".ctl{}", &compact[..8]));
    tokio::fs::create_dir(&staging)
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    let staged = staging.join("s");
    let bound = async {
        tokio::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).await?;
        let listener = tokio::net::UnixListener::bind(&staged)?;
        tokio::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o600)).await?;
        tokio::fs::rename(&staged, path).await?;
        std::io::Result::Ok(listener)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    bound.map_err(|error| HubError::message(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retiring_aborts_every_task_once_and_refuses_new_ones() {
        let registry = TaskRegistry::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let held = tokio::spawn(async move {
            let _keep = listener;
            std::future::pending::<()>().await;
        });
        registry.track(&held);
        assert_eq!(registry.live(), 1);
        assert_eq!(registry.retire(), Some(1));
        assert_eq!(registry.retire(), None);
        assert!(held.await.unwrap_err().is_cancelled());
        // The aborted task dropped its listener, so the port is free again.
        tokio::net::TcpListener::bind(&addr).await.unwrap();
        let late = tokio::spawn(std::future::pending::<()>());
        registry.track(&late);
        assert!(late.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn a_held_port_is_bound_once_the_previous_owner_lets_go() {
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = first.local_addr().unwrap().to_string();
        let waiter = {
            let addr = addr.clone();
            tokio::spawn(async move {
                bind_tcp_with_retry(
                    &addr,
                    BindBackoff::new(Duration::from_millis(20), Duration::from_millis(80)),
                )
                .await
            })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiter.is_finished());
        drop(first);
        let bound = tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bound.local_addr().unwrap().to_string(), addr);
    }

    #[test]
    fn backoff_doubles_up_to_its_cap() {
        let mut backoff = BindBackoff::new(Duration::from_millis(250), Duration::from_secs(1));
        let steps: Vec<_> = (0..5).map(|_| backoff.step().as_millis()).collect();
        assert_eq!(steps, [250, 500, 1000, 1000, 1000]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_successor_stops_a_live_foreign_hub_and_takes_over_its_control_socket() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!(
            "ferryx-ctl-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("control.sock");
        assert!(preempt_foreign_hub(&path).await.is_none());
        // An old-style hub: answers stop, keeps its listener.
        let old = crate::schedule::bind_native_socket(&path).await.unwrap();
        let (seen_tx, mut seen_rx) = tokio::sync::mpsc::unbounded_channel();
        let old_task = tokio::spawn(async move {
            loop {
                let (stream, _) = old.accept().await.unwrap();
                let mut reader = tokio::io::BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    continue;
                }
                let _ = seen_tx.send(line.trim().to_owned());
                let mut stream = reader.into_inner();
                let _ = stream
                    .write_all(b"{\"status\":\"ok\",\"op\":\"stop\"}\n")
                    .await;
            }
        });
        let reply = preempt_foreign_hub(&path).await.unwrap();
        assert!(reply.contains("\"op\":\"stop\""));
        assert_eq!(seen_rx.recv().await.unwrap(), "{\"op\":\"stop\"}");
        let ours = bind_control_socket(&path).await.unwrap();
        let accept = tokio::spawn(async move { ours.accept().await.is_ok() });
        let _client = tokio::net::UnixStream::connect(&path).await.unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), accept)
            .await
            .unwrap()
            .unwrap());
        assert!(
            seen_rx.try_recv().is_err(),
            "new clients reach the successor, not the old hub"
        );
        old_task.abort();
        let _ = std::fs::remove_dir_all(dir);
    }
}
