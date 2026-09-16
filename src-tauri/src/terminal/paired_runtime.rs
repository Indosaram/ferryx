//! In-memory native relay ownership. No PTY, shell, or SSH launcher exists here.
use super::paired_daemon::Proxy;
use crate::scoped_contracts::Epoch;
use parking_lot::Mutex;
use std::{collections::HashMap, path::{Path, PathBuf}, sync::Arc, time::Duration};
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
static TEST_FS_FAULT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

#[cfg(test)]
impl FsFault {
    pub(crate) fn set(fault: Self) {
        let val = match fault {
            Self::None => 0,
            Self::FailTempWrite => 1,
            Self::FailRename => 2,
        };
        TEST_FS_FAULT.store(val, std::sync::atomic::Ordering::SeqCst);
    }
    pub(crate) fn get() -> Self {
        match TEST_FS_FAULT.load(std::sync::atomic::Ordering::SeqCst) {
            1 => Self::FailTempWrite,
            2 => Self::FailRename,
            _ => Self::None,
        }
    }
}

fn fs_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailTempWrite {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "injected temp write failure"));
    }
    std::fs::write(path, bytes)
}

fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailRename {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "injected rename failure"));
    }
    std::fs::rename(from, to)
}

fn load_descriptors(path: &Path) -> HashMap<String, super::paired_daemon::Descriptor> {
    if let Ok(data) = std::fs::read(path) {
        if let Ok(map) = serde_json::from_slice::<HashMap<String, super::paired_daemon::Descriptor>>(&data) {
            return map;
        }
    }
    HashMap::new()
}

fn save_descriptors(path: &Path, descriptors: &HashMap<String, super::paired_daemon::Descriptor>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = match serde_json::to_vec_pretty(descriptors) {
        Ok(bytes) => bytes,
        Err(e) => return Err(format!("PAIRED_DESCRIPTOR_SERIALIZE: {e}")),
    };
    // N2: write-then-atomic-replace WITHOUT removing the destination first.
    // A failed temp write or failed rename must leave the previous good file
    // intact and surface the error; the temp file is cleaned up on failure.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
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

type Reply = oneshot::Sender<Result<(), String>>;
enum Command {
    Write(u64, Vec<u8>, Reply),
    Resize(u64, u16, u16, Reply),
    Interrupt(u64, Reply),
    Detach(Reply),
}
struct Owner {
    sender: mpsc::Sender<Command>,
    task: tokio::task::JoinHandle<()>,
    identity: Arc<()>,
    #[cfg(test)]
    completed: tokio::sync::watch::Receiver<bool>,
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
            let mut owners = owners.lock();
            if owners.get(&self.id).is_some_and(|owner| Arc::ptr_eq(&owner.identity, &self.identity)) {
                owners.remove(&self.id);
            }
        }
        #[cfg(test)]
        let _ = self.completed.send(true);
    }
}
impl Drop for Owner { fn drop(&mut self) { self.task.abort(); } }

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
        let descriptors = if let Some(ref path) = store_path {
            load_descriptors(path)
        } else {
            HashMap::new()
        };
        Self {
            owners: Arc::new(Mutex::new(HashMap::new())),
            descriptors: Arc::new(Mutex::new(descriptors)),
            store_path: Arc::new(Mutex::new(store_path)),
        }
    }

    pub fn set_store_path(&self, path: PathBuf) {
        // N1: single lock order — the store-path snapshot is taken BEFORE the
        // descriptors lock is ever acquired; no site may acquire store_path
        // while holding descriptors.
        let loaded = load_descriptors(&path);
        *self.store_path.lock() = Some(path.clone());
        let current = {
            let mut desc_guard = self.descriptors.lock();
            for (k, v) in loaded {
                desc_guard.entry(k).or_insert(v);
            }
            desc_guard.clone()
        };
        if let Err(e) = save_descriptors(&path, &current) {
            eprintln!("[paired_runtime] descriptor save failed after set_store_path: {e}");
        }
    }

    pub fn owns(id: &str) -> bool { id.starts_with("daemon-session:") }
    pub fn contains(&self, id: &str) -> bool { self.owners.lock().get(id).is_some_and(|o| !o.task.is_finished()) }
    pub fn list(&self) -> Vec<String> { self.owners.lock().iter().filter(|(_, o)| !o.task.is_finished()).map(|(id, _)| id.clone()).collect() }
    pub fn descriptor(&self, id: &str) -> Option<super::paired_daemon::Descriptor> {
        self.descriptors.lock().get(id).cloned()
    }
    pub fn remove_descriptor(&self, id: &str) {
        let path = self.store_path.lock().clone();
        let snapshot = {
            let mut descs = self.descriptors.lock();
            let existed = descs.remove(id).is_some();
            if !existed {
                return;
            }
            descs.clone()
        };
        if let Some(ref path) = path {
            if let Err(e) = save_descriptors(path, &snapshot) {
                eprintln!("[paired_runtime] descriptor save failed after remove: {e}");
            }
        }
    }
    pub fn install(&self, proxy: Proxy) -> Result<String, String> {
        let id = proxy.id().to_owned();
        let descriptor = proxy.descriptor().clone();
        let mut owners = self.owners.lock();
        if owners.get(&id).is_some_and(|o| !o.task.is_finished()) { return Err("CONTROL_CONFLICT".into()); }
        owners.remove(&id);

        // N1: store-path snapshot first, then the descriptors lock — never the
        // reverse order. The map is cloned under the short lock and the disk
        // save happens with NO runtime lock held.
        let path = self.store_path.lock().clone();
        let snapshot = {
            let mut descs = self.descriptors.lock();
            descs.insert(id.clone(), descriptor.clone());
            descs.clone()
        };
        if let Some(ref path) = path {
            if let Err(e) = save_descriptors(path, &snapshot) {
                // Roll back the in-memory insert so memory and disk stay
                // consistent: install did not persist, so it did not happen.
                self.descriptors.lock().remove(&id);
                return Err(e);
            }
        }

        let (sender, receiver) = mpsc::channel(32);
        let identity = Arc::new(());
        #[cfg(test)]
        let (completed_tx, completed) = tokio::sync::watch::channel(false);
        let reap = ReapOwner {
            owners: Arc::downgrade(&self.owners), id: id.clone(), identity: identity.clone(),
            #[cfg(test)]
            completed: completed_tx,
        };
        let descriptors = self.descriptors.clone();
        let store_path = self.store_path.clone();
        let task_id = id.clone();
        let task = tokio::spawn(async move {
            let _reap = reap;
            // Drop the proxy and receiver before publishing completion/removing
            // the map entry. A cloned sender then fails closed on this channel.
            let mut proxy = proxy;
            let mut receiver = receiver;
            let period = Duration::from_secs(5);
            let mut keepalive = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
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
                            {
                                let mut descs = descriptors.lock();
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                if let Some(ref path) = *store_path.lock() {
                                    save_descriptors(path, &descs);
                                }
                            }
                            drop(proxy);
                            let _ = reply.send(result);
                            return;
                        }
                        None => return,
                    },
                    _ = keepalive.tick(), if proxy.controller().is_some() => {
                        if let Some(g) = proxy.controller() { if proxy.ping(g).await.is_err() { return; } }
                    }
                    result = proxy.receive(), if proxy.controller().is_some() => {
                        if result.is_err() || proxy.controller().is_none() { return; }
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
                            if let Some(ref path) = *store_path.lock() {
                                let descs = descriptors.lock();
                                save_descriptors(path, &descs);
                            }
                        }
                    }
                }
            }
        });
        owners.insert(id.clone(), Owner {
            sender, task, identity,
            #[cfg(test)]
            completed,
        });
        Ok(id)
    }
    #[cfg(test)]
    pub(crate) fn completion_probe(&self, id: &str, pending: super::remote::RemoteOperation) -> impl std::future::Future<Output = ()> + '_ {
        let (weak, mut completed) = {
            let owners = self.owners.lock();
            let owner = owners.get(id).unwrap();
            (owner.sender.downgrade(), owner.completed.clone())
        };
        let id = id.to_owned();
        async move {
            completed.wait_for(|done| *done).await.unwrap();
            assert!(!self.owners.lock().contains_key(&id), "completed owner retained in map");
            let failure = pending.await.unwrap_err();
            assert_eq!(failure.kind, super::remote::RemoteFailureKind::Disconnected);
            assert_eq!(failure.message, "PAIRED_PROXY_UNAVAILABLE");
            assert_eq!(weak.strong_count(), 0, "owner sender retained");
            assert!(weak.upgrade().is_none());
        }
    }
    fn sender(&self, id: &str) -> Result<mpsc::Sender<Command>, String> {
        self.owners.lock().get(id).map(|o| o.sender.clone()).ok_or_else(|| "PAIRED_PROXY_MISSING".into())
    }
    pub fn write(self: &Arc<Self>, id: &str, generation: u64, data: Vec<u8>) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Write(generation, data, reply))
    }
    pub fn resize(self: &Arc<Self>, id: &str, generation: u64, cols: u16, rows: u16) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Resize(generation, cols, rows, reply))
    }
    pub fn interrupt(&self, id: &str, generation: u64) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Interrupt(generation, reply))
    }
    fn operation(&self, id: &str, command: impl FnOnce(Reply) -> Command + Send + 'static) -> Result<super::remote::RemoteOperation, super::PtyError> {
        let sender = self.sender(id).map_err(super::PtyError::Other)?;
        Ok(Box::pin(async move {
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                let (tx, rx) = oneshot::channel();
                sender.try_send(command(tx)).map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?;
                rx.await.map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?
            }).await.unwrap_or_else(|_| Err("TIMEOUT".into()));
            result.map_err(|message| super::remote::RemoteFailure { kind: super::remote::RemoteFailureKind::Disconnected, message })
        }))
    }
    /// Detach only; deliberately not remote session close.
    pub async fn detach(&self, id: &str) -> Result<(), String> {
        let mut owner = self.owners.lock().remove(id).ok_or("PAIRED_PROXY_MISSING")?;
        if owner.task.is_finished() { return Ok(()); }
        let (tx, rx) = oneshot::channel();
        owner.sender.send(Command::Detach(tx)).await.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        let result = tokio::time::timeout(Duration::from_secs(30), rx).await.map_err(|_| "TIMEOUT")?.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        // The acknowledgement is emitted only after socket and hub destruction.
        (&mut owner.task).await.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
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
                descs.insert(task_id.to_string(), super::paired_daemon::Descriptor {
                    host_id: "https://relay.example.com".into(),
                    generation: Epoch(1),
                    target: crate::remote::machine_protocol::RemoteTerminalTarget {
                        machine_id: "test-machine".into(),
                        daemon_epoch: Epoch(1),
                        session_id: task_id.to_string(),
                    },
                    after_sequence: seq,
                });
                true
            }
        };
        if changed {
            // N1: same single lock order as production paths — take the store
            // path snapshot first, mutate descriptors under the short lock,
            // then persist the snapshot with no lock held.
            let path = self.store_path.lock().clone();
            let snapshot = {
                let descs = self.descriptors.lock();
                descs.clone()
            };
            if let Some(ref path) = path {
                let _ = save_descriptors(path, &snapshot);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::output_hub::TerminalOutputHub;
    use crate::terminal::paired_daemon::Descriptor;
    use crate::remote::machine_protocol::RemoteTerminalTarget;

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
        static FAULT_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
        assert!(reloaded.descriptor(&id1).is_some(), "original descriptor must still be present");
    }

    #[tokio::test]
    async fn test_n2_save_failure_rename_preserves_existing_file() {
        static FAULT_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
        assert!(store_path.exists(), "existing store file must survive rename failure (destructive save defect)");
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert!(reloaded.descriptor(&id1).is_some(), "original descriptor must survive rename failure");
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

        let res = tokio::time::timeout(Duration::from_secs(3), async {
            let (res1, res2) = tokio::join!(t1, t2);
            res1.unwrap();
            res2.unwrap();
        }).await;

        assert!(res.is_ok(), "deadlock detected: opposite lock order hung tasks");
    }
}

