use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PushSubscriptionInfo {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStateNotification {
    pub agent_id: String,
    pub agent_name: String,
    pub state: String, // e.g. "waiting_input", "blocked", "completed"
    pub title: String,
    pub body: String,
}

pub fn format_agent_push_payload(notif: &AgentStateNotification) -> Result<String, String> {
    serde_json::to_string(notif).map_err(|e| e.to_string())
}

/// Thread-safe in-memory registry of active push subscriptions.
#[derive(Debug, Default)]
pub struct PushSubscriptionStore {
    subscriptions: Mutex<Vec<PushSubscriptionInfo>>,
}

impl PushSubscriptionStore {
    pub fn new() -> Self {
        Self { subscriptions: Mutex::new(Vec::new()) }
    }

    pub fn subscribe(&self, info: PushSubscriptionInfo) {
        let mut subs = self.subscriptions.lock().expect("push subscription lock poisoned");
        if !subs.iter().any(|existing| existing.endpoint == info.endpoint) {
            subs.push(info);
        }
    }

    pub fn unsubscribe(&self, endpoint: &str) {
        let mut subs = self.subscriptions.lock().expect("push subscription lock poisoned");
        subs.retain(|existing| existing.endpoint != endpoint);
    }

    pub fn list_subscriptions(&self) -> Vec<PushSubscriptionInfo> {
        let subs = self.subscriptions.lock().expect("push subscription lock poisoned");
        subs.clone()
    }
}

static GLOBAL_PUSH_STORE: OnceLock<PushSubscriptionStore> = OnceLock::new();

/// Returns the process-wide push subscription store, initializing it on first use.
pub fn global_push_store() -> &'static PushSubscriptionStore {
    GLOBAL_PUSH_STORE.get_or_init(PushSubscriptionStore::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_push_payload_dispatch() {
        let notif = AgentStateNotification {
            agent_id: "agent-1".to_string(),
            agent_name: "Builder".to_string(),
            state: "waiting_input".to_string(),
            title: "Agent needs input".to_string(),
            body: "Builder is waiting for your input.".to_string(),
        };

        let payload = format_agent_push_payload(&notif).expect("payload should serialize");
        assert!(payload.contains("\"agent_id\":\"agent-1\""));
        assert!(payload.contains("\"state\":\"waiting_input\""));
        assert!(payload.contains("Builder is waiting for your input."));

        let store = PushSubscriptionStore::new();
        let sub = PushSubscriptionInfo {
            endpoint: "https://push.example.com/sub/1".to_string(),
            keys: PushSubscriptionKeys {
                p256dh: "p256dh-key".to_string(),
                auth: "auth-key".to_string(),
            },
        };

        store.subscribe(sub.clone());
        // Subscribing the same endpoint twice should not duplicate entries.
        store.subscribe(sub.clone());
        let subs = store.list_subscriptions();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0], sub);

        store.unsubscribe(&sub.endpoint);
        assert!(store.list_subscriptions().is_empty());
    }
}
