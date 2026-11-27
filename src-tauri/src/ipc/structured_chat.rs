//! Tauri commands for the structured agent chat mode (lane G1, chat-t3-parity plan).
//!
//! One command, [`cmd_structured_chat`], mirrors the HTTP routes under
//! `/api/v1/structured-chat/` op by op, exactly the way `ipc/reference_chat.rs` mirrors its
//! routes through a single `cmd_desktop_reference_chat`. The commands run against the same
//! process-local [`StructuredChatStore`] the gateway routes use: whichever process hosts the
//! gateway owns the provider processes, so a session started over HTTP survives a GUI restart
//! and vice versa. Remote clients keep using the HTTP routes; these commands exist for the
//! desktop surface and tests.
//!
//! Request shape: `{op, sessionId?, body?}` with `body` the same JSON the HTTP route takes.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::ipc::error::{IpcError, IpcErrorCode};
use crate::structured_chat::store::StructuredChatStore;
use crate::structured_chat::{
    ApprovalBody, CompactBody, CreateSessionBody, EditMessageBody, ExtensionUiResponseBody,
    ForkBody, ModelPreferencesBody, NavigateTreeBody, RenameSessionBody, RollbackBody,
    SessionSettings, SteerBody, StructuredChatError, TurnBody,
};

/// One structured-chat operation, mirroring the HTTP route table.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredChatRequest {
    /// One of: createSession, listSessions, turn, history, events, steer, interrupt, rollback,
    /// approve, settings, capabilities, clear, compact, resume, closeSession.
    pub op: String,
    #[serde(default)]
    pub session_id: Option<String>,
    /// The route's body, when the op takes one. `after`/`turns` ride in the body object too.
    #[serde(default)]
    pub body: Option<Value>,
}

async fn store() -> Result<&'static Arc<StructuredChatStore>, StructuredChatError> {
    // Both surfaces share one store per process; the gateway opens it on its first route and
    // the GUI opens it here on its first command. `open` replays any journals, so sessions a
    // previous process owned come back as `ended` and resume on the next turn.
    static STORE: tokio::sync::OnceCell<Arc<StructuredChatStore>> = tokio::sync::OnceCell::const_new();
    STORE.get_or_try_init(StructuredChatStore::open).await
}

#[tauri::command]
pub async fn cmd_structured_chat(
    request: StructuredChatRequest,
) -> Result<Value, crate::ipc::IpcError> {
    let store = store().await.map_err(|error| {
        crate::ipc::IpcError::new(
            crate::ipc::IpcErrorCode::from_code_str(error.code()),
            format!("{}: {}", error.code(), error.message()),
        )
    })?;
    let session_id = request.session_id.as_deref().unwrap_or_default();
    match request.op.as_str() {
        "createSession" => {
            let body: CreateSessionBody = parse_body(request.body)?;
            to_ok(store.create_session(&body).await)
        }
        "listSessions" => Ok(serde_json::json!({ "sessions": store.list_sessions().await })),
        "turn" => {
            let body: TurnBody = parse_body(request.body)?;
            to_ok(store.start_turn(session_id, &body).await.map(|seq| serde_json::json!({ "seq": seq })))
        }
        "history" => {
            let query = crate::structured_chat::store::HistoryQuery {
                after: request.body.as_ref().and_then(|b| b.get("after")).and_then(Value::as_u64),
                before: request.body.as_ref().and_then(|b| b.get("before")).and_then(Value::as_u64),
                limit: request
                    .body
                    .as_ref()
                    .and_then(|b| b.get("limit"))
                    .and_then(Value::as_u64)
                    .map(|limit| limit as usize),
            };
            to_ok(store.history(session_id, &query).await.and_then(|page| {
                serde_json::to_value(page)
                    .map_err(|error| StructuredChatError::Provider(error.to_string()))
            }))
        }
        "events" => {
            let after = request.body.as_ref().and_then(|b| b.get("after")).and_then(Value::as_u64).unwrap_or(0);
            to_ok(store.wait_for_events(session_id, after).await.map(|parts| serde_json::json!({ "parts": parts })))
        }
        "steer" => {
            let body: SteerBody = parse_body(request.body)?;
            to_ok(store.steer(session_id, &body).await.map(|seq| serde_json::json!({ "seq": seq })))
        }
        "interrupt" => to_ok(ok_after(store.interrupt(session_id).await)),
        "rollback" => {
            let body: RollbackBody = parse_body(request.body)?;
            to_ok(ok_after(
                store
                    .rollback_turns(session_id, body.turns, body.revert_files)
                    .await,
            ))
        }
        "approve" => {
            let request_id = request
                .body
                .as_ref()
                .and_then(|b| b.get("requestId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let body: ApprovalBody = parse_body(request.body)?;
            let request_id = request_id.unwrap_or_default();
            to_ok(ok_after(
                store.answer_approval(session_id, &request_id, body.decision).await,
            ))
        }
        "settings" => {
            let body: SessionSettings = parse_body(request.body)?;
            to_ok(store.update_settings(session_id, &body).await)
        }
        "capabilities" => to_ok(store.capabilities(session_id).await),
        "clear" => {
            to_ok(store.clear(session_id).await.map(|seq| serde_json::json!({ "seq": seq })))
        }
        "compact" => {
            let body: CompactBody = request
                .body
                .map(|v| parse_body(Some(v)))
                .transpose()?
                .unwrap_or_default();
            to_ok(ok_after(
                store
                    .compact_with_instructions(session_id, body.custom_instructions.as_deref())
                    .await,
            ))
        }
        "resume" => to_ok(ok_after(store.resume(session_id).await)),
        "commands" => to_ok(
            store
                .commands(session_id)
                .await
                .map(|commands| serde_json::json!({ "commands": commands })),
        ),
        "answerExtensionUi" => {
            let request_id = request
                .body
                .as_ref()
                .and_then(|b| b.get("requestId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let body: ExtensionUiResponseBody = parse_body(request.body)?;
            to_ok(ok_after(
                store.answer_extension_ui(session_id, &request_id, body).await,
            ))
        }
        "tree" => to_ok(store.tree(session_id).await),
        "navigateTree" => {
            let body: NavigateTreeBody = parse_body(request.body)?;
            to_ok(ok_after(store.navigate_tree(session_id, &body).await))
        }
        "fork" => {
            let body: ForkBody = parse_body(request.body)?;
            to_ok(store.fork(session_id, &body.entry_id).await)
        }
        "clone" => to_ok(store.clone_session(session_id).await),
        "editMessage" => {
            let body: EditMessageBody = parse_body(request.body)?;
            to_ok(ok_after(store.edit_message(session_id, &body).await))
        }
        "rename" => {
            let body: RenameSessionBody = parse_body(request.body)?;
            to_ok(ok_after(store.rename_session(session_id, &body.name).await))
        }
        "stats" => to_ok(store.stats(session_id).await),
        "reload" => to_ok(ok_after(store.reload(session_id).await)),
        "media" => {
            let tool_call_id = request
                .body
                .as_ref()
                .and_then(|b| b.get("toolCallId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let content_index = request
                .body
                .as_ref()
                .and_then(|b| b.get("contentIndex"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32;
            to_ok(store.get_media(session_id, &tool_call_id, content_index).await)
        }
        "clearQueue" => to_ok(ok_after(store.clear_queue(session_id).await)),
        "modelPreferences" => {
            let body: ModelPreferencesBody = parse_body(request.body)?;
            to_ok(ok_after(store.set_model_preferences(session_id, &body).await))
        }
        "closeSession" => to_ok(ok_after(store.close_session(session_id).await)),
        other => Err(crate::ipc::IpcError::new(
            crate::ipc::IpcErrorCode::InvalidArgument,
            format!("unknown structured-chat op {other}"),
        )),
    }
}

fn parse_body<B: serde::de::DeserializeOwned>(body: Option<Value>) -> Result<B, crate::ipc::IpcError> {
    serde_json::from_value(body.unwrap_or(Value::Null)).map_err(|error| {
        crate::ipc::IpcError::new(
            crate::ipc::IpcErrorCode::InvalidArgument,
            format!("malformed structured-chat body: {error}"),
        )
    })
}

fn ok_after(result: Result<(), StructuredChatError>) -> Result<Value, StructuredChatError> {
    result.map(|_| serde_json::json!({ "ok": true }))
}

fn to_ok<T: serde::Serialize>(
    result: Result<T, StructuredChatError>,
) -> Result<Value, IpcError> {
    result
        .and_then(|value| {
            serde_json::to_value(value)
                .map_err(|error| StructuredChatError::Provider(error.to_string()))
        })
        .map_err(|error| {
            let code = match error {
                StructuredChatError::NotFound => IpcErrorCode::SessionNotFound,
                StructuredChatError::Conflict(_) | StructuredChatError::Unsupported(_) => {
                    IpcErrorCode::ControlConflict
                }
                StructuredChatError::InvalidRequest(_) | StructuredChatError::Provider(_) => {
                    IpcErrorCode::InternalError
                }
            };
            IpcError::new(code, format!("{}: {}", error.code(), error.message()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_after_maps_success_and_failure() {
        let ok = ok_after(Ok(())).unwrap();
        assert_eq!(ok, serde_json::json!({"ok": true}));
        let error = ok_after(Err(StructuredChatError::Unsupported(
            "COMPACT_UNSUPPORTED: none".into(),
        )))
        .unwrap_err();
        assert_eq!(error.code(), "COMPACT_UNSUPPORTED");
    }

    #[test]
    fn request_bodies_deserialize_camel_case() {
        let request: StructuredChatRequest = serde_json::from_value(serde_json::json!({
            "op": "turn",
            "sessionId": "s-1",
            "body": {"text": "hi", "attachments": [{"path": "/tmp/a.png"}]}
        }))
        .unwrap();
        assert_eq!(request.op, "turn");
        assert_eq!(request.session_id.as_deref(), Some("s-1"));
        let body: TurnBody = serde_json::from_value(request.body.unwrap()).unwrap();
        assert_eq!(body.text, "hi");
        assert_eq!(body.attachments.unwrap()[0].path, "/tmp/a.png");
    }
}
