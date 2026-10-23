use super::*;
fn host() -> SshHost {
    serde_json::from_value(serde_json::json!({"id":uuid::Uuid::new_v4().to_string(),"hostname":format!("{}.invalid",uuid::Uuid::new_v4()),"authMethod":"password"})).unwrap()
}

#[test]
fn credentials_are_transient_endpoint_scoped_and_redacted() {
    let host = host();
    let secret = "unique test password";
    assert_eq!(
        require(&host).unwrap_err().details.unwrap()["stage"],
        "authentication"
    );
    set(&host, Password::new(secret.into())).unwrap();
    let mut renamed = host.clone();
    renamed.id = "different-id".into();
    assert!(require(&renamed).is_ok());
    renamed.port = Some(2222);
    assert!(require(&renamed).is_err());
    let request = crate::daemon::protocol::DaemonRequest::SshPassword {
        host: host.clone(),
        password: Some(Password::new(secret.into())),
    };
    assert!(!format!("{request:?}").contains(secret));
    assert!(!serde_json::to_string(&host).unwrap().contains(secret));
    let plan = super::super::direct::ssh_plan(&host, "true".into(), false).unwrap();
    assert!(!format!("{plan:?}").contains(secret));
    let env = environment(&plan.args).unwrap();
    assert!(!format!("{env:?}").contains(secret));
    let port: u16 = env
        .iter()
        .find(|v| v.0 == "FERRYX_SSH_ASKPASS_PORT")
        .unwrap()
        .1
        .parse()
        .unwrap();
    let token = &env
        .iter()
        .find(|v| v.0 == "FERRYX_SSH_ASKPASS_TOKEN")
        .unwrap()
        .1;
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream.write_all(token.as_bytes()).unwrap();
    let mut received = String::new();
    stream.read_to_string(&mut received).unwrap();
    assert_eq!(received, secret);
    clear(&host).unwrap();
    assert!(require(&host).is_err());
}

#[test]
fn stale_generation_cannot_clear_replacement() {
    let host = host();
    set(&host, Password::new("first".into())).unwrap();
    let old = generation(&host).unwrap().unwrap();
    set(&host, Password::new("second".into())).unwrap();
    assert!(!clear_generation(&host, &old).unwrap());
    assert!(require(&host).is_ok());
    assert!(clear_generation(&host, &generation(&host).unwrap().unwrap()).unwrap());
    assert!(require(&host).is_err());
}

#[test]
fn argument_endpoint_lookup_matches_explicit_default_user_ipv6_and_jump() {
    for (hostname, username, port, jump) in [
        ("ssh-alias", None, None, None),
        ("2001:db8::1", Some("user"), Some(2222), None),
        (
            "destination",
            Some("user"),
            Some(22),
            Some("jump@bastion:2200"),
        ),
    ] {
        let mut host = host();
        host.hostname = hostname.into();
        host.username = username.map(Into::into);
        host.port = port;
        host.jump_host = jump.map(Into::into);
        set(&host, Password::new("endpoint-test".into())).unwrap();
        let plan = super::super::direct::ssh_plan(&host, "true".into(), false).unwrap();
        assert!(environment(&plan.args).is_ok());
        clear(&host).unwrap();
    }
}

#[test]
fn invalid_passwords_do_not_enter_store() {
    let host = host();
    for value in ["", "line\nbreak", "nul\0byte"] {
        assert!(set(&host, Password::new(value.into())).is_err());
    }
    assert!(require(&host).is_err());
}

#[test]
fn askpass_prompt_argument_detection() {
    assert!(is_askpass_prompt_args(&["ferryx-cli", "Password:"]));
    assert!(is_askpass_prompt_args(&[
        "ferryx",
        "user@example.test's password: "
    ]));
    assert!(!is_askpass_prompt_args(&["ferryx-cli"]));
    assert!(!is_askpass_prompt_args(&["ferryx-cli", "--daemon"]));
    assert!(!is_askpass_prompt_args(&["ferryx-cli", "open", "file.txt"]));
    assert!(!is_askpass_prompt_args(&[
        "ferryx-cli",
        "remote",
        "pair",
        "123456"
    ]));
}

#[test]
fn askpass_dispatch_returns_none_for_non_prompt_invocations() {
    // Ordinary CLI and daemon launches without askpass prompt or port environment return None
    assert_eq!(run_askpass_with(None, None, false), None);
    // Even if prompt is given without askpass port, returns None (and reports diagnostic)
    assert_eq!(run_askpass_with(None, None, true), None);
}

#[test]
fn askpass_dispatch_serves_password_when_credentials_match() {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    let expected_token = "secret-broker-token-1234";
    let expected_password = "correct-horse-battery-staple";

    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept connection");
        let mut buf = vec![0u8; expected_token.len()];
        socket.read_exact(&mut buf).expect("read token");
        assert_eq!(std::str::from_utf8(&buf).unwrap(), expected_token);
        use std::io::Write;
        socket
            .write_all(expected_password.as_bytes())
            .expect("write password");
    });

    let code = run_askpass_with(
        Some(port.to_string()),
        Some(expected_token.to_string()),
        true,
    );
    server.join().expect("server thread finished");
    assert_eq!(
        code,
        Some(0),
        "run_askpass must return 0 on successful credential exchange"
    );
}

#[test]
fn askpass_cli_process_serves_password_when_credentials_match() {
    use std::io::{BufReader, Read, Write};
    use std::process::{Command, Stdio};

    let cli_path = crate::test_support::ferryx_cli_binary().to_path_buf();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().unwrap().port();
    let expected_token = "secret-broker-token-1234";
    let expected_password = "correct-horse-battery-staple";

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (broker_tx, broker_rx) = std::sync::mpsc::channel();

    let server = std::thread::spawn(move || {
        ready_tx.send(()).expect("broker readiness signal sent");
        let (mut socket, _) = match listener.accept() {
            Ok(conn) => conn,
            Err(e) => {
                let _ = broker_tx.send(Err(format!("accept failed: {e}")));
                return;
            }
        };
        let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = socket.set_write_timeout(Some(Duration::from_secs(5)));
        let mut buf = vec![0u8; expected_token.len()];
        if let Err(e) = socket.read_exact(&mut buf) {
            let _ = broker_tx.send(Err(format!("read token failed: {e}")));
            return;
        }
        if buf != expected_token.as_bytes() {
            let _ = broker_tx.send(Err(format!(
                "token mismatch: expected {expected_token}, got {:?}",
                String::from_utf8_lossy(&buf)
            )));
            return;
        }
        if let Err(e) = socket.write_all(expected_password.as_bytes()) {
            let _ = broker_tx.send(Err(format!("write password failed: {e}")));
            return;
        }
        let _ = broker_tx.send(Ok(()));
    });

    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("broker listener ready before launch");

    struct BrokerCleanup(u16, Option<std::thread::JoinHandle<()>>);
    impl Drop for BrokerCleanup {
        fn drop(&mut self) {
            let _ = std::net::TcpStream::connect(("127.0.0.1", self.0));
            if let Some(h) = self.1.take() {
                let _ = h.join();
            }
        }
    }
    let mut broker_cleanup = BrokerCleanup(port, Some(server));

    struct OwnedChild(Option<std::process::Child>);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    let mut command = Command::new(&cli_path);
    command
        .arg("Password:")
        .env("FERRYX_SSH_ASKPASS_PORT", port.to_string())
        .env("FERRYX_SSH_ASKPASS_TOKEN", expected_token)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("launch ferryx-cli askpass process");
    let stdout = child.stdout.take().expect("capture child stdout");
    let stderr = child.stderr.take().expect("capture child stderr");
    let mut owned_child = OwnedChild(Some(child));

    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let res = BufReader::new(stdout).read_to_end(&mut out).map(|_| out);
        let _ = stdout_tx.send(res);
    });

    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    let stderr_reader = std::thread::spawn(move || {
        let mut err = Vec::new();
        let res = BufReader::new(stderr).read_to_end(&mut err).map(|_| err);
        let _ = stderr_tx.send(res);
    });

    let stdout_bytes = stdout_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("child stdout await timed out")
        .expect("read child stdout failed");

    let stderr_bytes = stderr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("child stderr await timed out")
        .expect("read child stderr failed");

    stdout_reader.join().expect("stdout reader finished");
    stderr_reader.join().expect("stderr reader finished");

    let status = owned_child
        .0
        .as_mut()
        .expect("owned child")
        .wait()
        .expect("wait for child process exit");
    owned_child.0.take();

    let broker_res = broker_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("broker connection await timed out");
    assert!(
        broker_res.is_ok(),
        "broker exchange failed: {:?}",
        broker_res.err()
    );

    broker_cleanup
        .1
        .take()
        .expect("broker handle")
        .join()
        .expect("server thread finished");

    assert_eq!(
        status.code(),
        Some(0),
        "ferryx-cli askpass process must exit 0, stderr: {}",
        String::from_utf8_lossy(&stderr_bytes)
    );
    assert_eq!(
        String::from_utf8_lossy(&stdout_bytes).trim_end_matches(['\r', '\n']),
        expected_password,
        "ferryx-cli askpass stdout must match expected credential"
    );
}
