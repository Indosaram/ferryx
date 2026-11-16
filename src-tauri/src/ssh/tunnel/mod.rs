//! Wire types and validation shared by tunnel storage, protocol and runtime ownership.
pub mod argv;
pub mod registry;
// The sidecar lock, process groups and start-time reads are unix-only; Windows
// refuses every tunnel command before reaching them, so this half is compiled out
// rather than stubbed.
#[cfg(unix)]
pub mod runtime_records;
use crate::ipc::{IpcError, IpcErrorCode};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::Ipv6Addr};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TunnelSpecId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TunnelRuntimeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelSpec {
    #[serde(default)]
    pub id: TunnelSpecId,
    pub label: String,
    pub local_port: Option<u16>,
    pub remote_host: String,
    pub remote_port: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelHostIdentity {
    pub hostname: String,
    pub username: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub jump_host: Option<String>,
    pub disabled: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelState {
    Stopped,
    Listening,
    Reconnecting,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelStopReason {
    UserRequested,
    HostChanged,
    HostDeleted,
    DaemonShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelFailureCode {
    PortInUse,
    HostUnreachable,
    AuthFailed,
    HostKeyRejected,
    ChannelFailed,
    UnsupportedPlatform,
    UnsupportedConfig,
    DaemonUnavailable,
    CredentialUnavailable,
    HostChanged,
}

/// Closed vocabulary for `unsupported_config`; no raw SSH directive crosses the wire.
pub const UNSUPPORTED_CONFIG_PIECES: [&str; 14] = [
    "config_alias",
    "config_path",
    "config_only_endpoint",
    "config_only_jump",
    "jump_identity",
    "jump_config",
    "jump_password",
    "identity_agent",
    "identity_file_token",
    "certificate_file",
    "match_block",
    "bad_option",
    "algorithm_mismatch",
    "unsupported_directive",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "TunnelViewWire")]
pub struct TunnelView {
    pub spec_id: TunnelSpecId,
    pub runtime_id: Option<TunnelRuntimeId>,
    pub label: String,
    pub host_id: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub state: TunnelState,
    /// Unix epoch milliseconds of the current runtime state; absent when never started.
    pub since: Option<u64>,
    pub attempt: u32,
    pub failure: Option<TunnelFailureCode>,
    pub failure_piece: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelViewWire {
    spec_id: TunnelSpecId,
    runtime_id: Option<TunnelRuntimeId>,
    label: String,
    host_id: String,
    local_port: u16,
    remote_host: String,
    remote_port: u16,
    state: TunnelState,
    since: Option<u64>,
    attempt: u32,
    failure: Option<TunnelFailureCode>,
    failure_piece: Option<String>,
}

impl TryFrom<TunnelViewWire> for TunnelView {
    type Error = &'static str;

    fn try_from(wire: TunnelViewWire) -> Result<Self, Self::Error> {
        if (wire.state == TunnelState::Stopped) != wire.runtime_id.is_none() {
            return Err("Stopped views have no runtime id; active views require one");
        }
        if let Some(piece) = &wire.failure_piece {
            if wire.failure != Some(TunnelFailureCode::UnsupportedConfig)
                || !UNSUPPORTED_CONFIG_PIECES.contains(&piece.as_str())
            {
                return Err("Invalid unsupported_config failure piece");
            }
        }
        if wire.failure == Some(TunnelFailureCode::UnsupportedConfig)
            && wire.failure_piece.is_none()
        {
            return Err("unsupported_config requires a failure piece");
        }
        Ok(Self {
            spec_id: wire.spec_id,
            runtime_id: wire.runtime_id,
            label: wire.label,
            host_id: wire.host_id,
            local_port: wire.local_port,
            remote_host: wire.remote_host,
            remote_port: wire.remote_port,
            state: wire.state,
            since: wire.since,
            attempt: wire.attempt,
            failure: wire.failure,
            failure_piece: wire.failure_piece,
        })
    }
}

impl TunnelView {
    pub fn stopped(spec: &TunnelSpec, host_id: String) -> Result<Self, IpcError> {
        validate_saved_spec(spec)?;
        let local_port = spec.local_port.ok_or_else(|| error("localPort"))?;
        Ok(Self {
            spec_id: spec.id.clone(),
            runtime_id: None,
            label: spec.label.clone(),
            host_id,
            local_port,
            remote_host: spec.remote_host.clone(),
            remote_port: spec.remote_port,
            state: TunnelState::Stopped,
            since: None,
            attempt: 0,
            failure: None,
            failure_piece: None,
        })
    }
}

fn error(field: &str) -> IpcError {
    IpcError::new(IpcErrorCode::InvalidArgument, "Invalid tunnel definition")
        .with_details(serde_json::json!({"field": field}))
}

pub fn validate_spec(spec: &TunnelSpec) -> Result<(), IpcError> {
    if spec.id.0.trim().is_empty() {
        return Err(error("id"));
    }
    if spec.label.trim().is_empty() {
        return Err(error("label"));
    }
    if spec.local_port == Some(0) {
        return Err(error("localPort"));
    }
    if spec.remote_port == 0 {
        return Err(error("remotePort"));
    }
    let host = &spec.remote_host;
    let dns = host.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let ipv6 = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    if host.contains('/') || host.contains('\0') || host.chars().any(char::is_whitespace)
        || !(dns || ipv6.parse::<Ipv6Addr>().is_ok())
    {
        return Err(error("remoteHost"));
    }
    Ok(())
}

pub fn validate_saved_spec(spec: &TunnelSpec) -> Result<(), IpcError> {
    validate_spec(spec)?;
    if spec.local_port.is_none() {
        return Err(error("localPort"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    DuplicateId { index: usize, id: TunnelSpecId },
    DuplicateLocalPort { index: usize, local_port: u16 },
}

/// Duplicate reservations are scoped to one host, including disabled definitions.
pub fn validate_specs_for_host(specs: &[TunnelSpec]) -> Vec<SpecError> {
    let mut ids = HashSet::new();
    let mut ports = HashSet::new();
    let mut errors = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        if !ids.insert(&spec.id) {
            errors.push(SpecError::DuplicateId { index, id: spec.id.clone() });
        }
        if let Some(local_port) = spec.local_port {
            if !ports.insert(local_port) {
                errors.push(SpecError::DuplicateLocalPort { index, local_port });
            }
        }
    }
    errors
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
