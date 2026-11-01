//! End-to-end checks of the omo-gateway behaviours ported onto the 1022.8 Slack turn path:
//! approval cards, dashboard chat, channel context, attachments, forwarded messages and the
//! missed-message backfill. Slack is a loopback stand-in; mhc is an executable JSONL peer.
use serde_json::{json, Value};

use super::session::SessionKey;
use super::slack::with_test_api;
use super::slack_turn::{
    admit, prepare_prompt_in, run_dashboard_turn, run_session_turn, AllowBots, SessionTurn,
    SlackTurnConfig,
};
use super::slack_web::mock::Calls;
use crate::meta::omo_client::{OmoClientConfig, FULL_ACCESS_TEST};

use crate::meta::rpc_mock::{serve, MockLog, Script};

const PUSH: &str = "git push origin main";

fn slack_changes() -> &'static tokio::sync::watch::Sender<u64> {
    static CHANGES: std::sync::OnceLock<tokio::sync::watch::Sender<u64>> =
        std::sync::OnceLock::new();
    CHANGES.get_or_init(|| tokio::sync::watch::channel(0).0)
}

// Notify only after the underlying mock has recorded a request. This lets tests subscribe
// before checking the log, without fixed sleeps or polling for asynchronous side effects.
async fn serve_scripted(bot: &str, script: Vec<(&str, Vec<Value>)>) -> (String, Calls) {
    let (upstream, calls) = super::slack_web::mock::serve_scripted(bot, script).await;
    let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let upstream = upstream.clone();
        async move {
            let (parts, body) = request.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
            let response = reqwest::Client::new()
                .request(parts.method, format!("{upstream}{}", parts.uri))
                .headers(parts.headers)
                .body(bytes)
                .send()
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = response.bytes().await.unwrap();
            slack_changes().send_modify(|version| *version += 1);
            let mut response = axum::response::Response::new(axum::body::Body::from(bytes));
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            response
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, calls)
}

fn config(omo_url: String) -> SlackTurnConfig {
    SlackTurnConfig {
        omo: OmoClientConfig::new(omo_url),

        allowed_users: vec!["UOWNER".into()],
        free_channels: vec!["CHOME".into()],
        default_model: None,
        allowed_channels: Vec::new(),
        ignored_channels: Vec::new(),
        thread_require_mention: false,
        auto_thread: true,
        allow_bots: AllowBots::None,
        allow_all_users: false,
    }
}

fn script(messages: &[&str], approval: bool) -> Script {
    Script {
        messages: messages.iter().map(|m| (*m).to_owned()).collect(),
        approval,
        live: false,
    }
}

fn calls_of(calls: &Calls, method: &str) -> Vec<Value> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(m, _)| m == method)
        .map(|(_, body)| body.clone())
        .collect()
}

fn approval_answer(log: &MockLog) -> Option<Value> {
    log.frames
        .lock()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "77" && f["type"] == "extension_ui_response")
        .cloned()
}

/// Waits for the approval card; returns (card ts, button value for `choice`). Subscribed to the
/// mock's call log before each check, so the card's own post wakes it.
async fn wait_card(calls: &Calls, choice: &str) -> (String, String) {
    let (index, card) = wait_call(calls, "an approval card", |method, body| {
        method == "chat.postMessage" && body.get("blocks").is_some()
    })
    .await;
    let value = card["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|block| block["elements"].as_array().cloned().unwrap_or_default())
        .filter_map(|element| element["value"].as_str().map(str::to_owned))
        .find(|value| value.ends_with(&format!(":{choice}")))
        .expect("card has the button");
    // The mock numbers a posted message by its position in the call log.
    (format!("1700000000.{:06}", index + 1), value)
}

/// Waits until the mock has recorded a call matching `matches`; returns its index and body.
async fn wait_call(
    calls: &Calls,
    what: &str,
    matches: impl Fn(&str, &Value) -> bool,
) -> (usize, Value) {
    let mut changed = super::slack_web::mock::recorded();
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let found = calls
                .lock()
                .unwrap()
                .iter()
                .enumerate()
                .find(|(_, (method, body))| matches(method, body))
                .map(|(index, (_, body))| (index, body.clone()));
            if let Some(found) = found {
                return found;
            }
            changed.changed().await.expect("the call log outlives the test");
        }
    })
    .await;
    match found {
        Ok(found) => found,
        Err(_) => panic!("no {what}: {:?}", calls.lock().unwrap()),
    }
}

fn is_status_post(method: &str, body: &Value) -> bool {
    let text = body["text"].as_str().unwrap_or("");
    method == "chat.postMessage" && text.starts_with("작업 중:")
}

fn click(user: &str, ts: &str, value: &str, response_url: &str) -> Value {
    json!({
        "type": "block_actions",
        "user": {"id": user},
        "channel": {"id": "CHOME"},
        "container": {"message_ts": ts, "channel_id": "CHOME"},
        "response_url": response_url,
        "actions": [{"action_id": "approval", "value": value}]
    })
}

fn slack_turn(text: &str) -> SessionTurn {
    SessionTurn {
        key: SessionKey::new("slack", Some("T1"), "CHOME", Some("100.1"), "").with_bot_id("UBOT"),
        reply_channel: Some("CHOME".into()),
        reply_thread: Some("100.1".into()),
        status_ts: Some("100.1".into()),
        text: text.into(),
        background: false,
    }
}

#[tokio::test]
async fn e2e_full_access_off_posts_an_approval_card_and_waits_for_the_owner() {
    let (omo_url, log) = serve(script(&["pushed"], true)).await;
    let config = config(omo_url);
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let response_url = format!("{base}/response_url");
    let owners = config.allowed_users.clone();

    let (reply, ts) = with_test_api(base.clone(), "xoxb-test".into(), async {
        FULL_ACCESS_TEST
            .scope(false, async {
                assert!(crate::meta::omo_client::approval_verdict(
                    &json!({"type":"extension_ui_request", "id":"77", "method":"confirm", "title":"Run command", "message":PUSH}),
                    None
                )
                .is_some());
                let clicker = async {
                    let (ts, allow) = wait_card(&calls, "once").await;
                    // A stranger's click is refused ephemerally and decides nothing.
                    assert!(
                        super::turn_approval::handle_click(
                            &click("USTRANGER", &ts, &allow, &response_url),
                            &owners
                        )
                        .await
                    );
                    assert!(approval_answer(&log).is_none());
                    assert!(
                        super::turn_approval::handle_click(
                            &click("UOWNER", &ts, &allow, &response_url),
                            &owners
                        )
                        .await
                    );
                    ts
                };
                tokio::join!(run_session_turn(&config, &pool, slack_turn("push it")), clicker)
            })
            .await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("pushed"));
    let answer = approval_answer(&log).expect("omo got an answer");
    assert_eq!(answer["confirmed"], true, "{answer}");

    let card = calls_of(&calls, "chat.postMessage")
        .into_iter()
        .find(|b| b.get("blocks").is_some())
        .unwrap();
    assert_eq!(card["channel"], "CHOME");
    assert_eq!(card["thread_ts"], "100.1");
    assert!(card["blocks"].to_string().contains(PUSH));
    let ephemeral = calls_of(&calls, "response_url");
    assert_eq!(ephemeral.len(), 1, "{ephemeral:?}");
    assert_eq!(ephemeral[0]["response_type"], "ephemeral");
    let updates = calls_of(&calls, "chat.update");
    assert!(
        updates
            .iter()
            .any(|u| u["ts"] == ts.as_str() && u["text"] == "✅ Approved once by <@UOWNER>"),
        "{updates:?}"
    );
}

#[tokio::test]
async fn e2e_a_denied_card_declines_and_always_allow_skips_the_next_card() {
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let owners = vec!["UOWNER".to_owned()];

    // Deny: omo is told the owner declined.
    let (omo_url, log) = serve(script(&["could not push"], true)).await;
    let first = config(omo_url);
    with_test_api(base.clone(), "xoxb-test".into(), async {
        FULL_ACCESS_TEST
            .scope(false, async {
                let clicker = async {
                    let (ts, deny) = wait_card(&calls, "deny").await;
                    super::turn_approval::handle_click(&click("UOWNER", &ts, &deny, ""), &owners)
                        .await;
                };
                tokio::join!(run_session_turn(&first, &pool, slack_turn("push")), clicker);
            })
            .await
    })
    .await;
    let answer = approval_answer(&log).unwrap();
    assert_eq!(answer["confirmed"], false);

    // Always allow: stored in the ledger, so the next identical request gets no card.
    calls.lock().unwrap().clear();
    let (omo_url, log) = serve(script(&["pushed"], true)).await;
    let second = config(omo_url);
    with_test_api(base.clone(), "xoxb-test".into(), async {
        FULL_ACCESS_TEST
            .scope(false, async {
                let clicker = async {
                    let (ts, always) = wait_card(&calls, "always").await;
                    super::turn_approval::handle_click(&click("UOWNER", &ts, &always, ""), &owners)
                        .await;
                };
                tokio::join!(
                    run_session_turn(&second, &pool, slack_turn("push")),
                    clicker
                );
            })
            .await
    })
    .await;
    assert_eq!(approval_answer(&log).unwrap()["confirmed"], true);
    calls.lock().unwrap().clear();
    let (omo_url, log) = serve(script(&["pushed again"], true)).await;
    let third = config(omo_url);
    let reply = with_test_api(base, "xoxb-test".into(), async {
        FULL_ACCESS_TEST
            .scope(false, run_session_turn(&third, &pool, slack_turn("push")))
            .await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("pushed again"));
    assert_eq!(approval_answer(&log).unwrap()["confirmed"], true);
    assert!(calls_of(&calls, "chat.postMessage")
        .iter()
        .all(|b| b.get("blocks").is_none()));
}

#[tokio::test]
async fn an_unanswered_card_expires_and_a_stale_click_clears_it() {
    use crate::meta::omo_client::{Approver, OwnerAnswer};
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let mut approver = super::turn_approval::SlackApprover::new(
        pool,
        "slack:expiry".into(),
        "CHOME".into(),
        Some("100.1".into()),
    );
    approver.timeout = std::time::Duration::from_millis(100);
    let frame = json!({"type":"extension_ui_request", "method":"confirm", "id":"5", "title":"Run command", "message":PUSH});
    let answer = with_test_api(base.clone(), "xoxb-test".into(), async {
        let answer = approver.ask(&frame, None, "needs approval").await;
        let (ts, once) = wait_card(&calls, "once").await;
        wait_call(&calls, "expired update", |method, _| method == "chat.update").await;
        assert!(
            super::turn_approval::handle_click(
                &click("UOWNER", &ts, &once, ""),
                &["UOWNER".into()]
            )
            .await
        );
        answer
    })
    .await;
    assert_eq!(answer, OwnerAnswer::Unanswered);
    let updates = calls_of(&calls, "chat.update");
    assert_eq!(updates.len(), 2, "{updates:?}");
    assert_eq!(updates[0]["text"], super::turn_approval::EXPIRED_TEXT);
    assert_eq!(updates[1]["text"], super::turn_approval::STALE_APPROVAL);
}

#[tokio::test]
async fn e2e_dashboard_chat_runs_an_omo_turn_for_web_and_slack_sessions() {
    let (omo_url, log) = serve(script(&["working", "done: 2 files"], false)).await;
    let first = config(omo_url);
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;

    // A dashboard-only session: no Slack traffic, the transcript carries both sides.
    let web = SessionKey::new("web", None::<String>, "dash-1", None::<String>, "dashboard");
    let reply = with_test_api(base.clone(), "xoxb-test".into(), async {
        run_dashboard_turn(&first, &pool, web.clone(), "summarise the repo".into()).await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("done: 2 files"));
    assert!(calls_of(&calls, "chat.postMessage").is_empty());
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM messages WHERE session_key = ? ORDER BY sequence",
    )
    .bind(web.storage_key())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows.first().unwrap(),
        &("user".to_owned(), "summarise the repo".to_owned())
    );
    assert_eq!(
        rows.last().unwrap(),
        &("assistant".to_owned(), "done: 2 files".to_owned())
    );
    let turn_start = log
        .frames
        .lock()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "prompt")
        .cloned()
        .unwrap();
    assert_eq!(turn_start["message"], "summarise the repo");

    // A Slack session typed into from the dashboard answers in its Slack thread.
    let (omo_url, _log) = serve(script(&["확인하겠습니다.", "파일 두 개를 수정했습니다."], false)).await;
    let second = config(omo_url);
    let slack =
        SessionKey::new("slack", Some("T1"), "CHOME", Some("200.1"), "").with_bot_id("UBOT");
    let reply = with_test_api(base, "xoxb-test".into(), async {
        run_dashboard_turn(&second, &pool, slack, "follow up".into()).await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("파일 두 개를 수정했습니다."));
    let posts = calls_of(&calls, "chat.postMessage");
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(posts
        .iter()
        .all(|p| p["channel"] == "CHOME" && p["thread_ts"] == "200.1"));
    assert_eq!(posts[0]["text"], "파일 두 개를 수정했습니다.");
}

#[tokio::test]
async fn e2e_a_mention_carries_topic_channel_context_forwards_and_attachments() {
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted(
        "UBOT",
        vec![
            (
                "conversations.info",
                vec![json!({"ok": true, "channel": {"topic": {"value": "release train"}}})],
            ),
            (
                "conversations.history",
                vec![json!({"ok": true, "messages": [
                    {"ts": "1761190800.000100", "user": "UOWNER", "text": "the message itself"},
                    {"ts": "1761190790.000000", "user": "UALICE", "text": "build 42 is red"},
                    {"ts": "1761190780.000000", "user": "UBOT", "text": "my own earlier reply"}
                ]})],
            ),
            (
                "users.info",
                vec![json!({"ok": true, "user": {"profile": {"display_name": "alice"}}})],
            ),
        ],
    )
    .await;
    let event = json!({
        "type": "app_mention",
        "user": "UOWNER",
        "channel": "CHOME",
        "ts": "1761190800.000100",
        "text": "<@UBOT> what broke?",
        "files": [{"id": "F1", "name": "log.txt", "mimetype": "text/plain", "size": 20,
                   "url_private": format!("{base}/files/log.txt")}],
        "attachments": [{"is_share": true, "author_name": "Kim", "text": "deploy failed at step 3"}]
    });
    let payload = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": event});
    let admitted = admit(&config("unused-mhc".into()), &payload, "UBOT", false).unwrap();
    let dir = std::env::temp_dir().join(format!("ferryx-ingress-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let prompt = with_test_api(base.clone(), "xoxb-test".into(), async {
        prepare_prompt_in(&pool, &admitted, &dir).await
    })
    .await;
    let local = dir.join("CHOME").join("1761190800.000100-log.txt");
    assert_eq!(
        std::fs::read_to_string(&local).unwrap(),
        "contents of log.txt"
    );
    let expected = format!(
        "[Thu 2025-10-23 03:40:00 UTC] [Recent channel context]\nalice: build 42 is red\n\n\
         [Channel Topic]\nrelease train\n\nwhat broke?\n\n[Forwarded]\nKim: deploy failed at step 3\n\n\
         [Attachment: log.txt (text/plain, 20 bytes) - {base}/files/log.txt | local path: {}]\n\n\
         [Content of log.txt]:\n\ncontents of log.txt",
        local.display()
    );
    assert_eq!(prompt, expected);
    let history = calls_of(&calls, "conversations.history");
    assert_eq!(history[0]["latest"], "1761190800.000100");
    let _ = std::fs::remove_dir_all(&dir);

    // A reply in an unknown thread quotes its root.
    let reply_event = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "user": "UOWNER", "channel": "CHOME", "ts": "7.2", "thread_ts": "7.1", "text": "and this?"}});
    let reply = admit(&config("unused-mhc".into()), &reply_event, "UBOT", false).unwrap();
    let (base2, _calls2) = serve_scripted(
        "UBOT",
        vec![
            ("conversations.replies", vec![json!({"ok": true, "messages": [{"ts": "7.1", "user": "UALICE", "text": "root text"}]})]),
            ("users.info", vec![json!({"ok": true, "user": {"real_name": "Alice"}})]),
            ("conversations.info", vec![json!({"ok": true, "channel": {}})]),
        ],
    )
    .await;
    let quoted = with_test_api(base2, "xoxb-test".into(), async {
        prepare_prompt_in(&pool, &reply, &dir).await
    })
    .await;
    assert_eq!(
        quoted,
        "[Thu 1970-01-01 00:00:07 UTC] > [Replying to @Alice]: root text\n\nand this?"
    );

    // A file-only message is admitted; a plain bot message is not (SLACK_ALLOW_BOTS=none).
    let file_only = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "subtype": "file_share", "user": "UOWNER", "channel": "CHOME", "ts": "5.1", "text": "",
        "files": [{"id": "F9", "name": "a.png", "url_private": "https://files/a.png"}]}});
    assert!(admit(&config("unused-mhc".into()), &file_only, "UBOT", false).is_some());
    let bot = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "subtype": "bot_message", "bot_id": "B1", "channel": "CHOME", "ts": "5.2", "text": "hi"}});
    assert!(admit(&config("unused-mhc".into()), &bot, "UBOT", false).is_none());
    let mut mentions = config("unused-mhc".into());
    mentions.allow_bots = AllowBots::Mentions;
    mentions.allow_all_users = true;
    let bot_mention = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "app_mention", "bot_id": "B1", "channel": "CHOME", "ts": "5.3", "text": "<@UBOT> hi"}});
    assert!(admit(&mentions, &bot_mention, "UBOT", false).is_some());
    let mut scoped = config("unused-mhc".into());
    scoped.ignored_channels = vec!["CHOME".into()];
    let plain = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "user": "UOWNER", "channel": "CHOME", "ts": "5.4", "text": "hi"}});
    assert!(admit(&scoped, &plain, "UBOT", false).is_none());
    let mut no_auto = config("unused-mhc".into());
    no_auto.auto_thread = false;
    let top = admit(&no_auto, &plain, "UBOT", false).unwrap();
    assert_eq!(top.key.thread_id, None);
    assert_eq!(top.reply_thread.as_deref(), Some("5.4"));
    let mut strict = config("unused-mhc".into());
    strict.free_channels.clear();
    strict.thread_require_mention = true;
    let thread_reply = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "user": "UOWNER", "channel": "CHOME", "ts": "5.6", "thread_ts": "5.5", "text": "more"}});
    assert!(admit(&strict, &thread_reply, "UBOT", true).is_none());
}

#[tokio::test]
async fn e2e_messages_missed_while_down_are_answered_once_after_restart() {
    use super::slack_backfill::{read_cursor, run, write_cursor, BotIdentity};
    let pool = crate::storage::memory().await.unwrap();
    let (omo_url, log) = serve(script(&["누락된 요청을 처리했습니다."], false)).await;
    let config = config(omo_url);
    let bot = BotIdentity {
        user_id: "UBOT".into(),
        team_id: "T1".into(),
    };
    // Before the outage the hub had seen the channel up to 1000.0.
    write_cursor(&pool, "UBOT", "CHOME", "", "1000.000000")
        .await
        .unwrap();
    let (base, calls) = serve_scripted(
        "UBOT",
        vec![
            ("conversations.open", vec![json!({"ok": true, "channel": {"id": "DOWNER"}})]),
            (
                "conversations.history",
                vec![
                    // CHOME page 1 (newest first) with has_more, then page 2 repeating a boundary.
                    json!({"ok": true, "has_more": true, "messages": [
                        {"ts": "1003.000000", "user": "UOWNER", "text": "third"},
                        {"ts": "1002.000000", "user": "USTRANGER", "text": "not for us"}
                    ]}),
                    json!({"ok": true, "has_more": false, "messages": [
                        {"ts": "1002.000000", "user": "USTRANGER", "text": "not for us"},
                        {"ts": "1001.000000", "user": "UOWNER", "text": "first"},
                        {"ts": "1000.000000", "user": "UOWNER", "text": "already answered"}
                    ]}),
                    // DOWNER has no cursor: it is seeded at its newest message, nothing replayed.
                    json!({"ok": true, "messages": [{"ts": "990.000000", "user": "UOWNER", "text": "old dm"}]}),
                ],
            ),
        ],
    )
    .await;
    let routed = with_test_api(base.clone(), "xoxb-test".into(), async {
        run(&config, &pool, &bot, None).await.unwrap()
    })
    .await;
    assert_eq!(routed, 2);
    let turn_texts: Vec<String> = log
        .frames
        .lock()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "prompt")
        .map(|f| f["message"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(turn_texts.len(), 2, "{turn_texts:?}");
    assert!(turn_texts[0].ends_with("] first"), "{turn_texts:?}");
    assert!(turn_texts[1].ends_with("] third"), "{turn_texts:?}");
    let posts = calls_of(&calls, "chat.postMessage");
    assert_eq!(posts.iter().filter(|p| p["text"] == "누락된 요청을 처리했습니다.").count(), 2);
    assert!(posts.iter().any(|p| p["thread_ts"] == "1001.000000"));
    assert_eq!(
        read_cursor(&pool, "UBOT", "CHOME", "")
            .await
            .unwrap()
            .as_deref(),
        Some("1003.000000")
    );
    assert_eq!(
        read_cursor(&pool, "UBOT", "DOWNER", "")
            .await
            .unwrap()
            .as_deref(),
        Some("990.000000")
    );
    let history = calls_of(&calls, "conversations.history");
    assert_eq!(history[0]["oldest"], "1000.000000");
    assert_eq!(history[1]["latest"], "1002.000000");

    // The next pass (or a live redelivery) does not answer them again.
    let (base, calls) = serve_scripted(
        "UBOT",
        vec![(
            "conversations.history",
            vec![
                // CHOME: Slack's `oldest` boundary repeats the cursor message itself.
                json!({"ok": true, "messages": [{"ts": "1003.000000", "user": "UOWNER", "text": "third"}]}),
                // DOWNER: nothing new since the seed.
                json!({"ok": true, "messages": [{"ts": "990.000000", "user": "UOWNER", "text": "old dm"}]}),
            ],
        )],
    )
    .await;
    let again = with_test_api(base, "xoxb-test".into(), async {
        run(&config, &pool, &bot, None).await.unwrap()
    })
    .await;
    assert_eq!(again, 0);
    assert!(calls_of(&calls, "chat.postMessage").is_empty());
}

#[tokio::test]
async fn e2e_a_cron_delivery_rides_on_the_next_turn_of_its_thread_once() {
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let turn_text = |log: &MockLog| {
        log.frames
            .lock()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "prompt")
            .map(|f| f["message"].as_str().unwrap_or("").to_owned())
            .unwrap()
    };

    // The thread's session exists after one turn.
    let (omo_url, _first_peer) = serve(script(&["hello"], false)).await;
    let first = config(omo_url);
    with_test_api(base.clone(), "xoxb-test".into(), async {
        run_session_turn(&first, &pool, slack_turn("hi")).await
    })
    .await;

    // A cron job posts into that thread.
    sqlx::query(
        "INSERT INTO scheduled_jobs(id, expression, payload_json) VALUES('wx', 'every 1h', ?)",
    )
    .bind(json!({"command": "printf '오늘 날씨는 맑습니다.'", "deliver": "slack:CHOME:100.1", "report_salience":"result"}).to_string())
    .execute(&pool)
    .await
    .unwrap();
    with_test_api(base.clone(), "xoxb-test".into(), async {
        crate::schedule::run_native_job(&pool, "wx", false, true)
            .await
            .unwrap()
    })
    .await;
    assert!(calls_of(&calls, "chat.postMessage")
        .iter()
        .any(|post| post["text"]
            .as_str()
            .unwrap_or("")
            .contains("오늘 날씨는 맑습니다.")));

    // The next turn in the thread carries it as context; the one after does not.
    let (omo_url, log) = serve(script(&["it said sunny"], false)).await;
    let second = config(omo_url);
    with_test_api(base.clone(), "xoxb-test".into(), async {
        run_session_turn(&second, &pool, slack_turn("what did the cron say?")).await
    })
    .await;
    let text = turn_text(&log);
    assert!(
        text.starts_with("[Context: messages you already posted"),
        "{text}"
    );
    assert!(text.contains("(wx)\n오늘 날씨는 맑습니다."), "{text}");
    assert!(text.ends_with("what did the cron say?"), "{text}");

    let (omo_url, log) = serve(script(&["ok"], false)).await;
    let third = config(omo_url);
    with_test_api(base, "xoxb-test".into(), async {
        run_session_turn(&third, &pool, slack_turn("thanks")).await
    })
    .await;
    assert_eq!(turn_text(&log), "thanks");
}

#[tokio::test]
async fn e2e_a_long_owner_dm_turn_posts_no_status_line() {
    let (omo_url, _log) = serve(script(&["pushed"], true)).await;
    let config = config(omo_url);
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let response_url = format!("{base}/response_url");
    let owners = config.allowed_users.clone();
    let dm = SessionTurn {
        key: SessionKey::new("slack", None::<String>, "D1", None::<String>, "UOWNER"),
        reply_channel: Some("D1".into()),
        reply_thread: None,
        status_ts: None,
        text: "[2026-10-23 15:00 KST] push   it".into(),
        background: false,
    };
    let reply = with_test_api(base, "xoxb-test".into(), async {
        super::turn_status::INTERVAL_OVERRIDE
            .scope(std::time::Duration::from_millis(150), async {
                FULL_ACCESS_TEST
                    .scope(false, async {
                        let clicker = async {
                            let (ts, allow) = wait_card(&calls, "once").await;
                            // Time is the behaviour here: let several status intervals pass so a
                            // status line would have been posted and edited by now.
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                            super::turn_approval::handle_click(
                                &click("UOWNER", &ts, &allow, &response_url),
                                &owners,
                            )
                            .await;
                        };
                        tokio::join!(run_session_turn(&config, &pool, dm), clicker).0
                    })
                    .await
            })
            .await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("pushed"));
    assert_no_status_line(&calls);
}

/// A status line is internal progress narration: the owner's DM never gets one, neither as a
/// post nor as an edit.
fn assert_no_status_line(calls: &Calls) {
    let posts = calls_of(calls, "chat.postMessage");
    assert!(
        !posts
            .iter()
            .any(|post| is_status_post("chat.postMessage", post)),
        "no status post in the owner DM: {posts:?}"
    );
    let updates = calls_of(calls, "chat.update");
    assert!(
        !updates.iter().any(|update| {
            let text = update["text"].as_str().unwrap_or("");
            text.starts_with("작업 중:") || text.starts_with("완료")
        }),
        "no status edit in the owner DM: {updates:?}"
    );
}

#[tokio::test]
async fn e2e_a_reply_claim_is_stripped_then_remeasured_and_corrected() {
    let marker = r#"빌드 결과는 3줄이에요. [[measure ["echo","4"] expect~"^3$"]]"#;
    let (omo_url, _peer) = serve(script(&[marker], false)).await;
    let config = config(omo_url);
    let pool = crate::storage::memory().await.unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let reply = with_test_api(base, "xoxb-test".into(), async {
        super::report_claims::DELAY_OVERRIDE
            .scope(std::time::Duration::from_millis(50), async {
                let reply = run_session_turn(&config, &pool, slack_turn("빌드 어때?")).await;
                wait_call(&calls, "the correction post", |method, body| {
                    method == "chat.postMessage"
                        && body["text"].as_str().unwrap_or("").starts_with("정정:")
                })
                .await;
                reply
            })
            .await
    })
    .await;
    assert_eq!(reply.as_deref(), Some("빌드 결과는 3줄이에요."));
    let posts = calls_of(&calls, "chat.postMessage");
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert_eq!(posts[0]["text"], "빌드 결과는 3줄이에요.");
    assert_eq!(posts[1]["thread_ts"], "100.1");
    assert_eq!(
        posts[1]["text"],
        "정정: 빌드 결과는 3줄이에요. -> 실제 측정값: 4"
    );
    let stored: String = sqlx::query_scalar(
        "SELECT content FROM messages WHERE role = 'assistant' ORDER BY rowid DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!stored.contains("[[measure"), "{stored}");
}

#[tokio::test]
async fn provider_503_retries_once_on_the_background_or_session_model() {
    let (binary, log) = serve(script(&["__provider503__"], false)).await;
    let mut config = config(binary);
    config.default_model = Some("mahoquot/gpt-6-astra".into());
    let pool = crate::storage::memory().await.unwrap();
    let mut turn = slack_turn("hello");
    turn.reply_channel = None;
    let background = crate::model_policy::background_model();
    let result = run_session_turn(&config, &pool, turn).await;
    let frames = log.frames.lock().unwrap();
    let models: Vec<_> = frames
        .iter()
        .filter(|f| f["type"] == "fixture_launch")
        .map(|f| {
            let args = f["args"].as_array().unwrap();
            let flag = args.iter().position(|arg| arg == "--model").unwrap();
            args[flag + 1].as_str().unwrap().to_owned()
        })
        .collect();
    // A failed turn is always retried once: on OMON_BACKGROUND_MODEL when one is configured,
    // else on the session model. There is no built-in fallback model.
    let expected: Vec<String> = vec![
        "mahoquot/gpt-6-astra".to_owned(),
        background
            .clone()
            .unwrap_or_else(|| "mahoquot/gpt-6-astra".to_owned()),
    ];
    assert_eq!(models, expected);
    assert_eq!(frames.iter().filter(|f| f["type"] == "prompt").count(), 2);
    match background.as_deref() {
        // The mock recovers only on this model name.
        Some(model) if model.ends_with("/gpt-6-luna") => {
            assert_eq!(result.as_deref(), Some("recovered"));
        }
        _ => assert_eq!(result, None),
    }
}

/// A supervisor conversation whose turn runs until the test releases it.
struct GatedConversation {
    release: std::sync::Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl super::slack_supervisor::SupervisorConversation for GatedConversation {
    async fn converse(
        &self,
        _scope: &crate::meta::mhc_profile::ConversationScope,
        _turn: u64,
        _message: &str,
    ) -> Result<String, String> {
        self.release.notified().await;
        Ok("supervised answer".into())
    }

    async fn ingest_owner(&self, _source: &str, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn e2e_a_supervised_owner_dm_posts_no_status_line_and_never_reaches_omo() {
    let (omo_url, log) = serve(script(&["omo must not answer"], false)).await;
    let config = config(omo_url);
    let pool = crate::storage::memory().await.unwrap();
    crate::supervisor::ensure_identity(&pool, "slack:T1:UOWNER")
        .await
        .unwrap();
    let (base, calls) = serve_scripted("UBOT", Vec::new()).await;
    let payload = json!({"team_id": "T1", "authorizations": [{"user_id": "UBOT"}], "event": {
        "type": "message", "channel_type": "im", "team": "T1", "user": "UOWNER",
        "channel": "D1", "ts": "8801.000001", "text": "push   it"}});
    let admitted = admit(&config, &payload, "UBOT", false).expect("the owner's DM");
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let conversation: std::sync::Arc<dyn super::slack_supervisor::SupervisorConversation> =
        std::sync::Arc::new(GatedConversation {
            release: release.clone(),
        });
    let handled = with_test_api(base, "xoxb-test".into(), async {
        super::turn_status::INTERVAL_OVERRIDE
            .scope(std::time::Duration::from_millis(150), async {
                super::slack_supervisor::TEST_CONVERSATION
                    .scope(conversation, async {
                        let owner = async {
                            // Time is the behaviour here: let several status intervals pass so
                            // a status line would have been posted and edited by now.
                            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                            release.notify_one();
                        };
                        let turn = super::slack_turn::handle(&config, &pool, admitted);
                        tokio::join!(turn, owner).0
                    })
                    .await
            })
            .await
    })
    .await;
    assert!(handled);
    assert!(
        log.frames.lock().unwrap().is_empty(),
        "the omo app-server is never contacted"
    );
    let posts = calls_of(&calls, "chat.postMessage");
    assert!(
        posts.iter().all(|post| post.get("blocks").is_none()),
        "no approval card: {posts:?}"
    );
    let answers: Vec<&Value> = posts
        .iter()
        .filter(|post| post["text"] == "supervised answer")
        .collect();
    assert_eq!(answers.len(), 1, "{posts:?}");
    assert_eq!(answers[0]["channel"], "D1");
    assert_no_status_line(&calls);
}
