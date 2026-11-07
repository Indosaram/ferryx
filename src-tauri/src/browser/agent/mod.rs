//! Unified browser automation authority (plan contracts C1 and C2).
//!
//! Every browser operation, whatever its ingress (local CLI socket, the
//! `browser pipe` NDJSON and `browser mcp` stdio adapters, framed remote control,
//! desktop chrome), is classified into one canonical [`ToolId`] and decided
//! against a host-constructed [`Principal`]. `policy` holds the pure decision
//! functions; `authority` is the controller registry, origin grants and tab
//! leases that enforce the returned [`Obligation`]s; `service` admits each CLI
//! request on that path. `pipe` and `mcp` are CLI-side clients of the same
//! socket: they translate their frames into CLI requests and hold no policy.

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
pub mod mcp;
pub mod pipe;
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
