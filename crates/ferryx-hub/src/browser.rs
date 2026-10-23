//! Chrome DevTools browser. omo drives Camofox, the signed-in Chrome, through CDP on port 9333.
//! `navigate` opens a public http(s) URL and `snapshot` lists tabs this tool opened.
//! `eval` and `screenshot` need a CDP WebSocket session, which this gateway does not open.
#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
use std::collections::HashSet;
#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
use std::sync::Mutex;

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
use serde_json::json;
use serde_json::Value;

use crate::error::{HubError, Result};

const DEFAULT_PORT: u16 = 9333;

pub fn cdp_port() -> u16 {
    crate::hub_env::var("FERRYX_HUB_CDP_PORT")
        .ok()
        .or_else(|| crate::hub_env::var("OMON_BROWSER_CDP_PORT").ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
fn owned_tabs() -> &'static Mutex<HashSet<String>> {
    static TABS: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    TABS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn public_http_url(raw: &str) -> Result<String> {
    let parsed = reqwest_url(raw)?;
    if !matches!(parsed.0.as_str(), "http" | "https") {
        return Err(HubError::message(
            "browser navigation only supports http and https URLs",
        ));
    }
    if is_internal_host(&parsed.1) {
        return Err(HubError::message(
            "browser navigation refuses loopback and private hosts",
        ));
    }
    Ok(parsed.2)
}

fn reqwest_url(raw: &str) -> Result<(String, String, String)> {
    let (scheme, rest) = raw
        .split_once("://")
        .ok_or_else(|| HubError::message(format!("invalid URL: {raw}")))?;
    let hostport = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = hostport
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(hostport);
    let host = host.strip_prefix('[').unwrap_or(host);
    let host = host.split([']', ':']).next().unwrap_or(host);
    if host.is_empty() {
        return Err(HubError::message(format!("invalid URL: {raw}")));
    }
    Ok((
        scheme.to_ascii_lowercase(),
        host.to_ascii_lowercase(),
        raw.to_owned(),
    ))
}

fn is_internal_host(host: &str) -> bool {
    if matches!(
        host,
        "localhost" | "localhost." | "0.0.0.0" | "127.0.0.1" | "::1" | "metadata.google.internal"
    ) {
        return true;
    }
    if let Ok(addr) = host.parse::<std::net::Ipv4Addr>() {
        return addr.is_loopback()
            || addr.is_private()
            || addr.is_link_local()
            || addr.is_unspecified()
            || addr.is_broadcast();
    }
    if let Ok(addr) = host.parse::<std::net::Ipv6Addr>() {
        return addr.is_loopback() || addr.is_unspecified() || is_unique_local(&addr);
    }
    false
}

fn is_unique_local(addr: &std::net::Ipv6Addr) -> bool {
    (addr.segments()[0] & 0xfe00) == 0xfc00
}

/// Runs one browser action against the CDP HTTP endpoint on `port`.
pub async fn execute_at(port: u16, args: &Value) -> Result<Value> {
    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    {
        execute_cdp(port, args).await
    }
    #[cfg(not(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice")))]
    {
        let _ = (port, args);
        Err(HubError::message(
            "browser CDP client needs hub-slack, hub-discord, or hub-voice",
        ))
    }
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
async fn execute_cdp(port: u16, args: &Value) -> Result<Value> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| HubError::message("missing 'action'"))?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| HubError::message(error.to_string()))?;
    let list_url = format!("http://127.0.0.1:{port}/json/list");
    let resp =
        client.get(&list_url).send().await.map_err(|error| {
            HubError::message(format!("CDP not reachable on port {port}: {error}"))
        })?;
    if !resp.status().is_success() {
        return Err(HubError::message(format!(
            "CDP returned {} for page listing",
            resp.status()
        )));
    }
    let pages: Value = resp
        .json()
        .await
        .map_err(|error| HubError::message(format!("invalid CDP response: {error}")))?;
    match action {
        "snapshot" => {
            let listed = pages.as_array().cloned().unwrap_or_default();
            let owned = owned_tabs()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mine: Vec<Value> = listed
                .into_iter()
                .filter(|page| {
                    page.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| owned.contains(id))
                })
                .collect();
            let count = mine.len();
            Ok(json!({"cdp_port": port, "active_pages_count": count, "pages": mine}))
        }
        "navigate" => {
            let url = args
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| HubError::message("missing 'url'"))?;
            let url = public_http_url(url)?;
            let open = format!(
                "http://127.0.0.1:{port}/json/new?{}",
                urlencoding_query(&url)
            );
            let opened =
                client.put(&open).send().await.map_err(|error| {
                    HubError::message(format!("failed to open new tab: {error}"))
                })?;
            if !opened.status().is_success() {
                return Err(HubError::message(format!(
                    "CDP returned {} while opening a tab",
                    opened.status()
                )));
            }
            let tab: Value = opened
                .json()
                .await
                .map_err(|error| HubError::message(format!("invalid CDP tab response: {error}")))?;
            let Some(tab_id) = tab.get("id").and_then(Value::as_str).map(str::to_owned) else {
                return Err(HubError::message(
                    "CDP did not return a tab id, so the final URL cannot be verified",
                ));
            };
            owned_tabs()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(tab_id);
            Ok(json!({"status": "navigated", "url": url, "tab": tab}))
        }
        "eval" | "screenshot" => Err(HubError::message(format!(
            "browser action `{action}` requires a CDP WebSocket session and is not configured"
        ))),
        other => Err(HubError::message(format!(
            "unknown browser action: {other}"
        ))),
    }
}

pub async fn execute(args: &Value) -> Result<Value> {
    execute_at(cdp_port(), args).await
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
fn urlencoding_query(url: &str) -> String {
    let mut out = String::new();
    for byte in url.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_are_refused_and_public_https_is_kept() {
        assert!(public_http_url("http://127.0.0.1/secret").is_err());
        assert!(public_http_url("https://localhost/x").is_err());
        assert!(public_http_url("https://10.1.2.3/x").is_err());
        assert!(public_http_url("https://192.168.1.9/x").is_err());
        assert!(public_http_url("file:///etc/passwd").is_err());
        assert_eq!(
            public_http_url("https://example.com/watch").unwrap(),
            "https://example.com/watch"
        );
    }

    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    #[tokio::test]
    async fn navigate_opens_a_cdp_tab_and_snapshot_lists_only_that_tab() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let pages = std::sync::Arc::new(Mutex::new(Vec::<Value>::new()));
        let recorded = pages.clone();
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let recorded = recorded.clone();
            async move {
                let path = request.uri().path().to_owned();
                let query = request.uri().query().unwrap_or("").to_owned();
                if path == "/json/list" {
                    let body = serde_json::to_string(&*recorded.lock().unwrap()).unwrap();
                    return axum::response::Response::builder()
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap();
                }
                if path == "/json/new" {
                    let url = urlencoding_decode(&query);
                    let tab = json!({"id": "tab-1", "url": url, "title": "watch"});
                    recorded.lock().unwrap().push(tab.clone());
                    return axum::response::Response::builder()
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(tab.to_string()))
                        .unwrap();
                }
                axum::response::Response::builder()
                    .status(404)
                    .body(axum::body::Body::from("missing"))
                    .unwrap()
            }
        });
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        owned_tabs().lock().unwrap().clear();
        let opened = execute_at(
            port,
            &json!({"action": "navigate", "url": "https://example.com/watch"}),
        )
        .await
        .unwrap();
        assert_eq!(opened["status"], "navigated");
        assert_eq!(opened["tab"]["id"], "tab-1");
        let snap = execute_at(port, &json!({"action": "snapshot"}))
            .await
            .unwrap();
        assert_eq!(snap["active_pages_count"], 1);
        assert_eq!(snap["pages"][0]["url"], "https://example.com/watch");
        let eval = execute_at(port, &json!({"action": "eval", "script": "1"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(eval.contains("CDP WebSocket session"));
        let blocked = execute_at(
            port,
            &json!({"action": "navigate", "url": "http://127.0.0.1/admin"}),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(blocked.contains("private"));
    }

    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    fn urlencoding_decode(query: &str) -> String {
        let bytes = query.as_bytes();
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'%' if index + 2 < bytes.len() => {
                    let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("00");
                    out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                    index += 3;
                }
                other => {
                    out.push(other);
                    index += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}
