//! In-memory native relay ownership. No PTY, shell, or SSH launcher exists here.
use super::paired_daemon::Proxy;
use crate::scoped_contracts::Epoch;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

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
#[derive(Default)]
pub struct Runtime { owners: Arc<Mutex<HashMap<String, Owner>>> }
impl Runtime {
    pub fn owns(id: &str) -> bool { id.starts_with("daemon-session:") }
    pub fn contains(&self, id: &str) -> bool { self.owners.lock().get(id).is_some_and(|o| !o.task.is_finished()) }
    pub fn list(&self) -> Vec<String> { self.owners.lock().iter().filter(|(_, o)| !o.task.is_finished()).map(|(id, _)| id.clone()).collect() }
    pub fn install(&self, proxy: Proxy) -> Result<String, String> {
        let id = proxy.id().to_owned();
        let mut owners = self.owners.lock();
        if owners.get(&id).is_some_and(|o| !o.task.is_finished()) { return Err("CONTROL_CONFLICT".into()); }
        owners.remove(&id);
        let (sender, receiver) = mpsc::channel(32);
        let identity = Arc::new(());
        #[cfg(test)]
        let (completed_tx, completed) = tokio::sync::watch::channel(false);
        let reap = ReapOwner {
            owners: Arc::downgrade(&self.owners), id: id.clone(), identity: identity.clone(),
            #[cfg(test)]
            completed: completed_tx,
        };
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
            let result = tokio::time::timeout(Duration::from_secs(10), async {
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
        let result = tokio::time::timeout(Duration::from_secs(10), rx).await.map_err(|_| "TIMEOUT")?.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        // The acknowledgement is emitted only after socket and hub destruction.
        (&mut owner.task).await.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        result
    }
}
