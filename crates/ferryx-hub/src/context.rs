//! Context pack and the tool names the gateway loop exposes.
pub const TOOLS: &[&str] = &[
    "browser",
    "cron",
    "file",
    "mcp",
    "message_context",
    "skills",
    "terminal",
    "web",
];

pub const MAX_MESSAGE_CHARS: usize = 2_000;
pub const MAX_PACK_CHARS: usize = 4_000;
const MAX_LINE_CHARS: usize = 300;

/// Renders the sections a turn's context pack carries: lanes, precedents, and
/// thread lines. Each line is one physical line, capped, and the whole pack stops
/// at `max_chars`.
pub fn render_pack(sections: &[(&str, &[String])], max_chars: usize) -> String {
    let mut out = String::new();
    for (header, lines) in sections {
        push_pack_line(&mut out, header, max_chars);
        if lines.is_empty() {
            push_pack_line(&mut out, "- none", max_chars);
            continue;
        }
        for line in *lines {
            push_pack_line(&mut out, &one_pack_line(line), max_chars);
        }
    }
    out
}

fn one_pack_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_LINE_CHARS)
        .collect()
}

fn push_pack_line(out: &mut String, line: &str, max_chars: usize) {
    if out.chars().count() >= max_chars {
        return;
    }
    let room = max_chars.saturating_sub(out.chars().count());
    let text = format!("{line}\n");
    out.extend(text.chars().take(room));
}

/// Caps each message and prefixes untrusted text so it cannot look like a new instruction.
pub fn pack(messages: &[String], memory: &[String], max_chars: usize) -> String {
    let mut out = String::new();
    for item in memory {
        push_capped(&mut out, "memory", item, max_chars);
    }
    for item in messages {
        push_capped(&mut out, "message", item, max_chars);
    }
    out
}

/// Pages one context pack reads per thread before it stops.
pub const MAX_REPLIES_PAGES: usize = 20;

/// Slack `conversations.replies` walk. Pages are deduped by `ts`. A real
/// `next_cursor` continues the walk; a repeated cursor stops it. With no cursor,
/// a page that adds nothing new is the end, and the walk also stops at 20 pages.
#[derive(Default)]
pub struct ReplyWalk {
    cursor: Option<String>,
    newest: Option<(u64, u64)>,
    newest_raw: Option<String>,
    seen: std::collections::HashSet<String>,
    pages: usize,
}

impl ReplyWalk {
    pub fn oldest(&self) -> Option<&str> {
        if self.cursor.is_some() {
            return None;
        }
        self.newest_raw.as_deref()
    }

    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }
}

fn ts_parts(ts: &str) -> Option<(u64, u64)> {
    let (secs, frac) = ts.split_once('.')?;
    let secs: u64 = secs.parse().ok()?;
    let padded = format!("{frac:0<6}");
    Some((secs, padded.get(..6)?.parse().ok()?))
}

/// Returns true when another page should be requested.
pub fn paginate_replies(
    all_replies: &mut Vec<serde_json::Value>,
    chunk: Vec<serde_json::Value>,
    walk: &mut ReplyWalk,
    next_cursor: Option<String>,
) -> bool {
    walk.pages += 1;
    let mut fresh = 0usize;
    for message in chunk {
        let Some(ts) = message
            .get("ts")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if !walk.seen.insert(ts.clone()) {
            continue;
        }
        fresh += 1;
        if let Some(parts) = ts_parts(&ts) {
            if walk.newest.is_none_or(|newest| parts > newest) {
                walk.newest = Some(parts);
                walk.newest_raw = Some(ts.clone());
            }
        }
        all_replies.push(message);
    }
    if walk.pages >= MAX_REPLIES_PAGES {
        return false;
    }
    match next_cursor {
        Some(cursor) if walk.cursor.as_deref() != Some(cursor.as_str()) => {
            walk.cursor = Some(cursor);
            true
        }
        Some(_) => false,
        None if walk.cursor.is_some() => false,
        None => fresh > 0,
    }
}

/// Live `conversations.replies`. `base` is `https://slack.com/api` unless a test overrides it.
/// Pages stop on the same cursor rules as [`paginate_replies`].
pub async fn conversations_replies(
    base: &str,
    token: &str,
    channel: &str,
    ts: &str,
) -> crate::error::Result<Vec<serde_json::Value>> {
    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    {
        fetch_replies(base, token, channel, ts).await
    }
    #[cfg(not(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice")))]
    {
        let _ = (base, token, channel, ts);
        Err(crate::error::HubError::message(
            "conversations.replies needs hub-slack, hub-discord, or hub-voice",
        ))
    }
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
async fn fetch_replies(
    base: &str,
    token: &str,
    channel: &str,
    ts: &str,
) -> crate::error::Result<Vec<serde_json::Value>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| crate::error::HubError::message(error.to_string()))?;
    let mut all = Vec::new();
    let mut walk = ReplyWalk::default();
    let mut cursor: Option<String> = None;
    loop {
        let mut url = format!(
            "{}/conversations.replies?channel={}&ts={}&limit=200",
            base.trim_end_matches('/'),
            urlencoding_query(channel),
            urlencoding_query(ts)
        );
        if let Some(cursor) = cursor.as_deref() {
            url.push_str("&cursor=");
            url.push_str(&urlencoding_query(cursor));
        }
        let response = client
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| crate::error::HubError::message(error.to_string()))?;
        if body.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
            let err = body
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            return Err(crate::error::HubError::message(format!(
                "conversations.replies {err}"
            )));
        }
        let messages = body
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let next = body
            .pointer("/response_metadata/next_cursor")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);
        let again = paginate_replies(&mut all, messages, &mut walk, next.clone());
        if !again {
            break;
        }
        cursor = next;
    }
    Ok(all)
}

#[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
fn urlencoding_query(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Reads `pages` of `{messages, next_cursor}` the way a Slack thread is walked.
pub fn walk_pages(pages: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut all = Vec::new();
    let mut walk = ReplyWalk::default();
    let Some(pages) = pages.get("pages").and_then(serde_json::Value::as_array) else {
        return all;
    };
    for page in pages {
        let messages = page
            .get("messages")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let cursor = page
            .get("next_cursor")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if !paginate_replies(&mut all, messages, &mut walk, cursor) {
            break;
        }
    }
    all
}

fn push_capped(out: &mut String, kind: &str, text: &str, max_chars: usize) {
    if out.chars().count() >= max_chars {
        return;
    }
    let capped: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    let line = format!("{kind}: {capped}\n");
    let room = max_chars.saturating_sub(out.chars().count());
    out.extend(line.chars().take(room));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pack_caps_messages_and_names_the_gateway_tools() {
        assert!(TOOLS.contains(&"message_context"));
        let long = "x".repeat(5_000);
        let packed = pack(&[long], &["remember".into()], 100);
        assert!(packed.chars().count() <= 100);
        assert!(packed.contains("memory: remember"));
        let rendered = render_pack(
            &[
                ("Precedents:", &["allow git status".into()]),
                ("Lanes in this thread:", &[]),
            ],
            MAX_PACK_CHARS,
        );
        assert!(rendered.contains("Precedents:\nallow git status\n"));
        assert!(rendered.contains("Lanes in this thread:\n- none\n"));
    }

    #[test]
    fn slack_thread_pages_dedupe_the_parent_and_follow_the_cursor() {
        use serde_json::json;
        let mut all = Vec::new();
        let mut walk = ReplyWalk::default();
        let parent = json!({"ts": "100.000001", "text": "root"});
        let reply = json!({"ts": "100.000002", "text": "one"});
        assert!(paginate_replies(
            &mut all,
            vec![parent.clone(), reply.clone()],
            &mut walk,
            Some("cursor-2".into()),
        ));
        assert!(!paginate_replies(
            &mut all,
            vec![parent, reply, json!({"ts": "100.000003", "text": "two"})],
            &mut walk,
            Some("cursor-2".into()),
        ));
        assert_eq!(all.len(), 3);
        assert_eq!(walk.cursor(), Some("cursor-2"));
        let mut stopped = ReplyWalk::default();
        let mut collected = Vec::new();
        for index in 1..=MAX_REPLIES_PAGES {
            let keep_going = paginate_replies(
                &mut collected,
                vec![json!({"ts": format!("200.{index:06}")})],
                &mut stopped,
                None,
            );
            if !keep_going {
                break;
            }
        }
        assert_eq!(collected.len(), MAX_REPLIES_PAGES);
    }

    #[cfg(any(feature = "hub-slack", feature = "hub-discord", feature = "hub-voice"))]
    #[tokio::test]
    async fn conversations_replies_walks_every_cursor_page() {
        use serde_json::json;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = axum::Router::new().route(
            "/conversations.replies",
            axum::routing::get(|request: axum::extract::Request| async move {
                let query = request.uri().query().unwrap_or("");
                let body = if query.contains("cursor=page-2") {
                    json!({"ok": true, "messages": [{"ts": "100.000002", "text": "one"}]})
                } else {
                    json!({
                        "ok": true,
                        "messages": [{"ts": "100.000001", "text": "root"}],
                        "response_metadata": {"next_cursor": "page-2"}
                    })
                };
                axum::Json(body)
            }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let messages = conversations_replies(
            &format!("http://127.0.0.1:{port}"),
            "xoxb-test",
            "C0",
            "100.000001",
        )
        .await
        .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["text"], "one");
    }
}
