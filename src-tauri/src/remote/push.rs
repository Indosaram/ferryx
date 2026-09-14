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

/// A paired device owns its push endpoints; without a bound, a single device
/// could register unbounded unique endpoints and grow process memory forever.
const MAX_PUSH_SUBSCRIPTIONS: usize = 128;
const MAX_PUSH_FIELD_LEN: usize = 4096;

/// Thread-safe in-memory registry of active push subscriptions.
#[derive(Debug, Default)]
pub struct PushSubscriptionStore {
    subscriptions: Mutex<Vec<PushSubscriptionInfo>>,
}

impl PushSubscriptionStore {
    pub fn new() -> Self {
        Self {
            subscriptions: Mutex::new(Vec::new()),
        }
    }

    /// Registers a subscription. Re-subscribing an existing endpoint replaces
    /// the stored keys; oversized fields and stores at capacity are rejected.
    pub fn subscribe(&self, info: PushSubscriptionInfo) -> Result<(), String> {
        if info.endpoint.len() > MAX_PUSH_FIELD_LEN
            || info.keys.p256dh.len() > MAX_PUSH_FIELD_LEN
            || info.keys.auth.len() > MAX_PUSH_FIELD_LEN
        {
            return Err("push subscription fields exceed size limits".to_string());
        }
        let mut subs = self
            .subscriptions
            .lock()
            .expect("push subscription lock poisoned");
        if let Some(existing) = subs
            .iter_mut()
            .find(|existing| existing.endpoint == info.endpoint)
        {
            *existing = info;
            return Ok(());
        }
        if subs.len() >= MAX_PUSH_SUBSCRIPTIONS {
            return Err("push subscription limit reached".to_string());
        }
        subs.push(info);
        Ok(())
    }

    pub fn unsubscribe(&self, endpoint: &str) {
        let mut subs = self
            .subscriptions
            .lock()
            .expect("push subscription lock poisoned");
        subs.retain(|existing| existing.endpoint != endpoint);
    }

    pub fn list_subscriptions(&self) -> Vec<PushSubscriptionInfo> {
        let subs = self
            .subscriptions
            .lock()
            .expect("push subscription lock poisoned");
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

        store
            .subscribe(sub.clone())
            .expect("subscribe should succeed");
        // Subscribing the same endpoint twice should replace, not duplicate.
        store
            .subscribe(sub.clone())
            .expect("re-subscribe should succeed");
        let subs = store.list_subscriptions();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0], sub);

        store.unsubscribe(&sub.endpoint);
        assert!(store.list_subscriptions().is_empty());
    }

    #[test]
    fn test_push_subscription_store_bounds() {
        let store = PushSubscriptionStore::new();
        let oversized = PushSubscriptionInfo {
            endpoint: "e".repeat(MAX_PUSH_FIELD_LEN + 1),
            keys: PushSubscriptionKeys {
                p256dh: "k".to_string(),
                auth: "a".to_string(),
            },
        };
        assert!(store.subscribe(oversized).is_err());

        for i in 0..MAX_PUSH_SUBSCRIPTIONS {
            store
                .subscribe(PushSubscriptionInfo {
                    endpoint: format!("https://push.example.com/sub/{i}"),
                    keys: PushSubscriptionKeys {
                        p256dh: "p256dh-key".to_string(),
                        auth: "auth-key".to_string(),
                    },
                })
                .expect("subscription within capacity should succeed");
        }
        assert!(store
            .subscribe(PushSubscriptionInfo {
                endpoint: "https://push.example.com/sub/overflow".to_string(),
                keys: PushSubscriptionKeys {
                    p256dh: "p256dh-key".to_string(),
                    auth: "auth-key".to_string(),
                },
            })
            .is_err());
        // Replacing an existing endpoint still works at capacity.
        assert!(store
            .subscribe(PushSubscriptionInfo {
                endpoint: "https://push.example.com/sub/0".to_string(),
                keys: PushSubscriptionKeys {
                    p256dh: "rotated".to_string(),
                    auth: "auth-key".to_string(),
                },
            })
            .is_ok());
        assert_eq!(store.list_subscriptions().len(), MAX_PUSH_SUBSCRIPTIONS);
    }
}
