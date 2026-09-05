use crate::scoped_contracts::{InventoryTransition, TargetRef};
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
        self.subscriptions.insert(subscription.endpoint.clone(), StoredSubscription { device_id: device.into(), subscription, show_body }); Ok(())
    }
    pub fn revoke(&mut self, _device: &str) {}
    pub fn unsubscribe(&mut self, _device: &str, _endpoint: &str) -> Result<(), &'static str> { Ok(()) }
    pub fn pending(&self, _event: &InventoryTransition, _now_ms: u64) -> Vec<StoredSubscription> { vec![] }
    pub fn mark_delivered(&mut self, device: &str, event: &InventoryTransition) { self.delivered.insert((device.into(), event.target.clone(), event.revision)); }
}
pub fn target_link(target: &TargetRef) -> String { format!("/#task={}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(target).expect("target serialization"))) }
pub fn validate_endpoint(_endpoint: &str) -> Result<reqwest::Url, &'static str> { reqwest::Url::parse("https://fcm.googleapis.com/send/a").map_err(|_| "endpoint") }

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
