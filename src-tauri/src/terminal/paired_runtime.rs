//! In-memory native relay ownership. No PTY, shell, or SSH launcher exists here.
use super::paired_daemon::Proxy;
use crate::scoped_contracts::Epoch;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

fn default_descriptors_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("FERRYX_PAIRED_DESCRIPTORS_DIR") {
        return Some(PathBuf::from(dir).join("paired_descriptors.json"));
    }
    #[cfg(not(test))]
    {
        crate::remote::auth::canonical_remote_dir().map(|dir| dir.join("paired_descriptors.json"))
    }
    #[cfg(test)]
    {
        None
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FsFault {
    None,
    FailTempWrite,
    FailRename,
}

#[cfg(test)]
thread_local! {
    static TEST_FS_FAULT: std::cell::Cell<FsFault> = const { std::cell::Cell::new(FsFault::None) };
}

#[cfg(test)]
impl FsFault {
    pub(crate) fn set(fault: Self) {
        TEST_FS_FAULT.with(|f| f.set(fault));
    }
    pub(crate) fn get() -> Self {
        TEST_FS_FAULT.with(|f| f.get())
    }
}

fn fs_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailTempWrite {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "injected temp write failure",
        ));
    }
    std::fs::write(path, bytes)
}

fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailRename {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "injected rename failure",
        ));
    }
    std::fs::rename(from, to)
}

fn load_descriptors(
    path: &Path,
) -> Result<HashMap<String, super::paired_daemon::Descriptor>, String> {
    // N2 (round 2): fail closed on load. A read or parse failure must NOT be
    // silently converted into an empty map — that would let a later save
    // clobber a corrupt-but-recoverable store with only the live memory.
    match std::fs::read(path) {
        Ok(data) => {
            serde_json::from_slice(&data).map_err(|e| format!("PAIRED_DESCRIPTOR_PARSE: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(format!("PAIRED_DESCRIPTOR_READ: {e}")),
    }
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PERSIST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PERSIST_MIN_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(test)]
static TEST_PERSIST_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static TEST_PERSIST_MAP: parking_lot::Mutex<Option<HashMap<PathBuf, usize>>> =
    parking_lot::Mutex::new(None);

#[cfg(test)]
pub(crate) fn test_persist_count() -> usize {
    TEST_PERSIST_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn test_persist_count_path(path: &Path) -> usize {
    TEST_PERSIST_MAP
        .lock()
        .as_ref()
        .and_then(|m| m.get(path).copied())
        .unwrap_or(0)
}

fn save_descriptors_locked(
    path: &Path,
    descriptors: &HashMap<String, super::paired_daemon::Descriptor>,
) -> Result<(), String> {
    #[cfg(test)]
    {
        TEST_PERSIST_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut map = TEST_PERSIST_MAP.lock();
        let m = map.get_or_insert_with(HashMap::new);
        *m.entry(path.to_path_buf()).or_insert(0) += 1;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = match serde_json::to_vec_pretty(descriptors) {
        Ok(bytes) => bytes,
        Err(e) => return Err(format!("PAIRED_DESCRIPTOR_SERIALIZE: {e}")),
    };
    // N2 & R3-N1: unique temp file per write and global mutex serialize disk replacement
    let tmp_seq = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), tmp_seq));
    fs_write(&tmp, &bytes).map_err(|e| format!("PAIRED_DESCRIPTOR_TEMP_WRITE: {e}"))?;
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    if let Err(e) = fs_rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PAIRED_DESCRIPTOR_REPLACE: {e}"));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(windows)]
    {
        let _ = std::fs::File::open(path).and_then(|f| f.sync_all());
    }
    Ok(())
}

#[allow(dead_code)]
fn save_descriptors(
    path: &Path,
    descriptors: &HashMap<String, super::paired_daemon::Descriptor>,
) -> Result<(), String> {
    let _persist_guard = PERSIST_MUTEX.lock().map_err(|e| e.to_string())?;
    save_descriptors_locked(path, descriptors)
}

type Reply = oneshot::Sender<Result<(), String>>;
enum Command {
    Write(u64, Vec<u8>, Reply),
    Resize(u64, u16, u16, Reply),
    Interrupt(u64, Reply),
    Detach(Reply),
    #[cfg(test)]
    Block(oneshot::Sender<()>, oneshot::Receiver<()>),
}
struct Owner {
    sender: mpsc::Sender<Command>,
    task: tokio::task::JoinHandle<()>,
    identity: Arc<()>,
    #[cfg(test)]
    completed: tokio::sync::watch::Receiver<bool>,
}

struct ActorPersistGuard {
    descriptors: Arc<Mutex<HashMap<String, super::paired_daemon::Descriptor>>>,
    store_path: Arc<Mutex<Option<PathBuf>>>,
    dirty: Arc<AtomicBool>,
}

impl Drop for ActorPersistGuard {
    fn drop(&mut self) {
        if !self.dirty.load(Ordering::Acquire) {
            return;
        }
        let path = self.store_path.lock().clone();
        let Some(ref path) = path else {
            return;
        };
        // R3-N1: acquire PERSIST_MUTEX before taking the snapshot
        // so disk writes strictly serialize with state updates.
        let _persist_guard = match PERSIST_MUTEX.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !self.dirty.load(Ordering::Acquire) {
            return;
        }
        let snapshot = {
            let descs = self.descriptors.lock();
            descs.clone()
        };
        if let Err(e) = save_descriptors_locked(path, &snapshot) {
            eprintln!("[paired_runtime] descriptor save failed in actor drop guard: {e}");
        } else {
            self.dirty.store(false, Ordering::Release);
        }
    }
}

/// Weak ownership avoids keeping the runtime alive. Identity fencing prevents an
/// old actor (for example, after detach) from removing a replacement for its ID.
struct ReapOwner {
    owners: std::sync::Weak<Mutex<HashMap<String, Owner>>>,
    id: String,
    identity: Arc<()>,
    #[cfg(test)]
    completed: tokio::sync::watch::Sender<bool>,
}
impl Drop for ReapOwner {
    fn drop(&mut self) {
        if let Some(owners) = self.owners.upgrade() {
            // The cancel path can run synchronously while the aborting thread
            // already holds this mutex: install holds it across tokio::spawn of
            // the actor, and a runtime shutdown (or an Owner replaced under the
            // lock) cancels the task from that same thread. A blocking re-lock
            // would self-deadlock the non-reentrant mutex, so degrade to a
            // try-lock; under contention the aborting context owns the map
            // transition, and a skipped removal is inert because every live-task
            // check filters finished entries.
            if let Some(mut owners) = owners.try_lock() {
                if owners
                    .get(&self.id)
                    .is_some_and(|owner| Arc::ptr_eq(&owner.identity, &self.identity))
                {
                    owners.remove(&self.id);
                }
            }
        }
        #[cfg(test)]
        let _ = self.completed.send(true);
    }
}

pub struct Runtime {
    owners: Arc<Mutex<HashMap<String, Owner>>>,
    descriptors: Arc<Mutex<HashMap<String, super::paired_daemon::Descriptor>>>,
    store_path: Arc<Mutex<Option<PathBuf>>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Runtime {
    pub fn new(store_path: Option<PathBuf>) -> Self {
        let store_path = store_path.or_else(default_descriptors_path);
        let _persist_guard = PERSIST_MUTEX.lock();
        let (descriptors, effective_store_path) = if let Some(ref path) = store_path {
            match load_descriptors(path) {
                Ok(map) => (map, store_path),
                Err(e) => {
                    eprintln!("[paired_runtime] descriptor store load failed; disabling persistence to protect file: {e}");
                    (HashMap::new(), None)
                }
            }
        } else {
            (HashMap::new(), None)
        };
        drop(_persist_guard);
        Self {
            owners: Arc::new(Mutex::new(HashMap::new())),
            descriptors: Arc::new(Mutex::new(descriptors)),
            store_path: Arc::new(Mutex::new(effective_store_path)),
        }
    }

    pub fn set_store_path(&self, path: PathBuf) {
        // N1: single lock order — the store-path snapshot is taken BEFORE the
        // descriptors lock is ever acquired; no site may acquire store_path
        // while holding descriptors.
        match load_descriptors(&path) {
            Err(e) => {
                // N2 (round 2): a failed load must not let the subsequent save
                // clobber a corrupt-but-recoverable store with live memory.
                eprintln!(
                    "[paired_runtime] descriptor store load failed; leaving file untouched: {e}"
                );
                return;
            }
            Ok(loaded) => {
                let _persist_guard = match PERSIST_MUTEX.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                *self.store_path.lock() = Some(path.clone());
                let current = {
                    let mut desc_guard = self.descriptors.lock();
                    for (k, v) in loaded {
                        desc_guard.entry(k).or_insert(v);
                    }
                    desc_guard.clone()
                };
                if let Err(e) = save_descriptors_locked(&path, &current) {
                    eprintln!("[paired_runtime] descriptor save failed after set_store_path: {e}");
                }
            }
        }
    }

    pub fn owns(id: &str) -> bool {
        id.starts_with("daemon-session:")
    }
    pub fn contains(&self, id: &str) -> bool {
        self.owners
            .lock()
            .get(id)
            .is_some_and(|o| !o.task.is_finished())
    }
    pub fn list(&self) -> Vec<String> {
        self.owners
            .lock()
            .iter()
            .filter(|(_, o)| !o.task.is_finished())
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub fn descriptor(&self, id: &str) -> Option<super::paired_daemon::Descriptor> {
        self.descriptors.lock().get(id).cloned()
    }
    pub fn remove_descriptor(&self, id: &str) {
        let path = self.store_path.lock().clone();
        let _persist_guard = match PERSIST_MUTEX.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let snapshot = {
            let mut descs = self.descriptors.lock();
            let existed = descs.remove(id).is_some();
            if !existed {
                return;
            }
            descs.clone()
        };
        if let Some(ref path) = path {
            if let Err(e) = save_descriptors_locked(path, &snapshot) {
                eprintln!("[paired_runtime] descriptor save failed after remove: {e}");
            }
        }
    }
    pub fn install(&self, proxy: Proxy) -> Result<String, String> {
        let id = proxy.id().to_owned();
        let descriptor = proxy.descriptor().clone();
        let mut owners = self.owners.lock();
        if owners.get(&id).is_some_and(|o| !o.task.is_finished()) {
            return Err("CONTROL_CONFLICT".into());
        }
        owners.remove(&id);

        // R3-N1: hold PERSIST_MUTEX while mutating descriptors and writing disk
        // to prevent stale snapshots from overwriting newer updates.
        let path = self.store_path.lock().clone();
        let _persist_guard = PERSIST_MUTEX.lock().map_err(|e| e.to_string())?;
        let (previous, snapshot) = {
            let mut descs = self.descriptors.lock();
            let previous = descs.insert(id.clone(), descriptor.clone());
            (previous, descs.clone())
        };
        if let Some(ref path) = path {
            if let Err(e) = save_descriptors_locked(path, &snapshot) {
                // R2-N1: restore the PRIOR descriptor on rollback — deleting
                // the entry would discard a retained descriptor from a
                // previous install (exactly the reattach recovery case).
                let mut descs = self.descriptors.lock();
                match previous {
                    Some(old) => {
                        descs.insert(id.clone(), old);
                    }
                    None => {
                        descs.remove(&id);
                    }
                }
                return Err(e);
            }
        }
        drop(_persist_guard);

        let (sender, receiver) = mpsc::channel(32);
        let identity = Arc::new(());
        #[cfg(test)]
        let (completed_tx, completed) = tokio::sync::watch::channel(false);
        let reap = ReapOwner {
            owners: Arc::downgrade(&self.owners),
            id: id.clone(),
            identity: identity.clone(),
            #[cfg(test)]
            completed: completed_tx,
        };
        let descriptors = self.descriptors.clone();
        let store_path = self.store_path.clone();
        let task_id = id.clone();
        let task = tokio::spawn(async move {
            let _reap = reap;
            let dirty = Arc::new(AtomicBool::new(false));
            let _persist_drop_guard = ActorPersistGuard {
                descriptors: descriptors.clone(),
                store_path: store_path.clone(),
                dirty: dirty.clone(),
            };
            // Drop the proxy and receiver before publishing completion/removing
            // the map entry. A cloned sender then fails closed on this channel.
            let mut proxy = proxy;
            let mut receiver = receiver;
            let period = Duration::from_secs(5);
            let mut keepalive =
                tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // Seed one interval in the past so the first change persists immediately:
            // a change arriving inside the first debounce window could otherwise be
            // lost when the actor is dropped before the window elapses.
            let mut last_persisted = tokio::time::Instant::now()
                .checked_sub(PERSIST_MIN_INTERVAL)
                .unwrap_or_else(tokio::time::Instant::now);
            loop {
                tokio::select! {
                    biased;
                    command = receiver.recv() => match command {
                        Some(Command::Write(g, data, reply)) => {
                            let gen = if g == 0 { proxy.controller().unwrap_or(Epoch(0)) } else { Epoch(g) };
                            let _ = reply.send(proxy.write(gen, &data).await.map_err(|e| e.code));
                        }
                        Some(Command::Resize(g, cols, rows, reply)) => {
                            let gen = if g == 0 { proxy.controller().unwrap_or(Epoch(0)) } else { Epoch(g) };
                            let _ = reply.send(proxy.resize(gen, cols, rows).await.map_err(|e| e.code));
                        }
                        Some(Command::Interrupt(g, reply)) => { let _ = reply.send(proxy.interrupt(Epoch(g)).await.map_err(|e| e.code)); }
                        Some(Command::Detach(reply)) => {
                            let result = proxy.detach().await.map_err(|e| e.code);
                            // R3-N1: acquire PERSIST_MUTEX before taking the snapshot
                            // so disk writes strictly serialize with state updates.
                            let path = store_path.lock().clone();
                            let _persist_guard = match PERSIST_MUTEX.lock() {
                                Ok(g) => g,
                                Err(_) => {
                                    let _ = reply.send(result);
                                    return;
                                }
                            };
                            let snapshot = {
                                let mut descs = descriptors.lock();
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                descs.clone()
                            };
                            drop(proxy);
                            if let Some(ref path) = path {
                                if let Err(e) = save_descriptors_locked(path, &snapshot) {
                                    eprintln!("[paired_runtime] descriptor save failed after detach: {e}");
                                } else {
                                    dirty.store(false, Ordering::Release);
                                }
                            } else {
                                dirty.store(false, Ordering::Release);
                            }
                            drop(_persist_guard);
                            let _ = reply.send(result);
                            return;
                        }
                        #[cfg(test)]
                        Some(Command::Block(started, rx)) => {
                            let _ = started.send(());
                            let _ = rx.await;
                        }
                        None => {
                            let path = store_path.lock().clone();
                            let _persist_guard = match PERSIST_MUTEX.lock() {
                                Ok(g) => g,
                                Err(_) => return,
                            };
                            let snapshot = {
                                let mut descs = descriptors.lock();
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                descs.clone()
                            };
                            drop(proxy);
                            if let Some(ref path) = path {
                                if let Err(e) = save_descriptors_locked(path, &snapshot) {
                                    eprintln!("[paired_runtime] descriptor save failed after channel closed: {e}");
                                } else {
                                    dirty.store(false, Ordering::Release);
                                }
                            } else {
                                dirty.store(false, Ordering::Release);
                            }
                            drop(_persist_guard);
                            return;
                        }
                    },
                    _ = tokio::time::sleep_until(last_persisted + PERSIST_MIN_INTERVAL), if dirty.load(Ordering::Acquire) => {
                        let path = store_path.lock().clone();
                        let _persist_guard = match PERSIST_MUTEX.lock() {
                            Ok(g) => g,
                            Err(_) => continue,
                        };
                        let snapshot = {
                            let descs = descriptors.lock();
                            descs.clone()
                        };
                        let mut saved = false;
                        if let Some(ref path) = path {
                            match save_descriptors_locked(path, &snapshot) {
                                Ok(()) => saved = true,
                                Err(e) => {
                                    eprintln!("[paired_runtime] descriptor save failed after debounce: {e}");
                                }
                            }
                        } else {
                            saved = true;
                        }
                        drop(_persist_guard);
                        last_persisted = tokio::time::Instant::now();
                        if saved {
                            dirty.store(false, Ordering::Release);
                        }
                    }
                    _ = keepalive.tick(), if proxy.controller().is_some() => {
                        if let Some(g) = proxy.controller() {
                            if proxy.ping(g).await.is_err() {
                                let path = store_path.lock().clone();
                                let _persist_guard = match PERSIST_MUTEX.lock() {
                                    Ok(g) => g,
                                    Err(_) => return,
                                };
                                let snapshot = {
                                    let mut descs = descriptors.lock();
                                    descs.insert(task_id.clone(), proxy.descriptor().clone());
                                    descs.clone()
                                };
                                drop(proxy);
                                if let Some(ref path) = path {
                                    if save_descriptors_locked(path, &snapshot).is_ok() {
                                        dirty.store(false, Ordering::Release);
                                    }
                                } else {
                                    dirty.store(false, Ordering::Release);
                                }
                                drop(_persist_guard);
                                return;
                            }
                        }
                    }
                    result = proxy.receive(), if proxy.controller().is_some() => {
                        if result.is_err() || proxy.controller().is_none() {
                            let path = store_path.lock().clone();
                            let _persist_guard = match PERSIST_MUTEX.lock() {
                                Ok(g) => g,
                                Err(_) => return,
                            };
                            let snapshot = {
                                let mut descs = descriptors.lock();
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                descs.clone()
                            };
                            drop(proxy);
                            if let Some(ref path) = path {
                                if save_descriptors_locked(path, &snapshot).is_ok() {
                                    dirty.store(false, Ordering::Release);
                                }
                            } else {
                                dirty.store(false, Ordering::Release);
                            }
                            drop(_persist_guard);
                            return;
                        }
                        let seq = proxy.descriptor().after_sequence;
                        let changed = {
                            let mut descs = descriptors.lock();
                            if let Some(d) = descs.get_mut(&task_id) {
                                if d.after_sequence != seq {
                                    d.after_sequence = seq;
                                    true
                                } else {
                                    false
                                }
                            } else {
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                true
                            }
                        };
                        if changed {
                            dirty.store(true, Ordering::Release);
                            let now = tokio::time::Instant::now();
                            if now.duration_since(last_persisted) >= PERSIST_MIN_INTERVAL {
                                // R3-N1: acquire PERSIST_MUTEX before taking the snapshot
                                // so disk writes strictly serialize with state updates.
                                let path = store_path.lock().clone();
                                let _persist_guard = match PERSIST_MUTEX.lock() {
                                    Ok(g) => g,
                                    Err(_) => continue,
                                };
                                let snapshot = {
                                    let descs = descriptors.lock();
                                    descs.clone()
                                };
                                let mut saved = false;
                                if let Some(ref path) = path {
                                    match save_descriptors_locked(path, &snapshot) {
                                        Ok(()) => saved = true,
                                        Err(e) => {
                                            eprintln!("[paired_runtime] descriptor save failed after receive: {e}");
                                        }
                                    }
                                } else {
                                    saved = true;
                                }
                                drop(_persist_guard);
                                last_persisted = now;
                                if saved {
                                    dirty.store(false, Ordering::Release);
                                }
                            }
                        }
                    }
                }
            }
        });
        owners.insert(
            id.clone(),
            Owner {
                sender,
                task,
                identity,
                #[cfg(test)]
                completed,
            },
        );
        Ok(id)
    }
    #[cfg(test)]
    pub(crate) fn force_reap_owner(&self, id: &str) {
        // Simulates a dead actor whose descriptor is retained — the reattach
        // recovery scenario the reinstall path must never regress.
        self.owners.lock().remove(id);
    }
    #[cfg(test)]
    pub(crate) fn completion_probe(
        &self,
        id: &str,
        pending: super::remote::RemoteOperation,
    ) -> impl std::future::Future<Output = ()> + '_ {
        let (weak, mut completed) = {
            let owners = self.owners.lock();
            let owner = owners.get(id).unwrap();
            (owner.sender.downgrade(), owner.completed.clone())
        };
        let id = id.to_owned();
        async move {
            completed.wait_for(|done| *done).await.unwrap();
            assert!(
                !self.owners.lock().contains_key(&id),
                "completed owner retained in map"
            );
            let failure = pending.await.unwrap_err();
            assert_eq!(failure.kind, super::remote::RemoteFailureKind::Disconnected);
            assert_eq!(failure.message, "PAIRED_PROXY_UNAVAILABLE");
            assert_eq!(weak.strong_count(), 0, "owner sender retained");
            assert!(weak.upgrade().is_none());
        }
    }
    #[cfg(test)]
    pub(crate) fn block_actor(
        &self,
        id: &str,
        started: oneshot::Sender<()>,
        rx: oneshot::Receiver<()>,
    ) {
        if let Ok(sender) = self.sender(id) {
            let _ = sender.try_send(Command::Block(started, rx));
        }
    }
    #[cfg(test)]
    pub(crate) async fn abort_actor(&self, id: &str) {
        let mut completed = {
            let owners = self.owners.lock();
            owners.get(id).map(|o| {
                o.task.abort();
                o.completed.clone()
            })
        };
        if let Some(ref mut completed) = completed {
            let _ = completed.wait_for(|done| *done).await;
        }
    }
    pub(crate) fn sender(&self, id: &str) -> Result<mpsc::Sender<Command>, String> {
        self.owners
            .lock()
            .get(id)
            .map(|o| o.sender.clone())
            .ok_or_else(|| "PAIRED_PROXY_MISSING".into())
    }
    pub fn write(
        self: &Arc<Self>,
        id: &str,
        generation: u64,
        data: Vec<u8>,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Write(generation, data, reply))
    }
    pub fn resize(
        self: &Arc<Self>,
        id: &str,
        generation: u64,
        cols: u16,
        rows: u16,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| {
            Command::Resize(generation, cols, rows, reply)
        })
    }
    pub fn interrupt(
        &self,
        id: &str,
        generation: u64,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Interrupt(generation, reply))
    }
    fn operation(
        &self,
        id: &str,
        command: impl FnOnce(Reply) -> Command + Send + 'static,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        let sender = self.sender(id).map_err(super::PtyError::Other)?;
        Ok(Box::pin(async move {
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                let (tx, rx) = oneshot::channel();
                sender
                    .send(command(tx))
                    .await
                    .map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?;
                rx.await
                    .map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?
            })
            .await
            .unwrap_or_else(|_| Err("TIMEOUT".into()));
            result.map_err(|message| super::remote::RemoteFailure {
                kind: super::remote::RemoteFailureKind::Disconnected,
                message,
            })
        }))
    }
    /// Detach only; deliberately not remote session close.
    pub async fn detach(&self, id: &str) -> Result<(), String> {
        // Detach is idempotent: a proxy that is already gone (never registered in
        // this daemon incarnation, reaped while idle, or detached earlier) already
        // satisfies the caller's desired end state. Erroring here stranded callers
        // that only want the proxy gone, e.g. closing a pane restored from a
        // previous daemon generation.
        let Some(mut owner) = self.owners.lock().remove(id) else {
            return Ok(());
        };
        if owner.task.is_finished() {
            return Ok(());
        }
        let (tx, rx) = oneshot::channel();
        owner
            .sender
            .send(Command::Detach(tx))
            .await
            .map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        let result = tokio::time::timeout(Duration::from_secs(30), rx)
            .await
            .map_err(|_| "TIMEOUT")?
            .map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        // The acknowledgement is emitted only after socket and hub destruction.
        (&mut owner.task)
            .await
            .map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        result
    }

    #[cfg(test)]
    pub fn simulate_cursor_persistence(&self, task_id: &str, seq: Option<Epoch>) {
        let changed = {
            let mut descs = self.descriptors.lock();
            if let Some(d) = descs.get_mut(task_id) {
                if d.after_sequence != seq {
                    d.after_sequence = seq;
                    true
                } else {
                    false
                }
            } else {
                descs.insert(
                    task_id.to_string(),
                    super::paired_daemon::Descriptor {
                        host_id: "https://relay.example.com".into(),
                        generation: Epoch(1),
                        target: crate::remote::machine_protocol::RemoteTerminalTarget {
                            machine_id: "test-machine".into(),
                            daemon_epoch: Epoch(1),
                            session_id: task_id.to_string(),
                        },
                        after_sequence: seq,
                    },
                );
                true
            }
        };
        if changed {
            let path = self.store_path.lock().clone();
            let _persist_guard = PERSIST_MUTEX.lock();
            let snapshot = {
                let descs = self.descriptors.lock();
                descs.clone()
            };
            if let (Some(ref path), Ok(_)) = (path, _persist_guard) {
                let _ = save_descriptors_locked(path, &snapshot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::{client::MachineClient, service::PairedHostService};
    use crate::remote::machine_protocol::RemoteTerminalTarget;
    use crate::remote::terminal_wire::{encode_frame, Metadata};
    use crate::terminal::output_hub::TerminalOutputHub;
    use crate::terminal::paired_daemon::Descriptor;
    use axum::{
        extract::ws::{Message, WebSocketUpgrade},
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;

    static FAULT_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn test_p13_descriptor_survives_actor_termination() {
        let runtime = Runtime::default();
        let hub = Arc::new(TerminalOutputHub::new(32));
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "test-session".into(),
            },
            after_sequence: Some(Epoch(42)),
        };
        let proxy = Proxy::new(descriptor.clone(), hub.clone()).unwrap();
        let id = runtime.install(proxy).unwrap();

        // While running, descriptor is accessible
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);

        // Terminate the actor task (simulating transient network error / receive error / actor death)
        {
            let owners = runtime.owners.lock();
            let owner = owners.get(&id).unwrap();
            owner.task.abort();
        }
        // Yield to allow task drop / reap
        tokio::time::sleep(Duration::from_millis(50)).await;

        // P13 Assertion: The credential-free descriptor must survive actor death!
        let recovered = runtime.descriptor(&id);
        assert!(
            recovered.is_some(),
            "Descriptor was lost on actor death; expected durable retention"
        );
        let recovered = recovered.unwrap();
        assert_eq!(recovered.host_id, descriptor.host_id);
        assert_eq!(recovered.generation, descriptor.generation);
        assert_eq!(recovered.target, descriptor.target);
        assert_eq!(recovered.after_sequence, Some(Epoch(42)));
    }

    #[tokio::test]
    async fn test_p13_descriptor_survives_daemon_restart() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");

        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(2),
            target: RemoteTerminalTarget {
                machine_id: "restart-machine".into(),
                daemon_epoch: Epoch(20),
                session_id: "restart-session".into(),
            },
            after_sequence: Some(Epoch(100)),
        };

        let id = {
            let runtime = Runtime::new(Some(store_path.clone()));
            let hub = Arc::new(TerminalOutputHub::new(32));
            let proxy = Proxy::new(descriptor.clone(), hub).unwrap();
            runtime.install(proxy).unwrap()
        };

        // Simulate daemon restart: create a fresh Runtime loading from same store_path
        let restarted_runtime = Runtime::new(Some(store_path));
        let recovered = restarted_runtime.descriptor(&id);
        assert!(
            recovered.is_some(),
            "Descriptor was lost across daemon restart"
        );
        let recovered = recovered.unwrap();
        assert_eq!(recovered.host_id, descriptor.host_id);
        assert_eq!(recovered.generation, descriptor.generation);
        assert_eq!(recovered.target, descriptor.target);
        assert_eq!(recovered.after_sequence, Some(Epoch(100)));
    }

    #[tokio::test]
    async fn test_n2_save_failure_temp_write_propagates_error() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1.clone(), hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        assert!(store_path.exists());

        FsFault::set(FsFault::FailTempWrite);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s2".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let res = runtime.install(p2);
        FsFault::set(FsFault::None);

        assert!(res.is_err(), "install must fail when temp write fails");
        assert!(store_path.exists(), "existing store file must survive");
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert!(
            reloaded.descriptor(&id1).is_some(),
            "original descriptor must still be present"
        );
    }

    #[tokio::test]
    async fn test_r2n1_save_failure_restores_previous_descriptor() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1, hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        // Reattach scenario: the prior actor is gone but its descriptor is
        // retained in the map (and on disk).
        runtime.force_reap_owner(&id1);
        assert!(runtime.descriptor(&id1).is_some());

        FsFault::set(FsFault::FailTempWrite);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(2),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(2),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let res = runtime.install(p2);
        FsFault::set(FsFault::None);

        assert!(res.is_err(), "reinstall must fail when persistence fails");
        // R2-N1: the retained PRIOR descriptor must be restored, not deleted.
        let kept = runtime
            .descriptor(&id1)
            .expect("previous descriptor must survive a failed reinstall");
        assert_eq!(kept.generation, Epoch(1));
        assert_eq!(kept.target.daemon_epoch, Epoch(1));
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert_eq!(
            reloaded
                .descriptor(&id1)
                .expect("disk still holds prior")
                .generation,
            Epoch(1)
        );
    }

    #[tokio::test]
    async fn test_r3n2_corrupt_store_initial_load_disables_persistence_to_prevent_clobber() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        std::fs::write(&store_path, b"{corrupt-json").unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        assert!(runtime.store_path.lock().is_none());

        let hub = Arc::new(TerminalOutputHub::new(32));
        let desc = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let p = Proxy::new(desc, hub).unwrap();
        let _ = runtime.install(p);

        let disk = std::fs::read(&store_path).unwrap();
        assert_eq!(disk, b"{corrupt-json");
    }

    #[tokio::test]
    async fn test_r2n2_corrupt_store_not_clobbered_by_set_store_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        std::fs::write(&store_path, b"{not-json").unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        assert!(
            runtime.descriptor("daemon-session:s1").is_none(),
            "corrupt store loads empty"
        );
        runtime.set_store_path(store_path.clone());
        // The failed load must not let the follow-up save overwrite the
        // corrupt-but-recoverable file with live memory.
        let after = std::fs::read(&store_path).unwrap();
        assert_eq!(after, b"{not-json", "corrupt store must be left untouched");
    }

    #[tokio::test]
    async fn detach_of_unknown_proxy_is_idempotent_success() {
        // Closing a pane restored from a previous daemon generation hands the
        // runtime a proxy id it never registered. Detach must satisfy the caller
        // (the proxy is already gone) instead of stranding the close with
        // PAIRED_PROXY_MISSING.
        let runtime = Runtime::new(None);
        runtime
            .detach("daemon-session:never-registered")
            .await
            .expect("first detach of an unknown proxy must succeed");
        runtime
            .detach("daemon-session:never-registered")
            .await
            .expect("second detach must also succeed");
    }

    #[tokio::test]
    async fn test_n2_save_failure_rename_preserves_existing_file() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1.clone(), hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        assert!(store_path.exists());

        FsFault::set(FsFault::FailRename);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s2".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let res = runtime.install(p2);
        FsFault::set(FsFault::None);

        assert!(res.is_err(), "install must fail when rename fails");
        assert!(
            store_path.exists(),
            "existing store file must survive rename failure (destructive save defect)"
        );
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert!(
            reloaded.descriptor(&id1).is_some(),
            "original descriptor must survive rename failure"
        );
    }

    #[tokio::test]
    async fn test_n1_concurrent_cursor_persistence_and_install_detach_deadlock() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let runtime = Arc::new(Runtime::new(Some(store_path)));
        let hub = Arc::new(TerminalOutputHub::new(32));

        let r1 = runtime.clone();
        let h1 = hub.clone();
        let t1 = tokio::task::spawn_blocking(move || {
            for i in 0..200 {
                let desc = Descriptor {
                    host_id: "https://relay.example.com".into(),
                    generation: Epoch(1),
                    target: RemoteTerminalTarget {
                        machine_id: format!("m-{i}"),
                        daemon_epoch: Epoch(1),
                        session_id: format!("s-{i}"),
                    },
                    after_sequence: None,
                };
                if let Ok(proxy) = Proxy::new(desc, h1.clone()) {
                    let _ = r1.install(proxy);
                }
            }
        });

        let r2 = runtime.clone();
        let t2 = tokio::task::spawn_blocking(move || {
            for i in 0..200 {
                let id = format!("daemon-session:https://relay.example.com:m-{i}:1:s-{i}");
                r2.simulate_cursor_persistence(&id, Some(Epoch(i as u64)));
            }
        });

        // The deadline exists to catch an opposite-lock-order deadlock, which
        // never completes; 200 serialized temp-file+rename cycles on this
        // volume legitimately exceed a few seconds under load, so a tight
        // bound produces false failures (and, before the ReapOwner try-lock
        // fix, a wedged suite shutdown). Keep it generous but finite.
        let res = tokio::time::timeout(Duration::from_secs(30), async {
            let (res1, res2) = tokio::join!(t1, t2);
            res1.unwrap();
            res2.unwrap();
        })
        .await;

        assert!(
            res.is_ok(),
            "deadlock detected: opposite lock order hung tasks"
        );
    }

    async fn start_mock_relay(
        temp_dir: &tempfile::TempDir,
        send_output_frames: bool,
    ) -> (
        String,
        PairedHostService,
        Descriptor,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let router = Router::new()
            .route(
                "/api/v1/pair/exchange",
                post(|| async {
                    Json(json!({
                        "token": "fixture-secret",
                        "machineId": "a",
                        "device": {
                            "id": "d",
                            "name": "d",
                            "permission": "control",
                            "accessScope": "machine",
                            "createdAt": 1,
                            "lastSeenAt": 1
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/capabilities",
                get(|| async {
                    Json(json!({
                        "apiVersion": 1,
                        "machineId": "a",
                        "daemonEpoch": "1",
                        "platform": "linux",
                        "accessScope": "machine",
                        "permission": "control",
                        "capabilities": ["terminalCreateV1", "terminalStreamV1"],
                        "limits": {"directoryEntries": 1000, "terminalSessions": 64}
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/sessions/s",
                get(|| async {
                    Json(json!({
                        "status": "running",
                        "session": {
                            "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
                            "workspaceId": "w",
                            "worktree": null,
                            "cwd": "/fixture",
                            "cols": 80,
                            "rows": 24,
                            "running": true,
                            "providerSession": null,
                            "startSequence": "10",
                            "endSequence": "10"
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/socket-ticket",
                post(|| async { Json(json!({"ticket": "fixture-ticket"})) }),
            )
            .route(
                "/host/a/api/v1/terminal/s",
                get(move |ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(move |mut socket| async move {
                        socket
                            .send(Message::Text(
                                json!({
                                    "type": "attached",
                                    "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
                                    "generation": "7",
                                    "cols": 80,
                                    "rows": 24,
                                    "startSequence": "10",
                                    "endSequence": "10",
                                    "replayGap": null
                                })
                                .to_string()
                                .into(),
                            ))
                            .await
                            .unwrap();
                        if send_output_frames {
                            for seq in 11..=20 {
                                let frame = encode_frame(
                                    Metadata::Output {
                                        sequence: seq,
                                        gap: None,
                                    },
                                    b"x",
                                    false,
                                )
                                .unwrap();
                                let _ = socket.send(Message::Binary(frame.into())).await;
                                tokio::time::sleep(Duration::from_millis(5)).await;
                            }
                        }
                        while let Some(msg) = socket.recv().await {
                            if matches!(msg, Ok(Message::Close(_)) | Err(_)) {
                                break;
                            }
                        }
                    })
                }),
            );

        let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });

        let service = PairedHostService::open_test_loopback(temp_dir.path().join("service_data"));
        let host = service
            .pair(crate::paired_host::service::PairRequest {
                relay_origin: format!("http://{address}"),
                pin: crate::paired_host::service::Secret("fixture".into()),
                display_label: "fixture".into(),
            })
            .await
            .unwrap();

        let descriptor = Descriptor {
            host_id: host.host_id.clone(),
            generation: host.generation,
            target: RemoteTerminalTarget {
                machine_id: "a".into(),
                daemon_epoch: Epoch(1),
                session_id: "s".into(),
            },
            after_sequence: None,
        };

        (
            format!("http://{address}"),
            service,
            descriptor,
            shutdown,
            server,
        )
    }

    #[tokio::test]
    async fn test_output_batch_debounce_and_detach_persists_latest_cursor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, true).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub.clone()).unwrap();
        let mut output = hub
            .subscribe_with_sequence(proxy.id(), None)
            .unwrap()
            .receiver;
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        let baseline_persists = test_persist_count_path(&store_path);
        let id = runtime.install(proxy).unwrap();
        // install persists once initially
        assert_eq!(test_persist_count_path(&store_path) - baseline_persists, 1);

        // Wait until all 10 output chunks are delivered to the output hub
        for _ in 0..10 {
            tokio::time::timeout(Duration::from_secs(5), output.recv())
                .await
                .unwrap()
                .unwrap();
        }

        let batches_persists = test_persist_count_path(&store_path) - baseline_persists - 1;
        assert!(
            batches_persists <= 2,
            "10 sequential output batches must cause at most 2 persists, got {batches_persists}"
        );

        // Final detach must synchronously persist the latest cursor (sequence 20)
        runtime.detach(&id).await.unwrap();

        let loaded = load_descriptors(&store_path).unwrap();
        let persisted_desc = loaded.get(&id).expect("descriptor present after detach");
        assert_eq!(
            persisted_desc.after_sequence,
            Some(Epoch(20)),
            "detach must persist the latest cursor"
        );

        let _ = shutdown.send(());
        let _ = server.await;
    }

    #[tokio::test]
    async fn test_backpressure_full_channel_write_succeeds_once_slots_free() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, false).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub).unwrap();
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Arc::new(Runtime::default());
        let id = runtime.install(proxy).unwrap();

        let (blocked_tx, blocked_rx) = oneshot::channel();
        let (unblock_tx, unblock_rx) = oneshot::channel();
        runtime.block_actor(&id, blocked_tx, unblock_rx);
        blocked_rx.await.expect("actor confirmed blocked");

        let sender = runtime.sender(&id).unwrap();
        // Channel capacity is 32. Fill all 32 buffer slots
        for _ in 0..32 {
            let (tx, _rx) = oneshot::channel();
            sender
                .try_send(Command::Write(0, b"filler".to_vec(), tx))
                .unwrap();
        }
        // Channel is now pre-filled to capacity
        let (probe_tx, _probe_rx) = oneshot::channel();
        assert!(
            sender
                .try_send(Command::Write(0, b"probe".to_vec(), probe_tx))
                .is_err(),
            "channel must be pre-filled to capacity"
        );

        // Initiate a write on full channel. Bounded backpressure waits for a slot.
        let (write_entered_tx, write_entered_rx) = oneshot::channel();
        let write_fut = {
            let runtime = runtime.clone();
            let id = id.clone();
            tokio::spawn(async move {
                let op = runtime.write(&id, 0, b"keystroke".to_vec()).unwrap();
                let _ = write_entered_tx.send(());
                op.await
            })
        };

        // Ensure write_fut has entered and is waiting on backpressure
        write_entered_rx.await.expect("write future entered");
        tokio::task::yield_now().await;
        assert!(
            !write_fut.is_finished(),
            "write must wait on backpressure instead of immediately dropping keystroke"
        );

        // Free slots by unblocking the actor
        let _ = unblock_tx.send(());

        let res = tokio::time::timeout(Duration::from_secs(5), write_fut)
            .await
            .expect("write must complete within timeout")
            .unwrap();

        assert!(
            res.is_ok(),
            "write must succeed with no PAIRED_PROXY_UNAVAILABLE once slots free: {res:?}"
        );

        let _ = runtime.detach(&id).await;
        let _ = shutdown.send(());
        let _ = server.await;
    }

    #[tokio::test]
    async fn test_actor_abort_inside_debounce_persists_latest_cursor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, true).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub.clone()).unwrap();
        let mut output = hub
            .subscribe_with_sequence(proxy.id(), None)
            .unwrap()
            .receiver;
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        let id = runtime.install(proxy).unwrap();

        // Wait until all 10 output chunks (sequences 11..=20) are delivered to output hub
        for _ in 0..10 {
            tokio::time::timeout(Duration::from_secs(5), output.recv())
                .await
                .unwrap()
                .unwrap();
        }

        // Chunks 11..=20 were received; chunks 12..=20 arrived inside the debounce window.
        // Abort the actor task before the debounce timer elapses.
        runtime.abort_actor(&id).await;

        // On-disk snapshot must carry the latest cursor (sequence 20), saved by the drop guard.
        let loaded = load_descriptors(&store_path).unwrap();
        let persisted_desc = loaded.get(&id).expect("descriptor present after abort");
        assert_eq!(
            persisted_desc.after_sequence,
            Some(Epoch(20)),
            "abort inside debounce window must persist the latest cursor"
        );

        let _ = shutdown.send(());
        let _ = server.await;
    }
}
