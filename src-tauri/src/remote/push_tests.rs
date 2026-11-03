//! Phone push against a local mock push service (an axum server on loopback). The mock holds
//! the browser's half of the keys, so it decrypts what the daemon sent.
use super::*;
use crate::remote::state::RemoteGatewayState;
use axum::{extract::State as AxState, http::StatusCode, routing::post, Router};
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
    assert!(state.decision_gate.is_new_card(REF, "p1"));
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

#[test]
fn the_gate_paces_decisions_on_the_hub_window() {
    assert_eq!(DECISION_BATCH_WINDOW, Duration::from_secs(30));
    let gate = DecisionGate::default();
    let t0 = Instant::now();
    assert_eq!(gate.window("a", t0), WindowVerdict::Send);
    assert_eq!(
        gate.window("b", t0 + Duration::from_secs(5)),
        WindowVerdict::Held { flush_in: Some(Duration::from_secs(25)) }
    );
    assert_eq!(gate.window("c", t0 + Duration::from_secs(6)), WindowVerdict::Held { flush_in: None });
    assert_eq!(gate.take_held(t0 + Duration::from_secs(30)), vec!["b".to_string(), "c".to_string()]);
    // The flush opened a new window.
    assert!(matches!(gate.window("d", t0 + Duration::from_secs(31)), WindowVerdict::Held { .. }));
    assert_eq!(gate.take_held(t0 + Duration::from_secs(62)), vec!["d".to_string()]);
    // Quiet for a whole window: the next card goes straight out.
    assert_eq!(gate.window("e", t0 + Duration::from_secs(93)), WindowVerdict::Send);
}

#[test]
fn deep_links_open_the_card_and_name_no_terminal_content() {
    assert_eq!(decision_deep_link("mac:s1"), "/?panel=decision&session=mac%3As1");
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
