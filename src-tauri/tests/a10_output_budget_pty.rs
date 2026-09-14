#![cfg(unix)]
use ferryx_lib::terminal::{output_hub::{TerminalOutputHub, machine_output::*}, pty::PtyManager};
use portable_pty::CommandBuilder;
use futures_util::FutureExt;
use std::panic::{AssertUnwindSafe, panic_any, resume_unwind};
use tokio::time::{timeout, Duration};

#[derive(Debug)]
struct InjectedPublishFailure;

#[tokio::test]
async fn sibling_progresses_when_real_pty_overflows_held_subscription() {
    run_publisher(false).await;
}

#[tokio::test]
async fn owned_resources_are_closed_when_mid_publish_panics() {
    run_publisher(true).await;
}

async fn run_publisher(inject_failure: bool) {
    // Given two subscribers installed before an owned real PTY publisher starts.
    let root = tempfile::tempdir().unwrap();
    let hub = TerminalOutputHub::default();
    hub.register_session("publisher");
    let held = hub.subscribe_machine("publisher", None).unwrap().unwrap();
    let MachineAttachment { snapshot, mut receiver } = hub.subscribe_machine("publisher", None).unwrap().unwrap();
    drop(snapshot);
    let manager = PtyManager::new();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "dd if=/dev/zero bs=65536 count=32 2>/dev/null"]);
    cmd.cwd(root.path());
    let (id, mut output) = manager.spawn(cmd, 80, 24).unwrap();
    let session = manager.get_session(&id).unwrap();
    let pid = session.pid().unwrap();
    eprintln!("A10_HUB_OWNED pid={pid} session={id} injected={inject_failure}");
    // When the real reader publishes while one subscriber never consumes.
    let progress = AssertUnwindSafe(timeout(Duration::from_secs(15), async {
        let mut bytes = 0;
        let mut peak = 0;
        while let Some(payload) = output.recv().await {
            let chunk = hub.publish("publisher", payload).unwrap();
            if inject_failure {
                panic_any(InjectedPublishFailure);
            }
            let consumed = receiver.recv().await.unwrap();
            assert_eq!(consumed.value.sequence, chunk.sequence);
            bytes += consumed.value.bytes.len();
            peak = peak.max(held.receiver.pending_bytes());
        }
        (bytes, peak)
    })).catch_unwind().await;
    // Catch assertion panics as well as watchdog expiry, then attempt both cleanups.
    // Keep the PTY reader draining while close signals/reaps the child. Dropping
    // its receiver here stops the reader before the PTY has shut down.
    let (closed, drained) = tokio::join!(
        timeout(Duration::from_secs(10), manager.close_session(&id)),
        timeout(Duration::from_secs(10), async {
            while output.recv().await.is_some() {}
        }),
    );
    drop(output);
    let reaped = session.is_reaped();
    let reader_finished = session.is_reader_finished();
    let root_closed = root.close();
    closed.unwrap().unwrap();
    root_closed.unwrap();
    drained.unwrap();
    assert!(reaped && reader_finished);
    eprintln!("A10_HUB_CLEANUP pid={pid} injected={inject_failure} reaped={reaped} reader_finished={reader_finished} root_removed=true");
    // Then an injected failure is distinguished from every real assertion failure.
    let result = match progress {
        Ok(result) => {
            assert!(!inject_failure, "injected failure was not reached");
            result
        }
        Err(panic) => {
            if inject_failure && panic.is::<InjectedPublishFailure>() {
                return;
            }
            resume_unwind(panic);
        }
    };
    // Then the sibling received all bytes and the held consumer overflowed.
    let (bytes, peak) = result.unwrap();
    assert_eq!(bytes, 2 * 1024 * 1024);
    assert!(peak <= MACHINE_OUTPUT_BYTES);
    assert_eq!(*held.receiver.termination().borrow(), Some(MachineOutputError::Overflow));
    assert!(reaped && reader_finished);
    eprintln!("A10_HUB_PTY pid={pid} sibling_bytes={bytes} peak_charged={peak} ceiling={} reaped={reaped} reader_finished={reader_finished}", MACHINE_OUTPUT_BYTES);
}
