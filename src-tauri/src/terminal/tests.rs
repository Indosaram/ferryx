use super::*;
use portable_pty::CommandBuilder;
use std::time::Duration;
use tokio::time::timeout;

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

    let _ = manager.kill(&session_id);
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

    let _ = manager.kill(&session_id);
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

    manager.kill(&session_id).expect("failed to kill session");

    // Wait briefly for the process to exit
    let mut exited = false;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if let Ok(alive) = manager.is_alive(&session_id) {
            if !alive {
                exited = true;
                break;
            }
        } else {
            exited = true;
            break;
        }
    }

    assert!(exited, "Process should have exited after kill");
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

    assert!(found1, "Session 1 output: {}", String::from_utf8_lossy(&text1));
    assert!(found2, "Session 2 output: {}", String::from_utf8_lossy(&text2));

    let _ = manager.kill(&id1);
    let _ = manager.kill(&id2);
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

    let removed = manager.remove_session(custom_id);
    assert!(removed.is_some());
    assert!(!manager.has_session(custom_id));

    if let Some(session) = removed {
        let _ = session.kill();
    }
}
