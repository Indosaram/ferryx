use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::agent_control::{AgentControl, AgentOp};
use crate::error::HubError;

/// One host in a fleet list. A failure is an entry, not an error for the whole list.
#[derive(Debug, Clone, PartialEq)]
pub struct InventoryEntry {
    pub host_id: String,
    pub sessions: Vec<Value>,
    pub error: Option<String>,
    /// True when this response is the last successful list because the host hung.
    pub stale: bool,
}

/// Multi-host inventory over paired hosts. Each host is one `AgentControl::execute`
/// of `agents.list`. A hung host returns its last cached list and is marked stale.
pub struct Fleet {
    control: Arc<dyn AgentControl>,
    cache: Mutex<HashMap<String, Vec<Value>>>,
    timeout: Duration,
}

impl Fleet {
    pub fn new(control: Arc<dyn AgentControl>, timeout: Duration) -> Self {
        Self {
            control,
            cache: Mutex::new(HashMap::new()),
            timeout,
        }
    }

    pub async fn seed_cache(&self, host_id: &str, sessions: Vec<Value>) {
        self.cache.lock().await.insert(host_id.to_owned(), sessions);
    }

    pub async fn list(&self, hosts: &[String]) -> Vec<InventoryEntry> {
        let mut cache = self.cache.lock().await;
        list_hosts(self.control.as_ref(), hosts, self.timeout, &mut cache).await
    }
}

/// Lists every host. A failure is an entry. A timeout returns `cache` for that host and marks it stale.
pub async fn list_hosts(
    control: &(impl AgentControl + ?Sized),
    hosts: &[String],
    timeout: Duration,
    cache: &mut HashMap<String, Vec<Value>>,
) -> Vec<InventoryEntry> {
    let mut entries = Vec::with_capacity(hosts.len());
    for host in hosts {
        entries.push(one_host(control, host, timeout, cache).await);
    }
    entries
}

async fn one_host(
    control: &(impl AgentControl + ?Sized),
    host: &str,
    timeout: Duration,
    cache: &mut HashMap<String, Vec<Value>>,
) -> InventoryEntry {
    let host_arg = (host != "local").then_some(host);
    match tokio::time::timeout(timeout, control.execute(host_arg, AgentOp::List)).await {
        Ok(Ok(value)) => {
            let sessions = value
                .get("sessions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            cache.insert(host.to_owned(), sessions.clone());
            InventoryEntry {
                host_id: host.to_owned(),
                sessions,
                error: None,
                stale: false,
            }
        }
        Ok(Err(error)) => InventoryEntry {
            host_id: host.to_owned(),
            sessions: Vec::new(),
            error: Some(error.to_string()),
            stale: false,
        },
        Err(_) => InventoryEntry {
            host_id: host.to_owned(),
            sessions: cache.get(host).cloned().unwrap_or_default(),
            error: Some(HubError::message(format!("host {host} timed out")).to_string()),
            stale: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use serde_json::json;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn list_keeps_a_failed_host_as_an_entry_and_a_hung_host_as_stale_cache() {
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"sessions": [{"sessionId": "live"}]});
        control.fail_hosts.lock().unwrap().insert("down".into());
        let fleet = Fleet::new(control.clone(), Duration::from_millis(30));
        fleet
            .seed_cache("hung", vec![json!({"sessionId": "cached"})])
            .await;

        let first = fleet.list(&["local".into(), "down".into()]).await;
        assert_eq!(first[0].sessions[0]["sessionId"], "live");
        assert!(first[0].error.is_none());
        assert!(!first[0].stale);
        assert!(first[1].error.as_ref().unwrap().contains("down"));
        assert!(first[1].sessions.is_empty());
        assert!(!first[1].stale);

        control.hang.store(true, Ordering::SeqCst);
        let hung = fleet.list(&["hung".into()]).await;
        assert!(hung[0].stale);
        assert_eq!(hung[0].sessions[0]["sessionId"], "cached");
        assert!(hung[0].error.as_ref().unwrap().contains("timed out"));
    }
}
