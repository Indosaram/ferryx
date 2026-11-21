use super::*;
use crate::remote::backend::{RemoteSessionBackend, RemoteSessionDetails};
use crate::terminal::{SessionAttachment, TerminalOutputHub, TerminalSignal};
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[tokio::test]
async fn unattached_paired_machine_upgrade_refuses_before_takeover() {
    use crate::remote::server::machine_input_fixture::Fixture;
    use crate::scoped_contracts::Epoch;
    use crate::terminal::paired_daemon::{Descriptor, Proxy};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error};

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let source = fixture.create().await;
        let terminal = fixture.owner.terminal_service();
        let proxy = Proxy::new(Descriptor {
            host_id: "https://unattached.invalid".into(), generation: Epoch(1),
            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                machine_id: "recovering".into(), daemon_epoch: Epoch(1),
                session_id: uuid::Uuid::new_v4().to_string(),
            }, after_sequence: None,
        }, terminal.output_hub().clone()).unwrap();
        let id = terminal.paired().install(proxy).unwrap();
        let sessions = &fixture.state.machine_services.as_ref().unwrap().sessions;
        sessions.retain_paired_metadata_for_test(source["target"]["sessionId"].as_str().unwrap(), &id);
        sessions.record_desktop_geometry(&id, 80, 24);
        let lease = crate::daemon::session_service::DaemonSessionService::acquire_machine_controller(
            &mut *sessions.machine_controllers.lock().await, &id, "foreign-holder",
        ).unwrap();
        let mut request = format!("{}/api/v1/terminal/{id}?daemonEpoch={}", fixture.base.replace("http:", "ws:"), source["target"]["daemonEpoch"].as_str().unwrap())
            .into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {}", fixture.token).parse().unwrap());
        let result = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await.unwrap();
        let Err(Error::Http(response)) = result else { panic!("output admission must not displace a foreign holder without takeover") };
        assert_eq!(response.status().as_u16(), 409);
        assert!(response.body().as_ref().unwrap().starts_with(b"CONTROL_CONFLICT"));
        assert_eq!(sessions.machine_controllers.lock().await[&id].device, "foreign-holder");
        assert!(!*lease.cancelled.borrow());

        let mut request = format!("{}/api/v1/terminal/{id}?daemonEpoch={}&takeover=1", fixture.base.replace("http:", "ws:"), source["target"]["daemonEpoch"].as_str().unwrap())
            .into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {}", fixture.token).parse().unwrap());
        let (mut socket, _) = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await.unwrap().unwrap();
        assert!(*lease.cancelled.borrow());
        tokio::time::timeout(DEADLINE, async {
            let mut attached = false;
            loop {
                let frame = socket.next().await.expect("recovery socket open").unwrap();
                if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == "attached" { attached = true; }
                    if value["type"] == "terminalOutcome" {
                        assert!(attached);
                        assert_eq!(value["state"], "owner_unavailable");
                        assert!(value["generation"].is_null());
                        break;
                    }
                }
            }
        }).await.unwrap();
        let observation = crate::remote::server::machine_input_probe::Observation::register(&id);
        socket.send(tokio_tungstenite::tungstenite::Message::Binary(b"forbidden-input".to_vec().into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.expect("input rejection delivered").unwrap();
                if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if value["type"] == "error" {
                        assert_eq!(value["code"], "SESSION_INPUT_UNAVAILABLE");
                        break;
                    }
                }
            }
        }).await.unwrap();
        assert_eq!(observation.0.borrow().started, 0);
        socket.close(None).await.unwrap();
        terminal.paired().detach(&id).await.unwrap();
        sessions.prune_paired_metadata_for_test();
        let target = crate::remote::machine_protocol::RemoteTerminalTarget {
            machine_id: source["target"]["machineId"].as_str().unwrap().into(),
            daemon_epoch: source["target"]["daemonEpoch"].as_str().unwrap().parse::<u64>().map(Epoch).unwrap(),
            session_id: id.clone(),
        };
        assert_eq!(sessions.validate_machine_target(&target).await.unwrap_err(), "SESSION_EXPIRED");
        assert!(sessions.desktop_geometry(&id).is_none());
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn retained_paired_blocked_and_exited_output_survives_pruning_without_input_authority() {
    use crate::remote::server::machine_input_fixture::Fixture;
    use crate::scoped_contracts::Epoch;
    use crate::terminal::paired_daemon::{Descriptor, Proxy};
    use futures_util::{FutureExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let source = fixture.create().await;
        let terminal = fixture.owner.terminal_service();
        let sessions = &fixture.state.machine_services.as_ref().unwrap().sessions;
        for (failure, expected) in [("PERMISSION_DENIED", "blocked"), ("PAIRED_SESSION_EXITED", "affirmative_exit")] {
            let mut proxy = Proxy::new(Descriptor {
                host_id: "https://retained.invalid".into(), generation: Epoch(1),
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id: "retained".into(), daemon_epoch: Epoch(1),
                    session_id: uuid::Uuid::new_v4().to_string(),
                }, after_sequence: None,
            }, terminal.output_hub().clone()).unwrap();
            proxy.recovery_failed(&crate::paired_host::client::ClientError::local(failure));
            let id = terminal.paired().install(proxy).unwrap();
            sessions.retain_paired_metadata_for_test(source["target"]["sessionId"].as_str().unwrap(), &id);
            sessions.record_desktop_geometry(&id, 80, 24);
            let mut paired = source.clone();
            paired["target"]["sessionId"] = id.clone().into();
            let mut socket = fixture.attach(&paired).await;
            tokio::time::timeout(DEADLINE, async {
                loop {
                    let frame = socket.next().await.expect("retained outcome delivered").unwrap();
                    if let Message::Text(text) = frame {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        if value["type"] == "terminalOutcome" {
                            assert_eq!(value["state"], expected);
                            assert!(value["generation"].is_null());
                            break;
                        }
                    }
                }
            }).await.unwrap();
            assert_eq!(sessions.write_machine_input(&id, 0, b"forbidden").await.unwrap_err(), "SESSION_INPUT_UNAVAILABLE");
            assert_eq!(sessions.resize_machine_input(&id, 0, 100, 30).await.unwrap_err(), "SESSION_INPUT_UNAVAILABLE");
            assert_eq!(sessions.interrupt_machine_input(&id, 0).await.unwrap_err(), "SESSION_INPUT_UNAVAILABLE");
            terminal.output_hub().remove_session(&id);
            tokio::time::timeout(DEADLINE, async {
                let mut final_outcome = None;
                while let Some(frame) = socket.next().await {
                    let Ok(frame) = frame else { break; };
                    if let Message::Text(text) = frame {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        if value["type"] == "terminalOutcome" {
                            final_outcome = Some(value);
                        }
                    }
                }
                assert_eq!(final_outcome.expect("outcome must precede output close")["state"], expected);
            }).await.unwrap();
            terminal.paired().detach(&id).await.unwrap();
            sessions.prune_paired_metadata_for_test();
            let target = serde_json::from_value(paired["target"].clone()).unwrap();
            assert_eq!(sessions.validate_machine_target(&target).await.unwrap_err(), "SESSION_EXPIRED");
            assert!(sessions.desktop_geometry(&id).is_none());
        }
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn machine_socket_transport_generation_change_fences_input_until_new_admission() {
    use crate::remote::server::{machine_input_fixture::Fixture, machine_input_probe::Observation};
    use crate::terminal::remote::{RemoteConnectionState, RemoteSessionDetails};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let mut socket = fixture.attach(&session).await;
        let observation = Observation::register(id);
        let terminal = fixture.owner.terminal_service();
        // Publish a different transport generation after the real HTTP upgrade.
        // The local PTY remains alive, so expiry cannot explain the refusal.
        let descriptor = serde_json::from_value(serde_json::json!({
            "backendSessionId":id,
            "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"remote-original"},
            "config":{
                "host":{"id":"host","label":"host","hostname":"example.invalid","source":"manual","authMethod":"agent"},
                "environment":{"platform":"posix","executor":"sh","version":"test","home":"/home/test","temp":"/tmp","git":true},
                "helper":{"executable":"/helper","root":"/root"},
                "projectId":"project","projectPath":"/project","worktree":null,"agentIdentity":null
            }, "clientRequestId":"request-original","remoteCursor":"0","cols":80,"rows":24
        })).unwrap();
        terminal.remote().record_settled(id, RemoteSessionDetails {
            descriptor, state: RemoteConnectionState::Connected, generation: 7,
            attempts: 0, failure: None, replay_gap: None, pid: None,
        });
        socket.send(Message::Binary(b"must-not-cross-generation".to_vec().into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.unwrap().unwrap();
                if let Message::Text(text) = frame {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if value["type"] == "error" {
                            assert_eq!(value["code"], "STALE_GENERATION");
                            break;
                        }
                    }
                }
            }
        }).await.unwrap();
        assert_eq!(observation.0.borrow().started, 0, "fenced before polling backend input");
        // A fresh admission captures generation 7; the old socket never rebases.
        let mut fresh = fixture.attach(&session).await;
        fresh.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                match fresh.next().await.expect("fresh admission closed before pong").unwrap() {
                    Message::Text(text) => {
                        let control: serde_json::Value = serde_json::from_str(&text).unwrap();
                        assert_ne!(control["type"], "error", "{control}");
                        if control["type"] == "pong" { break; }
                    }
                    Message::Close(_) => panic!("fresh admission closed before pong"),
                    _ => {} // PTY replay may precede the control reply.
                }
            }
        }).await.unwrap();
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn ssh_machine_exit_status_does_not_infer_exit_from_connection_state() {
    use crate::remote::server::machine_input_fixture::Fixture;
    use crate::terminal::remote::{RemoteConnectionState, RemoteFailure, RemoteFailureKind, RemoteSessionDetails};
    use crate::terminal::PtySessionState;
    use futures_util::FutureExt;

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let terminal = fixture.owner.terminal_service();
        let sessions = &fixture.state.machine_services.as_ref().unwrap().sessions;
        let id = "ssh-exit-observation";
        let descriptor = serde_json::from_value(serde_json::json!({
            "backendSessionId":id,
            "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"remote-original"},
            "config":{
                "host":{"id":"host","label":"host","hostname":"example.invalid","source":"manual","authMethod":"agent"},
                "environment":{"platform":"posix","executor":"sh","version":"test","home":"/home/test","temp":"/tmp","git":true},
                "helper":{"executable":"/helper","root":"/root"},
                "projectId":"project","projectPath":"/project","worktree":null,"agentIdentity":null
            },
            "clientRequestId":"request-original","remoteCursor":"0","cols":80,"rows":24
        })).unwrap();
        let mut details = RemoteSessionDetails {
            descriptor,
            state: RemoteConnectionState::Connected,
            generation: 7,
            attempts: 0,
            failure: None,
            replay_gap: None,
            pid: None,
        };
        for state in [RemoteConnectionState::Connected, RemoteConnectionState::Reconnecting,
            RemoteConnectionState::Disconnected, RemoteConnectionState::Expired]
        {
            details.state = state;
            details.failure = match state {
                RemoteConnectionState::Disconnected => Some(RemoteFailure {
                    kind: RemoteFailureKind::Authentication,
                    message: "Authentication refused while the remote process may remain alive".into(),
                }),
                RemoteConnectionState::Expired => Some(RemoteFailure {
                    kind: RemoteFailureKind::Expired,
                    message: "Helper no longer owns the target".into(),
                }),
                _ => None,
            };
            terminal.remote().record_settled(id, details.clone());
            assert!(terminal.remote().details(id).is_none());
            match state {
                RemoteConnectionState::Connected | RemoteConnectionState::Reconnecting => {
                    assert!(matches!(sessions.machine_exit_status(id), Some(PtySessionState::Running)));
                }
                RemoteConnectionState::Disconnected | RemoteConnectionState::Expired => {
                    assert!(sessions.machine_exit_status(id).is_none(), "{state:?} is not affirmative process exit");
                }
            }
        }
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[tokio::test]
async fn paired_machine_exit_status_requires_affirmative_exit() {
    use crate::remote::server::machine_input_fixture::Fixture;
    use crate::scoped_contracts::Epoch;
    use crate::terminal::{paired_daemon::{Descriptor, Proxy}, PtySessionState};
    use futures_util::FutureExt;

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let terminal = fixture.owner.terminal_service();
        let sessions = &fixture.state.machine_services.as_ref().unwrap().sessions;
        assert!(sessions.machine_exit_status("missing").is_none());
        for failure in [None, Some("SESSION_EXPIRED"), Some("PAIRED_SESSION_EXITED")] {
            let descriptor = Descriptor {
                host_id: "https://paired-exit.invalid".into(),
                generation: Epoch(1),
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id: "paired-exit-machine".into(),
                    daemon_epoch: Epoch(1),
                    session_id: uuid::Uuid::new_v4().to_string(),
                },
                after_sequence: None,
            };
            let mut proxy = Proxy::new(descriptor, terminal.output_hub().clone()).unwrap();
            if let Some(code) = failure {
                proxy.recovery_failed(&crate::paired_host::client::ClientError::local(code));
            }
            let id = terminal.paired().install(proxy).unwrap();
            assert!(sessions.owns_machine_session(&id));
            assert!(sessions.machine_pty(&id).is_none());
            assert!(terminal.remote().settled_details(&id).is_none());
            match failure {
                Some("PAIRED_SESSION_EXITED") => assert!(matches!(
                    sessions.machine_exit_status(&id), Some(PtySessionState::Exited { code: None })
                )),
                _ => assert!(
                    sessions.machine_exit_status(&id).is_none(),
                    "{failure:?} is not affirmative process exit"
                ),
            }
            terminal.paired().detach(&id).await.unwrap();
            assert!(sessions.machine_exit_status(&id).is_none());
        }
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[test]
fn a11_takeover_displaces_a_connected_foreign_holder_exactly_once() {
    use crate::daemon::session_service::DaemonSessionService;
    let mut controllers = std::collections::HashMap::new();
    let holder =
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "a").unwrap();
    let old_cancelled = holder.cancelled.clone();
    let geometry_fence = controllers["one"].geometry_fence.clone();
    let resized = controllers["one"].resized.clone();
    // A resize debt belongs to the session's geometry, so the lease that records it and the
    // controller that outlives it are the same flag.
    assert!(Arc::ptr_eq(&resized, &holder.resized));
    holder.resized.store(true, std::sync::atomic::Ordering::Release);

    // A connected holder is reserved indefinitely, so ordinary admission refuses.
    assert!(
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "b").is_err()
    );
    assert!(!*old_cancelled.borrow());

    // Takeover admits and cancels the displaced socket.
    let taken = DaemonSessionService::acquire_machine_controller_with(
        &mut controllers,
        "one",
        "b",
        true,
    )
    .unwrap();
    assert!(*old_cancelled.borrow());
    assert_eq!(taken.generation, holder.generation + 1);
    assert_eq!(controllers["one"].device, "b");
    assert!(Arc::ptr_eq(&geometry_fence, &controllers["one"].geometry_fence));
    // Takeover must not forget that the pane sits off the desktop baseline. The displaced
    // holder cannot restore it (it is no longer current), so the successor inherits the debt.
    assert!(Arc::ptr_eq(&resized, &controllers["one"].resized));
    assert!(Arc::ptr_eq(&resized, &taken.resized));
    assert!(
        taken.resized.load(std::sync::atomic::Ordering::Acquire),
        "takeover dropped the pending baseline restore"
    );

    // Takeover is one attempt, never a stored preference: a fresh foreign device is
    // still refused while B is connected, and so is A coming back without the flag.
    assert!(
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "c").is_err()
    );
    assert!(
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "a").is_err()
    );

    // Same device reacquiring is ordinary admission and still bumps the generation.
    let again =
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "b").unwrap();
    assert_eq!(again.generation, taken.generation + 1);
    assert!(Arc::ptr_eq(&geometry_fence, &controllers["one"].geometry_fence));
    assert!(Arc::ptr_eq(&resized, &again.resized));
}

#[cfg(unix)]
#[tokio::test]
async fn a11c_teardown_fence_outlives_a_replacement_that_resizes_first() {
    use crate::remote::server::{machine_input_fixture::Fixture, machine_teardown_probe};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

    async fn resize(socket: &mut crate::remote::server::machine_input_fixture::Socket, generation: u64, cols: u16, rows: u16) {
        socket.send(Message::Text(serde_json::json!({
            "type":"resize", "generation":generation.to_string(), "cols":cols, "rows":rows,
        }).to_string().into())).await.unwrap();
        socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                match socket.next().await.unwrap().unwrap() {
                    Message::Text(text) => {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        assert_ne!(value["type"], "error", "{value}");
                        if value["type"] == "pong" { break; }
                    }
                    Message::Close(_) => panic!("socket closed before resize barrier"),
                    _ => {},
                }
            }
        }).await.expect("resize must complete before the ordered pong");
    }

    let fixture = Fixture::new().await;
    let outcome = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let mut first = fixture.attach(&session).await;
        let services = fixture.state.machine_services.as_ref().unwrap();
        services.sessions.record_desktop_geometry(id, 80, 24);
        let generation = services.sessions.machine_controllers.lock().await[id].generation;
        let pty = fixture.owner.terminal_service().get_session(id).unwrap();
        resize(&mut first, generation, 90, 30).await;
        assert_eq!(pty.get_size(), (90, 30));

        let (entered, release, finished) = machine_teardown_probe::register(id);
        let pin = fixture.state.auth_manager.create_scoped_pairing_code(
            DevicePermission::Control, crate::remote::DeviceAccessScope::Machine,
        ).unwrap();
        let (token, other) = fixture.state.auth_manager.exchange_pairing_code(&pin, "takeover").unwrap();
        let mut request = format!("{}/api/v1/terminal/{id}?daemonEpoch={}&takeover=1",
            fixture.base.replace("http:", "ws:"), session["target"]["daemonEpoch"].as_str().unwrap(),
        ).into_client_request().unwrap();
        request.headers_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());
        let (mut second, _) = tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await.unwrap().unwrap();
        let attached = tokio::time::timeout(DEADLINE, second.next()).await.unwrap().unwrap().unwrap();
        let Message::Text(attached) = attached else { panic!("attached boundary") };
        let attached: serde_json::Value = serde_json::from_str(&attached).unwrap();
        let successor: u64 = attached["generation"].as_str().unwrap().parse().unwrap();
        assert_eq!(successor, generation + 1);
        tokio::time::timeout(DEADLINE, async {
            loop {
                match first.next().await.unwrap().unwrap() {
                    Message::Text(text) => {
                        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                        if value["type"] != "terminalOutcome" { continue; }
                        assert_eq!(value["state"], "blocked");
                        assert_eq!(value["blockedReason"], "SESSION_OWNERSHIP_CHANGED");
                        assert_eq!(value["target"]["sessionId"], id);
                        assert_eq!(value["target"]["backendSessionId"], id);
                        assert_eq!(value["target"]["daemonEpoch"], session["target"]["daemonEpoch"]);
                        assert_eq!(value["generation"], generation.to_string());
                        assert!(!value["incarnation"].as_str().unwrap().is_empty());
                        break;
                    }
                    Message::Close(_) => panic!("displaced socket closed without outcome"),
                    _ => {},
                }
            }
        }).await.unwrap();
        tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
        resize(&mut second, successor, 110, 40).await;
        assert_eq!(pty.get_size(), (110, 40));
        release.send(()).unwrap();
        tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
        assert_eq!(pty.get_size(), (110, 40), "displaced socket restored over successor geometry");
        assert_eq!(services.sessions.machine_controllers.lock().await[id].device, other.id);
        let client = reqwest::Client::builder().no_proxy().timeout(DEADLINE).build().unwrap();
        let close = || serde_json::json!({"requestId":uuid::Uuid::new_v4(),"daemonEpoch":session["target"]["daemonEpoch"]}).to_string();
        let url = format!("{}/api/v1/sessions/{id}", fixture.base);
        assert_eq!(client.delete(&url).bearer_auth(&fixture.token).body(close()).send().await.unwrap().status(), 409);
        assert_eq!(client.delete(&url).bearer_auth(&token).body(close()).send().await.unwrap().status(), 204);
        assert!(pty.is_reaped());
        fixture.state.auth_manager.revoke_device(&other.id).unwrap();
        drop(first);
        drop(second);
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = outcome { std::panic::resume_unwind(panic); }
}

#[cfg(unix)]
#[tokio::test]
async fn blocked_geometry_restore_does_not_block_another_sessions_input() {
    use crate::remote::server::{machine_input_fixture::Fixture, machine_teardown_probe};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    async fn resize(socket: &mut crate::remote::server::machine_input_fixture::Socket, generation: u64) {
        socket.send(Message::Text(serde_json::json!({
            "type":"resize", "generation":generation.to_string(), "cols":100, "rows":35,
        }).to_string().into())).await.unwrap();
        socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let Message::Text(text) = socket.next().await.unwrap().unwrap() else { continue; };
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "{value}");
                if value["type"] == "pong" { break; }
            }
        }).await.unwrap();
    }

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let first = fixture.create().await;
        let second = fixture.create().await;
        let first_id = first["target"]["sessionId"].as_str().unwrap();
        let second_id = second["target"]["sessionId"].as_str().unwrap();
        let services = fixture.state.machine_services.as_ref().unwrap();
        let mut first_socket = fixture.attach(&first).await;
        let first_generation = services.sessions.machine_controllers.lock().await[first_id].generation;
        services.sessions.record_desktop_geometry(first_id, 80, 24);
        resize(&mut first_socket, first_generation).await;
        let (entered, release, finished) = machine_teardown_probe::register(&format!("restore:{first_id}"));
        first_socket.send(Message::Close(None)).await.unwrap();
        tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
        let mut second_socket = fixture.attach(&second).await;
        let second_generation = services.sessions.machine_controllers.lock().await[second_id].generation;
        resize(&mut second_socket, second_generation).await;
        assert_eq!(fixture.owner.terminal_service().get_session(second_id).unwrap().get_size(), (100, 35));
        release.send(()).unwrap();
        tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
        assert_eq!(fixture.owner.terminal_service().get_session(first_id).unwrap().get_size(), (80, 24));
        drop(first_socket);
        drop(second_socket);
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_records_the_desktop_baseline_so_a_phone_resize_restores_without_a_seed() {
    use crate::remote::server::{machine_input_fixture::Fixture, machine_teardown_probe};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let session = fixture.create().await;
        let id = session["target"]["sessionId"].as_str().unwrap();
        let services = &fixture.state.machine_services.as_ref().unwrap().sessions;
        let pty = fixture.owner.terminal_service().get_session(id).unwrap();
        let baseline = pty.get_size();
        assert_eq!(
            services.desktop_geometry(id),
            Some(baseline),
            "spawn dimensions must be the desktop baseline with no manual seed"
        );

        let mut socket = fixture.attach(&session).await;
        let generation = services.machine_controllers.lock().await[id].generation;
        socket.send(Message::Text(serde_json::json!({
            "type": "resize", "generation": generation.to_string(), "cols": 52, "rows": 17,
        }).to_string().into())).await.unwrap();
        socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.expect("resize barrier").unwrap();
                let Message::Text(text) = frame else { continue; };
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "{value}");
                if value["type"] == "pong" { break; }
            }
        }).await.unwrap();
        assert_eq!(pty.get_size(), (52, 17));

        let (entered, release, finished) = machine_teardown_probe::register(id);
        socket.send(Message::Close(None)).await.unwrap();
        tokio::time::timeout(DEADLINE, entered).await.unwrap().unwrap();
        release.send(()).unwrap();
        tokio::time::timeout(DEADLINE, finished).await.unwrap().unwrap();
        assert_eq!(
            pty.get_size(),
            baseline,
            "teardown must restore the spawn geometry the desktop never resized away from"
        );
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}

#[test]
fn a11d_control_conflict_names_the_holder_only_when_refusing() {
    use crate::daemon::session_service::DaemonSessionService;
    let mut controllers = std::collections::HashMap::new();
    DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "holder-device")
        .unwrap();

    // Refused: the caller learns who holds it, which is what the banner shows.
    assert_eq!(
        DaemonSessionService::machine_controller_holder(
            &controllers,
            "one",
            "other-device",
            false
        ),
        Some("holder-device".to_string())
    );
    // Takeover is authorized, so there is no conflict to explain.
    assert_eq!(
        DaemonSessionService::machine_controller_holder(&controllers, "one", "other-device", true),
        None
    );
    // Same device is never its own conflicting holder.
    assert_eq!(
        DaemonSessionService::machine_controller_holder(
            &controllers,
            "one",
            "holder-device",
            false
        ),
        None
    );
    // An unknown session has no holder.
    assert_eq!(
        DaemonSessionService::machine_controller_holder(&controllers, "two", "a", false),
        None
    );
}

#[test]
fn a10_controller_generation_disconnect_and_exact_reservation_boundary() {
    use crate::daemon::session_service::DaemonSessionService;
    let mut controllers = std::collections::HashMap::new();
    let original =
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "a").unwrap();
    let old_cancelled = original.cancelled.clone();
    let replacement =
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "a").unwrap();
    assert!(*old_cancelled.borrow());
    assert_eq!(replacement.generation, original.generation + 1);
    drop(original);
    assert!(controllers["one"].disconnected.lock().is_none());
    assert!(
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "b").is_err()
    );
    drop(replacement);
    assert!(controllers["one"].disconnected.lock().is_some());
    let disconnected = controllers["one"].disconnected.lock().unwrap();
    assert!(controllers["one"].reserved_at(disconnected + Duration::from_millis(14999)));
    assert!(!controllers["one"].reserved_at(disconnected + Duration::from_secs(15)));
    // Controlled clock values exercise the production reservation predicate;
    // no wall-clock wait or probabilistic negative assertion.
    *controllers["one"].disconnected.lock() =
        Some(tokio::time::Instant::now() - Duration::from_secs(16));
    let other =
        DaemonSessionService::acquire_machine_controller(&mut controllers, "one", "b").unwrap();
    assert_eq!(controllers["one"].device, "b");
    drop(other);
    eprintln!("A10 sole controller: old drop cannot release replacement; disconnect records reservation; controlled expiry permits other device");
}

#[tokio::test]
async fn a10_real_socket_reservation_transfers_http_close_authority() {
    use futures_util::FutureExt;
    let (root, owner, request) = tokio::task::spawn_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("project")).unwrap();
        let owner = crate::daemon::server::DaemonServer::new_with_paths(Some(root.path().join("config")), Some(root.path().join("auth")));
        let services = owner.remote_state().machine_services.as_ref().unwrap();
        let workspace = services.workspaces.register_machine(root.path().join("project").to_str().unwrap()).unwrap();
        let request = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"workspaceId":workspace,"worktree":null,"inheritFromSessionId":null,"cwdRelative":null,"cols":80,"rows":24,"startup":{"kind":"shell"}});
        (root, owner, request)
    }).await.unwrap();
    let state = owner.remote_state().clone();
    let services = state.machine_services.as_ref().unwrap().clone();
    let backend = owner.terminal_service().clone();
    let machine_pair = |name: &str| {
        let pin = state
            .auth_manager
            .create_scoped_pairing_code(
                DevicePermission::Control,
                crate::remote::DeviceAccessScope::Machine,
            )
            .unwrap();
        state
            .auth_manager
            .exchange_pairing_code(&pin, name)
            .unwrap()
    };
    let (token, device) = machine_pair("creator");
    let (other, other_device) = machine_pair("other");
    let server = SecurityServer::start(state.clone()).await;
    let result = std::panic::AssertUnwindSafe(async {
        let (status, body) = server.request("POST", "/api/v1/sessions", Some(&token), Some(&request.to_string())).await;
        assert_eq!(status, 201, "{body}");
        let created: serde_json::Value = serde_json::from_str(&body).unwrap();
        let id = created["target"]["sessionId"].as_str().unwrap();
        let epoch = created["target"]["daemonEpoch"].as_str().unwrap();
        let pty = backend.get_session(id).unwrap();
        let pid = pty.pid();
        let path = format!("/api/v1/terminal/{id}?daemonEpoch={epoch}");
        let mut first = open_ws_stream(server.addr, &path, Some(&token)).await;
        assert!(matches!(frame(&mut first).await, ServerWebSocketFrame::Text(_)));
        write_client_ws_frame(&mut first, 8, &[]).await;
        while !matches!(frame(&mut first).await, ServerWebSocketFrame::Close) {}
        drop(first);
        assert!(services.sessions.machine_controllers.lock().await[id].disconnected.lock().is_some());
        assert_eq!(ws_handshake_status(server.addr, &path, Some(&other)).await, 409);
        *services.sessions.machine_controllers.lock().await[id].disconnected.lock() = Some(tokio::time::Instant::now() - Duration::from_secs(15));
        let mut second = open_ws_stream(server.addr, &path, Some(&other)).await;
        assert!(matches!(frame(&mut second).await, ServerWebSocketFrame::Text(_)));
        assert_eq!(pty.pid(), pid);
        let close = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string();
        assert_eq!(server.request("DELETE", &format!("/api/v1/sessions/{id}"), Some(&token), Some(&close)).await.0, 409);
        let close = serde_json::json!({"requestId":uuid::Uuid::new_v4().to_string(),"daemonEpoch":epoch}).to_string();
        assert_eq!(server.request("DELETE", &format!("/api/v1/sessions/{id}"), Some(&other), Some(&close)).await.0, 204);
        assert!(pty.is_reaped());
        while !matches!(frame(&mut second).await, ServerWebSocketFrame::Close) {}
        drop(second);
        eprintln!("A10 actual socket release: reserved other=409; controlled15s expiry=attach; creator close=409; sole current controller close=204; original_pid={pid:?} epoch={epoch} reaped=true");
    }).catch_unwind().await;
    let _ = state.auth_manager.revoke_device(&device.id);
    let _ = state.auth_manager.revoke_device(&other_device.id);
    for id in backend.list_sessions() {
        backend.close_session(&id).await.unwrap();
        services.sessions.wait_machine_lifecycle(&id).await.unwrap();
    }
    server.stop().await;
    drop(services);
    drop(state);
    drop(owner);
    tokio::task::spawn_blocking(move || root.close().unwrap())
        .await
        .unwrap();
    eprintln!("A10 joint socket-close cleanup: listener stopped, original PTYs/lifecycles reaped, root removed");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a10_pending_machine_input_is_dropped_on_disconnect_and_revoke() {
    // The machine branch writes to the real PtySession, not SocketBackend.
    // Preserve both lifetime assertions at that production boundary.
    crate::remote::server::machine_input_cancellation_tests::disconnect_and_revoke().await;
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
    pending: AtomicUsize,
}

impl Gate {
    async fn wait(&self) {
        struct Pending<'a>(&'a AtomicUsize);
        impl Drop for Pending<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _pending = Pending(&self.pending);
        self.entered.notify_one();
        self.release.notified().await;
    }
}

/// Only the PTY boundary is replaced: real auth, router, TCP/WebSocket framing,
/// output replay/broadcast hub and (feature-on) Ghostty mirror run unchanged.
struct SocketBackend {
    recovery_tx: Option<tokio::sync::watch::Sender<crate::remote::backend::RemoteRecoveryStatus>>,
    hub: TerminalOutputHub,
    attach_gate: Option<Arc<Gate>>,
    input_gate: Arc<Gate>,
    completed_inputs: AtomicUsize,
    acknowledged_outcomes: AtomicUsize,
    resize_calls: Arc<AtomicUsize>,
    restore_calls: Arc<AtomicUsize>,
}

impl SocketBackend {
    fn new(attach_gate: Option<Arc<Gate>>) -> Arc<Self> {
        let hub = TerminalOutputHub::default();
        hub.register_session("session");
        hub.publish("session", b"READY".to_vec()).unwrap();
        Arc::new(Self {
            recovery_tx: None,
            hub,
            attach_gate,
            input_gate: Arc::new(Gate::default()),
            completed_inputs: AtomicUsize::new(0),
            acknowledged_outcomes: AtomicUsize::new(0),
            resize_calls: Arc::new(AtomicUsize::new(0)),
            restore_calls: Arc::new(AtomicUsize::new(0)),
        })
    }
}

impl RemoteSessionBackend for SocketBackend {
    fn acknowledge_agent_outcome<'a>(
        &'a self,
        _: &'a str,
    ) -> BoxFuture<'a, Result<(), crate::remote::backend::OutcomeAckError>> {
        self.acknowledged_outcomes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
    fn recovery<'a>(
        &'a self,
        _: &'a str,
    ) -> BoxFuture<'a, Result<Option<crate::remote::backend::RecoveryStream>, String>> {
        Box::pin(async move {
            Ok(self.recovery_tx.as_ref().map(|tx| {
                Box::pin(futures_util::stream::unfold(
                    (tx.subscribe(), true),
                    |(mut rx, initial)| async move {
                        if !initial && rx.changed().await.is_err() {
                            return None;
                        }
                        let value = rx.borrow_and_update().clone();
                        Some((value, (rx, false)))
                    },
                )) as crate::remote::backend::RecoveryStream
            }))
        })
    }
    fn write_generation<'a>(
        &'a self,
        _: &'a str,
        generation: u64,
        bytes: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let status = self.recovery_tx.as_ref().unwrap().borrow();
            if status.generation == generation
                && status.state == crate::terminal::remote::RemoteConnectionState::Connected
            {
                self.hub.publish("session", bytes.to_vec()).unwrap();
                self.completed_inputs.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    }
    fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
        Box::pin(async { vec!["session".into()] })
    }
    fn describe_session<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>> {
        Box::pin(async move {
            Ok(RemoteSessionDetails {
                session_id: id.into(),
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
                cols: 80,
                rows: 24,
            })
        })
    }
    fn attach_with_sequence<'a>(
        &'a self,
        id: &'a str,
        sequence: Option<u64>,
    ) -> BoxFuture<'a, Result<SessionAttachment, String>> {
        Box::pin(async move {
            let attachment = self
                .hub
                .subscribe_with_sequence(id, sequence)
                .ok_or("unknown session")?;
            if let Some(gate) = &self.attach_gate {
                gate.wait().await;
            }
            Ok(attachment)
        })
    }
    fn write_input<'a>(&'a self, _: &'a str, _: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if self.recovery_tx.is_none() {
                self.input_gate.wait().await;
            }
            self.completed_inputs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
    fn resize<'a>(&'a self, _: &'a str, _: u16, _: u16) -> BoxFuture<'a, Result<(), String>> {
        self.resize_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
    fn restore_desktop_geometry<'a>(
        &'a self,
        _: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        self.restore_calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
    fn signal<'a>(&'a self, _: &'a str, _: TerminalSignal) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

fn socket_state(backend: Arc<SocketBackend>) -> Arc<RemoteGatewayState> {
    let state = Arc::new(RemoteGatewayState::new_with_backend(
        backend,
        crate::worktree::WorkspaceRegistry::new(),
    ));
    state.set_active_selection(RemoteActiveDesktopSelection {
        session_id: Some("session".into()),
        ..Default::default()
    });
    state
}

async fn frame(stream: &mut tokio::net::TcpStream) -> ServerWebSocketFrame {
    let f = tokio::time::timeout(DEADLINE, read_server_ws_frame(stream))
        .await
        .expect("bounded socket frame/close");
    match &f {
        ServerWebSocketFrame::Text(t) => eprintln!("[CLIENT_FRAME] Text: {}", t),
        ServerWebSocketFrame::Binary(b) => eprintln!("[CLIENT_FRAME] Binary len: {}", b.len()),
        ServerWebSocketFrame::Close => eprintln!("[CLIENT_FRAME] Close"),
    }
    f
}

#[tokio::test]
async fn ssh_reconnect_safety_web_raw_gap_order() {
    let backend = SocketBackend::new(None);
    let state = socket_state(backend.clone());
    let (token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state).await;
    let mut socket = open_ws_stream(server.addr, "/api/v1/terminal/session", Some(&token)).await;
    assert!(matches!(
        frame(&mut socket).await,
        ServerWebSocketFrame::Binary(_)
    ));
    backend.hub.publish_gap("session").unwrap();
    backend
        .hub
        .publish("session", b"RECOVERED".to_vec())
        .unwrap();
    let ServerWebSocketFrame::Binary(gap) = frame(&mut socket).await else {
        panic!("gap frame")
    };
    let metadata: serde_json::Value = serde_json::from_slice(
        &gap[b"\x1b]777;ferryx;".len()..gap.iter().position(|b| *b == 7).unwrap()],
    )
    .unwrap();
    assert_eq!(metadata["kind"], "replayGap");
    assert!(gap.ends_with(b"\x1bc"));
    let ServerWebSocketFrame::Binary(output) = frame(&mut socket).await else {
        panic!("output frame")
    };
    assert!(output.ends_with(b"RECOVERED"));
    drop(socket);
    server.stop().await;
}

#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn ssh_reconnect_safety_web_grid_gap_order() {
    let backend = SocketBackend::new(None);
    let state = socket_state(backend.clone());
    let (token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state).await;
    let mut socket = open_ws_stream(
        server.addr,
        "/api/v1/terminal/session?render=grid",
        Some(&token),
    )
    .await;
    assert!(matches!(
        frame(&mut socket).await,
        ServerWebSocketFrame::Text(_)
    ));
    backend.hub.publish("session", b"OLD".to_vec()).unwrap();
    let ServerWebSocketFrame::Text(old) = frame(&mut socket).await else {
        panic!("old grid frame")
    };
    assert!(old.contains("OLD"));
    backend.hub.publish_gap("session").unwrap();
    let ServerWebSocketFrame::Text(reset) = frame(&mut socket).await else {
        panic!("reset grid")
    };
    let reset: serde_json::Value = serde_json::from_str(&reset).unwrap();
    assert_eq!(reset["type"], "grid");
    assert!(reset.to_string().contains("OLD"));
    assert!(reset.to_string().contains("READY"));
    backend
        .hub
        .publish("session", b"RECOVERED".to_vec())
        .unwrap();
    let ServerWebSocketFrame::Text(output) = frame(&mut socket).await else {
        panic!("recovered grid")
    };
    assert!(output.contains("RECOVERED"));
    drop(socket);
    server.stop().await;
}

async fn recovery_status(socket: &mut tokio::net::TcpStream, expected: &str, generation: &str) {
    let ServerWebSocketFrame::Text(text) = frame(socket).await else {
        panic!("status frame")
    };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"type":"remoteStatus","state":expected,"generation":generation})
    );
}

async fn ssh_input_probe(path: &str) {
    use crate::remote::backend::RemoteRecoveryStatus;
    use crate::terminal::remote::RemoteConnectionState::*;
    let mut backend = SocketBackend::new(None);
    let (tx, _) = tokio::sync::watch::channel(RemoteRecoveryStatus {
        state: Connected,
        generation: 7,
    });
    Arc::get_mut(&mut backend).unwrap().recovery_tx = Some(tx.clone());
    let state = socket_state(backend.clone());
    let (token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state).await;
    let mut socket = open_ws_stream(server.addr, path, Some(&token)).await;
    recovery_status(&mut socket, "connected", "7").await;
    frame(&mut socket).await;
    tx.send_replace(RemoteRecoveryStatus {
        state: Reconnecting,
        generation: 8,
    });
    recovery_status(&mut socket, "reconnecting", "8").await;
    write_client_ws_frame(&mut socket, 2, b"unsafe-binary").await;
    write_client_ws_frame(
        &mut socket,
        1,
        br#"{"type":"remoteWrite","generation":"8","data":"outage"}"#,
    )
    .await;
    // Ordered input barrier: the following close control is not required; a
    // current-generation write after recovery provides the backend completion signal.
    tx.send_replace(RemoteRecoveryStatus {
        state: Disconnected,
        generation: 9,
    });
    recovery_status(&mut socket, "disconnected", "9").await;
    tx.send_replace(RemoteRecoveryStatus {
        state: Connected,
        generation: 10,
    });
    recovery_status(&mut socket, "connected", "10").await;
    write_client_ws_frame(
        &mut socket,
        1,
        br#"{"type":"remoteWrite","generation":"7","data":"stale"}"#,
    )
    .await;
    write_client_ws_frame(
        &mut socket,
        1,
        br#"{"type":"remoteWrite","generation":"10","data":"FRESH"}"#,
    )
    .await;
    let output = match frame(&mut socket).await {
        ServerWebSocketFrame::Text(s) => s,
        ServerWebSocketFrame::Binary(b) => String::from_utf8(b).unwrap(),
        _ => panic!("fresh output"),
    };
    assert!(output.contains("FRESH"));
    assert_eq!(backend.completed_inputs.load(Ordering::SeqCst), 1);
    tx.send_replace(RemoteRecoveryStatus {
        state: Expired,
        generation: 11,
    });
    recovery_status(&mut socket, "expired", "11").await;
    drop(socket);
    server.stop().await;
}

#[tokio::test]
async fn ssh_reconnect_safety_web_raw_status_input_probe() {
    ssh_input_probe("/api/v1/terminal/session").await;
}

#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn ssh_reconnect_safety_web_grid_status_input_probe() {
    ssh_input_probe("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_closes_device_sockets(path: &str) {
    // Given two open sockets for the victim and one for an unrelated device.
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, device) = pair(&state, DevicePermission::Control);
    let (other_token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let mut victims = Vec::new();
    for _ in 0..2 {
        let mut stream =
            tokio::time::timeout(DEADLINE, open_ws_stream(server.addr, path, Some(&token)))
                .await
                .unwrap();
        assert!(!matches!(
            frame(&mut stream).await,
            ServerWebSocketFrame::Close
        ));
        victims.push(stream);
    }
    let mut other = open_ws_stream(server.addr, path, Some(&other_token)).await;
    assert!(!matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    // When the management HTTP route revokes the victim and new output arrives.
    assert_eq!(
        server
            .request(
                "POST",
                &format!("/api/v1/devices/{}/revoke", device.id),
                Some(&other_token),
                None
            )
            .await
            .0,
        204
    );
    state.emit_event(r#"{"event":"after_revoke","payload":{}}"#.into());
    backend
        .hub
        .publish("session", b"AFTER_REVOKE".to_vec())
        .unwrap();
    // Then all victim sockets close without delivering that output, but the
    // unrelated socket still delivers an actual event/raw/grid frame.
    for victim in &mut victims {
        assert!(
            matches!(frame(victim).await, ServerWebSocketFrame::Close),
            "revoked socket delivered post-revocation output"
        );
    }
    assert!(!matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    let _ = state
        .auth_manager
        .revoke_device(&state.auth_manager.validate_token(&other_token).unwrap().id);
    assert!(matches!(
        frame(&mut other).await,
        ServerWebSocketFrame::Close
    ));
    drop(victims);
    drop(other);
    server.stop().await;
}

/// The browser WebSocket constructor cannot set an `Authorization` header, so the
/// direct-gateway terminal URL carried a PERMANENT device token in its query string,
/// where it lands in gateway access logs and browser history. The relay already
/// solved this with single-use tickets; the gateway must offer the same, so a URL
/// only ever carries a short-lived one-shot credential.
#[tokio::test]
async fn gateway_issues_single_use_socket_tickets_and_rejects_a_replayed_one() {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, _device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;

    // Minting requires the bearer, which stays in the header and never reaches a URL.
    let (status, body) = http_request(
        server.addr,
        "POST",
        "/api/v1/socket-ticket",
        Some(&token),
        Some(r#"{"target":"/api/v1/terminal/session"}"#),
    )
    .await;
    assert_eq!(status, 200, "the gateway must issue socket tickets: {body}");
    let issued: serde_json::Value =
        serde_json::from_str(body.trim()).expect("ticket response is JSON");
    let ticket = issued["ticket"].as_str().expect("ticket string").to_owned();
    assert!(!ticket.is_empty());
    assert_ne!(
        ticket, token,
        "a ticket must be a distinct one-shot credential, not the device token"
    );

    // The ticket authorizes exactly one upgrade, with no bearer on the request.
    let mut socket = open_ws_stream(
        server.addr,
        &format!("/api/v1/terminal/session?ticket={ticket}"),
        None,
    )
    .await;
    assert!(matches!(
        frame(&mut socket).await,
        ServerWebSocketFrame::Binary(_)
    ));

    // Replaying the same ticket must not authorize a second attachment.
    let replayed = ws_handshake_status(
        server.addr,
        &format!("/api/v1/terminal/session?ticket={ticket}"),
        None,
    )
    .await;
    assert_eq!(replayed, 401, "a socket ticket must be single-use");

    drop(socket);
    server.stop().await;
}

/// Once every client mints tickets, the legacy `?token=` query credential must stop
/// being accepted. Leaving it enabled keeps the vulnerable shape reachable: anything
/// that captured a URL-borne token from logs or history could still use it.
#[tokio::test]
async fn a_permanent_device_token_in_the_query_string_is_rejected() {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, _device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;

    for path in [
        format!("/api/v1/terminal/session?token={token}"),
        format!("/api/v1/events?token={token}"),
    ] {
        assert_eq!(
            ws_handshake_status(server.addr, &path, None).await,
            401,
            "a device token in the URL must not authorize a socket: {path}"
        );
    }

    // The same token still works as a bearer, so this rejects the LOCATION of the
    // credential rather than the credential itself.
    let (status, _) = http_request(
        server.addr,
        "POST",
        "/api/v1/socket-ticket",
        Some(&token),
        Some(r#"{"target":"/api/v1/events"}"#),
    )
    .await;
    assert_eq!(status, 200, "the bearer must still be accepted in a header");
    server.stop().await;
}

/// A ticket is scoped to the target it was minted for; it must not be a skeleton key
/// for another session's socket.
#[tokio::test]
async fn gateway_socket_ticket_is_bound_to_its_requested_target() {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, _device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;

    let (status, body) = http_request(
        server.addr,
        "POST",
        "/api/v1/socket-ticket",
        Some(&token),
        Some(r#"{"target":"/api/v1/events"}"#),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let issued: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
    let ticket = issued["ticket"].as_str().unwrap().to_owned();

    let crossed = ws_handshake_status(
        server.addr,
        &format!("/api/v1/terminal/session?ticket={ticket}"),
        None,
    )
    .await;
    assert_eq!(
        crossed, 401,
        "an events ticket must not open a terminal socket"
    );
    server.stop().await;
}

/// Minting must require a real bearer; otherwise the ticket endpoint would become an
/// unauthenticated credential vending machine.
#[tokio::test]
async fn gateway_refuses_to_mint_a_socket_ticket_without_a_valid_bearer() {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let server = SecurityServer::start(Arc::clone(&state)).await;

    for bearer in [None, Some("never-issued-anywhere-000")] {
        let (status, _) = http_request(
            server.addr,
            "POST",
            "/api/v1/socket-ticket",
            bearer,
            Some(r#"{"target":"/api/v1/terminal/session"}"#),
        )
        .await;
        assert_eq!(
            status, 401,
            "unauthenticated ticket minting must be refused"
        );
    }
    server.stop().await;
}

#[tokio::test]
async fn revoke_closes_all_event_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/events").await;
}

/// A prompt change on the agent bus reaches a connected events socket once: a reading whose
/// prompt and screen did not move sends nothing.
#[tokio::test]
async fn a_new_prompt_on_the_agent_bus_pushes_one_prompt_changed_frame() {
    use crate::agent_control::events::ObservationDraft;
    use crate::agent_control::{AgentControlState, AgentStateSource, AgentTier};
    let draft = |session: &str, state: AgentControlState, prompt: &str| ObservationDraft {
        session_id: session.into(),
        state,
        state_source: AgentStateSource::Screen,
        tier: AgentTier::Screen,
        prompt_id: Some(prompt.into()),
        kind: Some("question".into()),
        screen_revision: Some(format!("rev-{prompt}")),
        workspace_id: None,
    };
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, _device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let mut socket = open_ws_stream(server.addr, "/api/v1/events", Some(&token)).await;
    // The selection snapshot is the first frame; the bus was subscribed before it was sent.
    let ServerWebSocketFrame::Text(first) = frame(&mut socket).await else {
        panic!("the events socket opens with the selection snapshot");
    };
    assert!(first.contains(REMOTE_ACTIVE_SELECTION_CHANGED_EVENT));

    state
        .agent_events
        .publish(draft("session", AgentControlState::Blocked, "p1"));
    let ServerWebSocketFrame::Text(pushed) = frame(&mut socket).await else {
        panic!("a new prompt must push a text frame");
    };
    let pushed: serde_json::Value = serde_json::from_str(&pushed).unwrap();
    assert_eq!(pushed["type"], "agentPromptChanged");
    assert_eq!(pushed["sessionId"], "session");
    assert_eq!(pushed["promptId"], "p1");
    assert_eq!(pushed["screenRevision"], "rev-p1");

    // Same prompt and screen, only the state moved: no frame. The next frame on the socket is
    // therefore the other session's prompt, never a repeat for the first.
    state
        .agent_events
        .publish(draft("session", AgentControlState::Working, "p1"));
    state
        .agent_events
        .publish(draft("other", AgentControlState::Blocked, "p2"));
    let ServerWebSocketFrame::Text(next) = frame(&mut socket).await else {
        panic!("the second prompt must push a text frame");
    };
    let next: serde_json::Value = serde_json::from_str(&next).unwrap();
    assert_eq!(next["sessionId"], "other");
    assert_eq!(next["promptId"], "p2");
    drop(socket);
    server.stop().await;
}
#[tokio::test]
async fn revoke_closes_all_raw_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_closes_all_grid_sockets_only_for_that_device() {
    revocation_closes_device_sockets("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_cancels_pending_input(path: &str) {
    let backend = SocketBackend::new(None);
    let state = socket_state(Arc::clone(&backend));
    let (token, device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let mut socket = open_ws_stream(server.addr, path, Some(&token)).await;
    frame(&mut socket).await;
    // Exact backend entry acknowledges that input was accepted before revoke.
    let entered = backend.input_gate.entered.notified();
    write_client_ws_frame(&mut socket, 2, b"pending input").await;
    tokio::time::timeout(DEADLINE, entered).await.unwrap();
    assert_eq!(backend.input_gate.pending.load(Ordering::SeqCst), 1);
    assert!(state.auth_manager.revoke_device(&device.id).unwrap());
    assert!(matches!(
        frame(&mut socket).await,
        ServerWebSocketFrame::Close
    ));
    // Closure is a lifecycle barrier, not a timed negative observation.
    assert_eq!(
        backend.input_gate.pending.load(Ordering::SeqCst),
        0,
        "input future was detached rather than dropped"
    );
    assert_eq!(backend.completed_inputs.load(Ordering::SeqCst), 0);
    server.stop().await;
}

#[tokio::test]
async fn revoke_cancels_pending_raw_input() {
    revocation_cancels_pending_input("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_cancels_pending_grid_input() {
    revocation_cancels_pending_input("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_during_attachment(path: &str) {
    let gate = Arc::new(Gate::default());
    let backend = SocketBackend::new(Some(Arc::clone(&gate)));
    let state = socket_state(backend);
    let (token, device) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let entered = gate.entered.notified();
    let handshake = ws_handshake_status(server.addr, path, Some(&token));
    tokio::pin!(handshake);
    tokio::time::timeout(DEADLINE, async {
        tokio::select! {
            _ = entered => {},
            status = &mut handshake => panic!("attachment was not gated: {status}"),
        }
    })
    .await
    .unwrap();
    assert!(state.auth_manager.revoke_device(&device.id).unwrap());
    gate.release.notify_one();
    let status = tokio::time::timeout(DEADLINE, handshake).await.unwrap();
    assert_eq!(
        status, 401,
        "revocation during attachment must prevent upgrade"
    );
    assert_eq!(gate.pending.load(Ordering::SeqCst), 0);
    server.stop().await;
}

#[tokio::test]
async fn revoke_during_raw_attachment_prevents_upgrade() {
    revocation_during_attachment("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_during_grid_attachment_prevents_upgrade() {
    revocation_during_attachment("/api/v1/terminal/session?render=grid").await;
}

async fn revocation_before_upgrade_callback(path: &str) {
    let state = socket_state(SocketBackend::new(None));
    let (token, device) = pair(&state, DevicePermission::Control);
    let gate = Arc::new(Gate::default());
    let gate_for_layer = Arc::clone(&gate);
    // Delay the real 101 response after the handler has authorized/subscribed,
    // before Hyper can invoke its on_upgrade future. No production test hook.
    let router = create_remote_router(Arc::clone(&state)).layer(axum::middleware::from_fn(
        move |request: axum::extract::Request, next: axum::middleware::Next| {
            let gate = Arc::clone(&gate_for_layer);
            async move {
                let response = next.run(request).await;
                assert_eq!(response.status(), 101);
                gate.wait().await;
                response
            }
        },
    ));
    let server = SecurityServer::start_router(router).await;
    let entered = gate.entered.notified();
    let handshake = open_ws_stream(server.addr, path, Some(&token));
    tokio::pin!(handshake);
    tokio::time::timeout(DEADLINE, async {
        tokio::select! {
            _ = entered => {},
            _ = &mut handshake => panic!("upgrade response was not gated"),
        }
    })
    .await
    .unwrap();
    assert!(state.auth_manager.revoke_device(&device.id).unwrap());
    gate.release.notify_one();
    let mut socket = tokio::time::timeout(DEADLINE, handshake).await.unwrap();
    assert!(
        matches!(frame(&mut socket).await, ServerWebSocketFrame::Close),
        "revoked upgrade emitted its initial snapshot"
    );
    server.stop().await;
}

#[tokio::test]
async fn revoke_before_event_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/events").await;
}
#[tokio::test]
async fn revoke_before_raw_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/terminal/session").await;
}
#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn revoke_before_grid_upgrade_is_not_lost() {
    revocation_before_upgrade_callback("/api/v1/terminal/session?render=grid").await;
}

#[tokio::test]
async fn socket_close_restores_desktop_geometry_only_if_resized() {
    let backend = SocketBackend::new(None);
    let state = socket_state(backend.clone());
    let (token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(state).await;

    // 1. Socket connects, does NOT resize, and closes: restore_calls must stay 0.
    {
        let mut socket = open_ws_stream(server.addr, "/api/v1/terminal/session", Some(&token)).await;
        let _ = frame(&mut socket).await;
        write_client_ws_frame(&mut socket, 8, &[]).await;
        while !matches!(frame(&mut socket).await, ServerWebSocketFrame::Close) {}
        drop(socket);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        backend.restore_calls.load(Ordering::SeqCst),
        0,
        "a socket that never resized must not trigger restore on close"
    );

    // 2. Socket connects, performs a resize, and closes: restore_calls must be 1.
    {
        let mut socket = open_ws_stream(server.addr, "/api/v1/terminal/session", Some(&token)).await;
        let _ = frame(&mut socket).await;
        write_client_ws_frame(
            &mut socket,
            1,
            br#"{"type":"resize","cols":100,"rows":30}"#,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(backend.resize_calls.load(Ordering::SeqCst), 1);

        write_client_ws_frame(&mut socket, 8, &[]).await;
        while !matches!(frame(&mut socket).await, ServerWebSocketFrame::Close) {}
        drop(socket);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        backend.restore_calls.load(Ordering::SeqCst),
        1,
        "a socket that resized must trigger restore on close"
    );

    server.stop().await;
}

/// Sends one outcome acknowledgement and then closes. The socket reads its frames in
/// order and awaits the acknowledgement inline, so observing the close proves the
/// acknowledgement frame was fully handled.
async fn acknowledged_outcomes_after_close(path: &str, permission: DevicePermission) -> usize {
    let backend = SocketBackend::new(None);
    let state = socket_state(backend.clone());
    let (token, _) = pair(&state, permission);
    let server = SecurityServer::start(state).await;
    let mut socket = open_ws_stream(server.addr, path, Some(&token)).await;
    assert!(!matches!(frame(&mut socket).await, ServerWebSocketFrame::Close));
    write_client_ws_frame(&mut socket, 1, br#"{"type":"acknowledgeAgentOutcome"}"#).await;
    write_client_ws_frame(&mut socket, 8, &[]).await;
    while !matches!(frame(&mut socket).await, ServerWebSocketFrame::Close) {}
    drop(socket);
    server.stop().await;
    backend.acknowledged_outcomes.load(Ordering::SeqCst)
}

#[tokio::test]
async fn raw_socket_outcome_acknowledgement_requires_control() {
    let path = "/api/v1/terminal/session";
    assert_eq!(acknowledged_outcomes_after_close(path, DevicePermission::View).await, 0);
    assert_eq!(acknowledged_outcomes_after_close(path, DevicePermission::Control).await, 1);
}

#[cfg(feature = "native-terminal")]
#[tokio::test]
async fn grid_socket_outcome_acknowledgement_requires_control() {
    let path = "/api/v1/terminal/session?render=grid";
    assert_eq!(acknowledged_outcomes_after_close(path, DevicePermission::View).await, 0);
    assert_eq!(acknowledged_outcomes_after_close(path, DevicePermission::Control).await, 1);
}

#[test]
fn only_mutating_frames_require_the_input_generation() {
    use crate::remote::server::machine_frame_mutates_input;
    use axum::extract::ws::Message;
    for text in [
        r#"{"type":"resize","generation":"7","cols":80,"rows":24}"#,
        r#"{"type":"signal","generation":"7","signal":"interrupt"}"#,
    ] {
        assert!(machine_frame_mutates_input(&Message::Text(text.into())), "{text}");
    }
    assert!(machine_frame_mutates_input(&Message::Binary(b"ls\r".to_vec().into())));
    for text in [
        r#"{"type":"ping"}"#,
        r#"{"type":"scroll","generation":"7","rows":-3}"#,
        r#"{"type":"acknowledgeAgentOutcome","generation":"7"}"#,
    ] {
        assert!(!machine_frame_mutates_input(&Message::Text(text.into())), "{text}");
    }
    assert!(!machine_frame_mutates_input(&Message::Pong(Vec::new().into())));
}

#[cfg(unix)]
#[tokio::test]
async fn retained_pane_keeps_answering_ping_and_scroll_after_input_generation_advances() {
    use crate::remote::server::machine_input_fixture::Fixture;
    use crate::scoped_contracts::Epoch;
    use crate::terminal::paired_daemon::{Descriptor, Proxy};
    use futures_util::{FutureExt, SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::{client::IntoClientRequest, Message}};

    let fixture = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let source = fixture.create().await;
        let terminal = fixture.owner.terminal_service();
        let sessions = &fixture.state.machine_services.as_ref().unwrap().sessions;
        let mut proxy = Proxy::new(Descriptor {
            host_id: "https://read-only.invalid".into(),
            generation: Epoch(1),
            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                machine_id: "read-only".into(),
                daemon_epoch: Epoch(1),
                session_id: uuid::Uuid::new_v4().to_string(),
            },
            after_sequence: None,
        }, terminal.output_hub().clone()).unwrap();
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PERMISSION_DENIED"));
        let id = terminal.paired().install(proxy).unwrap();
        sessions.retain_paired_metadata_for_test(source["target"]["sessionId"].as_str().unwrap(), &id);
        sessions.record_desktop_geometry(&id, 80, 24);
        let mut request = format!(
            "{}/api/v1/terminal/{id}?daemonEpoch={}&render=grid",
            fixture.base.replace("http:", "ws:"),
            source["target"]["daemonEpoch"].as_str().unwrap(),
        ).into_client_request().unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", fixture.token).parse().unwrap(),
        );
        let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(request))
            .await.expect("grid admission").unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.expect("retained outcome delivered").unwrap();
                let Message::Text(text) = frame else { continue; };
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "{value}");
                if value["type"] == "terminalOutcome" {
                    assert_eq!(value["state"], "blocked");
                    break;
                }
            }
        }).await.unwrap();
        let generation = sessions.machine_controllers.lock().await[&id].generation;

        // A blocked pane reports input generation 0 and is never "ready", so the input
        // fence refuses it outright. Read-only frames must not be routed through it.
        socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.expect("pong after generation advance").unwrap();
                let Message::Text(text) = frame else { continue; };
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "ping must not be input-fenced: {value}");
                if value["type"] == "pong" { break; }
            }
        }).await.unwrap();

        socket.send(Message::Text(serde_json::json!({
            "type": "scroll", "generation": generation.to_string(), "rows": -2,
        }).to_string().into())).await.unwrap();
        socket.send(Message::Text(r#"{"type":"ping"}"#.into())).await.unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                let frame = socket.next().await.expect("scroll answered in order").unwrap();
                let Message::Text(text) = frame else { continue; };
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_ne!(value["type"], "error", "scroll must not be input-fenced: {value}");
                if value["type"] == "pong" { break; }
            }
        }).await.unwrap();

        assert_eq!(
            sessions.write_machine_input(&id, 0, b"forbidden").await.unwrap_err(),
            "SESSION_INPUT_UNAVAILABLE"
        );
        terminal.paired().detach(&id).await.unwrap();
        sessions.prune_paired_metadata_for_test();
    }).catch_unwind().await;
    fixture.cleanup().await;
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
