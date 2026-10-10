use crate::scoped_contracts::{InventoryTransition, TargetRef, TransitionKind, TransitionSource};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Subscription {
    pub endpoint: String,
    pub keys: Keys,
    pub expiration_time: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keys { pub p256dh: String, pub auth: String }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSubscription { pub device_id: String, pub subscription: Subscription, pub show_body: bool }
#[derive(Default, Serialize, Deserialize)]
pub struct PushStore {
    pub subscriptions: HashMap<String, StoredSubscription>,
    pub delivered: HashSet<(String, TargetRef, u64)>,
}
impl PushStore {
    pub fn subscribe(&mut self, device: &str, subscription: Subscription, show_body: bool) -> Result<(), &'static str> {
        validate_endpoint(&subscription.endpoint)?;
        if self.subscriptions.get(&subscription.endpoint).is_some_and(|stored| stored.device_id != device) {
            return Err("endpoint belongs to another device");
        }
        self.subscriptions.insert(subscription.endpoint.clone(), StoredSubscription { device_id: device.into(), subscription, show_body }); Ok(())
    }
    pub fn revoke(&mut self, device: &str) {
        self.subscriptions.retain(|_, stored| stored.device_id != device);
        self.delivered.retain(|(owner, _, _)| owner != device);
    }
    pub fn unsubscribe(&mut self, device: &str, endpoint: &str) -> Result<(), &'static str> {
        if self.subscriptions.get(endpoint).is_some_and(|stored| stored.device_id != device) {
            return Err("endpoint belongs to another device");
        }
        self.subscriptions.remove(endpoint);
        Ok(())
    }
    pub fn pending(&self, event: &InventoryTransition, now_ms: u64) -> Vec<StoredSubscription> {
        match event.kind {
            TransitionKind::Waiting => {},
            TransitionKind::TaskComplete if matches!(event.source, TransitionSource::Provider { .. }) => {},
            _ => return Vec::new(),
        }
        self.subscriptions.values().filter(|stored| {
            !stored.subscription.expiration_time.is_some_and(|expires| expires <= now_ms)
                && !self.delivered.contains(&(stored.device_id.clone(), event.target.clone(), event.revision))
        }).cloned().collect()
    }
    pub fn mark_delivered(&mut self, device: &str, event: &InventoryTransition) { self.delivered.insert((device.into(), event.target.clone(), event.revision)); }
}
pub fn target_link(target: &TargetRef) -> String { format!("/#task={}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(target).expect("target serialization"))) }
pub fn validate_endpoint(endpoint: &str) -> Result<reqwest::Url, &'static str> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "endpoint")?;
    if url.scheme() != "https" || url.host_str() != Some("fcm.googleapis.com")
        || !url.username().is_empty() || url.password().is_some()
        || url.port_or_known_default() != Some(443) || url.fragment().is_some()
    {
        return Err("endpoint");
    }
    Ok(url)
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
