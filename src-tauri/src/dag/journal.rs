use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DagJournalError {
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Wave references unknown node id: {node_id} in wave index {wave_index}")]
    UnknownWaveNode { node_id: String, wave_index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DagRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Paused,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DagNodeState {
    Pending,
    Scheduled,
    Blocked,
    Running,
    Completed,
    Failed,
    Skipped,
    Cancelled,
    Paused,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DagRoute {
    Category {
        category: String,
    },
    Agent {
        agent: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagNodeSnapshot {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub state: DagNodeState,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub attempt: usize,
    pub route: DagRoute,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<DagNodeError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagWave {
    pub index: usize,
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagBottleneck {
    pub node_id: String,
    pub blocked_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunCounts {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub running: usize,
}

pub type Counts = DagRunCounts;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunSnapshot {
    pub run_id: String,
    pub run_key: String,
    pub name: String,
    pub status: DagRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub amend_count: usize,
    pub nodes: Vec<DagNodeSnapshot>,
    pub edges: Vec<DagEdge>,
    pub waves: Vec<DagWave>,
    pub critical_path: Vec<String>,
    pub bottlenecks: Vec<DagBottleneck>,
    pub counts: DagRunCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagRunSummary {
    pub run_id: String,
    pub run_key: String,
    pub name: String,
    pub status: DagRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub amend_count: usize,
    pub counts: DagRunCounts,
}

impl From<&DagRunSnapshot> for DagRunSummary {
    fn from(s: &DagRunSnapshot) -> Self {
        Self {
            run_id: s.run_id.clone(),
            run_key: s.run_key.clone(),
            name: s.name.clone(),
            status: s.status,
            started_at: s.started_at.clone(),
            completed_at: s.completed_at.clone(),
            updated_at: s.updated_at.clone(),
            amend_count: s.amend_count,
            counts: s.counts,
        }
    }
}

impl From<DagRunSnapshot> for DagRunSummary {
    fn from(s: DagRunSnapshot) -> Self {
        Self::from(&s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DagJournalEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub seq: u64,
    #[serde(default, rename = "runId")]
    pub run_id: Option<String>,
    #[serde(default, rename = "nodeId")]
    pub node_id: Option<String>,
    #[serde(default)]
    pub from: Option<DagNodeState>,
    #[serde(default)]
    pub to: Option<DagNodeState>,
    #[serde(default)]
    pub at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCheckpoint {
    run_id: String,
    run_key: String,
    name: String,
    status: DagRunStatus,
    #[serde(default)]
    started_at: Option<String>,
    #[serde(default)]
    completed_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    amend_count: Option<usize>,
    #[serde(default)]
    amend_history: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    nodes: Vec<DagNodeSnapshot>,
    #[serde(default)]
    edges: Vec<DagEdge>,
    #[serde(default)]
    waves: Vec<DagWave>,
    #[serde(default)]
    critical_path: Vec<String>,
    #[serde(default)]
    bottlenecks: Vec<DagBottleneck>,
}

pub fn derive_counts(nodes: &[DagNodeSnapshot]) -> Counts {
    let mut counts = DagRunCounts {
        total: nodes.len(),
        ..Default::default()
    };
    for node in nodes {
        match node.state {
            DagNodeState::Completed => counts.completed += 1,
            DagNodeState::Failed => counts.failed += 1,
            DagNodeState::Cancelled => counts.cancelled += 1,
            DagNodeState::Skipped => counts.skipped += 1,
            DagNodeState::Running => counts.running += 1,
            DagNodeState::Pending
            | DagNodeState::Scheduled
            | DagNodeState::Blocked
            | DagNodeState::Paused
            | DagNodeState::Unknown => {}
        }
    }
    counts
}

pub fn parse_run_checkpoint(json: &str) -> Result<DagRunSnapshot, DagJournalError> {
    let raw: RawCheckpoint = serde_json::from_str(json)?;
    let node_ids: HashSet<&str> = raw.nodes.iter().map(|n| n.id.as_str()).collect();

    for wave in &raw.waves {
        for node_id in &wave.node_ids {
            if !node_ids.contains(node_id.as_str()) {
                return Err(DagJournalError::UnknownWaveNode {
                    node_id: node_id.clone(),
                    wave_index: wave.index,
                });
            }
        }
    }

    Ok(DagRunSnapshot {
        counts: derive_counts(&raw.nodes),
        amend_count: raw
            .amend_count
            .unwrap_or_else(|| raw.amend_history.as_ref().map_or(0, |h| h.len())),
        run_id: raw.run_id,
        run_key: raw.run_key,
        name: raw.name,
        status: raw.status,
        started_at: raw.started_at,
        completed_at: raw.completed_at,
        updated_at: raw.updated_at,
        nodes: raw.nodes,
        edges: raw.edges,
        waves: raw.waves,
        critical_path: raw.critical_path,
        bottlenecks: raw.bottlenecks,
    })
}

pub fn list_run_summaries(dir: &Path) -> Result<Vec<DagRunSummary>, DagJournalError> {
    let runs_dir = dir.join("runs");
    let target_dir = if runs_dir.is_dir() { &runs_dir } else { dir };
    let mut summaries = Vec::new();

    for entry in std::fs::read_dir(target_dir)? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            let content = std::fs::read_to_string(&path)?;
            summaries.push(DagRunSummary::from(parse_run_checkpoint(&content)?));
        }
    }

    summaries.sort_by(|a, b| {
        let a_time = a.updated_at.as_deref().unwrap_or("");
        let b_time = b.updated_at.as_deref().unwrap_or("");
        b_time.cmp(a_time)
    });
    Ok(summaries)
}

pub fn parse_events(jsonl: &str) -> Result<Vec<DagJournalEvent>, DagJournalError> {
    let mut events = Vec::new();
    for line in jsonl.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            events.push(serde_json::from_str(trimmed)?);
        }
    }
    Ok(events)
}
