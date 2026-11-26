//! Spoken replies through the OpenAI Realtime API, paid for by the Codex sign-in this machine
//! already has, so the supervisor can talk back without a separate speech key.
//!
//! This is the same standalone WebSocket the Codex desktop app opens for voice
//! (`thread/realtime/start`, codex-rs `realtime_websocket`): `wss://api.openai.com/v1/realtime`
//! with the ChatGPT OAuth bearer and account header. The ChatGPT backend host refuses WebSocket
//! upgrades, but the API host accepts the same bearer. It is undocumented for third parties and
//! can change without notice, so every failure is reported as text for the caller to surface.
//!
//! One call is one session: configure audio-only output, add the text as a user item with an
//! instruction to read it verbatim, request one response, and collect its PCM16 24 kHz deltas.

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

use crate::codex_speech;

const REALTIME_WS_ROOT: &str = "wss://api.openai.com/v1/realtime";
/// The model the Codex desktop app asks for on its standalone voice socket.
pub const DEFAULT_REALTIME_MODEL: &str = "gpt-realtime-1.5";
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);
/// A reply is a sentence or two; refuse to buffer more than ~4 minutes of 24 kHz mono PCM16.
const MAX_PCM_BYTES: usize = 24_000 * 2 * 240;

const READ_ALOUD_INSTRUCTIONS: &str = "You are a text-to-speech voice. Read the user's message aloud \
exactly as written, in the language it is written in. Do not answer it, comment on it, translate \
it, or add any words of your own.";

/// The upstream socket URL for `model`. The `intent=quicksilver` parameter is what the Codex app
/// sends on its v1 standalone path; the ChatGPT-auth voice entitlement is keyed on it.
pub fn realtime_url(model: &str) -> String {
    format!(
        "{REALTIME_WS_ROOT}?intent=quicksilver&model={}",
        urlencode(model)
    )
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The client events of one read-aloud session, in send order.
pub fn session_events(voice: &str, text: &str) -> [Value; 3] {
    [
        json!({
            "type": "session.update",
            "session": {
                "type": "realtime",
                "instructions": READ_ALOUD_INSTRUCTIONS,
                "output_modalities": ["audio"],
                "audio": {
                    "output": {
                        "format": { "type": "audio/pcm", "rate": crate::voice::OPENAI_PCM_RATE },
                        "voice": voice,
                    }
                }
            }
        }),
        json!({
            "type": "conversation.item.create",
            "item": {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }]
            }
        }),
        json!({ "type": "response.create" }),
    ]
}

/// What one server event means for the collector.
#[derive(Debug, PartialEq, Eq)]
pub enum ServerEvent {
    Audio(Vec<u8>),
    Done,
    Failed(String),
    Ignore,
}

/// Classifies one server event. Both the GA (`response.output_audio.delta`) and the beta
/// (`response.audio.delta`) names are accepted, since the voice backend has served both.
pub fn classify(event: &Value) -> ServerEvent {
    match event.get("type").and_then(Value::as_str).unwrap_or_default() {
        "response.output_audio.delta" | "response.audio.delta" => event
            .get("delta")
            .and_then(Value::as_str)
            .and_then(|delta| STANDARD.decode(delta).ok())
            .map(ServerEvent::Audio)
            .unwrap_or(ServerEvent::Ignore),
        "response.done" => match event.pointer("/response/status").and_then(Value::as_str) {
            Some("failed") => ServerEvent::Failed(
                event
                    .pointer("/response/status_details/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the realtime response failed")
                    .to_owned(),
            ),
            _ => ServerEvent::Done,
        },
        "error" => ServerEvent::Failed(
            event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("the realtime session reported an error")
                .to_owned(),
        ),
        _ => ServerEvent::Ignore,
    }
}

/// Whether spoken replies can run: the realtime voice needs the same Codex sign-in as dictation.
pub fn available() -> bool {
    codex_speech::available()
}

/// Speaks `text` with `voice` and returns raw PCM16 mono at [`crate::voice::OPENAI_PCM_RATE`].
pub async fn synthesize_pcm(model: &str, voice: &str, text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let login = codex_speech::login().ok_or("no Codex sign-in on this machine; run `codex login`")?;
    let mut request = realtime_url(model)
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let headers = request.headers_mut();
    let bearer = HeaderValue::from_str(&format!("Bearer {}", login.access_token()))
        .map_err(|_| "the Codex sign-in token is not a valid header")?;
    headers.insert("authorization", bearer);
    if let Some(account) = login.account_id() {
        if let Ok(value) = HeaderValue::from_str(account) {
            headers.insert("chatgpt-account-id", value);
        }
    }
    headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));

    tokio::time::timeout(SESSION_TIMEOUT, run_session(request, voice, text))
        .await
        .map_err(|_| "the realtime voice timed out".to_owned())?
}

async fn run_session(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    voice: &str,
    text: &str,
) -> Result<Vec<u8>, String> {
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                let status = response.status().as_u16();
                if status == 401 || status == 403 {
                    "the Codex sign-in was rejected by the realtime voice; run `codex login` again".to_owned()
                } else {
                    format!("the realtime voice refused the connection ({status})")
                }
            }
            other => other.to_string(),
        })?;
    for event in session_events(voice, text) {
        socket
            .send(Message::text(event.to_string()))
            .await
            .map_err(|error| error.to_string())?;
    }
    let mut pcm = Vec::new();
    while let Some(frame) = socket.next().await {
        let frame = frame.map_err(|error| error.to_string())?;
        let payload = match frame {
            Message::Text(text) => text.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(event) = serde_json::from_str::<Value>(&payload) else { continue };
        match classify(&event) {
            ServerEvent::Audio(bytes) => {
                if pcm.len() + bytes.len() > MAX_PCM_BYTES {
                    let _ = socket.close(None).await;
                    return Err("the realtime reply was longer than a spoken receipt".into());
                }
                pcm.extend_from_slice(&bytes);
            }
            ServerEvent::Done => break,
            ServerEvent::Failed(message) => {
                let _ = socket.close(None).await;
                return Err(message);
            }
            ServerEvent::Ignore => {}
        }
    }
    let _ = socket.close(None).await;
    if pcm.is_empty() {
        return Err("the realtime voice returned no audio".into());
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_carries_the_codex_voice_intent_and_an_encoded_model() {
        assert_eq!(
            realtime_url("gpt-realtime-1.5"),
            "wss://api.openai.com/v1/realtime?intent=quicksilver&model=gpt-realtime-1.5"
        );
        assert!(realtime_url("a b&c").ends_with("model=a%20b%26c"));
    }

    #[test]
    fn the_session_is_audio_only_at_the_playback_rate_with_the_chosen_voice() {
        let [update, item, response] = session_events("alloy", "Done.");
        assert_eq!(update["session"]["output_modalities"], json!(["audio"]));
        assert_eq!(update["session"]["audio"]["output"]["voice"], "alloy");
        assert_eq!(update["session"]["audio"]["output"]["format"]["rate"], 24_000);
        assert_eq!(item["item"]["content"][0]["text"], "Done.");
        assert_eq!(response["type"], "response.create");
    }

    #[test]
    fn both_audio_delta_names_decode_to_pcm() {
        let delta = STANDARD.encode([1u8, 2, 3, 4]);
        for name in ["response.output_audio.delta", "response.audio.delta"] {
            assert_eq!(
                classify(&json!({ "type": name, "delta": delta })),
                ServerEvent::Audio(vec![1, 2, 3, 4])
            );
        }
    }

    #[test]
    fn errors_and_failed_responses_end_the_session_with_their_message() {
        assert_eq!(
            classify(&json!({ "type": "error", "error": { "message": "bad" } })),
            ServerEvent::Failed("bad".into())
        );
        assert_eq!(
            classify(&json!({ "type": "response.done", "response": { "status": "failed",
                "status_details": { "error": { "message": "quota" } } } })),
            ServerEvent::Failed("quota".into())
        );
        assert_eq!(
            classify(&json!({ "type": "response.done", "response": { "status": "completed" } })),
            ServerEvent::Done
        );
        assert_eq!(classify(&json!({ "type": "session.created" })), ServerEvent::Ignore);
    }
}
