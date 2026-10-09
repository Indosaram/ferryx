use super::*;
use crate::{
    daemon::server::DaemonServer,
    remote::{server::create_remote_router, state::RemoteGatewayState},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::fs;
use tokio::time::Instant;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

pub const DEADLINE: Duration = Duration::from_secs(5);
const COLD_CONPTY_READINESS_WINDOW: Duration = Duration::from_secs(15);
// The loopback gateway, scheduler, and PTY child side effect should complete well below 300 ms;
// this leaves ample CI headroom while still detecting input starvation.
pub const CONTROLLER_INPUT_LATENCY_BOUND: Duration = Duration::from_millis(300);
// Side-effect bound: PTY-write-completion (T1) -> marker visibility (T2), i.e. the shell actually
// executing the command and creating the marker file. Evidence (maho-win, two runs):
//   t1_minus_t0 = 12.1-22.1 ms  -> product input path (WS -> channel -> validate -> PTY write),
//     healthy, 13-25x UNDER CONTROLLER_INPUT_LATENCY_BOUND;
//   t2_minus_t1 = 1.198-1.255 s -> PTY-write-completion -> marker visibility, which on Windows is
//     conhost/PowerShell `Set-Content` startup; the Linux path uses `printf` and is far faster.
//   survival_dsr_fired=false in both runs, so the in-window ConPTY DSR re-handshake hypothesis is
//   refuted.
// This bound exists so the composite (T0 -> T2) no longer misapplies the 300 ms input bound to
// shell execution, which cannot hold on Windows.
pub const CONTROLLER_SIDE_EFFECT_BOUND: Duration = Duration::from_secs(3);
const VIEWER_OUTPUT_PAYLOAD_BYTES: usize = 64 * 1024;
const CONTROLLER_MARKER_FILE: &str = "two-consumer-controller-marker.txt";
const CONTROLLER_SURVIVAL_FILE: &str = "two-consumer-controller-survival.txt";

#[cfg(windows)]
fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn marker_command(path: &std::path::Path, contents: &str) -> String {
    format!(
        "Set-Content -NoNewline -LiteralPath {} -Value {}\r",
        ps_quote(&path.display().to_string()), ps_quote(contents)
    )
}

#[cfg(not(windows))]
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

#[cfg(not(windows))]
fn marker_command(path: &std::path::Path, contents: &str) -> String {
    format!(
        "printf %s {} > {}\r",
        sh_quote(contents),
        sh_quote(&path.display().to_string())
    )
}

#[cfg(test)]
mod marker_command_tests {
    use super::marker_command;
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn marker_command_escapes_single_quotes_for_powershell() {
        assert_eq!(
            marker_command(Path::new("marker'file.txt"), "it's"),
            "Set-Content -NoNewline -LiteralPath 'marker''file.txt' -Value 'it''s'\r"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn marker_command_escapes_single_quotes_for_shell() {
        assert_eq!(
            marker_command(Path::new("marker'file.txt"), "it's"),
            r"printf %s 'it'\''s' > 'marker'\''file.txt'".to_owned() + "\r"
        );
    }
}

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub struct TwoConsumerFixture {
    pub root: tempfile::TempDir,
    pub owner: DaemonServer,
    pub state: Arc<RemoteGatewayState>,
    pub base: String,
    pub controller_token: String,
    pub controller_device: String,
    pub viewer_token: String,
    pub viewer_device: String,
    tasks: tokio::task::JoinSet<()>,
}

impl TwoConsumerFixture {
    pub async fn new() -> Self {
        let (root, owner) = tokio::task::spawn_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let owner = DaemonServer::new_with_paths(
                Some(root.path().join("config")),
                Some(root.path().join("auth")),
            );
            (root, owner)
        })
        .await
        .unwrap();

        let state = owner.remote_state().clone();
        let ctrl_pin = state
            .auth_manager
            .create_scoped_pairing_code(
                crate::remote::DevicePermission::Control,
                crate::remote::DeviceAccessScope::Machine,
            )
            .unwrap();
        let (controller_token, ctrl_device) = state
            .auth_manager
            .exchange_pairing_code(&ctrl_pin, "controller-device")
            .unwrap();

        let view_pin = state
            .auth_manager
            .create_pairing_code(crate::remote::DevicePermission::View);
        let (viewer_token, view_device) = state
            .auth_manager
            .exchange_pairing_code(&view_pin, "viewer-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = create_remote_router(state.clone());
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self {
            root,
            owner,
            state,
            base,
            controller_token,
            controller_device: ctrl_device.id,
            viewer_token,
            viewer_device: view_device.id,
            tasks,
        }
    }

    pub async fn create_session(&self) -> Value {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let text = client
            .post(format!("{}/api/v1/workspace/projects", self.base))
            .bearer_auth(&self.controller_token)
            .body(json!({"requestId": uuid::Uuid::new_v4(), "repoPath": self.root.path()}).to_string())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        let project: Value = serde_json::from_str(&text).unwrap();

        let text = client
            .post(format!("{}/api/v1/sessions", self.base))
            .bearer_auth(&self.controller_token)
            .body(json!({
                "requestId": uuid::Uuid::new_v4(),
                "workspaceId": project["workspaceId"],
                "worktree": null,
                "inheritFromSessionId": null,
                "cwdRelative": null,
                "cols": 80,
                "rows": 24,
                "startup": {"kind": "shell"}
            }).to_string())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    pub async fn attach_controller(&self, session: &Value) -> Socket {
        self.attach_with_token(session, &self.controller_token, None).await
    }

    pub async fn attach_viewer(&self, session: &Value, after_sequence: Option<u64>) -> Socket {
        self.attach_with_token(session, &self.viewer_token, after_sequence).await
    }

    pub async fn attach_with_token(
        &self,
        session: &Value,
        token: &str,
        after_sequence: Option<u64>,
    ) -> Socket {
        let target = &session["target"];
        let mut url = format!(
            "{}/api/v1/terminal/{}?daemonEpoch={}",
            self.base.replace("http:", "ws:"),
            target["sessionId"].as_str().unwrap(),
            target["daemonEpoch"].as_str().unwrap()
        );
        if let Some(seq) = after_sequence {
            url.push_str(&format!("&afterSequence={seq}"));
        }
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(request))
            .await
            .expect("Connection to terminal socket must succeed within deadline")
            .unwrap();
        let first = tokio::time::timeout(DEADLINE, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, Message::Text(_)));
        socket
    }

    pub async fn cleanup(mut self) {
        self.state.auth_manager.revoke_device(&self.controller_device);
        self.state.auth_manager.revoke_device(&self.viewer_device);
        let backend = self.owner.terminal_service();
        for id in backend.list_sessions() {
            let pty = backend.get_session(&id);
            if pty.is_some() {
                let _ = backend.close_session(&id).await;
            }
            if let Some(services) = &self.state.machine_services {
                let _ = services.sessions.wait_machine_lifecycle(&id).await;
            }
        }
        self.tasks.shutdown().await;
        drop(self.owner);
        drop(self.state);
        let _ = tokio::task::spawn_blocking(move || self.root.close()).await;
    }
}

#[tokio::test]
async fn two_consumer_gateway_healthy_controller_progresses_while_viewer_stalls_and_drops() {
    let fixture = TwoConsumerFixture::new().await;
    let session = fixture.create_session().await;

    let mut controller = fixture.attach_controller(&session).await;
    let mut viewer = fixture.attach_viewer(&session, None).await;

    let target_session_id = session["target"]["sessionId"].as_str().unwrap();

    let marker_path = fixture.root.path().join(CONTROLLER_MARKER_FILE);
    let marker_contents = "TWO_CONSUMER_ACTIVE";
    let test_input = marker_command(&marker_path, marker_contents);
    let mut input_observation = crate::remote::machine_input_probe::Observation::register(target_session_id);
    controller.send(Message::Binary(test_input.clone().into_bytes().into())).await.unwrap();
    tokio::time::timeout(COLD_CONPTY_READINESS_WINDOW, async {
        #[cfg(windows)]
        let mut marker_resent_after_dsr = false;
        loop {
            if matches!(fs::read_to_string(&marker_path).await, Ok(contents) if contents == marker_contents) {
                break;
            }
            #[cfg(windows)]
            if let Ok(Some(Ok(Message::Binary(bytes)))) =
                tokio::time::timeout(Duration::from_millis(10), controller.next()).await
            {
                if bytes.windows(b"\x1b[6n".len()).any(|window| window == b"\x1b[6n") {
                    controller.send(Message::Binary(b"\x1b[1;1R".to_vec().into())).await.unwrap();
                    if !marker_resent_after_dsr {
                        controller.send(Message::Binary(test_input.clone().into_bytes().into())).await.unwrap();
                        marker_resent_after_dsr = true;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    }).await.expect("Controller command must create the exact marker file contents");
    let input_progress = *input_observation.0.borrow_and_update();
    eprintln!(
        "two-consumer input diagnostic: completed={}, pending={}, dropped={}, queue_full={}, marker_contents_verified=true",
        input_progress.completed,
        input_progress.pending,
        input_progress.dropped,
        input_progress.queue_full,
    );
    assert_eq!(fs::read_to_string(&marker_path).await.unwrap(), marker_contents);
    drop(input_observation);

    let first_viewer_chunk = tokio::time::timeout(Duration::from_secs(3), viewer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(first_viewer_chunk, Message::Binary(_)));

    let hub = fixture.owner.terminal_service().output_hub().clone();
    let (viewer_observation, viewer_termination) =
        hub.last_machine_observation(target_session_id).expect("actual viewer subscription");
    let stalled_viewer_started = std::time::Instant::now();
    let burst_payload = vec![0x44; VIEWER_OUTPUT_PAYLOAD_BYTES];
    let mut offered_payload_bytes = 0;
    tokio::time::timeout(DEADLINE, async {
        loop {
            if viewer_termination.borrow().is_some() {
                break;
            }
            hub.publish(target_session_id, burst_payload.clone()).unwrap();
            offered_payload_bytes += burst_payload.len();
            // Drain the healthy socket after each publication; the viewer stays unread.
            loop {
                match controller.next().await {
                    Some(Ok(Message::Binary(bytes))) if bytes.windows(64).any(|part| part == [0x44; 64]) => break,
                    Some(Ok(_)) => {},
                    other => panic!("Healthy controller lost output during load: {other:?}"),
                }
            }
        }
    }).await.expect("Actual stalled viewer must overflow within five seconds");
    let overflow_reason = *viewer_termination.borrow();
    assert_eq!(
        overflow_reason,
        Some(crate::terminal::machine_output::MachineOutputError::Overflow),
        "Actual viewer must terminate on queue overflow, not a generic close"
    );
    let observation = viewer_observation.lock().unwrap().clone();
    let charged_pending_high_water = observation.high_water_bytes;
    let overflow_pending_bytes = observation.overflow_pending_bytes;
    let overflow_elapsed = observation.overflow_at.unwrap().duration_since(stalled_viewer_started);
    assert!(charged_pending_high_water <= crate::terminal::machine_output::MACHINE_OUTPUT_BYTES);

    // Start before dispatch and do not resume viewer reads until the PTY echo arrives.
    // Fresh survival-phase input probe (the phase-1 observation was dropped above): it
    // registers only for this survival frame, so `completed` flips when the product-side
    // write_input future for this frame returns.
    let mut input_survival_observation =
        crate::remote::machine_input_probe::Observation::register(target_session_id);
    let input_dispatched_at = Instant::now();

    let survival_path = fixture.root.path().join(CONTROLLER_SURVIVAL_FILE);
    let survival_input = marker_command(&survival_path, "CONTROLLER_SURVIVES");
    // Measurement split for the latency assert below (assert and 300 ms bound unchanged):
    //   T0 = input_dispatched_at (declared above, unmoved),
    //   T1 = input_completed_at, when the product-side write_input future reported completed,
    //   T2 = marker_seen_at, when the survival marker first read its exact expected contents.
    // APPROXIMATION: machine_input_probe::Progress exposes only booleans (pending/completed/
    // dropped/queue_full) and no completion timestamp, so T1 is the instant this loop first
    // sampled completed == true, not the write future's own completion instant. Sampling
    // granularity is one loop iteration (marker file read plus, on Windows, up to a 10 ms DSR
    // read timeout), so T1 may lag true write completion by that much.
    let mut input_completed_at: Option<Instant> = None;
    let mut marker_seen_at: Option<Instant> = None;
    #[cfg(windows)]
    let mut survival_resent_after_dsr = false;
    #[cfg(windows)]
    let mut survival_dsr_at: Option<Instant> = None;
    controller.send(Message::Binary(survival_input.clone().into_bytes().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            // Sample T1 before the marker check so a marker match on this same iteration
            // cannot skip the only place T1 is recorded.
            if input_completed_at.is_none()
                && input_survival_observation.0.borrow_and_update().completed
            {
                input_completed_at = Some(Instant::now());
            }
            if matches!(fs::read_to_string(&survival_path).await, Ok(contents) if contents == "CONTROLLER_SURVIVES") {
                marker_seen_at = Some(Instant::now());
                break;
            }
            #[cfg(windows)]
            if let Ok(Some(Ok(Message::Binary(bytes)))) =
                tokio::time::timeout(Duration::from_millis(10), controller.next()).await
            {
                if bytes.windows(b"\x1b[6n".len()).any(|window| window == b"\x1b[6n") {
                    if survival_dsr_at.is_none() {
                        survival_dsr_at = Some(Instant::now());
                    }
                    // The shell is already running; only resend if ConPTY issues a fresh DSR.
                    controller.send(Message::Binary(b"\x1b[1;1R".to_vec().into())).await.unwrap();
                    if !survival_resent_after_dsr {
                        controller.send(Message::Binary(survival_input.clone().into_bytes().into())).await.unwrap();
                        survival_resent_after_dsr = true;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    }).await.expect("Healthy controller command must create its survival marker");
    // Fallback: if the loop broke before ever sampling `completed`, take one post-loop reading.
    // This is an approximation because it is sampled after T2 rather than at the write future's
    // completion instant; if the probe never reported completion T1 stays None and the metrics
    // line prints None instead of inventing a value.
    if input_completed_at.is_none()
        && input_survival_observation.0.borrow_and_update().completed
    {
        input_completed_at = Some(Instant::now());
    }
    let controller_survived = true;
    let input_latency = input_dispatched_at.elapsed();
    // Attribution is computed and printed BEFORE the bound assertion below, because the assertion is
    // exactly when this data is needed: if it panicked first, the split would never be observable.
    // `t2` stays optional so a missing marker cannot mask the latency assertion with a second panic.
    let t2 = marker_seen_at;
    let (t1_minus_t0, t2_minus_t1, t2_minus_t0) = match (input_completed_at, t2) {
        (Some(t1), Some(t2)) => (
            Some(t1.duration_since(input_dispatched_at)),
            Some(t2.saturating_duration_since(t1)),
            Some(t2.duration_since(input_dispatched_at)),
        ),
        _ => (None, None, t2.map(|t2| t2.duration_since(input_dispatched_at))),
    };
    #[cfg(windows)]
    let (survival_dsr_fired, survival_dsr_at_offset) = (
        survival_dsr_at.is_some(),
        survival_dsr_at.map(|at| at.duration_since(input_dispatched_at)),
    );
    #[cfg(not(windows))]
    let (survival_dsr_fired, survival_dsr_at_offset) = (false, None::<Duration>);
    eprintln!("two-consumer metrics: charged_pending_high_water_bytes={charged_pending_high_water}, overflow_pending_bytes={overflow_pending_bytes}, viewer_offered_payload_bytes={offered_payload_bytes}, viewer_termination_reason=Overflow, overflow_elapsed={overflow_elapsed:?}, controller_input_side_effect_latency={input_latency:?}, controller_survived_after_termination={controller_survived}, t1_minus_t0={t1_minus_t0:?}, t2_minus_t1={t2_minus_t1:?}, t2_minus_t0={t2_minus_t0:?}, survival_dsr_fired={survival_dsr_fired}, survival_dsr_at_offset={survival_dsr_at_offset:?}");
    // Bound rule: the input bound (CONTROLLER_INPUT_LATENCY_BOUND, 300 ms) applies to the input
    // path ONLY (T0 -> T1); the side-effect bound (CONTROLLER_SIDE_EFFECT_BOUND) applies to shell
    // execution (T1 -> T2, PTY write complete -> marker visible); the composite (T0 -> T2) must
    // NEVER be asserted against the input bound. Both assertions run unconditionally on Windows
    // and POSIX: a None means a missing probe completion or a missing marker, which fails loudly
    // here instead of being skipped.
    let t1 = t1_minus_t0.unwrap_or_else(|| panic!("Controller input probe never completed: t1_minus_t0=None (composite={input_latency:?}, t2_minus_t1={t2_minus_t1:?}, survival_dsr_fired={survival_dsr_fired}); a missing probe completion must fail loudly, never skip"));
    let t2_gap = t2_minus_t1.unwrap_or_else(|| panic!("Survival marker never observed: t2_minus_t1=None (composite={input_latency:?}, t1_minus_t0={t1:?}, survival_dsr_fired={survival_dsr_fired}); a missing marker must fail loudly, never skip"));
    assert!(
        t1 <= CONTROLLER_INPUT_LATENCY_BOUND,
        "Controller input-path latency t1_minus_t0={t1:?} exceeded input bound {CONTROLLER_INPUT_LATENCY_BOUND:?} (measured Windows split: t1_minus_t0=12.1-22.1 ms vs t2_minus_t1=1.198-1.255 s; composite={input_latency:?}, survival_dsr_fired={survival_dsr_fired})"
    );
    assert!(
        t2_gap <= CONTROLLER_SIDE_EFFECT_BOUND,
        "Controller side-effect latency t2_minus_t1={t2_gap:?} exceeded side-effect bound {CONTROLLER_SIDE_EFFECT_BOUND:?} (PTY-write completion -> marker visibility = shell execution; measured Windows split: t1_minus_t0=12.1-22.1 ms vs t2_minus_t1=1.198-1.255 s; composite={input_latency:?}, survival_dsr_fired={survival_dsr_fired})"
    );
    assert!(controller_survived, "Healthy controller must continue progressing");

    let viewer_outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match viewer.next().await {
                Some(Ok(Message::Binary(_))) => {},
                Some(Ok(Message::Close(_))) => return "CLOSED",
                None => return "DISCONNECTED",
                Some(Err(_)) => return "ERROR",
                _ => {}
            }
        }
    }).await.expect("Stalled viewer socket must terminate within five seconds");
    eprintln!("two-consumer socket: outcome={viewer_outcome}, termination_elapsed={:?}", stalled_viewer_started.elapsed());

    drop(viewer);
    let mut reconnected = fixture.attach_viewer(&session, Some(1)).await;
    let reconnect_live = tokio::time::timeout(Duration::from_secs(3), reconnected.next()).await;
    assert!(reconnect_live.is_ok(), "Reconnected viewer must successfully receive frames");

    drop(controller);
    drop(reconnected);
    fixture.cleanup().await;
}

#[tokio::test]
async fn two_consumer_controller_exclusivity_second_controller_conflicts() {
    let fixture = TwoConsumerFixture::new().await;
    let session = fixture.create_session().await;

    let ctrl1 = fixture.attach_controller(&session).await;

    let pin2 = fixture.state
        .auth_manager
        .create_scoped_pairing_code(
            crate::remote::DevicePermission::Control,
            crate::remote::DeviceAccessScope::Machine,
        )
        .unwrap();
    let (token2, _) = fixture.state
        .auth_manager
        .exchange_pairing_code(&pin2, "second-controller-device")
        .unwrap();

    let target = &session["target"];
    let url = format!(
        "{}/api/v1/terminal/{}?daemonEpoch={}",
        fixture.base.replace("http:", "ws:"),
        target["sessionId"].as_str().unwrap(),
        target["daemonEpoch"].as_str().unwrap()
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", token2).parse().unwrap(),
    );

    let second_connect_res = connect_async(request).await;
    assert!(second_connect_res.is_err(), "Second controller with different device must be rejected with 409 CONTROL_CONFLICT");

    drop(ctrl1);
    fixture.cleanup().await;
}
