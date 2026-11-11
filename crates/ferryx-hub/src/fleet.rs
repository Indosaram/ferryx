use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::agent_control::{AgentControl, AgentOp};
use crate::error::{HubError, Result};
use crate::supervisor::panel_contract::{
    attention_group, ActionEntry, ActionLog, Attention, AttentionOwnership, Escalation,
    FleetSession, HealthKind, PanelSnapshot, PendingPrompt, RuleStore, SessionRef, StandingRule,
};

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

/// Lists `hosts` with the last successful lists kept in `pool`, so a host that hangs is shown
/// stale with its last sessions even when the caller (a CLI or a new MCP process) is new.
pub async fn list_persisted(
    control: &(impl AgentControl + ?Sized),
    pool: &sqlx::SqlitePool,
    hosts: &[String],
    timeout: Duration,
) -> Vec<InventoryEntry> {
    let mut cache = HashMap::new();
    for host in hosts {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT sessions FROM fleet_inventory_cache WHERE host_id = ?")
                .bind(host)
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
        if let Some(sessions) =
            row.and_then(|(text,)| serde_json::from_str::<Vec<Value>>(&text).ok())
        {
            cache.insert(host.clone(), sessions);
        }
    }
    let entries = list_hosts(control, hosts, timeout, &mut cache).await;
    for entry in entries.iter().filter(|entry| entry.error.is_none()) {
        let _ = sqlx::query(
            "INSERT INTO fleet_inventory_cache (host_id, sessions, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(host_id) DO UPDATE SET sessions = excluded.sessions, updated_at = excluded.updated_at",
        )
        .bind(&entry.host_id)
        .bind(Value::Array(entry.sessions.clone()).to_string())
        .bind(crate::storage::now_rfc3339())
        .execute(pool)
        .await;
    }
    entries
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

/// Merges per-machine agent-control lists into the sessions clients render.
///
/// `labels` maps a host id to its display name (the host id itself when absent). A machine that
/// did not answer (`error` set, or a hung host served from cache) yields its last known
/// sessions with `stale: true`; a machine that never answered yields none. `stateSince` and
/// `screenChangedAt` are optional on the wire (older daemons omit them) and map to `None`.
pub fn merge_sessions(
    labels: &HashMap<String, String>,
    entries: &[InventoryEntry],
    now_ms: i64,
) -> Vec<FleetSession> {
    entries
        .iter()
        .flat_map(|entry| {
            let label = labels
                .get(&entry.host_id)
                .map_or(entry.host_id.as_str(), String::as_str);
            let stale = entry.stale || entry.error.is_some();
            entry.sessions.iter().filter_map(move |value| {
                fleet_session(&entry.host_id, label, value, stale, now_ms)
            })
        })
        .collect()
}

fn fleet_session(
    host_id: &str,
    host_label: &str,
    value: &Value,
    stale: bool,
    now_ms: i64,
) -> Option<FleetSession> {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    let millis = |key: &str| value.get(key).and_then(Value::as_i64);
    let backend_id = text("sessionId")?;
    let cwd = text("cwd").unwrap_or_default();
    let title = text("title")
        .map(str::to_owned)
        .or_else(|| {
            cwd.rsplit(['/', '\\'])
                .find(|part| !part.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| backend_id.to_owned());
    let state = text("state")
        .filter(|state| matches!(*state, "working" | "blocked" | "idle"))
        .unwrap_or("unknown");
    Some(FleetSession {
        session_ref: SessionRef::new(host_id, backend_id),
        host_id: host_id.to_owned(),
        host_label: host_label.to_owned(),
        title,
        cwd: cwd.to_owned(),
        agent: text("agent").unwrap_or_default().to_owned(),
        state: state.to_owned(),
        state_source: text("stateSource").unwrap_or("none").to_owned(),
        state_since_ms: millis("stateSince"),
        screen_changed_at_ms: millis("screenChangedAt"),
        pending_prompt: value
            .get("pending")
            .and_then(|pending| pending_prompt(pending, text("screenRevision"))),
        last_assistant_line: text("lastAssistantLine").map(str::to_owned),
        // The daemon's own read time, so a cached (stale) entry keeps its real age; an older
        // daemon that omits it falls back to the merge time.
        updated_at_ms: millis("observedAt").unwrap_or(now_ms),
        stale,
        health: value
            .get("health")
            .and_then(|health| serde_json::from_value::<HealthKind>(health.clone()).ok())
            .filter(|health| matches!(health, HealthKind::Failed { .. })),
        attention: value
            .get("attention")
            .and_then(|attention| serde_json::from_value::<Attention>(attention.clone()).ok()),
        reason: text("reason").map(str::to_owned),
    })
}

/// Sets who handles `session` from the hub's own state and judges its group again. A live
/// handling entry for the session with no pending decision for it makes the supervisor the
/// owner of the next step; a pending decision hands it back to the owner. A session with no
/// attention (an older peer) is left as it is.
pub fn overlay_attention(session: &mut FleetSession, handling: &[ActionEntry], decisions: &[Escalation]) {
    let Some(attention) = session.attention.as_mut() else { return };
    let ours = |session_ref: &SessionRef| session_ref.as_str() == session.session_ref.as_str();
    let decided = decisions.iter().any(|decision| ours(&decision.session_ref));
    let handled = handling
        .iter()
        .filter(|entry| entry.live)
        .any(|entry| entry.session_ref.as_ref().is_some_and(ours));
    if handled && !decided {
        attention.axes.ownership = AttentionOwnership::Supervisor;
    } else if attention.axes.ownership == AttentionOwnership::Supervisor {
        attention.axes.ownership = AttentionOwnership::Owner;
    }
    attention.group = attention_group(&attention.axes);
}

/// [`overlay_attention`] for one live session frame, over the stores in `sources`: the action
/// log since `since_ms` and the pending decisions. A store that fails counts as empty.
pub async fn overlay_attention_from_sources(
    mut session: FleetSession,
    sources: &PanelSources,
    since_ms: i64,
) -> FleetSession {
    let log = match &sources.log {
        Some(store) => store.list_today(since_ms).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let decisions = match &sources.escalations {
        Some(source) => source.pending().await.unwrap_or_default(),
        None => Vec::new(),
    };
    overlay_attention(&mut session, &log, &decisions);
    session
}

/// The daemon's `pending` card (a reference prompt) as the panel's prompt.
pub fn pending_prompt(pending: &Value, screen_revision: Option<&str>) -> Option<PendingPrompt> {
    let text = |key: &str| {
        pending
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    let prompt_id = text("id")?.to_owned();
    let kind = match text("kind") {
        Some("approval" | "plan") => "approval",
        _ => "question",
    };
    Some(PendingPrompt {
        prompt_id,
        kind: kind.to_owned(),
        question: text("question").or(text("title")).unwrap_or_default().to_owned(),
        command: (kind == "approval")
            .then(|| text("body").map(str::to_owned))
            .flatten(),
        options: pending
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| option.get("label").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        screen_revision: screen_revision.map(str::to_owned),
    })
}

/// Decisions the supervisor escalated to the owner and that are still open. The concrete store
/// comes with the answer-on-behalf work; the panel only reads it.
#[async_trait]
pub trait EscalationSource: Send + Sync {
    async fn pending(&self) -> Result<Vec<Escalation>>;
}

/// The stores behind the panel. Every one is optional: an unconfigured store is an empty section.
#[derive(Clone, Default)]
pub struct PanelSources {
    pub log: Option<Arc<dyn ActionLog>>,
    pub rules: Option<Arc<dyn RuleStore>>,
    pub escalations: Option<Arc<dyn EscalationSource>>,
}

/// Assembles the panel snapshot. `log` is every action entry updated since `since_ms` and
/// `handling` its live subset. A store that fails is logged and shown empty, never an error for
/// the whole panel. With no hub running (`hub_running == false`) the panel asks for setup.
pub async fn assemble_panel(
    sources: &PanelSources,
    sessions: Vec<FleetSession>,
    paused: bool,
    hub_running: bool,
    since_ms: i64,
) -> PanelSnapshot {
    let log: Vec<ActionEntry> = match &sources.log {
        Some(store) => store.list_today(since_ms).await.unwrap_or_else(|error| {
            tracing::warn!(%error, "panel: action log unavailable");
            Vec::new()
        }),
        None => Vec::new(),
    };
    let rules: Vec<StandingRule> = match &sources.rules {
        Some(store) => store.list().await.unwrap_or_else(|error| {
            tracing::warn!(%error, "panel: rule store unavailable");
            Vec::new()
        }),
        None => Vec::new(),
    };
    let decisions: Vec<Escalation> = match &sources.escalations {
        Some(source) => source.pending().await.unwrap_or_else(|error| {
            tracing::warn!(%error, "panel: escalation source unavailable");
            Vec::new()
        }),
        None => Vec::new(),
    };
    let mut sessions = sessions;
    for session in &mut sessions {
        overlay_attention(session, &log, &decisions);
    }
    PanelSnapshot {
        decisions,
        handling: log.iter().filter(|entry| entry.live).cloned().collect(),
        sessions,
        log,
        rules,
        paused,
        setup_required: !hub_running,
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

    #[tokio::test]
    async fn persisted_list_shows_a_hung_host_with_its_last_sessions_across_callers() {
        let pool = crate::storage::memory().await.unwrap();
        let control = Arc::new(RecordingControl::default());
        *control.result.lock().unwrap() = json!({"sessions": [{"sessionId": "remote-pane"}]});
        let hosts = vec!["h1".to_string()];
        let first =
            list_persisted(control.as_ref(), &pool, &hosts, Duration::from_millis(30)).await;
        assert!(!first[0].stale);

        // A new caller with no in-memory state, and the host now hangs.
        control.hang.store(true, Ordering::SeqCst);
        let hung = list_persisted(control.as_ref(), &pool, &hosts, Duration::from_millis(30)).await;
        assert!(hung[0].stale);
        assert_eq!(hung[0].sessions[0]["sessionId"], "remote-pane");
    }

    fn host_entry(host: &str, sessions: Vec<Value>, error: Option<&str>, stale: bool) -> InventoryEntry {
        InventoryEntry {
            host_id: host.into(),
            sessions,
            error: error.map(str::to_owned),
            stale,
        }
    }

    #[test]
    fn merge_builds_refs_and_marks_machines_that_did_not_answer_stale() {
        let labels = HashMap::from([("mac".to_string(), "This Mac".to_string())]);
        let entries = vec![
            host_entry(
                "mac",
                vec![json!({
                    "sessionId": "s1", "cwd": "/work/ferryx", "agent": "claude",
                    "state": "blocked", "stateSource": "provider",
                    "stateSince": 1000, "screenChangedAt": 2000,
                    "screenRevision": "r7", "lastAssistantLine": "Done with the migration",
                    "pending": {"id": "p1", "kind": "approval", "question": "Run it?",
                        "body": "sqlx migrate run", "options": [{"label": "Allow"}, {"label": "Deny"}]},
                })],
                None,
                false,
            ),
            host_entry(
                "win",
                vec![json!({"sessionId": "w1", "state": "working", "stateSource": "screen"})],
                None,
                false,
            ),
            host_entry(
                "linux",
                vec![json!({"sessionId": "l1", "state": "idle"})],
                Some("host linux timed out"),
                true,
            ),
            host_entry("down", Vec::new(), Some("down unreachable"), false),
        ];
        let merged = merge_sessions(&labels, &entries, 42);
        assert_eq!(merged.len(), 3, "an offline machine with no cache has no sessions");

        let mac = &merged[0];
        assert_eq!(mac.session_ref, SessionRef::new("mac", "s1"));
        assert_eq!(mac.host_label, "This Mac");
        assert_eq!(mac.title, "ferryx");
        assert_eq!(mac.state, "blocked");
        assert_eq!(mac.state_since_ms, Some(1000));
        assert_eq!(mac.screen_changed_at_ms, Some(2000));
        assert_eq!(mac.last_assistant_line.as_deref(), Some("Done with the migration"));
        assert_eq!(mac.updated_at_ms, 42);
        assert!(!mac.stale);
        let prompt = mac.pending_prompt.as_ref().expect("pending prompt");
        assert_eq!(prompt.prompt_id, "p1");
        assert_eq!(prompt.kind, "approval");
        assert_eq!(prompt.command.as_deref(), Some("sqlx migrate run"));
        assert_eq!(prompt.options, vec!["Allow", "Deny"]);
        assert_eq!(prompt.screen_revision.as_deref(), Some("r7"));

        let win = &merged[1];
        assert_eq!(win.session_ref, SessionRef::new("win", "w1"));
        assert_eq!(win.host_label, "win", "an unlabeled host shows its id");
        assert_eq!(win.state_since_ms, None, "an older daemon omits the timestamps");
        assert_eq!(win.screen_changed_at_ms, None);
        assert_eq!(win.last_assistant_line, None);
        assert!(!win.stale);

        let linux = &merged[2];
        assert_eq!(linux.session_ref, SessionRef::new("linux", "l1"));
        assert!(linux.stale, "a host that timed out is served from cache, stale");
    }

    #[test]
    fn a_session_keeps_the_time_its_daemon_read_it_not_the_merge_time() {
        let entries = vec![host_entry(
            "mac",
            vec![
                json!({"sessionId": "fresh", "observedAt": 5000, "stateSince": 1000}),
                json!({"sessionId": "old-daemon"}),
            ],
            Some("host mac timed out"),
            true,
        )];
        let merged = merge_sessions(&HashMap::new(), &entries, 99_000);
        assert_eq!(merged[0].updated_at_ms, 5000, "a cached entry keeps its real age");
        assert_eq!(merged[0].state_since_ms, Some(1000));
        assert!(merged[0].stale);
        assert_eq!(merged[1].updated_at_ms, 99_000, "no observedAt falls back to the merge time");
    }

    struct FakeLog(Vec<ActionEntry>);

    #[async_trait]
    impl ActionLog for FakeLog {
        async fn upsert_incident(&self, entry: ActionEntry) -> Result<ActionEntry> {
            Ok(entry)
        }
        async fn list_today(&self, _since_ms: i64) -> Result<Vec<ActionEntry>> {
            Ok(self.0.clone())
        }
        async fn mark_undone(&self, _id: i64, _now_ms: i64) -> Result<()> {
            Ok(())
        }
    }

    struct FakeRules;

    #[async_trait]
    impl RuleStore for FakeRules {
        async fn list(&self) -> Result<Vec<StandingRule>> {
            Ok(crate::supervisor::panel_contract::default_rules())
        }
        async fn upsert(&self, rule: StandingRule) -> Result<StandingRule> {
            Ok(rule)
        }
    }

    struct FakeEscalations(Vec<Escalation>);

    #[async_trait]
    impl EscalationSource for FakeEscalations {
        async fn pending(&self) -> Result<Vec<Escalation>> {
            Ok(self.0.clone())
        }
    }

    fn action(id: i64, live: bool) -> ActionEntry {
        use crate::supervisor::panel_contract::ActionKind;
        ActionEntry {
            id,
            incident_key: format!("incident-{id}"),
            session_ref: None,
            kind: ActionKind::Retried,
            summary: "Retried".into(),
            basis: "default rule".into(),
            evidence: Vec::new(),
            undo: None,
            live,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    fn one_session() -> Vec<FleetSession> {
        merge_sessions(
            &HashMap::new(),
            &[host_entry("mac", vec![json!({"sessionId": "s1"})], None, false)],
            1,
        )
    }

    #[tokio::test]
    async fn panel_assembles_every_section_and_asks_for_setup_only_without_a_hub() {
        let escalation = Escalation {
            session_ref: SessionRef::new("mac", "s1"),
            prompt: PendingPrompt {
                prompt_id: "p1".into(),
                kind: "approval".into(),
                question: "Run the migration?".into(),
                command: None,
                options: vec!["Allow".into(), "Deny".into()],
                screen_revision: None,
            },
            recommended_index: 1,
            reason: "non-local database".into(),
            created_at_ms: 3,
        };
        let sources = PanelSources {
            log: Some(Arc::new(FakeLog(vec![action(1, true), action(2, false)]))),
            rules: Some(Arc::new(FakeRules)),
            escalations: Some(Arc::new(FakeEscalations(vec![escalation.clone()]))),
        };
        let panel = assemble_panel(&sources, one_session(), true, true, 0).await;
        assert_eq!(panel.decisions, vec![escalation]);
        assert_eq!(panel.sessions.len(), 1);
        assert_eq!(panel.log.len(), 2);
        assert_eq!(panel.handling.iter().map(|entry| entry.id).collect::<Vec<_>>(), vec![1]);
        assert_eq!(panel.rules.len(), 4);
        assert!(panel.paused);
        assert!(!panel.setup_required);

        let no_hub = assemble_panel(&sources, Vec::new(), false, false, 0).await;
        assert!(no_hub.setup_required, "no hub running asks for setup");
        assert!(!no_hub.paused);

        let unconfigured = assemble_panel(&PanelSources::default(), one_session(), false, true, 0).await;
        assert!(unconfigured.decisions.is_empty());
        assert!(unconfigured.handling.is_empty());
        assert!(unconfigured.log.is_empty());
        assert!(unconfigured.rules.is_empty());
        assert_eq!(unconfigured.sessions.len(), 1);
    }

    fn attended(session_id: &str, demand: &str) -> Value {
        json!({
            "sessionId": session_id, "state": "blocked",
            "attention": {
                "axes": {"demand": demand, "activity": "stopped", "completion": "none",
                    "read": "unread", "ownership": "owner"},
                "group": "needsYou"
            }
        })
    }

    #[test]
    fn an_old_peer_without_attention_still_parses_and_a_new_one_carries_it() {
        use crate::supervisor::panel_contract::{AttentionDemand, AttentionGroup};
        let entries = vec![host_entry(
            "mac",
            vec![json!({"sessionId": "old", "state": "idle"}), attended("new", "question")],
            None,
            false,
        )];
        let merged = merge_sessions(&HashMap::new(), &entries, 1);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].attention, None, "an old peer has no attention");
        assert_eq!(merged[0].state, "idle");
        let attention = merged[1].attention.expect("a new peer's attention");
        assert_eq!(attention.axes.demand, AttentionDemand::Question);
        assert_eq!(attention.group, AttentionGroup::NeedsYou);
    }

    fn handled(session: &str, live: bool) -> ActionEntry {
        ActionEntry { session_ref: Some(SessionRef::new("mac", session)), ..action(9, live) }
    }

    #[tokio::test]
    async fn a_handled_session_is_seen_until_it_is_escalated_back_to_the_owner() {
        use crate::supervisor::panel_contract::{AttentionGroup, AttentionOwnership};
        let sessions = || {
            merge_sessions(
                &HashMap::new(),
                &[host_entry("mac", vec![attended("s1", "error"), attended("s2", "error")], None, false)],
                1,
            )
        };
        let handling = vec![handled("s1", true), handled("s2", false)];
        let sources = PanelSources {
            log: Some(Arc::new(FakeLog(handling.clone()))),
            ..PanelSources::default()
        };
        let panel = assemble_panel(&sources, sessions(), false, true, 0).await;
        let s1 = panel.sessions[0].attention.unwrap();
        assert_eq!(s1.axes.ownership, AttentionOwnership::Supervisor);
        assert_eq!(s1.group, AttentionGroup::Seen);
        let s2 = panel.sessions[1].attention.unwrap();
        assert_eq!(s2.group, AttentionGroup::NeedsYou, "a finished handling entry hands nothing over");

        let escalation = Escalation {
            session_ref: SessionRef::new("mac", "s1"),
            prompt: PendingPrompt {
                prompt_id: "p".into(),
                kind: "question".into(),
                question: "?".into(),
                command: None,
                options: Vec::new(),
                screen_revision: None,
            },
            recommended_index: 0,
            reason: "r".into(),
            created_at_ms: 1,
        };
        let escalated = PanelSources {
            escalations: Some(Arc::new(FakeEscalations(vec![escalation]))),
            ..sources.clone()
        };
        let frame = overlay_attention_from_sources(sessions().remove(0), &escalated, 0).await;
        let s1 = frame.attention.unwrap();
        assert_eq!(s1.axes.ownership, AttentionOwnership::Owner, "an escalation returns it to the owner");
        assert_eq!(s1.group, AttentionGroup::NeedsYou);
        let frame = overlay_attention_from_sources(sessions().remove(0), &sources, 0).await;
        assert_eq!(frame.attention.unwrap().group, AttentionGroup::Seen, "a live frame uses the same rule");
    }
}
