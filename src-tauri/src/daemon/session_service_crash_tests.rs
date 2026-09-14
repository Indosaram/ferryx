use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::os::fd::{FromRawFd, AsRawFd};

#[tokio::test]
async fn child() {
    let Some(root) = std::env::var_os("A09_PRIVATE_CRASH_ROOT") else { return; };
    let root = PathBuf::from(root);
    let request: CreateSessionRequest = serde_json::from_slice(&std::fs::read(root.join("request.json")).unwrap()).unwrap();
    let target: RemoteTerminalTarget = serde_json::from_slice(&std::fs::read(root.join("target.json")).unwrap()).unwrap();
    let owner = DaemonServer::new_with_paths(Some(root.join("config")), Some(root.join("auth")));
    let phase = std::env::var("A09_PRIVATE_CRASH_PHASE").unwrap();
    let address = std::env::var("A09_PRIVATE_CRASH_ADDRESS").unwrap();
    let backend = owner.terminal_service().clone();
    let id = target.session_id.clone();
    *owner.session_service.workspace_service.transaction_probe.write() = Some(Arc::new(move |event| {
        if event != phase { return; }
        use std::io::{Read, Write};
        let mut socket = std::net::TcpStream::connect(&address).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let pid = backend.get_session(&id).and_then(|p| p.pid());
        writeln!(socket, "{}", serde_json::json!({"pid":pid,"target":id})).unwrap();
        let mut acknowledgement = [0]; socket.read_exact(&mut acknowledgement).unwrap();
        assert_eq!(acknowledgement, [1]);
        std::process::exit(73);
    }));
    let _ = owner.session_service.spawn_machine(request, "device".into(), "digest".into(), target,
        Instant::now() + Duration::from_secs(20), Arc::new(|| Ok(()))).await;
    panic!("crash probe did not terminate the private owner");
}

#[tokio::test]
async fn actual_owner_crash_reconciles_without_respawn() {
    for phase in ["sessionIntent", "sessionSpawned", "sessionCommitted"] {
        let (root, owner, request) = fixture().await;
        let target = target();
        std::fs::write(root.path().join("request.json"), serde_json::to_vec(&request).unwrap()).unwrap();
        std::fs::write(root.path().join("target.json"), serde_json::to_vec(&target).unwrap()).unwrap();
        drop(owner);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "daemon::session_service::machine_tests::crash::child", "--nocapture"])
            .env("A09_PRIVATE_CRASH_ROOT", root.path()).env("A09_PRIVATE_CRASH_PHASE", phase)
            .env("A09_PRIVATE_CRASH_ADDRESS", listener.local_addr().unwrap().to_string()).kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let (socket, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept()).await.unwrap().unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            tokio::time::timeout(Duration::from_secs(10), socket.read_line(&mut line)).await.unwrap().unwrap();
            let observed: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(observed["target"], target.session_id);
            let pid = observed["pid"].as_u64();
            assert_eq!(pid.is_some(), phase != "sessionIntent");
            let exit = pid.map(|pid| {
                let fd = unsafe { libc::kqueue() }; assert!(fd >= 0);
                let queue = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
                let change = libc::kevent { ident: pid as _, filter: libc::EVFILT_PROC, flags: libc::EV_ADD | libc::EV_ONESHOT,
                    fflags: libc::NOTE_EXIT, data: 0, udata: std::ptr::null_mut() };
                assert_eq!(unsafe { libc::kevent(queue.as_raw_fd(), &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) }, 0);
                tokio::task::spawn_blocking(move || {
                    let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                    let timeout = libc::timespec { tv_sec: 10, tv_nsec: 0 };
                    let count = unsafe { libc::kevent(queue.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, &timeout) };
                    if count != 1 {
                        // Only the PID reported by this private child is authorized for cleanup.
                        unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
                    }
                    (count, event.fflags)
                })
            });
            // Register exact kernel process exit BEFORE allowing owner termination.
            socket.get_mut().write_all(&[1]).await.unwrap();
            let status = tokio::time::timeout(Duration::from_secs(10), child.wait()).await.unwrap().unwrap();
            assert_eq!(status.code(), Some(73));
            if let Some(exit) = exit { let (count, flags) = exit.await.unwrap(); assert_eq!(count, 1); assert_ne!(flags & libc::NOTE_EXIT, 0); }
            let config = root.path().join("config"); let auth = root.path().join("auth");
            let restored = tokio::task::spawn_blocking(move || DaemonServer::new_with_paths(Some(config), Some(auth))).await.unwrap();
            let journal = restored.session_service.workspace_service.journal.reconcile("device", &request.request_id).unwrap().unwrap();
            assert_eq!(serde_json::from_str::<RemoteTerminalTarget>(&journal.resource).unwrap(), target);
            let replay = restored.session_service.spawn_machine(request.clone(), "device".into(), "digest".into(), super::target(),
                Instant::now() + Duration::from_secs(10), Arc::new(|| Ok(()))).await;
            if phase == "sessionCommitted" {
                assert!(matches!(journal.operation, Operation::Completed { .. }));
                assert_eq!(replay.unwrap(), target.session_id);
                assert!(matches!(restored.session_service.machine_detail(&target.session_id, Epoch(124)).unwrap(), SessionDetail::Expired { .. }));
            } else {
                assert!(matches!(journal.operation, Operation::OutcomeUnknown { .. }));
                assert_eq!(replay.unwrap_err(), "OPERATION_OUTCOME_UNKNOWN");
            }
            assert!(restored.terminal_service().list_sessions().is_empty());
            drop(restored);
            eprintln!("A09 actual-owner-crash phase={phase} exit=73 original_pty_pid={pid:?} kernel-exit-observed={} original-intent-restored=true no-respawn=true", pid.is_some());
        }).catch_unwind().await;
        if child.try_wait().unwrap().is_none() { child.kill().await.unwrap(); }
        child.wait().await.unwrap(); drop(listener);
        tokio::task::spawn_blocking(move || root.close().unwrap()).await.unwrap();
        eprintln!("A09 crash cleanup: private owner waited, listener dropped, root removed");
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    }
}
