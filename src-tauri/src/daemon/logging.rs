//! Headless daemon log: private, bounded, and independent of stderr pipes.
//!
//! The daemon's stderr is not durable anywhere: the desktop spawns it with a pipe it never
//! drains, and a handover successor inherits that dead pipe while the spawner points its stdout
//! at `/dev/null`. This file is therefore the only surface that can carry a daemon death, a
//! refused transfer, or an unexpected second handover, so it admits every record at INFO or
//! below -- not only agent-state diagnostics.
use std::io::{self, Seek, Write};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::mpsc;

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_RECORD: usize = 8192;

enum Message {
    Record(Vec<u8>),
    Stop,
}

#[derive(Clone)]
struct Writer {
    tx: mpsc::Sender<Message>,
    dropped: Arc<AtomicUsize>,
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_RECORD || self.tx.try_send(Message::Record(bytes.to_vec())).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) struct DaemonLogging {
    tx: mpsc::Sender<Message>,
    task: tokio::task::JoinHandle<()>,
}

impl DaemonLogging {
    pub(crate) async fn start() -> anyhow::Result<Self> {
        let file = crate::ipc::run_blocking(|| {
            open_log().map_err(|e| {
                crate::ipc::error::IpcError::internal(format!("daemon log initialization: {e}"))
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let (tx, mut rx) = mpsc::channel(256);
        let dropped = Arc::new(AtomicUsize::new(0));
        let writer = Writer {
            tx: tx.clone(),
            dropped: dropped.clone(),
        };
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(move || writer.clone())
                    // Every target at INFO or below. Restricting this to one target once made a
                    // handover invisible: the successor logged `Starting daemon with handover
                    // from ...` and one line per adopted session, and all of it was dropped by
                    // this filter, so the 2026-09-26 handover that lost 26 of 37 sessions left no
                    // primary evidence at all.
                    .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                        *meta.level() <= tracing::Level::INFO
                    })),
            )
            .try_init()?;
        let task = tokio::spawn(async move {
            let mut file = file;
            while let Some(message) = rx.recv().await {
                let (mut bytes, stop) = match message {
                    Message::Record(bytes) => (bytes, false),
                    Message::Stop => (Vec::new(), true),
                };
                let lost = dropped.swap(0, Ordering::Relaxed);
                if lost != 0 {
                    bytes.extend_from_slice(
                        format!("daemon_log_dropped_records={lost}\n").as_bytes(),
                    );
                }
                let written = crate::ipc::run_blocking(move || {
                    append_bounded(&mut file, &bytes).map_err(|e| {
                        crate::ipc::error::IpcError::internal(format!("daemon log write: {e}"))
                    })?;
                    Ok(file)
                })
                .await;
                file = match written {
                    Ok(file) => file,
                    Err(error) => {
                        report_failure(&anyhow::anyhow!("{error:?}"));
                        return;
                    }
                };
                if stop {
                    break;
                }
            }
        });
        Ok(Self { tx, task })
    }

    pub(crate) async fn finish(self) -> anyhow::Result<()> {
        // Stop follows all already-queued records; await the actual disk writes, not a delay.
        // A closed receiver means the worker already reported a write failure.
        let _closed_or_sent = self.tx.send(Message::Stop).await;
        self.task.await?;
        Ok(())
    }
}

/// Formats the panic record written by [`install_panic_recorder`].
///
/// Split out so the shape is testable without installing a process-wide hook.
pub(crate) fn panic_record(location: &str, thread: &str, payload: &str) -> String {
    format!(
        "daemon_panic thread={thread} at={location} payload={}\n",
        payload.chars().take(1024).collect::<String>()
    )
}

/// Appends every panic to the persistent daemon log before chaining to the previous hook.
///
/// A panic is the event that explains a daemon's death, and stderr is not durable here: the
/// GUI spawns the daemon with a piped stderr that it discards, so a panic leaves no trace on
/// disk. On 2026-09-25 a daemon died mid-write of `remote_sessions.json` -- the half-written
/// temporary file survived, but nothing on the machine recorded why. Writing the message,
/// location, and thread into the same bounded file as the agent-state records closes that gap
/// without depending on the desktop draining a pipe.
pub(crate) fn install_panic_recorder() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_string());
        let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "non-string panic payload".to_string()
        };
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        let record = panic_record(&location, &thread, &payload);
        if let Ok(mut file) = open_log() {
            let _ = append_bounded(&mut file, record.as_bytes());
        }
        previous(info);
    }));
}

pub(crate) fn report_failure(error: &anyhow::Error) {
    // One bounded report, never a retry loop into the desktop's undrained pipe.
    // Use non-panicking write_all on locked stderr so broken pipes do not abort the process.
    let detail: String = error.to_string().chars().take(256).collect();
    let message = format!("FERRYX_DAEMON_LOGGING_DISABLED: {detail}\n");
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(message.as_bytes());
    let _ = stderr.flush();
}

fn open_log() -> io::Result<std::fs::File> {
    use std::path::PathBuf;
    let base = std::env::var_os("FERRYX_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            #[cfg(windows)]
            {
                std::env::var_os("LOCALAPPDATA")
                    .map(|p| PathBuf::from(p).join("Ferryx"))
                    .or_else(|| {
                        std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".ferryx"))
                    })
            }
            #[cfg(not(windows))]
            {
                std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".ferryx"))
            }
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "no private daemon data directory")
        })?;
    let dir = base.join("logs");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    if std::fs::symlink_metadata(&dir)?.file_type().is_symlink() {
        return Err(io::Error::other(
            "daemon log directory must not be a symlink",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(dir.join("daemon.log"))?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("daemon log must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn append_bounded(file: &mut std::fs::File, bytes: &[u8]) -> io::Result<()> {
    #[cfg(test)]
    if std::env::var("FERRYX_LOGGING_FIXTURE").as_deref() == Ok("write_failure") {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "fixture disk full",
        ));
    }
    // A shared file (including during handover); lock before checking size or truncating.
    file.lock()?;
    let result = (|| {
        if file.metadata()?.len() + bytes.len() as u64 > MAX_BYTES {
            file.set_len(0)?;
        }
        file.seek(io::SeekFrom::End(0))?;
        file.write_all(bytes)?;
        file.flush()
    })();
    let unlocked = file.unlock();
    result.and(unlocked)
}

#[cfg(test)]
mod tests {
    use super::panic_record;

    #[test]
    fn panic_record_carries_location_thread_and_payload() {
        let record = panic_record("src/daemon/server.rs:42:9", "tokio-runtime-worker", "boom");
        assert_eq!(
            record,
            "daemon_panic thread=tokio-runtime-worker at=src/daemon/server.rs:42:9 payload=boom\n"
        );
    }

    #[test]
    fn panic_record_bounds_an_oversized_payload() {
        // A pathological payload must not blow past the bounded log record size.
        let record = panic_record("at", "t", &"x".repeat(4096));
        assert!(record.len() < 1100, "record length {}", record.len());
        assert!(record.ends_with('\n'));
    }

    use super::*;

    #[test]
    fn full_log_discards_old_records_before_appending() {
        // Given: an existing log at its disk limit, including across daemon restarts.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        // Fresh worktrees have no target/ inside the manifest dir (CARGO_TARGET_DIR may
        // point elsewhere); tempfile_in requires the directory to exist.
        std::fs::create_dir_all(&root).unwrap();
        let mut file = tempfile::tempfile_in(root).unwrap();
        file.set_len(MAX_BYTES).unwrap();
        // When: another record is persisted.
        append_bounded(&mut file, b"reason=manual_reset\n").unwrap();
        // Then: the retained file contains only the new record, not an unbounded archive.
        assert_eq!(file.metadata().unwrap().len(), 20);
    }
}
