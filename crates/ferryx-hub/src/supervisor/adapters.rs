//! Production adapters behind the supervisor's session tools: [`SessionControl`] over
//! [`AgentControl`] (the daemon's owner-socket bridge in production) and a [`FleetSource`] over
//! the paired hosts with the stale merge.
//!
//! Every write the adapters make carries a request id that starts with
//! [`SUPERVISOR_REQUEST_PREFIX`], which is how the daemon tells a supervisor answer from the
//! owner's own and keeps the owner's answer history free of the supervisor's choices.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use sqlx::SqlitePool;

use super::panel_contract::FleetSession;
use crate::agent_control::{AgentControl, AgentOp, LOCAL_HOST_ID};
use crate::error::{HubError, Result};
use crate::fleet::{list_persisted, merge_sessions, InventoryEntry};
use crate::meta::mhc_supervisor::FleetSource;

/// Request ids of writes the supervisor itself made start with this.
pub const SUPERVISOR_REQUEST_PREFIX: &str = "sup-tool-";

/// True for a request id minted by the supervisor, never for the owner's own click or typing.
pub fn is_supervisor_request(request_id: Option<&str>) -> bool {
    request_id.is_some_and(|id| id.starts_with(SUPERVISOR_REQUEST_PREFIX))
}

/// The agent control the supervisor's session tools run on. It wraps the daemon's bridge so
/// that every call the tools make is
/// - owner-only: the hub database must hold exactly one supervisor owner identity;
/// - routed by where the session lives: a session ref naming this computer (`"local"` or the
///   gateway's own host id) goes to the local daemon, any other host id to that paired host;
/// - marked as the supervisor's: every mutation carries a request id starting with
///   [`SUPERVISOR_REQUEST_PREFIX`], so the daemon never learns it as the owner's own answer.
pub struct SupervisorAgentControl {
    inner: Arc<dyn AgentControl>,
    local_hosts: Vec<String>,
    pool: SqlitePool,
}

impl SupervisorAgentControl {
    pub fn new(inner: Arc<dyn AgentControl>, local_hosts: Vec<String>, pool: SqlitePool) -> Self {
        Self {
            inner,
            local_hosts,
            pool,
        }
    }
}

fn marked(request_id: String) -> String {
    if request_id.starts_with(SUPERVISOR_REQUEST_PREFIX) {
        request_id
    } else {
        format!("{SUPERVISOR_REQUEST_PREFIX}{request_id}")
    }
}

#[async_trait]
impl AgentControl for SupervisorAgentControl {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value> {
        super::admission::installation_owner(&self.pool)
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        let host = host_id.filter(|host| {
            !host.is_empty()
                && *host != LOCAL_HOST_ID
                && !self.local_hosts.iter().any(|known| known == host)
        });
        let op = match op {
            AgentOp::Send {
                session_id,
                text,
                request_id,
            } => AgentOp::Send {
                session_id,
                text,
                request_id: marked(request_id),
            },
            AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                request_id,
            } => AgentOp::Answer {
                session_id,
                prompt_id,
                screen_revision,
                answer,
                request_id: marked(request_id),
            },
            AgentOp::Stop {
                session_id,
                request_id,
            } => AgentOp::Stop {
                session_id,
                request_id: marked(request_id),
            },
            other => other,
        };
        self.inner.execute(host, op).await
    }
}

/// A paired computer the fleet lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairedHost {
    pub host_id: String,
    pub label: String,
    /// Offline hosts are not asked; their last known sessions are shown stale.
    pub online: bool,
}

/// The paired computers with a machine grant, online or not: an offline one stays in the fleet,
/// shown stale with its last known sessions.
#[async_trait]
pub trait HostDirectory: Send + Sync {
    async fn hosts(&self) -> Vec<PairedHost>;
}

/// How long one paired host may take to list before its cached sessions are shown stale.
pub const REMOTE_LIST_TIMEOUT: Duration = Duration::from_secs(3);

/// [`FleetSource`] over the paired hosts only. This computer's own sessions come from the
/// gateway in-process; the caller adds them. A host that does not answer keeps its last known
/// sessions, marked `stale`.
pub struct RemoteFleetSource {
    control: Arc<dyn AgentControl>,
    pool: SqlitePool,
    directory: Arc<dyn HostDirectory>,
    timeout: Duration,
}

impl RemoteFleetSource {
    pub fn new(
        control: Arc<dyn AgentControl>,
        pool: SqlitePool,
        directory: Arc<dyn HostDirectory>,
    ) -> Self {
        Self {
            control,
            pool,
            directory,
            timeout: REMOTE_LIST_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn cached(&self, host_id: &str) -> Vec<Value> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT sessions FROM fleet_inventory_cache WHERE host_id = ?")
                .bind(host_id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);
        row.and_then(|(text,)| serde_json::from_str::<Vec<Value>>(&text).ok())
            .unwrap_or_default()
    }
}

#[async_trait]
impl FleetSource for RemoteFleetSource {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        let paired = self.directory.hosts().await;
        let offline: Vec<String> = paired
            .iter()
            .filter(|host| !host.online && host.host_id != LOCAL_HOST_ID)
            .map(|host| host.host_id.clone())
            .collect();
        let ids: Vec<String> = paired
            .iter()
            .filter(|host| host.online && host.host_id != LOCAL_HOST_ID)
            .map(|host| host.host_id.clone())
            .collect();
        let labels: HashMap<String, String> = paired
            .into_iter()
            .map(|host| (host.host_id, host.label))
            .collect();
        let mut entries = list_persisted(self.control.as_ref(), &self.pool, &ids, self.timeout).await;
        // A host that failed outright has no sessions in the entry; its last good list is
        // what the panel shows, marked stale.
        for entry in entries.iter_mut() {
            if entry.error.is_some() && entry.sessions.is_empty() {
                entry.sessions = self.cached(&entry.host_id).await;
                entry.stale = true;
            }
        }
        for host_id in offline {
            let sessions = self.cached(&host_id).await;
            entries.push(InventoryEntry {
                host_id,
                sessions,
                error: Some("the computer is offline".to_owned()),
                stale: true,
            });
        }
        let entries: Vec<InventoryEntry> = entries;
        Ok(merge_sessions(&labels, &entries, now_ms()))
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::RecordingControl;
    use serde_json::json;

    use crate::supervisor::panel_contract::{SessionControl, SessionRef};
    use crate::supervisor::session_control::AgentSessionControl;

    fn session(host: &str, id: &str) -> SessionRef {
        SessionRef::new(host, id)
    }

    async fn owned_pool() -> SqlitePool {
        let pool = crate::storage::memory().await.unwrap();
        crate::supervisor::provision_owner(
            &pool,
            "slack:T1:U-owner",
            "ferryx",
            "github.com/example/ferryx",
            "host-a",
            "/repo/.git",
        )
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn the_session_tools_reach_agent_control_routed_and_marked_as_the_supervisor() {
        let recording = Arc::new(RecordingControl::default());
        let guarded = Arc::new(SupervisorAgentControl::new(
            recording.clone(),
            vec!["mac".to_owned()],
            owned_pool().await,
        ));
        let control = AgentSessionControl::new(guarded);

        control.stop(&session("mac", "s1")).await.unwrap();
        control.send(&session("local", "s2"), "continue").await.unwrap();
        control
            .answer(&session("far", "s3"), "p1", Some("r7"), 1)
            .await
            .unwrap();
        control.read_tail(&session("mac", "s1"), 1).await.ok();

        let ops = recording.ops();
        assert_eq!(ops[0].0, None, "the gateway's own host id is this computer");
        assert!(matches!(&ops[0].1, AgentOp::Stop { session_id, request_id }
            if session_id == "s1" && is_supervisor_request(Some(request_id.as_str()))));
        assert_eq!(ops[1].0, None);
        assert!(matches!(&ops[1].1, AgentOp::Send { text, request_id, .. }
            if text == "continue" && is_supervisor_request(Some(request_id.as_str()))));
        assert_eq!(ops[2].0.as_deref(), Some("far"), "another host id is a paired host");
        assert!(matches!(&ops[2].1, AgentOp::Answer { prompt_id, screen_revision, answer, request_id, .. }
            if prompt_id == "p1" && screen_revision == "r7"
                && answer == &json!({"optionIndex": 1})
                && is_supervisor_request(Some(request_id.as_str()))));
        assert!(matches!(&ops[3].1, AgentOp::Read { .. }));
    }

    #[tokio::test]
    async fn without_a_single_supervisor_owner_nothing_is_sent() {
        let pool = crate::storage::memory().await.unwrap();
        let recording = Arc::new(RecordingControl::default());
        let guarded = SupervisorAgentControl::new(recording.clone(), Vec::new(), pool.clone());
        assert!(guarded.execute(None, AgentOp::List).await.is_err());
        assert!(recording.ops().is_empty());

        crate::supervisor::provision_owner(
            &pool,
            "slack:T1:U-owner",
            "ferryx",
            "github.com/example/ferryx",
            "host-a",
            "/repo/.git",
        )
        .await
        .unwrap();
        guarded.execute(None, AgentOp::List).await.unwrap();
        assert_eq!(recording.ops().len(), 1);
    }

    struct Directory(Vec<PairedHost>);

    #[async_trait]
    impl HostDirectory for Directory {
        async fn hosts(&self) -> Vec<PairedHost> {
            self.0.clone()
        }
    }

    fn paired(id: &str, label: &str) -> PairedHost {
        PairedHost {
            host_id: id.to_owned(),
            label: label.to_owned(),
            online: true,
        }
    }

    fn offline(id: &str, label: &str) -> PairedHost {
        PairedHost {
            online: false,
            ..paired(id, label)
        }
    }

    #[tokio::test]
    async fn the_remote_fleet_lists_paired_hosts_and_keeps_an_offline_one_stale() {
        let pool = crate::storage::memory().await.unwrap();
        let recording = Arc::new(RecordingControl::default());
        *recording.result.lock().unwrap() = json!({"sessions": [{
            "sessionId": "live", "cwd": "/work/a", "agent": "claude", "state": "working"
        }]});
        let directory = Arc::new(Directory(vec![paired("omarchy", "Omarchy"), paired("away", "Laptop")]));
        let source = RemoteFleetSource::new(recording.clone(), pool.clone(), directory.clone())
            .with_timeout(Duration::from_millis(50));

        // First pass: both hosts answer; the cache learns both lists.
        let first = source.list().await.unwrap();
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|session| !session.stale));
        assert_eq!(first[0].session_ref, SessionRef::new("omarchy", "live"));
        assert_eq!(first[0].host_label, "Omarchy");

        // The laptop goes offline: its last known session stays, marked stale, and the other
        // host is untouched.
        recording.fail_hosts.lock().unwrap().insert("away".to_owned());
        let second = source.list().await.unwrap();
        let away: Vec<_> = second.iter().filter(|s| s.host_id == "away").collect();
        assert_eq!(away.len(), 1);
        assert!(away[0].stale);
        assert_eq!(away[0].host_label, "Laptop");
        let omarchy: Vec<_> = second.iter().filter(|s| s.host_id == "omarchy").collect();
        assert_eq!(omarchy.len(), 1);
        assert!(!omarchy[0].stale);
    }

    #[tokio::test]
    async fn an_offline_paired_host_is_not_asked_and_shows_its_cached_sessions_stale() {
        let pool = crate::storage::memory().await.unwrap();
        let recording = Arc::new(RecordingControl::default());
        *recording.result.lock().unwrap() =
            json!({"sessions": [{"sessionId": "kept", "cwd": "/w/x", "state": "idle"}]});
        let online = Arc::new(Directory(vec![paired("laptop", "Laptop")]));
        RemoteFleetSource::new(recording.clone(), pool.clone(), online)
            .list()
            .await
            .unwrap();
        let asked = recording.ops().len();

        let away = Arc::new(Directory(vec![offline("laptop", "Laptop")]));
        let listed = RemoteFleetSource::new(recording.clone(), pool, away).list().await.unwrap();

        assert_eq!(recording.ops().len(), asked, "an offline host is not asked");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].stale);
        assert_eq!(listed[0].session_ref, SessionRef::new("laptop", "kept"));
    }

    #[tokio::test]
    async fn a_host_that_never_answered_yields_no_sessions_and_no_error() {
        let pool = crate::storage::memory().await.unwrap();
        let recording = Arc::new(RecordingControl::default());
        recording.fail_hosts.lock().unwrap().insert("new".to_owned());
        let source = RemoteFleetSource::new(
            recording,
            pool,
            Arc::new(Directory(vec![paired("new", "New")])),
        );
        assert!(source.list().await.unwrap().is_empty());
    }
}
