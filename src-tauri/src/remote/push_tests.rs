//! Phone push against a local mock push service (an axum server on loopback). The mock holds
//! the browser's half of the keys, so it decrypts what the daemon sent.
use super::*;
use ferryx_hub::supervisor::push_policy::{DECISION_BATCH_WINDOW, PANEL_PRESENCE_STALE};
use crate::remote::state::RemoteGatewayState;
use axum::{extract::State as AxState, http::StatusCode, response::IntoResponse, routing::post, Router};
use base64::Engine;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

struct Delivery {
    encoding: Option<String>,
    authorization: Option<String>,
    body: Vec<u8>,
}

/// A browser's subscription keys and the mock push service holding them.
struct MockPush {
    addr: std::net::SocketAddr,
    deliveries: mpsc::UnboundedReceiver<Delivery>,
    components: ece::EcKeyComponents,
    auth: [u8; 16],
    tasks: tokio::task::JoinSet<()>,
}

impl MockPush {
    async fn start(answer: StatusCode) -> Self {
        let (tx, deliveries) = mpsc::unbounded_channel();
        let app = Router::new()
            .route(
                "/push/{id}",
                post(
                    |AxState((tx, answer)): AxState<(mpsc::UnboundedSender<Delivery>, StatusCode)>,
                     headers: axum::http::HeaderMap,
                     body: axum::body::Bytes| async move {
                        let header = |name: &str| {
                            headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
                        };
                        let _ = tx.send(Delivery {
                            encoding: header("content-encoding"),
                            authorization: header("authorization"),
                            body: body.to_vec(),
                        });
                        answer
                    },
                ),
            )
            .with_state((tx, answer));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let (keypair, auth) = ece::generate_keypair_and_auth_secret().unwrap();
        Self { addr, deliveries, components: keypair.raw_components().unwrap(), auth, tasks }
    }

    fn subscription(&self, id: &str, needs_input: bool, mode: FinishedMode) -> PushSubscribeRequest {
        PushSubscribeRequest {
            endpoint: format!("http://{}/push/{id}", self.addr),
            keys: PushSubscriptionKeys {
                p256dh: B64.encode(self.components.public_key()),
                auth: B64.encode(self.auth),
            },
            phone_needs_input: needs_input,
            phone_finished_mode: mode,
        }
    }

    fn decrypt(&self, delivery: &Delivery) -> serde_json::Value {
        let plain = ece::decrypt(&self.components, &self.auth, &delivery.body).unwrap();
        serde_json::from_slice(&plain).unwrap()
    }

    /// Every request already received. Callers await the send first, so nothing is in flight.
    fn received(&mut self) -> Vec<Delivery> {
        let mut out = Vec::new();
        while let Ok(delivery) = self.deliveries.try_recv() {
            out.push(delivery);
        }
        out
    }
}

impl Drop for MockPush {
    fn drop(&mut self) {
        self.tasks.abort_all();
    }
}

fn gateway(dir: &std::path::Path) -> Arc<RemoteGatewayState> {
    let terminal = Arc::new(crate::terminal::TerminalService::new(
        Arc::new(crate::terminal::PtyManager::new()),
        Arc::new(crate::terminal::TerminalOutputHub::default()),
    ));
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        terminal,
        crate::worktree::WorkspaceRegistry::new(),
        Some(dir.join("remote-config.json")),
        Some(dir.join("remote-auth.json")),
    ));
    state.push.store.allow_loopback_endpoints();
    state
}

/// A local stand-in for Apple's push host. `POST /3/device/{token}` records the token and the
/// `authorization` header, and answers the configured status plus an optional `reason` body, so
/// the whole delivery path (JWT, headers, gone-token mapping) runs without reaching Apple.
struct MockApns {
    base_url: String,
    sends: mpsc::UnboundedReceiver<(String, Option<String>)>,
    tasks: tokio::task::JoinSet<()>,
}

impl MockApns {
    /// `status`/`reason` are fixed, so a test asserts an outcome without racing the mock.
    async fn start(status: StatusCode, reason: Option<&'static str>) -> Self {
        let (tx, sends) = mpsc::unbounded_channel();
        let app = Router::new().route(
            "/3/device/{token}",
            post(
                |AxState((tx, status, reason)): AxState<(
                    mpsc::UnboundedSender<(String, Option<String>)>,
                    StatusCode,
                    Option<&'static str>,
                )>,
                 axum::extract::Path(token): axum::extract::Path<String>,
                 headers: axum::http::HeaderMap| async move {
                    let authorization = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    let _ = tx.send((token, authorization));
                    match reason {
                        Some(reason) => (status, axum::Json(serde_json::json!({ "reason": reason }))).into_response(),
                        None => status.into_response(),
                    }
                },
            ),
        )
        .with_state((tx, status, reason));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { base_url: format!("http://{addr}"), sends, tasks }
    }

    /// Points `state`'s APNs sender at this mock. Must run before the first send.
    fn install(&self, state: &Arc<RemoteGatewayState>) {
        let credentials = crate::remote::apns::ApnsCredentials::for_test(
            test_key_pem(),
            "HLC2K69Y8G",
            "5DUM8WPB4C",
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let sender = crate::remote::apns::ApnsSender::for_test(
            credentials,
            "com.ferryx.mobile".to_string(),
            client,
            self.base_url.clone(),
        );
        state.push.install_apns_for_test(Arc::new(sender));
    }

    /// The sends already received. Callers await the send first, so nothing is in flight.
    fn received(&mut self) -> Vec<(String, Option<String>)> {
        let mut out = Vec::new();
        while let Ok(send) = self.sends.try_recv() {
            out.push(send);
        }
        out
    }
}

impl Drop for MockApns {
    fn drop(&mut self) {
        self.tasks.abort_all();
    }
}

/// A throwaway P-256 key in PKCS#8 PEM: the shape Apple's `.p8` has, and enough for the JWT the
/// mock host accepts.
fn test_key_pem() -> String {
    use jwt_simple::algorithms::ES256KeyPair;
    ES256KeyPair::generate().to_pem().unwrap()
}

fn paired_device(state: &RemoteGatewayState) -> String {
    let code = state
        .auth_manager
        .create_pairing_code(crate::remote::auth::DevicePermission::Control);
    let (_, device) = state.auth_manager.exchange_pairing_code(&code, "Phone").unwrap();
    device.id
}

const REF: &str = "mac:s1";

fn panel_frame(session_ref: &str, prompt_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "panel.decision",
        "sessionRef": session_ref,
        "decision": {
            "sessionRef": session_ref,
            "prompt": {
                "promptId": prompt_id,
                "kind": "approval",
                "question": "Run the migration?",
                "command": null,
                "options": ["Yes", "No"],
                "screenRevision": null
            },
            "recommendedIndex": 1,
            "reason": "It touches a remote database.",
            "createdAtMs": 1
        }
    })
}

fn subscribed_phone(state: &RemoteGatewayState, mock: &MockPush) {
    let device = paired_device(state);
    state.push.store.subscribe(&device, mock.subscription("a", true, FinishedMode::Off)).unwrap();
}

#[tokio::test]
async fn a_new_escalation_with_no_focused_client_sends_one_push_with_a_deep_link() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 1);
    // The same card announced again is not a second push.
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 0);

    let received = mock.received();
    assert_eq!(received.len(), 1, "exactly one POST");
    assert_eq!(received[0].encoding.as_deref(), Some("aes128gcm"));
    assert!(received[0].authorization.as_deref().is_some_and(|v| v.starts_with("vapid t=")));
    assert!(serde_json::from_slice::<serde_json::Value>(&received[0].body).is_err(), "body is ciphertext");
    let payload = mock.decrypt(&received[0]);
    assert_eq!(payload["state"], "decision");
    assert_eq!(payload["agent_id"], REF);
    assert_eq!(payload["url"], "/?panel=decision&session=mac%3As1");
    for key in payload.as_object().unwrap().keys() {
        assert!(
            ["agent_id", "agent_name", "worktree", "state", "title", "body", "url"].contains(&key.as_str()),
            "unexpected payload field {key}"
        );
    }
}

#[tokio::test]
async fn a_focused_desktop_sends_no_decision_push_and_a_stale_report_counts_as_unfocused() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    state.set_desktop_window_focused(true);
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 0);
    assert!(mock.received().is_empty(), "a focused desktop suppresses the push");

    let now = Instant::now();
    assert!(desktop_focus_suppresses(Some((true, now)), now + Duration::from_secs(119)));
    assert!(!desktop_focus_suppresses(Some((true, now)), now + DESKTOP_FOCUS_STALE));
    assert!(!desktop_focus_suppresses(Some((false, now)), now));
    assert!(!desktop_focus_suppresses(None, now));

    state.set_desktop_window_focused(false);
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p2")).await, 1);
    assert_eq!(mock.received().len(), 1);
}

#[tokio::test]
async fn retries_progress_finished_sessions_and_resolved_cards_send_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    let frames = [
        // A recovery retry and its in-place updates.
        serde_json::json!({"type": "panel.action", "action": {"id": "a1", "sessionRef": REF, "kind": "retry"}}),
        // A session that finished, or whose state changed.
        serde_json::json!({"type": "panel.session", "session": {"sessionRef": REF, "state": "idle"}}),
        serde_json::json!({"type": "panel.rules", "rules": []}),
        // A card that was answered or withdrawn.
        serde_json::json!({"type": "panel.decision", "sessionRef": REF, "decision": null}),
        // A decision frame with no usable escalation.
        serde_json::json!({"type": "panel.decision", "sessionRef": REF, "decision": {"reason": "x"}}),
        serde_json::json!({"type": "supervisorChanged"}),
    ];
    for frame in &frames {
        assert_eq!(state.handle_panel_frame(frame).await, 0, "{frame}");
    }
    assert!(mock.received().is_empty());

    // A resolved card re-arms its session: the next escalation with the same prompt id is new.
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 1);
    let resolved = serde_json::json!({"type": "panel.decision", "sessionRef": REF, "decision": null});
    assert_eq!(state.handle_panel_frame(&resolved).await, 0);
    assert!(state.decision_gate.is_new_card(REF, "p1"), "a resolved card re-arms its session");
}

#[tokio::test]
async fn a_held_push_is_withdrawn_when_its_session_leaves_needs_you() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    let t0 = Instant::now();
    assert_eq!(state.push_decision("mac:s1", "p1", t0).await, 1);
    assert_eq!(state.push_decision("mac:s2", "p1", t0 + Duration::from_secs(1)).await, 0, "held");
    assert_eq!(state.push_decision("mac:s3", "p1", t0 + Duration::from_secs(2)).await, 0, "held");

    // The owner acknowledged s2 on another device: its panel.session frame is no longer needsYou.
    let left = serde_json::json!({"type": "panel.session", "sessionRef": "mac:s2", "session": {
        "sessionRef": "mac:s2", "state": "blocked",
        "attention": {"axes": {"demand": "question", "activity": "stopped", "completion": "none",
                               "read": "read", "ownership": "owner"}, "group": "seen"}}});
    assert_eq!(state.handle_panel_frame(&left).await, 0);

    assert_eq!(state.flush_decision_batch(t0 + DECISION_BATCH_WINDOW).await, 1);
    let received = mock.received();
    assert_eq!(received.len(), 2);
    let payload = mock.decrypt(&received[1]);
    assert_eq!(payload["url"], "/?panel=decision&session=mac%3As3", "only the session still waiting pushes");
}

#[tokio::test]
async fn a_muted_session_sends_no_push_and_the_mute_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    state.set_session_push_muted(REF, true).await.unwrap();
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 0);
    assert!(mock.received().is_empty(), "a muted session gets no push");

    let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
    assert!(reloaded.is_session_muted(REF), "the mute is persisted beside the subscriptions");
    assert_eq!(reloaded.list_subscriptions().len(), 1, "muting keeps the subscriptions");

    // Another session still pushes.
    assert_eq!(state.handle_panel_frame(&panel_frame("mac:s2", "p1")).await, 1);

    state.set_session_push_muted(REF, false).await.unwrap();
    assert!(!state.push.store.is_session_muted(REF));
    assert!(!PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE))).is_session_muted(REF));
    assert!(state.set_session_push_muted("no-colon", true).await.is_err(), "a malformed ref is refused");
}

#[tokio::test]
async fn a_second_card_inside_the_window_is_held_and_flushed_once() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    let t0 = Instant::now();
    assert_eq!(state.push_decision("mac:s1", "p1", t0).await, 1);
    assert_eq!(state.push_decision("mac:s2", "p1", t0 + Duration::from_secs(5)).await, 0, "held");
    assert_eq!(state.push_decision("mac:s3", "p1", t0 + Duration::from_secs(6)).await, 0, "held");
    assert_eq!(mock.received().len(), 1);

    assert_eq!(state.flush_decision_batch(t0 + DECISION_BATCH_WINDOW).await, 1);
    let received = mock.received();
    assert_eq!(received.len(), 1, "the held cards go out as one push");
    let payload = mock.decrypt(&received[0]);
    assert_eq!(payload["url"], "/?panel=decision");
    assert_eq!(payload["body"], "2 sessions need a decision");
    assert_eq!(state.flush_decision_batch(t0 + DECISION_BATCH_WINDOW).await, 0, "nothing left to flush");
}

#[tokio::test]
async fn a_phone_showing_the_panel_holds_the_push_and_the_card_never_comes_back() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);
    let phone = paired_device(&state);

    state.set_panel_visible(&phone, true);
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 0, "the panel is in front of the owner");
    assert!(mock.received().is_empty());

    // Leaving the panel does not turn the card the owner already saw into a push.
    state.set_panel_visible(&phone, false);
    assert_eq!(state.handle_panel_frame(&panel_frame(REF, "p1")).await, 0);
    assert!(mock.received().is_empty());
    // A different card does push once nobody shows the panel.
    assert_eq!(state.handle_panel_frame(&panel_frame("mac:s2", "p1")).await, 1);
    assert_eq!(mock.received().len(), 1);
}

#[tokio::test]
async fn any_one_client_showing_the_panel_is_enough_and_a_stale_report_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);
    let now = Instant::now();

    state.panel_presence.report("phone", true, now);
    state.panel_presence.report("tablet", true, now);
    state.panel_presence.report("phone", false, now);
    assert!(state.panel_shown(now), "the tablet still shows the panel");
    assert!(state.panel_presence.shown(now + PANEL_PRESENCE_STALE - Duration::from_secs(1)));
    assert!(!state.panel_presence.shown(now + PANEL_PRESENCE_STALE), "a silent client stops counting");

    // The desktop window counts as a client too.
    let quiet = gateway(dir.path());
    assert!(!quiet.panel_shown(now));
    quiet.set_desktop_window_focused(true);
    assert!(quiet.panel_shown(Instant::now()));
}

#[tokio::test]
async fn a_card_held_by_the_window_is_dropped_if_the_panel_is_shown_at_the_flush() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);
    let phone = paired_device(&state);

    let t0 = Instant::now();
    assert_eq!(state.push_decision("mac:s1", "p1", t0).await, 1);
    assert_eq!(state.push_decision("mac:s2", "p1", t0 + Duration::from_secs(5)).await, 0, "held");
    state.set_panel_visible(&phone, true);
    assert_eq!(state.flush_decision_batch(Instant::now() + DECISION_BATCH_WINDOW).await, 0);
    assert_eq!(mock.received().len(), 1, "only the first push went out");
}

#[tokio::test]
async fn a_card_muted_while_held_is_dropped_at_the_flush_and_the_rest_still_go() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);

    let t0 = Instant::now();
    assert_eq!(state.push_decision("mac:s1", "p1", t0).await, 1);
    assert_eq!(state.push_decision("mac:s2", "p1", t0 + Duration::from_secs(5)).await, 0);
    assert_eq!(state.push_decision("mac:s3", "p1", t0 + Duration::from_secs(6)).await, 0);
    state.set_session_push_muted("mac:s2", true).await.unwrap();
    assert_eq!(state.flush_decision_batch(t0 + DECISION_BATCH_WINDOW).await, 1);
    let received = mock.received();
    assert_eq!(received.len(), 2);
    let payload = mock.decrypt(&received[1]);
    assert_eq!(payload["agent_id"], "mac:s3", "the flush names the one card left, not the muted one");
    assert_eq!(payload["url"], "/?panel=decision&session=mac%3As3");
}

/// The consumer loop: frames on the panel channel reach push with no test shim. The muted
/// session's frame comes first and one consumer handles frames in order, so the first delivery
/// being the second session proves the muted one sent nothing; the extra frame kinds in between
/// (rules, retry action, finished session) must not send either.
#[tokio::test]
async fn the_panel_channel_feeds_push_and_a_muted_session_stays_silent() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    subscribed_phone(&state, &mock);
    state.set_session_push_muted("chan:muted", true).await.unwrap();
    let (frames, source) = tokio::sync::broadcast::channel(16);
    state.start_panel_push_from(source);
    // A second start is a no-op: one consumer, so nothing is delivered twice.
    state.start_panel_push_from(frames.subscribe());

    frames.send(serde_json::json!({"type": "panel.rules", "rules": []})).unwrap();
    frames
        .send(serde_json::json!({"type": "panel.action", "action": {"id": 1, "sessionRef": "chan:loud", "kind": "retried"}}))
        .unwrap();
    frames
        .send(serde_json::json!({"type": "panel.session", "session": {"sessionRef": "chan:loud", "state": "idle"}}))
        .unwrap();
    frames.send(panel_frame("chan:muted", "p1")).unwrap();
    frames.send(panel_frame("chan:loud", "p1")).unwrap();

    let delivery = tokio::time::timeout(Duration::from_secs(10), mock.deliveries.recv())
        .await
        .expect("the panel channel reaches push")
        .expect("mock alive");
    let payload = mock.decrypt(&delivery);
    assert_eq!(payload["agent_id"], "chan:loud");
    assert_eq!(payload["url"], "/?panel=decision&session=chan%3Aloud");
    assert!(mock.received().is_empty(), "nothing else was sent");
}

#[test]
fn deep_links_open_the_card_and_name_no_terminal_content() {
    assert_eq!(ferryx_hub::supervisor::push_policy::decision_deep_link("mac:s1"), "/?panel=decision&session=mac%3As1");
    let one = decision_notification(&["mac:s1".to_string()], "Claude Code", Some("feature-x"));
    assert_eq!(one.state, "decision");
    assert_eq!(one.body, "Claude Code in feature-x");
    assert_eq!(one.url.as_deref(), Some("/?panel=decision&session=mac%3As1"));
    let many = decision_notification(&["mac:s1".to_string(), "win:s2".to_string()], "", None);
    assert_eq!(many.url.as_deref(), Some("/?panel=decision"));
    assert_eq!(many.agent_id, "");
}

#[tokio::test]
async fn revoke_removes_the_device_subscriptions_on_disk_too() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mock = MockPush::start(StatusCode::CREATED).await;
    let phone = paired_device(&state);
    let other = paired_device(&state);
    state.push.store.subscribe(&phone, mock.subscription("a", true, FinishedMode::Off)).unwrap();
    state.push.store.subscribe(&phone, mock.subscription("b", true, FinishedMode::Off)).unwrap();
    state.push.store.subscribe(&other, mock.subscription("c", true, FinishedMode::Off)).unwrap();
    assert!(
        state.push.store.subscribe(&other, mock.subscription("a", true, FinishedMode::Off)).is_err(),
        "an endpoint stays with the device that registered it"
    );

    assert!(state.auth_manager.revoke_device(&phone).unwrap());
    let left = state.push.store.list_subscriptions();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].device_id, other);

    let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
    assert_eq!(reloaded.list_subscriptions(), left, "revocation is persisted");
}

#[tokio::test]
async fn an_expired_device_gets_no_push_and_is_pruned_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    let device = paired_device(&state);
    state.push.store.subscribe(&device, mock.subscription("a", true, FinishedMode::Off)).unwrap();

    // Idle past the expiry: the device is no longer paired, though nothing revoked it.
    let later = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + crate::remote::auth::DEVICE_IDLE_EXPIRY_SECS
        + 60;
    state.auth_manager.set_test_clock(later);
    assert!(state.auth_manager.live_device(&device).is_none());
    assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 0);
    assert!(mock.received().is_empty(), "no POST for an unpaired device");

    // A device missing from the auth store at startup has its subscriptions dropped.
    let store = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
    assert_eq!(store.list_subscriptions().len(), 1);
    drop(state);
    std::fs::remove_file(dir.path().join("remote-auth.json")).unwrap();
    let restarted = gateway(dir.path());
    assert!(restarted.push.store.list_subscriptions().is_empty());
    assert!(PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)))
        .list_subscriptions()
        .is_empty());
}

#[tokio::test]
async fn fan_out_follows_session_visibility_per_access_scope() {
    use crate::remote::auth::{DeviceAccessScope, DevicePermission};
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mirror_mock = MockPush::start(StatusCode::CREATED).await;
    let mut machine_mock = MockPush::start(StatusCode::CREATED).await;
    let mirror = paired_device(&state);
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .unwrap();
    let (_, machine) = state.auth_manager.exchange_pairing_code(&pin, "Machine phone").unwrap();
    assert_eq!(machine.access_scope, DeviceAccessScope::Machine);
    state.push.store.subscribe(&mirror, mirror_mock.subscription("m", true, FinishedMode::Off)).unwrap();
    state.push.store.subscribe(&machine.id, machine_mock.subscription("x", true, FinishedMode::Off)).unwrap();
    *state.machine_owned_override.write() = Some(Arc::new(|id: &str| id.starts_with("machine-")));

    assert_eq!(state.dispatch_decision_push(&["mac:machine-1".to_string()]).await, 1);
    assert_eq!(machine_mock.received().len(), 1, "a Machine Control device gets its machine session");
    assert!(mirror_mock.received().is_empty(), "a Mirror device never sees a machine session");

    assert_eq!(state.dispatch_decision_push(&["mac:desktop-1".to_string()]).await, 1);
    assert_eq!(mirror_mock.received().len(), 1, "a Mirror device gets a mirrored session");
    assert!(machine_mock.received().is_empty(), "a Machine device does not get mirrored sessions");
}

#[tokio::test]
async fn gone_answer_deletes_the_subscription() {
    for answer in [StatusCode::GONE, StatusCode::NOT_FOUND] {
        let dir = tempfile::tempdir().unwrap();
        let state = gateway(dir.path());
        let mut mock = MockPush::start(answer).await;
        let device = paired_device(&state);
        state.push.store.subscribe(&device, mock.subscription("a", true, FinishedMode::Off)).unwrap();

        assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 1);
        assert_eq!(mock.received().len(), 1);
        assert!(state.push.store.list_subscriptions().is_empty(), "{answer} deletes the subscription");
        let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
        assert!(reloaded.list_subscriptions().is_empty());
    }
}

#[test]
fn vapid_keys_are_created_once_and_owner_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(PUSH_VAPID_FILE);
    let first = VapidKeys::load_or_create(Some(&path)).unwrap();
    let second = VapidKeys::load_or_create(Some(&path)).unwrap();
    assert_eq!(first.public_key(), second.public_key());
    let raw = B64.decode(first.public_key()).unwrap();
    assert_eq!(raw.len(), 65, "uncompressed P-256 point");
    assert_eq!(raw[0], 4);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn endpoint_policy_admits_push_services_only() {
    for url in [
        "https://fcm.googleapis.com/fcm/send/a",
        "https://web.push.apple.com/QGx",
        "https://updates.push.services.mozilla.com/wpush/v2/a",
        "https://wns2-par02p.notify.windows.com/w/?token=a",
    ] {
        assert!(validate_endpoint(url, false).is_ok(), "{url}");
    }
    for url in [
        "http://fcm.googleapis.com/fcm/send/a",
        "https://127.0.0.1/a",
        "https://fcm.googleapis.com.evil.test/a",
        "https://evilfcm.googleapis.com.test/a",
        "https://user@fcm.googleapis.com/a",
        "https://fcm.googleapis.com:444/a",
        "https://fcm.googleapis.com/a#x",
        "https://internal.example/a",
        "http://127.0.0.1:9/a",
    ] {
        assert!(validate_endpoint(url, false).is_err(), "{url}");
    }
    assert!(validate_endpoint("http://127.0.0.1:9/a", true).is_ok());
    assert!(validate_endpoint("http://10.0.0.1:9/a", true).is_err());
}

#[test]
fn preferences_update_only_the_callers_subscriptions() {
    let store = PushStore::new();
    store.allow_loopback_endpoints();
    let sub = |endpoint: &str| PushSubscribeRequest {
        endpoint: endpoint.into(),
        keys: PushSubscriptionKeys { p256dh: "k".into(), auth: "a".into() },
        phone_needs_input: true,
        phone_finished_mode: FinishedMode::LongTurns,
    };
    store.subscribe("d1", sub("http://127.0.0.1:1/a")).unwrap();
    store.subscribe("d2", sub("http://127.0.0.1:1/b")).unwrap();
    let prefs = PushPreferences { phone_needs_input: Some(false), phone_finished_mode: Some(FinishedMode::EveryTurn) };
    assert_eq!(store.set_preferences("d1", &prefs).unwrap(), 1);
    let d1 = &store.for_device("d1")[0];
    assert!(!d1.needs_input);
    assert_eq!(d1.finished_mode, FinishedMode::EveryTurn);
    let d2 = &store.for_device("d2")[0];
    assert!(d2.needs_input);
    assert_eq!(d2.finished_mode, FinishedMode::LongTurns);
    let body: PushSubscribeRequest = serde_json::from_value(serde_json::json!({
        "endpoint": "https://fcm.googleapis.com/x", "keys": {"p256dh": "k", "auth": "a"}
    }))
    .unwrap();
    assert!(body.phone_needs_input);
    assert_eq!(body.phone_finished_mode, FinishedMode::LongTurns);
}

/// Persistence and ownership rules for the APNs half of the store, plus the wire format both
/// transports share. This is a single-threaded sequence: it proves that an APNs row round-trips
/// through the file, that one token cannot be adopted or removed by another device, and that a
/// gone token is dropped from disk. The lock order between the two maps is proven by
/// [`concurrent_web_push_and_apns_writes_do_not_deadlock`], not here.
#[test]
fn apns_subscriptions_persist_and_keep_one_owner_per_token() {
    use crate::remote::apns::ApnsEnvironment;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(PUSH_SUBSCRIPTIONS_FILE);
    let store = PushStore::load(Some(path.clone()));
    let token = "ab".repeat(32);

    store
        .subscribe_apns("d1", &token, ApnsEnvironment::Sandbox, true)
        .unwrap();
    store
        .subscribe(
            "d1",
            PushSubscribeRequest {
                endpoint: "https://fcm.googleapis.com/fcm/send/a".to_string(),
                keys: PushSubscriptionKeys { p256dh: "k".into(), auth: "a".into() },
                phone_needs_input: true,
                phone_finished_mode: FinishedMode::LongTurns,
            },
        )
        .unwrap();

    assert_eq!(store.list_apns().len(), 1);
    assert_eq!(store.list_subscriptions().len(), 1);
    let reloaded = PushStore::load(Some(path.clone()));
    assert_eq!(reloaded.list_apns()[0].environment, ApnsEnvironment::Sandbox);
    assert_eq!(reloaded.list_apns()[0].token, token);

    // Re-subscribing the same token must replace the stored environment rather than duplicate
    // it: a phone that moved to a distribution build has to start receiving production pushes.
    store
        .subscribe_apns("d1", &token, ApnsEnvironment::Production, false)
        .unwrap();
    let stored = store.list_apns();
    assert_eq!(stored.len(), 1, "one row per token");
    assert_eq!(stored[0].environment, ApnsEnvironment::Production);
    assert!(!stored[0].needs_input, "the latest preference wins");

    // Another device may not adopt a token it does not own, and its removal must not succeed.
    assert!(store
        .subscribe_apns("d2", &token, ApnsEnvironment::Production, true)
        .is_err());
    assert!(store.unsubscribe_apns("d2", &token).is_err());
    assert_eq!(store.list_apns().len(), 1);

    // A gone token is dropped, and the drop survives a reload.
    store.remove_gone_apns(&token).unwrap();
    assert!(store.list_apns().is_empty());
    assert!(PushStore::load(Some(path)).list_apns().is_empty());
}

/// A store that persists both transports must acquire `subscriptions` before `apns` on every
/// path that takes both; an inverted order deadlocks the two writers against each other. The
/// two writers here run at the same time and are released together by a barrier, so neither can
/// finish before the other starts, and the test fails by hanging into the deadline rather than
/// by passing a sequential sequence.
#[test]
fn concurrent_web_push_and_apns_writes_do_not_deadlock() {
    use crate::remote::apns::ApnsEnvironment;
    use std::sync::Barrier;

    const ROUNDS: usize = 200;
    // In memory: this test is about lock acquisition, not about the file, and a shared path would
    // add disk races that have nothing to do with the property under test.
    let store = Arc::new(PushStore::new());
    store.allow_loopback_endpoints();
    let barrier = Arc::new(Barrier::new(2));
    let (done_tx, done_rx) = std::sync::mpsc::channel();

    let web = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let done = done_tx.clone();
        std::thread::spawn(move || {
            barrier.wait();
            for round in 0..ROUNDS {
                store
                    .subscribe(
                        "web",
                        PushSubscribeRequest {
                            endpoint: format!("http://127.0.0.1:9/push/{round}"),
                            keys: PushSubscriptionKeys { p256dh: "k".into(), auth: "a".into() },
                            phone_needs_input: true,
                            phone_finished_mode: FinishedMode::Off,
                        },
                    )
                    .unwrap();
                store.set_preferences("web", &PushPreferences { phone_needs_input: Some(round % 2 == 0), phone_finished_mode: None }).unwrap();
            }
            let _ = done.send("web");
        })
    };
    let apns = {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let done = done_tx.clone();
        std::thread::spawn(move || {
            barrier.wait();
            for round in 0..ROUNDS {
                let token = format!("{round:064x}");
                store.subscribe_apns("ios", &token, ApnsEnvironment::Sandbox, true).unwrap();
            }
            let _ = done.send("apns");
        })
    };

    // Both writers must finish. A deadlock never sends, so the deadline is what reports it; the
    // timeout is a deadlock detector, not a substitute for waiting on the real completion signal.
    for _ in 0..2 {
        done_rx
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("an inverted lock order deadlocked the two writers");
    }
    web.join().unwrap();
    apns.join().unwrap();
    assert_eq!(store.list_subscriptions().len(), ROUNDS, "every endpoint the Web Push writer registered is stored");
    assert_eq!(store.list_apns().len(), ROUNDS, "every token the APNs writer registered is stored");
}

/// An iOS install registers an APNs token and never a Web Push subscription, so a device whose
/// only row is an APNs token must still be pushed to. This is the regression that made an iOS
/// phone receive nothing while the daemon logged healthy Web Push fan-out.
#[tokio::test]
async fn an_apns_only_device_receives_the_decision_push() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockApns::start(StatusCode::OK, None).await;
    mock.install(&state);
    let device = paired_device(&state);
    let token = "ab".repeat(32);
    state
        .push
        .store
        .subscribe_apns(&device, &token, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();

    assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 1);
    let received = mock.received();
    assert_eq!(received.len(), 1, "the APNs token is a delivery target on its own");
    assert_eq!(received[0].0, token, "the request names the registered token");
    assert!(
        received[0].1.as_deref().is_some_and(|auth| auth.starts_with("bearer ")),
        "Apple authenticates the send with a provider JWT: {:?}",
        received[0].1
    );
}

/// Apple answers 410 (or 400 `BadDeviceToken`/`Unregistered`) for a token that can never be
/// delivered again. That row must be deleted, or every later fan-out keeps posting to a dead
/// token; the deletion must also reach the file, since a restart reloads from it.
#[tokio::test]
async fn a_gone_apns_token_is_deleted_and_the_deletion_is_persisted() {
    for (status, reason) in [
        (StatusCode::GONE, None),
        (StatusCode::BAD_REQUEST, Some("BadDeviceToken")),
        (StatusCode::BAD_REQUEST, Some("Unregistered")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let state = gateway(dir.path());
        let mut mock = MockApns::start(status, reason).await;
        mock.install(&state);
        let device = paired_device(&state);
        let token = "cd".repeat(32);
        state
            .push
            .store
            .subscribe_apns(&device, &token, crate::remote::apns::ApnsEnvironment::Sandbox, true)
            .unwrap();

        assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 1);
        assert_eq!(mock.received().len(), 1);
        assert!(
            state.push.store.list_apns().is_empty(),
            "{status} {reason:?} must delete the token"
        );
        let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
        assert!(
            reloaded.list_apns().is_empty(),
            "{status} {reason:?} deletion must be persisted"
        );
    }
}

/// A token Apple still accepts must survive, so the test above cannot pass by deleting every row.
#[tokio::test]
async fn a_delivered_apns_token_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockApns::start(StatusCode::OK, None).await;
    mock.install(&state);
    let device = paired_device(&state);
    let token = "ef".repeat(32);
    state
        .push
        .store
        .subscribe_apns(&device, &token, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();

    assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 1);
    assert_eq!(mock.received().len(), 1);
    assert_eq!(state.push.store.list_apns().len(), 1, "a delivered token stays registered");
}

/// A revoke is the owner deleting the device, so both transports' rows must go. Before this the
/// hook cleaned only the Web Push map, leaving the APNs token able to receive alerts.
#[tokio::test]
async fn revoke_removes_the_device_apns_token_too() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let device = paired_device(&state);
    let other = paired_device(&state);
    let mine = "11".repeat(32);
    let theirs = "22".repeat(32);
    state
        .push
        .store
        .subscribe_apns(&device, &mine, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();
    state
        .push
        .store
        .subscribe_apns(&other, &theirs, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();

    assert!(state.auth_manager.revoke_device(&device).unwrap());
    let left = state.push.store.list_apns();
    assert_eq!(left.len(), 1, "only the revoked device's token is dropped");
    assert_eq!(left[0].device_id, other);
    let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
    assert_eq!(reloaded.list_apns(), left, "the revocation is persisted");
}

/// The restart-time prune runs before any send, and an unpaired device must lose both rows: an
/// iOS install has only the APNs row, so pruning the Web Push map alone left its token live.
#[tokio::test]
async fn an_unpaired_device_loses_its_apns_token_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let device = paired_device(&state);
    state
        .push
        .store
        .subscribe_apns(&device, &"33".repeat(32), crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();
    drop(state);
    // The auth store is gone, so the device is unpaired; only the push store still names it.
    std::fs::remove_file(dir.path().join("remote-auth.json")).unwrap();

    let restarted = gateway(dir.path());
    assert!(restarted.push.store.list_apns().is_empty(), "the APNs token of an unpaired device is pruned");
    assert!(PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)))
        .list_apns()
        .is_empty());
}

/// Preferences are the owner's single switch panel, and an iOS install's only row is its APNs
/// token, so turning "needs input" off has to reach that row: otherwise the switch saves nothing
/// and the phone keeps alerting.
#[tokio::test]
async fn preferences_reach_the_apns_row_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let device = paired_device(&state);
    let token = "44".repeat(32);
    state
        .push
        .store
        .subscribe_apns(&device, &token, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();

    let updated = state
        .push
        .store
        .set_preferences(&device, &PushPreferences { phone_needs_input: Some(false), phone_finished_mode: None })
        .unwrap();
    assert_eq!(updated, 1, "the APNs row counts as one subscription");
    assert!(!state.push.store.list_apns()[0].needs_input);
    assert!(
        !PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE))).list_apns()[0].needs_input,
        "the preference is persisted"
    );

    // And the fan-out honours it: a token with needs_input off gets no decision alert.
    let mut mock = MockApns::start(StatusCode::OK, None).await;
    mock.install(&state);
    assert_eq!(state.dispatch_decision_push(&[REF.to_string()]).await, 0);
    assert!(mock.received().is_empty());
}

/// The settings "test alert" on an installed iOS app used to answer 404: the handler looked only
/// for a Web Push subscription, and an iOS install has none. It must reach APNs for such a
/// device, and the test has to go through the real route to prove the phone sees a 200.
#[tokio::test]
async fn push_test_sends_through_apns_for_an_apns_only_device() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockApns::start(StatusCode::OK, None).await;
    mock.install(&state);
    let code = state
        .auth_manager
        .create_pairing_code(crate::remote::auth::DevicePermission::Control);
    let (token, device) = state.auth_manager.exchange_pairing_code(&code, "iPhone").unwrap();
    let apns_token = "55".repeat(32);
    state
        .push
        .store
        .subscribe_apns(&device.id, &apns_token, crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, crate::remote::server::create_remote_router(state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = http
        .post(format!("http://{addr}/api/v1/push/test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK, "an iOS install is a valid test target");
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["sent"], 1);
    assert_eq!(body["delivered"], 1);
    let received = mock.received();
    assert_eq!(received.len(), 1, "the test alert reached APNs");
    assert_eq!(received[0].0, apns_token);
    server.abort();
}

/// A device with no subscription on either transport still gets 404: the APNs path must not turn
/// an unregistered phone into a 200.
#[tokio::test]
async fn push_test_still_answers_404_without_any_subscription() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let code = state
        .auth_manager
        .create_pairing_code(crate::remote::auth::DevicePermission::Control);
    let (token, _) = state.auth_manager.exchange_pairing_code(&code, "iPhone").unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, crate::remote::server::create_remote_router(state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = http
        .post(format!("http://{addr}/api/v1/push/test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    server.abort();
}

/// A token is registered but this daemon holds no Apple key, so the alert cannot be sent. Saying
/// 200 here would tell the phone its alerts work when nothing left the machine.
#[tokio::test]
async fn push_test_reports_a_missing_apple_key_instead_of_success() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let code = state
        .auth_manager
        .create_pairing_code(crate::remote::auth::DevicePermission::Control);
    let (token, device) = state.auth_manager.exchange_pairing_code(&code, "iPhone").unwrap();
    state
        .push
        .store
        .subscribe_apns(&device.id, &"66".repeat(32), crate::remote::apns::ApnsEnvironment::Sandbox, true)
        .unwrap();
    // No APNs sender installed: the env probe finds no FERRYX_APNS_* key in this test process.
    assert!(state.push.apns().await.is_none(), "this test needs APNs to be unconfigured");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, crate::remote::server::create_remote_router(state))
            .await
            .unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = http
        .post(format!("http://{addr}/api/v1/push/test"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    server.abort();
}
