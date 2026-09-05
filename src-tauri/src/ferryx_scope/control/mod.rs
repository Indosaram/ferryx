use crate::scoped_contracts::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use tokio::sync::broadcast;
pub mod service;
pub mod router;
pub mod local;
pub mod lease;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod boundary_tests;

pub type Result<T> = std::result::Result<T, ScopeError>;
pub fn error(code: ScopeErrorCode, message: impl Into<String>) -> ScopeError {
    ScopeError { code, message: message.into(), retryable: false, details: Value::Null }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub target: TargetRef,
    pub workspace_id: String,
    pub label: String,
    pub state: String,
    pub revision: u64,
    pub source: TransitionSource,
}
pub struct Inventory {
    items: HashMap<TargetRef, Agent>,
    hosts: HashMap<String, (String, Epoch, bool)>,
    revision: u64,
    retained: VecDeque<ScopeEvent<Value>>,
    capacity: usize,
    tx: broadcast::Sender<ScopeEvent<Value>>,
}
impl Inventory {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self { items: HashMap::new(), hosts: HashMap::new(), revision: 0, retained: VecDeque::new(), capacity: capacity.max(1), tx }
    }
    pub fn register_host(&mut self, host: String, owner: String, epoch: Epoch, complete: bool) {
        self.hosts.insert(host, (owner, epoch, complete));
    }
    pub fn insert(&mut self, mut agent: Agent) -> Result<()> {
        self.check_epoch(&agent.target)?;
        if self.items.contains_key(&agent.target) { return Err(error(ScopeErrorCode::RequestConflict,"target already registered")); }
        self.revision += 1;
        agent.revision = self.revision;
        self.items.insert(agent.target.clone(), agent.clone());
        self.emit(&agent.target,"registered",serde_json::to_value(&agent).expect("agent serializes"));
        Ok(())
    }
    fn check_epoch(&self, target: &TargetRef) -> Result<()> {
        match self.hosts.get(&target.host_id) {
            Some((owner,epoch,_)) if owner == &target.owner_id && epoch == &target.epoch => Ok(()),
            Some(_) => Err(error(ScopeErrorCode::TargetExpired,"owner runtime replaced")),
            None => Err(error(ScopeErrorCode::NotFound,"host not registered")),
        }
    }
    pub fn validate(&self, target: &TargetRef) -> Result<&Agent> {
        self.check_epoch(target)?;
        let a = self.items.get(target).ok_or_else(|| error(ScopeErrorCode::NotFound, "target not registered"))?;
        if a.state == "exited" { return Err(error(ScopeErrorCode::TargetExpired,"target exited")); }
        Ok(a)
    }
    pub fn report(&mut self, target: &TargetRef, kind: TransitionKind, source: TransitionSource) -> Result<()> {
        self.validate(target)?;
        if kind == TransitionKind::TaskComplete && !matches!(source,TransitionSource::Provider {..}) { return Err(error(ScopeErrorCode::InvalidRequest,"completion requires provider evidence")); }
        let (state,event_type) = match kind {
            TransitionKind::Waiting => ("waiting","waiting"), TransitionKind::Working => ("working","working"),
            TransitionKind::Idle => ("idle","idle"), TransitionKind::TaskComplete => ("idle","turn.completed"),
            TransitionKind::Stopped => ("exited","stopped"), TransitionKind::Removed => ("exited","removed"),
        };
        self.revision += 1;
        let a = self.items.get_mut(target).expect("validated membership");
        a.state = state.into(); a.source = source; a.revision = self.revision;
        let data = serde_json::to_value(a).expect("agent serializes");
        self.emit(target,event_type,data);
        Ok(())
    }
    fn emit(&mut self, target: &TargetRef, event_type: &str, data: Value) {
        let event = ScopeEvent {sequence:self.revision,revision:self.revision,target:target.clone(),event_type:event_type.into(),data};
        self.retained.push_back(event.clone());
        while self.retained.len() > self.capacity { self.retained.pop_front(); }
        // No receivers is normal: replay remains authoritative.
        let _ = self.tx.send(event);
    }
    pub fn snapshot(&self) -> InventorySnapshot<Agent> {
        let mut unavailable_hosts: Vec<_> = self.hosts.iter().filter(|(_,(_,_,complete))| !complete).map(|(h,_)|h.clone()).collect();
        unavailable_hosts.sort();
        let completeness = if self.hosts.is_empty() {InventoryCompleteness::Unknown} else if unavailable_hosts.is_empty() {InventoryCompleteness::Complete} else {InventoryCompleteness::Partial};
        let mut items:Vec<_> = self.items.values().filter(|a|self.check_epoch(&a.target).is_ok()).cloned().collect();
        items.sort_by_key(|a| (a.target.host_id.clone(),a.target.backend_session_id.clone()));
        InventorySnapshot { revision: self.revision, items, completeness, unavailable_hosts }
    }
    pub fn subscribe(&self) -> (InventorySnapshot<Agent>, broadcast::Receiver<ScopeEvent<Value>>) { (self.snapshot(), self.tx.subscribe()) }
    pub fn replay(&self, after: u64) -> EventReplay<Value, Agent> {
        if after > self.revision || self.retained.front().is_some_and(|e| after.saturating_add(1) < e.sequence) { return EventReplay::Gap {snapshot:self.snapshot(),after_sequence:self.revision}; }
        EventReplay::Events {events:self.retained.iter().filter(|e|e.sequence > after).cloned().collect(),after_sequence:self.revision}
    }
}
