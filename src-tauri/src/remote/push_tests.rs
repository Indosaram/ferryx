//! Phone push against a local mock push service (an axum server on loopback). The mock holds
//! the browser's half of the keys, so it decrypts what the daemon sent.
use super::*;
use crate::agent_control::events::ObservationDraft;
use crate::agent_control::{AgentStateSource, AgentTier};
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

fn draft(session: &str, state: AgentControlState) -> ObservationDraft {
    ObservationDraft {
        session_id: session.into(),
        state,
        state_source: AgentStateSource::Provider,
        tier: AgentTier::Native,
        prompt_id: None,
        kind: None,
        screen_revision: None,
        workspace_id: None,
    }
}

#[tokio::test]
async fn waiting_transition_sends_one_encrypted_post_without_terminal_content() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    let device = paired_device(&state);
    state
        .push
        .store
        .subscribe(&device, mock.subscription("a", true, FinishedMode::Off))
        .unwrap();

    let mut turns = TurnTracker::default();
    let t0 = Instant::now();
    assert_eq!(turns.observe("s1", AgentControlState::Working, t0), None);
    let kind = turns.observe("s1", AgentControlState::Blocked, t0).expect("needs input");
    assert_eq!(kind, PushKind::NeedsInput);
    assert_eq!(state.dispatch_push("s1", kind).await, 1);
    // A repeated blocked reading is the same wait, not a second alert.
    assert_eq!(turns.observe("s1", AgentControlState::Blocked, t0), None);

    let received = mock.received();
    assert_eq!(received.len(), 1, "exactly one POST");
    assert_eq!(received[0].encoding.as_deref(), Some("aes128gcm"));
    assert!(received[0].authorization.as_deref().is_some_and(|v| v.starts_with("vapid t=")));
    assert!(serde_json::from_slice::<serde_json::Value>(&received[0].body).is_err(), "body is ciphertext");
    let payload = mock.decrypt(&received[0]);
    assert_eq!(payload["state"], "waiting_input");
    assert_eq!(payload["agent_id"], "s1");
    let keys: Vec<_> = payload.as_object().unwrap().keys().cloned().collect();
    for key in &keys {
        assert!(
            ["agent_id", "agent_name", "worktree", "state", "title", "body"].contains(&key.as_str()),
            "unexpected payload field {key}"
        );
    }
}

#[tokio::test]
async fn long_turns_mode_sends_only_after_a_long_turn() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    let device = paired_device(&state);
    state
        .push
        .store
        .subscribe(&device, mock.subscription("a", false, FinishedMode::LongTurns))
        .unwrap();

    let mut turns = TurnTracker::default();
    let t0 = Instant::now();
    turns.observe("s1", AgentControlState::Working, t0);
    let short = turns
        .observe("s1", AgentControlState::Idle, t0 + Duration::from_secs(10))
        .expect("a finished turn");
    assert_eq!(short, PushKind::Finished(Duration::from_secs(10)));
    assert_eq!(state.dispatch_push("s1", short).await, 0);
    assert!(mock.received().is_empty(), "a 10s turn sends nothing in longTurns mode");

    let t1 = t0 + Duration::from_secs(20);
    turns.observe("s1", AgentControlState::Working, t1);
    turns.observe("s1", AgentControlState::Working, t1 + Duration::from_secs(30));
    let long = turns
        .observe("s1", AgentControlState::Idle, t1 + Duration::from_secs(90))
        .expect("a finished turn");
    assert_eq!(long, PushKind::Finished(Duration::from_secs(90)), "timed from the turn's first Working");
    assert_eq!(state.dispatch_push("s1", long).await, 1);
    let received = mock.received();
    assert_eq!(received.len(), 1);
    assert_eq!(mock.decrypt(&received[0])["state"], "finished");

    // Idle without a turn in progress is not a finished turn.
    assert_eq!(turns.observe("s1", AgentControlState::Idle, t1 + Duration::from_secs(95)), None);
    assert!(FinishedMode::EveryTurn.allows(Duration::from_secs(1)));
    assert!(!FinishedMode::Off.allows(Duration::from_secs(600)));
}

#[tokio::test]
async fn focused_desktop_sends_nothing_and_a_stale_report_counts_as_unfocused() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    let device = paired_device(&state);
    state
        .push
        .store
        .subscribe(&device, mock.subscription("a", true, FinishedMode::EveryTurn))
        .unwrap();

    state.set_desktop_window_focused(true);
    assert_eq!(state.dispatch_push("s1", PushKind::NeedsInput).await, 0);
    assert_eq!(state.dispatch_push("s1", PushKind::Finished(Duration::from_secs(120))).await, 0);
    assert!(mock.received().is_empty(), "focused desktop suppresses every push");

    let now = Instant::now();
    assert!(desktop_focus_suppresses(Some((true, now)), now + Duration::from_secs(119)));
    assert!(!desktop_focus_suppresses(Some((true, now)), now + DESKTOP_FOCUS_STALE));
    assert!(!desktop_focus_suppresses(Some((false, now)), now));
    assert!(!desktop_focus_suppresses(None, now));

    state.set_desktop_window_focused(false);
    assert_eq!(state.dispatch_push("s1", PushKind::NeedsInput).await, 1);
    assert_eq!(mock.received().len(), 1);
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
    assert_eq!(state.dispatch_push("s1", PushKind::NeedsInput).await, 0);
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

    assert_eq!(state.dispatch_push("machine-1", PushKind::NeedsInput).await, 1);
    assert_eq!(machine_mock.received().len(), 1, "a Machine Control device gets its machine session");
    assert!(mirror_mock.received().is_empty(), "a Mirror device never sees a machine session");

    assert_eq!(state.dispatch_push("desktop-1", PushKind::NeedsInput).await, 1);
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

        assert_eq!(state.dispatch_push("s1", PushKind::NeedsInput).await, 1);
        assert_eq!(mock.received().len(), 1);
        assert!(state.push.store.list_subscriptions().is_empty(), "{answer} deletes the subscription");
        let reloaded = PushStore::load(Some(dir.path().join(PUSH_SUBSCRIPTIONS_FILE)));
        assert!(reloaded.list_subscriptions().is_empty());
    }
}

#[tokio::test]
async fn published_agent_events_drive_the_dispatcher() {
    let dir = tempfile::tempdir().unwrap();
    let state = gateway(dir.path());
    let mut mock = MockPush::start(StatusCode::CREATED).await;
    let device = paired_device(&state);
    state.push.store.subscribe(&device, mock.subscription("a", true, FinishedMode::Off)).unwrap();
    state.start_push_dispatch();

    state.agent_events.publish(draft("s1", AgentControlState::Working));
    state.agent_events.publish(draft("s1", AgentControlState::Blocked));
    let delivery = tokio::time::timeout(Duration::from_secs(10), mock.deliveries.recv())
        .await
        .expect("the dispatcher sends within the bound")
        .expect("a delivery");
    assert_eq!(mock.decrypt(&delivery)["state"], "waiting_input");
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
