#![cfg(unix)]
use ferryx_lib::terminal::{PtyManager, PtySessionState};
use futures_util::FutureExt;
use std::{
    io::{Read, Write},
    panic::AssertUnwindSafe,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn writing_child() {
    let Some(path) = std::env::var_os("A10_RECEIVER_CONTROL") else {
        return;
    };
    let mut control = std::os::unix::net::UnixStream::connect(path).unwrap();
    control
        .write_all(&std::process::id().to_le_bytes())
        .unwrap();
    let mut start = [0];
    control.read_exact(&mut start).unwrap();
    let bytes = [b'x'; 4096];
    for _ in 0..16 {
        std::io::stdout().write_all(&bytes).unwrap();
    }
    control.read_exact(&mut start).unwrap();
    // SAFETY: fcntl's integer-only F_GETFL/F_SETFL ABI needs no pointer;
    // stdout is the live slave descriptor installed by this owned PTY spawn.
    let flags = unsafe { libc::fcntl(1, libc::F_GETFL) };
    assert!(flags >= 0);
    // SAFETY: F_SETFL takes the integer flags returned above, not a pointer.
    assert_eq!(
        unsafe { libc::fcntl(1, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    loop {
        // SAFETY: write borrows exactly bytes.len() initialized bytes from
        // this live array for the duration of the call and retains no pointer.
        let n = unsafe { libc::write(1, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::WouldBlock
            );
            break;
        }
    }
    // SAFETY: restore this descriptor's checked original integer flags.
    assert_eq!(unsafe { libc::fcntl(1, libc::F_SETFL, flags) }, 0);
    control.write_all(&[2]).unwrap();
    loop {
        std::io::stdout().write_all(&bytes).unwrap();
    }
}

#[derive(Debug)]
struct InjectedScenarioFailure;

#[tokio::test]
async fn dropped_sole_receiver_closes_and_reaps_continuous_writer() {
    run_receiver_drop(false).await;
}

#[tokio::test]
async fn injected_failure_after_receiver_drop_still_reaps_owned_writer() {
    run_receiver_drop(true).await;
}

async fn run_receiver_drop(inject_failure: bool) {
    // Given a real owned PTY and a control listener installed before spawn.
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::UnixListener::bind(root.path().join("control")).unwrap();
    let manager = PtyManager::new();
    let mut cmd = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "writing_child", "--nocapture"]);
    cmd.env("A10_RECEIVER_CONTROL", root.path().join("control"));
    cmd.cwd(root.path());
    let (id, mut output) = manager.spawn(cmd, 80, 24).unwrap();
    let session = manager.get_session(&id).unwrap();
    let pid = session.pid().unwrap();
    eprintln!("A10_RECEIVER_OWNED pid={pid} session={id} injected={inject_failure}");
    let result = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let (mut control, _) = listener.accept().await.unwrap();
            assert_eq!(control.read_u32_le().await.unwrap(), pid);
            control.write_all(&[1]).await.unwrap();
            // Actual output receipt is the readiness barrier, not elapsed time.
            let mut bytes = 0;
            while bytes < 65536 {
                bytes += output.recv().await.unwrap().len();
            }
            drop(output);
            control.write_all(&[2]).await.unwrap();
            assert_eq!(
                control.read_u8().await.unwrap(),
                2,
                "kernel output saturated"
            );
            // When the scenario fails after the real receiver-drop/saturation
            // barrier, unwind through the same teardown used by normal success.
            if inject_failure {
                std::panic::panic_any(InjectedScenarioFailure);
            }
        })
        .await
        .expect("receiver-drop close watchdog");
    })
    .catch_unwind()
    .await;
    // Then the existing lifecycle authority is the only reaper. No blocking
    // rescue worker or competing waitpid survives a failed scenario.
    let closed = tokio::time::timeout(Duration::from_secs(10), manager.close_session(&id)).await;
    drop(listener);
    let root_closed = root.close();
    eprintln!("A10_RECEIVER_CLOSE pid={pid} injected={inject_failure} result={closed:?} state={:?} reaped={} reader_finished={}", session.state(), session.is_reaped(), session.is_reader_finished());
    closed
        .expect("receiver-drop close watchdog")
        .expect("receiver-drop close must succeed");
    root_closed.unwrap();
    assert!(session.is_reaped(), "exact child must be reaped");
    assert!(
        session.is_reader_finished(),
        "reader must finish before close returns"
    );
    assert!(matches!(session.state(), PtySessionState::Exited { .. }));
    assert!(!manager.has_session(&id));
    eprintln!("A10_RECEIVER_CLEANUP pid={pid} injected={inject_failure} root_removed=true production_reaped=true reader_finished=true");
    match result {
        Ok(()) => assert!(!inject_failure, "injected failure was not reached"),
        Err(panic) if inject_failure && panic.is::<InjectedScenarioFailure>() => {
            eprintln!("A10_RECEIVER_INJECTED_FAILURE_ACCEPTED pid={pid}");
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
