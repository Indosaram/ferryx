use super::*;
use crate::daemon::session_service::DaemonSessionService;
use crate::remote::server::machine_input_fixture::Fixture;
use futures_util::FutureExt;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error};

enum Holder {
    Live,
    Revoked,
    Unknown,
}

async fn conflict_holder_wire(holder: Holder) {
    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let display_name = "Continuity holder laptop";
        let code = fixture.state.auth_manager.create_scoped_pairing_code(
            DevicePermission::Control, crate::remote::DeviceAccessScope::Machine,
        ).unwrap();
        let (_, device) = fixture.state.auth_manager
            .exchange_pairing_code(&code, display_name).unwrap();
        assert_eq!(fixture.state.auth_manager.live_device(&device.id).unwrap().name, display_name);
        let holder_id = match holder {
            Holder::Live | Holder::Revoked => device.id.clone(),
            Holder::Unknown => uuid::Uuid::new_v4().to_string(),
        };
        // Keep the reservation alive without a socket: revoking a real holder
        // socket would race teardown and make this fallback test nondeterministic.
        let services = fixture.state.machine_services.as_ref().unwrap();
        let lease = DaemonSessionService::acquire_machine_controller(
            &mut *services.sessions.machine_controllers.lock().await, id, &holder_id,
        ).unwrap();
        let expected_name = match holder {
            Holder::Live => display_name,
            Holder::Revoked => {
                fixture.state.auth_manager.revoke_device(&device.id).unwrap();
                assert!(fixture.state.auth_manager.live_device(&device.id).is_none());
                "another device"
            }
            Holder::Unknown => {
                assert!(fixture.state.auth_manager.live_device(&holder_id).is_none());
                "another device"
            }
        };
        let mut request = format!("{}/api/v1/terminal/{id}?daemonEpoch={}",
            fixture.base.replace("http:", "ws:"), session["target"]["daemonEpoch"].as_str().unwrap(),
        ).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {}", fixture.token).parse().unwrap());
        let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await.unwrap();
        let Err(Error::Http(response)) = result else { panic!("foreign controller must refuse upgrade") };
        assert_eq!(response.status().as_u16(), 409);
        let expected = format!("CONTROL_CONFLICT\n{expected_name}");
        assert_eq!(response.body().as_deref(), Some(expected.as_bytes()));
        assert_eq!(services.sessions.machine_controllers.lock().await[id].device, holder_id);
        assert!(!*lease.cancelled.borrow(), "refusal must not displace the holder");
        if fixture.state.auth_manager.live_device(&device.id).is_some() {
            fixture.state.auth_manager.revoke_device(&device.id).unwrap();
        }
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn control_conflict_wire_uses_live_holder_device_name() {
    conflict_holder_wire(Holder::Live).await;
}

#[tokio::test]
async fn control_conflict_wire_hides_revoked_holder_name() {
    conflict_holder_wire(Holder::Revoked).await;
}

#[tokio::test]
async fn control_conflict_wire_uses_fallback_for_unknown_holder() {
    conflict_holder_wire(Holder::Unknown).await;
}

#[tokio::test]
async fn paired_scoped_and_grid_upgrades_require_exact_desktop_exposure() {
    use crate::scoped_contracts::Epoch;
    use crate::terminal::paired_daemon::{Descriptor, Proxy};
    let state = state();
    let proxy = Proxy::new(Descriptor {
        host_id: "https://paired-scoped-exposure.invalid".into(),
        generation: Epoch(1),
        target: crate::remote::machine_protocol::RemoteTerminalTarget {
            machine_id: "paired-scoped-exposure".into(),
            daemon_epoch: Epoch(1),
            session_id: uuid::Uuid::new_v4().to_string(),
        },
        after_sequence: None,
    }, state.terminal_service.output_hub().clone()).unwrap();
    let id = state.terminal_service.paired().install(proxy).unwrap();
    let (view, _) = pair(&state, DevicePermission::View);
    let (control, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state.clone()).await;
    let outcome = std::panic::AssertUnwindSafe(async {
        for (shape, selection, expected) in [
            ("none", None, 403),
            ("no-session", Some(None), 403),
            ("matching", Some(Some(id.as_str())), 101),
            ("other", Some(Some("other-session")), 403),
        ] {
            state.set_active_selection_opt(selection.map(|session_id| RemoteActiveDesktopSelection {
                session_id: session_id.map(str::to_string), ..Default::default()
            }));
            for (permission, token) in [("view", &view), ("control", &control)] {
                for (target, render) in [
                    (id.clone(), "?render=grid"),
                    (format!("paired-scoped-exposure::{id}"), ""),
                    (format!("paired-scoped-exposure::{id}"), "?render=grid"),
                ] {
                    let mut request = format!("ws://{}/api/v1/terminal/{target}{render}", server.addr)
                        .into_client_request().unwrap();
                    request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
                    let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request))
                        .await.expect("bounded paired exposure handshake");
                    let status = match result {
                        Ok((socket, response)) => { drop(socket); response.status().as_u16() }
                        Err(Error::Http(response)) => response.status().as_u16(),
                        Err(error) => panic!("unexpected handshake failure: {error}"),
                    };
                    assert_eq!(status, expected, "{shape}/{permission}/{target}{render}");
                }
            }
        }
    }).catch_unwind().await;
    state.terminal_service.paired().detach(&id).await.unwrap();
    server.stop().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}
