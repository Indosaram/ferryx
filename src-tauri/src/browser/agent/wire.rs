//! The controller management sub-protocol on the local browser endpoint: an envelope field
//! `agent` beside the transport token. It pairs controllers and asks for grants; it never
//! touches a page, so it is not a `BrowserCliRequest`.

use serde::{Deserialize, Serialize};

use super::catalog::GrantScope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum AgentRequest {
    /// Proposes a new controller identity; the desktop user approves it in the app.
    Pair { id: String, label: String, secret: String },
    /// Asks for an exact-origin grant (`origin`) or a one-shot tool capability (`tool`).
    #[serde(rename_all = "camelCase")]
    Grant {
        #[serde(default)]
        origin: Option<String>,
        #[serde(default)]
        scopes: Vec<GrantScope>,
        #[serde(default)]
        tool: Option<String>,
    },
    /// Waits until the desktop user resolves an approval or it expires.
    #[serde(rename_all = "camelCase")]
    Await { approval_id: String },
    /// The calling controller's record and grants.
    Controllers,
    /// The calling controller revokes itself.
    Revoke,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentResponse {
    #[serde(rename_all = "camelCase")]
    Pending { approval_id: String },
    Resolved { resolution: String },
    Controllers { controllers: serde_json::Value },
    Revoked,
    Error {
        code: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

#[cfg(feature = "desktop")]
fn error(error: crate::ipc::error::IpcError) -> AgentResponse {
    let code = serde_json::to_value(&error.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{:?}", error.code));
    AgentResponse::Error { code, message: error.message, details: error.details }
}

#[cfg(feature = "desktop")]
pub async fn execute<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    auth: Option<&crate::ipc::browser_cli::ControllerAuth>,
    request: AgentRequest,
) -> AgentResponse {
    use super::service;
    use super::AuthorityError;
    let need_auth = || auth.ok_or_else(|| crate::ipc::error::IpcError::from(AuthorityError::PairingRequired));
    let outcome = match request {
        AgentRequest::Pair { id, label, secret } => service::request_pairing(app, &id, &label, &secret)
            .await
            .map(|approval| AgentResponse::Pending { approval_id: approval.to_string() }),
        AgentRequest::Grant { origin, scopes, tool } => match need_auth() {
            Ok(auth) => service::request_grant(app, auth, origin.as_deref(), &scopes, tool.as_deref())
                .await
                .map(|approval| AgentResponse::Pending { approval_id: approval.to_string() }),
            Err(error) => Err(error),
        },
        AgentRequest::Await { approval_id } => service::await_resolution(&approval_id).await.map(|resolution| {
            AgentResponse::Resolved {
                resolution: serde_json::to_value(resolution)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_default(),
            }
        }),
        AgentRequest::Controllers => match need_auth() {
            Ok(auth) => service::controller_summary(auth).await.map(|list| AgentResponse::Controllers {
                controllers: serde_json::to_value(list).unwrap_or_default(),
            }),
            Err(error) => Err(error),
        },
        AgentRequest::Revoke => match need_auth() {
            Ok(auth) => service::revoke_self(app, auth).await.map(|()| AgentResponse::Revoked),
            Err(error) => Err(error),
        },
    };
    outcome.unwrap_or_else(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_responses_have_a_stable_wire_shape() {
        let pair: AgentRequest =
            serde_json::from_str(r#"{"op":"pair","id":"x","label":"qa-a","secret":"s"}"#).unwrap();
        assert_eq!(pair, AgentRequest::Pair { id: "x".into(), label: "qa-a".into(), secret: "s".into() });
        let grant: AgentRequest =
            serde_json::from_str(r#"{"op":"grant","origin":"http://127.0.0.1:18761","scopes":["read","interact"]}"#)
                .unwrap();
        assert_eq!(
            grant,
            AgentRequest::Grant {
                origin: Some("http://127.0.0.1:18761".into()),
                scopes: vec![GrantScope::Read, GrantScope::Interact],
                tool: None,
            }
        );
        assert!(serde_json::from_str::<AgentRequest>(r#"{"op":"grant","scopes":["admin"]}"#).is_err());
        assert_eq!(
            serde_json::to_value(AgentResponse::Pending { approval_id: "a".into() }).unwrap(),
            serde_json::json!({"type": "pending", "approvalId": "a"})
        );
    }
}
