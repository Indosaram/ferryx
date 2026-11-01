//! Principal, approval and error model for browser automation authority (C1).
//!
//! A [`Principal`] is constructed by the host from transport-verified facts. It
//! deliberately implements neither `Deserialize` nor any constructor that takes
//! a caller-supplied role, so request payloads cannot mint authority.

use std::fmt;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::catalog::{ToolId, Unsupported};
use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::remote::auth::DevicePermission;

/// Old binaries and unpaired callers: transport admission alone grants nothing.
pub const BROWSER_CONTROLLER_PAIRING_REQUIRED: &str = "BROWSER_CONTROLLER_PAIRING_REQUIRED";
/// The controller identity was revoked; its credential no longer authorizes anything.
pub const BROWSER_CONTROLLER_REVOKED: &str = "BROWSER_CONTROLLER_REVOKED";
/// The controller session predates the current revocation/session epoch.
pub const BROWSER_CONTROLLER_SESSION_STALE: &str = "BROWSER_CONTROLLER_SESSION_STALE";
/// Capability, command or action name outside the canonical catalog.
pub const BROWSER_UNKNOWN_CAPABILITY: &str = "BROWSER_UNKNOWN_CAPABILITY";
/// Same code the framed remote transport already uses for permission refusals.
pub const BROWSER_FORBIDDEN: &str = "BROWSER_FORBIDDEN";
/// Same code the local CLI already uses for malformed requests.
pub const BROWSER_CLI_REQUEST_INVALID: &str = "BROWSER_CLI_REQUEST_INVALID";

/// Exact command named by pairing errors and help text.
pub const PAIRING_COMMAND: &str = "ferryx browser controllers pair --name <label>";
/// Pending pairing/grant requests expire after this long.
pub const PAIRING_APPROVAL_TTL: Duration = Duration::from_secs(60);
/// Pending approval cap per controller; duplicates coalesce.
pub const MAX_PENDING_APPROVALS_PER_CONTROLLER: usize = 16;
/// The only webview whose IPC may act as [`Principal::DesktopUser`].
pub const APP_CHROME_WEBVIEW_LABEL: &str = "main";

/// Paired local controller identity (generated UUID, never a display name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControllerId(Uuid);

impl ControllerId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Parses a credential's controller id. A malformed id is treated as an
    /// unpaired caller rather than a distinct, probeable error.
    pub fn parse(raw: &str) -> Result<Self, AuthorityError> {
        Uuid::parse_str(raw.trim())
            .map(Self)
            .map_err(|_| AuthorityError::PairingRequired)
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl fmt::Display for ControllerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for ControllerId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// Server-issued session/revocation epoch for a controller. Revocation and GUI
/// restart advance it; every dispatch rechecks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SessionEpoch(pub u64);

impl fmt::Display for SessionEpoch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Host-constructed authority subject. Not `Deserialize` by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// Paired automation controller authenticated on a local connection.
    LocalController {
        id: ControllerId,
        session_epoch: SessionEpoch,
    },
    /// Paired remote device admitted by the browser gateway. `permission` is
    /// the gateway-validated device permission bound to the session handle.
    /// `subscription` is absent until the viewer subscription exists.
    RemoteDevice {
        device: String,
        connection: String,
        subscription: Option<String>,
        permission: DevicePermission,
    },
    /// The human using the desktop app chrome. Never a guest page.
    DesktopUser,
}

impl Principal {
    /// Derives the principal for IPC that originated in a webview. Only the
    /// app-chrome webview is the desktop user; guest pages (`browser-*`) and any
    /// other webview are refused instead of inheriting desktop authority.
    pub fn for_app_webview(webview_label: &str) -> Result<Self, AuthorityError> {
        if webview_label == APP_CHROME_WEBVIEW_LABEL {
            Ok(Self::DesktopUser)
        } else {
            Err(AuthorityError::Forbidden {
                tool: None,
                reason: "browser authority requires the desktop app chrome; guest pages are never the desktop user",
            })
        }
    }

    pub const fn kind(&self) -> PrincipalKind {
        match self {
            Self::LocalController { .. } => PrincipalKind::LocalController,
            Self::RemoteDevice { .. } => PrincipalKind::RemoteDevice,
            Self::DesktopUser => PrincipalKind::DesktopUser,
        }
    }
}

/// Serializable, secret-free label of a principal for audit and responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrincipalKind {
    LocalController,
    RemoteDevice,
    DesktopUser,
}

/// Current registry standing of a controller identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerStanding {
    /// Not paired, or pairing still pending approval.
    Unpaired,
    Active {
        session_epoch: SessionEpoch,
    },
    Revoked,
}

/// Read-only view of the controller registry consulted on every dispatch.
pub trait ControllerDirectory {
    fn standing(&self, id: &ControllerId) -> ControllerStanding;
}

/// Pending approval identifier returned to the requesting controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApprovalId(Uuid);

impl ApprovalId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses an approval id a client echoes back; anything else is unknown.
    pub fn parse(raw: &str) -> Option<Self> {
        Uuid::parse_str(raw.trim()).ok().map(Self)
    }
}

impl fmt::Display for ApprovalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for ApprovalId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// What a pending approval asks the desktop user to consent to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ApprovalKind {
    PairController {
        controller: ControllerId,
        label: String,
    },
    OriginGrant {
        controller: ControllerId,
        origin: String,
        scopes: Vec<super::catalog::GrantScope>,
    },
    FileCapability {
        controller: ControllerId,
        tool: ToolId,
    },
    NativeCapability {
        controller: ControllerId,
        tool: ToolId,
    },
}

/// Outcome of a pending approval. Only app-chrome GUI IPC may resolve one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalResolution {
    ApproveOnce,
    /// Requires the separate explicit persistence checkbox (off by default).
    ApprovePersistent,
    Deny,
    Expired,
}

/// Authority refusal. Converts to `{code,message,details}` via [`IpcError`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthorityError {
    #[error(
        "browser controller pairing required: run `{}` and click Allow on the approval card in the Ferryx app (Settings > Browser > Agent access can turn this requirement off)",
        PAIRING_COMMAND
    )]
    PairingRequired,
    #[error(
        "browser controller {controller} was revoked: pair again with `{}`",
        PAIRING_COMMAND
    )]
    Revoked { controller: ControllerId },
    #[error("browser controller session is stale (epoch {presented}, current {current}); reconnect to start a new session")]
    StaleSession {
        presented: SessionEpoch,
        current: SessionEpoch,
    },
    #[error("unknown browser capability `{name}`")]
    UnknownCapability { name: String },
    #[error("{reason}")]
    Forbidden {
        tool: Option<ToolId>,
        reason: &'static str,
    },
    #[error("{} is unsupported on {}: {}", .0.tool, .0.platform, .0.reason)]
    Unsupported(Unsupported),
}

impl AuthorityError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::PairingRequired => BROWSER_CONTROLLER_PAIRING_REQUIRED,
            Self::Revoked { .. } => BROWSER_CONTROLLER_REVOKED,
            Self::StaleSession { .. } => BROWSER_CONTROLLER_SESSION_STALE,
            Self::UnknownCapability { .. } => BROWSER_UNKNOWN_CAPABILITY,
            Self::Forbidden { .. } => BROWSER_FORBIDDEN,
            // Matches the existing screenshot refusal (`IpcErrorCode::Unsupported`).
            Self::Unsupported(_) => "UNSUPPORTED",
        }
    }

    pub fn details(&self) -> serde_json::Value {
        match self {
            Self::PairingRequired => json!({ "pairingCommand": PAIRING_COMMAND }),
            Self::Revoked { controller } => json!({
                "controllerId": controller,
                "pairingCommand": PAIRING_COMMAND,
            }),
            Self::StaleSession { presented, current } => json!({
                "presentedEpoch": presented,
                "currentEpoch": current,
            }),
            Self::UnknownCapability { name } => json!({ "capability": name }),
            Self::Forbidden { tool, .. } => json!({ "tool": tool }),
            Self::Unsupported(unsupported) => json!({
                "tool": unsupported.tool,
                "platform": unsupported.platform,
            }),
        }
    }
}

impl From<Unsupported> for AuthorityError {
    fn from(value: Unsupported) -> Self {
        Self::Unsupported(value)
    }
}

impl From<AuthorityError> for IpcError {
    fn from(error: AuthorityError) -> Self {
        IpcError::new(IpcErrorCode::from_code_str(error.code()), error.to_string())
            .with_details(error.details())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_user_only_from_app_chrome_never_from_guest() {
        assert_eq!(
            Principal::for_app_webview(APP_CHROME_WEBVIEW_LABEL),
            Ok(Principal::DesktopUser)
        );
        let guest_label = format!("browser-{}", Uuid::new_v4());
        for label in [guest_label.as_str(), "", "Main", "main ", "popup-main"] {
            let error = Principal::for_app_webview(label).expect_err("guest must not be desktop");
            assert_eq!(error.code(), BROWSER_FORBIDDEN);
        }
    }

    #[test]
    fn malformed_controller_id_is_treated_as_unpaired() {
        assert_eq!(
            ControllerId::parse("qa-a"),
            Err(AuthorityError::PairingRequired)
        );
        let id = ControllerId::generate();
        assert_eq!(ControllerId::parse(&id.to_string()), Ok(id));
    }

    #[test]
    fn authority_errors_are_ipc_code_message_details_compatible() {
        let error: IpcError = AuthorityError::PairingRequired.into();
        let value = serde_json::to_value(&error).expect("serialize");
        assert_eq!(value["code"], BROWSER_CONTROLLER_PAIRING_REQUIRED);
        assert!(value["message"]
            .as_str()
            .is_some_and(|message| message.contains("ferryx browser controllers pair")));
        assert_eq!(value["details"]["pairingCommand"], PAIRING_COMMAND);

        let stale: IpcError = AuthorityError::StaleSession {
            presented: SessionEpoch(1),
            current: SessionEpoch(2),
        }
        .into();
        let value = serde_json::to_value(&stale).expect("serialize");
        assert_eq!(value["code"], BROWSER_CONTROLLER_SESSION_STALE);
        assert_eq!(value["details"]["currentEpoch"], 2);

        let revoked: IpcError = AuthorityError::Revoked {
            controller: ControllerId::generate(),
        }
        .into();
        assert_eq!(
            serde_json::to_value(&revoked).expect("serialize")["code"],
            BROWSER_CONTROLLER_REVOKED
        );
    }
}
