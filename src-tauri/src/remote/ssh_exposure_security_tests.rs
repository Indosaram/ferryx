//! W6: exercise the real credential, selection and terminal-upgrade path.
use super::*;
use crate::terminal::remote::{
    RemoteConnectionState, RemoteExportState, RemoteSessionConfig, RemoteSessionDescriptor,
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error};

fn connected_ssh(state: &RemoteGatewayState) {
    connected_ssh_in_project(state, "project");
}

fn ssh_descriptor(project_id: &str) -> RemoteSessionDescriptor {
    RemoteSessionDescriptor {
        backend_session_id: "ssh-session".into(),
        target: crate::scoped_contracts::TargetRef {
            host_id: "test-host".into(),
            owner_id: "test-owner".into(),
            epoch: crate::scoped_contracts::Epoch(42),
            backend_session_id: "remote-target".into(),
        },
        config: RemoteSessionConfig {
            host: crate::ssh::SshHost {
                id: "test-host".into(),
                label: "test-host".into(),
                hostname: "127.0.0.1".into(),
                username: None,
                port: None,
                identity_file: None,
                jump_host: None,
                source: crate::ssh::SshHostSource::Config,
                auth_method: crate::ssh::SshAuthMethod::Agent,
                disabled: None,
                shell: None,
            },
            environment: crate::ssh::runtime::RemoteEnvironment {
                platform: crate::ssh::runtime::RemotePlatform::Posix,
                executor: crate::ssh::runtime::RemoteExecutor::Sh,
                version: "test".into(),
                home: "/home".into(),
                temp: "/tmp".into(),
                git: true,
            },
            helper: crate::ssh::helper_setup::HelperLocation {
                executable: "/unused/helper".into(),
                root: "/unused".into(),
            },
            project_id: project_id.into(),
            project_path: "/project".into(),
            worktree: None,
            agent_identity: None,
        },
        client_request_id: "ssh-exposure-fixture".into(),
        remote_cursor: crate::ssh::bridge::RemoteCursor(0),
        cols: 80,
        rows: 24,
    }
}

fn connected_ssh_in_project(state: &RemoteGatewayState, project_id: &str) {
    // Live import with no transport installs a connected runtime entry without
    // dialing SSH, spawning a PTY, scheduling reconnects or waiting for a clock.
    state
        .terminal_service
        .remote()
        .live_import(
            RemoteExportState {
                descriptor: ssh_descriptor(project_id),
                generation: 1,
                pending_size: None,
                pid: None,
                bridge_transfer: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        state.terminal_service.remote().details("ssh-session").unwrap().state,
        RemoteConnectionState::Connected
    );
}

/// A connector that can only fail, with a retry delay that never elapses: the runtime parks
/// the restored descriptor in `Reconnecting` and stays there without dialing, planned backoff
/// or any wall-clock wait. That is exactly the state a daemon restart leaves behind.
#[cfg(unix)]
struct NeverConnects;

#[cfg(unix)]
impl crate::terminal::remote::Connector for NeverConnects {
    fn connect<'a>(
        &'a self,
        _: &'a RemoteSessionDescriptor,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Arc<dyn crate::terminal::remote::Transport>,
                        crate::ssh::bridge::BridgeError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Err(crate::ssh::bridge::BridgeError::ConnectionClosed) })
    }
    fn delay(&self, _: u32) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(std::future::pending())
    }
}

/// `restore` is the restart path: it registers the persisted identity and parks the session in
/// `Reconnecting` synchronously, before any redial can land.
#[cfg(unix)]
fn state_with_restored_ssh(project_id: &str) -> Arc<RemoteGatewayState> {
    let terminal_service = Arc::new(
        TerminalService::with_remote_connector_for_test(Arc::new(NeverConnects)),
    );
    let state = Arc::new(RemoteGatewayState::new(
        terminal_service,
        crate::worktree::WorkspaceRegistry::new(),
    ));
    state
        .terminal_service
        .remote()
        .restore(ssh_descriptor(project_id))
        .expect("restore the persisted SSH descriptor");
    assert_eq!(
        state.terminal_service.remote().details("ssh-session").unwrap().state,
        RemoteConnectionState::Reconnecting
    );
    state
}

#[tokio::test]
async fn connected_ssh_requires_exact_desktop_exposure_for_view_and_control() {
    let state = state();
    connected_ssh(&state);
    // TerminalService describes local PTYs only; SSH validity must use the
    // connected runtime fallback in the production handler.
    assert!(state.session_backend.describe_session("ssh-session").await.is_err());
    let (view, _) = pair(&state, DevicePermission::View);
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (shape, selection, expected) in [
        ("none", None, 403),
        ("no-session", Some(None), 403),
        ("matching", Some(Some("ssh-session")), 101),
        ("other", Some(Some("other-session")), 403),
    ] {
        state.set_active_selection_opt(selection.map(|session_id| RemoteActiveDesktopSelection {
            session_id: session_id.map(str::to_string),
            ..Default::default()
        }));
        for (permission, token) in [("view", &view), ("control", &control)] {
            for target in ["ssh-session", "test-host::ssh-session"] {
                for render in ["", "?render=grid"] {
                    let mut request = format!(
                        "ws://{}/api/v1/terminal/{target}{render}", server.addr
                    ).into_client_request().unwrap();
                    request.headers_mut().insert(
                        "Authorization",
                        format!("Bearer {token}").parse().unwrap(),
                    );
                    // Await the actual parsed HTTP upgrade response, not a sleep
                    // or a single TCP read that could contain a partial header.
                    let result = tokio::time::timeout(
                        DEADLINE, tokio_tungstenite::connect_async(request)
                    ).await.expect("bounded exposure handshake");
                    let status = match result {
                        Ok((socket, response)) => {
                            drop(socket);
                            response.status().as_u16()
                        }
                        Err(Error::Http(response)) => response.status().as_u16(),
                        Err(error) => panic!("unexpected handshake failure: {error}"),
                    };
                    assert_eq!(status, expected, "{shape}/{permission}/{target}{render}");
                }
            }
        }
    }
    server.stop().await;
}

#[tokio::test]
async fn paired_session_requires_exact_desktop_exposure_for_view_and_control() {
    use crate::scoped_contracts::Epoch;
    use crate::terminal::paired_daemon::{Descriptor, Proxy};
    let state = state();
    let proxy = Proxy::new(Descriptor {
        host_id: "https://paired-exposure.invalid".into(),
        generation: Epoch(1),
        target: crate::remote::machine_protocol::RemoteTerminalTarget {
            machine_id: "paired-exposure".into(),
            daemon_epoch: Epoch(1),
            session_id: uuid::Uuid::new_v4().to_string(),
        },
        after_sequence: None,
    }, state.terminal_service.output_hub().clone()).unwrap();
    let id = state.terminal_service.paired().install(proxy).unwrap();
    let (view, _) = pair(&state, DevicePermission::View);
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (selection, expected) in [
        (None, 403), (Some(None), 403),
        (Some(Some(id.as_str())), 101), (Some(Some("other-session")), 403),
    ] {
        state.set_active_selection_opt(selection.map(|session_id| RemoteActiveDesktopSelection {
            session_id: session_id.map(str::to_string), ..Default::default()
        }));
        for token in [&view, &control] {
            let mut request = format!("ws://{}/api/v1/terminal/{id}", server.addr)
                .into_client_request().unwrap();
            request.headers_mut().insert("Authorization", format!("Bearer {token}").parse().unwrap());
            let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await.unwrap();
            let status = match result {
                Ok((socket, response)) => { drop(socket); response.status().as_u16() },
                Err(Error::Http(response)) => response.status().as_u16(),
                Err(error) => panic!("unexpected handshake failure: {error}"),
            };
            assert_eq!(status, expected);
        }
    }
    state.terminal_service.paired().detach(&id).await.unwrap();
    server.stop().await;
}

async fn assert_mirror_ssh_inventory(state: Arc<RemoteGatewayState>) {
    let (view, _) = pair(&state, DevicePermission::View);
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (shape, selection) in [("none", None), ("empty", Some(None)),
        ("matching", Some(Some("ssh-session"))), ("other", Some(Some("other-session")))] {
        state.set_active_selection_opt(selection.map(|sid| RemoteActiveDesktopSelection {
            session_id: sid.map(str::to_string), ..Default::default()
        }));
        let exposed = selection == Some(Some("ssh-session"));
        for token in [&view, &control] {
            let (status, body) = server.request("GET", "/api/v1/sessions", Some(token.as_str()), None).await;
            assert_eq!(status, 200, "{shape}: {body}");
            let rows: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
            if exposed {
                assert_eq!(rows.len(), 1, "{shape}: {body}");
                assert_eq!(rows[0]["sessionId"], "ssh-session");
                assert_eq!(rows[0]["running"], true);
            } else { assert!(rows.is_empty(), "{shape}: private session in mirror list: {body}"); }
            let (status, body) = server.request("GET", "/api/v1/workspace/state", Some(token.as_str()), None).await;
            assert_eq!(status, 200, "{shape}: {body}");
            let workspace: serde_json::Value = serde_json::from_str(&body).unwrap();
            let listed = workspace["sessions"].as_array().unwrap().iter().any(|row| row["sessionId"] == "ssh-session");
            assert_eq!(listed, exposed, "{shape}: workspace inventory");
            if !exposed {
                assert_ne!(workspace["activeContext"]["sessionId"], "ssh-session", "{shape}: synthesized active session");
            }
        }
    }
    server.stop().await;
}

#[tokio::test]
async fn mirror_lists_only_the_selected_connected_ssh_session() {
    let state = state();
    connected_ssh(&state);
    assert_mirror_ssh_inventory(state).await;
}

#[tokio::test]
async fn session_list_hides_connected_ssh_session_of_a_removed_project() {
    let state = state();
    connected_ssh_in_project(&state, "ssh:removed-project");
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (shape, selection) in [
        ("none", None),
        ("no-session", Some(None)),
        ("matching", Some(Some("ssh-session"))),
        ("other", Some(Some("other-session"))),
    ] {
        state.set_active_selection_opt(selection.map(|session_id| {
            RemoteActiveDesktopSelection {
                session_id: session_id.map(str::to_string),
                ..Default::default()
            }
        }));
        let (status, body) = server
            .request("GET", "/api/v1/sessions", Some(control.as_str()), None)
            .await;
        assert_eq!(status, 200, "{shape}: list refused: {body}");
        assert_eq!(body.trim(), "[]", "{shape}: a removed SSH project was listed: {body}");
    }
    server.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn mirror_lists_only_the_selected_reconnecting_ssh_session() {
    assert_mirror_ssh_inventory(state_with_restored_ssh("project")).await;
}

/// The removed-project rule is about inventory, not transport state: a restored session whose
/// project is missing from the enabled SSH inventory stays hidden in the same `Reconnecting`
/// state that made it visible above. Explicit `Some(None)` (a desktop with no selected session)
/// is the shape that must not resurrect it through the desktop-absent fallback.
#[cfg(unix)]
#[tokio::test]
async fn session_list_hides_restored_reconnecting_ssh_session_of_a_removed_project() {
    let state = state_with_restored_ssh("ssh:removed-project");
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (shape, selection) in [
        ("none", None),
        ("no-session", Some(None)),
        ("matching", Some(Some("ssh-session"))),
        ("other", Some(Some("other-session"))),
    ] {
        state.set_active_selection_opt(selection.map(|session_id| {
            RemoteActiveDesktopSelection {
                session_id: session_id.map(str::to_string),
                ..Default::default()
            }
        }));
        let (status, body) = server
            .request("GET", "/api/v1/sessions", Some(control.as_str()), None)
            .await;
        assert_eq!(status, 200, "{shape}: list refused: {body}");
        assert_eq!(
            body.trim(),
            "[]",
            "{shape}: a removed SSH project was listed: {body}"
        );
    }
    server.stop().await;
}

/// The listed row is an inventory fact, not an exposure grant. Mirror credentials keep the exact
/// selection rule; a Machine-scope Control device is routed to the machine endpoint instead, which
/// is where account inventory attaches and takes over. This fixture has no machine services, so
/// the machine branch answers MACHINE_SERVICE_UNAVAILABLE: reaching it at all proves the ruler
/// below it (the Mirror exposure gate) was never consulted.
#[tokio::test]
async fn session_list_routes_machine_scope_to_machine_endpoint_not_mirror_exposure() {
    let state = state();
    connected_ssh(&state);
    state.set_active_selection_opt(None);
    let (mirror, _) = pair(&state, DevicePermission::Control);
    let machine = state
        .auth_manager
        .create_scoped_pairing_code(
            DevicePermission::Control,
            crate::remote::auth::DeviceAccessScope::Machine,
        )
        .expect("machine-scope pairing code");
    let (machine, _) = state
        .auth_manager
        .exchange_pairing_code(&machine, "Machine fixture")
        .unwrap();
    let server = SecurityServer::start(state.clone()).await;
    let (status, body) = server
        .request("GET", "/api/v1/sessions", Some(mirror.as_str()), None)
        .await;
    assert_eq!(status, 200, "mirror list refused: {body}");
    assert_eq!(serde_json::from_str::<Vec<serde_json::Value>>(&body).unwrap().len(), 0);
    let (status, body) = server
        .request("GET", "/api/v1/sessions", Some(machine.as_str()), None)
        .await;
    assert_eq!(status, 503, "service-less fixture must not fall back to mirror inventory");
    let refusal: crate::remote::machine_protocol::ErrorEnvelope = serde_json::from_str(&body).unwrap();
    assert_eq!(refusal.error.code, "MACHINE_SERVICE_UNAVAILABLE");
    let epoch = state.daemon_epoch.load(std::sync::atomic::Ordering::Acquire);
    for (label, token, expected) in [
        ("mirror", &mirror, 403),
        ("machine", &machine, 503),
    ] {
        let mut request = format!(
            "ws://{}/api/v1/terminal/ssh-session?daemonEpoch={epoch}",
            server.addr
        )
        .into_client_request()
        .unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {token}").parse().unwrap(),
        );
        let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
            .await
            .expect("bounded scope-routing handshake");
        let (status, body) = match result {
            Ok((socket, response)) => {
                drop(socket);
                (response.status().as_u16(), String::new())
            }
            Err(Error::Http(response)) => {
                let status = response.status().as_u16();
                let body = response
                    .body()
                    .as_ref()
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                    .unwrap_or_default();
                (status, body)
            }
            Err(error) => panic!("{label}: unexpected handshake failure: {error}"),
        };
        assert_eq!(status, expected, "{label}: routed to the wrong endpoint: {body}");
        if label == "machine" {
            assert!(
                body.contains("MACHINE_SERVICE_UNAVAILABLE"),
                "machine scope must take the machine branch: {body}"
            );
            assert!(
                !body.contains("not the active desktop session"),
                "machine scope must not be judged by the mirror exposure gate: {body}"
            );
        }
    }
    server.stop().await;
}

/// For an SSH session, the mirror lists the row precisely when its upgrade is exposed.
#[tokio::test]
async fn session_list_visibility_does_not_widen_terminal_upgrade_authority() {
    let state = state();
    connected_ssh(&state);
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    for (shape, selection, expected) in [
        ("none", None, 403),
        ("no-session", Some(None), 403),
        ("matching", Some(Some("ssh-session")), 101),
        ("other", Some(Some("other-session")), 403),
    ] {
        state.set_active_selection_opt(selection.map(|session_id| {
            RemoteActiveDesktopSelection {
                session_id: session_id.map(str::to_string),
                ..Default::default()
            }
        }));
        let (status, body) = server
            .request("GET", "/api/v1/sessions", Some(control.as_str()), None)
            .await;
        assert_eq!(status, 200, "{shape}: list refused: {body}");
        let rows: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(rows.iter().any(|row| row["sessionId"] == "ssh-session"), expected == 101, "{shape}");
        let mut request = format!("ws://{}/api/v1/terminal/ssh-session", server.addr)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {control}").parse().unwrap(),
        );
        let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
            .await
            .expect("bounded exposure handshake");
        let upgrade = match result {
            Ok((socket, response)) => {
                drop(socket);
                response.status().as_u16()
            }
            Err(Error::Http(response)) => response.status().as_u16(),
            Err(error) => panic!("unexpected handshake failure: {error}"),
        };
        assert_eq!(upgrade, expected, "{shape}: listing changed upgrade authority");
    }
    server.stop().await;
}
