use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::ipc::run_blocking;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State};
use tauri::utils::{config::BundleType, platform::bundle_type};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Host platform of the running binary, as a plain value so the ownership decision below can be
/// unit tested for every platform from any platform.
enum UpdateHost {
    Windows,
    MacOs,
    Linux,
}

impl UpdateHost {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// Pure decision behind [`updater_managed_externally`]: whether another channel owns update
/// delivery for this install.
///
/// A Linux distribution package is owned by the package manager: `tauri-plugin-updater` routes a
/// `Deb`/`Rpm` bundle to `dpkg`/`rpm`, which reject the raw AppImage Ferryx publishes, so an
/// in-app update there can never succeed. An AppImage is replaced in place, and an unpackaged
/// build reports no bundle type and keeps self-updating. Windows Store ownership is a
/// package-identity fact rather than a bundle type (`BundleType::Msi` is the self-updating MSI
/// installer), so the bundle decision never claims a Windows install.
fn managed_externally_for(host: UpdateHost, bundle: Option<BundleType>) -> bool {
    match host {
        UpdateHost::Linux => matches!(bundle, Some(BundleType::Deb | BundleType::Rpm)),
        UpdateHost::Windows | UpdateHost::MacOs => false,
    }
}

#[cfg(windows)]
fn has_package_identity() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentPackageFullName(
            packageFullNameLength: *mut u32,
            packageFullName: *mut u16,
        ) -> i32;
    }
    let mut len: u32 = 0;
    let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
    rc == 122 || rc == 0
}

/// Store (MSIX) installs live under the write-protected `WindowsApps` tree where the Store
/// owns update delivery; self-updating those installs would fail or duplicate the app. The
/// bundle term is always false here and keeps the whole decision in one place.
#[cfg(windows)]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
        || has_package_identity()
        || std::env::var_os("FERRYX_STORE_BUILD").is_some()
        || std::env::current_exe()
            .map(|exe| {
                let p = exe.to_string_lossy().to_lowercase();
                p.contains(r"\windowsapps\")
                    || p.contains("windowsapps")
                    || exe.parent().map(|d| d.join("msix.marker").exists()).unwrap_or(false)
            })
            .unwrap_or(false)
}

/// macOS self-updates from the `.app`. On Linux only the AppImage is replaced in place by the
/// in-app updater; a distribution package is updated by `dpkg`/`rpm` instead.
#[cfg(not(windows))]
pub fn updater_managed_externally() -> bool {
    managed_externally_for(UpdateHost::current(), bundle_type())
}

#[tauri::command]
pub async fn cmd_updater_managed_externally() -> Result<bool, IpcError> {
    Ok(updater_managed_externally())
}

/// Windows ships through two channels with different update ownership: MSIX installs are
/// updated by the Microsoft Store, NSIS installs by the in-app updater. macOS and the Linux
/// AppImage self-update; a Linux distribution package keeps the same `native` channel while
/// `updater_managed_externally` hides the in-app update card.
pub fn distribution_channel() -> &'static str {
    if cfg!(windows) {
        if updater_managed_externally() {
            "store"
        } else {
            "installer"
        }
    } else {
        "native"
    }
}

#[tauri::command]
pub async fn cmd_distribution_channel() -> Result<&'static str, IpcError> {
    Ok(distribution_channel())
}

/// No-progress window: a download that delivers no bytes for this long is abandoned. It is not a
/// total-request timeout, so a slow download that keeps delivering bytes never trips it.
const DEFAULT_STALL_WINDOW: Duration = Duration::from_secs(60);

const UPDATER_MANAGED_EXTERNALLY: &str = "UPDATER_MANAGED_EXTERNALLY";
const UPDATER_VERSION_MISMATCH: &str = "UPDATER_VERSION_MISMATCH";
const UPDATER_NO_PENDING_INSTALL: &str = "UPDATER_NO_PENDING_INSTALL";

/// Progress stream delivered to the frontend through the command's channel.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum UpdaterDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started { content_length: Option<u64> },
    #[serde(rename_all = "camelCase")]
    Progress { chunk_length: usize },
    Finished,
    Cancelled,
    Stalled,
    Error { message: String },
}

/// What the download task reports while it runs.
#[derive(Debug)]
enum DownloadSignal {
    Chunk {
        len: usize,
        content_length: Option<u64>,
    },
    /// The body is fully received; only signature verification remains, so the no-progress
    /// watchdog is disarmed.
    BodyComplete,
    Done(Result<Vec<u8>, String>),
}

#[derive(Debug, PartialEq)]
enum DownloadOutcome {
    Completed(Vec<u8>),
    Cancelled,
    Stalled,
    Failed(String),
}

/// Supervises one download: races the signal stream against the cancel token and a no-progress
/// watchdog. The watchdog is armed before the first signal, so a server that never answers the
/// request (no headers, no bytes) stalls exactly like one that stops mid-body, and every chunk
/// pushes the deadline out again.
async fn supervise_download(
    mut signals: mpsc::UnboundedReceiver<DownloadSignal>,
    cancel: CancellationToken,
    stall_window: Duration,
    mut on_event: impl FnMut(UpdaterDownloadEvent),
) -> DownloadOutcome {
    let mut deadline = tokio::time::Instant::now() + stall_window;
    let mut watchdog_armed = true;
    let mut started = false;
    loop {
        let stall = async {
            if watchdog_armed {
                tokio::time::sleep_until(deadline).await
            } else {
                std::future::pending::<()>().await
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return DownloadOutcome::Cancelled,
            signal = signals.recv() => match signal {
                Some(DownloadSignal::Chunk { len, content_length }) => {
                    if !started {
                        started = true;
                        on_event(UpdaterDownloadEvent::Started { content_length });
                    }
                    on_event(UpdaterDownloadEvent::Progress { chunk_length: len });
                    deadline = tokio::time::Instant::now() + stall_window;
                }
                Some(DownloadSignal::BodyComplete) => watchdog_armed = false,
                Some(DownloadSignal::Done(Ok(bytes))) => return DownloadOutcome::Completed(bytes),
                Some(DownloadSignal::Done(Err(message))) => return DownloadOutcome::Failed(message),
                None => {
                    return DownloadOutcome::Failed("update download ended without a result".into())
                }
            },
            _ = stall => return DownloadOutcome::Stalled,
        }
    }
}

/// Download bookkeeping kept in managed state: the cancel token of the running download (tagged
/// with a generation so a superseded download cannot claim the slot) and the verified bytes
/// waiting for install. Generic over the pending payload so the state machine is testable
/// without a real [`Update`].
struct DownloadSlot<P> {
    inner: Mutex<SlotInner<P>>,
}

struct SlotInner<P> {
    generation: u64,
    running: Option<(u64, CancellationToken)>,
    pending: Option<P>,
}

struct DownloadTicket {
    generation: u64,
    token: CancellationToken,
}

impl<P> Default for DownloadSlot<P> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(SlotInner {
                generation: 0,
                running: None,
                pending: None,
            }),
        }
    }
}

impl<P> DownloadSlot<P> {
    fn lock(&self) -> std::sync::MutexGuard<'_, SlotInner<P>> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a download, dropping any earlier pending bytes. Refuses a second concurrent one.
    fn begin(&self) -> Result<DownloadTicket, IpcError> {
        let mut inner = self.lock();
        if inner.running.is_some() {
            return Err(IpcError::new(
                IpcErrorCode::RequestConflict,
                "an update download is already running",
            ));
        }
        inner.pending = None;
        inner.generation += 1;
        let token = CancellationToken::new();
        inner.running = Some((inner.generation, token.clone()));
        Ok(DownloadTicket {
            generation: inner.generation,
            token,
        })
    }

    /// Ends the download that owns `ticket`. Bytes are kept only for a completed download whose
    /// token was never cancelled, so a cancel that races completion still leaves nothing to
    /// install. Returns whether the payload was stored.
    fn finish(&self, ticket: &DownloadTicket, payload: Option<P>) -> bool {
        let mut inner = self.lock();
        let owns_slot = inner
            .running
            .as_ref()
            .is_some_and(|(generation, _)| *generation == ticket.generation);
        if owns_slot {
            inner.running = None;
        }
        match payload {
            Some(payload) if owns_slot && !ticket.token.is_cancelled() => {
                inner.pending = Some(payload);
                true
            }
            _ => false,
        }
    }

    /// Cancels the running download (if any) and discards pending bytes.
    fn cancel(&self) {
        let mut inner = self.lock();
        if let Some((_, token)) = inner.running.take() {
            token.cancel();
        }
        inner.pending = None;
    }

    /// Hands the verified bytes to install exactly once.
    fn take_pending(&self) -> Option<P> {
        self.lock().pending.take()
    }
}

/// Managed state for the in-app updater download/install commands.
#[derive(Default)]
pub struct UpdaterDownloadState {
    slot: DownloadSlot<(Update, Vec<u8>)>,
}

/// Runs `touch` (the only path that reaches the updater plugin) only when the in-app updater
/// owns this install. `app.updater()` panics when the plugin was never registered, which is the
/// case for Store/deb/rpm installs.
fn gate_updater<T>(managed_externally: bool, touch: impl FnOnce() -> T) -> Result<T, IpcError> {
    if managed_externally {
        return Err(IpcError::new(
            IpcErrorCode::Custom(UPDATER_MANAGED_EXTERNALLY.into()),
            "updates for this install are delivered by another channel",
        ));
    }
    Ok(touch())
}

/// The UI asked to download the version it showed the user; a fresh check that offers anything
/// else (or nothing) is refused rather than silently downloading a different build.
fn select_expected_update<T>(
    expected: &str,
    offered: Option<T>,
    version_of: impl Fn(&T) -> &str,
) -> Result<T, IpcError> {
    match offered {
        Some(update) if version_of(&update) == expected => Ok(update),
        Some(update) => Err(IpcError::new(
            IpcErrorCode::Custom(UPDATER_VERSION_MISMATCH.into()),
            format!(
                "update check now offers {}, not {expected}",
                version_of(&update)
            ),
        )
        .with_details(serde_json::json!({ "expected": expected, "offered": version_of(&update) }))),
        None => Err(IpcError::new(
            IpcErrorCode::NotFound,
            format!("no update is available (expected {expected})"),
        )),
    }
}

fn stall_window() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(secs) = std::env::var("FERRYX_UPDATER_STALL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        return Duration::from_secs(secs);
    }
    DEFAULT_STALL_WINDOW
}

fn build_updater<R: Runtime>(app: &AppHandle<R>) -> Result<Updater, IpcError> {
    #[allow(unused_mut)]
    let mut builder = app.updater_builder();
    #[cfg(debug_assertions)]
    if let Ok(endpoint) = std::env::var("FERRYX_UPDATER_ENDPOINT") {
        let url = endpoint.trim().parse().map_err(|error| {
            IpcError::new(
                IpcErrorCode::InvalidArgument,
                format!("FERRYX_UPDATER_ENDPOINT is not a URL: {error}"),
            )
        })?;
        builder = builder.endpoints(vec![url]).map_err(IpcError::internal)?;
    }
    builder.build().map_err(IpcError::internal)
}

/// Checks for the update the UI expects and downloads it under a cancel token and a no-progress
/// watchdog. The outcome arrives on `on_event`; verified bytes stay in managed state for
/// [`cmd_updater_install`].
#[tauri::command]
pub async fn cmd_updater_download<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, UpdaterDownloadState>,
    expected_version: String,
    on_event: Channel<UpdaterDownloadEvent>,
) -> Result<(), IpcError> {
    let updater = gate_updater(updater_managed_externally(), || build_updater(&app))??;
    let offered = updater.check().await.map_err(IpcError::internal)?;
    let update = select_expected_update(&expected_version, offered, |update: &Update| {
        update.version.as_str()
    })?;

    let ticket = state.slot.begin()?;
    let (tx, rx) = mpsc::unbounded_channel();
    let download = {
        let update = update.clone();
        tauri::async_runtime::spawn(async move {
            let chunk_tx = tx.clone();
            let finish_tx = tx.clone();
            let result = update
                .download(
                    move |len, content_length| {
                        let _ = chunk_tx.send(DownloadSignal::Chunk {
                            len,
                            content_length,
                        });
                    },
                    move || {
                        let _ = finish_tx.send(DownloadSignal::BodyComplete);
                    },
                )
                .await
                .map_err(|error| error.to_string());
            let _ = tx.send(DownloadSignal::Done(result));
        })
    };

    let outcome = supervise_download(rx, ticket.token.clone(), stall_window(), |event| {
        let _ = on_event.send(event);
    })
    .await;
    // Dropping the in-flight request is what actually stops the transfer.
    download.abort();

    let event = match outcome {
        DownloadOutcome::Completed(bytes) => {
            if state.slot.finish(&ticket, Some((update, bytes))) {
                UpdaterDownloadEvent::Finished
            } else {
                UpdaterDownloadEvent::Cancelled
            }
        }
        DownloadOutcome::Cancelled => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Cancelled
        }
        DownloadOutcome::Stalled => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Stalled
        }
        DownloadOutcome::Failed(message) => {
            state.slot.finish(&ticket, None);
            UpdaterDownloadEvent::Error { message }
        }
    };
    let _ = on_event.send(event);
    Ok(())
}

/// Cancels the running download and discards any downloaded bytes.
#[tauri::command]
pub async fn cmd_updater_cancel(state: State<'_, UpdaterDownloadState>) -> Result<(), IpcError> {
    gate_updater(updater_managed_externally(), || state.slot.cancel())
}

/// Installs the bytes kept by a finished [`cmd_updater_download`]. Not cancellable. On Windows
/// the plugin launches the installer and exits the process, so this never returns there.
#[tauri::command]
pub async fn cmd_updater_install(state: State<'_, UpdaterDownloadState>) -> Result<(), IpcError> {
    let pending = gate_updater(updater_managed_externally(), || state.slot.take_pending())?;
    let (update, bytes) = pending.ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::Custom(UPDATER_NO_PENDING_INSTALL.into()),
            "no downloaded update is waiting to be installed",
        )
    })?;
    run_blocking(move || update.install(bytes).map_err(IpcError::internal)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distribution_channel_non_windows() {
        if !cfg!(windows) {
            assert_eq!(distribution_channel(), "native");
            // A test binary is never packaged as a distribution package, so it keeps the
            // in-app updater.
            assert!(!updater_managed_externally());
        }
    }

    #[test]
    fn test_managed_externally_for_linux_packages() {
        // dpkg/rpm own these installs: the plugin routes the bundle type to the package
        // manager, which rejects the raw AppImage Ferryx publishes.
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Deb)
        ));
        assert!(managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::Rpm)
        ));
        // The AppImage is replaced in place, and an unpackaged build has no channel but itself.
        assert!(!managed_externally_for(
            UpdateHost::Linux,
            Some(BundleType::AppImage)
        ));
        assert!(!managed_externally_for(UpdateHost::Linux, None));
        // Windows Store ownership is a package-identity fact, not a bundle type; macOS
        // self-updates from the `.app`.
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Msi)
        ));
        assert!(!managed_externally_for(
            UpdateHost::Windows,
            Some(BundleType::Nsis)
        ));
        assert!(!managed_externally_for(
            UpdateHost::MacOs,
            Some(BundleType::App)
        ));
    }

    #[test]
    fn test_distribution_channel_windows() {
        if cfg!(windows) {
            std::env::set_var("FERRYX_STORE_BUILD", "1");
            assert_eq!(distribution_channel(), "store");
            assert!(updater_managed_externally());
            std::env::remove_var("FERRYX_STORE_BUILD");
        }
    }

    const WINDOW: Duration = Duration::from_secs(60);

    fn chunk(len: usize) -> DownloadSignal {
        DownloadSignal::Chunk {
            len,
            content_length: Some(1_000),
        }
    }

    /// Runs the supervisor on its own task so the test can drive the signal stream and the
    /// paused clock; returns the sender, the cancel token, the join handle and the event log.
    fn spawn_supervisor() -> (
        mpsc::UnboundedSender<DownloadSignal>,
        CancellationToken,
        tokio::task::JoinHandle<DownloadOutcome>,
        std::sync::Arc<Mutex<Vec<UpdaterDownloadEvent>>>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        let handle = tokio::spawn(supervise_download(rx, cancel.clone(), WINDOW, move |event| {
            sink.lock().unwrap().push(event)
        }));
        (tx, cancel, handle, events)
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_before_install() {
        let slot: DownloadSlot<Vec<u8>> = DownloadSlot::default();
        let ticket = slot.begin().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let supervisor = tokio::spawn(supervise_download(
            rx,
            ticket.token.clone(),
            WINDOW,
            |_| {},
        ));
        tx.send(chunk(100)).unwrap();
        tokio::task::yield_now().await;
        slot.cancel();
        assert_eq!(supervisor.await.unwrap(), DownloadOutcome::Cancelled);
        // The body completing after the cancel still never reaches install.
        assert!(!slot.finish(&ticket, Some(vec![1, 2, 3])));
        assert_eq!(slot.take_pending(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn stall_fires_after_no_progress_window() {
        let (tx, _cancel, handle, events) = spawn_supervisor();
        tx.send(chunk(10)).unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(WINDOW - Duration::from_millis(1)).await;
        assert!(!handle.is_finished(), "stalled before the full window elapsed");
        tokio::time::advance(Duration::from_millis(2)).await;
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Stalled);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[
                UpdaterDownloadEvent::Started {
                    content_length: Some(1_000)
                },
                UpdaterDownloadEvent::Progress { chunk_length: 10 },
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_progress_never_stalls() {
        let (tx, _cancel, handle, _events) = spawn_supervisor();
        // Ten minutes of one byte every 59 s: far past any total timeout, never idle a full window.
        for _ in 0..10 {
            tokio::time::advance(WINDOW - Duration::from_secs(1)).await;
            tx.send(chunk(1)).unwrap();
            tokio::task::yield_now().await;
            assert!(!handle.is_finished());
        }
        tx.send(DownloadSignal::BodyComplete).unwrap();
        // Signature verification may take longer than the window once the body is in.
        tokio::time::advance(WINDOW * 3).await;
        assert!(!handle.is_finished());
        tx.send(DownloadSignal::Done(Ok(vec![9; 10]))).unwrap();
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Completed(vec![9; 10]));
    }

    #[tokio::test(start_paused = true)]
    async fn completed_download_yields_bytes() {
        let slot: DownloadSlot<Vec<u8>> = DownloadSlot::default();
        let ticket = slot.begin().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let events = std::sync::Arc::new(Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        let supervisor = tokio::spawn(supervise_download(
            rx,
            ticket.token.clone(),
            WINDOW,
            move |event| sink.lock().unwrap().push(event),
        ));
        tx.send(chunk(3)).unwrap();
        tx.send(DownloadSignal::BodyComplete).unwrap();
        tx.send(DownloadSignal::Done(Ok(vec![1, 2, 3]))).unwrap();
        let DownloadOutcome::Completed(bytes) = supervisor.await.unwrap() else {
            panic!("download did not complete");
        };
        assert!(slot.finish(&ticket, Some(bytes)));
        assert_eq!(slot.take_pending(), Some(vec![1, 2, 3]));
        // Install consumes the bytes exactly once.
        assert_eq!(slot.take_pending(), None);
        assert_eq!(events.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn stall_fires_when_headers_never_arrive() {
        let (_tx, _cancel, handle, events) = spawn_supervisor();
        // No signal at all: the request never produced headers or bytes.
        tokio::time::advance(WINDOW + Duration::from_millis(1)).await;
        assert_eq!(handle.await.unwrap(), DownloadOutcome::Stalled);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn version_mismatch_is_refused() {
        let pick = |offered: Option<&'static str>| {
            select_expected_update("2026.1014.1", offered, |version: &&str| version)
        };
        assert_eq!(pick(Some("2026.1014.1")).unwrap(), "2026.1014.1");
        let mismatch = pick(Some("2026.1015.1")).unwrap_err();
        assert_eq!(
            mismatch.code,
            IpcErrorCode::Custom(UPDATER_VERSION_MISMATCH.into())
        );
        assert_eq!(pick(None).unwrap_err().code, IpcErrorCode::NotFound);
    }

    #[test]
    fn managed_externally_returns_typed_error_without_touching_plugin() {
        let touched = std::cell::Cell::new(false);
        let error = gate_updater(true, || touched.set(true)).unwrap_err();
        assert!(!touched.get(), "the plugin path ran on an externally managed install");
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(wire["code"], UPDATER_MANAGED_EXTERNALLY);
        assert!(gate_updater(false, || touched.set(true)).is_ok());
        assert!(touched.get());
    }
}
