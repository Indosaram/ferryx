//! One always-on hub. The daemon opens it after it is ready. Unit tests that build a
//! daemon server do not open it.
use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::agent_control::AgentControl;
use crate::config::HubConfig;
use crate::error::Result;
use crate::import::import_once;
use crate::sessions::watch::ObserveOutcome;
use crate::sessions::watch::{NotifyPolicy, Observation, Watcher};

pub struct Hub {
    pub pool: SqlitePool,
    pub control: Arc<dyn AgentControl>,
    pub watcher: Arc<Watcher>,
    pub config: HubConfig,
}

impl Hub {
    pub async fn open(config: HubConfig, control: Arc<dyn AgentControl>) -> Result<Arc<Self>> {
        let path = config
            .database_path
            .clone()
            .unwrap_or_else(default_database_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pool = crate::storage::connect(&path).await?;
        if config.import_invoked {
            if let Some(source) = &config.import_from {
                import_once(&pool, source).await?;
            }
        }
        let watcher = Arc::new(Watcher::new(pool.clone(), NotifyPolicy::from_env()));
        Ok(Arc::new(Self {
            pool,
            control,
            watcher,
            config,
        }))
    }

    pub async fn ingest(&self, observation: &Observation) -> Result<ObserveOutcome> {
        let now = chrono::Utc::now();
        let outcome = self.watcher.on_observation(observation, now).await?;
        for notice in &outcome.due {
            self.watcher.deliver(notice, now).await?;
        }
        Ok(outcome)
    }
}

pub fn default_database_path() -> PathBuf {
    if let Ok(path) = std::env::var("FERRYX_HUB_DB") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home {
        Some(home) => PathBuf::from(home)
            .join(".ferryx")
            .join("hub")
            .join("hub.sqlite"),
        None => PathBuf::from("ferryx-hub.sqlite"),
    }
}
