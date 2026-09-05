use super::*;
use crate::remote::{AuthManager, DeviceInfo, DevicePermission};
use futures_util::future::BoxFuture;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{Mutex, watch};

/// Host adapters must resolve registered workspace IDs; never accept client paths.
/// Allocation must return a backend-authored identity. Start must launch that exact allocation.
pub trait ControlBackend: Send + Sync {
    fn create(&self, params: Value) -> BoxFuture<'_, Result<Agent>>;
    fn execute(&self, target: TargetRef, operation: String, params: Value) -> BoxFuture<'_, Result<Value>>;
    fn read(&self, target: TargetRef, limit: usize) -> BoxFuture<'_, Result<Value>>;
}
pub struct Service {
    pub inventory: Mutex<Inventory>,
    pub auth: AuthManager,
    pub allow_control: AtomicBool,
    backends: parking_lot::RwLock<HashMap<String, Arc<dyn ControlBackend>>>,
    scopes: parking_lot::RwLock<HashMap<String, Vec<String>>>,
    receipts: Mutex<HashMap<(String, String), (Value, Result<Value>)>>,
    changed: watch::Sender<u64>,
}
impl Service {
    pub fn new(auth: AuthManager, capacity: usize) -> Arc<Self> {
        let (changed, _) = watch::channel(0);
        Arc::new(Self { inventory: Mutex::new(Inventory::new(capacity)), auth, allow_control: AtomicBool::new(false), backends: parking_lot::RwLock::new(HashMap::new()), scopes: parking_lot::RwLock::new(HashMap::new()), receipts: Mutex::new(HashMap::new()), changed })
    }
    pub async fn register_backend(&self, host: String, owner: String, epoch: Epoch, complete: bool, backend: Arc<dyn ControlBackend>) {
        self.inventory.lock().await.register_host(host.clone(), owner, epoch, complete);
        self.backends.write().insert(host, backend);
    }
    pub fn grant_hosts(&self, device: String, hosts: Vec<String>) { self.scopes.write().insert(device, hosts); self.notify_policy(); }
    pub fn notify_policy(&self) { self.changed.send_modify(|v| *v += 1); }
    pub fn revoke(&self, device: &str) { self.auth.revoke_device(device); self.scopes.write().remove(device); self.notify_policy(); }
    pub fn authorize(&self, token: &str, host: Option<&str>, mutation: bool) -> Result<DeviceInfo> {
        let d = self.auth.validate_token(token).map_err(|_| error(ScopeErrorCode::Unauthorized, "device token expired or revoked"))?;
        if mutation && (!self.allow_control.load(Ordering::SeqCst) || d.permission != DevicePermission::Control) { return Err(error(ScopeErrorCode::Forbidden, "control permission required")); }
        if let Some(host) = host { if !self.scopes.read().get(&d.id).is_some_and(|hosts| hosts.iter().any(|h| h == host)) { return Err(error(ScopeErrorCode::Forbidden, "host outside device scope")); } }
        Ok(d)
    }
    pub async fn mutate(&self, token: &str, op: &str, envelope: MutationEnvelope<Value>) -> Result<Value> {
        if !matches!(op,"create"|"start"|"prompt"|"stop") { return Err(error(ScopeErrorCode::Unsupported,"operation unsupported")); }
        let host = envelope.target.as_ref().map(|t| t.host_id.as_str()).or_else(||envelope.params.get("hostId").and_then(Value::as_str)).ok_or_else(||error(ScopeErrorCode::InvalidRequest,"hostId or target required"))?;
        let device = self.authorize(token,Some(host),true)?;
        if envelope.request_id.is_empty() || envelope.request_id.len()>128 { return Err(error(ScopeErrorCode::InvalidRequest,"requestId must be 1..128 bytes")); }
        let payload = serde_json::json!({"operation":op,"target":envelope.target,"params":envelope.params});
        let key = (device.id,envelope.request_id.clone());
        // Serialize dispatch and receipt recording: concurrent identical retries cannot both write.
        let mut receipts = self.receipts.lock().await;
        let mut inventory = self.inventory.lock().await;
        if let Some(t) = &envelope.target { inventory.check_epoch(t)?; }
        if let Some((prior,result)) = receipts.get(&key) {
            return if prior == &payload {result.clone()} else {Err(error(ScopeErrorCode::RequestConflict,"request ID used with another payload"))};
        }
        if receipts.len() >= 10000 { return Err(error(ScopeErrorCode::ControlConflict,"receipt capacity reached; owner maintenance required")); }
        self.authorize(token,Some(host),true)?;
        let backend = self.backend(host)?;
        let result = if op == "create" {
            if envelope.target.is_some() { return Err(error(ScopeErrorCode::InvalidRequest,"create does not accept target")); }
            match backend.create(envelope.params.clone()).await {
                Ok(agent) => {
                    if agent.target.host_id != host { Err(error(ScopeErrorCode::TargetExpired,"backend returned foreign host")) }
                    else { inventory.insert(agent.clone()).map(|_|serde_json::to_value(agent).expect("agent serializes")) }
                }, Err(e) => Err(e),
            }
        } else {
            let target = envelope.target.clone().ok_or_else(||error(ScopeErrorCode::InvalidRequest,"target required"))?;
            if op == "stop" && inventory.items.get(&target).is_some_and(|a|a.state=="exited") { Ok(serde_json::json!({"target":target,"stopped":true})) }
            else {
                inventory.validate(&target)?;
                let result = backend.execute(target.clone(),op.into(),envelope.params.clone()).await;
                if result.is_ok() && op == "stop" { inventory.report(&target,TransitionKind::Stopped,TransitionSource::Lifecycle)?; }
                result
            }
        };
        receipts.insert(key,(payload,result.clone()));
        result
    }
    pub async fn list(&self, token: &str) -> Result<InventorySnapshot<Agent>> {
        let device = self.authorize(token, None, false)?;
        let mut s = self.inventory.lock().await.snapshot();
        let scopes = self.scopes.read();
        let hosts = scopes.get(&device.id);
        s.items.retain(|a| hosts.is_some_and(|h| h.contains(&a.target.host_id)));
        s.unavailable_hosts.retain(|a| hosts.is_some_and(|h| h.contains(a)));
        Ok(s)
    }
    pub async fn read(&self, token: &str, target: TargetRef, limit: usize) -> Result<Value> {
        self.authorize(token, Some(&target.host_id), false)?;
        if limit == 0 || limit > 1000 { return Err(error(ScopeErrorCode::InvalidRequest, "limit must be 1..1000")); }
        self.inventory.lock().await.validate(&target)?;
        let backend = self.backend(&target.host_id)?;
        backend.read(target, limit).await
    }
    fn backend(&self, host: &str) -> Result<Arc<dyn ControlBackend>> { self.backends.read().get(host).cloned().ok_or_else(|| error(ScopeErrorCode::InventoryIncomplete, "host backend unavailable")) }
    pub async fn wait(&self, token: &str, target: TargetRef, after: u64, until: &str, timeout_ms: u64) -> Result<ScopeEvent<Value>> {
        self.authorize(token, Some(&target.host_id), false)?;
        if timeout_ms == 0 || timeout_ms > 300_000 { return Err(error(ScopeErrorCode::InvalidRequest, "timeout must be 1..300000ms")); }
        let mut policy = self.changed.subscribe();
        let (replay, mut rx) = { let i = self.inventory.lock().await; (i.replay(after), i.tx.subscribe()) };
        let events = match replay { EventReplay::Events { events, .. } => events, EventReplay::Gap { .. } => return Err(error(ScopeErrorCode::InventoryIncomplete, "event gap; fetch snapshot")) };
        if let Some(e) = events.into_iter().find(|e| e.target == target && (e.event_type == until || e.event_type == "stopped")) { return Ok(e); }
        tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
            loop {
                tokio::select! {
                    e = rx.recv() => {
                        let e = e.map_err(|_| error(ScopeErrorCode::InventoryIncomplete, "event gap; fetch snapshot"))?;
                        self.authorize(token, Some(&target.host_id), false)?;
                        if e.target == target && (e.event_type == until || e.event_type == "stopped") { return Ok(e); }
                    }
                    _ = policy.changed() => { self.authorize(token, Some(&target.host_id), false)?; }
                }
            }
        }).await.map_err(|_| error(ScopeErrorCode::Timeout, "wait deadline exceeded"))?
    }
}
