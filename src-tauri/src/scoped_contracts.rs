//! Frozen section-2 boundary DTOs. No daemon, provider, or desktop implementation.
use serde::{Deserialize, Serialize};

/// Runtime replacement epoch; the scoped wire uses canonical decimal strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Epoch(pub u64);

impl Serialize for Epoch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Epoch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let value = text.parse::<u64>().map_err(serde::de::Error::custom)?;
        if value.to_string() != text {
            return Err(serde::de::Error::custom("epoch must be canonical decimal u64"));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetRef {
    pub host_id: String,
    pub owner_id: String,
    pub epoch: Epoch,
    pub backend_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunTarget {
    #[default]
    Local,
    Ssh { #[serde(rename = "hostId")] host_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScopeCapability { ScopeControlV1, SshHelperV1, ManagedCodexV1, CaptureV1 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InventoryCompleteness { Complete, Partial, Unknown }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot<T> {
    pub revision: u64,
    pub items: Vec<T>,
    pub completeness: InventoryCompleteness,
    pub unavailable_hosts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransitionKind { Waiting, Working, Idle, TaskComplete, Stopped, Removed }

/// Waiting carries provenance; only provider-confirmed completion uses TaskComplete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TransitionSource {
    Provider { provider: CanonicalProvider, request_id: Option<String> },
    TerminalDetection { detector: String },
    Lifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryTransition {
    pub revision: u64,
    pub target: TargetRef,
    pub kind: TransitionKind,
    pub source: TransitionSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CanonicalProvider { Codex, Claude }

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationClaimKey {
    pub host_id: String,
    pub provider: CanonicalProvider,
    pub conversation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ConversationOwner {
    Native { target: TargetRef },
    Managed { target: TargetRef },
    Provisional { request_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLease {
    pub target: TargetRef,
    pub device_id: String,
    pub lease_id: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationEnvelope<P> {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TargetRef>,
    pub params: P,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScopeErrorCode {
    InvalidRequest, Unauthorized, Forbidden, NotFound, TargetExpired,
    ControlConflict, RequestConflict, ProviderOwned, Unsupported, Timeout,
    InventoryIncomplete, PayloadTooLarge, CaptureUnsupported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeError<D = serde_json::Value> {
    pub code: ScopeErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: D,
}

/// Literal bool marker makes mismatched success/error envelopes unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireBool<const VALUE: bool>;
impl<const VALUE: bool> Serialize for WireBool<VALUE> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(VALUE)
    }
}
impl<'de, const VALUE: bool> Deserialize<'de> for WireBool<VALUE> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? != VALUE {
            return Err(serde::de::Error::custom("incorrect result discriminant"));
        }
        Ok(Self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ScopeResult<T, D = serde_json::Value> {
    Success { ok: WireBool<true>, data: T, #[serde(rename = "requestId")] request_id: String },
    Failure { ok: WireBool<false>, error: ScopeError<D>, #[serde(rename = "requestId")] request_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeEvent<T> {
    pub sequence: u64,
    pub target: TargetRef,
    pub revision: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: T,
}

/// Subscription acquisition must pair this snapshot/cursor atomically.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EventReplay<T, S> {
    Events { events: Vec<ScopeEvent<T>>, after_sequence: u64 },
    Gap { snapshot: InventorySnapshot<S>, after_sequence: u64 },
}

pub const ATTACHMENT_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const ATTACHMENT_MAX_FILES_PER_TURN: usize = 4;
pub const ATTACHMENT_MAX_TURN_BYTES: u64 = 20 * 1024 * 1024;
pub const ATTACHMENT_UNREFERENCED_TTL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentMediaType {
    #[serde(rename = "image/png")] Png,
    #[serde(rename = "image/jpeg")] Jpeg,
    #[serde(rename = "image/webp")] Webp,
    #[serde(rename = "text/plain")] Text,
    #[serde(rename = "application/pdf")] Pdf,
}

/// Host-private opaque receipt; intentionally contains no filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentReceipt {
    pub host_id: String,
    pub attachment_id: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub media_type: AttachmentMediaType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDraft {
    pub text: String,
    pub attachments: Vec<AttachmentReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedDraft {
    pub target: TargetRef,
    pub browser_generation: Epoch,
    pub attachments: Vec<AttachmentReceipt>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryStage { Staged, Accepted, ProviderRead }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    pub request_id: String,
    pub target: TargetRef,
    pub stage: DeliveryStage,
}

#[cfg(test)]
#[path = "scoped_contracts_tests.rs"]
mod tests;
