use super::*;
use crate::remote::{DeviceAccessScope, DevicePermission};
use crate::remote::server::machine_teardown_probe;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

const DEADLINE: Duration = Duration::from_secs(10);
type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct Gateway(tokio::task::JoinHandle<()>);
impl Drop for Gateway {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn token(daemon: &DaemonServer) -> String {
    let auth = &daemon.remote_state().auth_manager;
    let pin = auth.create_scoped_pairing_code(
        DevicePermission::Control, DeviceAccessScope::Machine,
    ).unwrap();
    auth.exchange_pairing_code(&pin, "real SSH machine regression").unwrap().0
}

async fn control(socket: &mut Socket, value: Value) {
    tokio::time::timeout(DEADLINE, socket.send(Message::Text(value.to_string().into())))
        .await.expect("bounded machine control send").unwrap();
}

async fn text(socket: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(DEADLINE, async {
        loop {
            match socket.next().await.expect("machine socket EOF").unwrap() {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(value["type"], "error", "{value}");
                    if value["type"] == kind { return value; }
                }
                Message::Close(_) => panic!("machine socket closed before {kind}"),
                _ => {}
            }
        }
    }).await.unwrap_or_else(|_| panic!("bounded machine {kind} event"))
}

async fn attach(base: &str, daemon: &DaemonServer, id: &str, token: &str, takeover: bool) -> (Socket, u64) {
    let epoch = daemon.remote_state().daemon_epoch.load(std::sync::atomic::Ordering::Acquire);
    let url = format!("{base}/api/v1/terminal/{id}?daemonEpoch={epoch}&takeover={}", u8::from(takeover));
    let (socket, attached) = open_machine_wire(url, token).await;
    let generation = attached["generation"].as_str().unwrap().parse().unwrap();
    (socket, generation)
}

async fn open_machine_wire(url: String, token: &str) -> (Socket, Value) {
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
        .await.expect("bounded actual SSH machine admission").unwrap();
    let attached = text(&mut socket, "attached").await;
    (socket, attached)
}

async fn output(socket: &mut Socket, marker: &str) {
    tokio::time::timeout(DEADLINE, async {
        let mut bytes = Vec::new();
        loop {
            match socket.next().await.expect("machine output EOF").unwrap() {
                Message::Binary(frame) => {
                    let frame = crate::remote::terminal_wire::decode_frame(&frame).unwrap();
                    bytes.extend_from_slice(frame.terminal_bytes);
                    if String::from_utf8_lossy(&bytes).contains(marker) { break; }
                }
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(value["type"], "error", "{value}");
                }
                Message::Close(_) => panic!("machine socket closed before {marker}"),
                _ => {}
            }
        }
    }).await.unwrap_or_else(|_| panic!("bounded actual SSH output {marker}"));
}

async fn command(socket: &mut Socket, command: &str) {
    tokio::time::timeout(DEADLINE, socket.send(Message::Binary(command.as_bytes().to_vec().into())))
        .await.expect("bounded machine input send").unwrap();
}

async fn resize(socket: &mut Socket, generation: u64, cols: u16, rows: u16) {
    control(socket, json!({"type":"resize", "generation":generation.to_string(), "cols":cols, "rows":rows})).await;
    control(socket, json!({"type":"ping"})).await;
    text(socket, "pong").await;
}

async fn geometry(socket: &mut Socket, cols: u16, rows: u16) {
    // Octal output prefix cannot be satisfied by the terminal's command echo.
    // Each request has a fresh marker, so replay cannot satisfy a later check.
    let marker = format!("SSH-SIZE-{}:", uuid::Uuid::new_v4());
    let encoded: String = marker.bytes().map(|byte| format!("\\{byte:03o}")).collect();
    command(socket, &format!("printf '{encoded}'; stty size\n")).await;
    output(socket, &format!("{marker}{rows} {cols}")).await;
}

async fn close(socket: &mut Socket, id: &str) {
    let (entered, release, finished) = machine_teardown_probe::register(id);
    tokio::time::timeout(DEADLINE, socket.send(Message::Close(None)))
        .await.expect("bounded machine close send").unwrap();
    tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
    release.send(()).unwrap();
    tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
}

async fn mirror_grant_cannot_open_real_ssh_session(daemon: &DaemonServer, base: &str, id: &str) {
    let auth = &daemon.remote_state().auth_manager;
    let pairing_token =
        crate::remote::machine_grant_admission_tests::issue_and_apply(auth, crate::remote::account_protocol::AccountGrantScope::Mirror)
            .await;
    let (device_token, device) = auth
        .exchange_pairing_code(&pairing_token, "SSH mirror gate")
        .expect("mirror pairing token is exchangeable");
    assert_eq!(device.access_scope, DeviceAccessScope::Mirror);
    let epoch = daemon.remote_state().daemon_epoch.load(std::sync::atomic::Ordering::Acquire);
    let mut request = format!("{base}/api/v1/terminal/{id}?daemonEpoch={epoch}")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {device_token}").parse().unwrap(),
    );
    let refusal = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
        .await
        .expect("bounded mirror admission attempt")
        .expect_err("a mirror grant must not open a real SSH session");
    let tokio_tungstenite::tungstenite::Error::Http(response) = refusal else {
        panic!("a refused admission must answer HTTP, not {refusal}");
    };
    assert_eq!(
        response.status().as_u16(),
        403,
        "a mirror grant reaches the desktop-exposure gate, which refuses an unselected SSH pane"
    );
}

async fn machine_grant_admits_real_ssh_session(daemon: &DaemonServer, base: &str, id: &str) {
    let auth = &daemon.remote_state().auth_manager;
    let pairing_token =
        crate::remote::machine_grant_admission_tests::issue_and_apply(auth, crate::remote::account_protocol::AccountGrantScope::Machine)
            .await;
    let (device_token, device) = auth
        .exchange_pairing_code(&pairing_token, "SSH machine grant")
        .expect("machine pairing token is exchangeable");
    assert_eq!(device.permission, DevicePermission::Control);
    assert_eq!(device.access_scope, DeviceAccessScope::Machine);
    let http_base = base.replacen("ws://", "http://", 1);
    let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().unwrap();
    let state = daemon.remote_state();
    let initial = state.active_selection();
    let mut first_row = None;
    for selection in [None, Some(None), Some(Some(id)), Some(Some("other-session"))] {
        state.set_active_selection_opt(selection.map(|sid| crate::remote::protocol::RemoteActiveDesktopSelection {
            session_id: sid.map(str::to_string), ..Default::default()
        }));
        let inventory = client.get(format!("{http_base}/api/v1/sessions"))
            .bearer_auth(&device_token).send().await.unwrap();
        assert_eq!(inventory.status(), reqwest::StatusCode::OK);
        let listing: crate::remote::machine_protocol::Sessions = inventory.json().await.unwrap();
        let row = listing.sessions.into_iter().find(|row| row.target.session_id == id)
            .expect("a machine grant must discover the live SSH session regardless of desktop selection");
        assert!(row.running);
        if let Some(first) = &first_row { assert_eq!(first, &row.target); } else { first_row = Some(row.target); }
    }
    state.set_active_selection_opt(initial);
    let listed = first_row.unwrap();
    let (mut socket, generation) = attach(base, daemon, &listed.session_id, &device_token, false).await;
    resize(&mut socket, generation, 101, 33).await;
    geometry(&mut socket, 101, 33).await;
    close(&mut socket, id).await;

    // The phone requests a grid, not the byte stream exercised above. Use its exact
    // URL and inventory target; only output from the shell can contain the plain marker.
    let url = format!("{base}/api/v1/terminal/{}?daemonEpoch={}&render=grid", listed.session_id, listed.daemon_epoch.0);
    let (mut grid, attached) = open_machine_wire(url, &device_token).await;
    assert_eq!(attached["target"], serde_json::to_value(&listed).unwrap());
    let initial = text(&mut grid, "grid").await;
    assert_eq!((initial["cols"].as_u64(), initial["rows"].as_u64()), (Some(101), Some(33)));
    let marker = format!("GRID-{}", uuid::Uuid::new_v4().simple());
    let octal: String = marker.bytes().map(|byte| format!("\\{byte:03o}")).collect();
    command(&mut grid, &format!("printf '\\n{octal}\\n'\n")).await;
    tokio::time::timeout(DEADLINE, async {
        loop {
            match grid.next().await.expect("phone grid EOF").expect("phone grid frame") {
                Message::Text(text) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(value["type"], "error", "{value}");
                    if super::qa::extract_grid_text(&value).contains(&marker) { break; }
                }
                Message::Close(_) => panic!("phone grid closed before showing SSH output"),
                _ => {}
            }
        }
    }).await.expect("the phone grid displays the result of its own SSH input");
    close(&mut grid, id).await;
    println!("FERRYX_MACHINE_SSH_GRID_OK listed-target rendered-input-output");
}

/// The restore debt belongs to the session, not to the socket that incurred it. The controller
/// that resized is displaced, and its teardown is refused by the generation check, so the
/// successor inherits an already-resized PTY. The successor never resizes, yet its close is what
/// must put the desktop baseline back. The displaced teardown is held open on the probe so the
/// takeover installs its controller first: the ordering is exact, never a race.
async fn baseline_restores_after_a_successor_that_never_resized(
    daemon: &DaemonServer,
    base: &str,
    id: &str,
) {
    let services = daemon.remote_state().machine_services.as_ref().unwrap();
    assert_eq!(
        services.sessions.desktop_geometry(id),
        Some((101, 33)),
        "precondition: the desktop baseline is the spawn geometry"
    );
    let first_token = token(daemon);
    // The previous controller's release reserves the pane for 15s, so a fresh device must state
    // takeover explicitly rather than depend on that window having elapsed.
    let (mut first, generation) = attach(base, daemon, id, &first_token, true).await;
    resize(&mut first, generation, 122, 39).await;
    geometry(&mut first, 122, 39).await;

    let (entered, release, finished) = machine_teardown_probe::register(id);
    let successor_token = token(daemon);
    let (mut second, successor) = attach(base, daemon, id, &successor_token, true).await;
    assert_eq!(successor, generation + 1);
    let displaced = text(&mut first, "terminalOutcome").await;
    assert_eq!(displaced["state"], "blocked");
    assert_eq!(displaced["blockedReason"], "SESSION_OWNERSHIP_CHANGED");
    tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
    // B takes over the resized PTY and never sends a resize of its own.
    geometry(&mut second, 122, 39).await;
    release.send(()).unwrap();
    tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
    geometry(&mut second, 122, 39).await;

    close(&mut second, id).await;
    // The probe-bounded close returns only after the restore block ran, so the settled flag is
    // observable here: a restore that never cleared the debt would repeat on every teardown.
    let debt = services
        .sessions
        .machine_controllers
        .lock()
        .await
        .get(id)
        .map(|controller| controller.resized.load(std::sync::atomic::Ordering::Acquire));
    assert_ne!(
        debt,
        Some(true),
        "a completed baseline restore must settle the session's resize debt"
    );
    // Contract: the pane is back at the desktop baseline the instant the last controller is
    // gone. A per-socket restore gate leaves it at 122x39, because the only controller that
    // resized was displaced and the controller that closed never resized. Daemon bookkeeping
    // first, then the remote PTY, so a failure says which side lost the restore.
    let restored = daemon.terminal_service.remote().details(id).unwrap();
    assert_eq!(
        (restored.descriptor.cols, restored.descriptor.rows),
        (101, 33),
        "closing the last controller must restore the desktop baseline"
    );
    let (mut observer, next) = attach(base, daemon, id, &successor_token, false).await;
    assert_eq!(next, successor + 1);
    geometry(&mut observer, 101, 33).await;
    close(&mut observer, id).await;
    println!("FERRYX_MACHINE_SSH_BASELINE_OK successor-never-resized");
}

pub(super) async fn exercise(daemon: &DaemonServer, id: &str, store: &Path, restored: bool) {
    let original = daemon.terminal_service.remote().details(id).unwrap();
    *daemon.remote_state().ssh_store_path.write() = Some(store.to_path_buf());
    let state = Arc::clone(daemon.remote_state());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    let mut server = Gateway(tokio::spawn(async move {
        axum::serve(listener, crate::remote::create_remote_router(state)).await.unwrap();
    }));
    if !restored {
        mirror_grant_cannot_open_real_ssh_session(daemon, &base, id).await;
        machine_grant_admits_real_ssh_session(daemon, &base, id).await;
    }
    let first_token = token(daemon);
    let services = daemon.remote_state().machine_services.as_ref().unwrap();
    if restored {
        // A fresh daemon restored this id, so no controller lease is outstanding.
        let (mut first, generation) = attach(&base, daemon, id, &first_token, false).await;
        assert_eq!(
            services.sessions.desktop_geometry(id),
            Some((101, 33)),
            "a restored session must carry the persisted descriptor as its desktop baseline"
        );
        geometry(&mut first, 101, 33).await;
        resize(&mut first, generation, 127, 43).await;
        geometry(&mut first, 127, 43).await;
        close(&mut first, id).await;
        let (mut restored_again, _) = attach(&base, daemon, id, &first_token, false).await;
        geometry(&mut restored_again, 101, 33).await;
        close(&mut restored_again, id).await;
        println!("FERRYX_MACHINE_SSH_RESTART_OK remote-stty persisted-geometry new-admission");
    } else {
        // The machine-grant socket just released this pane, which reserves it for that closing
        // device. A fresh device states takeover explicitly instead of waiting out the window.
        let (mut first, generation) = attach(&base, daemon, id, &first_token, true).await;
        assert_eq!(
            services.sessions.desktop_geometry(id),
            Some((101, 33)),
            "spawn dimensions must be the desktop geometry baseline"
        );
        resize(&mut first, generation, 113, 37).await;
        geometry(&mut first, 113, 37).await;

        // Displaced teardown is held before geometry fencing. The successor must
        // resize first, then the old teardown must not overwrite the winner.
        let (entered, release, finished) = machine_teardown_probe::register(id);
        let second_token = token(daemon);
        let (mut second, successor) = attach(&base, daemon, id, &second_token, true).await;
        assert_eq!(successor, generation + 1);
        let displaced = text(&mut first, "terminalOutcome").await;
        assert_eq!(displaced["state"], "blocked");
        assert_eq!(displaced["blockedReason"], "SESSION_OWNERSHIP_CHANGED");
        assert_eq!(displaced["generation"], generation.to_string());
        assert_eq!(displaced["target"]["sessionId"], id);
        tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
        resize(&mut second, successor, 119, 41).await;
        geometry(&mut second, 119, 41).await;
        release.send(()).unwrap();
        tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
        geometry(&mut second, 119, 41).await;

        control(&mut second, json!({"type":"resize", "generation":generation.to_string(), "cols":70, "rows":20})).await;
        let error = tokio::time::timeout(DEADLINE, async {
            loop {
                match second.next().await.expect("stale control EOF").unwrap() {
                    Message::Text(text) => {
                        let value: Value = serde_json::from_str(&text).unwrap();
                        if value["type"] == "error" { break value; }
                    }
                    Message::Close(_) => panic!("closed before stale-control error"),
                    _ => {}
                }
            }
        }).await.unwrap();
        assert_eq!(error["code"], "INVALID_CONTROL_OR_GENERATION");
        control(&mut second, json!({"type":"ping"})).await;
        text(&mut second, "pong").await;
        geometry(&mut second, 119, 41).await;

        // Perl installs SIGINT before READY and blocks in a read, not a sleep.
        // Hex-encoded sentinels prevent PTY echo from passing either assertion.
        command(&mut second, "stty isig -echo; perl -e '$|=1; $SIG{INT}=sub { print pack(\"H*\",\"5353482d494e544552525550542d4f4b0a\"); exit 0 }; print pack(\"H*\",\"5353482d494e544552525550542d52454144590a\"); while (1) { sysread(STDIN, my $b, 1) }'\n").await;
        output(&mut second, "SSH-INTERRUPT-READY").await;
        control(&mut second, json!({"type":"signal", "generation":successor.to_string(), "signal":"interrupt"})).await;
        output(&mut second, "SSH-INTERRUPT-OK").await;
        control(&mut second, json!({"type":"ping"})).await;
        text(&mut second, "pong").await;
        command(&mut second, "stty echo\n").await;
        geometry(&mut second, 119, 41).await;

        close(&mut second, id).await;
        // The exact teardown completion precedes re-admission: baseline restore
        // must be visible on the remote PTY, not just daemon bookkeeping.
        let (mut reattached, next) = attach(&base, daemon, id, &second_token, false).await;
        assert_eq!(next, successor + 1);
        geometry(&mut reattached, 101, 33).await;
        close(&mut reattached, id).await;
        baseline_restores_after_a_successor_that_never_resized(daemon, &base, id).await;
        println!("FERRYX_MACHINE_SSH_OK takeover generation-fence teardown-geometry ETX reconnect-geometry successor-baseline-restore");
    }
    let final_details = daemon.terminal_service.remote().details(id).unwrap();
    assert_eq!(final_details.pid, original.pid);
    assert_eq!(final_details.descriptor.target, original.descriptor.target);
    assert_eq!(final_details.generation, original.generation);
    server.0.abort();
    assert!((&mut server.0).await.as_ref().is_err_and(|error| error.is_cancelled()));
}
