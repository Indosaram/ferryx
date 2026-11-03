use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IpcErrorCode {
    PairingRequired,
    PairingDenied,
    PairingLockedOut,
    PairingDisabled,
    CredentialRejected,
    InvalidPin,
    ConsentTimeout,
    HandshakeTimeout,
    RemoteClosed,
    NetworkUnreachable,
    CleanupFailed,
    Cancelled,
    ConnectionFailed,
    IncompatiblePeer,
}

impl IpcErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PairingRequired => "pairing-required",
            Self::PairingDenied => "pairing-denied",
            Self::PairingLockedOut => "pairing-locked-out",
            Self::PairingDisabled => "pairing-disabled",
            Self::CredentialRejected => "credential-rejected",
            Self::InvalidPin => "invalid-pin",
            Self::ConsentTimeout => "consent-timeout",
            Self::HandshakeTimeout => "handshake-timeout",
            Self::RemoteClosed => "remote-closed",
            Self::NetworkUnreachable => "network-unreachable",
            Self::CleanupFailed => "cleanup-failed",
            Self::Cancelled => "cancelled",
            Self::ConnectionFailed => "connection-failed",
            Self::IncompatiblePeer => "incompatible-peer",
        }
    }

    pub const fn is_retryable_by_default(self) -> bool {
        match self {
            Self::ConsentTimeout
            | Self::HandshakeTimeout
            | Self::NetworkUnreachable
            | Self::ConnectionFailed => true,
            Self::PairingRequired
            | Self::PairingDenied
            | Self::PairingLockedOut
            | Self::PairingDisabled
            | Self::CredentialRejected
            | Self::InvalidPin
            | Self::RemoteClosed
            | Self::CleanupFailed
            | Self::Cancelled
            | Self::IncompatiblePeer => false,
        }
    }
}

impl fmt::Display for IpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IpcErrorStage {
    Client,
    Connect,
    Preauth,
    #[serde(rename = "tls-psk")]
    TlsPsk,
    Handshake,
    Runtime,
    Cleanup,
}

impl IpcErrorStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Connect => "connect",
            Self::Preauth => "preauth",
            Self::TlsPsk => "tls-psk",
            Self::Handshake => "handshake",
            Self::Runtime => "runtime",
            Self::Cleanup => "cleanup",
        }
    }
}

impl fmt::Display for IpcErrorStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    pub stage: IpcErrorStage,
    pub retryable: bool,
}

impl IpcError {
    pub fn new(code: IpcErrorCode, stage: IpcErrorStage, message: impl Into<String>) -> Self {
        let retryable = code.is_retryable_by_default();
        Self {
            code,
            stage,
            message: message.into(),
            retryable,
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn pairing_required(message: impl Into<String>) -> Self {
        Self::new(
            IpcErrorCode::PairingRequired,
            IpcErrorStage::Preauth,
            message,
        )
    }

    pub fn credential_rejected(message: impl Into<String>) -> Self {
        Self::new(
            IpcErrorCode::CredentialRejected,
            IpcErrorStage::TlsPsk,
            message,
        )
    }

    pub fn invalid_pin(message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::InvalidPin, IpcErrorStage::Client, message)
    }

    pub fn incompatible_peer(message: impl Into<String>) -> Self {
        Self::new(
            IpcErrorCode::IncompatiblePeer,
            IpcErrorStage::Handshake,
            message,
        )
    }

    pub fn connection_failed(stage: IpcErrorStage, message: impl Into<String>) -> Self {
        Self::new(IpcErrorCode::ConnectionFailed, stage, message)
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}: {}] {}",
            self.stage.as_str(),
            self.code.as_str(),
            self.message
        )
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_error_code_and_stage_serialization() {
        let expected_codes = [
            (IpcErrorCode::PairingRequired, "pairing-required"),
            (IpcErrorCode::PairingDenied, "pairing-denied"),
            (IpcErrorCode::PairingLockedOut, "pairing-locked-out"),
            (IpcErrorCode::PairingDisabled, "pairing-disabled"),
            (IpcErrorCode::CredentialRejected, "credential-rejected"),
            (IpcErrorCode::InvalidPin, "invalid-pin"),
            (IpcErrorCode::ConsentTimeout, "consent-timeout"),
            (IpcErrorCode::HandshakeTimeout, "handshake-timeout"),
            (IpcErrorCode::RemoteClosed, "remote-closed"),
            (IpcErrorCode::NetworkUnreachable, "network-unreachable"),
            (IpcErrorCode::CleanupFailed, "cleanup-failed"),
            (IpcErrorCode::Cancelled, "cancelled"),
            (IpcErrorCode::ConnectionFailed, "connection-failed"),
            (IpcErrorCode::IncompatiblePeer, "incompatible-peer"),
        ];

        for (code, expected_str) in expected_codes {
            assert_eq!(code.as_str(), expected_str);
            let serialized = serde_json::to_string(&code).unwrap();
            assert_eq!(serialized, format!("\"{expected_str}\""));
            let deserialized: IpcErrorCode = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, code);
        }

        let expected_stages = [
            (IpcErrorStage::Client, "client"),
            (IpcErrorStage::Connect, "connect"),
            (IpcErrorStage::Preauth, "preauth"),
            (IpcErrorStage::TlsPsk, "tls-psk"),
            (IpcErrorStage::Handshake, "handshake"),
            (IpcErrorStage::Runtime, "runtime"),
            (IpcErrorStage::Cleanup, "cleanup"),
        ];

        for (stage, expected_str) in expected_stages {
            assert_eq!(stage.as_str(), expected_str);
            let serialized = serde_json::to_string(&stage).unwrap();
            assert_eq!(serialized, format!("\"{expected_str}\""));
            let deserialized: IpcErrorStage = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, stage);
        }
    }

    #[test]
    fn test_ipc_error_machine_fields_and_no_secret_leak() {
        let err = IpcError::pairing_required("PIN required for initial authorization");
        let json = serde_json::to_string(&err).unwrap();

        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["code"], "pairing-required");
        assert_eq!(val["stage"], "preauth");
        assert_eq!(val["message"], "PIN required for initial authorization");
        assert_eq!(val["retryable"], false);

        let obj = val.as_object().unwrap();
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["code", "message", "retryable", "stage"]);
    }
}
