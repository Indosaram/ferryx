//! Machine DTOs only. Admission and persistence are owned by later packets.
use crate::scoped_contracts::{Epoch, RunTarget};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MACHINE_JSON_MAX_BYTES: usize = 64 * 1024;
pub const DIRECTORY_JSON_MAX_BYTES: usize = 256 * 1024;
pub const CONTROL_JSON_MAX_BYTES: usize = 16 * 1024;


#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("PAYLOAD_TOO_LARGE")]
    PayloadTooLarge,
    #[error("INVALID_REQUEST: {0}")]
    InvalidRequest(#[from] serde_json::Error),
}

pub fn decode_json<T: serde::de::DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T, DecodeError> {
    if bytes.len() > limit { return Err(DecodeError::PayloadTooLarge); }
    Ok(serde_json::from_slice(bytes)?)
}

fn required<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if s.trim().is_empty() { return Err(serde::de::Error::custom("identity is required")); }
    Ok(s)
}

fn explicit_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where D: serde::Deserializer<'de>, T: Deserialize<'de> { Option::<T>::deserialize(d) }
fn nullable_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(d)?;
    if value.as_ref().is_some_and(|s| s.trim().is_empty()) {
        return Err(serde::de::Error::custom("nonempty string or null required"));
    }
    Ok(value)
}
fn text_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let value = Vec::<String>::deserialize(d)?;
    if value.iter().any(|s| s.trim().is_empty()) {
        return Err(serde::de::Error::custom("nonempty list entries required"));
    }
    Ok(value)
}
fn request_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = required(d)?;
    if s.len() != 36 || !s.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
    }) { return Err(serde::de::Error::custom("hyphenated UUID required")); }
    Ok(s)
}
fn optional_text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> { required(d).map(Some) }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderWire {
    key: crate::daemon::protocol::AgentProviderSessionKey,
    #[serde(deserialize_with = "required")]
    id: String,
    #[serde(default, deserialize_with = "optional_text")]
    transcript_path: Option<String>,
}
impl From<ProviderWire> for crate::daemon::protocol::AgentProviderSession {
    fn from(v: ProviderWire) -> Self { Self { key: v.key, id: v.id, transcript_path: v.transcript_path } }
}
fn provider<'de, D: serde::Deserializer<'de>>(d: D) -> Result<crate::daemon::protocol::AgentProviderSession, D::Error> {
    ProviderWire::deserialize(d).map(Into::into)
}
fn nullable_provider<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<crate::daemon::protocol::AgentProviderSession>, D::Error> {
    Option::<ProviderWire>::deserialize(d).map(|v| v.map(Into::into))
}
fn version_one<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let n = u32::deserialize(d)?;
    if n != 1 { return Err(serde::de::Error::custom("unsupported apiVersion")); }
    Ok(n)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalTarget {
    #[serde(deserialize_with = "required")]
    pub machine_id: String,
    pub daemon_epoch: Epoch,
    #[serde(deserialize_with = "required")]
    pub session_id: String,
}

pub fn desktop_workspace_id(host_key: &str, remote_workspace_id: &str) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(&["pairedDaemon", host_key, remote_workspace_id])?;
    Ok(format!("daemon:{:x}", Sha256::digest(bytes)))
}

pub fn proxy_backend_id(host_key: &str, target: &RemoteTerminalTarget) -> Result<String, serde_json::Error> {
    let epoch = target.daemon_epoch.0.to_string();
    let bytes = serde_json::to_vec(&[host_key, &target.machine_id, &epoch, &target.session_id])?;
    Ok(format!("daemon-session:{:x}", Sha256::digest(bytes)))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "PairedProjectWire")]
pub struct PairedProject {
    pub workspace_id: String,
    pub repo_root: String,
    pub target: RunTarget,
    pub remote_workspace_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairedProjectWire { workspace_id: String, repo_root: String, target: RunTarget, remote_workspace_id: String }
impl TryFrom<PairedProjectWire> for PairedProject {
    type Error = &'static str;
    fn try_from(v: PairedProjectWire) -> Result<Self, Self::Error> {
        if !matches!(v.target, RunTarget::PairedDaemon { .. }) || v.remote_workspace_id.trim().is_empty()
            || v.repo_root.trim().is_empty() || !v.workspace_id.starts_with("daemon:")
            || v.workspace_id.len() != 71 || !v.workspace_id[7..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err("invalid paired project");
        }
        Ok(Self { workspace_id: v.workspace_id, repo_root: v.repo_root, target: v.target, remote_workspace_id: v.remote_workspace_id })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeIdentity {
    #[serde(deserialize_with = "required")]
    pub ws_id: String,
    #[serde(deserialize_with = "required")]
    pub slug: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    #[serde(deserialize_with = "required")]
    pub repo_root: String,
    #[serde(deserialize_with = "nullable_text")]
    pub git_root: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub git_common_dir: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub git_remote: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub git_branch: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub git_head: Option<String>, pub availability: Availability, pub revision: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability { Ready, Missing, PermissionDenied, Invalid }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Completeness { Complete, Partial }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projects { pub revision: Epoch, pub completeness: Completeness, pub projects: Vec<Project>,
    #[serde(deserialize_with = "text_list")]
    pub unavailable_workspace_ids: Vec<String> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    #[serde(deserialize_with = "explicit_null")]
    pub identity: Option<WorktreeIdentity>,
    #[serde(deserialize_with = "required")]
    pub path: String, pub head: String,
    #[serde(deserialize_with = "nullable_text")]
    pub branch: Option<String>, pub bare: bool, pub detached: bool,
    #[serde(deserialize_with = "nullable_text")]
    pub locked: Option<String>,
    #[serde(deserialize_with = "nullable_text")]
    pub prunable: Option<String>, pub managed: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktrees { pub revision: Epoch, pub worktrees: Vec<Worktree> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub target: RemoteTerminalTarget,
    #[serde(deserialize_with = "required")]
    pub workspace_id: String,
    #[serde(deserialize_with = "explicit_null")]
    pub worktree: Option<WorktreeIdentity>,
    #[serde(deserialize_with = "required")]
    pub cwd: String, pub cols: u16, pub rows: u16, pub running: bool,
    #[serde(deserialize_with = "nullable_provider")]
    pub provider_session: Option<crate::daemon::protocol::AgentProviderSession>, pub start_sequence: Epoch, pub end_sequence: Epoch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sessions { pub revision: Epoch, pub completeness: Completeness, pub sessions: Vec<Session>,
    #[serde(deserialize_with = "text_list")]
    pub unavailable_workspace_ids: Vec<String> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    #[serde(deserialize_with = "required")]
    pub name: String,
    #[serde(deserialize_with = "required")]
    pub path: String, pub hidden: bool }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directories {
    #[serde(deserialize_with = "required")]
    pub path: String,
    #[serde(deserialize_with = "nullable_text")]
    pub parent_path: Option<String>,
    #[serde(deserialize_with = "required")]
    pub home_path: String, pub entries: Vec<DirectoryEntry>, pub truncated: bool
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessScope { Mirror, Machine }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Permission { View, Control }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform { Linux, Macos, Windows }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits { pub directory_entries: u32, pub terminal_sessions: u32 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(deserialize_with = "version_one")]
    pub api_version: u32,
    #[serde(deserialize_with = "required")]
    pub machine_id: String, pub daemon_epoch: Epoch, pub platform: Platform, pub access_scope: AccessScope, pub permission: Permission,
    #[serde(deserialize_with = "text_list")]
    pub capabilities: Vec<String>, pub limits: Limits
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineError {
    #[serde(deserialize_with = "required")]
    pub code: String,
    #[serde(deserialize_with = "required")]
    pub message: String, pub retryable: bool,
    #[serde(deserialize_with = "request_id")]
    pub request_id: String, pub details: serde_json::Map<String, serde_json::Value> }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope { pub error: MachineError }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGap { pub requested_after_sequence: Epoch, pub available_from_sequence: Epoch }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Attached {
    Attached { target: RemoteTerminalTarget, generation: Epoch, cols: u16, rows: u16, start_sequence: Epoch, end_sequence: Epoch,
        #[serde(deserialize_with = "explicit_null")]
        replay_gap: Option<ReplayGap> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteUploadChunkRequest {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    pub upload_id: String,
    pub file_name: String,
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteUploadChunkResult {
    pub remote_path: Option<String>,
    pub chunk_index: u32,
}

#[cfg(test)]
#[path = "machine_protocol_tests.rs"]
mod tests;

#[path = "machine_protocol_lifecycle.rs"]
mod lifecycle;
pub use lifecycle::*;
