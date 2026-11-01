//! Exhaustive request classification and the authority decision (C1, C2).
//!
//! Every `BrowserCliRequest` and `RemoteBrowserOperation` variant maps to one
//! canonical [`ToolId`] by an exhaustive match with no wildcard arm, so a new
//! variant cannot reach a webview until it is classified here. Free-form
//! action/command strings are matched against closed lists; anything else is
//! refused as an unknown capability.

use serde::Serialize;

use super::catalog::{GrantScope, Platform, Scope, ToolId};
use super::protocol::{
    AuthorityError, ControllerDirectory, ControllerStanding, Principal, PrincipalKind,
};
use crate::browser::model::BrowserWaitCondition;
use crate::ipc::browser_cli::{BrowserCliRequest, RemoteBrowserOperation};
use crate::remote::auth::DevicePermission;

/// Who is calling, as established by the ingress before policy runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// Admitted by the transport token only (old binaries, no controller credential).
    TransportOnly,
    Principal(Principal),
}

/// What a request touches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Target {
    /// No tab: listings, tab creation, editor operations.
    Global,
    Browser(String),
    /// Tab addressed by 1-based listing index; resolved by the dispatcher.
    TabIndex(usize),
    /// Neither id nor index given; the dispatcher rejects it as invalid.
    Unspecified,
    Download(String),
}

/// Remote identity values carried in a request body. They are assertions to be
/// checked against the principal, never a source of identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssertedIdentity {
    pub device: Option<String>,
    pub connection: Option<String>,
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub tool: ToolId,
    pub target: Target,
    /// Destination URL for navigations/opens; needs a target-origin grant.
    pub destination: Option<String>,
    pub asserted: AssertedIdentity,
}

impl Classification {
    fn new(tool: ToolId, target: Target) -> Self {
        Self {
            tool,
            target,
            destination: None,
            asserted: AssertedIdentity::default(),
        }
    }

    fn with_destination(mut self, url: Option<&str>) -> Self {
        self.destination = url.map(str::to_string);
        self
    }

    fn with_asserted(
        mut self,
        device: Option<&str>,
        connection: Option<&str>,
        subscription: Option<&str>,
    ) -> Self {
        self.asserted = AssertedIdentity {
            device: device.map(str::to_string),
            connection: connection.map(str::to_string),
            subscription: subscription.map(str::to_string),
        };
        self
    }
}

/// Checks the dispatcher must enforce after an allow, before touching a webview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Obligation {
    /// Controller holds this exact-origin grant for the tab's current document.
    OriginGrant { scope: GrantScope },
    /// Controller holds an interact grant for the destination origin; about:blank exempt.
    DestinationOriginGrant { url: String },
    /// Controller holds the per-tab local lease.
    TabLease,
    /// Remote device holds the global driver lease (existing broker semantics).
    DriverLease,
    /// One-shot file capability approved in app chrome.
    FileCapability,
    /// Advertised native capability approved in app chrome.
    NativeCapability,
    /// Results are limited to tabs this controller is granted or owns.
    FilterToGranted,
    /// Desktop command implicitly reclaims the targeted tab from agents.
    ReclaimTab,
    /// Not a browser operation; the existing non-browser policy applies unchanged.
    NonBrowserPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub principal: Option<PrincipalKind>,
    pub tool: ToolId,
    pub obligations: Vec<Obligation>,
}

/// Host state consulted by every decision.
pub struct PolicyContext<'a> {
    pub controllers: &'a dyn ControllerDirectory,
    pub platform: Platform,
}

fn browser(id: &str) -> Target {
    Target::Browser(id.to_string())
}

fn tab_target(browser_id: Option<&str>, index: Option<usize>) -> Target {
    match (browser_id, index) {
        (Some(id), _) => browser(id),
        (None, Some(index)) => Target::TabIndex(index),
        (None, None) => Target::Unspecified,
    }
}

fn unknown(kind: &str, value: &str) -> AuthorityError {
    AuthorityError::UnknownCapability {
        name: format!("{kind}:{value}"),
    }
}

/// True when evaluating the condition runs caller-supplied script.
fn wait_runs_script(condition: &BrowserWaitCondition) -> bool {
    match condition {
        BrowserWaitCondition::Function { .. } => true,
        BrowserWaitCondition::WithTimeout { inner, .. } => wait_runs_script(inner),
        BrowserWaitCondition::Selector { .. }
        | BrowserWaitCondition::Text { .. }
        | BrowserWaitCondition::UrlContains { .. }
        | BrowserWaitCondition::LoadState { .. } => false,
    }
}

fn wait_tool(condition: &BrowserWaitCondition) -> ToolId {
    if wait_runs_script(condition) {
        ToolId::WaitFunction
    } else {
        ToolId::Wait
    }
}

/// Framed `execute` wait params. A string condition is script (as the backend
/// normalizes it); anything not provably script-free is classified as script.
fn execute_wait_tool(params: Option<&serde_json::Value>) -> ToolId {
    let condition = params.and_then(|params| params.get("condition"));
    match condition {
        Some(value) if !value.is_string() => {
            match serde_json::from_value::<BrowserWaitCondition>(value.clone()) {
                Ok(condition) => wait_tool(&condition),
                Err(_) => ToolId::WaitFunction,
            }
        }
        Some(_) | None => ToolId::WaitFunction,
    }
}

/// Classifies a local CLI request. Exhaustive over every variant.
pub fn classify_cli(request: &BrowserCliRequest) -> Result<Classification, AuthorityError> {
    use BrowserCliRequest as R;
    let classification = match request {
        R::List | R::TabList => Classification::new(ToolId::List, Target::Global),
        R::Identify => Classification::new(ToolId::Identify, Target::Global),
        R::Snapshot { browser_id } => Classification::new(ToolId::Snapshot, browser(browser_id)),
        R::Act { request } => Classification::new(ToolId::Act, browser(&request.browser_id)),
        R::OpenFile { .. } => Classification::new(ToolId::OpenFile, Target::Global),
        R::Open { url, .. } => {
            Classification::new(ToolId::Open, Target::Global).with_destination(Some(url.as_str()))
        }
        R::TabNew { url, .. } => {
            Classification::new(ToolId::Open, Target::Global).with_destination(url.as_deref())
        }
        R::Navigate { browser_id, url } => {
            Classification::new(ToolId::Navigate, browser(browser_id))
                .with_destination(Some(url.as_str()))
        }
        R::Back { browser_id } => Classification::new(ToolId::Back, browser(browser_id)),
        R::Forward { browser_id } => Classification::new(ToolId::Forward, browser(browser_id)),
        R::Reload { browser_id } => Classification::new(ToolId::Reload, browser(browser_id)),
        R::Close { browser_id } => Classification::new(ToolId::Close, browser(browser_id)),
        R::TabClose { browser_id, index } => {
            Classification::new(ToolId::Close, tab_target(browser_id.as_deref(), *index))
        }
        R::Eval { browser_id, .. } => Classification::new(ToolId::Eval, browser(browser_id)),
        R::Wait {
            browser_id,
            condition,
        } => Classification::new(wait_tool(condition), browser(browser_id)),
        R::Console { browser_id, .. } => Classification::new(ToolId::Console, browser(browser_id)),
        R::Focus { browser_id } => Classification::new(ToolId::Focus, browser(browser_id)),
        R::TabSwitch { browser_id, index } => Classification::new(
            ToolId::TabActivate,
            tab_target(browser_id.as_deref(), *index),
        ),
        R::Screenshot { browser_id, .. } => {
            Classification::new(ToolId::Screenshot, browser(browser_id))
        }
        R::Cookies {
            browser_id, action, ..
        } => {
            let tool = match action.to_ascii_lowercase().as_str() {
                "get" | "list" => ToolId::CookiesRead,
                "set" | "clear" | "delete" => ToolId::CookiesWrite,
                _ => return Err(unknown("cookies", action)),
            };
            Classification::new(tool, browser(browser_id))
        }
        R::Storage {
            browser_id, action, ..
        } => {
            let tool = match action.to_ascii_lowercase().as_str() {
                "get" => ToolId::StorageRead,
                "set" | "clear" | "delete" | "remove" => ToolId::StorageWrite,
                _ => return Err(unknown("storage", action)),
            };
            Classification::new(tool, browser(browser_id))
        }
        R::StateSave { browser_id, .. } => {
            Classification::new(ToolId::StateExport, browser(browser_id))
        }
        R::StateLoad { browser_id, .. } => {
            Classification::new(ToolId::StateImport, browser(browser_id))
        }
        R::Dialog {
            browser_id, action, ..
        } => {
            let tool = match action.as_str() {
                "accept" | "dismiss" => ToolId::DialogRespond,
                "list" => ToolId::DialogList,
                "policy" => ToolId::DialogPolicy,
                _ => return Err(unknown("dialog", action)),
            };
            Classification::new(tool, browser(browser_id))
        }
        R::Download {
            browser_id,
            url,
            id,
            action,
            ..
        } => match action.as_str() {
            "start" => Classification::new(
                ToolId::DownloadStart,
                tab_target(browser_id.as_deref(), None),
            )
            .with_destination(url.as_deref()),
            "list" => Classification::new(ToolId::DownloadList, Target::Global),
            "cancel" => Classification::new(
                ToolId::DownloadCancel,
                id.as_deref()
                    .map_or(Target::Unspecified, |id| Target::Download(id.to_string())),
            ),
            _ => return Err(unknown("download", action)),
        },
        R::Highlight { browser_id, .. } => {
            Classification::new(ToolId::Highlight, browser(browser_id))
        }
        R::RemoteAttach { .. } => Classification::new(ToolId::RemoteAttach, Target::Global),
    };
    Ok(classification)
}

/// Classifies a framed remote operation. Exhaustive over every variant.
pub fn classify_remote(
    operation: &RemoteBrowserOperation,
) -> Result<Classification, AuthorityError> {
    use RemoteBrowserOperation as O;
    let classification = match operation {
        O::Navigate { browser_id, url } => {
            Classification::new(ToolId::Navigate, browser(browser_id))
                .with_destination(Some(url.as_str()))
        }
        O::Back { browser_id } => Classification::new(ToolId::Back, browser(browser_id)),
        O::Forward { browser_id } => Classification::new(ToolId::Forward, browser(browser_id)),
        O::Reload { browser_id } => Classification::new(ToolId::Reload, browser(browser_id)),
        O::Click { browser_id, .. }
        | O::Fill { browser_id, .. }
        | O::Keypress { browser_id, .. } => Classification::new(ToolId::Act, browser(browser_id)),
        // `has_approval` is deliberately ignored: it never confers authority.
        O::Wait {
            browser_id,
            condition,
            ..
        } => Classification::new(wait_tool(condition), browser(browser_id)),
        O::Eval { browser_id, .. } => Classification::new(ToolId::Eval, browser(browser_id)),
        O::GetState { browser_id } => Classification::new(ToolId::GetState, browser(browser_id)),
        O::Snapshot { browser_id } => Classification::new(ToolId::Snapshot, browser(browser_id)),
        O::List { .. } => Classification::new(ToolId::List, Target::Global),
        O::Execute {
            browser_id,
            command,
            params,
            device_id,
            connection_id,
            ..
        } => {
            let tool = match command.as_str() {
                "snapshot" => ToolId::Snapshot,
                "getState" => ToolId::GetState,
                "navigate" => ToolId::Navigate,
                "back" => ToolId::Back,
                "forward" => ToolId::Forward,
                "reload" => ToolId::Reload,
                "click" | "fill" | "keypress" => ToolId::Act,
                "eval" => ToolId::Eval,
                "wait" => execute_wait_tool(params.as_ref()),
                _ => return Err(unknown("command", command)),
            };
            let destination = if tool == ToolId::Navigate {
                params
                    .as_ref()
                    .and_then(|params| params.get("url"))
                    .and_then(serde_json::Value::as_str)
            } else {
                None
            };
            Classification::new(tool, browser(browser_id))
                .with_destination(destination)
                .with_asserted(device_id.as_deref(), connection_id.as_deref(), None)
        }
        O::SubscribeViewer {
            browser_id,
            device_id,
            ..
        } => Classification::new(ToolId::ViewerSubscribe, browser(browser_id)).with_asserted(
            Some(device_id.as_str()),
            None,
            None,
        ),
        O::UnsubscribeViewer {
            browser_id,
            subscription_id,
        } => Classification::new(ToolId::ViewerUnsubscribe, browser(browser_id)).with_asserted(
            None,
            None,
            Some(subscription_id.as_str()),
        ),
        O::ClaimDriver {
            browser_id,
            device_id,
            connection_id,
            subscription_id,
            ..
        } => Classification::new(ToolId::DriverClaim, browser(browser_id)).with_asserted(
            Some(device_id.as_str()),
            Some(connection_id.as_str()),
            Some(subscription_id.as_str()),
        ),
        O::ReleaseDriver {
            subscription_id, ..
        } => Classification::new(ToolId::DriverRelease, Target::Global).with_asserted(
            None,
            None,
            Some(subscription_id.as_str()),
        ),
        O::HeartbeatDriver {
            device_id,
            connection_id,
            subscription_id,
            ..
        } => Classification::new(ToolId::DriverHeartbeat, Target::Global).with_asserted(
            Some(device_id.as_str()),
            Some(connection_id.as_str()),
            Some(subscription_id.as_str()),
        ),
    };
    Ok(classification)
}

pub fn authorize_cli(
    caller: &Caller,
    request: &BrowserCliRequest,
    context: &PolicyContext<'_>,
) -> Result<Authorization, AuthorityError> {
    authorize(caller, &classify_cli(request)?, context)
}

pub fn authorize_remote(
    caller: &Caller,
    operation: &RemoteBrowserOperation,
    context: &PolicyContext<'_>,
) -> Result<Authorization, AuthorityError> {
    authorize(caller, &classify_remote(operation)?, context)
}

/// Tools that exist only as the framed remote-control protocol.
const fn is_remote_protocol_tool(tool: ToolId) -> bool {
    matches!(
        tool,
        ToolId::ViewerSubscribe
            | ToolId::ViewerUnsubscribe
            | ToolId::DriverClaim
            | ToolId::DriverRelease
            | ToolId::DriverHeartbeat
    )
}

fn forbidden(tool: ToolId, reason: &'static str) -> AuthorityError {
    AuthorityError::Forbidden {
        tool: Some(tool),
        reason,
    }
}

/// The single authority decision. Authentication standing is checked first,
/// then per-principal tool admission, then platform support.
pub fn authorize(
    caller: &Caller,
    classification: &Classification,
    context: &PolicyContext<'_>,
) -> Result<Authorization, AuthorityError> {
    let tool = classification.tool;
    let spec = tool.spec();

    // OpenFile is an editor operation under its existing policy. It neither
    // needs nor manufactures a browser principal.
    if tool == ToolId::OpenFile {
        return Ok(Authorization {
            principal: match caller {
                Caller::TransportOnly => None,
                Caller::Principal(principal) => Some(principal.kind()),
            },
            tool,
            obligations: vec![Obligation::NonBrowserPolicy],
        });
    }

    let principal = match caller {
        Caller::TransportOnly => return Err(AuthorityError::PairingRequired),
        Caller::Principal(principal) => principal,
    };

    let mut obligations = Vec::new();
    match principal {
        Principal::LocalController { id, session_epoch } => {
            match context.controllers.standing(id) {
                ControllerStanding::Unpaired => return Err(AuthorityError::PairingRequired),
                ControllerStanding::Revoked => {
                    return Err(AuthorityError::Revoked { controller: *id })
                }
                ControllerStanding::Active {
                    session_epoch: current,
                } => {
                    if current != *session_epoch {
                        return Err(AuthorityError::StaleSession {
                            presented: *session_epoch,
                            current,
                        });
                    }
                }
            }
            if tool == ToolId::RemoteAttach {
                return Err(forbidden(
                    tool,
                    "remote attach requires the browser gateway credential, not a controller",
                ));
            }
            if is_remote_protocol_tool(tool) {
                return Err(forbidden(
                    tool,
                    "remote driver operations require a remote device session",
                ));
            }
            if let Some(scope) = spec.origin_grant {
                obligations.push(Obligation::OriginGrant { scope });
            }
            if let Some(url) = &classification.destination {
                obligations.push(Obligation::DestinationOriginGrant { url: url.clone() });
            }
            if spec.requires_lease {
                obligations.push(Obligation::TabLease);
            }
            match spec.scope {
                Scope::File => obligations.push(Obligation::FileCapability),
                Scope::Native => obligations.push(Obligation::NativeCapability),
                Scope::Read | Scope::Interact | Scope::Eval => {}
            }
            if matches!(
                tool,
                ToolId::List | ToolId::Identify | ToolId::DownloadList | ToolId::DownloadCancel
            ) {
                obligations.push(Obligation::FilterToGranted);
            }
        }
        Principal::RemoteDevice {
            device,
            connection,
            subscription,
            permission,
        } => {
            let asserted = &classification.asserted;
            let mismatched = asserted
                .device
                .as_ref()
                .is_some_and(|value| value != device)
                || asserted
                    .connection
                    .as_ref()
                    .is_some_and(|value| value != connection)
                || asserted
                    .subscription
                    .as_ref()
                    .is_some_and(|value| subscription.as_ref() != Some(value));
            if mismatched {
                return Err(forbidden(
                    tool,
                    "request identity does not match the authenticated remote session",
                ));
            }
            // Closed remote surface; same set as RemoteBrowserOperation.
            let required = match tool {
                ToolId::List | ToolId::ViewerSubscribe => None,
                ToolId::GetState | ToolId::Snapshot | ToolId::ViewerUnsubscribe => {
                    Some(DevicePermission::View)
                }
                ToolId::Navigate
                | ToolId::Back
                | ToolId::Forward
                | ToolId::Reload
                | ToolId::Act
                | ToolId::Wait
                | ToolId::DriverClaim
                | ToolId::DriverRelease
                | ToolId::DriverHeartbeat => Some(DevicePermission::Control),
                ToolId::Eval | ToolId::WaitFunction => {
                    return Err(forbidden(
                        tool,
                        "script evaluation is not available to remote devices",
                    ))
                }
                ToolId::Identify
                | ToolId::Console
                | ToolId::DialogList
                | ToolId::DownloadList
                | ToolId::CookiesRead
                | ToolId::StorageRead
                | ToolId::Open
                | ToolId::Close
                | ToolId::Highlight
                | ToolId::DialogRespond
                | ToolId::DialogPolicy
                | ToolId::DownloadCancel
                | ToolId::CookiesWrite
                | ToolId::StorageWrite
                | ToolId::RemoteAttach
                | ToolId::Focus
                | ToolId::TabActivate
                | ToolId::Screenshot
                | ToolId::StateExport
                | ToolId::StateImport
                | ToolId::DownloadStart
                | ToolId::OpenFile => {
                    return Err(forbidden(
                        tool,
                        "operation is not exposed to remote devices",
                    ))
                }
            };
            if *permission == DevicePermission::View && required == Some(DevicePermission::Control)
            {
                return Err(forbidden(tool, "Control permission required"));
            }
            if subscription.is_none() && required.is_some() {
                return Err(forbidden(tool, "an active viewer subscription is required"));
            }
            if spec.requires_lease {
                obligations.push(Obligation::DriverLease);
            }
        }
        Principal::DesktopUser => {
            if tool == ToolId::RemoteAttach || is_remote_protocol_tool(tool) {
                return Err(forbidden(
                    tool,
                    "remote control protocol is not a desktop operation",
                ));
            }
            if spec.requires_lease {
                obligations.push(Obligation::ReclaimTab);
            }
        }
    }

    spec.check_platform(context.platform)?;

    Ok(Authorization {
        principal: Some(principal.kind()),
        tool,
        obligations,
    })
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{
        ControllerId, SessionEpoch, BROWSER_CONTROLLER_PAIRING_REQUIRED,
        BROWSER_CONTROLLER_REVOKED, BROWSER_CONTROLLER_SESSION_STALE, BROWSER_FORBIDDEN,
        BROWSER_UNKNOWN_CAPABILITY,
    };
    use super::*;
    use crate::browser::model::{BrowserAutomationAction, BrowserAutomationRequest};
    use serde_json::json;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeDirectory(HashMap<ControllerId, ControllerStanding>);

    impl ControllerDirectory for FakeDirectory {
        fn standing(&self, id: &ControllerId) -> ControllerStanding {
            self.0
                .get(id)
                .copied()
                .unwrap_or(ControllerStanding::Unpaired)
        }
    }

    fn context(directory: &FakeDirectory, platform: Platform) -> PolicyContext<'_> {
        PolicyContext {
            controllers: directory,
            platform,
        }
    }

    fn active_controller(directory: &mut FakeDirectory, epoch: u64) -> Principal {
        let id = ControllerId::generate();
        directory.0.insert(
            id,
            ControllerStanding::Active {
                session_epoch: SessionEpoch(epoch),
            },
        );
        Principal::LocalController {
            id,
            session_epoch: SessionEpoch(epoch),
        }
    }

    fn remote(permission: DevicePermission) -> Principal {
        Principal::RemoteDevice {
            device: "dev-1".into(),
            connection: "conn-1".into(),
            subscription: Some("sub-conn-1".into()),
            permission,
        }
    }

    fn cli(value: serde_json::Value) -> BrowserCliRequest {
        serde_json::from_value(value).expect("valid cli request")
    }

    fn op(value: serde_json::Value) -> RemoteBrowserOperation {
        serde_json::from_value(value).expect("valid remote operation")
    }

    fn every_cli_request() -> Vec<(BrowserCliRequest, ToolId)> {
        let id = || "b1".to_string();
        vec![
            (BrowserCliRequest::List, ToolId::List),
            (
                BrowserCliRequest::Snapshot { browser_id: id() },
                ToolId::Snapshot,
            ),
            (
                BrowserCliRequest::Act {
                    request: BrowserAutomationRequest {
                        browser_id: id(),
                        generation: 1,
                        action: BrowserAutomationAction::Click {
                            reference: "e1".into(),
                        },
                    },
                },
                ToolId::Act,
            ),
            (
                BrowserCliRequest::OpenFile {
                    path: "a.rs".into(),
                    line: None,
                    col: None,
                },
                ToolId::OpenFile,
            ),
            (
                BrowserCliRequest::Open {
                    url: "https://a.test".into(),
                    workspace_id: None,
                    worktree_path: None,
                },
                ToolId::Open,
            ),
            (
                BrowserCliRequest::Navigate {
                    browser_id: id(),
                    url: "https://a.test".into(),
                },
                ToolId::Navigate,
            ),
            (BrowserCliRequest::Back { browser_id: id() }, ToolId::Back),
            (
                BrowserCliRequest::Forward { browser_id: id() },
                ToolId::Forward,
            ),
            (
                BrowserCliRequest::Reload { browser_id: id() },
                ToolId::Reload,
            ),
            (BrowserCliRequest::Close { browser_id: id() }, ToolId::Close),
            (BrowserCliRequest::Identify, ToolId::Identify),
            (
                BrowserCliRequest::Eval {
                    browser_id: id(),
                    script: "1".into(),
                },
                ToolId::Eval,
            ),
            (
                BrowserCliRequest::Wait {
                    browser_id: id(),
                    condition: BrowserWaitCondition::Selector {
                        selector: "#a".into(),
                    },
                },
                ToolId::Wait,
            ),
            (
                BrowserCliRequest::Wait {
                    browser_id: id(),
                    condition: BrowserWaitCondition::WithTimeout {
                        inner: Box::new(BrowserWaitCondition::Function {
                            script: "true".into(),
                        }),
                        timeout_ms: 10,
                    },
                },
                ToolId::WaitFunction,
            ),
            (
                BrowserCliRequest::Console {
                    browser_id: id(),
                    errors_only: None,
                    clear: None,
                },
                ToolId::Console,
            ),
            (BrowserCliRequest::Focus { browser_id: id() }, ToolId::Focus),
            (
                BrowserCliRequest::Screenshot {
                    browser_id: id(),
                    out_path: "a.png".into(),
                },
                ToolId::Screenshot,
            ),
            (
                BrowserCliRequest::Cookies {
                    browser_id: id(),
                    action: "list".into(),
                    name: None,
                    value: None,
                    domain: None,
                    path: None,
                },
                ToolId::CookiesRead,
            ),
            (
                BrowserCliRequest::Cookies {
                    browser_id: id(),
                    action: "SET".into(),
                    name: Some("a".into()),
                    value: None,
                    domain: None,
                    path: None,
                },
                ToolId::CookiesWrite,
            ),
            (
                BrowserCliRequest::Storage {
                    browser_id: id(),
                    kind: "local".into(),
                    action: "get".into(),
                    key: None,
                    value: None,
                    clear_all: None,
                    url: None,
                    domain: None,
                },
                ToolId::StorageRead,
            ),
            (
                BrowserCliRequest::Storage {
                    browser_id: id(),
                    kind: "local".into(),
                    action: "remove".into(),
                    key: None,
                    value: None,
                    clear_all: None,
                    url: None,
                    domain: None,
                },
                ToolId::StorageWrite,
            ),
            (BrowserCliRequest::TabList, ToolId::List),
            (
                BrowserCliRequest::TabNew {
                    url: None,
                    workspace_id: None,
                    worktree_path: None,
                },
                ToolId::Open,
            ),
            (
                BrowserCliRequest::TabSwitch {
                    browser_id: None,
                    index: Some(2),
                },
                ToolId::TabActivate,
            ),
            (
                BrowserCliRequest::TabClose {
                    browser_id: Some(id()),
                    index: None,
                },
                ToolId::Close,
            ),
            (
                BrowserCliRequest::StateSave {
                    browser_id: id(),
                    out_path: "s.json".into(),
                },
                ToolId::StateExport,
            ),
            (
                BrowserCliRequest::StateLoad {
                    browser_id: id(),
                    in_path: "s.json".into(),
                },
                ToolId::StateImport,
            ),
            (
                BrowserCliRequest::Dialog {
                    browser_id: id(),
                    action: "accept".into(),
                    prompt_text: None,
                    policy: None,
                },
                ToolId::DialogRespond,
            ),
            (
                BrowserCliRequest::Dialog {
                    browser_id: id(),
                    action: "list".into(),
                    prompt_text: None,
                    policy: None,
                },
                ToolId::DialogList,
            ),
            (
                BrowserCliRequest::Dialog {
                    browser_id: id(),
                    action: "policy".into(),
                    prompt_text: None,
                    policy: Some("accept".into()),
                },
                ToolId::DialogPolicy,
            ),
            (
                BrowserCliRequest::Download {
                    browser_id: Some(id()),
                    url: Some("https://a.test/f".into()),
                    path: Some("f".into()),
                    id: None,
                    action: "start".into(),
                },
                ToolId::DownloadStart,
            ),
            (
                BrowserCliRequest::Download {
                    browser_id: None,
                    url: None,
                    path: None,
                    id: None,
                    action: "list".into(),
                },
                ToolId::DownloadList,
            ),
            (
                BrowserCliRequest::Download {
                    browser_id: None,
                    url: None,
                    path: None,
                    id: Some("d1".into()),
                    action: "cancel".into(),
                },
                ToolId::DownloadCancel,
            ),
            (
                BrowserCliRequest::Highlight {
                    browser_id: id(),
                    selector: "#a".into(),
                    duration_ms: None,
                },
                ToolId::Highlight,
            ),
            (
                BrowserCliRequest::RemoteAttach {
                    protocol_version: Some(1),
                },
                ToolId::RemoteAttach,
            ),
        ]
    }

    fn every_remote_operation() -> Vec<(RemoteBrowserOperation, ToolId)> {
        vec![
            (
                op(json!({"operation":"navigate","browserId":"b1","url":"https://a.test"})),
                ToolId::Navigate,
            ),
            (
                op(json!({"operation":"back","browserId":"b1"})),
                ToolId::Back,
            ),
            (
                op(json!({"operation":"forward","browserId":"b1"})),
                ToolId::Forward,
            ),
            (
                op(json!({"operation":"reload","browserId":"b1"})),
                ToolId::Reload,
            ),
            (
                op(
                    json!({"operation":"click","browserId":"b1","reference":"e1","snapshotId":"s","mapRevision":1}),
                ),
                ToolId::Act,
            ),
            (
                op(json!({"operation":"fill","browserId":"b1","reference":"e1","value":"x"})),
                ToolId::Act,
            ),
            (
                op(json!({"operation":"keypress","browserId":"b1","key":"Enter"})),
                ToolId::Act,
            ),
            (
                op(
                    json!({"operation":"wait","browserId":"b1","condition":{"condition":"text","text":"Saved"}}),
                ),
                ToolId::Wait,
            ),
            (
                op(
                    json!({"operation":"wait","browserId":"b1","condition":{"condition":"function","script":"true"}}),
                ),
                ToolId::WaitFunction,
            ),
            (
                op(json!({"operation":"eval","browserId":"b1","script":"1"})),
                ToolId::Eval,
            ),
            (
                op(json!({"operation":"getState","browserId":"b1"})),
                ToolId::GetState,
            ),
            (
                op(json!({"operation":"snapshot","browserId":"b1"})),
                ToolId::Snapshot,
            ),
            (op(json!({"operation":"list"})), ToolId::List),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"getState"})),
                ToolId::GetState,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"snapshot"})),
                ToolId::Snapshot,
            ),
            (
                op(
                    json!({"operation":"execute","browserId":"b1","command":"navigate","params":{"url":"https://a.test"}}),
                ),
                ToolId::Navigate,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"back"})),
                ToolId::Back,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"forward"})),
                ToolId::Forward,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"reload"})),
                ToolId::Reload,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"click"})),
                ToolId::Act,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"fill"})),
                ToolId::Act,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"keypress"})),
                ToolId::Act,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"eval"})),
                ToolId::Eval,
            ),
            (
                op(
                    json!({"operation":"execute","browserId":"b1","command":"wait","params":{"condition":"document.title"}}),
                ),
                ToolId::WaitFunction,
            ),
            (
                op(
                    json!({"operation":"execute","browserId":"b1","command":"wait","params":{"condition":{"condition":"selector","selector":"#a"}}}),
                ),
                ToolId::Wait,
            ),
            (
                op(json!({"operation":"execute","browserId":"b1","command":"wait"})),
                ToolId::WaitFunction,
            ),
            (
                op(
                    json!({"operation":"subscribeViewer","browserId":"b1","deviceId":"dev-1","viewerInstanceId":"v1"}),
                ),
                ToolId::ViewerSubscribe,
            ),
            (
                op(
                    json!({"operation":"unsubscribeViewer","browserId":"b1","subscriptionId":"sub-conn-1"}),
                ),
                ToolId::ViewerUnsubscribe,
            ),
            (
                op(
                    json!({"operation":"claimDriver","browserId":"b1","deviceId":"dev-1","connectionId":"conn-1","subscriptionId":"sub-conn-1"}),
                ),
                ToolId::DriverClaim,
            ),
            (
                op(json!({"operation":"releaseDriver","subscriptionId":"sub-conn-1"})),
                ToolId::DriverRelease,
            ),
            (
                op(
                    json!({"operation":"heartbeatDriver","deviceId":"dev-1","connectionId":"conn-1","subscriptionId":"sub-conn-1"}),
                ),
                ToolId::DriverHeartbeat,
            ),
        ]
    }

    #[test]
    fn every_cli_request_variant_classifies_to_its_tool() {
        for (request, expected) in every_cli_request() {
            let classification = classify_cli(&request).expect("classified");
            assert_eq!(classification.tool, expected, "{request:?}");
        }
    }

    #[test]
    fn every_remote_operation_variant_classifies_to_its_tool() {
        for (operation, expected) in every_remote_operation() {
            let classification = classify_remote(&operation).expect("classified");
            assert_eq!(classification.tool, expected, "{operation:?}");
        }
    }

    #[test]
    fn transport_token_only_caller_requires_pairing_for_every_browser_request() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::MacOs);
        for (request, tool) in every_cli_request() {
            let result = authorize_cli(&Caller::TransportOnly, &request, &ctx);
            if tool == ToolId::OpenFile {
                assert_eq!(
                    result.expect("open-file keeps its policy").obligations,
                    vec![Obligation::NonBrowserPolicy]
                );
            } else {
                let error = result.expect_err("token-only must not authorize");
                assert_eq!(
                    error.code(),
                    BROWSER_CONTROLLER_PAIRING_REQUIRED,
                    "{request:?}"
                );
            }
        }
        for (operation, _) in every_remote_operation() {
            let error = authorize_remote(&Caller::TransportOnly, &operation, &ctx)
                .expect_err("token-only must not authorize");
            assert_eq!(error.code(), BROWSER_CONTROLLER_PAIRING_REQUIRED);
        }
    }

    #[test]
    fn unpaired_local_controller_is_denied() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::MacOs);
        let unpaired = Caller::Principal(Principal::LocalController {
            id: ControllerId::generate(),
            session_epoch: SessionEpoch(1),
        });
        for (request, tool) in every_cli_request() {
            if tool == ToolId::OpenFile {
                continue;
            }
            let error = authorize_cli(&unpaired, &request, &ctx).expect_err("unpaired denied");
            assert_eq!(
                error.code(),
                BROWSER_CONTROLLER_PAIRING_REQUIRED,
                "{request:?}"
            );
        }
    }

    #[test]
    fn stale_session_epoch_is_denied() {
        let mut directory = FakeDirectory::default();
        let principal = active_controller(&mut directory, 3);
        let Principal::LocalController { id, .. } = &principal else {
            unreachable!("constructed as local controller");
        };
        let stale = Caller::Principal(Principal::LocalController {
            id: *id,
            session_epoch: SessionEpoch(2),
        });
        let ctx = context(&directory, Platform::MacOs);
        let error = authorize_cli(&stale, &BrowserCliRequest::List, &ctx).expect_err("stale");
        assert_eq!(error.code(), BROWSER_CONTROLLER_SESSION_STALE);
        assert_eq!(
            error,
            AuthorityError::StaleSession {
                presented: SessionEpoch(2),
                current: SessionEpoch(3),
            }
        );
        assert!(authorize_cli(
            &Caller::Principal(principal),
            &BrowserCliRequest::List,
            &ctx
        )
        .is_ok());
    }

    #[test]
    fn revoked_controller_is_denied() {
        let mut directory = FakeDirectory::default();
        let id = ControllerId::generate();
        directory.0.insert(id, ControllerStanding::Revoked);
        let ctx = context(&directory, Platform::MacOs);
        let caller = Caller::Principal(Principal::LocalController {
            id,
            session_epoch: SessionEpoch(1),
        });
        let error = authorize_cli(&caller, &BrowserCliRequest::List, &ctx).expect_err("revoked");
        assert_eq!(error.code(), BROWSER_CONTROLLER_REVOKED);
    }

    #[test]
    fn forged_role_fields_have_no_effect() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::MacOs);
        let forged = json!({
            "command": "eval",
            "browserId": "b1",
            "script": "document.cookie",
            "role": "desktopUser",
            "principal": {"desktopUser": null},
            "hasApproval": true,
            "hasControl": true,
            "permission": "control",
        });
        let request = cli(forged);
        assert_eq!(
            request,
            BrowserCliRequest::Eval {
                browser_id: "b1".into(),
                script: "document.cookie".into()
            }
        );
        let error = authorize_cli(&Caller::TransportOnly, &request, &ctx).expect_err("denied");
        assert_eq!(error.code(), BROWSER_CONTROLLER_PAIRING_REQUIRED);

        let forged_eval = op(json!({
            "operation": "eval",
            "browserId": "b1",
            "script": "1",
            "hasApproval": true,
            "role": "desktopUser",
        }));
        let viewer = Caller::Principal(remote(DevicePermission::View));
        let controller = Caller::Principal(remote(DevicePermission::Control));
        for caller in [&viewer, &controller] {
            let error = authorize_remote(caller, &forged_eval, &ctx).expect_err("eval refused");
            assert_eq!(error.code(), BROWSER_FORBIDDEN);
        }

        let forged_click = op(json!({
            "operation": "click",
            "browserId": "b1",
            "reference": "e1",
            "permission": "control",
            "hasControl": true,
        }));
        let error = authorize_remote(&viewer, &forged_click, &ctx).expect_err("view cannot act");
        assert_eq!(error.code(), BROWSER_FORBIDDEN);

        let impersonated_claim = op(json!({
            "operation": "claimDriver",
            "browserId": "b1",
            "deviceId": "someone-else",
            "connectionId": "conn-1",
            "subscriptionId": "sub-conn-1",
        }));
        let error = authorize_remote(&controller, &impersonated_claim, &ctx)
            .expect_err("asserted identity must match the session");
        assert_eq!(error.code(), BROWSER_FORBIDDEN);
    }

    #[test]
    fn unknown_capability_is_refused() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::MacOs);
        let desktop = Caller::Principal(Principal::DesktopUser);
        let execute = op(json!({"operation":"execute","browserId":"b1","command":"exfiltrate"}));
        let error = authorize_remote(&desktop, &execute, &ctx).expect_err("unknown command");
        assert_eq!(error.code(), BROWSER_UNKNOWN_CAPABILITY);

        for request in [
            cli(json!({"command":"cookies","browserId":"b1","action":"export"})),
            cli(json!({"command":"storage","browserId":"b1","kind":"local","action":"dump"})),
            cli(json!({"command":"dialog","browserId":"b1","action":"ACCEPT"})),
            cli(json!({"command":"download","action":"resume"})),
        ] {
            let error = authorize_cli(&desktop, &request, &ctx).expect_err("unknown action");
            assert_eq!(error.code(), BROWSER_UNKNOWN_CAPABILITY, "{request:?}");
        }
        let unknown_command = serde_json::from_value::<BrowserCliRequest>(
            json!({"command":"teleport","browserId":"b1"}),
        );
        assert!(unknown_command.is_err());
    }

    #[test]
    fn desktop_user_is_distinct_from_guest_page() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::MacOs);
        let eval = BrowserCliRequest::Eval {
            browser_id: "b1".into(),
            script: "1".into(),
        };
        let desktop = Principal::for_app_webview("main").expect("app chrome");
        let allowed = authorize_cli(&Caller::Principal(desktop), &eval, &ctx).expect("desktop");
        assert_eq!(allowed.principal, Some(PrincipalKind::DesktopUser));
        assert_eq!(allowed.obligations, vec![Obligation::ReclaimTab]);

        let guest = Principal::for_app_webview("browser-0d6f6d1c-4c38-4f8e-9a61-1f1f1f1f1f1f");
        assert_eq!(guest.expect_err("guest refused").code(), BROWSER_FORBIDDEN);

        let attach = BrowserCliRequest::RemoteAttach {
            protocol_version: None,
        };
        let error = authorize_cli(&Caller::Principal(Principal::DesktopUser), &attach, &ctx)
            .expect_err("desktop does not attach as remote");
        assert_eq!(error.code(), BROWSER_FORBIDDEN);
    }

    #[test]
    fn paired_controller_gets_scoped_obligations_never_eval_from_interact() {
        let mut directory = FakeDirectory::default();
        let caller = Caller::Principal(active_controller(&mut directory, 1));
        let ctx = context(&directory, Platform::Linux);

        let requests = every_cli_request();
        let act = requests
            .iter()
            .find_map(|(request, tool)| (*tool == ToolId::Act).then_some(request))
            .expect("act fixture");
        let obligations = authorize_cli(&caller, act, &ctx).expect("act").obligations;
        assert!(obligations.contains(&Obligation::OriginGrant {
            scope: GrantScope::Interact
        }));
        assert!(obligations.contains(&Obligation::TabLease));

        let eval = BrowserCliRequest::Eval {
            browser_id: "b1".into(),
            script: "1".into(),
        };
        let obligations = authorize_cli(&caller, &eval, &ctx)
            .expect("eval")
            .obligations;
        assert!(obligations.contains(&Obligation::OriginGrant {
            scope: GrantScope::Eval
        }));

        let cookies = cli(json!({"command":"cookies","browserId":"b1","action":"get"}));
        let obligations = authorize_cli(&caller, &cookies, &ctx)
            .expect("cookies")
            .obligations;
        assert!(obligations.contains(&Obligation::OriginGrant {
            scope: GrantScope::Eval
        }));

        let state = BrowserCliRequest::StateSave {
            browser_id: "b1".into(),
            out_path: "s".into(),
        };
        let obligations = authorize_cli(&caller, &state, &ctx)
            .expect("state")
            .obligations;
        assert!(obligations.contains(&Obligation::FileCapability));

        let navigate = BrowserCliRequest::Navigate {
            browser_id: "b1".into(),
            url: "https://b.test".into(),
        };
        let obligations = authorize_cli(&caller, &navigate, &ctx)
            .expect("navigate")
            .obligations;
        assert!(obligations.contains(&Obligation::DestinationOriginGrant {
            url: "https://b.test".into()
        }));

        let attach = BrowserCliRequest::RemoteAttach {
            protocol_version: None,
        };
        assert_eq!(
            authorize_cli(&caller, &attach, &ctx)
                .expect_err("no attach")
                .code(),
            BROWSER_FORBIDDEN
        );
        let operations = every_remote_operation();
        let claim = operations
            .iter()
            .find_map(|(operation, tool)| (*tool == ToolId::DriverClaim).then_some(operation))
            .expect("claim fixture");
        assert_eq!(
            authorize_remote(&caller, claim, &ctx)
                .expect_err("no remote claim")
                .code(),
            BROWSER_FORBIDDEN
        );
    }

    #[test]
    fn remote_device_surface_matches_permission() {
        let directory = FakeDirectory::default();
        let ctx = context(&directory, Platform::Windows);
        let viewer = Caller::Principal(remote(DevicePermission::View));
        let controller = Caller::Principal(remote(DevicePermission::Control));
        let snapshot = op(json!({"operation":"snapshot","browserId":"b1"}));
        assert!(authorize_remote(&viewer, &snapshot, &ctx).is_ok());
        let navigate = op(json!({"operation":"navigate","browserId":"b1","url":"https://a.test"}));
        assert!(authorize_remote(&viewer, &navigate, &ctx).is_err());
        let allowed = authorize_remote(&controller, &navigate, &ctx).expect("control");
        assert_eq!(allowed.obligations, vec![Obligation::DriverLease]);

        let cookies = cli(json!({"command":"cookies","browserId":"b1","action":"get"}));
        assert_eq!(
            authorize_cli(&controller, &cookies, &ctx)
                .expect_err("not remote")
                .code(),
            BROWSER_FORBIDDEN
        );

        let unsubscribed = Caller::Principal(Principal::RemoteDevice {
            device: "dev-1".into(),
            connection: "conn-1".into(),
            subscription: None,
            permission: DevicePermission::Control,
        });
        assert!(authorize_remote(&unsubscribed, &navigate, &ctx).is_err());
        let list = op(json!({"operation":"list"}));
        assert!(authorize_remote(&unsubscribed, &list, &ctx).is_ok());
    }

    #[test]
    fn unsupported_platform_returns_typed_unsupported() {
        let directory = FakeDirectory::default();
        let screenshot = BrowserCliRequest::Screenshot {
            browser_id: "b1".into(),
            out_path: "a.png".into(),
        };
        let desktop = Caller::Principal(Principal::DesktopUser);
        let error = authorize_cli(&desktop, &screenshot, &context(&directory, Platform::Other))
            .expect_err("unsupported");
        assert!(
            matches!(error, AuthorityError::Unsupported(ref u) if u.tool == ToolId::Screenshot)
        );
        assert_eq!(error.code(), "UNSUPPORTED");
        for platform in [Platform::MacOs, Platform::Windows, Platform::Linux] {
            assert!(authorize_cli(&desktop, &screenshot, &context(&directory, platform)).is_ok());
        }
    }
}
