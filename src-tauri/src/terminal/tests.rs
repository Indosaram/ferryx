use super::*;
use portable_pty::CommandBuilder;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

async fn wait_for_session_removal(manager: &PtyManager, session_id: &str) {
    timeout(Duration::from_secs(5), async {
        while manager.has_session(session_id) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("session should be removed from registry");
}

#[tokio::test]
async fn test_spawn_write_echo_and_read() {
    let manager = PtyManager::new();
    let cmd = CommandBuilder::new("/bin/sh");
    let (session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("failed to spawn");

    assert!(manager.has_session(&session_id));
    assert!(manager.is_alive(&session_id).unwrap_or(false));

    manager
        .write_input(&session_id, b"echo hello\n")
        .expect("failed to write input");

    let mut accumulated = Vec::new();
    let mut found = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(chunk)) => {
                accumulated.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&accumulated);
                if text.contains("hello") {
                    found = true;
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    assert!(
        found,
        "Expected output containing 'hello', got: {}",
        String::from_utf8_lossy(&accumulated)
    );

    manager
        .close_session(&session_id)
        .await
        .expect("close failed");
}

#[tokio::test]
async fn test_resize() {
    let manager = PtyManager::new();
    let cmd = CommandBuilder::new("/bin/sh");
    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("failed to spawn");

    let session = manager.get_session(&session_id).expect("session not found");
    assert_eq!(session.get_size(), (80, 24));

    manager
        .resize(&session_id, 120, 40)
        .expect("failed to resize");
    assert_eq!(session.get_size(), (120, 40));

    manager
        .resize(&session_id, 200, 60)
        .expect("failed to resize second time");
    assert_eq!(session.get_size(), (200, 60));

    manager
        .close_session(&session_id)
        .await
        .expect("close failed");
}

#[tokio::test]
async fn resize_is_observed_by_the_child_shell() {
    let manager = PtyManager::new();
    let cmd = CommandBuilder::new("/bin/sh");
    let (session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("failed to spawn");

    manager
        .resize(&session_id, 120, 40)
        .expect("failed to resize");

    manager
        .write_input(&session_id, b"stty size\n")
        .expect("failed to write input");

    let mut accumulated = Vec::new();
    let mut found = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(chunk)) => {
                accumulated.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&accumulated);
                if text.contains("40 120") {
                    found = true;
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    let output_text = String::from_utf8_lossy(&accumulated);
    assert!(
        found,
        "Expected child shell to report '40 120', got: {output_text}"
    );

    manager
        .close_session(&session_id)
        .await
        .expect("close failed");
}

#[tokio::test]
async fn test_kill() {
    let manager = PtyManager::new();
    let cmd = CommandBuilder::new("/bin/sh");
    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("failed to spawn");

    assert!(manager.has_session(&session_id));
    let pid = manager
        .get_session(&session_id)
        .and_then(|s| s.pid())
        .expect("pid must exist");

    assert!(pid > 0);

    manager
        .close_session(&session_id)
        .await
        .expect("failed to close session");
    assert!(!manager.has_session(&session_id));
}

#[tokio::test]
async fn test_multiple_concurrent_sessions() {
    let manager = PtyManager::new();

    let cmd1 = CommandBuilder::new("/bin/sh");
    let (id1, mut rx1) = manager.spawn(cmd1, 80, 24).expect("failed to spawn 1");

    let cmd2 = CommandBuilder::new("/bin/sh");
    let (id2, mut rx2) = manager.spawn(cmd2, 80, 24).expect("failed to spawn 2");

    assert_ne!(id1, id2);
    assert_eq!(manager.session_count(), 2);

    manager
        .write_input(&id1, b"echo session_one\n")
        .expect("write 1");
    manager
        .write_input(&id2, b"echo session_two\n")
        .expect("write 2");

    let mut text1 = Vec::new();
    let mut text2 = Vec::new();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut found1 = false;
    let mut found2 = false;

    while tokio::time::Instant::now() < deadline && (!found1 || !found2) {
        tokio::select! {
            Some(chunk) = rx1.recv(), if !found1 => {
                text1.extend_from_slice(&chunk);
                if String::from_utf8_lossy(&text1).contains("session_one") {
                    found1 = true;
                }
            }
            Some(chunk) = rx2.recv(), if !found2 => {
                text2.extend_from_slice(&chunk);
                if String::from_utf8_lossy(&text2).contains("session_two") {
                    found2 = true;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    assert!(
        found1,
        "Session 1 output: {}",
        String::from_utf8_lossy(&text1)
    );
    assert!(
        found2,
        "Session 2 output: {}",
        String::from_utf8_lossy(&text2)
    );

    manager.close_session(&id1).await.expect("close 1");
    manager.close_session(&id2).await.expect("close 2");
}

#[tokio::test]
async fn test_session_lifecycle_and_errors() {
    let manager = PtyManager::new();

    // Invalid session operations
    let non_existent = "non-existent-session-id";
    assert!(!manager.has_session(non_existent));
    assert!(matches!(
        manager.write_input(non_existent, b"test"),
        Err(PtyError::SessionNotFound(_))
    ));
    assert!(matches!(
        manager.resize(non_existent, 80, 24),
        Err(PtyError::SessionNotFound(_))
    ));
    assert!(matches!(
        manager.kill(non_existent),
        Err(PtyError::SessionNotFound(_))
    ));
    assert!(matches!(
        manager.is_alive(non_existent),
        Err(PtyError::SessionNotFound(_))
    ));

    // Custom session ID
    let custom_id = "custom-pty-123";
    let cmd = CommandBuilder::new("/bin/sh");
    let _rx = manager
        .spawn_with_id(custom_id, cmd, 80, 24)
        .expect("failed spawn with id");

    assert!(manager.has_session(custom_id));
    let sessions = manager.list_sessions();
    assert!(sessions.contains(&custom_id.to_string()));

    manager
        .close_session(custom_id)
        .await
        .expect("close failed");
    assert!(!manager.has_session(custom_id));
}

#[tokio::test]
async fn test_lifecycle_poll_interval_is_relaxed_and_event_driven() {
    assert!(
        LIFECYCLE_POLL_INTERVAL >= Duration::from_millis(250),
        "Lifecycle watcher poll interval must be relaxed (>= 250ms), got {:?}",
        LIFECYCLE_POLL_INTERVAL
    );

    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("exit 42");

    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn");
    let session = manager.get_session(&session_id).expect("session");

    wait_for_session_removal(&manager, &session_id).await;
    assert_eq!(
        session.state(),
        PtySessionState::Exited { code: Some(42) },
        "exit code must be accurately captured"
    );
    assert!(session.is_reaped(), "child process must be reaped");
}

#[tokio::test]
async fn natural_child_exit_auto_removes_session_and_records_exit_code() {
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("sleep 0.1; exit 7");

    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn");
    let session = manager.get_session(&session_id).expect("session");

    wait_for_session_removal(&manager, &session_id).await;
    assert_eq!(
        session.state(),
        PtySessionState::Exited { code: Some(7) },
        "natural exit must be reaped and persisted in lifecycle state"
    );
    assert!(session.is_reaped(), "child handle must be reaped");
}

#[tokio::test]
async fn explicit_close_kills_reaps_and_removes_session() {
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("sleep 30");

    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn");
    let session = manager.get_session(&session_id).expect("session");

    manager.close_session(&session_id).await.expect("close");

    assert!(!manager.has_session(&session_id));
    assert!(matches!(session.state(), PtySessionState::Exited { .. }));
    assert!(session.is_reaped(), "explicit close must reap the child");
}

#[tokio::test]
async fn close_session_is_idempotent() {
    let manager = PtyManager::new();
    let (session_id, _rx) = manager
        .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn");

    manager
        .close_session(&session_id)
        .await
        .expect("first close");
    manager
        .close_session(&session_id)
        .await
        .expect("second close");
    assert!(!manager.has_session(&session_id));
}

#[tokio::test]
async fn dropped_output_receiver_still_cleans_reader_and_session() {
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("sleep 30");

    let (session_id, rx) = manager.spawn(cmd, 80, 24).expect("spawn");
    let session = manager.get_session(&session_id).expect("session");
    drop(rx);

    wait_for_session_removal(&manager, &session_id).await;
    assert!(session.is_reader_finished(), "reader task must finish");
    assert!(session.is_reaped(), "receiver-drop cleanup must reap child");
}

#[tokio::test]
async fn fast_spawn_close_race_is_safe() {
    let manager = PtyManager::new();

    for _ in 0..16 {
        let (session_id, _rx) = manager
            .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
            .expect("spawn");
        manager.close_session(&session_id).await.expect("close");
        assert!(!manager.has_session(&session_id));
    }

    assert_eq!(manager.session_count(), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn interrupt_signal_targets_foreground_pty_process_group() {
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg("sleep 30");

    let (session_id, _rx) = manager.spawn(cmd, 80, 24).expect("spawn");
    manager
        .signal(&session_id, TerminalSignal::Interrupt)
        .expect("interrupt");

    wait_for_session_removal(&manager, &session_id).await;
    assert!(!manager.has_session(&session_id));
}

#[tokio::test]
async fn terminal_service_natural_exit_closes_daemon_owned_output_stream() {
    let pty_manager = Arc::new(PtyManager::new());
    let output_hub = Arc::new(TerminalOutputHub::new(1024));
    let service = TerminalService::new(Arc::clone(&pty_manager), Arc::clone(&output_hub));
    let mut command = CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("printf done");

    let repo = tempfile::tempdir().expect("tempdir");
    crate::worktree::run_git(repo.path(), &["init"]).expect("git init");
    let manager = crate::worktree::WorktreeManager::try_new(repo.path()).expect("worktree manager");
    command.cwd(repo.path());
    let (session_id, _legacy_rx) = service
        .spawn_in_worktree(command, 80, 24, &manager, repo.path())
        .expect("spawn service session");
    let attachment = service
        .attach_with_sequence(&session_id, None)
        .expect("attach while session is live");
    let mut receiver = attachment.receiver;

    timeout(Duration::from_secs(5), async {
        loop {
            match receiver.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
    .await
    .expect("natural exit closes output stream");
    assert!(!output_hub.has_session(&session_id));
}

#[tokio::test]
async fn test_terminal_service_sequence_safe_attach_and_replay() {
    let pty_manager = Arc::new(PtyManager::new());
    let output_hub = Arc::new(TerminalOutputHub::new(1024));
    let service = TerminalService::new(Arc::clone(&pty_manager), Arc::clone(&output_hub));

    let (session_id, _rx) = pty_manager
        .spawn(CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn");
    let _hub_rx = output_hub.register_session(&session_id);

    output_hub.publish(&session_id, b"chunk 1;".to_vec());
    output_hub.publish(&session_id, b"chunk 2;".to_vec());

    // Test non-existent session
    let err = service.attach_with_sequence("invalid-session", None);
    assert!(matches!(err, Err(PtyError::SessionNotFound(_))));

    // Test valid attach full history
    let attach = service
        .attach_with_sequence(&session_id, None)
        .expect("attach successful");
    assert_eq!(attach.snapshot.history_start_sequence, Some(1));
    assert_eq!(attach.snapshot.history_end_sequence, Some(2));
    assert_eq!(attach.snapshot.history, b"chunk 1;chunk 2;");
    assert_eq!(attach.snapshot.gap, None);

    // Test replay after sequence 1
    let attach_partial = service
        .attach_with_sequence(&session_id, Some(1))
        .expect("partial attach");
    assert_eq!(attach_partial.snapshot.history_start_sequence, Some(2));
    assert_eq!(attach_partial.snapshot.history_end_sequence, Some(2));
    assert_eq!(attach_partial.snapshot.history, b"chunk 2;");
    assert_eq!(attach_partial.snapshot.gap, None);

    service.close_session(&session_id).await.expect("close");
    assert!(!service.output_hub().has_session(&session_id));
}

#[tokio::test]
async fn spawned_pty_advertises_a_real_terminal_type() {
    // A PTY is a real terminal, so TERM must never be inherited as `dumb` from a GUI-launched
    // daemon: agent TUIs downgrade to non-interactive mode and stop reporting their state.
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "printf 'TERMIS<%s>' \"$TERM\""]);
    let (_session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("spawn");

    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv()).await {
            Ok(Some(chunk)) => {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains("TERMIS<") && seen.contains('>') {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let start = seen.find("TERMIS<").expect("shell reported TERM") + "TERMIS<".len();
    let rest = &seen[start..];
    let term = &rest[..rest.find('>').expect("terminated marker")];
    assert_ne!(term, "dumb", "a PTY must not advertise TERM=dumb");
    assert!(
        term.starts_with("xterm"),
        "expected an xterm-family TERM, got {term}"
    );
}

#[tokio::test]
async fn spawned_pty_advertises_truecolor_support() {
    // TERM=xterm-256color only claims 256 indexed colors. Truecolor-capable agent TUIs read
    // COLORTERM to decide, and render a degraded monochrome palette when it is absent.
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "printf 'CTIS<%s>' \"$COLORTERM\""]);
    let (_session_id, mut rx) = manager.spawn(cmd, 80, 24).expect("spawn");

    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(20);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv()).await {
            Ok(Some(chunk)) => {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains("CTIS<") && seen.contains('>') {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    let start = seen.find("CTIS<").expect("shell reported COLORTERM") + "CTIS<".len();
    let rest = &seen[start..];
    let colorterm = &rest[..rest.find('>').expect("terminated marker")];
    assert_eq!(
        colorterm, "truecolor",
        "a PTY must advertise truecolor so agent TUIs keep their full palette"
    );
}
