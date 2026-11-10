//! The Supervisor voice call over the gateway: the paired phone reads the same speech status the
//! desktop reads, sends a recording, and receives the spoken reply. The daemon's own IPC commands
//! do the work; this module is only the phone-reachable surface in front of them, so both clients
//! answer with one status, one transcript shape and one WAV.
//!
//! Access follows agent control: any device may read the status, and only a device that may mutate
//! may spend provider credit on a transcript or a spoken reply.

use super::*;

/// Axum body bound for one recording: the base64 form of the 8 MiB WAV limit plus its JSON
/// envelope. The relay admits the same shape with the same bound.
pub(crate) const VOICE_TRANSCRIBE_BODY_MAX: usize = 12 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct TranscribeBody {
    wav_base64: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpeakBody {
    text: String,
}

/// A voice refusal as the frozen machine envelope. The call's own three codes keep their names so
/// the surface can tell "not configured here" from "the provider failed"; everything else is a
/// bug in this daemon and answers 500.
fn refusal_wire(code: &str) -> (StatusCode, &'static str) {
    match code {
        "VOICE_UNAVAILABLE" => (StatusCode::SERVICE_UNAVAILABLE, "VOICE_UNAVAILABLE"),
        "VOICE_INPUT_INVALID" => (StatusCode::BAD_REQUEST, "INVALID_REQUEST"),
        "VOICE_PROVIDER_FAILED" => (StatusCode::BAD_GATEWAY, "VOICE_PROVIDER_FAILED"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
    }
}

fn refusal(error: crate::ipc::error::IpcError) -> Response {
    let (status, wire) = refusal_wire(&error.code.to_string());
    machine_error_with_details(status, wire, &error.message, serde_json::Map::new())
}

fn no_store_value(value: impl serde::Serialize) -> Response {
    match serde_json::to_value(value) {
        Ok(value) => no_store_response(value),
        Err(_) => machine_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
    }
}

fn no_store_response(value: serde_json::Value) -> Response {
    (
        StatusCode::OK,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        axum::Json(value),
    )
        .into_response()
}

/// `GET /api/v1/voice/status`: whether this computer can speak, and the recording shape it wants.
pub(crate) async fn http_status(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, false) {
        return response;
    }
    no_store_value(crate::ipc::voice_call::cmd_voice_call_status())
}

/// `POST /api/v1/voice/transcribe`: one WAV recording becomes text.
pub(crate) async fn http_transcribe(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<TranscribeBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    let Ok(Json(body)) = body else {
        return reference_chat_error(
            ScopeErrorCode::InvalidRequest,
            "the body must be {wavBase64}",
        );
    };
    match crate::ipc::voice_call::cmd_voice_call_transcribe(body.wav_base64).await {
        Ok(value) => no_store_response(value),
        Err(error) => refusal(error),
    }
}

/// `POST /api/v1/voice/speak`: the supervisor's receipt becomes a WAV the surface can play.
pub(crate) async fn http_speak(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    body: Result<Json<SpeakBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if let Err(response) = agent_control_service::http_device(&state, &headers, true) {
        return response;
    }
    let Ok(Json(body)) = body else {
        return reference_chat_error(ScopeErrorCode::InvalidRequest, "the body must be {text}");
    };
    match crate::ipc::voice_call::cmd_voice_call_speak(body.text).await {
        Ok(spoken) => no_store_value(spoken),
        Err(error) => refusal(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::error::{IpcError, IpcErrorCode};

    #[test]
    fn each_voice_code_keeps_its_own_wire_name() {
        let cases = [
            ("VOICE_UNAVAILABLE", StatusCode::SERVICE_UNAVAILABLE, "VOICE_UNAVAILABLE"),
            ("VOICE_INPUT_INVALID", StatusCode::BAD_REQUEST, "INVALID_REQUEST"),
            ("VOICE_PROVIDER_FAILED", StatusCode::BAD_GATEWAY, "VOICE_PROVIDER_FAILED"),
        ];
        for (code, status, wire) in cases {
            assert_eq!(refusal_wire(code), (status, wire), "{code}");
            let response = refusal(IpcError::new(IpcErrorCode::Custom(code.into()), "why"));
            assert_eq!(response.status(), status, "{code}");
        }
        assert_eq!(refusal_wire("SOMETHING_ELSE"), (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"));
    }

    #[test]
    fn an_unknown_code_is_a_daemon_bug_not_a_voice_state() {
        let response = refusal(IpcError::new(IpcErrorCode::InternalError, "boom"));
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn the_transcribe_body_refuses_extra_fields() {
        assert!(serde_json::from_str::<TranscribeBody>(r#"{"wavBase64":"AA=="}"#).is_ok());
        assert!(serde_json::from_str::<TranscribeBody>(r#"{"wavBase64":"AA==","extra":1}"#).is_err());
        assert!(serde_json::from_str::<TranscribeBody>(r#"{"wav_base64":"AA=="}"#).is_err());
    }

    #[test]
    fn the_speak_body_is_exactly_the_text() {
        assert!(serde_json::from_str::<SpeakBody>(r#"{"text":"Done."}"#).is_ok());
        assert!(serde_json::from_str::<SpeakBody>(r#"{"text":"Done.","voice":"x"}"#).is_err());
    }
}
