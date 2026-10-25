#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use crate::daemon::protocol::{
        DaemonResponse, DaemonSessionDetails, RecoveryAuthorityEvidence,
    };

    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_consistency_commit_retains_unsettled_expired_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let terminals = Arc::new(crate::terminal::TerminalService::default());
        let descriptor = serde_json::from_value(serde_json::json!({
            "backendSessionId":"dead-at-commit", "target":{"hostId":"host","ownerId":"owner","epoch":"1","backendSessionId":"target"},
            "config":{"host":{"id":"host","label":"host","hostname":"127.0.0.1","port":1,"source":"manual","authMethod":"agent"},"environment":{"platform":"posix","executor":"sh","version":"test","home":"/tmp","temp":"/tmp","git":true},"helper":{"executable":"/helper","root":"/root"},"projectId":"ssh:dead","projectPath":"/project","worktree":null,"agentIdentity":null},
            "clientRequestId":"dead-request","remoteCursor":"0","cols":80,"rows":24
        })).unwrap();
        terminals.remote().live_import(crate::terminal::remote::RemoteExportState {
            descriptor, generation: 1, pending_size: None, pid: None, bridge_transfer: None,
        }, None).unwrap();
        terminals.remote().entry_for_test("dead-at-commit").unwrap().state.lock().details.state = crate::terminal::remote::RemoteConnectionState::Expired;
        let manager = Arc::new(crate::daemon::handover::HandoverManager::new(dir.path().join("daemon.sock")));
        let legacy = dir.path().join("legacy.sock");
        let _listener = tokio::net::UnixListener::bind(&legacy).unwrap();
        manager.force_prepared_for_test(legacy);
        let (done, retired) = tokio::sync::watch::channel(false);
        manager.set_retirement_action(move || { done.send_replace(true); });
        let committing = manager.clone();
        tokio::task::spawn_blocking(move || committing.commit_handover_v5(&terminals)).await.unwrap().unwrap();
        assert!(!*retired.borrow());
        assert_eq!(manager.status(), crate::daemon::handover::HandoverStatus::Draining);
    }

    #[test]
    fn recovery_consistency_inv6_old_optional_metadata_backward_decode() {
        let legacy_json = r#"{
            "sessionId": "legacy-session-1",
            "workspaceId": "ws-1",
            "worktree": null,
            "cwd": "/tmp",
            "cols": 80,
            "rows": 24,
            "running": true,
            "startSequence": 0,
            "endSequence": 100,
            "lastOutputAgeMs": 50,
            "suspended": false
        }"#;

        let details: DaemonSessionDetails =
            serde_json::from_str(legacy_json).expect("legacy JSON must decode cleanly");

        assert_eq!(details.session_id, "legacy-session-1");
        assert!(details.is_recoverable_legacy());
        assert_eq!(details.incarnation, None);
        assert_eq!(details.recovery_authority, None);
        assert_eq!(details.is_paired_proxy, None);
        assert!(!details.is_affirmative_exit());
        assert!(!details.is_paired_proxy_actor());

        let legacy_response_json = format!(
            r#"{{"type":"describeSessionOk","session":{},"daemonEpoch":"42"}}"#,
            legacy_json
        );
        let resp: DaemonResponse = serde_json::from_str(&legacy_response_json)
            .expect("legacy DescribeSessionOk must decode cleanly");
        if let DaemonResponse::DescribeSessionOk {
            session,
            daemon_epoch,
        } = resp
        {
            assert_eq!(session.session_id, "legacy-session-1");
            assert_eq!(daemon_epoch.as_deref(), Some("42"));
            assert_eq!(session.recovery_authority, None);
        } else {
            panic!("Expected DescribeSessionOk");
        }
    }

    #[test]
    fn recovery_consistency_inv4_paired_actor_absence_is_not_process_exit() {
        let paired_details = DaemonSessionDetails {
            session_id: "paired-session-proxy".into(),
            workspace_id: None,
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            running: false,
            start_sequence: Some(0),
            end_sequence: Some(50),
            last_output_age_ms: None,
            suspended: false,
            reader_paused: None,
            kernel_stopped: None,
            registry_suspended: None,
            suspension_source: None,
            incarnation: Some("proxy-actor-inc-1".into()),
            recovery_authority: Some(RecoveryAuthorityEvidence {
                status: "owner_unavailable".into(),
                exit_code: None,
                exit_reason: None,
                incarnation: Some("proxy-actor-inc-1".into()),
                daemon_epoch: None,
                is_paired_proxy: Some(true),
            }),
            is_paired_proxy: Some(true),
            generation: None,
        };

        assert!(paired_details.is_paired_proxy_actor());
        assert!(!paired_details.is_affirmative_exit());
        assert!(!paired_details.running);
        assert_eq!(
            paired_details.recovery_authority.as_ref().unwrap().status,
            "owner_unavailable"
        );
    }

    #[test]
    fn recovery_consistency_inv2_matching_incarnation_affirmative_exit() {
        let exited_details = DaemonSessionDetails {
            session_id: "local-session-1".into(),
            workspace_id: Some("ws-local".into()),
            worktree: None,
            cwd: Some("/repo".into()),
            cols: 80,
            rows: 24,
            running: false,
            start_sequence: Some(0),
            end_sequence: Some(100),
            last_output_age_ms: Some(1000),
            suspended: false,
            reader_paused: None,
            kernel_stopped: None,
            registry_suspended: None,
            suspension_source: None,
            incarnation: Some("pty-incarnation-alpha".into()),
            recovery_authority: Some(RecoveryAuthorityEvidence {
                status: "affirmative_exit".into(),
                exit_code: Some(0),
                exit_reason: None,
                incarnation: Some("pty-incarnation-alpha".into()),
                daemon_epoch: Some("10".into()),
                is_paired_proxy: Some(false),
            }),
            is_paired_proxy: Some(false),
            generation: None,
        };

        assert!(exited_details.is_affirmative_exit());
        assert!(!exited_details.is_paired_proxy_actor());
        assert!(exited_details.matches_incarnation("pty-incarnation-alpha"));
        assert!(!exited_details.matches_incarnation("pty-incarnation-beta"));
    }

    #[test]
    fn recovery_consistency_inv3_mismatched_incarnation_blocks() {
        let live_details = DaemonSessionDetails {
            session_id: "session-conflict".into(),
            workspace_id: None,
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            running: true,
            start_sequence: None,
            end_sequence: None,
            last_output_age_ms: None,
            suspended: false,
            reader_paused: None,
            kernel_stopped: None,
            registry_suspended: None,
            suspension_source: None,
            incarnation: Some("incarnation-epoch-2".into()),
            recovery_authority: Some(RecoveryAuthorityEvidence {
                status: "running".into(),
                exit_code: None,
                exit_reason: None,
                incarnation: Some("incarnation-epoch-2".into()),
                daemon_epoch: Some("2".into()),
                is_paired_proxy: Some(false),
            }),
            is_paired_proxy: Some(false),
            generation: None,
        };

        assert!(!live_details.matches_incarnation("incarnation-epoch-1"));
        assert!(live_details.matches_incarnation("incarnation-epoch-2"));
    }

    #[test]
    fn recovery_consistency_inv1_scoped_missing_preserves_uncertainty() {
        let err_resp = DaemonResponse::Error {
            message: "Session 'sess-unknown' not found".into(),
            code: Some("SESSION_NOT_FOUND".into()),
            details: Some(serde_json::json!({
                "source": "daemon_describe",
                "kind": "session_not_found",
                "sessionId": "sess-unknown",
                "recoveryOutcome": "retryable",
                "reason": "scoped_missing"
            })),
        };

        let json = serde_json::to_string(&err_resp).expect("must serialize");
        let decoded: DaemonResponse = serde_json::from_str(&json).expect("must deserialize");

        if let DaemonResponse::Error { code, details, .. } = decoded {
            assert_eq!(code.as_deref(), Some("SESSION_NOT_FOUND"));
            let det = details.expect("details must be present");
            assert_eq!(det["kind"], "session_not_found");
            assert_eq!(det["recoveryOutcome"], "retryable");
            assert_eq!(det["reason"], "scoped_missing");
        } else {
            panic!("Expected Error response");
        }
    }

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_consistency_real_local_process_state_authority_projection() {
        let server = crate::daemon::server::DaemonServer::new();
        let server = std::sync::Arc::new(server);
        let server_clone = Arc::clone(&server);
        let (_repo, home_dir) = crate::ipc::run_blocking(move || -> Result<_, crate::ipc::error::IpcError> {
            let repo = init_test_git_repo();
            server_clone.handle_register_workspace("test-local-ws", repo.path().to_str().unwrap()).unwrap();
            Ok((repo, tempfile::tempdir().unwrap()))
        })
        .await
        .unwrap();
        let helper_home = home_dir.path().to_str().unwrap().to_string();
        let session_id = server
            .handle_spawn("local-proj-session", "test-local-ws", crate::daemon::session_service::SpawnOptions { worktree: None, cwd: None, cols: 80, rows: 24, shell: Some("/bin/sh".to_string()) }, None, Some(helper_home))
            .await
            .unwrap();
        let resp = server.describe_session_identity(&session_id).await;
        let DaemonResponse::DescribeSessionOk { session, daemon_epoch } = resp else {
            panic!("Expected DescribeSessionOk, got {resp:?}");
        };
        assert!(daemon_epoch.is_some());
        assert!(session.running);
        assert_eq!(session.is_paired_proxy, Some(false));
        let auth = session.recovery_authority.expect("recovery_authority must be projected");
        assert_eq!(auth.status, "running");
        assert_eq!(auth.daemon_epoch, daemon_epoch);
        assert_eq!(auth.incarnation, session.incarnation);
        assert_eq!(auth.is_paired_proxy, Some(false));
        assert!(auth.exit_code.is_none());
        server.handle_close(&session_id).await.unwrap();
    }

    #[tokio::test]
    async fn recovery_consistency_real_paired_absent_actor_projection() {
        let server = crate::daemon::server::DaemonServer::new();
        let (store_file, _tmp_dir) = crate::ipc::run_blocking(|| -> Result<_, crate::ipc::error::IpcError> {
            let tmp_dir = tempfile::tempdir().unwrap();
            let store_file = tmp_dir.path().join("paired_descriptors.json");
            let descriptor = crate::terminal::paired_daemon::Descriptor {
                host_id: "host-test-1".into(),
                generation: crate::scoped_contracts::Epoch(1),
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id: "machine-1".into(),
                    daemon_epoch: crate::scoped_contracts::Epoch(42),
                    session_id: "remote-target-1".into(),
                },
                after_sequence: None,
            };
            let mut descriptors = std::collections::HashMap::new();
            descriptors.insert("daemon-session:absent-proxy-1".to_string(), descriptor);
            let json = serde_json::to_string(&descriptors).unwrap();
            std::fs::write(&store_file, json).unwrap();
            Ok((store_file, tmp_dir))
        })
        .await
        .unwrap();
        server.terminal_service().paired().set_store_path(store_file.clone());

        let resp = server.describe_session_identity("daemon-session:absent-proxy-1").await;
        let DaemonResponse::DescribeSessionOk { session, daemon_epoch } = resp else {
            panic!("Expected DescribeSessionOk, got {resp:?}");
        };
        assert_eq!(daemon_epoch, Some(server.epoch.to_string()));
        assert!(!session.running);
        assert_eq!(session.is_paired_proxy, Some(true));
        let auth = session.recovery_authority.expect("recovery_authority must be present for absent proxy");
        assert_eq!(auth.status, "owner_unavailable");
        assert_eq!(auth.daemon_epoch, Some(server.epoch.to_string()));
        assert_eq!(auth.is_paired_proxy, Some(true));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_consistency_real_running_process_exit_signal_lifecycle() {
        let server = crate::daemon::server::DaemonServer::new();
        let server = std::sync::Arc::new(server);
        let server_clone = Arc::clone(&server);
        let (_repo, home_dir) = crate::ipc::run_blocking(move || -> Result<_, crate::ipc::error::IpcError> {
            let repo = init_test_git_repo();
            server_clone
                .handle_register_workspace("test-exit-ws", repo.path().to_str().unwrap())
                .unwrap();
            Ok((repo, tempfile::tempdir().unwrap()))
        })
        .await
        .unwrap();
        let helper_home = home_dir.path().to_str().unwrap().to_string();
        let session_id = server
            .handle_spawn(
                "local-exit-session",
                "test-exit-ws",
                crate::daemon::session_service::SpawnOptions { worktree: None, cwd: None, cols: 80, rows: 24, shell: Some("/bin/sh".to_string()) },
                None,
                Some(helper_home),
            )
            .await
            .unwrap();

        let removal = server.terminal_service().pty_manager().removal_notify();
        let notified = removal.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        let session = server
            .terminal_service()
            .get_session(&session_id)
            .expect("session must exist");

        server
            .terminal_service()
            .pty_manager()
            .write_input(&session_id, b"exit 42\n")
            .expect("write_input must succeed");

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            if server.terminal_service().pty_manager().has_session(&session_id) {
                notified.await;
            }
            while server.terminal_service().pty_manager().has_session(&session_id) {
                let next_notified = removal.notified();
                tokio::pin!(next_notified);
                next_notified.as_mut().enable();
                if !server.terminal_service().pty_manager().has_session(&session_id) {
                    break;
                }
                next_notified.await;
            }
        })
        .await
        .expect("removal notification must arrive within bounded timeout");

        assert_eq!(
            session.state(),
            crate::terminal::PtySessionState::Exited { code: Some(42) }
        );
        assert!(session.is_reaped(), "PTY child process must be reaped");

        let resp = server.describe_session_identity(&session_id).await;
        let DaemonResponse::Error { code, details, .. } = resp else {
            panic!("Expected Error response after removal, got {resp:?}");
        };
        assert_eq!(code.as_deref(), Some("SESSION_NOT_FOUND"));
        let det = details.expect("details must be present");
        assert_eq!(det["kind"], "session_not_found");
        assert_eq!(det["reason"], "scoped_missing");
    }

    #[tokio::test]
    async fn recovery_consistency_authority_persistence_roundtrip() {
        let auth = RecoveryAuthorityEvidence {
            status: "affirmative_exit".into(),
            exit_code: Some(137),
            exit_reason: Some("SIGKILL".into()),
            incarnation: Some("pty-alpha-42".into()),
            daemon_epoch: Some("9007199254740991".into()),
            is_paired_proxy: Some(false),
        };
        let json = serde_json::to_string(&auth).unwrap();
        let decoded: RecoveryAuthorityEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(auth, decoded);

        let descriptor = crate::terminal::paired_daemon::Descriptor {
            host_id: "host-rt-1".into(),
            generation: crate::scoped_contracts::Epoch(999),
            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                machine_id: "machine-rt".into(),
                daemon_epoch: crate::scoped_contracts::Epoch(100),
                session_id: "session-rt".into(),
            },
            after_sequence: Some(crate::scoped_contracts::Epoch(50)),
        };
        let desc_json = serde_json::to_string(&descriptor).unwrap();
        let desc_decoded: crate::terminal::paired_daemon::Descriptor = serde_json::from_str(&desc_json).unwrap();
        assert_eq!(descriptor.host_id, desc_decoded.host_id);
        assert_eq!(descriptor.generation, desc_decoded.generation);

        crate::ipc::run_blocking(move || -> Result<_, crate::ipc::error::IpcError> {
            let tmp_dir = tempfile::tempdir().unwrap();
            let auth_file = tmp_dir.path().join("durable_authority.json");
            std::fs::write(&auth_file, &json).unwrap();
            let read_back = std::fs::read_to_string(&auth_file).unwrap();
            let disk_decoded: RecoveryAuthorityEvidence =
                serde_json::from_str(&read_back).unwrap();
            assert_eq!(auth, disk_decoded);
            Ok(())
        })
        .await
        .unwrap();
    }
}
