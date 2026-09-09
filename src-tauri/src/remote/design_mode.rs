use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomElementBox {
    pub id: String,
    pub tag: String,
    pub bounds: [f64; 4], // [x, y, width, height]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignModeSnapshot {
    pub session_id: String,
    pub timestamp_ms: u64,
    pub screenshot_png_base64: String,
    pub dom_elements: Vec<DomElementBox>,
}

#[derive(Debug, Default, Clone)]
pub struct DesignModeStagingStore {
    snapshots: Arc<RwLock<HashMap<String, DesignModeSnapshot>>>,
}

impl DesignModeStagingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stage(&self, snapshot: DesignModeSnapshot) {
        let mut lock = self.snapshots.write().unwrap();
        lock.insert(snapshot.session_id.clone(), snapshot);
    }

    pub fn get(&self, session_id: &str) -> Option<DesignModeSnapshot> {
        let lock = self.snapshots.read().unwrap();
        lock.get(session_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_design_mode_snapshot_staging() {
        let snapshot = DesignModeSnapshot {
            session_id: "session-123".to_string(),
            timestamp_ms: 1_694_012_345_678,
            screenshot_png_base64: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".to_string(),
            dom_elements: vec![
                DomElementBox {
                    id: "el-1".to_string(),
                    tag: "button".to_string(),
                    bounds: [10.0, 20.0, 100.0, 40.0],
                    text: Some("Submit".to_string()),
                },
                DomElementBox {
                    id: "el-2".to_string(),
                    tag: "div".to_string(),
                    bounds: [0.0, 0.0, 800.0, 600.0],
                    text: None,
                },
            ],
        };

        let store = DesignModeStagingStore::new();
        store.stage(snapshot.clone());

        let retrieved = store.get("session-123").expect("snapshot should be staged");
        assert_eq!(retrieved, snapshot);
    }
}
