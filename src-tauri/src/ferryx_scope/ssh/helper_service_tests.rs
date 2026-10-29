use super::*;

fn private_tempdir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    super::super::private_file(dir.path()).unwrap();
    dir
}

/// Bridge output the test can read after the bridge has consumed its `'static` writer.
#[derive(Clone, Default)]
struct SharedOutput(Arc<Mutex<Vec<u8>>>);

impl Write for SharedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedOutput {
    fn frames(&self) -> Vec<Value> {
        let bytes = self.0.lock().unwrap().clone();
        let mut cursor = std::io::Cursor::new(bytes);
        std::iter::from_fn(|| read_frame(&mut cursor).unwrap()).collect()
    }
}

/// Client stdin that never delivers a byte or EOF while the returned sender is alive: the
/// state of an SSH channel whose client vanished without the server noticing.
struct SilentInput(std::sync::mpsc::Receiver<Vec<u8>>, std::collections::VecDeque<u8>);

impl Read for SilentInput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(bytes) => self.1.extend(bytes),
                Err(_) => return Ok(0),
            }
        }
        let n = self.1.len().min(buf.len());
        for (slot, byte) in buf.iter_mut().zip(self.1.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

fn silent_input() -> (std::sync::mpsc::Sender<Vec<u8>>, SilentInput) {
    let (sender, receiver) = std::sync::mpsc::channel();
    (sender, SilentInput(receiver, Default::default()))
}

/// Runs `run` on a thread and returns its result, failing the test if it never ends.
fn finishes_within<T: Send + 'static>(
    bound: std::time::Duration,
    run: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (done, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(run());
    });
    result
        .recv_timeout(bound)
        .expect("bridge did not exit within the bound")
}

#[test]
fn ssh_bridge_exits_when_daemon_connection_closes_while_client_is_silent() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-daemon-gone".into()).unwrap();
    let root = dir.path().to_path_buf();
    let (_client_alive, input) = silent_input();
    let bridge = std::thread::spawn(move || bridge(&root, input, SharedOutput::default()));
    // The daemon accepts the bridge and then goes away, as when it is OOM-killed or replaced.
    let (stream, _) = bound.listener.accept().unwrap();
    drop(stream);
    drop(bound);

    let result = finishes_within(std::time::Duration::from_secs(10), move || bridge.join());
    assert!(
        matches!(result, Ok(Err(ref error)) if error.starts_with("REMOTE_UNAVAILABLE")),
        "{result:?}"
    );
}

#[test]
fn ssh_bridge_exits_after_idle_timeout_when_client_sends_nothing() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-idle".into()).unwrap();
    let runtime = bound.runtime.clone();
    let daemon = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone())
    });
    let root = dir.path().to_path_buf();
    let (_client_alive, input) = silent_input();
    let idle = std::time::Duration::from_millis(200);
    let started = std::time::Instant::now();

    let result = finishes_within(std::time::Duration::from_secs(10), move || {
        bridge_with_idle_timeout(&root, input, SharedOutput::default(), idle)
    });
    assert!(
        matches!(result, Err(ref error) if error.starts_with("REMOTE_IDLE_TIMEOUT")),
        "{result:?}"
    );
    assert!(started.elapsed() >= idle);
    // The bridge closed its daemon connection, and the daemon itself keeps serving.
    assert!(daemon.join().unwrap().is_ok());
    let auth = endpoint(dir.path()).unwrap();
    assert!(runtime
        .handle(Request {
            protocol: 1,
            token: auth.token,
            op: "handshake".into(),
            params: json!({})
        })
        .is_ok());
}

#[test]
fn ssh_bridge_idle_timeout_waits_for_in_flight_request() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-in-flight".into()).unwrap();
    let (answer, answered) = std::sync::mpsc::channel::<()>();
    // A daemon that answers only when the test says so: a long poll in progress.
    let daemon = std::thread::spawn(move || {
        let (mut stream, _) = bound.listener.accept().unwrap();
        let request = read_frame(&mut stream).unwrap().unwrap();
        answered.recv().unwrap();
        write_frame(&mut stream, &json!({"ok":true,"data":{"op":request["op"]}})).unwrap();
        while read_frame(&mut stream).unwrap().is_some() {}
    });
    let root = dir.path().to_path_buf();
    let (client, input) = silent_input();
    let mut frame = Vec::new();
    write_frame(&mut frame, &json!({"protocol":1,"op":"pty.read","params":{}})).unwrap();
    client.send(frame).unwrap();
    let output = SharedOutput::default();
    let idle = std::time::Duration::from_millis(100);
    let bridge_output = output.clone();
    let bridge = std::thread::spawn(move || {
        bridge_with_idle_timeout(&root, input, bridge_output, idle)
    });

    std::thread::sleep(idle * 3);
    assert!(!bridge.is_finished(), "an in-flight request must hold the bridge open");
    answer.send(()).unwrap();
    let result = finishes_within(std::time::Duration::from_secs(10), move || bridge.join());
    assert!(
        matches!(result, Ok(Err(ref error)) if error.starts_with("REMOTE_IDLE_TIMEOUT")),
        "{result:?}"
    );
    assert_eq!(output.frames(), vec![json!({"ok":true,"data":{"op":"pty.read"}})]);
    drop(client);
    daemon.join().unwrap();
}

#[test]
fn ssh_bridge_keeps_responses_in_request_order_around_rejected_ops() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-order".into()).unwrap();
    let worker = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone()).unwrap();
    });
    let mut input = Vec::new();
    for op in ["pty.describe", "shell.exec", "handshake"] {
        write_frame(&mut input, &json!({"protocol":1,"op":op,"params":{}})).unwrap();
    }
    let output = SharedOutput::default();
    bridge(dir.path(), std::io::Cursor::new(input), output.clone()).unwrap();
    worker.join().unwrap();

    let frames = output.frames();
    assert_eq!(frames.len(), 3, "{frames:?}");
    assert_eq!(
        frames[0]["error"].as_str().unwrap().split(':').next(),
        Some("INVALID_REQUEST")
    );
    assert_eq!(
        frames[1]["error"],
        json!("UNSUPPORTED: operation not allowlisted")
    );
    assert_eq!(frames[2]["data"]["hostId"], json!("qa-order"));
}

#[test]
fn ssh_reconnect_safety_live_runtime_cannot_be_replaced() {
    let dir = private_tempdir();
    let first = bind_runtime(dir.path(), "qa-lock".into()).unwrap();
    let original = std::fs::read(dir.path().join("endpoint.json")).unwrap();

    assert!(matches!(
        bind_runtime(dir.path(), "qa-lock".into()),
        Err(error) if error.starts_with("REMOTE_RUNTIME_CONFLICT:")
    ));
    assert_eq!(
        std::fs::read(dir.path().join("endpoint.json")).unwrap(),
        original
    );
    drop(first);
}

#[test]
fn ssh_process_survival_stale_endpoint_replaced_only_after_lock_release() {
    let dir = private_tempdir();
    let first = bind_runtime(dir.path(), "qa-stale".into()).unwrap();
    let original = endpoint(dir.path()).unwrap().token;
    drop(first);

    let second = bind_runtime(dir.path(), "qa-stale".into()).unwrap();
    assert_ne!(endpoint(dir.path()).unwrap().token, original);
    drop(second);
}

#[test]
fn ssh_reconnect_spawned_helper_losing_lock_race_attaches_to_winner() {
    let dir = private_tempdir();
    let root = dir.path().to_path_buf();
    let mut server = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let outcome = wait_for_spawned_helper(dir.path(), "qa-race", deadline, || {
        // The concurrent start wins the lock and comes up only after our child has exited.
        let winner = bind_runtime(&root, "qa-race".into()).unwrap();
        server = Some(std::thread::spawn(move || {
            let (stream, _) = winner.listener.accept().unwrap();
            serve(stream, winner.runtime.clone()).unwrap();
        }));
        Some("REMOTE_RUNTIME_EXITED: helper daemon exited (exit status: 1): REMOTE_RUNTIME_CONFLICT: lock acquisition failed".into())
    });
    assert!(matches!(outcome, SpawnWait::Ready));
    server.expect("exit was observed").join().unwrap();
}

#[test]
fn ssh_reconnect_spawned_helper_real_exit_still_fails() {
    let dir = private_tempdir();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let outcome = wait_for_spawned_helper(dir.path(), "qa-exit", deadline, || {
        Some("REMOTE_RUNTIME_EXITED: helper daemon exited (exit status: 1): FORBIDDEN: bad root".into())
    });
    assert!(matches!(outcome, SpawnWait::Exited(ref message) if message.contains("FORBIDDEN")));
}

#[cfg(unix)]
#[test]
fn ssh_reconnect_safety_rejects_symlink_root_and_endpoint() {
    use std::os::unix::fs::symlink;
    let dir = private_tempdir();
    let target = private_tempdir();
    let link = dir.path().join("alias");
    symlink(target.path(), &link).unwrap();
    assert!(bind_runtime(&link, "qa-link".into()).is_err());

    let sentinel = target.path().join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    symlink(&sentinel, dir.path().join("endpoint.json")).unwrap();
    assert!(bind_runtime(dir.path(), "qa-link".into()).is_err());
    assert!(endpoint(dir.path()).is_err());
    assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
}

#[cfg(windows)]
#[test]
fn ssh_reconnect_safety_rejects_untrusted_windows_acl() {
    let dir = private_tempdir();
    let file = dir.path().join("endpoint.json");
    std::fs::write(&file, b"{}").unwrap();
    super::super::private_file(&file).unwrap();
    assert!(validate_private(&file).is_ok());

    let grant = std::process::Command::new("icacls")
        .arg(&file)
        .args(["/grant", "*S-1-1-0:(R)"])
        .output()
        .expect("icacls grant Everyone SID");
    assert!(grant.status.success());

    assert!(matches!(
        validate_private(&file),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
    assert!(matches!(
        endpoint(dir.path()),
        Err(error) if error.starts_with("FORBIDDEN:")
    ));
}

#[test]
fn ssh_process_survival_bridge_allows_describe_without_stopping_runtime() {
    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-bridge".into()).unwrap();
    let runtime = bound.runtime.clone();
    let worker = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone()).unwrap();
    });
    let mut input = Vec::new();
    write_frame(
        &mut input,
        &json!({"protocol":1,"op":"pty.describe","params":{}}),
    )
    .unwrap();
    let output = SharedOutput::default();
    bridge(dir.path(), std::io::Cursor::new(input), output.clone()).unwrap();
    let response = output.frames().remove(0);
    assert_eq!(
        response["error"].as_str().unwrap().split(':').next(),
        Some("INVALID_REQUEST")
    );
    worker.join().unwrap();
    let auth = endpoint(dir.path()).unwrap();
    assert!(runtime
        .handle(Request {
            protocol: 1,
            token: auth.token,
            op: "handshake".into(),
            params: json!({})
        })
        .is_ok());
}

#[test]
fn ssh_process_survival_agent_state_survives_bridge_eof() {
    use std::net::TcpStream;

    let dir = private_tempdir();
    let bound = bind_runtime(dir.path(), "qa-agent-bridge".into()).unwrap();
    let runtime = bound.runtime.clone();
    let worker = std::thread::spawn(move || {
        let (stream, _) = bound.listener.accept().unwrap();
        serve(stream, bound.runtime.clone()).unwrap();
    });

    let mut input = Vec::new();
    write_frame(
        &mut input,
        &json!({"protocol":1,"op":"handshake","params":{}}),
    )
    .unwrap();
    bridge(dir.path(), std::io::Cursor::new(input), SharedOutput::default()).unwrap();
    worker.join().unwrap();

    let server_port = runtime.agent_state_server().unwrap().port();
    let stream = TcpStream::connect(("127.0.0.1", server_port));
    assert!(
        stream.is_ok(),
        "Agent state server must remain alive after bridge EOF"
    );
}

#[test]
fn ssh_helper_unit_launch_args_forward_env_and_drop_connection_vars() {
    use std::ffi::OsString;

    let env = vec![
        (
            OsString::from("FERRYX_HELPER_HYGIENE_SECS"),
            OsString::from("7"),
        ),
        (OsString::from("PATH"), OsString::from("/usr/bin")),
        (
            OsString::from("SSH_CONNECTION"),
            OsString::from("100.78.73.127 5 100.91.254.71 22"),
        ),
        (OsString::from("XDG_RUNTIME_DIR"), OsString::from("")),
        (
            OsString::from("BASH_FUNC_probe%%"),
            OsString::from("() { :; }"),
        ),
        (OsString::from("LANG"), OsString::from("C.UTF-8")),
        (OsString::from("LC_ALL"), OsString::from("C.UTF-8")),
        (
            OsString::from("AWS_SECRET_ACCESS_KEY"),
            OsString::from("secret"),
        ),
        (
            OsString::from("FERRYX_AGENT_STATE_TOKEN"),
            OsString::from("pane-token"),
        ),
        (OsString::from("XDG_SESSION_ID"), OsString::from("42")),
        (OsString::from("SSH_AUTH_SOCK"), OsString::from("/tmp/ssh-x/agent.1")),
        (OsString::from("TZ"), OsString::from("line\nbreak")),
    ];
    let args = unit_launch_args(
        std::path::Path::new("/home/u/.ferryx/bin/ferryx-remote-helper"),
        std::path::Path::new("/home/u/.ferryx/helper/ssh-omarchy"),
        "ssh-omarchy",
        &env,
    );
    let text: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

    assert!(text.contains(&"--setenv=FERRYX_HELPER_HYGIENE_SECS=7".to_string()));
    assert!(text.contains(&"--setenv=PATH=/usr/bin".to_string()));
    assert!(text.contains(&"--user".to_string()));
    assert!(text.contains(&"--collect".to_string()));
    assert!(!text
        .iter()
        .any(|arg| arg.starts_with("--setenv=SSH_CONNECTION=")));
    assert!(!text
        .iter()
        .any(|arg| arg.starts_with("--setenv=XDG_RUNTIME_DIR=")));
    assert!(!text.iter().any(|arg| arg.contains("BASH_FUNC_probe")));
    // Allowlist, not a denylist: only the named session variables reach the unit.
    assert!(text.contains(&"--setenv=LANG=C.UTF-8".to_string()));
    assert!(text.contains(&"--setenv=LC_ALL=C.UTF-8".to_string()));
    let forwarded: Vec<&str> = text
        .iter()
        .filter_map(|arg| arg.strip_prefix("--setenv="))
        .filter_map(|kv| kv.split_once('=').map(|(key, _)| key))
        .collect();
    assert_eq!(
        forwarded,
        ["FERRYX_HELPER_HYGIENE_SECS", "PATH", "LANG", "LC_ALL"],
        "{text:?}"
    );
    assert_eq!(
        &text[text.len() - 7..],
        &[
            "--",
            "/home/u/.ferryx/bin/ferryx-remote-helper",
            "daemon",
            "--root",
            "/home/u/.ferryx/helper/ssh-omarchy",
            "--host-id",
            "ssh-omarchy",
        ]
    );
    // Launch and lookup must agree on the unit for the same host and root.
    assert!(text.contains(&format!(
        "--unit={}",
        helper_unit_name("ssh-omarchy", std::path::Path::new("/home/u/.ferryx/helper/ssh-omarchy"))
    )));
}

#[test]
fn ssh_helper_unit_name_is_deterministic_and_scoped_to_the_root() {
    let root = std::path::Path::new("/home/u/.ferryx/r/2026.930.1/aaaaaaaa");
    let other_root = std::path::Path::new("/home/u/.ferryx/r/2026.930.1/bbbbbbbb");
    let other_version = std::path::Path::new("/home/u/.ferryx/r/2026.917.1/aaaaaaaa");

    // Deterministic for the same host and root; never shared across roots.
    assert_eq!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", root)
    );
    assert_ne!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", other_root)
    );
    assert_ne!(
        helper_unit_name("ssh-omarchy", root),
        helper_unit_name("ssh-omarchy", other_version)
    );

    // Host sanitization and truncation are unchanged, with a 16-hex root suffix.
    let long_host = "x".repeat(200);
    for (host, sanitized) in [
        ("ssh-omarchy", "ssh-omarchy".to_string()),
        ("a/b:c d", "a-b-c-d".to_string()),
        ("", "default".to_string()),
        (long_host.as_str(), "x".repeat(64)),
    ] {
        let unit = helper_unit_name(host, root);
        let (prefix, digest) = unit.rsplit_once('-').expect("unit suffix");
        assert_eq!(prefix, format!("ferryx-helper-{sanitized}"));
        assert_eq!(digest.len(), 16);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
