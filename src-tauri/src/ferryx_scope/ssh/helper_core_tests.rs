use super::*;
use std::time::{Duration, Instant};

fn shell_program_and_args() -> (Option<String>, Vec<String>) {
    if cfg!(windows) {
        (Some("cmd.exe".to_string()), vec![])
    } else {
        (Some("/bin/sh".to_string()), vec![])
    }
}

fn env_set(var: &str, val: &str) -> String {
    if cfg!(windows) {
        format!("set {var}={val}\r\n")
    } else {
        format!("export {var}=\"{val}\"\n")
    }
}

fn env_echo(var: &str) -> String {
    if cfg!(windows) {
        format!("echo {var}=%{var}%\r\n")
    } else {
        format!("echo \"{var}=${var}\"\n")
    }
}

fn make_runtime(dir: &tempfile::TempDir, token: &str) -> Runtime {
    Runtime::new(
        dir.path().to_path_buf(),
        "test-host".to_string(),
        token.to_string(),
    )
    .expect("failed to create Runtime")
}

fn register_project(
    runtime: &Runtime,
    token: &str,
    project_id: &str,
    path: &Path,
) -> Result<Value, String> {
    runtime.handle(Request {
        protocol: 1,
        token: token.to_string(),
        op: "project.register".to_string(),
        params: json!({
            "id": project_id,
            "path": path.to_string_lossy(),
        }),
    })
}

fn read_until(
    runtime: &Runtime,
    token: &str,
    target: &Value,
    cursor: &mut String,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut accumulated = String::new();
    let mut cursor_reports = 0;

    while Instant::now() < deadline {
        let remaining_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(2000) as u64;
        let wait_ms = remaining_ms.max(50);
        let res = runtime.handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "cursor": cursor.clone(),
                "waitMs": wait_ms,
            }),
        })?;

        if let Some(next_cur) = res.get("cursor").and_then(Value::as_str) {
            *cursor = next_cur.to_string();
        } else if let Some(seq) = res.get("afterSequence").and_then(Value::as_u64) {
            *cursor = seq.to_string();
        }

        if let Some(chunks) = res.get("chunks").and_then(Value::as_array) {
            for c in chunks {
                if let Some(b64) = c.get("data").and_then(Value::as_str) {
                    if let Ok(bytes) = BASE64_STANDARD.decode(b64) {
                        accumulated.push_str(&String::from_utf8_lossy(&bytes));
                    }
                } else if let Some(bytes_val) = c.get("bytes").and_then(Value::as_array) {
                    let bytes: Vec<u8> = bytes_val
                        .iter()
                        .filter_map(Value::as_u64)
                        .map(|b| b as u8)
                        .collect();
                    accumulated.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
        }

        let queries = accumulated.matches("\x1b[6n").count();
        while cursor_reports < queries {
            runtime.handle(Request {
                protocol: 1,
                token: token.to_string(),
                op: "pty.write".to_string(),
                params: json!({ "target": target, "text": "\x1b[1;1R" }),
            })?;
            cursor_reports += 1;
        }
        if accumulated.contains(needle) {
            return Ok(accumulated);
        }
        if res.get("exited").and_then(Value::as_bool).unwrap_or(false) {
            break;
        }
    }

    if accumulated.contains(needle) {
        Ok(accumulated)
    } else {
        Err(format!(
            "Timed out waiting for '{needle}'. Accumulated output: {accumulated}"
        ))
    }
}

fn initialize_terminal(runtime: &Runtime, token: &str, target: &Value, cursor: &mut String) {
    if cfg!(windows) {
        read_until(
            runtime,
            token,
            target,
            cursor,
            "\x1b[6n",
            Duration::from_secs(5),
        )
        .expect("ConPTY cursor query must be answered before sending commands");
    }
}

#[test]
fn ssh_process_survival_project_root_decoupled() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-decouple");

    // Project registration outside runtime.root must succeed
    let reg = register_project(&runtime, "tok-decouple", "proj-ext", project_dir.path()).unwrap();
    assert_eq!(reg["projectId"], "proj-ext");

    // Re-registering with identical path is idempotent
    let reg_again =
        register_project(&runtime, "tok-decouple", "proj-ext", project_dir.path()).unwrap();
    assert_eq!(reg_again["projectId"], "proj-ext");

    // Re-registering with a different path produces REQUEST_CONFLICT
    let other_dir = tempfile::tempdir().unwrap();
    let conflict = register_project(&runtime, "tok-decouple", "proj-ext", other_dir.path());
    assert!(conflict.is_err());
    assert!(conflict.unwrap_err().contains("REQUEST_CONFLICT"));

    // Registering a non-existent path fails
    let fake_path = project_dir.path().join("does_not_exist");
    let non_exist = register_project(&runtime, "tok-decouple", "fake", &fake_path);
    assert!(non_exist.is_err());

    // Spawning a PTY in registered external project succeeds
    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-decouple".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "proj-ext",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    assert!(spawn["pid"].as_u64().is_some());

    // CWD escaping project root to existing parent directory is rejected
    let escape_spawn = runtime.handle(Request {
        protocol: 1,
        token: "tok-decouple".to_string(),
        op: "pty.spawn".to_string(),
        params: json!({
            "projectId": "proj-ext",
            "worktree": "..",
        }),
    });
    assert!(escape_spawn.is_err());
    assert!(escape_spawn.unwrap_err().contains("FORBIDDEN"));

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-decouple".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_spawn_dedupe_and_conflict() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-dedupe");
    register_project(&runtime, "tok-dedupe", "p-dedupe", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn_params = json!({
        "clientRequestId": "req-spawn-12345",
        "projectId": "p-dedupe",
        "program": prog,
        "args": args,
        "cols": 90,
        "rows": 30,
    });

    let spawn1 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dedupe".to_string(),
            op: "pty.spawn".to_string(),
            params: spawn_params.clone(),
        })
        .unwrap();
    let target1 = spawn1["target"].clone();
    let pid1 = spawn1["pid"].as_u64().unwrap();

    // Replay exact same request with same clientRequestId -> returns same target and pid without spawning new process
    let spawn2 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dedupe".to_string(),
            op: "pty.spawn".to_string(),
            params: spawn_params.clone(),
        })
        .unwrap();
    assert_eq!(spawn2["target"], target1);
    assert_eq!(spawn2["pid"].as_u64().unwrap(), pid1);

    // Mismatched request with same clientRequestId -> rejected with REQUEST_CONFLICT
    let conflict_params = json!({
        "clientRequestId": "req-spawn-12345",
        "projectId": "p-dedupe",
        "program": prog,
        "args": args,
        "cols": 120, // different cols!
        "rows": 30,
    });
    let spawn_conflict = runtime.handle(Request {
        protocol: 1,
        token: "tok-dedupe".to_string(),
        op: "pty.spawn".to_string(),
        params: conflict_params,
    });
    assert!(spawn_conflict.is_err());
    assert!(spawn_conflict.unwrap_err().contains("REQUEST_CONFLICT"));

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dedupe".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target1 }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_retains_pid_and_memory_state() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-survival");
    register_project(&runtime, "tok-survival", "p-surv", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-surv",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    let initial_pid = spawn["pid"].as_u64().unwrap();
    let mut cursor = "0".to_string();
    initialize_terminal(&runtime, "tok-survival", &target, &mut cursor);

    // Set unique memory state in the running process
    let nonce = format!("NONCE_{}", uuid::Uuid::new_v4().simple());
    let set_cmd = env_set("SURVIVAL_NONCE", &nonce);
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": set_cmd,
            }),
        })
        .unwrap();

    // Verify first command execution
    let echo_cmd = env_echo("SURVIVAL_NONCE");
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": echo_cmd,
            }),
        })
        .unwrap();

    let expected1 = format!("SURVIVAL_NONCE={nonce}");
    let out1 = read_until(
        &runtime,
        "tok-survival",
        &target,
        &mut cursor,
        &expected1,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out1.contains(&expected1));

    // SIMULATE RECONNECT: Client lost connection and reconnects with only the TargetRef and cursor.
    // Querying the process again without restarting or spawning must preserve the in-memory variable and PID!
    let probe_marker = format!("PROBE_{}", uuid::Uuid::new_v4().simple());
    let probe_cmd = if cfg!(windows) {
        format!("echo PROBE=%SURVIVAL_NONCE%:{probe_marker}\r\n")
    } else {
        format!("echo \"PROBE=$SURVIVAL_NONCE:{probe_marker}\"\n")
    };
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": probe_cmd,
            }),
        })
        .unwrap();

    let expected_probe = format!("PROBE={nonce}:{probe_marker}");
    let out2 = read_until(
        &runtime,
        "tok-survival",
        &target,
        &mut cursor,
        &expected_probe,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out2.contains(&expected_probe));

    // Describe session to verify PID remained exactly identical
    let desc = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.describe".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
    assert_eq!(desc["pid"].as_u64().unwrap(), initial_pid);
    assert_eq!(desc["target"], target);
    assert!(!desc["exited"].as_bool().unwrap());

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-survival".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_binary_safe_base64_io() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-binary");
    register_project(&runtime, "tok-binary", "p-bin", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-binary".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-bin",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    let mut cursor = "0".to_string();
    initialize_terminal(&runtime, "tok-binary", &target, &mut cursor);

    // 1. Write via base64 data
    let marker_b64 = "FERRYX_B64_SENTINEL_771";
    let cmd_str = format!(
        "echo {marker_b64}{}",
        if cfg!(windows) { "\r\n" } else { "\n" }
    );
    let cmd_b64 = BASE64_STANDARD.encode(cmd_str.as_bytes());

    let write_res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-binary".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "data": cmd_b64,
            }),
        })
        .unwrap();
    assert_eq!(write_res["accepted"], true);

    // Read and verify chunks contain base64 `data`
    let out = read_until(
        &runtime,
        "tok-binary",
        &target,
        &mut cursor,
        marker_b64,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out.contains(marker_b64));

    // 2. Write via existing text parameter for backward compatibility
    let marker_txt = "FERRYX_TXT_SENTINEL_882";
    let text_cmd = format!("echo {marker_txt}\n");
    let write_txt_res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-binary".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": text_cmd,
            }),
        })
        .unwrap();
    assert_eq!(write_txt_res["accepted"], true);

    let out2 = read_until(
        &runtime,
        "tok-binary",
        &target,
        &mut cursor,
        marker_txt,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out2.contains(marker_txt));

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-binary".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_canonical_decimal_cursor() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-cursor");
    register_project(&runtime, "tok-cursor", "p-cur", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-cursor".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-cur",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();

    // Valid canonical decimal cursor "0"
    let read_valid = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-cursor".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "cursor": "0",
                "waitMs": 0,
            }),
        })
        .unwrap();
    // cursor field in response must be a string (canonical decimal u64)
    assert!(read_valid["cursor"].is_string());

    // Invalid non-canonical cursor with leading zeros is rejected
    let non_canon = runtime.handle(Request {
        protocol: 1,
        token: "tok-cursor".to_string(),
        op: "pty.read".to_string(),
        params: json!({
            "target": target,
            "cursor": "007",
            "waitMs": 0,
        }),
    });
    assert!(non_canon.is_err());
    assert!(non_canon.unwrap_err().contains("INVALID_REQUEST"));

    // Invalid string (letters) is rejected
    let bad_str = runtime.handle(Request {
        protocol: 1,
        token: "tok-cursor".to_string(),
        op: "pty.read".to_string(),
        params: json!({
            "target": target,
            "cursor": "abc",
            "waitMs": 0,
        }),
    });
    assert!(bad_str.is_err());
    assert!(bad_str.unwrap_err().contains("INVALID_REQUEST"));

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-cursor".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_frame_bounded_below_1mib() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-frame");
    register_project(&runtime, "tok-frame", "p-frame", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-frame".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-frame",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();

    // Generate output burst
    let gen_cmd = if cfg!(windows) {
        "for /L %i in (1,1,200) do @echo LINE_%i_BURST_OUTPUT_PADDING_1234567890_ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n"
    } else {
        "for i in $(seq 1 300); do echo \"LINE_${i}_BURST_OUTPUT_PADDING_1234567890_ABCDEFGHIJKLMNOPQRSTUVWXYZ\"; done\n"
    };
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-frame".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": gen_cmd,
            }),
        })
        .unwrap();

    // Read frame
    let res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-frame".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "cursor": "0",
                "waitMs": 1000,
            }),
        })
        .unwrap();

    // Response serialized to JSON must be strictly below 1 MiB
    let json_bytes = serde_json::to_vec(&res).unwrap();
    assert!(
        json_bytes.len() < MAX_FRAME,
        "Serialized frame {} exceeds MAX_FRAME {}",
        json_bytes.len(),
        MAX_FRAME
    );
    assert!(res["cursor"].is_string());

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-frame".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_ring_gap_detection() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-gap");
    register_project(&runtime, "tok-gap", "p-gap", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-gap".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-gap",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();

    // Initial read from 0 has no gap
    let res1 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-gap".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "cursor": "0",
                "waitMs": 0,
            }),
        })
        .unwrap();
    assert_eq!(res1["gap"], false);

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-gap".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_default_platform_login_shell() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-login-shell");
    register_project(&runtime, "tok-login-shell", "p-shell", project_dir.path()).unwrap();

    // Spawn without specifying program or args -> defaults to platform login shell
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-login-shell".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-shell",
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    assert!(spawn["pid"].as_u64().is_some());
    let mut cursor = "0".to_string();
    initialize_terminal(&runtime, "tok-login-shell", &target, &mut cursor);

    let sentinel = "LOGIN_SHELL_CONFIRMED_OK";
    let cmd = format!(
        "echo {sentinel}{}",
        if cfg!(windows) { "\r\n" } else { "\n" }
    );
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-login-shell".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": cmd,
            }),
        })
        .unwrap();

    let out = read_until(
        &runtime,
        "tok-login-shell",
        &target,
        &mut cursor,
        sentinel,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out.contains(sentinel));

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-login-shell".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_respects_cols_rows() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-size");
    register_project(&runtime, "tok-size", "p-size", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-size".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-size",
                "program": prog,
                "args": args,
                "cols": 132,
                "rows": 43,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();

    // Describe session
    let desc = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-size".to_string(),
            op: "pty.describe".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
    assert_eq!(desc["cols"], 132);
    assert_eq!(desc["rows"], 43);

    // Resize
    let resize = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-size".to_string(),
            op: "pty.resize".to_string(),
            params: json!({
                "target": target,
                "cols": 160,
                "rows": 50,
            }),
        })
        .unwrap();
    assert!(resize.is_object());

    // Invalid dimensions rejected
    let bad_resize = runtime.handle(Request {
        protocol: 1,
        token: "tok-size".to_string(),
        op: "pty.resize".to_string(),
        params: json!({
            "target": target,
            "cols": 0,
            "rows": 50,
        }),
    });
    assert!(bad_resize.is_err());

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-size".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_env_retains_session_id_without_fabrication() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-env");
    register_project(&runtime, "tok-env", "p-env", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-env",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap();
    let mut cursor = "0".to_string();
    initialize_terminal(&runtime, "tok-env", &target, &mut cursor);

    // Check environment: FERRYX_SESSION_ID must match backendSessionId, and no agent provider IDs are fabricated
    let probe_cmd = if cfg!(windows) {
        "echo FERRYX_ID=%FERRYX_SESSION_ID%\r\nif defined CODEX_SESSION_ID (echo CODEX_ID=%CODEX_SESSION_ID%) else (echo CODEX_ID=NONE)\r\nif defined CLAUDE_SESSION_ID (echo CLAUDE_ID=%CLAUDE_SESSION_ID%) else (echo CLAUDE_ID=NONE)\r\n".to_string()
    } else {
        "echo \"FERRYX_ID=$FERRYX_SESSION_ID\"; echo \"CODEX_ID=${CODEX_SESSION_ID:-NONE}\"; echo \"CLAUDE_ID=${CLAUDE_SESSION_ID:-NONE}\"\n".to_string()
    };

    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env".to_string(),
            op: "pty.write".to_string(),
            params: json!({
                "target": target,
                "text": probe_cmd,
            }),
        })
        .unwrap();

    let expected_ferryx = format!("FERRYX_ID={session_id}");
    let out = read_until(
        &runtime,
        "tok-env",
        &target,
        &mut cursor,
        "CLAUDE_ID=NONE",
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(out.contains(&expected_ferryx));
    assert!(
        out.contains("CODEX_ID=NONE"),
        "Agent provider ID CODEX_SESSION_ID must not be fabricated"
    );
    assert!(
        out.contains("CLAUDE_ID=NONE"),
        "Agent provider ID CLAUDE_SESSION_ID must not be fabricated"
    );

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_reconnect_safety_auth_and_target_validation() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-auth");
    register_project(&runtime, "tok-auth", "p-auth", project_dir.path()).unwrap();

    // Unauthorized request with invalid token
    let bad_auth = runtime.handle(Request {
        protocol: 1,
        token: "wrong-token".to_string(),
        op: "handshake".to_string(),
        params: json!({}),
    });
    assert_eq!(bad_auth.unwrap_err(), "UNAUTHORIZED");

    // Unsupported protocol version
    let bad_proto = runtime.handle(Request {
        protocol: 99,
        token: "tok-auth".to_string(),
        op: "handshake".to_string(),
        params: json!({}),
    });
    assert!(bad_proto.is_err());
    assert!(bad_proto.unwrap_err().contains("UNSUPPORTED"));

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-auth".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-auth",
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let valid_target = spawn["target"].clone();

    // Target expired: wrong hostId
    let mut bad_host_target = valid_target.clone();
    bad_host_target["hostId"] = json!("wrong-host");
    let res_host = runtime.handle(Request {
        protocol: 1,
        token: "tok-auth".to_string(),
        op: "pty.read".to_string(),
        params: json!({ "target": bad_host_target, "waitMs": 0 }),
    });
    assert_eq!(res_host.unwrap_err(), "TARGET_EXPIRED");

    // Target expired: wrong epoch
    let mut bad_epoch_target = valid_target.clone();
    bad_epoch_target["epoch"] = json!("999999999999");
    let res_epoch = runtime.handle(Request {
        protocol: 1,
        token: "tok-auth".to_string(),
        op: "pty.read".to_string(),
        params: json!({ "target": bad_epoch_target, "waitMs": 0 }),
    });
    assert_eq!(res_epoch.unwrap_err(), "TARGET_EXPIRED");

    // Target expired: wrong ownerId
    let mut bad_owner_target = valid_target.clone();
    bad_owner_target["ownerId"] = json!("wrong-owner");
    let res_owner = runtime.handle(Request {
        protocol: 1,
        token: "tok-auth".to_string(),
        op: "pty.read".to_string(),
        params: json!({ "target": bad_owner_target, "waitMs": 0 }),
    });
    assert_eq!(res_owner.unwrap_err(), "TARGET_EXPIRED");

    // Session not found: valid host/owner/epoch but unknown backendSessionId
    let mut not_found_target = valid_target.clone();
    not_found_target["backendSessionId"] = json!(uuid::Uuid::new_v4().to_string());
    let res_not_found = runtime.handle(Request {
        protocol: 1,
        token: "tok-auth".to_string(),
        op: "pty.read".to_string(),
        params: json!({ "target": not_found_target, "waitMs": 0 }),
    });
    assert_eq!(res_not_found.unwrap_err(), "NOT_FOUND");

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-auth".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": valid_target }),
        })
        .unwrap();
}

#[test]
fn ssh_reconnect_safety_rejects_shutdown_op() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-shutdown");

    for bad_op in ["shutdown", "Shutdown", "daemon.stop", "exit", "quit"] {
        let res = runtime.handle(Request {
            protocol: 1,
            token: "tok-shutdown".to_string(),
            op: bad_op.to_string(),
            params: json!({}),
        });
        assert!(res.is_err(), "Operation '{bad_op}' must be rejected");
        assert!(res.unwrap_err().contains("UNSUPPORTED"));
    }
}

#[test]
fn ssh_reconnect_safety_target_describe_and_list_metadata() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-meta");
    register_project(&runtime, "tok-meta", "p-meta", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-meta".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-meta",
                "program": prog,
                "args": args,
                "cols": 100,
                "rows": 35,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    let pid = spawn["pid"].as_u64().unwrap();

    // pty.describe exposes target, pid, cwd, cols, rows, cursor, exited
    let desc = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-meta".to_string(),
            op: "pty.describe".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
    assert_eq!(desc["target"], target);
    assert_eq!(desc["pid"].as_u64().unwrap(), pid);
    assert_eq!(desc["cols"], 100);
    assert_eq!(desc["rows"], 35);
    assert!(desc["cursor"].is_string());
    assert_eq!(desc["exited"], false);

    // pty.list includes this session with metadata
    let list = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-meta".to_string(),
            op: "pty.list".to_string(),
            params: json!({}),
        })
        .unwrap();
    let arr = list.as_array().unwrap();
    let item = arr
        .iter()
        .find(|s| s["target"] == target)
        .expect("session must be in list");
    assert_eq!(item["pid"].as_u64().unwrap(), pid);
    assert_eq!(item["cols"], 100);
    assert_eq!(item["rows"], 35);

    // Cleanup
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-meta".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": target }),
        })
        .unwrap();
}

#[test]
fn ssh_process_survival_concurrent_spawn_atomic_reservation() {
    struct PauseSeam {
        released: Mutex<bool>,
        cv: Condvar,
        entered: std::sync::atomic::AtomicUsize,
        ready_tx: std::sync::mpsc::Sender<()>,
    }

    impl PauseSeam {
        fn on_seam(&self) {
            let prev = self
                .entered
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if prev == 0 {
                self.ready_tx.send(()).expect("failed to signal ready_tx");
                let mut lock = self.released.lock().unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                while !*lock {
                    let now = Instant::now();
                    assert!(
                        now < deadline,
                        "PauseSeam timed out: release was not signaled within 5s"
                    );
                    let (new_lock, timeout_res) =
                        self.cv.wait_timeout(lock, deadline - now).unwrap();
                    lock = new_lock;
                    if timeout_res.timed_out() && !*lock {
                        panic!("PauseSeam timed out: release was not signaled within 5s");
                    }
                }
            }
        }

        fn release(&self) {
            let mut lock = self.released.lock().unwrap();
            *lock = true;
            self.cv.notify_all();
        }
    }

    struct SeamReleaseGuard(Arc<PauseSeam>);
    impl Drop for SeamReleaseGuard {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let r_path = runtime_dir.path().to_path_buf();
    let p_path = project_dir.path().to_path_buf();
    eprintln!(
        "QA_CLEANUP_TRACKING test=concurrent_spawn runtime_dir={:?} project_dir={:?}",
        r_path, p_path
    );

    let runtime = Arc::new(make_runtime(&runtime_dir, "tok-concurrent"));
    register_project(&runtime, "tok-concurrent", "p-race", project_dir.path()).unwrap();

    let (prog, args) = shell_program_and_args();
    let client_req_id = "req-concurrent-dedupe-1";

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let seam = Arc::new(PauseSeam {
        released: Mutex::new(false),
        cv: Condvar::new(),
        entered: std::sync::atomic::AtomicUsize::new(0),
        ready_tx,
    });

    let _guard = SeamReleaseGuard(seam.clone());

    // Install deterministic test seam hook
    {
        let seam_clone = seam.clone();
        runtime.set_spawn_hook(Some(Arc::new(move |req_id: &str| {
            if req_id == client_req_id {
                seam_clone.on_seam();
            }
        })));
    }

    let spawn_params_base = json!({
        "clientRequestId": client_req_id,
        "projectId": "p-race",
        "program": prog,
        "args": args,
        "cols": 90,
        "rows": 30,
    });

    // Spawn Thread 1
    let rt1 = runtime.clone();
    let params1 = spawn_params_base.clone();
    let h1 = std::thread::spawn(move || {
        rt1.handle(Request {
            protocol: 1,
            token: "tok-concurrent".to_string(),
            op: "pty.spawn".to_string(),
            params: params1,
        })
    });

    // Bounded wait: Thread 1 must reach the reservation seam within 5s
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("Thread 1 did not reach reservation seam within 5s");

    // Now while Thread 1 is in-progress:
    // 1. Thread 3 calls with SAME clientRequestId but DIFFERENT parameters (mismatched cols)
    let mut conflict_params = spawn_params_base.clone();
    conflict_params["cols"] = json!(140);
    let conflict_res = runtime.handle(Request {
        protocol: 1,
        token: "tok-concurrent".to_string(),
        op: "pty.spawn".to_string(),
        params: conflict_params,
    });
    assert!(
        conflict_res.is_err(),
        "Concurrent mismatched spawn must be rejected with REQUEST_CONFLICT"
    );
    assert!(conflict_res.unwrap_err().contains("REQUEST_CONFLICT"));

    // 2. Thread 2 calls with SAME clientRequestId and MATCHING parameters concurrently
    let rt2 = runtime.clone();
    let params2 = spawn_params_base.clone();
    let h2 = std::thread::spawn(move || {
        rt2.handle(Request {
            protocol: 1,
            token: "tok-concurrent".to_string(),
            op: "pty.spawn".to_string(),
            params: params2,
        })
    });

    // Explicitly release Thread 1 to complete its spawn
    seam.release();

    let res1 = h1
        .join()
        .expect("Thread 1 panicked")
        .expect("Thread 1 spawn failed");
    let res2 = h2
        .join()
        .expect("Thread 2 panicked")
        .expect("Thread 2 spawn failed");

    // Both must report the exact same target and real PID
    assert_eq!(res1["target"], res2["target"]);
    assert_eq!(res1["pid"], res2["pid"]);

    // pty.list must show EXACTLY 1 active session, proving no duplicate PTY was created
    let list = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-concurrent".to_string(),
            op: "pty.list".to_string(),
            params: json!({}),
        })
        .unwrap();
    let arr = list.as_array().unwrap();
    assert_eq!(
        arr.len(),
        1,
        "Concurrent spawn must produce exactly one PTY session in pty.list, got {}",
        arr.len()
    );

    // Cleanup
    runtime.set_spawn_hook(None);
    runtime
        .handle(Request {
            protocol: 1,
            token: "tok-concurrent".to_string(),
            op: "pty.stop".to_string(),
            params: json!({ "target": res1["target"] }),
        })
        .unwrap();

    drop(runtime);
    drop(runtime_dir);
    drop(project_dir);
    eprintln!(
        "QA_CLEANUP_RECEIPT test=concurrent_spawn removed runtime_dir={:?} exists={} project_dir={:?} exists={}",
        r_path,
        r_path.exists(),
        p_path,
        p_path.exists()
    );
}

#[test]
fn helper_handshake_includes_helper_version() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-version-test");
    let resp = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-version-test".to_string(),
            op: "handshake".to_string(),
            params: json!({}),
        })
        .unwrap();

    assert_eq!(resp["helperVersion"], super::super::process::HELPER_VERSION);
}

#[test]
fn helper_process_version_flag_prints_version() {
    let mut out = Vec::new();
    let res = super::super::process::run_with_io(vec!["--version".to_string()], &mut out);
    assert!(res.is_ok());
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.trim(), super::super::process::HELPER_VERSION);
}

#[test]
fn helper_process_capabilities_flag_reports_machine_json() {
    let mut out = Vec::new();
    let res = super::super::process::run_with_io(vec!["--capabilities".to_string()], &mut out);
    assert!(res.is_ok());
    let report: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        report,
        json!({
            "protocol": 1,
            "helperVersion": super::super::process::HELPER_VERSION,
            "capabilities": super::super::process::HELPER_CAPABILITIES,
            "runtimeConditional": [super::super::process::CONDITIONAL_AGENT_STATE],
        })
    );
}

#[test]
fn helper_handshake_capabilities_follow_shared_constant_list() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-cap-guard");
    let resp = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-cap-guard".to_string(),
            op: "handshake".to_string(),
            params: json!({}),
        })
        .unwrap();

    let bound = runtime.agent_state_server().is_some();
    let expected: Vec<String> = super::super::process::HELPER_CAPABILITIES
        .iter()
        .filter(|capability| {
            bound || **capability != super::super::process::CONDITIONAL_AGENT_STATE
        })
        .map(|capability| (*capability).to_string())
        .collect();
    assert_eq!(resp["capabilities"], json!(expected));
    assert_eq!(
        resp["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some(super::super::process::CONDITIONAL_AGENT_STATE)),
        bound
    );
}

#[test]
fn helper_dag_inventory_and_poll_operations() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-dag-test");

    let reg = register_project(&runtime, "tok-dag-test", "proj-1", project_dir.path()).unwrap();
    assert_eq!(reg["projectId"], "proj-1");

    let initial = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dag-test".to_string(),
            op: "dag.inventory".to_string(),
            params: json!({ "projectId": "proj-1" }),
        })
        .unwrap();
    assert_eq!(initial["runs"].as_array().unwrap().len(), 0);

    let runs_dir = project_dir.path().join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs_dir).unwrap();
    let sample_run = json!({
        "runId": "dag-run-abc",
        "runKey": "key-1",
        "name": "sample",
        "status": "running",
        "nodes": [],
        "edges": [],
        "waves": [],
        "counts": {}
    });
    std::fs::write(
        runs_dir.join("run-1.json"),
        serde_json::to_string(&sample_run).unwrap(),
    )
    .unwrap();

    let inv = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dag-test".to_string(),
            op: "dag.inventory".to_string(),
            params: json!({ "projectId": "proj-1" }),
        })
        .unwrap();
    let runs = inv["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["runId"], "dag-run-abc");

    let polled = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-dag-test".to_string(),
            op: "dag.poll".to_string(),
            params: json!({ "projectId": "proj-1", "afterMtimeMs": 0 }),
        })
        .unwrap();
    assert_eq!(polled["runs"].as_array().unwrap().len(), 1);
    assert!(polled["maxMtimeMs"].as_u64().unwrap() > 0);
}

#[test]
fn helper_dag_poll_delivers_changed_known_run_at_same_mtime() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "dag-same-time");
    register_project(&runtime, "dag-same-time", "project", project_dir.path()).unwrap();
    let runs = project_dir.path().join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs).unwrap();
    let path = runs.join("run.json");
    std::fs::write(&path, r#"{"runId":"same-run","status":"running"}"#).unwrap();
    let original_time = std::fs::metadata(&path).unwrap().modified().unwrap();
    let cursor = original_time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    std::fs::write(&path, r#"{"runId":"same-run","status":"success"}"#).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(original_time))
        .unwrap();
    let result = runtime
        .handle(Request {
            protocol: 1,
            token: "dag-same-time".to_string(),
            op: "dag.poll".to_string(),
            params: json!({"projectId":"project", "afterMtimeMs":cursor,
            "knownRuns":[{"runId":"same-run","status":"running"}]}),
        })
        .unwrap();
    assert_eq!(result["runs"].as_array().unwrap().len(), 1);
    assert_eq!(result["runs"][0]["status"], "success");
}

#[test]
fn helper_dag_rejects_unregistered_directory_as_project_id() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "dag-unregistered");
    for op in ["dag.inventory", "dag.poll"] {
        let result = runtime.handle(Request {
            protocol: 1,
            token: "dag-unregistered".to_string(),
            op: op.to_string(),
            params: json!({"projectId":project_dir.path().to_string_lossy()}),
        });
        assert_eq!(result.unwrap_err(), "NOT_FOUND", "{op}");
    }
}

#[test]
fn helper_dag_poll_suppresses_unchanged_known_snapshot() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "dag-dedup");
    register_project(&runtime, "dag-dedup", "project", project_dir.path()).unwrap();
    let runs = project_dir.path().join(".omo/senpi-task/dag/runs");
    std::fs::create_dir_all(&runs).unwrap();
    let snapshot = json!({"runId":"same-run", "status":"running"});
    std::fs::write(
        runs.join("run.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let result = runtime
        .handle(Request {
            protocol: 1,
            token: "dag-dedup".to_string(),
            op: "dag.poll".to_string(),
            params: json!({"projectId":"project", "afterMtimeMs":0, "knownRuns":[snapshot]}),
        })
        .unwrap();
    assert!(result["runs"].as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// SSH Input Isolation Regression Tests (TDD - Writer Lock Contention)
// ---------------------------------------------------------------------------

struct ControlledBlockingWriter {
    inner: Box<dyn Write + Send>,
    started_tx: std::sync::mpsc::SyncSender<()>,
    release_rx: Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl Write for ControlledBlockingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.started_tx.try_send(());
        if !self.released.load(std::sync::atomic::Ordering::SeqCst) {
            if let Ok(rx) = self.release_rx.lock() {
                if !self.released.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = rx.recv();
                    self.released.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct ReleaseOnDrop {
    tx: Option<std::sync::mpsc::SyncSender<()>>,
    released: Arc<std::sync::atomic::AtomicBool>,
}

impl ReleaseOnDrop {
    fn new(tx: std::sync::mpsc::SyncSender<()>, released: Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self { tx: Some(tx), released }
    }

    fn release(&mut self) {
        self.released.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.release();
    }
}

fn swap_session_writer(
    runtime: &Runtime,
    backend_session_id: &str,
    new_writer: Box<dyn Write + Send>,
) -> Box<dyn Write + Send> {
    let session = {
        let sessions = runtime.sessions.lock().unwrap();
        sessions
            .get(backend_session_id)
            .cloned()
            .expect("session must exist to swap writer")
    };
    let mut writer_guard = session.writer.lock().unwrap();
    std::mem::replace(&mut *writer_guard, new_writer)
}

fn setup_session(runtime: &Runtime, token: &str, project_id: &str) -> (Value, String) {
    let (prog, args) = shell_program_and_args();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: token.to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": project_id,
                "program": prog,
                "args": args,
            }),
        })
        .unwrap();
    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    (target, session_id)
}

fn attach_blocked_writer(
    runtime: &Runtime,
    session_id: &str,
) -> (
    Box<dyn Write + Send>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::SyncSender<()>,
    Arc<std::sync::atomic::AtomicBool>,
) {
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let orig_writer = swap_session_writer(
        runtime,
        session_id,
        Box::new(ControlledBlockingWriter {
            inner: Box::new(std::io::sink()),
            started_tx,
            release_rx: Arc::new(Mutex::new(release_rx)),
            released: released.clone(),
        }),
    );
    (orig_writer, started_rx, release_tx, released)
}

#[test]
fn ssh_input_isolation_blocked_writer_does_not_stall_unrelated_session() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-iso-unrel");
    register_project(&runtime, "tok-iso-unrel", "p-iso", project_dir.path()).unwrap();

    let (target_a, session_id_a) = setup_session(&runtime, "tok-iso-unrel", "p-iso");
    let (target_b, _) = setup_session(&runtime, "tok-iso-unrel", "p-iso");

    let (orig_writer_a, started_rx, release_tx, released) =
        attach_blocked_writer(&runtime, &session_id_a);

    std::thread::scope(|s| {
        let mut release_guard = ReleaseOnDrop::new(release_tx, released);

        let rt = &runtime;
        let target_a_writer = target_a.clone();
        let writer_thread = s.spawn(move || {
            rt.handle(Request {
                protocol: 1,
                token: "tok-iso-unrel".to_string(),
                op: "pty.write".to_string(),
                params: json!({ "target": target_a_writer, "text": "blocked_input\n" }),
            })
        });

        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Session A writer must enter write()");

        // Verify describe, read, and write on unrelated Session B complete without stalling
        let (describe_tx, describe_rx) = std::sync::mpsc::sync_channel(1);
        let target_b_desc = target_b.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-unrel".to_string(),
                op: "pty.describe".to_string(),
                params: json!({ "target": target_b_desc }),
            });
            let _ = describe_tx.send(res);
        });
        assert!(describe_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Unrelated Session B describe must not stall while Session A writer is blocked")
            .is_ok());

        let (read_b_tx, read_b_rx) = std::sync::mpsc::sync_channel(1);
        let target_b_read = target_b.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-unrel".to_string(),
                op: "pty.read".to_string(),
                params: json!({ "target": target_b_read, "waitMs": 0 }),
            });
            let _ = read_b_tx.send(res);
        });
        assert!(read_b_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Unrelated Session B read must not stall while Session A writer is blocked")
            .is_ok());

        let (write_b_tx, write_b_rx) = std::sync::mpsc::sync_channel(1);
        let target_b_write = target_b.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-unrel".to_string(),
                op: "pty.write".to_string(),
                params: json!({ "target": target_b_write, "text": "echo hello\n" }),
            });
            let _ = write_b_tx.send(res);
        });
        assert!(write_b_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Unrelated Session B write must not stall while Session A writer is blocked")
            .is_ok());

        release_guard.release();
        assert!(writer_thread.join().unwrap().is_ok());
    });

    swap_session_writer(&runtime, &session_id_a, orig_writer_a);
    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-iso-unrel".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target_a }),
    });
    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-iso-unrel".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target_b }),
    });
}

#[test]
fn ssh_input_isolation_blocked_writer_does_not_stall_same_session_describe_or_read() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-iso-same");
    register_project(&runtime, "tok-iso-same", "p-same", project_dir.path()).unwrap();

    let (target_a, session_id_a) = setup_session(&runtime, "tok-iso-same", "p-same");
    let (orig_writer_a, started_rx, release_tx, released) =
        attach_blocked_writer(&runtime, &session_id_a);

    std::thread::scope(|s| {
        let mut release_guard = ReleaseOnDrop::new(release_tx, released);

        let rt = &runtime;
        let target_a_writer = target_a.clone();
        let writer_thread = s.spawn(move || {
            rt.handle(Request {
                protocol: 1,
                token: "tok-iso-same".to_string(),
                op: "pty.write".to_string(),
                params: json!({ "target": target_a_writer, "text": "blocked_input\n" }),
            })
        });

        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Session A writer must enter write()");

        // Verify same-session describe and read complete without stalling
        let (describe_tx, describe_rx) = std::sync::mpsc::sync_channel(1);
        let target_a_desc = target_a.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-same".to_string(),
                op: "pty.describe".to_string(),
                params: json!({ "target": target_a_desc }),
            });
            let _ = describe_tx.send(res);
        });
        assert!(describe_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Same session describe must not stall while writer is blocked")
            .is_ok());

        let (read_tx, read_rx) = std::sync::mpsc::sync_channel(1);
        let target_a_read = target_a.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-same".to_string(),
                op: "pty.read".to_string(),
                params: json!({ "target": target_a_read, "waitMs": 0 }),
            });
            let _ = read_tx.send(res);
        });
        assert!(read_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Same session read must not stall while writer is blocked")
            .is_ok());

        release_guard.release();
        assert!(writer_thread.join().unwrap().is_ok());
    });

    swap_session_writer(&runtime, &session_id_a, orig_writer_a);
    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-iso-same".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target_a }),
    });
}

#[test]
fn ssh_input_isolation_blocked_writer_does_not_stall_same_session_resize() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-iso-resize");
    register_project(&runtime, "tok-iso-resize", "p-resize", project_dir.path()).unwrap();

    let (target_a, session_id_a) = setup_session(&runtime, "tok-iso-resize", "p-resize");
    let (orig_writer_a, started_rx, release_tx, released) =
        attach_blocked_writer(&runtime, &session_id_a);

    std::thread::scope(|s| {
        let mut release_guard = ReleaseOnDrop::new(release_tx, released);

        let rt = &runtime;
        let target_a_writer = target_a.clone();
        let writer_thread = s.spawn(move || {
            rt.handle(Request {
                protocol: 1,
                token: "tok-iso-resize".to_string(),
                op: "pty.write".to_string(),
                params: json!({ "target": target_a_writer, "text": "blocked_input\n" }),
            })
        });

        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Session A writer must enter write()");

        // Verify same-session resize completes without stalling
        let (resize_tx, resize_rx) = std::sync::mpsc::sync_channel(1);
        let target_a_resize = target_a.clone();
        s.spawn(move || {
            let res = rt.handle(Request {
                protocol: 1,
                token: "tok-iso-resize".to_string(),
                op: "pty.resize".to_string(),
                params: json!({ "target": target_a_resize, "cols": 100, "rows": 30 }),
            });
            let _ = resize_tx.send(res);
        });

        let resize_res = resize_rx
            .recv_timeout(Duration::from_millis(300))
            .expect("Same session resize must not stall while writer is blocked");
        assert!(resize_res.is_ok(), "resize on same session must succeed");
        let resize_val = resize_res.unwrap();
        assert_eq!(resize_val["cols"], 100);
        assert_eq!(resize_val["rows"], 30);

        release_guard.release();
        assert!(writer_thread.join().unwrap().is_ok());
    });

    swap_session_writer(&runtime, &session_id_a, orig_writer_a);
    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-iso-resize".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target_a }),
    });
}

#[test]
fn ssh_agent_state_real_tcp_close_after_write_updates_state() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-tcp-write");
    register_project(&runtime, "tok-tcp-write", "p-tcp", project_dir.path()).unwrap();

    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-tcp-write".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({ "projectId": "p-tcp" }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": token,
        "detail": "indexing workspace",
    });
    stream.write_all(payload.to_string().as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    drop(stream);

    // Await the exact ingestion of the close-after-write report on the session's report
    // condvar: the report arrives over a real socket, so acceptance timing must never be
    // raced by the next pty.read.
    let server = runtime.agent_state_server().expect("agent state server");
    assert!(
        server.wait_for_agent_revision_for_test(&session_id, 1, Duration::from_secs(5)),
        "close-after-write report must be ingested as revision 1"
    );

    let res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-tcp-write".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "agentAfterRevision": "0",
                "waitMs": 0,
            }),
        })
        .unwrap();
    let state_obj = res.get("agentState").unwrap();
    assert_eq!(state_obj["revision"], "1");
    assert_eq!(state_obj["state"], "working");
    assert_eq!(state_obj["agent"], "omo");
    assert_eq!(state_obj["detail"], "indexing workspace");
    assert!(state_obj.get("token").is_none());

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-tcp-write".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });
}

#[test]
fn ssh_agent_state_auth_and_limits_rejected() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-auth-limits");
    register_project(&runtime, "tok-auth-limits", "p-auth", project_dir.path()).unwrap();

    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-auth-limits".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({ "projectId": "p-auth" }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    let mut s1 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let bad_token_payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": "wrong-token-value",
    });
    s1.write_all(bad_token_payload.to_string().as_bytes()).unwrap();
    s1.write_all(b"\n").unwrap();
    drop(s1);

    let mut s2 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let wrong_session_payload = json!({
        "type": "agentState",
        "sessionId": "unknown-session-id",
        "state": "working",
        "agent": "omo",
        "token": token,
    });
    s2.write_all(wrong_session_payload.to_string().as_bytes()).unwrap();
    s2.write_all(b"\n").unwrap();
    drop(s2);

    let mut s3 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let invalid_state_payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "unknown_state",
        "agent": "omo",
        "token": token,
    });
    s3.write_all(invalid_state_payload.to_string().as_bytes()).unwrap();
    s3.write_all(b"\n").unwrap();
    drop(s3);

    let mut s4 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let wrong_type_payload = json!({
        "type": "otherType",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": token,
    });
    s4.write_all(wrong_type_payload.to_string().as_bytes()).unwrap();
    s4.write_all(b"\n").unwrap();
    drop(s4);

    let mut s5 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let oversized = vec![b'a'; 17 * 1024];
    let _ = s5.write_all(&oversized);
    drop(s5);

    let mut s6 = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let no_newline_payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": token,
    });
    s6.write_all(no_newline_payload.to_string().as_bytes()).unwrap();
    drop(s6);

    let res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-auth-limits".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "agentAfterRevision": "0",
                "waitMs": 50,
            }),
        })
        .unwrap();

    assert!(
        res.get("agentState").is_none(),
        "No agentState should be accepted from invalid reports"
    );

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-auth-limits".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });
}

/// Cross-platform silent child for agent-state barrier tests: it writes nothing and
/// never exits, so a blocking `pty.read` has exactly one possible wake source — an
/// agent-state revision change — and shell startup output can no longer release it.
fn silent_agent_state_probe() -> (String, Vec<String>) {
    if cfg!(windows) {
        let comspec = std::env::var("COMSPEC")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
        (
            comspec,
            vec![
                "/d".to_string(),
                "/s".to_string(),
                "/c".to_string(),
                "set /p x=".to_string(),
            ],
        )
    } else {
        (
            "/bin/sh".to_string(),
            vec!["-c".to_string(), "read x".to_string()],
        )
    }
}

/// Deterministically quiesces a PTY before a barrier read: consumes every buffered
/// byte, answers ConPTY cursor queries so conhost stops prompting, and returns once a
/// bounded wait observes no further output. No sleeps — every iteration is the helper's
/// own bounded condvar wait, so this can never mask a wake by waiting longer.
fn drain_pty_until_quiet(runtime: &Runtime, token: &str, target: &Value, cursor: &mut String) {
    for _ in 0..20 {
        let res = runtime
            .handle(Request {
                protocol: 1,
                token: token.to_string(),
                op: "pty.read".to_string(),
                params: json!({
                    "target": target,
                    "cursor": cursor.clone(),
                    "waitMs": 200,
                }),
            })
            .expect("pty.read during drain must succeed");
        if let Some(next) = res.get("cursor").and_then(Value::as_str) {
            *cursor = next.to_string();
        }
        let chunks = res
            .get("chunks")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if chunks.is_empty() {
            return;
        }
        let mut text = String::new();
        for chunk in &chunks {
            if let Some(b64) = chunk.get("data").and_then(Value::as_str) {
                if let Ok(bytes) = BASE64_STANDARD.decode(b64) {
                    text.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
        }
        for _ in 0..text.matches("\x1b[6n").count() {
            runtime
                .handle(Request {
                    protocol: 1,
                    token: token.to_string(),
                    op: "pty.write".to_string(),
                    params: json!({ "target": target, "text": "\x1b[1;1R" }),
                })
                .expect("answering a ConPTY cursor query must succeed");
        }
    }
}

#[test]
fn ssh_agent_state_idle_read_wake_wakes_blocked_reader() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-idle-wake");
    register_project(&runtime, "tok-idle-wake", "p-wake", project_dir.path()).unwrap();

    // Silent child: the PTY produces no output at all, so the blocking read below
    // cannot be satisfied by shell output instead of the report (the old flake).
    let (probe_program, probe_args) = silent_agent_state_probe();
    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-idle-wake".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({
                "projectId": "p-wake",
                "program": probe_program,
                "args": probe_args,
            }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    // Reach a drained, quiet boundary before arming the barrier read.
    let mut drained_cursor = "0".to_string();
    drain_pty_until_quiet(&runtime, "tok-idle-wake", &target, &mut drained_cursor);

    let rt = &runtime;
    let target_for_thread = target.clone();
    let cursor_for_thread = drained_cursor;

    std::thread::scope(|s| {
        let (started_tx, started_rx) = std::sync::mpsc::channel();

        let reader_handle = s.spawn(move || {
            started_tx.send(()).unwrap();
            let start = Instant::now();
            let res = rt
                .handle(Request {
                    protocol: 1,
                    token: "tok-idle-wake".to_string(),
                    op: "pty.read".to_string(),
                    params: json!({
                        "target": target_for_thread,
                        "cursor": cursor_for_thread,
                        "agentAfterRevision": "0",
                        "waitMs": 5000,
                    }),
                })
                .unwrap();
            (start.elapsed(), res)
        });

        // The report is a durable state change, so this ordering only needs the reader
        // to be running: the quiet PTY leaves no other wake source either way.
        started_rx.recv().unwrap();

        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let payload = json!({
            "type": "agentState",
            "sessionId": session_id,
            "state": "blocked",
            "agent": "omo",
            "token": token,
            "detail": "waiting for permission",
        });
        stream.write_all(payload.to_string().as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        drop(stream);

        let (elapsed, res) = reader_handle.join().unwrap();
        let state_obj = res.get("agentState").expect(
            "blocking pty.read on a quiet PTY must be released by the agent-state report",
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "Idle reader should wake via condvar well before 5s timeout, took: {elapsed:?}"
        );
        assert_eq!(state_obj["revision"], "1");
        assert_eq!(state_obj["state"], "blocked");
        assert_eq!(state_obj["detail"], "waiting for permission");
    });

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-idle-wake".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });
}

#[test]
fn ssh_agent_state_ack_wait_and_revision_validation() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-rev-val");
    register_project(&runtime, "tok-rev-val", "p-val", project_dir.path()).unwrap();

    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-rev-val".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({ "projectId": "p-val" }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    let err_res = runtime.handle(Request {
        protocol: 1,
        token: "tok-rev-val".to_string(),
        op: "pty.read".to_string(),
        params: json!({
            "target": target,
            "agentAfterRevision": "1",
            "waitMs": 0,
        }),
    });
    assert!(err_res.is_err());
    assert!(err_res.unwrap_err().contains("INVALID_REQUEST"));

    let err_non_canonical = runtime.handle(Request {
        protocol: 1,
        token: "tok-rev-val".to_string(),
        op: "pty.read".to_string(),
        params: json!({
            "target": target,
            "agentAfterRevision": "01",
            "waitMs": 0,
        }),
    });
    assert!(err_non_canonical.is_err());

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "idle",
        "agent": "omo",
        "token": token,
    });
    stream.write_all(payload.to_string().as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    drop(stream);

    // The report travels over a real TCP socket, so the accept loop may ingest it a park
    // interval later than the write. Await that exact ingestion on the session's report
    // condvar instead of retrying pty.read and racing the accept timing.
    let server = runtime.agent_state_server().expect("agent state server");
    assert!(
        server.wait_for_agent_revision_for_test(&session_id, 1, Duration::from_secs(5)),
        "valid report must be ingested as revision 1"
    );

    let rev1 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-rev-val".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "agentAfterRevision": "0",
                "waitMs": 0,
            }),
        })
        .unwrap();
    let rev1_state = rev1
        .get("agentState")
        .expect("ingested revision 1 must be returned to pty.read");
    assert_eq!(rev1_state["revision"], "1");
    assert_eq!(rev1_state["state"], "idle");

    let res_same_rev = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-rev-val".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "agentAfterRevision": "1",
                "waitMs": 20,
            }),
        })
        .unwrap();
    assert!(
        res_same_rev.get("agentState").is_none(),
        "agentState should not be included when not newer than requested revision"
    );

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-rev-val".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });
}

#[test]
fn ssh_agent_state_reconnect_snapshot_returns_latest_state() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-reconnect");
    register_project(&runtime, "tok-reconnect", "p-rec", project_dir.path()).unwrap();

    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-reconnect".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({ "projectId": "p-rec" }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": token,
        "providerSession": { "id": "test-provider-session" },
        "detail": "reconnect test detail",
    });
    stream.write_all(payload.to_string().as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    drop(stream);

    // Await the exact report ingestion rather than retrying reads against accept timing.
    let server = runtime.agent_state_server().expect("agent state server");
    assert!(
        server.wait_for_agent_revision_for_test(&session_id, 1, Duration::from_secs(5)),
        "reconnect report must be ingested as revision 1"
    );

    let res = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-reconnect".to_string(),
            op: "pty.read".to_string(),
            params: json!({
                "target": target,
                "agentAfterRevision": "0",
                "waitMs": 0,
            }),
        })
        .unwrap();
    let state = res
        .get("agentState")
        .expect("reconnect snapshot must return state")
        .clone();
    assert_eq!(state["revision"], "1");
    assert_eq!(state["state"], "working");
    assert_eq!(state["agent"], "omo");
    assert_eq!(state["providerSession"]["id"], "test-provider-session");
    assert_eq!(state["detail"], "reconnect test detail");

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-reconnect".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });
}

#[test]
fn ssh_agent_state_env_scrubbing_and_idempotency() {
    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-env-idemp");
    register_project(&runtime, "tok-env-idemp", "p-env", project_dir.path()).unwrap();

    let hs = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env-idemp".to_string(),
            op: "handshake".to_string(),
            params: json!({}),
        })
        .unwrap();

    assert_eq!(hs["protocol"], 1);
    let caps = hs["capabilities"].as_array().unwrap();
    assert!(caps.iter().any(|c| c == "agentStateV1"));
    assert!(caps.iter().any(|c| c == "sshHelperV1"));

    let spawn_params = json!({
        "clientRequestId": "req-idemp-1",
        "projectId": "p-env",
        "env": {
            "FERRYX_AGENT_STATE_SOCKET": "/tmp/evil.sock",
            "ferryx_agent_state_socket": "/tmp/evil2.sock",
            "FERRYX_SESSION_ID": "spoofed-session-id",
            "FERRYX_AGENT_STATE_PORT": "9999",
            "FERRYX_AGENT_STATE_TOKEN": "stolen-token",
            "CUSTOM_VAR": "custom_value",
        }
    });

    let spawn1 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env-idemp".to_string(),
            op: "pty.spawn".to_string(),
            params: spawn_params.clone(),
        })
        .unwrap();

    let target1 = spawn1["target"].clone();
    let pid1 = spawn1["pid"].as_u64().unwrap();
    let session_id = target1["backendSessionId"].as_str().unwrap();
    assert_ne!(session_id, "spoofed-session-id");

    let spawn2 = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-env-idemp".to_string(),
            op: "pty.spawn".to_string(),
            params: spawn_params,
        })
        .unwrap();

    assert_eq!(spawn2["target"], target1);
    assert_eq!(spawn2["pid"], pid1);

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-env-idemp".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target1 }),
    });
}

#[test]
fn ssh_agent_state_revoked_on_stop_and_rollback_on_failed_spawn() {
    use std::io::Write;
    use std::net::TcpStream;

    let runtime_dir = tempfile::tempdir().unwrap();
    let project_dir = tempfile::tempdir().unwrap();
    let runtime = make_runtime(&runtime_dir, "tok-rev-stop");
    register_project(&runtime, "tok-rev-stop", "p-rev", project_dir.path()).unwrap();

    let spawn = runtime
        .handle(Request {
            protocol: 1,
            token: "tok-rev-stop".to_string(),
            op: "pty.spawn".to_string(),
            params: json!({ "projectId": "p-rev" }),
        })
        .unwrap();

    let target = spawn["target"].clone();
    let session_id = target["backendSessionId"].as_str().unwrap().to_string();
    let port = runtime.agent_state_server().unwrap().port();
    let token = runtime.session_token(&session_id).unwrap();

    assert!(runtime.agent_state_server().unwrap().is_registered(&session_id));

    let _ = runtime.handle(Request {
        protocol: 1,
        token: "tok-rev-stop".to_string(),
        op: "pty.stop".to_string(),
        params: json!({ "target": target }),
    });

    assert!(!runtime.agent_state_server().unwrap().is_registered(&session_id));

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let payload = json!({
        "type": "agentState",
        "sessionId": session_id,
        "state": "working",
        "agent": "omo",
        "token": token,
    });
    stream.write_all(payload.to_string().as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    drop(stream);

    let failed_spawn = runtime.handle(Request {
        protocol: 1,
        token: "tok-rev-stop".to_string(),
        op: "pty.spawn".to_string(),
        params: json!({
            "projectId": "p-rev",
            "worktree": "../outside_project",
        }),
    });
    assert!(failed_spawn.is_err());
}

#[test]
fn ssh_agent_state_shutdown_is_allocation_independent() {
    // The accept loop must stop on its own signal: no wake connection is created (an
    // fd-exhausted process cannot create one), so nothing here ever connects to the port.
    let server = agent_state::AgentStateServer::bind().expect("bind agent state server");

    server.request_stop_for_test();
    assert!(
        server.wait_for_accept_exit_for_test(Duration::from_secs(5)),
        "accept loop must exit on the allocation-free stop signal"
    );
    drop(server);

    // Same contract through `Drop`: a server that was never connected to must still tear
    // down, instead of leaving `accept()` blocked (the fd-exhaustion hang).
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let silent = agent_state::AgentStateServer::bind().expect("bind silent agent state server");
        drop(silent);
        let _ = done_tx.send(());
    });
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("agent state shutdown must complete with no wake connection available");
    worker.join().expect("shutdown worker must join");
}
