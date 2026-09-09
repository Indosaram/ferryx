use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportType {
    Tailscale,
    Mdns,
    SshTunnel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostAuthStatus {
    Paired,
    Unpaired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostEndpoint {
    pub host_id: String,
    pub name: String,
    pub address: String, // e.g. "100.x.y.z:43821" or "mac-mini.local:43821"
    pub transport: TransportType,
    pub latency_ms: Option<u64>,
    pub auth_status: HostAuthStatus,
    pub online: bool,
}

#[derive(Debug, Default, Clone)]
pub struct TargetRegistry {
    endpoints: Arc<RwLock<std::collections::HashMap<String, HostEndpoint>>>,
}

impl TargetRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn upsert(&self, endpoint: HostEndpoint) {
        self.endpoints
            .write()
            .await
            .insert(endpoint.host_id.clone(), endpoint);
    }
    pub async fn list(&self) -> Vec<HostEndpoint> {
        self.endpoints.read().await.values().cloned().collect()
    }
    pub async fn get(&self, host_id: &str) -> Option<HostEndpoint> {
        self.endpoints.read().await.get(host_id).cloned()
    }
    pub async fn remove(&self, host_id: &str) -> Option<HostEndpoint> {
        self.endpoints.write().await.remove(host_id)
    }
}

pub mod tailscale;

pub mod mdns;
pub mod ssh_tunnel;
