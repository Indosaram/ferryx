//! Unified browser automation authority (plan contracts C1 and C2).
//!
//! Every browser operation, whatever its ingress (local CLI socket, future pipe
//! and MCP adapters, framed remote control, desktop chrome), is classified into
//! one canonical [`ToolId`] and decided against a host-constructed [`Principal`].
//! This module defines the model and the pure decision functions only; ingress
//! wiring, the controller registry, origin grants and tab ownership enforce the
//! returned [`Obligation`]s in later stages.

pub mod authority;
mod catalog;
#[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub mod cdp;
pub mod client;
#[cfg(all(feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub mod cef_backend;
#[cfg(all(test, feature = "desktop", any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod fake_cdp;
pub(crate) mod platform;
mod policy;
#[cfg(feature = "desktop")]
pub mod observe;
mod protocol;
pub mod redact;
#[cfg(feature = "desktop")]
pub mod page;
#[cfg(feature = "desktop")]
pub mod service;
pub mod wire;

pub use catalog::{GrantScope, Platform, PlatformSupport, Scope, ToolId, ToolSpec, Unsupported};
pub use policy::{
    authorize, authorize_cli, authorize_remote, classify_cli, classify_remote, AssertedIdentity,
    Authorization, Caller, Classification, Obligation, PolicyContext, Target,
};
pub use protocol::{
    ApprovalId, ApprovalKind, ApprovalResolution, AuthorityError, ControllerDirectory,
    ControllerId, ControllerStanding, Principal, PrincipalKind, SessionEpoch,
    APP_CHROME_WEBVIEW_LABEL, BROWSER_CLI_REQUEST_INVALID, BROWSER_CONTROLLER_PAIRING_REQUIRED,
    BROWSER_CONTROLLER_REVOKED, BROWSER_CONTROLLER_SESSION_STALE, BROWSER_FORBIDDEN,
    BROWSER_UNKNOWN_CAPABILITY, MAX_PENDING_APPROVALS_PER_CONTROLLER, PAIRING_APPROVAL_TTL,
    PAIRING_COMMAND,
};
