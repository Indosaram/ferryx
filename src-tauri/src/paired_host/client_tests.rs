use super::super::service::{PairRequest, Secret};
use super::*;
use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
fn caps(machine: &str) -> serde_json::Value {
    json!({"apiVersion":1,"machineId":machine,"daemonEpoch":"1","platform":"linux","accessScope":"machine","permission":"control","capabilities":["directoryBrowseV1","machineWorkspaceV1"],"limits":{"directoryEntries":1000,"terminalSessions":64}})
}
async fn fixture(
    extra: Router,
) -> (
    tempfile::TempDir,
    PairedHostService,
    HostView,
    tokio::task::JoinHandle<()>,
) {
    fixture_identity(
        extra,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await
}
async fn fixture_identity(
    extra: Router,
    wrong: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (
    tempfile::TempDir,
    PairedHostService,
    HostView,
    tokio::task::JoinHandle<()>,
) {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router=Router::new().route("/api/v1/pair/exchange",post(||async {Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))})).route("/host/a/api/v1/capabilities",get(move ||{let wrong=wrong.clone();async move {Json(caps(if wrong.load(std::sync::atomic::Ordering::SeqCst) {"b"} else {"a"}))}})).merge(extra);
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let service = PairedHostService::open_test_loopback(root.path().join("data"));
    let host = service
        .pair(PairRequest {
            relay_origin: origin,
            pin: Secret("fixture".into()),
            display_label: "fixture".into(),
        })
        .await
        .unwrap();
    (root, service, host, task)
}
fn request(host: &HostView, operation: Operation) -> OperationRequest {
    OperationRequest {
        host_id: host.host_id.clone(),
        generation: host.generation,
        operation,
    }
}
async fn cleanup(root: tempfile::TempDir, task: tokio::task::JoinHandle<()>) {
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn real_http_non_json_redirect_and_body_limit() {
    for (mode, expected) in [
        (0, "PAIRED_HOST_INVALID_RESPONSE"),
        (1, "PAIRED_HOST_REDIRECT_REJECTED"),
        (2, "PAYLOAD_TOO_LARGE"),
    ] {
        let route = Router::new().route(
            "/host/a/api/v1/fs/directories",
            get(move |headers: axum::http::HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer fixture-secret");
                match mode {
                    0 => {
                        (axum::http::StatusCode::BAD_GATEWAY, "private relay error").into_response()
                    }
                    1 => axum::response::Redirect::temporary("http://127.0.0.1:1/stolen")
                        .into_response(),
                    _ => (
                        [("content-type", "application/json")],
                        "x".repeat(m::DIRECTORY_JSON_MAX_BYTES + 1),
                    )
                        .into_response(),
                }
            }),
        );
        let (root, service, host, task) = fixture(route).await;
        let error = MachineClient::new()
            .execute(
                &service,
                request(
                    &host,
                    Operation::Directories {
                        path: Some("/space ?&雪".into()),
                        include_hidden: true,
                    },
                ),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, expected);
        assert!(!format!("{error:?}").contains("private relay"));
        cleanup(root, task).await;
    }
}
#[tokio::test]
async fn generation_cancels_blocked_http_without_release() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let route = Router::new().route(
        "/host/a/api/v1/fs/directories",
        get(move || {
            let tx = tx.clone();
            async move {
                tx.send(()).await.unwrap();
                std::future::pending::<String>().await
            }
        }),
    );
    let (root, service, host, task) = fixture(route).await;
    let client = MachineClient::new();
    let pending = client.execute(
        &service,
        request(
            &host,
            Operation::Directories {
                path: None,
                include_hidden: false,
            },
        ),
    );
    let change = async {
        rx.recv().await.unwrap();
        service
            .forget(host.host_id.clone(), host.generation)
            .await
            .unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(pending, change)
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().code, "PAIRED_HOST_STALE_GENERATION");
    cleanup(root, task).await;
}
#[tokio::test]
async fn ambiguous_journal_never_repeats_mutation() {
    let id = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";
    let sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = sent.clone();
    let route = Router::new()
        .route(
            "/host/a/api/v1/workspace/operations/{id}",
            get(move || async move { Json(json!({"state":"pending","requestId":id})) }),
        )
        .route(
            "/host/a/api/v1/workspace/projects",
            post(move || {
                let sent = sent.clone();
                async move {
                    sent.store(true, std::sync::atomic::Ordering::SeqCst);
                    "unexpected mutation"
                }
            }),
        );
    let (root, service, host, task) = fixture(route).await;
    let error = MachineClient::new()
        .execute(
            &service,
            request(
                &host,
                Operation::RegisterProject {
                    request: m::RegisterRequest {
                        request_id: id.into(),
                        repo_path: "/same".into(),
                    },
                },
            ),
        )
        .await
        .unwrap_err();
    assert!(error.ambiguous);
    assert_eq!(error.request_id.as_deref(), Some(id));
    assert!(!observed.load(std::sync::atomic::Ordering::SeqCst));
    cleanup(root, task).await;
}
#[tokio::test]
async fn pending_mutation_response_retains_request_and_ambiguity() {
    let id = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";
    let route=Router::new().route("/host/a/api/v1/workspace/operations/{id}",get(move ||async move {(axum::http::StatusCode::NOT_FOUND,Json(json!({"error":{"code":"OPERATION_NOT_FOUND","message":"missing","retryable":false,"requestId":id,"details":{}}})))})).route("/host/a/api/v1/workspace/projects",post(move ||async move {(axum::http::StatusCode::ACCEPTED,Json(json!({"state":"pending","requestId":id})))}));
    let (root, service, host, task) = fixture(route).await;
    let error = MachineClient::new()
        .execute(
            &service,
            request(
                &host,
                Operation::RegisterProject {
                    request: m::RegisterRequest {
                        request_id: id.into(),
                        repo_path: "/same".into(),
                    },
                },
            ),
        )
        .await
        .unwrap_err();
    cleanup(root, task).await;
    assert!(
        error.ambiguous,
        "pending response must not lose its mutation identity: {error:?}"
    );
    assert_eq!(error.request_id.as_deref(), Some(id));
}
#[tokio::test]
async fn wrong_authenticated_machine_cannot_be_adopted() {
    let wrong = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (root, service, host, task) = fixture_identity(Router::new(), wrong.clone()).await;
    wrong.store(true, std::sync::atomic::Ordering::SeqCst);
    let error = MachineClient::new()
        .execute(&service, request(&host, Operation::Capabilities))
        .await
        .unwrap_err();
    assert_eq!(error.code, "PAIRED_HOST_WRONG_MACHINE");
    cleanup(root, task).await;
}
#[tokio::test]
async fn capability_absence_fails_closed() {
    let (root, service, host, task) = fixture(Router::new()).await;
    let error = MachineClient::new()
        .execute(
            &service,
            request(
                &host,
                Operation::Worktrees {
                    workspace_id: "same".into(),
                },
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "PAIRED_HOST_CAPABILITY_UNAVAILABLE");
    cleanup(root, task).await;
}

#[tokio::test]
async fn real_machine_catalog_replay_checks_digest_and_preserves_metadata() {
    use crate::{
        daemon::server::DaemonServer,
        remote::{
            auth::{DeviceAccessScope, DevicePermission},
            workspace_api,
        },
    };
    let (root, plain, server) = crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let plain = root.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        let server = DaemonServer::new_with_paths(
            Some(root.path().join("remote/config")),
            Some(root.path().join("remote/auth")),
        );
        Ok((root, plain, server))
    })
    .await
    .unwrap();
    let state = server.remote_state().clone();
    let auth = state.auth_manager.clone();
    let (token, device) = crate::ipc::run_blocking(move || {
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        Ok(auth.exchange_pairing_code(&pin, "native fixture").unwrap())
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    // Real shared service handlers; the fixture explicitly advertises the catalog
    // capability still withheld by the production gateway pending downstream gates.
    let router = Router::new()
        .route(
            "/host/a/api/v1/workspace/projects",
            get(workspace_api::list).post(workspace_api::register),
        )
        .route(
            "/host/a/api/v1/workspace/operations/{id}",
            get(workspace_api::operation),
        )
        .with_state(state.clone())
        .route(
            "/host/a/api/v1/capabilities",
            get(|| async { Json(caps("a")) }),
        )
        .route(
            "/api/v1/pair/exchange",
            post(move || {
                let token = token.clone();
                let device = device.clone();
                async move { Json(json!({"token":token,"machineId":"a","device":device})) }
            }),
        );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let service = PairedHostService::open_test_loopback(root.path().join("native"));
    let host = service
        .pair(PairRequest {
            relay_origin: origin,
            pin: Secret("fixture".into()),
            display_label: "fixture".into(),
        })
        .await
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let client = MachineClient::new();
    let operation = Operation::RegisterProject {
        request: m::RegisterRequest {
            request_id: id.clone(),
            repo_path: plain.to_str().unwrap().into(),
        },
    };
    let first = client
        .execute(&service, request(&host, operation.clone()))
        .await
        .unwrap();
    let replay = client
        .execute(&service, request(&host, operation))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&replay).unwrap()
    );
    let error = client
        .execute(
            &service,
            request(
                &host,
                Operation::RegisterProject {
                    request: m::RegisterRequest {
                        request_id: id,
                        repo_path: "/".into(),
                    },
                },
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "REQUEST_CONFLICT");
    let rows = client
        .execute(&service, request(&host, Operation::Projects))
        .await
        .unwrap();
    let OperationResult::Projects(rows) = rows.result else {
        panic!("project result")
    };
    assert_eq!(rows.projects.len(), 1);
    let OperationResult::RegisterProject(project) = first.result else {
        panic!("registration result")
    };
    assert_eq!(rows.projects[0], project);
    assert!(project.metadata.git_root.is_none());
    drop(state);
    drop(server);
    drop(service);
    cleanup(root, task).await;
}

#[tokio::test]
async fn response_provenance_is_fenced_over_http() {
    let id = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";
    let wt = json!({"wsId":"wanted","slug":"branch"});
    let worktree = json!({"workspaceId":"wanted","identity":wt,"path":"/repo/branch","head":"abc","branch":null,"bare":false,"detached":false,"locked":null,"prunable":null,"managed":true});
    let session = json!({"target":{"machineId":"a","daemonEpoch":"7","sessionId":"s"},"workspaceId":"wanted","worktree":wt,"cwd":"/repo","cols":80,"rows":24,"running":true,"providerSession":null,"startSequence":"0","endSequence":"0"});
    let mut cases = vec![];
    for field in ["workspaceId", "identity"] {
        let mut bad = worktree.clone(); bad[field] = if field == "identity" {json!({"wsId":"wrong","slug":"branch"})} else {json!("wrong")};
        cases.push((json!({"kind":"worktrees","workspaceId":"wanted"}), json!({"revision":"1","worktrees":[bad.clone()]}), "/workspace/worktrees"));
        cases.push((json!({"kind":"createWorktree","request":{"requestId":id,"workspaceId":"wanted","worktree":wt}}), bad, "/workspace/worktrees"));
    }
    let status = json!({"workspaceId":"wanted","worktree":wt,"dirty":{"isDirty":false,"files":[]},"dirtyCount":0,"branchDeletion":null,"locked":null,"prunable":null,"liveSessionIds":[],"revision":"1"});
    for field in ["workspaceId", "worktree"] {
        let mut bad = status.clone(); bad[field] = if field == "worktree" {json!({"wsId":"wanted","slug":"wrong"})} else {json!("wrong")};
        cases.push((json!({"kind":"worktreeStatus","workspaceId":"wanted","worktree":wt}),bad,"/workspace/worktrees/status"));
    }
    for field in ["workspaceId", "worktree", "target"] {
        let mut bad = session.clone(); bad[field] = match field { "worktree" => json!({"wsId":"wanted","slug":"wrong"}), "target" => json!({"machineId":"wrong","daemonEpoch":"7","sessionId":"s"}), _ => json!("wrong") };
        cases.push((json!({"kind":"createSession","request":{"requestId":id,"workspaceId":"wanted","worktree":wt,"cols":80,"rows":24,"inheritFromSessionId":null,"cwdRelative":null,"startup":{"kind":"shell"}}}),bad.clone(),"/sessions"));
        if field != "worktree" { cases.push((json!({"kind":"sessions","workspaceId":"wanted"}),json!({"revision":"1","completeness":"complete","sessions":[bad.clone()],"unavailableWorkspaceIds":[]}),"/sessions")); }
        if field == "target" { cases.push((json!({"kind":"operation","requestId":id}),json!({"state":"completed","requestId":id,"outcome":{"kind":"session","session":bad}}),"/workspace/operations/3941b9de-b16d-4d9a-ae0a-118f90fd91f4")); }
    }
    let mut adopted = vec![];
    for (op, body, path) in cases {
        let operation: Operation = serde_json::from_value(op.clone()).unwrap();
        let route = Router::new().route(&format!("/host/a/api/v1{path}"), get({let body=body.clone(); move || {let body=body.clone();async move {Json(body)}}}).post(move || {let body=body.clone();async move {Json(body)}}));
        let (root, service, host, task) = provenance_fixture(route).await;
        let result = MachineClient::new().execute(&service, request(&host, operation)).await;
        if result.is_ok() { adopted.push(op); }
        cleanup(root, task).await;
    }
    assert!(adopted.is_empty(), "wrong provenance adopted: {adopted:?}");
}
async fn provenance_fixture(extra: Router) -> (tempfile::TempDir, PairedHostService, HostView, tokio::task::JoinHandle<()>) {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}",listener.local_addr().unwrap());
    let router = Router::new()
        .route("/api/v1/pair/exchange",post(|| async {Json(json!({"token":"fixture-secret","machineId":"a","device":{"id":"d","name":"d","permission":"control","accessScope":"machine","createdAt":1,"lastSeenAt":1}}))}))
        .route("/host/a/api/v1/capabilities",get(|| async { let mut c=caps("a"); c["capabilities"]=json!(["machineWorkspaceV1","managedWorktreesV1","terminalCreateV1","futureAdditiveV2"]);Json(c)}))
        .merge(extra).fallback(|| async { (axum::http::StatusCode::NOT_FOUND, Json(json!({"error":{"code":"OPERATION_NOT_FOUND","message":"missing","retryable":false,"requestId":"3941b9de-b16d-4d9a-ae0a-118f90fd91f4","details":{}}}))) });
    let task=tokio::spawn(async move {axum::serve(listener,router).await.unwrap()});
    let service=PairedHostService::open_test_loopback(root.path().join("data"));
    let host=service.pair(PairRequest {relay_origin:origin,pin:Secret("fixture".into()),display_label:"fixture".into()}).await.unwrap();
    (root,service,host,task)
}

#[tokio::test]
async fn predecessor_epoch_and_additive_capabilities_are_preserved() {
    let row=json!({"target":{"machineId":"a","daemonEpoch":"7","sessionId":"s"},"workspaceId":"wanted","worktree":null,"cwd":"/repo","cols":80,"rows":24,"running":true,"providerSession":null,"startSequence":"0","endSequence":"0"});
    let route=Router::new().route("/host/a/api/v1/sessions",get(move || {let row=row.clone();async move {Json(json!({"revision":"1","completeness":"complete","sessions":[row],"unavailableWorkspaceIds":[]}))}}));
    let (root,service,host,task)=provenance_fixture(route).await;
    let result=MachineClient::new().execute(&service,request(&host,Operation::Sessions {workspace_id:Some("wanted".into())})).await;
    cleanup(root,task).await;
    let OperationResult::Sessions(rows)=result.unwrap().result else {panic!("sessions")};
    assert_eq!(rows.sessions[0].target.daemon_epoch,Epoch(7));
}
#[tokio::test]
async fn gatefix_remote_error_never_projects_bearer_text() {
    for code in ["WORKTREE_REMOVED_BRANCH_RETAINED", "fixture-secret"] {
        let route = Router::new().route("/host/a/api/v1/fs/directories", get(move |headers: axum::http::HeaderMap| async move {
            assert_eq!(headers["authorization"], "Bearer fixture-secret");
            (axum::http::StatusCode::CONFLICT, Json(json!({"error": {
                "code": code, "message": "Bearer fixture-secret", "retryable": false,
                "requestId": "3941b9de-b16d-4d9a-ae0a-118f90fd91f4",
                "details": {"branch": "fixture-secret", "nested": {"token": "fixture-secret"},
                    "fixture-secret": true, "pruned": "fixture-secret", "worktreeRemoved": true, "branchDeleted": false}
            }})))
        }));
        let (root, service, host, task) = fixture(route).await;
        let error = MachineClient::new().execute(&service, request(&host, Operation::Directories { path: None, include_hidden: false })).await.unwrap_err();
        cleanup(root, task).await;
        let wire = serde_json::to_string(&error).unwrap();
        assert!(!wire.contains("fixture-secret"), "native error leaked fixture bearer: {wire}");
        assert!(!format!("{error:?}").contains("fixture-secret"));
        let projected = error.machine_error.unwrap();
        if code == "WORKTREE_REMOVED_BRANCH_RETAINED" {
            assert_eq!(error.code, code);
            assert_eq!(serde_json::to_value(projected.details).unwrap(), json!({"worktreeRemoved": true, "branchDeleted": false}));
        } else {
            assert_eq!(error.code, "PAIRED_HOST_REMOTE_ERROR");
            assert!(projected.details.is_empty());
        }
    }
}

#[tokio::test]
async fn gatefix_journal_error_uses_the_same_safe_projection() {
    let id = "3941b9de-b16d-4d9a-ae0a-118f90fd91f4";
    let route = Router::new().route("/host/a/api/v1/workspace/operations/{id}", get(move || async move {
        Json(json!({"state":"completed", "requestId":id, "outcome":{"kind":"error", "error":{
            "code":"WORKTREE_REMOVED_PRUNE_FAILED", "message":"fixture-secret", "retryable":false,
            "requestId":id, "details":{"branch":"fixture-secret", "pruned":false, "worktreeRemoved":true}
        }}}))
    }));
    let (root, service, host, task) = fixture(route).await;
    let result = MachineClient::new().execute(&service, request(&host, Operation::Operation { request_id: id.into() })).await.unwrap();
    cleanup(root, task).await;
    assert!(!serde_json::to_string(&result).unwrap().contains("fixture-secret"));
    let OperationResult::Operation(m::Operation::Completed { outcome: m::OperationOutcome::Error { error }, .. }) = result.result else { panic!("journal error"); };
    assert_eq!(serde_json::to_value(error.details).unwrap(), json!({"pruned":false, "worktreeRemoved":true}));
}

#[tokio::test]
async fn gatefix_create_only_peer_receives_zero_socket_requests() {
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let route = Router::new()
        .route("/host/a/api/v1/sessions/s", get(|| async { Json(json!({"status":"running","session":{"target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"workspaceId":"w","worktree":null,"cwd":"/fixture","cols":80,"rows":24,"running":true,"providerSession":null,"startSequence":"0","endSequence":"0"}})) }))
        .route("/host/a/api/v1/terminal/s", get(move || { let attempts = attempts.clone(); async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            axum::http::StatusCode::FORBIDDEN
        }}));
    // This fixture advertises terminalCreateV1 but not terminalStreamV1.
    let (root, service, host, task) = provenance_fixture(route).await;
    let descriptor = crate::terminal::paired_daemon::Descriptor {
        host_id: host.host_id, generation: host.generation,
        target: m::RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "s".into() }, after_sequence: None,
    };
    let result = MachineClient::new().attach_terminal(&service, &descriptor).await;
    cleanup(root, task).await;
    assert_eq!(observed.load(Ordering::SeqCst), 0, "create-only peer received a socket request");
    let error = result.err().expect("stream capability must be required");
    assert_eq!(error.code, "PAIRED_HOST_CAPABILITY_UNAVAILABLE");
    assert_eq!(serde_json::to_value(error).unwrap()["ambiguous"], false);
}

#[test]
fn operation_requires_machine_workspace_capability() {
    let operation=Operation::Operation {request_id:"3941b9de-b16d-4d9a-ae0a-118f90fd91f4".into()};
    assert_eq!(operation.route().unwrap().capability,Some("machineWorkspaceV1"));
}

#[tokio::test]
async fn gatefix_p06_relay_ticket_failure_surfaces_typed_error_without_ws_retry() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    for (status_code, expected_code, json_body) in [
        (
            axum::http::StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            Some(json!({
                "error": {
                    "code": "UNAUTHORIZED",
                    "message": "Revoked credential",
                    "retryable": false,
                    "requestId": "3941b9de-b16d-4d9a-ae0a-118f90fd91f4",
                    "details": { "status": 401 }
                }
            })),
        ),
        (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "RATE_LIMITED",
            Some(json!({
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "Rate limit exceeded",
                    "retryable": true,
                    "requestId": "3941b9de-b16d-4d9a-ae0a-118f90fd91f4",
                    "details": { "status": 429 }
                }
            })),
        ),
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "MACHINE_SERVICE_UNAVAILABLE",
            None,
        ),
        (
            axum::http::StatusCode::NOT_FOUND,
            "NOT_FOUND",
            None,
        ),
    ] {
        let ws_attempts = Arc::new(AtomicUsize::new(0));
        let ws_attempts_clone = ws_attempts.clone();
        let body_clone = json_body.clone();
        let route = Router::new()
            .route(
                "/host/a/api/v1/sessions/s",
                get(|| async {
                    Json(json!({
                        "status": "running",
                        "session": {
                            "target": { "machineId": "a", "daemonEpoch": "1", "sessionId": "s" },
                            "workspaceId": "w",
                            "worktree": null,
                            "cwd": "/fixture",
                            "cols": 80,
                            "rows": 24,
                            "running": true,
                            "providerSession": null,
                            "startSequence": "0",
                            "endSequence": "0"
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/socket-ticket",
                post(move || {
                    let body = body_clone.clone();
                    async move {
                        if let Some(b) = body {
                            (status_code, Json(b)).into_response()
                        } else {
                            status_code.into_response()
                        }
                    }
                }),
            )
            .route(
                "/host/a/api/v1/terminal/s",
                get(move || {
                    let attempts = ws_attempts_clone.clone();
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        axum::http::StatusCode::FORBIDDEN
                    }
                }),
            );

        let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap()))
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/api/v1/pair/exchange",
                post(|| async {
                    Json(json!({
                        "token": "fixture-secret",
                        "machineId": "a",
                        "device": {
                            "id": "d",
                            "name": "d",
                            "permission": "control",
                            "accessScope": "machine",
                            "createdAt": 1,
                            "lastSeenAt": 1
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/capabilities",
                get(|| async {
                    let mut c = caps("a");
                    c["capabilities"] = json!(["machineWorkspaceV1", "terminalCreateV1", "terminalStreamV1"]);
                    Json(c)
                }),
            )
            .merge(route);

        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let host = service
            .pair(PairRequest {
                relay_origin: origin,
                pin: Secret("fixture".into()),
                display_label: "fixture".into(),
            })
            .await
            .unwrap();

        let descriptor = crate::terminal::paired_daemon::Descriptor {
            host_id: host.host_id,
            generation: host.generation,
            target: m::RemoteTerminalTarget {
                machine_id: "a".into(),
                daemon_epoch: Epoch(1),
                session_id: "s".into(),
            },
            after_sequence: None,
        };

        let result = MachineClient::new().attach_terminal(&service, &descriptor).await;
        cleanup(root, task).await;

        assert_eq!(
            ws_attempts.load(Ordering::SeqCst),
            0,
            "No WebSocket retry must be attempted when ticket minting fails on relay transport ({})",
            expected_code
        );

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("ticket mint failure must be returned as an error"),
        };
        assert_eq!(err.code, expected_code);
        let machine_err = err.machine_error.expect("structured machine error must be preserved");
        assert_eq!(machine_err.code, expected_code);
        assert_eq!(
            machine_err.details.get("status").and_then(|v| v.as_u64()),
            Some(status_code.as_u16() as u64)
        );
        assert_eq!(
            machine_err.details.get("httpStatus").and_then(|v| v.as_u64()),
            Some(status_code.as_u16() as u64)
        );
    }
}

#[test]
fn test_is_relay_transport_classification() {
    assert!(is_relay_transport(&Url::parse("https://relay.checka.cc/host/m1").unwrap()));
    assert!(is_relay_transport(&Url::parse("http://127.0.0.1:8787/host/target-id").unwrap()));
    assert!(is_relay_transport(&Url::parse("https://custom.relay.io:8443/host/abc%20123/api/v1").unwrap()));

    assert!(!is_relay_transport(&Url::parse("http://127.0.0.1:43821").unwrap()));
    assert!(!is_relay_transport(&Url::parse("http://127.0.0.1:43821/").unwrap()));
    assert!(!is_relay_transport(&Url::parse("http://192.168.1.10:43821/api/v1").unwrap()));
    assert!(!is_relay_transport(&Url::parse("https://direct-gateway.internal:443/api/v1/terminal/s").unwrap()));
}

#[test]
fn test_map_ticket_error_structured_and_plain() {
    // 1. Structured JSON ErrorEnvelope
    let body = serde_json::to_vec(&json!({
        "error": {
            "code": "UNAUTHORIZED",
            "message": "Token expired",
            "retryable": false,
            "requestId": "3941b9de-b16d-4d9a-ae0a-118f90fd91f4",
            "details": { "reason": "expired" }
        }
    })).unwrap();
    let err = map_ticket_error(reqwest::StatusCode::UNAUTHORIZED, &body);
    assert_eq!(err.code, "UNAUTHORIZED");
    let me = err.machine_error.unwrap();
    assert_eq!(me.details["status"], 401);
    assert_eq!(me.details["httpStatus"], 401);
    assert_eq!(err.request_id.as_deref(), Some("3941b9de-b16d-4d9a-ae0a-118f90fd91f4"));

    // 2. Relay simple json error
    let relay_err_body = serde_json::to_vec(&json!({
        "error": "Device token not authorized for this machine"
    })).unwrap();
    let err2 = map_ticket_error(reqwest::StatusCode::UNAUTHORIZED, &relay_err_body);
    assert_eq!(err2.code, "UNAUTHORIZED");
    let me2 = err2.machine_error.unwrap();
    assert_eq!(me2.message, "Device token not authorized for this machine");
    assert_eq!(me2.details["status"], 401);
    assert_eq!(me2.details["httpStatus"], 401);
    assert!(!me2.retryable);

    // 3. Plain text / empty body 429
    let err3 = map_ticket_error(reqwest::StatusCode::TOO_MANY_REQUESTS, b"");
    assert_eq!(err3.code, "RATE_LIMITED");
    let me3 = err3.machine_error.unwrap();
    assert_eq!(me3.details["status"], 429);
    assert!(me3.retryable);

    // 4. Plain text / empty body 503
    let err4 = map_ticket_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, b"Service Unavailable");
    assert_eq!(err4.code, "MACHINE_SERVICE_UNAVAILABLE");
    let me4 = err4.machine_error.unwrap();
    assert_eq!(me4.details["status"], 503);
    assert!(me4.retryable);

    // 5. 404 NOT_FOUND
    let err5 = map_ticket_error(reqwest::StatusCode::NOT_FOUND, b"");
    assert_eq!(err5.code, "NOT_FOUND");
    let me5 = err5.machine_error.unwrap();
    assert_eq!(me5.details["status"], 404);
    assert!(!me5.retryable);
}

#[tokio::test]
async fn test_p05_response_body_mapping_table() {
    let cases = vec![
        (axum::http::StatusCode::NOT_FOUND, json!({"error": "PIN not found"}), "PIN_NOT_FOUND"),
        (axum::http::StatusCode::TOO_MANY_REQUESTS, json!({"code": "RATE_LIMITED", "message": "Too many attempts"}), "RATE_LIMITED"),
        (axum::http::StatusCode::SERVICE_UNAVAILABLE, json!({"message": "Gateway temporarily down"}), "SERVICE_UNAVAILABLE"),
        (axum::http::StatusCode::UNAUTHORIZED, json!({"code": "PIN_EXPIRED", "message": "The PIN has expired"}), "PIN_EXPIRED"),
    ];

    for (status, body, expected_code) in cases {
        let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let body_val = body.clone();
        let router = Router::new().route(
            "/api/v1/pair/exchange",
            post(move || async move { (status, Json(body_val.clone())) }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let err = service.pair(PairRequest {
            relay_origin: origin,
            pin: Secret("123456".into()),
            display_label: "fixture".into(),
        }).await.unwrap_err();
        task.abort();
        assert_eq!(err.code, expected_code, "status {status} body {body:?} should produce code {expected_code}, got {}", err.code);
    }

    // Malformed response (200 OK with non-JSON body)
    {
        let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route(
            "/api/v1/pair/exchange",
            post(|| async { (axum::http::StatusCode::OK, "not-valid-json") }),
        );
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let err = service.pair(PairRequest {
            relay_origin: origin,
            pin: Secret("123456".into()),
            display_label: "fixture".into(),
        }).await.unwrap_err();
        task.abort();
        assert_eq!(err.code, "MALFORMED_RESPONSE", "malformed response body must map to MALFORMED_RESPONSE, got {}", err.code);
    }

    // Transport failure (connection refused)
    {
        let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // Close listener immediately
        let service = PairedHostService::open_test_loopback(root.path().join("data"));
        let err = service.pair(PairRequest {
            relay_origin: format!("http://{addr}"),
            pin: Secret("123456".into()),
            display_label: "fixture".into(),
        }).await.unwrap_err();
        assert_eq!(err.code, "TRANSPORT", "transport failure must map to TRANSPORT, got {}", err.code);
    }
}

#[tokio::test]
async fn test_p14_inventory_mutation_emits_inventory_changed_event() {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/api/v1/pair/exchange",
        post(|| async {
            Json(json!({
                "token": "fixture-secret",
                "machineId": "a",
                "device": {
                    "id": "d",
                    "name": "d",
                    "permission": "control",
                    "accessScope": "machine",
                    "createdAt": 1,
                    "lastSeenAt": 1
                }
            }))
        }),
    ).route(
        "/host/a/api/v1/capabilities",
        get(|| async { Json(caps("a")) }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut service = PairedHostService::open_test_loopback(root.path().join("data"));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    service.set_event_sink(std::sync::Arc::new(move |event| {
        let _ = tx.send(event);
    }));

    // 1. Pair mutation
    let host = service.pair(PairRequest {
        relay_origin: origin,
        pin: Secret("fixture".into()),
        display_label: "fixture".into(),
    }).await.unwrap();

    let pair_event = rx.try_recv().expect("pair mutation must emit inventory changed event");
    assert_eq!(pair_event.r#type, "pair");
    assert_eq!(pair_event.host_id.as_deref(), Some(host.host_id.as_str()));
    assert_eq!(pair_event.generation.as_deref(), Some(host.generation.0.to_string().as_str()));
    let event_host = pair_event.host.as_ref().expect("pair event must include host view");
    assert_eq!(event_host.host_id, host.host_id);
    assert_eq!(event_host.generation, host.generation);
    assert!(event_host.online);

    // Verify wire serialization conforms to ui/src/lib/pairedHostInventory.ts contract
    let json_val = serde_json::to_value(&pair_event).unwrap();
    assert_eq!(json_val["type"], "pair");
    assert_eq!(json_val["hostId"], host.host_id);
    assert_eq!(json_val["generation"], host.generation.0.to_string());
    assert_eq!(json_val["host"]["hostId"], host.host_id);
    assert_eq!(json_val["host"]["generation"], host.generation.0.to_string());
    assert_eq!(json_val["host"]["online"], true);

    // 2. Forget mutation
    service.forget(host.host_id.clone(), host.generation).await.unwrap();
    let forget_event = rx.try_recv().expect("forget mutation must emit inventory changed event");
    assert_eq!(forget_event.r#type, "forget");
    assert_eq!(forget_event.host_id.as_deref(), Some(host.host_id.as_str()));
    assert_eq!(forget_event.generation.as_deref(), Some(host.generation.0.to_string().as_str()));

    task.abort();
}

#[tokio::test]
async fn test_r4_n1_migrate_event_carries_correlated_host_view() {
    let root = crate::ipc::run_blocking(|| Ok(tempfile::tempdir().unwrap())).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/api/v1/pair/exchange",
        post(|| async {
            Json(json!({
                "token": "fixture-secret",
                "machineId": "a",
                "device": {
                    "id": "d",
                    "name": "d",
                    "permission": "control",
                    "accessScope": "machine",
                    "createdAt": 1,
                    "lastSeenAt": 1
                }
            }))
        }),
    ).route(
        "/host/a/api/v1/capabilities",
        get(|| async { Json(caps("a")) }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut service = PairedHostService::open_test_loopback(root.path().join("data"));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    service.set_event_sink(std::sync::Arc::new(move |event| {
        let _ = tx.send(event);
    }));

    let receipt = service
        .migrate_legacy(super::super::service::MigrationRequest {
            relay_origin: origin,
            machine_id: "a".into(),
            display_label: "fixture".into(),
            device_token: Secret("fixture-secret".into()),
        })
        .await
        .unwrap();

    let event = rx.try_recv().expect("migrate must emit the inventory changed event");
    assert_eq!(event.r#type, "migrate");
    assert_eq!(event.host_id.as_deref(), Some(receipt.host_id.as_str()));
    assert_eq!(event.generation.as_deref(), Some(receipt.generation.0.to_string().as_str()));
    let event_host = event.host.as_ref().expect("R4-N1: migrate event must carry the correlated host view");
    assert_eq!(event_host.host_id, receipt.host_id);
    assert_eq!(event_host.generation, receipt.generation);

    // The GUI fence keys on the serialized hostId/generation pair — assert the
    // exact wire shape the IPC layer delivers to pairedHostInventory.ts.
    let json_val = serde_json::to_value(&event).unwrap();
    assert_eq!(json_val["type"], "migrate");
    assert_eq!(json_val["host"]["hostId"], receipt.host_id);
    assert_eq!(json_val["host"]["generation"], receipt.generation.0.to_string());

    cleanup(root, task).await;
}

#[test]
fn test_r5_n3_machine_rejection_stays_definitive_and_transport_uncertainty_relabeled() {
    let mut machine = ClientError {
        code: "UNAUTHORIZED".into(),
        machine_error: Some(m::MachineError {
            code: "UNAUTHORIZED".into(),
            message: "denied".into(),
            retryable: false,
            request_id: "srv-1".into(),
            details: Default::default(),
        }),
        request_id: None,
        ambiguous: false,
    };
    machine.relabel_transport_uncertainty("req-1");
    assert_eq!(machine.request_id.as_deref(), Some("req-1"));
    assert!(!machine.ambiguous, "a structured machine rejection is definitive");

    let mut transport = ClientError {
        code: "TIMEOUT".into(),
        machine_error: None,
        request_id: None,
        ambiguous: false,
    };
    transport.relabel_transport_uncertainty("req-2");
    assert_eq!(transport.request_id.as_deref(), Some("req-2"));
    assert!(transport.ambiguous, "transport uncertainty must be relabeled ambiguous");
}


