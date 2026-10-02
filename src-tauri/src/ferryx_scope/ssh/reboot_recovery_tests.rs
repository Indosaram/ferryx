use super::*;
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn make_test_runtime_with_boot(
    runtime_dir: &tempfile::TempDir,
    storage_dir: &tempfile::TempDir,
    boot_id: &str,
    host: &str,
    token: &str,
) -> Runtime {
    #[cfg(unix)]
    super::super::private_file(storage_dir.path()).unwrap();
    Runtime::new_with_options(
        runtime_dir.path().to_path_buf(),
        host.to_string(),
        token.to_string(),
        Some(storage_dir.path().to_path_buf()),
        Some(boot_identity::BootIdSource::Injected(boot_id.to_string())),
    )
    .expect("failed to create Runtime with boot identity")
}

fn register_test_project(runtime: &Runtime, token: &str, id: &str, path: &Path) {
    runtime
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "project.register".to_string(),
            params: json!({
                "id": id,
                "path": path.to_string_lossy(),
            }),
        })
        .expect("project register should succeed");
}

fn create_mock_transcript(dir: &Path, session_id: &str) {
    let omo_sessions = dir.join(".omo").join("sessions");
    std::fs::create_dir_all(&omo_sessions).unwrap();
    let file = omo_sessions.join(format!("{session_id}.jsonl"));
    std::fs::write(&file, format!(r#"{{"type":"session","id":"{session_id}"}}"#)).unwrap();
    super::super::private_file(&file).unwrap();
}

fn make_echo_args_script(dir: &Path) -> String {
    if cfg!(windows) {
        let scripts = dir.join("mock cli");
        std::fs::create_dir_all(&scripts).unwrap();
        let bat = scripts.join("mock_omo.bat");
        std::fs::write(&bat, "@echo off\r\necho RESUMED_ARGS: %~1 %~2\r\nset /p stop=\r\n").unwrap();
        bat.to_string_lossy().to_string()
    } else {
        let sh = dir.join("mock_omo.sh");
        std::fs::write(&sh, "#!/bin/sh\necho \"RESUMED_ARGS: $@\"\nread stop\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        sh.to_string_lossy().to_string()
    }
}

#[cfg(windows)]
#[test]
fn reboot_recovery_windows_cli_wrapper_preserves_arguments_with_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("mock cli");
    std::fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("mock.bat");
    std::fs::write(&script, "@echo off\r\necho RESUMED_ARGS: %~1 %~2\r\n").unwrap();
    let command = recovery::resume_command_builder(script.to_str().unwrap(), &["--session".into(), "exact-id".into()]).unwrap();
    let args = command.get_argv();
    let output = std::process::Command::new(&args[0]).args(&args[1..]).output().unwrap();
    assert!(output.status.success(), "argv: {args:?}; stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8_lossy(&output.stdout).contains("RESUMED_ARGS: --session exact-id"),
        "argv: {args:?}; stdout: {}", String::from_utf8_lossy(&output.stdout));
}

#[cfg(unix)]
#[test]
fn reboot_recovery_accepts_owned_readable_journal_but_refuses_other_writers() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    create_mock_transcript(dir.path(), "exact-owned-id");
    let path = dir.path().join(".omo/sessions/exact-owned-id.jsonl");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(recovery::verify_agent_transcript_exists("omo", "exact-owned-id", path.to_str(), dir.path()).is_ok());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(recovery::verify_agent_transcript_exists("omo", "exact-owned-id", path.to_str(), dir.path()).is_err());
}

fn await_fixture_output(runtime: &Runtime, target: &Value, expected: &str) -> String {
    let id = target["backendSessionId"].as_str().unwrap();
    let session = runtime.sessions.lock().unwrap().get(id).unwrap().clone();
    let (lock, signal) = &*session.output;
    let contents = |state: &Output| -> String {
        String::from_utf8_lossy(&state.chunks.iter().flat_map(|(_, bytes)| bytes.iter().copied()).collect::<Vec<_>>()).into_owned()
    };
    let state = lock.lock().unwrap();
    let (state, timeout) = signal.wait_timeout_while(state, Duration::from_secs(10), |state| {
        !contents(state).contains(expected) && !state.exited
    }).unwrap();
    let output = contents(&state);
    drop(state);
    assert!(!timeout.timed_out() && output.contains(expected), "fixture output missing: {output}");
    output
}

#[test]
fn reboot_recovery_capture_survives_restart_and_cleared_runtime_temp() {
    let runtime_dir1 = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "test-reboot-host";
    let token = "test-token-1";
    let logical_id = "req-reboot-session-1";

    let rt1 = make_test_runtime_with_boot(&runtime_dir1, &storage_dir, "boot-epoch-1", host, token);
    register_test_project(&rt1, token, "proj-1", project_dir.path());

    let spawn_res = rt1
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
                "cols": 100,
                "rows": 30,
            }),
        })
        .expect("pty.spawn should succeed");

    let prev_target = spawn_res.get("target").cloned().unwrap();
    let prev_pid = spawn_res.get("pid").and_then(Value::as_u64).unwrap() as u32;
    assert!(prev_pid > 0);

    create_mock_transcript(project_dir.path(), "omo-session-recovery-abc");

    rt1.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-recovery-abc",
            })),
        )
        .expect("update_agent_state should succeed");

    let loaded = rt1
        .recovery_store()
        .load_record(logical_id)
        .unwrap()
        .expect("record should exist");
    assert_eq!(loaded.agent.as_deref(), Some("omo"));
    assert_eq!(loaded.cols, 100);
    assert_eq!(loaded.rows, 30);
    assert_eq!(loaded.boot_id, "boot-epoch-1");
    assert_eq!(loaded.project_root, project_dir.path().canonicalize().unwrap());

    drop(rt1);
    let _ = std::fs::remove_dir_all(runtime_dir1.path());

    let runtime_dir2 = tempfile::tempdir().unwrap();
    let rt2 = make_test_runtime_with_boot(&runtime_dir2, &storage_dir, "boot-epoch-2", host, token);

    let script_path = make_echo_args_script(project_dir.path());
    let script_clone = script_path.clone();
    rt2.set_executable_resolver_for_test(Some(Arc::new(move |_prog| Some(script_clone.clone()))));

    let recover_res = rt2
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": prev_target,
            }),
        })
        .expect("pty.recover should succeed across cleared temp, rehydrated project, and changed boot");

    let new_target = recover_res.get("target").cloned().unwrap();
    let new_pid = recover_res.get("pid").and_then(Value::as_u64).unwrap() as u32;

    assert_ne!(
        new_target["backendSessionId"], prev_target["backendSessionId"],
        "Recovery must allocate independent TargetRef per execution"
    );
    assert_ne!(
        new_target["ownerId"], prev_target["ownerId"],
        "New runtime instance has independent ownerId"
    );
    assert!(new_pid > 0);

    #[cfg(windows)]
    rt2.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "pty.write".to_string(),
        params: json!({"target": new_target, "text": "\u{1b}[1;1R"}),
    }).unwrap();
    let accumulated = await_fixture_output(&rt2, &new_target, "RESUMED_ARGS: --session omo-session-recovery-abc");
    assert!(
        accumulated.contains("RESUMED_ARGS:") && accumulated.contains("omo-session-recovery-abc"),
        "PTY output must confirm process ran with exact args, got: {accumulated}"
    );

    let updated_record = rt2
        .recovery_store()
        .load_record(logical_id)
        .unwrap()
        .expect("updated record must exist");
    assert_eq!(updated_record.boot_id, "boot-epoch-2");
    assert_eq!(
        updated_record.exact_previous_target.backend_session_id,
        new_target["backendSessionId"].as_str().unwrap()
    );
}

#[test]
fn reboot_recovery_same_boot_refusal() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "test-same-boot-host";
    let token = "test-token";
    let logical_id = "req-same-boot";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-unchanged", host, token);
    register_test_project(&rt, token, "proj-same", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-same",
            }),
        })
        .unwrap();

    let prev_target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-session-123");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-123",
            })),
        )
        .unwrap();

    let recover_res = rt.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "pty.recover".to_string(),
        params: json!({
            "logicalSessionId": logical_id,
            "previousTarget": prev_target,
        }),
    });

    assert!(recover_res.is_err());
    let err = recover_res.unwrap_err();
    assert!(
        err.contains("RECOVERY_REFUSED: automatic recovery requires OS boot identity change"),
        "expected same boot refusal error, got: {err}"
    );
}

#[test]
fn reboot_recovery_wrong_target_owner_host_refusal() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-alpha";
    let token = "test-token";
    let logical_id = "req-wrong-target";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();

    let real_target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-session-123");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-123",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let mut wrong_host_target = real_target.clone();
    wrong_host_target["hostId"] = json!("host-beta");
    let err_host = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": wrong_host_target,
            }),
        })
        .unwrap_err();
    assert!(
        err_host.contains("TARGET_EXPIRED"),
        "expected target expired for wrong host, got: {err_host}"
    );

    let mut wrong_backend_target = real_target.clone();
    wrong_backend_target["backendSessionId"] = json!("wrong-backend-uuid");
    let err_target = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": wrong_backend_target,
            }),
        })
        .unwrap_err();
    assert!(
        err_target.contains("REQUEST_CONFLICT: previousTarget does not match recorded target"),
        "expected request conflict for wrong target, got: {err_target}"
    );
}

#[test]
fn reboot_recovery_closed_session_refusal() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-closed";
    let token = "test-token";
    let logical_id = "req-closed-session";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();

    let target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-session-closed");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-closed",
            })),
        )
        .unwrap();

    rt.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    })
    .expect("pty.stop should succeed");

    let rec = rt.recovery_store().load_record(logical_id).unwrap().unwrap();
    assert!(rec.disabled, "pty.stop must mark recovery record disabled");

    rt.set_boot_id_for_test("boot-2".to_string());

    let recover_res = rt.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "pty.recover".to_string(),
        params: json!({
            "logicalSessionId": logical_id,
            "previousTarget": target,
        }),
    });

    assert!(recover_res.is_err());
    let err = recover_res.unwrap_err();
    assert!(
        err.contains("RECOVERY_REFUSED: session was already terminated"),
        "expected terminated session refusal, got: {err}"
    );
}

#[test]
fn reboot_recovery_missing_or_corrupt_provider_session_refusal() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-provider";
    let token = "test-token";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let logical_no_provider = "req-no-prov";
    let spawn1 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_no_provider,
                "projectId": "proj-1",
            }),
        })
        .unwrap();
    let target1 = spawn1.get("target").cloned().unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let err1 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_no_provider,
                "previousTarget": target1,
            }),
        })
        .unwrap_err();
    assert!(
        err1.contains("REMOTE_RECOVERY_UNSUPPORTED: missing authoritative agent reference"),
        "expected missing agent error, got: {err1}"
    );

    let logical_wrong_key = "req-wrong-key";
    let spawn2 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_wrong_key,
                "projectId": "proj-1",
            }),
        })
        .unwrap();
    let target2 = spawn2.get("target").cloned().unwrap();

    let mut corrupted = rt.recovery_store().load_record(logical_wrong_key).unwrap().unwrap();
    corrupted.agent = Some("omo".to_string());
    corrupted.provider_session = Some(json!({"key": "conversation_id", "id": "conv-123"}));
    rt.recovery_store().save_record(&corrupted).unwrap();
    rt.set_boot_id_for_test("boot-3".to_string());

    let err2 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_wrong_key,
                "previousTarget": target2,
            }),
        })
        .unwrap_err();
    assert!(
        err2.contains("REMOTE_RECOVERY_INVALID: providerSession key must be session_id"),
        "expected wrong key rejection, got: {err2}"
    );
}

#[test]
fn reboot_recovery_missing_transcript_fails_closed() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-missing-transcript";
    let token = "test-token";
    let logical_id = "req-missing-transcript";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();
    let target = spawn_res.get("target").cloned().unwrap();

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-nonexistent",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let err = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": target,
            }),
        })
        .unwrap_err();
    assert!(
        err.contains("REMOTE_RECOVERY_REFUSED: transcript for OMO session"),
        "expected missing transcript refusal, got: {err}"
    );
}

#[test]
fn reboot_recovery_lost_response_retry_returns_same_target() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-lost-response";
    let token = "test-token";
    let logical_id = "req-lost-response";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();

    let orig_target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-lost-resp-id");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-lost-resp-id",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let script_path = make_echo_args_script(project_dir.path());
    let script_clone = script_path.clone();
    rt.set_executable_resolver_for_test(Some(Arc::new(move |_prog| Some(script_clone.clone()))));

    let first_recover = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": orig_target,
            }),
        })
        .unwrap();

    let recovered_target = first_recover.get("target").cloned().unwrap();
    let recovered_pid = first_recover.get("pid").and_then(Value::as_u64).unwrap();

    let retry_recover = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_id,
                "previousTarget": orig_target,
            }),
        })
        .unwrap();

    assert_eq!(
        retry_recover.get("target").cloned().unwrap(),
        recovered_target,
        "Retry after lost response must return exact same target"
    );
    assert_eq!(
        retry_recover.get("pid").and_then(Value::as_u64).unwrap(),
        recovered_pid,
        "Retry after lost response must return exact same PID without launching another process"
    );
}

#[test]
fn reboot_recovery_progress_crash_fails_closed() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-crash";
    let token = "test-token";
    let logical_id = "req-crash-progress";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();

    let orig_target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-session-crash");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-crash",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let marker = recovery::PreLaunchMarker {
        logical_session_id: logical_id.to_string(),
        attempt_id: "attempt-abandoned".to_string(),
        host: host.to_string(),
        boot_id: "boot-2".to_string(),
        source_target: serde_json::from_value(orig_target.clone()).unwrap(),
        provider_key: Some("omo:omo-session-crash".to_string()),
        timestamp: 12345678,
    };
    rt.recovery_store().record_pre_launch(&marker).unwrap();

    let recover_res = rt.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "pty.recover".to_string(),
        params: json!({
            "logicalSessionId": logical_id,
            "previousTarget": orig_target,
        }),
    });

    assert!(recover_res.is_err());
    let err = recover_res.unwrap_err();
    assert!(
        err.contains("RECOVERY_FAILED: ambiguous interrupted launch"),
        "expected ambiguous interrupted launch failure, got: {err}"
    );
}

#[test]
fn reboot_recovery_concurrent_repeated_recover_serialized() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-concurrent";
    let token = "test-token";
    let logical_id = "req-concurrent-recover";

    let rt = Arc::new(make_test_runtime_with_boot(
        &runtime_dir,
        &storage_dir,
        "boot-1",
        host,
        token,
    ));
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn_res = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_id,
                "projectId": "proj-1",
            }),
        })
        .unwrap();

    let orig_target = spawn_res.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-session-concurrent");

    rt.recovery_store()
        .update_agent_state(
            logical_id,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-session-concurrent",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let script_path = make_echo_args_script(project_dir.path());
    let script_clone = script_path.clone();
    rt.set_executable_resolver_for_test(Some(Arc::new(move |_prog| Some(script_clone.clone()))));

    let thread_count = 4;
    let barrier = Arc::new(Barrier::new(thread_count));
    let mut handles = Vec::new();

    for _ in 0..thread_count {
        let rt_clone = Arc::clone(&rt);
        let token_clone = token.to_string();
        let logical_clone = logical_id.to_string();
        let target_clone = orig_target.clone();
        let b = Arc::clone(&barrier);

        handles.push(std::thread::spawn(move || {
            b.wait();
            rt_clone.handle(Request {
                protocol: 1,
                token: token_clone,
                op: "pty.recover".to_string(),
                params: json!({
                    "logicalSessionId": logical_clone,
                    "previousTarget": target_clone,
                }),
            })
        }));
    }

    let mut results = Vec::new();
    for h in handles {
        let res = h.join().unwrap().expect("all concurrent calls must succeed");
        results.push(res);
    }

    let first_target = results[0].get("target").cloned().unwrap();
    let first_pid = results[0].get("pid").and_then(Value::as_u64).unwrap();

    for res in &results[1..] {
        assert_eq!(
            res.get("target").cloned().unwrap(),
            first_target,
            "all concurrent recoveries must return identical target"
        );
        assert_eq!(
            res.get("pid").and_then(Value::as_u64).unwrap(),
            first_pid,
            "all concurrent recoveries must return identical PID"
        );
    }
}

#[test]
fn reboot_recovery_provider_uniqueness_refuses_a_second_controller() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let storage_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();

    let host = "host-adoption";
    let token = "test-token";
    let logical_1 = "req-provider-1";
    let logical_2 = "req-provider-2";

    let rt = make_test_runtime_with_boot(&runtime_dir, &storage_dir, "boot-1", host, token);
    register_test_project(&rt, token, "proj-1", project_dir.path());

    let spawn1 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_1,
                "projectId": "proj-1",
            }),
        })
        .unwrap();
    let target1 = spawn1.get("target").cloned().unwrap();

    let spawn2 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "clientRequestId": logical_2,
                "projectId": "proj-1",
            }),
        })
        .unwrap();
    let target2 = spawn2.get("target").cloned().unwrap();

    create_mock_transcript(project_dir.path(), "omo-shared-provider-id");

    rt.recovery_store()
        .update_agent_state(
            logical_1,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-shared-provider-id",
            })),
        )
        .unwrap();

    rt.recovery_store()
        .update_agent_state(
            logical_2,
            Some("omo".to_string()),
            Some(json!({
                "key": "session_id",
                "id": "omo-shared-provider-id",
            })),
        )
        .unwrap();

    rt.set_boot_id_for_test("boot-2".to_string());

    let script_path = make_echo_args_script(project_dir.path());
    let script_clone = script_path.clone();
    rt.set_executable_resolver_for_test(Some(Arc::new(move |_prog| Some(script_clone.clone()))));

    let rec1 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_1,
                "previousTarget": target1,
            }),
        })
        .unwrap();

    assert!(rec1["pid"].as_u64().unwrap() > 0);

    let rec2 = rt
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.recover".to_string(),
            params: json!({
                "logicalSessionId": logical_2,
                "previousTarget": target2,
            }),
        })
        .unwrap_err();
    assert!(rec2.starts_with("REQUEST_CONFLICT:"));
}

#[test]
fn reboot_recovery_demonstrates_atomic_replacement() {
    let storage_dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    super::super::private_file(storage_dir.path()).unwrap();
    let store = recovery::RecoveryStore::new(storage_dir.path().to_path_buf(), "test-host").unwrap();

    let target1 = TargetRef {
        host_id: "test-host".into(),
        owner_id: "owner-1".into(),
        epoch: Epoch(1),
        backend_session_id: "sess-1".into(),
    };

    let mut record = recovery::RecoveryRecord {
        logical_session_id: "req-replace-test".into(),
        host: "test-host".into(),
        exact_previous_target: target1,
        boot_id: "boot-initial".into(),
        project_id: "proj-1".into(),
        project_root: storage_dir.path().to_path_buf(),
        worktree: None,
        cwd: storage_dir.path().to_path_buf(),
        cols: 80,
        rows: 24,
        agent: None,
        provider_session: None,
        disabled: false,
        updated_at: 1000,
    };

    store.save_record(&record).expect("initial save must succeed");
    let loaded1 = store.load_record("req-replace-test").unwrap().unwrap();
    assert_eq!(loaded1.boot_id, "boot-initial");
    assert_eq!(loaded1.cols, 80);

    record.cols = 120;
    record.boot_id = "boot-second".into();
    record.updated_at = 2000;
    store.save_record(&record).expect("replacement save must succeed");

    let loaded2 = store.load_record("req-replace-test").unwrap().unwrap();
    assert_eq!(loaded2.boot_id, "boot-second");
    assert_eq!(loaded2.cols, 120);
    assert_eq!(loaded2.updated_at, 2000);
}

#[test]
fn reboot_recovery_exact_command_plan_resolution_matrix() {
    let cwd_dir = tempfile::tempdir().unwrap();
    let cwd = cwd_dir.path().to_path_buf();
    let dummy_target = TargetRef {
        host_id: "h".into(),
        owner_id: "o".into(),
        epoch: Epoch(1),
        backend_session_id: "b".into(),
    };

    create_mock_transcript(&cwd, "sess-123");

    let make_rec = |agent: &str, key: &str, id: &str| recovery::RecoveryRecord {
        logical_session_id: "req-test".into(),
        host: "h".into(),
        exact_previous_target: dummy_target.clone(),
        boot_id: "b".into(),
        project_id: "p".into(),
        project_root: cwd.clone(),
        worktree: None,
        cwd: cwd.clone(),
        cols: 80,
        rows: 24,
        agent: Some(agent.into()),
        provider_session: Some(json!({ "key": key, "id": id })),
        disabled: false,
        updated_at: 100,
    };

    let omo = recovery::resolve_resume_command(&make_rec("omo", "session_id", "sess-123"), &cwd).unwrap();
    assert_eq!(omo.program, "omo");
    assert_eq!(omo.args, vec!["--session", "sess-123"]);

    let claude_journal = cwd.join("claude.jsonl");
    std::fs::write(&claude_journal, r#"{"sessionId":"sess-456","type":"user"}"#).unwrap();
    super::super::private_file(&claude_journal).unwrap();
    let mut claude_record = make_rec("claude", "sessionId", "sess-456");
    claude_record.provider_session.as_mut().unwrap()["transcriptPath"] = json!(claude_journal);
    let claude = recovery::resolve_resume_command(&claude_record, &cwd).unwrap();
    assert_eq!(claude.program, "claude");
    assert_eq!(claude.args, vec!["--resume", "sess-456"]);

    let codex_journal = cwd.join("codex.jsonl");
    std::fs::write(&codex_journal, r#"{"type":"session_meta","payload":{"id":"sess-789"}}"#).unwrap();
    super::super::private_file(&codex_journal).unwrap();
    let mut codex_record = make_rec("codex", "session_id", "sess-789");
    codex_record.provider_session.as_mut().unwrap()["transcriptPath"] = json!(codex_journal);
    let codex = recovery::resolve_resume_command(&codex_record, &cwd).unwrap();
    assert_eq!(codex.program, "codex");
    assert_eq!(codex.args, vec!["resume", "sess-789"]);

    assert!(recovery::resolve_resume_command(&make_rec("antigravity", "session_id", "agy-1"), &cwd).is_err());

    let bad_agent = recovery::resolve_resume_command(&make_rec("unsupported_agent", "session_id", "s"), &cwd);
    assert!(bad_agent.is_err());
    assert!(bad_agent.unwrap_err().contains("REMOTE_RECOVERY_UNSUPPORTED"));

    let bad_key = recovery::resolve_resume_command(&make_rec("omo", "bad_key", "s"), &cwd);
    assert!(bad_key.is_err());
    assert!(bad_key.unwrap_err().contains("REMOTE_RECOVERY_INVALID"));
}
