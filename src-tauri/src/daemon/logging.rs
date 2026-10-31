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
/// `daemon.log.1` ..= `daemon.log.{N}`; with the live file, disk use stays near `(N + 1) * MAX_BYTES`.
const ROTATED_GENERATIONS: usize = 3;
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
        let (file, path) = crate::ipc::run_blocking(|| {
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
            let (mut file, mut path) = (file, path);
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
                    append_bounded(&mut file, &path, &bytes).map_err(|e| {
                        crate::ipc::error::IpcError::internal(format!("daemon log write: {e}"))
                    })?;
                    Ok((file, path))
                })
                .await;
                (file, path) = match written {
                    Ok(state) => state,
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

/// Formats the termination-signal record written by [`next_shutdown_signal`].
///
/// Split out so the shape is testable without installing a process-wide handler.
#[cfg(any(unix, test))]
pub(crate) fn signal_record(signal: i32) -> String {
    format!("daemon_signal signal={signal} action=shutdown\n")
}

/// Appends one durable record to the private daemon log.
pub(crate) fn append_record(record: &str) {
    if let Ok((mut file, path)) = open_log() {
        let _ = append_bounded(&mut file, &path, record.as_bytes());
    }
}

/// Resolves on the first `SIGTERM`, `SIGINT`, or `SIGHUP` this daemon receives.
///
/// The daemon had no signal handling at all, which is why the 2026-10-07 death produced no record:
/// a plain `SIGTERM` terminates the process with the default disposition and no code of ours runs,
/// so every PTY it owned vanished with nothing in `daemon.log` to say why. Owning the signal turns
/// that silence into a timestamped line plus a flush of what outlives the process.
#[cfg(unix)]
pub(crate) async fn next_shutdown_signal() -> Option<i32> {
    use tokio::signal::unix::{signal, SignalKind};
    let mut terminate = signal(SignalKind::terminate()).ok()?;
    let mut interrupt = signal(SignalKind::interrupt()).ok()?;
    let mut hangup = signal(SignalKind::hangup()).ok()?;
    tokio::select! {
        _ = terminate.recv() => Some(libc::SIGTERM),
        _ = interrupt.recv() => Some(libc::SIGINT),
        _ = hangup.recv() => Some(libc::SIGHUP),
    }
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
        append_record(&record);
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

fn open_log() -> io::Result<(std::fs::File, std::path::PathBuf)> {
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
    let path = dir.join("daemon.log");
    let file = options.open(&path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("daemon log must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    Ok((file, path))
}

fn append_bounded(
    file: &mut std::fs::File,
    path: &std::path::Path,
    bytes: &[u8],
) -> io::Result<()> {
    #[cfg(test)]
    if std::env::var("FERRYX_LOGGING_FIXTURE").as_deref() == Ok("write_failure") {
        return Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "fixture disk full",
        ));
    }
    // A shared file (including during handover); lock before checking size or rotating.
    file.lock()?;
    let result = (|| {
        if file.metadata()?.len() + bytes.len() as u64 > MAX_BYTES {
            rotate(file, path)?;
        }
        file.seek(io::SeekFrom::End(0))?;
        file.write_all(bytes)?;
        file.flush()
    })();
    let unlocked = file.unlock();
    result.and(unlocked)
}

fn rotated_path(path: &std::path::Path, generation: usize) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{generation}"));
    name.into()
}

fn ignore_not_found(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Moves the full log into `daemon.log.1` (older generations shift up, the oldest falls off) and
/// leaves `file` empty. Caller holds `file.lock()`.
///
/// Copy-then-truncate rather than rename: during a handover both daemons hold an fd on
/// `daemon.log`, so renaming it would leave the other writer appending into `daemon.log.1`
/// forever; truncating the shared inode under the lock moves every holder to the fresh log.
fn rotate(file: &mut std::fs::File, path: &std::path::Path) -> io::Result<()> {
    for generation in (1..ROTATED_GENERATIONS).rev() {
        // Renaming a symlink moves the link itself; it never touches the target.
        ignore_not_found(std::fs::rename(
            rotated_path(path, generation),
            rotated_path(path, generation + 1),
        ))?;
    }
    let first = rotated_path(path, 1);
    ignore_not_found(std::fs::remove_file(&first))?;
    let mut options = std::fs::OpenOptions::new();
    // create_new (O_EXCL) refuses any entry that reappeared, symlink or not.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut rotated = options.open(&first)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        rotated.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.seek(io::SeekFrom::Start(0))?;
    io::copy(file, &mut rotated)?;
    rotated.flush()?;
    file.set_len(0)
}

#[cfg(test)]
mod tests {
    use super::{panic_record, signal_record};

    #[test]
    fn panic_record_carries_location_thread_and_payload() {
        let record = panic_record("src/daemon/server.rs:42:9", "tokio-runtime-worker", "boom");
        assert_eq!(
            record,
            "daemon_panic thread=tokio-runtime-worker at=src/daemon/server.rs:42:9 payload=boom\n"
        );
    }

    #[test]
    fn signal_record_names_the_signal() {
        assert_eq!(signal_record(15), "daemon_signal signal=15 action=shutdown\n");
    }

    #[test]
    fn panic_record_bounds_an_oversized_payload() {
        // A pathological payload must not blow past the bounded log record size.
        let record = panic_record("at", "t", &"x".repeat(4096));
        assert!(record.len() < 1100, "record length {}", record.len());
        assert!(record.ends_with('\n'));
    }

    use super::*;

    fn fixture_log() -> (tempfile::TempDir, std::path::PathBuf, std::fs::File) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        // Fresh worktrees have no target/ inside the manifest dir (CARGO_TARGET_DIR may
        // point elsewhere); tempdir_in requires the directory to exist.
        std::fs::create_dir_all(&root).unwrap();
        let dir = tempfile::tempdir_in(root).unwrap();
        let path = dir.path().join("daemon.log");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        (dir, path, file)
    }

    /// Writes `marker` and pads the log to exactly its disk limit.
    fn fill_to_limit(file: &mut std::fs::File, marker: &str) {
        file.set_len(0).unwrap();
        file.seek(io::SeekFrom::Start(0)).unwrap();
        file.write_all(marker.as_bytes()).unwrap();
        file.set_len(MAX_BYTES).unwrap();
    }

    #[test]
    fn full_log_rotates_old_records_into_first_generation() {
        // Given: an existing log at its disk limit, including across daemon restarts.
        let (_dir, path, mut file) = fixture_log();
        fill_to_limit(&mut file, "relay_warning=old\n");
        // When: another record is persisted.
        append_bounded(&mut file, &path, b"reason=manual_reset\n").unwrap();
        // Then: the history survives in daemon.log.1 and the live log holds only the new record.
        let rotated = std::fs::read(rotated_path(&path, 1)).unwrap();
        assert_eq!(rotated.len() as u64, MAX_BYTES);
        assert!(rotated.starts_with(b"relay_warning=old\n"));
        assert_eq!(std::fs::read(&path).unwrap(), b"reason=manual_reset\n");
        assert_eq!(file.metadata().unwrap().len(), 20);
    }

    #[test]
    fn rotation_caps_the_generation_count() {
        let (_dir, path, mut file) = fixture_log();
        for generation in 0..5 {
            fill_to_limit(&mut file, &format!("generation={generation}\n"));
            append_bounded(&mut file, &path, b"next\n").unwrap();
        }
        // Newest first: .1 holds the last full log, .3 the oldest kept; nothing beyond.
        for (rotated, generation) in [(1, 4), (2, 3), (3, 2)] {
            let content = std::fs::read(rotated_path(&path, rotated)).unwrap();
            assert!(
                content.starts_with(format!("generation={generation}\n").as_bytes()),
                "daemon.log.{rotated}"
            );
        }
        assert!(!rotated_path(&path, ROTATED_GENERATIONS + 1).exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"next\n");
    }

    #[cfg(unix)]
    #[test]
    fn rotated_log_is_private_and_never_follows_a_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, path, mut file) = fixture_log();
        // A planted daemon.log.1 symlink must be replaced, not written through.
        let victim = dir.path().join("victim");
        std::fs::write(&victim, b"untouched").unwrap();
        std::os::unix::fs::symlink(&victim, rotated_path(&path, 1)).unwrap();
        fill_to_limit(&mut file, "old\n");
        append_bounded(&mut file, &path, b"new\n").unwrap();
        let first = rotated_path(&path, 1);
        let metadata = std::fs::symlink_metadata(&first).unwrap();
        assert!(metadata.file_type().is_file());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        // The displaced link shifted to .2 as a link; its target is still never written.
        assert!(std::fs::symlink_metadata(rotated_path(&path, 2))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
