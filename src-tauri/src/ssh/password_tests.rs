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
    stream
        .write_all(format!("fixture@{}'s password: ", host.hostname).as_bytes())
        .unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
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
    assert_eq!(run_askpass_with(None, None, None), None);
    // Even if prompt is given without askpass port, returns None (and reports diagnostic)
    assert_eq!(run_askpass_with(None, None, Some("Password:".into())), None);
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
        let mut prompt = String::new();
        socket.read_to_string(&mut prompt).expect("read prompt");
        assert_eq!(prompt, "Password:");
        use std::io::Write;
        socket
            .write_all(expected_password.as_bytes())
            .expect("write password");
    });

    let code = run_askpass_with(
        Some(port.to_string()),
        Some(expected_token.to_string()),
        Some("Password:".into()),
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
        let mut prompt = String::new();
        if let Err(e) = socket.read_to_string(&mut prompt) {
            let _ = broker_tx.send(Err(format!("read prompt failed: {e}")));
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

#[test]
fn credential_key_is_unchanged_for_manual_hosts_and_follows_the_alias_for_config_hosts() {
    let mut manual = host();
    manual.username = Some("user".into());
    manual.port = Some(2222);
    manual.jump_host = Some("jump@bastion:2200".into());
    // The pre-alias formula: `SshHost::key()` plus the jump host.
    assert_eq!(key(&manual), format!("{}|jump@bastion:2200", manual.key()));
    manual.port = None;
    manual.jump_host = None;
    assert_eq!(key(&manual), format!("{}|", manual.key()));

    let mut config = host();
    config.id = format!("ssh-alias-{}", uuid::Uuid::new_v4());
    config.source = crate::ssh::SshHostSource::Config;
    config.username = Some("user".into());
    config.port = Some(2222);
    let alias = config.id.trim_start_matches("ssh-").to_string();
    config.system_config_alias = Some(alias.clone());
    assert_eq!(key(&config), format!("user@{alias}:22|"));
    set(&config, Password::new("alias-secret".into())).unwrap();
    let plan = super::super::direct::ssh_plan(&config, "true".into(), false).unwrap();
    assert!(environment(&plan.args).is_ok());
    clear(&config).unwrap();
    assert!(environment(&plan.args).is_err());
}

/// A launch session for `host` exactly as `environment` registers it from the ssh argv.
fn session_for(host: &SshHost) -> Session {
    let plan = super::super::direct::ssh_plan(host, "true".into(), false).unwrap();
    let target = plan.args[plan.args.len() - 2].clone();
    Session {
        id: uuid::Uuid::new_v4().to_string(),
        endpoint: endpoint_key(&plan.args, &target),
        target,
        jump: plan.args.iter().any(|arg| arg == "-J"),
        created: Instant::now(),
    }
}

/// A UI stand-in: records each prompt and answers it from another thread.
fn answering_ui(
    prompts: &'static Prompts,
    answer: Option<&'static str>,
) -> Arc<Mutex<Vec<AuthPrompt>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    prompts.set_listener(
        true,
        Arc::new(move |prompt: &AuthPrompt| {
            record.lock().unwrap().push(prompt.clone());
            let id = prompt.id.clone();
            std::thread::spawn(move || {
                prompts
                    .answer(&id, answer.map(|value| Password::new(value.into())))
                    .unwrap()
            });
        }),
    );
    seen
}

fn leaked_prompts() -> &'static Prompts {
    Box::leak(Box::default())
}

fn agent_host(hostname: &str) -> SshHost {
    let mut host = host();
    host.auth_method = SshAuthMethod::Agent;
    host.hostname = hostname.into();
    host.username = Some("dev".into());
    host
}

#[test]
fn prompt_classification_and_askpass_detection_cover_passphrase_otp_and_localized_text() {
    assert_eq!(classify("Enter passphrase for key '/home/u/.ssh/id_ed25519': "), PromptKind::Passphrase);
    assert_eq!(classify("dev@box's password: "), PromptKind::Password);
    assert_eq!(classify("(dev@box) Verification code: "), PromptKind::Code);
    assert_eq!(classify("(dev@box) One-time password (OATH) for `dev': "), PromptKind::Code);
    assert_eq!(classify("Duo two-factor login. Choose an option?"), PromptKind::Other);
    assert_eq!(prompt_host("(Dev@Box.example) Verification code: "), Some((Some("dev".into()), "box.example".into())));
    assert_eq!(prompt_host("dev@10.0.0.1's password: "), Some((Some("dev".into()), "10.0.0.1".into())));
    assert_eq!(prompt_host("Enter passphrase for key '/k': "), None);
    for prompt in [
        "Enter passphrase for key '/home/u/.ssh/id_ed25519': ",
        "(dev@box) Verification code: ",
        "Kennwort:",
        "Continue?",
    ] {
        assert!(is_askpass_prompt_args(&["ferryx", prompt]), "{prompt}");
    }
    assert!(!is_askpass_prompt_args(&["ferryx", "--daemon:"]));
    assert!(!is_askpass_prompt_args(&["ferryx", "open"]));
    assert!(is_plain_password_prompt("dev@box's password: "));
    assert!(is_plain_password_prompt("(dev@box) Password: "));
    assert!(is_plain_password_prompt("Password:"));
    assert!(!is_plain_password_prompt("(dev@box) One-time password (OATH) for `dev': "));
    assert!(!is_plain_password_prompt("(dev@box) New password: "));
    assert!(!is_plain_password_prompt("Enter passphrase for key '/k': "));
}

#[test]
fn passphrase_prompt_is_answered_by_the_ui_and_reused_for_the_same_key_only() {
    let prompts = leaked_prompts();
    let session = session_for(&agent_host("passphrase.invalid"));
    let seen = answering_ui(prompts, Some("key passphrase"));
    let prompt = "Enter passphrase for key '/home/dev/.ssh/id_ed25519': ";
    let answer = prompts.ask(&session, prompt, Duration::from_secs(10)).unwrap();
    assert_eq!(answer.0, "key passphrase");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].kind, PromptKind::Passphrase);
        assert_eq!(seen[0].host, "dev@passphrase.invalid");
        assert_eq!(seen[0].prompt, prompt);
        // The event payload carries the question, never an answer.
        assert!(!serde_json::to_string(&seen[0]).unwrap().contains("key passphrase"));
    }
    // The next probe of the same connection test is not asked again for the same key...
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(10)).unwrap().0, "key passphrase");
    assert_eq!(seen.lock().unwrap().len(), 1);
    // ...but a different key is.
    prompts.ask(&session, "Enter passphrase for key '/other': ", Duration::from_secs(10)).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn otp_prompt_goes_to_the_ui_every_time_and_never_gets_the_stored_password() {
    let prompts = leaked_prompts();
    let mut target = host();
    target.username = Some("dev".into());
    set(&target, Password::new("stored-password".into())).unwrap();
    let session = session_for(&target);
    let seen = answering_ui(prompts, Some("123456"));
    let otp = format!("(dev@{}) Verification code: ", target.hostname);
    for _ in 0..2 {
        assert_eq!(prompts.ask(&session, &otp, Duration::from_secs(10)).unwrap().0, "123456");
    }
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "codes are never cached");
    assert!(seen.iter().all(|prompt| prompt.kind == PromptKind::Code));
    // The stored password still answers the target's own password prompt without the UI.
    let own = format!("dev@{}'s password: ", target.hostname);
    assert_eq!(prompts.ask(&session, &own, Duration::from_secs(10)).unwrap().0, "stored-password");
    assert_eq!(seen.len(), 2);
    drop(seen);
    // An OTP question phrased as a "password" is still the user's to answer.
    let oath = format!("(dev@{}) One-time password (OATH) for `dev': ", target.hostname);
    assert_eq!(prompts.ask(&session, &oath, Duration::from_secs(10)).unwrap().0, "123456");
    clear(&target).unwrap();
}

#[test]
fn jump_host_password_prompt_never_receives_the_target_password() {
    let prompts = leaked_prompts();
    let mut target = host();
    target.username = Some("dev".into());
    target.jump_host = Some("ops@bastion.invalid".into());
    set(&target, Password::new("target-secret".into())).unwrap();
    let session = session_for(&target);
    assert!(session.jump);
    let seen = answering_ui(prompts, Some("bastion-secret"));

    let jump = prompts
        .ask(&session, "ops@bastion.invalid's password: ", Duration::from_secs(10))
        .unwrap();
    assert_eq!(jump.0, "bastion-secret", "the jump host's prompt is the user's to answer");
    // A prompt that names no host cannot be attributed while a jump host is involved.
    assert_eq!(prompts.ask(&session, "Password: ", Duration::from_secs(10)).unwrap().0, "bastion-secret");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].host, "ops@bastion.invalid");
        assert_eq!(seen[0].kind, PromptKind::Password);
    }
    let own = format!("dev@{}'s password: ", target.hostname);
    assert_eq!(prompts.ask(&session, &own, Duration::from_secs(10)).unwrap().0, "target-secret");
    assert_eq!(seen.lock().unwrap().len(), 2);

    // The password stored for one host is never handed to a different host's launch.
    let other = session_for(&{
        let mut other = host();
        other.auth_method = SshAuthMethod::Agent;
        other.username = Some("dev".into());
        other
    });
    assert!(stored_answer(&other, &own).is_none());
    clear(&target).unwrap();
}

#[test]
fn unanswered_prompt_times_out_with_a_typed_error_and_no_listener_fails_at_once() {
    let prompts = leaked_prompts();
    let session = session_for(&agent_host("timeout.invalid"));
    let prompt = "(dev@timeout.invalid) Verification code: ";
    let started = Instant::now();
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(5)).err(), Some(PromptFailure::NoListener));
    assert!(started.elapsed() < Duration::from_secs(1));

    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    prompts.set_listener(true, Arc::new(move |p: &AuthPrompt| record.lock().unwrap().push(p.id.clone())));
    let started = Instant::now();
    assert_eq!(prompts.ask(&session, prompt, Duration::from_millis(300)).err(), Some(PromptFailure::TimedOut));
    assert!(started.elapsed() >= Duration::from_millis(300));
    // The expired prompt can no longer be answered, and says so with the typed code.
    let id = seen.lock().unwrap()[0].clone();
    let late = prompts.answer(&id, Some(Password::new("123456".into()))).unwrap_err();
    assert_eq!(late.code, IpcErrorCode::Timeout);
    assert!(prompts.lock().timed_out.contains_key(&session.endpoint));
    assert_eq!(prompt_timeout_error().code, IpcErrorCode::Timeout);
    assert_eq!(prompt_timeout_error().details.unwrap()["reason"], "prompt_timeout");

    // ssh retries keyboard-interactive: the same launch is not asked again after a timeout.
    let shown = seen.lock().unwrap().len();
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(5)).err(), Some(PromptFailure::TimedOut));
    assert_eq!(seen.lock().unwrap().len(), shown);

    // Cancelling from the UI and unregistering the last listener both end the wait at once.
    let session = session_for(&agent_host("timeout.invalid"));
    let ui = answering_ui(prompts, None);
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(10)).err(), Some(PromptFailure::Cancelled));
    assert_eq!(ui.lock().unwrap().len(), 1);
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(10)).err(), Some(PromptFailure::Cancelled));
    assert_eq!(ui.lock().unwrap().len(), 1);
    prompts.set_listener(false, Arc::new(|_: &AuthPrompt| {}));
    prompts.set_listener(false, Arc::new(|_: &AuthPrompt| {}));
    let session = session_for(&agent_host("timeout.invalid"));
    assert_eq!(prompts.ask(&session, prompt, Duration::from_secs(5)).err(), Some(PromptFailure::NoListener));
}

#[test]
fn answers_are_validated_and_unknown_capabilities_are_refused() {
    let prompts = leaked_prompts();
    for bad in ["", "two\nlines", "nul\0"] {
        assert!(prompts.answer("missing", Some(Password::new(bad.into()))).is_err());
    }
    assert_eq!(prompts.answer("missing", None).unwrap_err().code, IpcErrorCode::Timeout);
    // A non-password plan may now prompt: it carries a per-launch capability, and the broker
    // refuses any capability it did not issue.
    let plan = super::super::direct::ssh_plan(&agent_host("cap.invalid"), "true".into(), false).unwrap();
    let env = environment(&plan.args).unwrap();
    let port: u16 = env.iter().find(|v| v.0 == "FERRYX_SSH_ASKPASS_PORT").unwrap().1.parse().unwrap();
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    stream.write_all(uuid::Uuid::new_v4().to_string().as_bytes()).unwrap();
    stream.write_all(b"Password: ").unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();
    let mut received = Vec::new();
    let _ = stream.read_to_end(&mut received);
    assert!(received.is_empty());
    // An interactive pane of a non-password host asks on its own terminal, as before.
    let pane = super::super::direct::ssh_plan(&agent_host("cap.invalid"), "true".into(), true).unwrap();
    assert!(pane.args.iter().any(|arg| arg == "-tt"));
    assert!(environment(&pane.args).unwrap().is_empty());
}

#[tokio::test]
async fn prompt_wait_does_not_count_against_operation_deadlines() {
    let registry = prompts();
    let session = session_for(&agent_host("deadline.invalid"));
    let (shown_tx, shown_rx) = std::sync::mpsc::channel::<String>();
    let shown_tx = Mutex::new(shown_tx);
    registry.set_listener(
        true,
        Arc::new(move |prompt: &AuthPrompt| {
            let _ = shown_tx.lock().unwrap().send(prompt.id.clone());
        }),
    );
    let asker = std::thread::spawn(move || {
        prompts().ask(&session, "(dev@deadline.invalid) Verification code: ", Duration::from_secs(10))
    });
    let id = shown_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // The operation's own budget is 200ms; the user takes 600ms to answer.
    let operation = with_prompt_deadline(Duration::from_millis(200), async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        "finished"
    });
    let answer = async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        prompts().answer(&id, Some(Password::new("424242".into()))).unwrap();
    };
    let (result, ()) = tokio::join!(operation, answer);
    assert_eq!(result, Some("finished"));
    assert_eq!(asker.join().unwrap().unwrap().0, "424242");
    registry.set_listener(false, Arc::new(|_: &AuthPrompt| {}));
    // Without a prompt the budget is enforced as before.
    assert_eq!(
        with_prompt_deadline(Duration::from_millis(50), tokio::time::sleep(Duration::from_secs(5))).await,
        None
    );
}
