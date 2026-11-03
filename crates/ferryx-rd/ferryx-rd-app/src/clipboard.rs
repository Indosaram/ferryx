use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use ferryx_rd_proto::MAX_CLIPBOARD_TEXT_BYTES;
use thiserror::Error;

pub const CLIPBOARD_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
pub const CONCEALED_CLIPBOARD_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardDecision {
    Unchanged,
    SuppressEcho,
    Duplicate,
    Concealed,
    Empty,
    Oversized,
    Send(String),
}

/// Pure clipboard echo-suppression state, separated from platform pasteboard I/O.
#[derive(Debug, Clone)]
pub struct ClipboardSynchronizer {
    last_change_count: u64,
    last_written_change_count: Option<u64>,
    last_text_hash: Option<u64>,
}

impl ClipboardSynchronizer {
    pub fn new(initial_change_count: u64) -> Self {
        Self {
            last_change_count: initial_change_count,
            last_written_change_count: None,
            last_text_hash: None,
        }
    }

    /// Records a remote write and returns the modeled resulting change count.
    pub fn apply_remote(&mut self, text: &str) -> u64 {
        self.last_change_count = self.last_change_count.wrapping_add(1);
        self.last_written_change_count = Some(self.last_change_count);
        self.last_text_hash = Some(text_hash(text));
        self.last_change_count
    }

    pub fn record_remote_write(&mut self, text: &str, resulting_change_count: u64) {
        self.last_change_count = resulting_change_count;
        self.last_written_change_count = Some(resulting_change_count);
        self.last_text_hash = Some(text_hash(text));
    }

    pub fn poll(&mut self, change_count: u64, text: &str, types: &[&str]) -> ClipboardDecision {
        if self.last_written_change_count == Some(change_count) {
            self.last_written_change_count = None;
            self.last_change_count = change_count;
            return ClipboardDecision::SuppressEcho;
        }
        if change_count == self.last_change_count {
            return ClipboardDecision::Unchanged;
        }
        self.last_change_count = change_count;
        if types
            .iter()
            .any(|ty| CONCEALED_CLIPBOARD_TYPES.contains(ty))
        {
            return ClipboardDecision::Concealed;
        }
        if text.is_empty() {
            return ClipboardDecision::Empty;
        }
        let hash = text_hash(text);
        if self.last_text_hash == Some(hash) {
            return ClipboardDecision::Duplicate;
        }
        if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
            return ClipboardDecision::Oversized;
        }
        self.last_text_hash = Some(hash);
        ClipboardDecision::Send(text.to_owned())
    }
}

fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardSnapshot {
    pub change_count: u64,
    pub text: Option<String>,
    pub types: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("clipboard unavailable: {0}")]
    Unavailable(String),
}

pub trait PlatformClipboard: Send + Sync + 'static {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError>;
    fn set_text(&self, text: &str) -> Result<u64, ClipboardError>;
}

/// Polls a platform clipboard at the protocol's 500 ms cadence.
pub struct ClipboardMonitor<C: PlatformClipboard> {
    clipboard: Arc<C>,
    synchronizer: Arc<Mutex<ClipboardSynchronizer>>,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl<C: PlatformClipboard> ClipboardMonitor<C> {
    pub fn new(clipboard: C) -> Result<Self, ClipboardError> {
        let clipboard = Arc::new(clipboard);
        let initial_count = clipboard.snapshot()?.change_count;
        Ok(Self {
            clipboard,
            synchronizer: Arc::new(Mutex::new(ClipboardSynchronizer::new(initial_count))),
            stop: None,
            worker: None,
        })
    }

    pub fn apply_remote(&self, text: &str) -> Result<(), ClipboardError> {
        // Hold the synchronizer lock across the write and the record so the
        // polling worker cannot observe the remote text before it is recorded,
        // nor classify a pre-write snapshot against the post-write state.
        let mut state = self
            .synchronizer
            .lock()
            .map_err(|_| ClipboardError::Unavailable("clipboard state lock poisoned".to_owned()))?;
        let change_count = self.clipboard.set_text(text)?;
        state.record_remote_write(text, change_count);
        Ok(())
    }

    pub fn poll_once(&self) -> Result<ClipboardDecision, ClipboardError> {
        let mut state = self
            .synchronizer
            .lock()
            .map_err(|_| ClipboardError::Unavailable("clipboard state lock poisoned".to_owned()))?;
        let snapshot = self.clipboard.snapshot()?;
        let type_refs = snapshot
            .types
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        Ok(state.poll(
            snapshot.change_count,
            snapshot.text.as_deref().unwrap_or_default(),
            &type_refs,
        ))
    }

    pub fn start(
        &mut self,
        on_local_change: impl Fn(String) + Send + 'static,
    ) -> Result<(), ClipboardError> {
        if self.worker.is_some() {
            return Ok(());
        }
        let (stop_tx, stop_rx) = mpsc::channel();
        let clipboard = Arc::clone(&self.clipboard);
        let synchronizer = Arc::clone(&self.synchronizer);
        self.worker = Some(thread::spawn(move || loop {
            match stop_rx.recv_timeout(CLIPBOARD_POLL_INTERVAL) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            // Snapshot and classify under one lock acquisition, then release it
            // before the callback so apply_remote cannot interleave between them.
            let decision = synchronizer.lock().ok().and_then(|mut state| {
                let snapshot = clipboard.snapshot().ok()?;
                let type_refs = snapshot
                    .types
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                Some(state.poll(
                    snapshot.change_count,
                    snapshot.text.as_deref().unwrap_or_default(),
                    &type_refs,
                ))
            });
            if let Some(ClipboardDecision::Send(text)) = decision {
                on_local_change(text);
            }
        }));
        self.stop = Some(stop_tx);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl<C: PlatformClipboard> Drop for ClipboardMonitor<C> {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Default, Clone)]
pub struct MemoryClipboard {
    state: Arc<Mutex<ClipboardSnapshot>>,
}

impl PlatformClipboard for MemoryClipboard {
    fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
        Ok(self
            .state
            .lock()
            .expect("memory clipboard poisoned")
            .clone())
    }

    fn set_text(&self, text: &str) -> Result<u64, ClipboardError> {
        let mut state = self.state.lock().expect("memory clipboard poisoned");
        state.change_count = state.change_count.wrapping_add(1);
        state.text = Some(text.to_owned());
        state.types = vec!["public.utf8-plain-text".to_owned()];
        Ok(state.change_count)
    }
}

#[cfg(target_os = "macos")]
pub mod platform {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;

    use super::{ClipboardError, ClipboardSnapshot, PlatformClipboard};

    #[derive(Debug, Default, Clone, Copy)]
    pub struct SystemClipboard;

    impl PlatformClipboard for SystemClipboard {
        fn snapshot(&self) -> Result<ClipboardSnapshot, ClipboardError> {
            let pasteboard = NSPasteboard::generalPasteboard();
            let change_count = pasteboard.changeCount() as u64;
            let types = pasteboard
                .types()
                .map(|types| {
                    types
                        .to_vec()
                        .into_iter()
                        .map(|ty| ty.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let text = unsafe { pasteboard.stringForType(NSPasteboardTypeString) }
                .map(|text| text.to_string());
            Ok(ClipboardSnapshot {
                change_count,
                text,
                types,
            })
        }

        fn set_text(&self, text: &str) -> Result<u64, ClipboardError> {
            let pasteboard = NSPasteboard::generalPasteboard();
            let text = NSString::from_str(text);
            unsafe {
                pasteboard.clearContents();
                if !pasteboard.setString_forType(&text, NSPasteboardTypeString) {
                    return Err(ClipboardError::Unavailable(
                        "AppKit rejected the string write".to_owned(),
                    ));
                }
                Ok(pasteboard.changeCount() as u64)
            }
        }
    }
}
