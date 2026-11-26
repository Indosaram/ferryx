//! Per-host client registry for multiplexed Noise-encrypted gateway tunnels (`FXMUX1`).
//!
//! One live [`RelayMuxClient`] per `(host_id, generation)` carries every paired HTTP request,
//! terminal WebSocket, and DAG watch over a single encrypted session. Concurrent callers
//! coalesce onto one dial; an older daemon that answers HTTP 400 to `FXMUX1` is remembered per
//! `(host_id, generation)` and falls back to the legacy one-session-per-stream path.

use super::client::ClientError;
use super::path_select::AttachPath;
use crate::remote::relay_mux::{
    MuxHandshakeError, RelayMuxClient, RelayMuxStream, RELAY_MUX_PING_INTERVAL,
    RELAY_MUX_SILENCE_LIMIT,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MuxKey {
    pub host_id: String,
    pub generation: u64,
}

#[derive(Clone)]
pub struct MuxEntry {
    pub client: Arc<RelayMuxClient>,
    pub path: AttachPath,
}

#[derive(Clone)]
enum DialOutcome {
    Ready(MuxEntry),
    Unsupported,
    Failed(ClientError),
}

struct Registry {
    live: HashMap<MuxKey, MuxEntry>,
    dialing: HashMap<MuxKey, watch::Receiver<Option<DialOutcome>>>,
    legacy_no_mux: HashSet<MuxKey>,
}

impl Registry {
    fn new() -> Self {
        Self {
            live: HashMap::new(),
            dialing: HashMap::new(),
            legacy_no_mux: HashSet::new(),
        }
    }
}

static GLOBAL_RELAY_MUXES: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::new()));

pub fn mux_enabled() -> bool {
    !matches!(std::env::var("FERRYX_RELAY_MUX").as_deref(), Ok("0" | "false" | "FALSE"))
}

pub fn active_path(host_id: &str) -> Option<AttachPath> {
    let reg = GLOBAL_RELAY_MUXES.lock().unwrap_or_else(|p| p.into_inner());
    reg.live
        .iter()
        .find(|(k, entry)| k.host_id == host_id && entry.client.is_live())
        .map(|(_, entry)| entry.path)
}

pub fn evict_host(host_id: &str) {
    let removed: Vec<MuxEntry> = {
        let mut reg = GLOBAL_RELAY_MUXES.lock().unwrap_or_else(|p| p.into_inner());
        reg.legacy_no_mux.retain(|k| k.host_id != host_id);
        let keys: Vec<MuxKey> = reg
            .live
            .keys()
            .filter(|k| k.host_id == host_id)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|k| reg.live.remove(&k))
            .collect()
    };
    for entry in removed {
        entry.client.mark_dead();
    }
}

pub fn evict_generation(host_id: &str, generation: u64) {
    let key = MuxKey {
        host_id: host_id.to_owned(),
        generation,
    };
    let removed = {
        let mut reg = GLOBAL_RELAY_MUXES.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.remove(&key)
    };
    if let Some(entry) = removed {
        entry.client.mark_dead();
    }
}

pub enum OpenChannelOutcome {
    Stream {
        stream: RelayMuxStream,
        path: AttachPath,
        reused: bool,
    },
    UseLegacy,
}

pub async fn open_channel_with<F, Fut>(
    key: MuxKey,
    dial: F,
) -> Result<OpenChannelOutcome, ClientError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(Arc<RelayMuxClient>, AttachPath), Result<MuxHandshakeError, ClientError>>>,
{
    open_channel_in(&GLOBAL_RELAY_MUXES, key, mux_enabled(), dial).await
}

enum OpenAction {
    Done(OpenChannelOutcome),
    Wait(watch::Receiver<Option<DialOutcome>>),
    Dial(watch::Sender<Option<DialOutcome>>),
}

async fn open_channel_in<F, Fut>(
    registry: &Mutex<Registry>,
    key: MuxKey,
    enabled: bool,
    dial: F,
) -> Result<OpenChannelOutcome, ClientError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(Arc<RelayMuxClient>, AttachPath), Result<MuxHandshakeError, ClientError>>>,
{
    if !enabled {
        return Ok(OpenChannelOutcome::UseLegacy);
    }

    let action = {
        let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live.retain(|k, entry| {
            k.host_id != key.host_id || (k.generation == key.generation && entry.client.is_live())
        });
        reg.legacy_no_mux
            .retain(|k| k.host_id != key.host_id || k.generation == key.generation);

        if reg.legacy_no_mux.contains(&key) {
            OpenAction::Done(OpenChannelOutcome::UseLegacy)
        } else if let Some(stream) = reg.live.get(&key).and_then(|entry| {
            entry.client.open_stream().ok().map(|stream| OpenChannelOutcome::Stream {
                stream,
                path: entry.path,
                reused: true,
            })
        }) {
            OpenAction::Done(stream)
        } else {
            reg.live.remove(&key);
            if let Some(rx) = reg.dialing.get(&key) {
                OpenAction::Wait(rx.clone())
            } else {
                let (tx, rx) = watch::channel(None);
                reg.dialing.insert(key.clone(), rx);
                OpenAction::Dial(tx)
            }
        }
    };

    match action {
        OpenAction::Done(outcome) => Ok(outcome),
        OpenAction::Dial(tx) => {
            let outcome = run_dial(registry, &key, tx, dial).await;
            finish_dial_outcome(outcome)
        }
        OpenAction::Wait(mut rx) => {
            let _ = rx.wait_for(|v| v.is_some()).await;
            let outcome = rx
                .borrow()
                .clone()
                .unwrap_or_else(|| DialOutcome::Failed(ClientError::local("HOST_UNAVAILABLE")));
            finish_dial_outcome(outcome)
        }
    }
}

struct DialCleanup<'a> {
    registry: &'a Mutex<Registry>,
    key: MuxKey,
    tx: Option<watch::Sender<Option<DialOutcome>>>,
}

impl Drop for DialCleanup<'_> {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            self.registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .dialing
                .remove(&self.key);
            let _ = tx.send(Some(DialOutcome::Failed(ClientError::local(
                "HOST_UNAVAILABLE",
            ))));
        }
    }
}

async fn run_dial<F, Fut>(
    registry: &Mutex<Registry>,
    key: &MuxKey,
    tx: watch::Sender<Option<DialOutcome>>,
    dial: F,
) -> DialOutcome
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(Arc<RelayMuxClient>, AttachPath), Result<MuxHandshakeError, ClientError>>>,
{
    let mut cleanup = DialCleanup {
        registry,
        key: key.clone(),
        tx: Some(tx),
    };
    let outcome = match dial().await {
        Ok((client, path)) => {
            let entry = MuxEntry { client, path };
            let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.insert(key.clone(), entry.clone());
            reg.dialing.remove(key);
            DialOutcome::Ready(entry)
        }
        Err(Ok(MuxHandshakeError::Unsupported)) => {
            let mut reg = registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.legacy_no_mux.insert(key.clone());
            reg.dialing.remove(key);
            DialOutcome::Unsupported
        }
        Err(Ok(MuxHandshakeError::Transport(_))) => {
            registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .dialing
                .remove(key);
            DialOutcome::Failed(ClientError::local("HOST_UNAVAILABLE"))
        }
        Err(Err(client_err)) => {
            registry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .dialing
                .remove(key);
            DialOutcome::Failed(client_err)
        }
    };
    if let Some(tx) = cleanup.tx.take() {
        let _ = tx.send(Some(outcome.clone()));
    }
    outcome
}

fn finish_dial_outcome(outcome: DialOutcome) -> Result<OpenChannelOutcome, ClientError> {
    match outcome {
        DialOutcome::Ready(entry) => entry
            .client
            .open_stream()
            .map(|stream| OpenChannelOutcome::Stream {
                stream,
                path: entry.path,
                reused: false,
            })
            .map_err(|_| ClientError::local("HOST_UNAVAILABLE")),
        DialOutcome::Unsupported => Ok(OpenChannelOutcome::UseLegacy),
        DialOutcome::Failed(err) => Err(err),
    }
}

pub async fn handshake_stream<S>(
    secure: crate::remote::attach_crypto::SecureStream<S>,
) -> Result<Arc<RelayMuxClient>, MuxHandshakeError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    RelayMuxClient::handshake(secure, RELAY_MUX_PING_INTERVAL, RELAY_MUX_SILENCE_LIMIT).await
}

#[cfg(test)]
#[path = "relay_mux_tests.rs"]
mod tests;
