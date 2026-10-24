//! Who may drive agent control, and the audit line each remote operation leaves.
//!
//! The grant is the existing machine-scope Control permission. There is no new scope variant.
//! `enabled` is the machine's "allow agent control" switch on top of that grant. It is off on a
//! fresh install and is turned on from Settings; one device can still be excluded while it is on.
//! The choice is persisted to `<data>/remote/agent-control.json` (0600). An install upgraded from
//! a build where agent control was always on keeps it on once, when a device already holds a
//! machine Control grant. The workspace operation journal is not reused. Its records become `OutcomeUnknown`
//! workspace operations on restart, which would mix supervisor keystrokes into worktree recovery.
//! This log keeps the device id, the operation and the outcome beside that journal instead.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::remote::auth::{DeviceAccessScope, DeviceInfo, DevicePermission};
use crate::scoped_contracts::{ScopeError, ScopeErrorCode};

use super::AgentControlOp;

/// The largest JSON body an agent-control mutation route accepts.
pub const AGENT_CONTROL_HTTP_BODY_MAX: usize = 64 * 1024;

/// A person who typed in the pane inside this window blocks supervisor input.
pub const HUMAN_TYPING_WINDOW_MS: u64 = 3_000;

/// `wait` never blocks longer than this, whatever the caller asked.
pub const AGENT_CONTROL_TIMEOUT_MAX_MS: u64 = 300_000;

/// The file, beside the remote auth store, that holds the agent-control setting.
pub const AGENT_CONTROL_FILE: &str = "agent-control.json";

/// The warning code logged when the setting file cannot be read. Agent control then stays off.
pub const AGENT_CONTROL_SETTINGS_UNREADABLE: &str = "AGENT_CONTROL_SETTINGS_UNREADABLE";

/// How many audit records the Settings card shows.
pub const AGENT_CONTROL_SETTINGS_AUDIT_LIMIT: usize = 50;

/// What `agent-control.json` holds. `enabled` is required: a file without it is unreadable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAgentControl {
    pub enabled: bool,
    #[serde(default)]
    pub excluded_device_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restricted_device_ids: Option<Vec<String>>,
}

/// The Settings card's view: the switch, the excluded devices and the newest audit records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlSettings {
    pub enabled: bool,
    pub excluded_device_ids: Vec<String>,
    /// Newest first, at most [`AGENT_CONTROL_SETTINGS_AUDIT_LIMIT`].
    pub audit: Vec<AuditRecord>,
}

#[derive(Debug)]
pub enum StoredAgentControl {
    Missing,
    Loaded(PersistedAgentControl),
    Unreadable(String),
}

pub fn read_persisted(path: &Path) -> StoredAgentControl {
    match std::fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoredAgentControl::Missing,
        Err(error) => StoredAgentControl::Unreadable(error.to_string()),
        Ok(bytes) => match serde_json::from_slice::<PersistedAgentControl>(&bytes) {
            Ok(saved) => StoredAgentControl::Loaded(saved),
            Err(error) => StoredAgentControl::Unreadable(error.to_string()),
        },
    }
}

#[derive(Debug)]
pub struct AgentControlPolicy {
    enabled: AtomicBool,
    /// `None` allows every device that passes the scope and permission checks.
    /// `Some` allows only those device ids.
    restricted: RwLock<Option<HashSet<String>>>,
    /// Devices refused even while agent control is on.
    excluded: RwLock<HashSet<String>>,
}

impl Default for AgentControlPolicy {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            restricted: RwLock::new(None),
            excluded: RwLock::new(HashSet::new()),
        }
    }
}

impl AgentControlPolicy {
    /// The policy saved at `path`. With no path the policy is the fresh default, off.
    ///
    /// A missing file is a fresh install unless `has_control_grant` reports a device that already
    /// holds a machine Control grant: that install ran a build where agent control was always on,
    /// so it stays on and `enabled: true` is written once. An unreadable file keeps control off.
    pub fn load(path: Option<&Path>, has_control_grant: impl FnOnce() -> bool) -> Self {
        let policy = Self::default();
        let Some(path) = path else {
            return policy;
        };
        match read_persisted(path) {
            StoredAgentControl::Loaded(saved) => policy.apply(&saved),
            StoredAgentControl::Missing => {
                if has_control_grant() {
                    let kept = PersistedAgentControl {
                        enabled: true,
                        ..PersistedAgentControl::default()
                    };
                    policy.apply(&kept);
                    tracing::info!(
                        path = %path.display(),
                        "agent control kept on for existing Control grant"
                    );
                    if let Err(error) = crate::remote::auth::write_private_json(path, &kept) {
                        tracing::warn!(
                            path = %path.display(),
                            %error,
                            "could not save the kept agent control setting; it is decided again on the next start"
                        );
                    }
                }
            }
            StoredAgentControl::Unreadable(error) => {
                tracing::warn!(
                    error_code = AGENT_CONTROL_SETTINGS_UNREADABLE,
                    path = %path.display(),
                    %error,
                    "agent control settings are unreadable; agent control stays off"
                );
            }
        }
        policy
    }

    pub fn permits(&self, device_id: &str) -> bool {
        if !self.enabled.load(Ordering::Acquire) {
            return false;
        }
        if self.excluded.read().contains(device_id) {
            return false;
        }
        match self.restricted.read().as_ref() {
            None => true,
            Some(allowed) => allowed.contains(device_id),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn restrict_to<I, S>(&self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        *self.restricted.write() = Some(ids.into_iter().map(Into::into).collect());
    }

    /// Replace the excluded devices.
    pub fn exclude<I, S>(&self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        *self.excluded.write() = ids.into_iter().map(Into::into).collect();
    }

    /// What would be written to `agent-control.json` now. Device ids are sorted.
    pub fn snapshot(&self) -> PersistedAgentControl {
        let mut excluded: Vec<String> = self.excluded.read().iter().cloned().collect();
        excluded.sort();
        let restricted = self.restricted.read().as_ref().map(|ids| {
            let mut ids: Vec<String> = ids.iter().cloned().collect();
            ids.sort();
            ids
        });
        PersistedAgentControl {
            enabled: self.enabled(),
            excluded_device_ids: excluded,
            restricted_device_ids: restricted,
        }
    }

    pub fn apply(&self, saved: &PersistedAgentControl) {
        *self.restricted.write() = saved
            .restricted_device_ids
            .as_ref()
            .map(|ids| ids.iter().cloned().collect());
        self.exclude(saved.excluded_device_ids.iter().cloned());
        self.set_enabled(saved.enabled);
    }
}

/// Whether this device may call agent control right now.
///
/// Mirror is refused for reads and mutations. View is refused for mutations. A device whose
/// agent-control capability is off is refused for both. The caller re-runs this on every write
/// with a freshly validated token, so a revoke between the two checks is a refusal.
pub fn authorize_agent_control(
    device: &DeviceInfo,
    mutation: bool,
    agent_control_allowed: bool,
) -> Result<(), ScopeError> {
    if device.access_scope != DeviceAccessScope::Machine {
        return Err(refusal(
            ScopeErrorCode::Forbidden,
            "a mirror device cannot use agent control",
        ));
    }
    if mutation && device.permission != DevicePermission::Control {
        return Err(refusal(
            ScopeErrorCode::Forbidden,
            "a view-only device cannot type into an agent pane",
        ));
    }
    if !agent_control_allowed {
        return Err(refusal(
            ScopeErrorCode::Forbidden,
            "this device is not allowed to control agents",
        ));
    }
    Ok(())
}

pub fn refusal(code: ScopeErrorCode, message: impl Into<String>) -> ScopeError {
    ScopeError {
        code,
        message: message.into(),
        retryable: false,
        details: serde_json::Value::Null,
    }
}

/// Clamp a wait budget. Zero is rejected. Anything above five minutes becomes five minutes.
pub fn clamp_wait_timeout(timeout_ms: u64) -> Result<u64, ScopeError> {
    if timeout_ms == 0 {
        return Err(refusal(
            ScopeErrorCode::InvalidRequest,
            "timeout must be at least 1ms and is clamped to 300000ms",
        ));
    }
    Ok(timeout_ms.min(AGENT_CONTROL_TIMEOUT_MAX_MS))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub device_id: String,
    pub op: String,
    pub session_id: Option<String>,
    pub request_id: Option<String>,
    pub code: String,
    /// Wall-clock milliseconds when the operation finished.
    pub at_ms: u64,
    /// What the operation put into the pane, shortened: the sent text or the chosen answer.
    pub text: Option<String>,
}

/// The longest excerpt of supervisor input an audit line keeps.
pub const AUDIT_TEXT_MAX_CHARS: usize = 160;

impl AuditRecord {
    pub fn for_op(device_id: &str, op: &AgentControlOp, code: impl Into<String>) -> Self {
        Self {
            device_id: device_id.to_string(),
            op: audit_op_name(op).to_string(),
            session_id: audit_session(op),
            request_id: audit_request(op),
            code: code.into(),
            at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as u64),
            text: audit_text(op),
        }
    }

    /// Send, answer and stop change a pane; the rest only look at it.
    pub fn is_input(&self) -> bool {
        matches!(self.op.as_str(), "send" | "answer" | "stop")
    }
}

#[derive(Debug, Default)]
pub struct AgentControlAudit {
    records: Mutex<VecDeque<AuditRecord>>,
}

impl AgentControlAudit {
    pub fn record(&self, record: AuditRecord) {
        let mut records = self.records.lock();
        records.push_back(record);
        while records.len() > 1024 {
            records.pop_front();
        }
    }

    pub fn recent(&self) -> Vec<AuditRecord> {
        self.records.lock().iter().cloned().collect()
    }

    /// The newest `limit` records, newest first.
    pub fn latest(&self, limit: usize) -> Vec<AuditRecord> {
        self.records.lock().iter().rev().take(limit).cloned().collect()
    }
}

pub fn audit_op_name(op: &AgentControlOp) -> &'static str {
    match op {
        AgentControlOp::List => "list",
        AgentControlOp::Read { .. } => "read",
        AgentControlOp::Send { .. } => "send",
        AgentControlOp::Answer { .. } => "answer",
        AgentControlOp::Stop { .. } => "stop",
        AgentControlOp::Wait { .. } => "wait",
        AgentControlOp::Events { .. } => "events",
    }
}

pub fn audit_session(op: &AgentControlOp) -> Option<String> {
    match op {
        AgentControlOp::Read { session_id }
        | AgentControlOp::Send { session_id, .. }
        | AgentControlOp::Answer { session_id, .. }
        | AgentControlOp::Stop { session_id, .. }
        | AgentControlOp::Wait { session_id, .. } => Some(session_id.clone()),
        AgentControlOp::List | AgentControlOp::Events { .. } => None,
    }
}

pub fn audit_request(op: &AgentControlOp) -> Option<String> {
    match op {
        AgentControlOp::Send { request_id, .. }
        | AgentControlOp::Answer { request_id, .. }
        | AgentControlOp::Stop { request_id, .. } => request_id.clone(),
        _ => None,
    }
}

/// The input an operation types, shortened to one line of at most AUDIT_TEXT_MAX_CHARS.
pub fn audit_text(op: &AgentControlOp) -> Option<String> {
    let raw = match op {
        AgentControlOp::Send { text, .. } => text.clone(),
        AgentControlOp::Answer { answer, .. } => match (&answer.custom_text, &answer.option_indices, answer.option_index) {
            (Some(custom), _, _) if !custom.trim().is_empty() => custom.clone(),
            (_, Some(indices), _) if !indices.is_empty() => format!(
                "options {}",
                indices.iter().map(|index| (index + 1).to_string()).collect::<Vec<_>>().join(", ")
            ),
            (_, _, Some(index)) => format!("option {}", index + 1),
            _ => return None,
        },
        AgentControlOp::Stop { .. } => "Ctrl-C".to_string(),
        _ => return None,
    };
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= AUDIT_TEXT_MAX_CHARS {
        return Some(line);
    }
    let mut short: String = line.chars().take(AUDIT_TEXT_MAX_CHARS - 1).collect();
    short.push('…');
    Some(short)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::AuthManager;

    fn device(permission: DevicePermission, scope: DeviceAccessScope) -> DeviceInfo {
        DeviceInfo {
            id: "dev-1".into(),
            name: "phone".into(),
            permission,
            access_scope: scope,
            created_at: 0,
            last_seen_at: 0,
            revoked: false,
            installation_id: None,
            attach_public_key: None,
        }
    }

    #[test]
    fn mirror_is_refused_and_view_cannot_mutate_and_a_disabled_capability_is_refused() {
        let mirror = device(DevicePermission::Control, DeviceAccessScope::Mirror);
        assert_eq!(
            authorize_agent_control(&mirror, false, true)
                .unwrap_err()
                .code,
            ScopeErrorCode::Forbidden
        );
        let view = device(DevicePermission::View, DeviceAccessScope::Machine);
        assert!(authorize_agent_control(&view, false, true).is_ok());
        assert_eq!(
            authorize_agent_control(&view, true, true).unwrap_err().code,
            ScopeErrorCode::Forbidden
        );
        let control = device(DevicePermission::Control, DeviceAccessScope::Machine);
        assert_eq!(
            authorize_agent_control(&control, true, false)
                .unwrap_err()
                .code,
            ScopeErrorCode::Forbidden
        );
        assert!(authorize_agent_control(&control, true, true).is_ok());
    }

    #[test]
    fn a_revoked_token_fails_the_next_authorization_check() {
        let auth = AuthManager::new();
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .expect("machine control pin");
        let (token, device) = auth.exchange_pairing_code(&pin, "hub").expect("exchange");
        let check = || {
            let device = auth
                .validate_token(&token)
                .map_err(|_| refusal(ScopeErrorCode::Unauthorized, "revoked"))?;
            authorize_agent_control(&device, true, true)
        };
        assert!(check().is_ok());
        auth.revoke_device(&device.id).expect("revoke");
        assert_eq!(check().unwrap_err().code, ScopeErrorCode::Unauthorized);
    }

    #[test]
    fn the_policy_can_exclude_one_device_and_timeouts_clamp_at_five_minutes() {
        let policy = AgentControlPolicy::default();
        assert!(!policy.permits("dev-1"), "agent control is off until enabled");
        policy.set_enabled(true);
        assert!(policy.permits("dev-1"));
        policy.exclude(["dev-1"]);
        assert!(!policy.permits("dev-1"));
        assert!(policy.permits("dev-2"));
        policy.exclude(Vec::<String>::new());
        policy.restrict_to(["other"]);
        assert!(!policy.permits("dev-1"));
        policy.set_enabled(false);
        policy.restrict_to(Vec::<String>::new());
        assert!(!policy.permits("other"));
        assert_eq!(
            clamp_wait_timeout(600_000).unwrap(),
            AGENT_CONTROL_TIMEOUT_MAX_MS
        );
        assert_eq!(
            clamp_wait_timeout(0).unwrap_err().code,
            ScopeErrorCode::InvalidRequest
        );
    }

    #[test]
    fn agent_control_is_off_until_enabled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(AGENT_CONTROL_FILE);
        let fresh = AgentControlPolicy::load(Some(&path), || false);
        assert!(!fresh.enabled());
        assert!(!fresh.permits("dev-1"));
        assert!(!path.exists(), "a fresh install writes nothing until the owner chooses");
        assert!(!AgentControlPolicy::load(None, || true).permits("dev-1"));
        fresh.set_enabled(true);
        assert!(fresh.permits("dev-1"));
    }

    #[test]
    fn a_corrupt_agent_control_file_keeps_agent_control_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(AGENT_CONTROL_FILE);
        std::fs::write(&path, b"{\"enabled\": tru").expect("write corrupt file");
        let policy = AgentControlPolicy::load(Some(&path), || true);
        assert!(!policy.permits("dev-1"), "an unreadable file never turns control on");
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"enabled\": tru", "the file is left for the owner");
        std::fs::write(&path, b"{\"excludedDeviceIds\": []}").expect("write file without enabled");
        assert!(!AgentControlPolicy::load(Some(&path), || true).permits("dev-1"));
    }

    #[test]
    fn audit_lines_keep_a_short_excerpt_of_what_was_typed() {
        let send = AgentControlOp::Send {
            session_id: "s".into(),
            text: "run   the\ntests\n".into(),
            request_id: None,
        };
        let record = AuditRecord::for_op("hub", &send, "OK");
        assert_eq!(record.text.as_deref(), Some("run the tests"));
        assert!(record.is_input());
        assert!(record.at_ms > 0);

        let long = AgentControlOp::Send {
            session_id: "s".into(),
            text: "x".repeat(400),
            request_id: None,
        };
        let excerpt = audit_text(&long).unwrap();
        assert_eq!(excerpt.chars().count(), AUDIT_TEXT_MAX_CHARS);
        assert!(excerpt.ends_with('…'));

        let answer = |answer: crate::remote::reference_chat::types::ReferencePromptAnswer| AgentControlOp::Answer {
            session_id: "s".into(),
            prompt_id: "p".into(),
            screen_revision: "r".into(),
            answer,
            request_id: None,
        };
        assert_eq!(
            audit_text(&answer(crate::remote::reference_chat::types::ReferencePromptAnswer {
                option_index: Some(0),
                ..Default::default()
            }))
            .as_deref(),
            Some("option 1")
        );
        assert_eq!(
            audit_text(&answer(crate::remote::reference_chat::types::ReferencePromptAnswer {
                custom_text: Some("use the cache".into()),
                ..Default::default()
            }))
            .as_deref(),
            Some("use the cache")
        );
        let read = AuditRecord::for_op("hub", &AgentControlOp::Read { session_id: "s".into() }, "OK");
        assert_eq!(read.text, None);
        assert!(!read.is_input());
    }
}
