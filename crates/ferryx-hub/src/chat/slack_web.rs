//! Slack Web API calls whose answer matters (a posted `ts`, a permalink, a thread root).
//! Same base URL and token as the rest of the Slack transport, so tests point it at a mock.
use serde_json::Value;

use crate::error::{HubError, Result};

fn client(base: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15));
    let host = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
        .unwrap_or(base);
    if host.starts_with("127.0.0.1") || host.starts_with("localhost") || host.starts_with("[::1]") {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|error| HubError::message(error.to_string()))
}

fn checked(method: &str, value: Value) -> Result<Value> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        return Err(HubError::message(format!("slack {method} failed: {error}")));
    }
    Ok(value)
}

fn token() -> Result<String> {
    super::slack::bot_token().ok_or_else(|| HubError::message("SLACK_BOT_TOKEN is not set"))
}

/// A write method with a JSON body (`chat.postMessage`, `chat.update`).
pub async fn post_json(method: &str, body: &Value) -> Result<Value> {
    let token = token()?;
    let base = super::slack::api_base();
    let value: Value = client(&base)?
        .post(format!("{base}/{method}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    checked(method, value)
}

/// A read method, which Slack only takes form-encoded (`conversations.replies`, `chat.getPermalink`).
pub async fn post_form(method: &str, fields: &[(&str, &str)]) -> Result<Value> {
    let token = token()?;
    let base = super::slack::api_base();
    let value: Value = client(&base)?
        .post(format!("{base}/{method}"))
        .bearer_auth(token)
        .form(fields)
        .send()
        .await
        .map_err(|error| HubError::message(error.to_string()))?
        .json()
        .await
        .map_err(|error| HubError::message(error.to_string()))?;
    checked(method, value)
}

/// Posts a top-level message and returns its `ts`.
pub async fn post_message(channel: &str, text: &str) -> Result<String> {
    let value = post_json(
        "chat.postMessage",
        &serde_json::json!({"channel": channel, "text": text}),
    )
    .await?;
    value
        .get("ts")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| HubError::message("slack chat.postMessage returned no ts"))
}

pub async fn update_message(channel: &str, ts: &str, text: &str) -> Result<()> {
    post_json(
        "chat.update",
        &serde_json::json!({"channel": channel, "ts": ts, "text": text}),
    )
    .await?;
    Ok(())
}

pub async fn permalink(channel: &str, ts: &str) -> Result<String> {
    let value = post_form(
        "chat.getPermalink",
        &[("channel", channel), ("message_ts", ts)],
    )
    .await?;
    value
        .get("permalink")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| HubError::message("slack chat.getPermalink returned no permalink"))
}

/// The thread's root message (the first entry of `conversations.replies`).
pub async fn thread_root(channel: &str, ts: &str) -> Result<Option<Value>> {
    let value = post_form(
        "conversations.replies",
        &[
            ("channel", channel),
            ("ts", ts),
            ("limit", "1"),
            ("inclusive", "true"),
        ],
    )
    .await?;
    Ok(value
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| messages.first().cloned()))
}

/// A mock Slack Web API for tests: records `(method, body)` for every call.
#[cfg(test)]
pub mod mock {
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    pub type Calls = Arc<Mutex<Vec<(String, Value)>>>;

    #[derive(Clone)]
    struct State {
        calls: Calls,
        bot_user: String,
        roots: Arc<Mutex<std::collections::HashMap<String, String>>>,
    }

    async fn record(
        axum::extract::State(state): axum::extract::State<State>,
        axum::extract::Path(method): axum::extract::Path<String>,
        headers: axum::http::HeaderMap,
        body: axum::body::Bytes,
    ) -> axum::Json<Value> {
        let form = headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"));
        let body: Value = if form {
            Value::Object(
                String::from_utf8_lossy(&body)
                    .split('&')
                    .filter_map(|pair| pair.split_once('='))
                    .map(|(k, v)| (decode(k), json!(decode(v))))
                    .collect(),
            )
        } else {
            serde_json::from_slice(&body).unwrap_or(Value::Null)
        };
        state
            .calls
            .lock()
            .unwrap()
            .push((method.clone(), body.clone()));
        let answer = match method.as_str() {
            "chat.postMessage" => {
                let ts = format!("1700000000.{:06}", state.calls.lock().unwrap().len());
                if let Some(text) = body["text"].as_str() {
                    state
                        .roots
                        .lock()
                        .unwrap()
                        .insert(ts.clone(), text.to_owned());
                }
                json!({"ok": true, "ts": ts, "channel": body["channel"]})
            }
            "chat.update" => {
                if let (Some(ts), Some(text)) = (body["ts"].as_str(), body["text"].as_str()) {
                    state
                        .roots
                        .lock()
                        .unwrap()
                        .insert(ts.to_owned(), text.to_owned());
                }
                json!({"ok": true})
            }
            "chat.getPermalink" => json!({
                "ok": true,
                "permalink": format!(
                    "https://acme.slack.com/archives/{}/p{}",
                    body["channel"].as_str().unwrap_or(""),
                    body["message_ts"].as_str().unwrap_or("").replace('.', "")
                )
            }),
            "conversations.replies" => {
                let ts = body["ts"].as_str().unwrap_or("").to_owned();
                match state.roots.lock().unwrap().get(&ts) {
                    Some(text) => {
                        json!({"ok": true, "messages": [{"ts": ts, "user": state.bot_user, "text": text}]})
                    }
                    None => {
                        json!({"ok": true, "messages": [{"ts": ts, "user": "USOMEONE", "text": "hello"}]})
                    }
                }
            }
            _ => json!({"ok": true, "ts": "1700000000.999999"}),
        };
        axum::Json(answer)
    }

    fn decode(raw: &str) -> String {
        let bytes = raw.replace('+', " ").into_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(byte) =
                    u8::from_str_radix(&String::from_utf8_lossy(&bytes[i + 1..i + 3]), 16)
                {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Serves the mock on a loopback port; returns its base URL and the call log.
    pub async fn serve(bot_user: &str) -> (String, Calls) {
        let calls: Calls = Arc::new(Mutex::new(Vec::new()));
        let app = axum::Router::new()
            .route("/{method}", axum::routing::post(record))
            .with_state(State {
                calls: calls.clone(),
                bot_user: bot_user.to_owned(),
                roots: Arc::default(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://127.0.0.1:{port}"), calls)
    }
}
