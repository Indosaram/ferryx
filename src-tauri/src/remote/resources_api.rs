//! `GET /api/v1/resources`: this machine's resource sample for a paired desktop.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};

use crate::daemon::resource_usage::{HostResourceSnapshot, ResourceSampler, SessionProbe};
use crate::remote::state::RemoteGatewayState;

pub const RESOURCE_USAGE_CAPABILITY: &str = "resourceUsageV1";

/// Shared so CPU utilization is differenced against the previous remote read.
static SAMPLER: LazyLock<Arc<ResourceSampler>> = LazyLock::new(|| Arc::new(ResourceSampler::new()));

pub(super) async fn read(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
) -> Response {
    let id = uuid::Uuid::new_v4().to_string();
    let _admission = match super::workspace_api::admit(state.clone(), headers, false, &id).await {
        Ok(admission) => admission,
        Err(refusal) => return refusal.into(),
    };
    let probes = probes(&state);
    let sampler = Arc::clone(&SAMPLER);
    let disk = crate::daemon::server::get_runtime_dir();
    let Ok(snapshot) = crate::ipc::run_blocking(move || Ok(sampler.sample(&probes, &disk))).await else {
        return super::session_api::failure("MACHINE_SERVICE_UNAVAILABLE", &id);
    };
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, "no-store")],
        Json(redact_paths(snapshot)),
    )
        .into_response()
}

fn probes(state: &RemoteGatewayState) -> Vec<SessionProbe> {
    let terminals = &state.terminal_service;
    terminals
        .list_sessions()
        .into_iter()
        .filter_map(|session_id| {
            let session = terminals.get_session(&session_id)?;
            Some(SessionProbe {
                session_id,
                pid: session.pid(),
                worktree_path: session
                    .worktree_path()
                    .map(|path| path.to_string_lossy().into_owned()),
            })
        })
        .collect()
}

/// Remote endpoints never expose absolute paths: only the worktree directory name leaves.
pub(crate) fn redact_paths(mut snapshot: HostResourceSnapshot) -> HostResourceSnapshot {
    for session in &mut snapshot.sessions {
        session.worktree_path = session.worktree_path.take().and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::resource_usage::SessionResourceUsage;

    fn snapshot(paths: &[Option<&str>]) -> HostResourceSnapshot {
        HostResourceSnapshot {
            sampled_at_ms: 1,
            platform: "linux".into(),
            cpu_count: 4,
            cpu_utilization: None,
            load_average_1m: None,
            memory_total_bytes: None,
            memory_used_bytes: None,
            swap_total_bytes: None,
            swap_used_bytes: None,
            uptime_seconds: None,
            disk_total_bytes: None,
            disk_free_bytes: None,
            process_count: None,
            sessions: paths
                .iter()
                .enumerate()
                .map(|(index, path)| SessionResourceUsage {
                    session_id: format!("s{index}"),
                    pid: None,
                    worktree_path: path.map(str::to_owned),
                    cpu_percent: None,
                    resident_bytes: None,
                    process_count: None,
                })
                .collect(),
            unavailable: vec![],
        }
    }

    #[test]
    fn redact_paths_keeps_only_the_worktree_directory_name() {
        let redacted = redact_paths(snapshot(&[
            Some("/home/me/code/app/.ferryx-worktrees/ws/fix-login"),
            Some("/home/me/code/app"),
            None,
        ]));
        let paths: Vec<_> = redacted
            .sessions
            .iter()
            .map(|session| session.worktree_path.as_deref())
            .collect();
        assert_eq!(paths, vec![Some("fix-login"), Some("app"), None]);
        assert!(redacted
            .sessions
            .iter()
            .filter_map(|session| session.worktree_path.as_deref())
            .all(|path| !path.contains('/')));
    }

    #[tokio::test]
    async fn http_route_samples_for_machine_control_and_refuses_everyone_else() {
        use crate::remote::auth::{DeviceAccessScope, DevicePermission};
        use futures_util::FutureExt;
        use std::time::Duration;

        let (data, daemon) = crate::ipc::run_blocking(|| {
            let data = tempfile::tempdir().unwrap();
            let daemon = crate::daemon::server::DaemonServer::new_with_paths(
                Some(data.path().join("config.json")),
                Some(data.path().join("auth.json")),
            );
            Ok((data, daemon))
        })
        .await
        .unwrap();
        let state = Arc::clone(daemon.remote_state());
        let auth = Arc::clone(&state.auth_manager);
        let (machine, mirror) = crate::ipc::run_blocking(move || {
            let pin = auth
                .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
                .unwrap();
            let machine = auth.exchange_pairing_code(&pin, "resources").unwrap().0;
            let pin = auth.create_pairing_code(DevicePermission::Control);
            let mirror = auth.exchange_pairing_code(&pin, "mirror").unwrap().0;
            Ok((machine, mirror))
        })
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, crate::remote::server::create_remote_router(state))
                .with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });
        let outcome = std::panic::AssertUnwindSafe(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap();
            let capabilities: serde_json::Value = client
                .get(format!("http://{addr}/api/v1/capabilities"))
                .bearer_auth(&machine)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(capabilities["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == RESOURCE_USAGE_CAPABILITY));

            let url = format!("http://{addr}/api/v1/resources");
            let response = client.get(&url).bearer_auth(&machine).send().await.unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let snapshot: HostResourceSnapshot = response.json().await.unwrap();
            assert_eq!(snapshot.platform, std::env::consts::OS);
            assert!(snapshot.cpu_count >= 1);
            assert!(
                snapshot.memory_total_bytes.is_some()
                    || snapshot.unavailable.iter().any(|m| m == "memoryTotalBytes")
            );

            for (token, expected) in [("invalid", 401u16), (mirror.as_str(), 403)] {
                let response = client.get(&url).bearer_auth(token).send().await.unwrap();
                assert_eq!(response.status().as_u16(), expected, "token {token}");
            }
            let anonymous = client.get(&url).send().await.unwrap();
            assert_eq!(anonymous.status().as_u16(), 401);
        })
        .catch_unwind()
        .await;
        stop_tx.send(()).unwrap();
        server.await.unwrap();
        drop(daemon);
        crate::ipc::run_blocking(move || {
            data.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}
