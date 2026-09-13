#![cfg(unix)]
use ferryx_lib::terminal::PtyManager;
use std::{io::{Read, Write}, time::Duration};

#[test]
fn spawn_without_runtime_returns_error_without_registering_session() {
    let manager = PtyManager::new();
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.args(["-c", "exit 0"]);

    let error = manager.spawn(command, 80, 24).unwrap_err();

    assert!(matches!(error, ferryx_lib::terminal::PtyError::SpawnError(_)));
    assert_eq!(manager.session_count(), 0);
}

#[test]
fn input_child() {
    let Some(path) = std::env::var_os("A10_INPUT_CONTROL") else { return; };
    let mut control = std::os::unix::net::UnixStream::connect(path).unwrap();
    unsafe {
        let mut termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(0, &mut termios), 0);
        libc::cfmakeraw(&mut termios);
        assert_eq!(libc::tcsetattr(0, libc::TCSANOW, &termios), 0);
    }
    control.write_all(&std::process::id().to_le_bytes()).unwrap();
    let mut start = [0]; control.read_exact(&mut start).unwrap();
    let mut input = std::io::stdin();
    let mut total = 0;
    let mut bytes = [0;4096];
    loop {
        let n = input.read(&mut bytes).unwrap();
        assert_ne!(n,0,"PTY closed before sentinel");
        total += n;
        if bytes[..n].contains(&b'!') { break; }
    }
    control.write_all(&(total as u64).to_le_bytes()).unwrap();
    control.read_exact(&mut start).unwrap();
}

#[tokio::test]
async fn saturated_production_input_is_cancellable() {
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::UnixListener::bind(root.path().join("control")).unwrap();
    let manager = PtyManager::new();
    let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "input_child", "--nocapture"]);
    command.cwd(root.path());
    command.env("A10_INPUT_CONTROL",root.path().join("control"));
    let (id, mut output) = manager.spawn(command,80,24).unwrap();
    let pty = manager.get_session(&id).unwrap();
    let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
    use tokio::io::{AsyncReadExt,AsyncWriteExt};
    let (mut control,_) = tokio::time::timeout(Duration::from_secs(10),listener.accept()).await.unwrap().unwrap();
    let pid = control.read_u32_le().await.unwrap();
    assert_eq!(pty.pid(),Some(pid));
    assert!(pty.write_input_cancellable(&vec![0;65537]).await.is_err());
    // Fill to an actual kernel WouldBlock, not a timed guess about saturation.
    let fd = pty.raw_master_fd().unwrap();
    let bytes = vec![b'x';65536];
    let mut accepted = 0usize;
    loop {
        let n = unsafe { libc::write(fd,bytes.as_ptr().cast(),bytes.len()) };
        if n < 0 { assert_eq!(std::io::Error::last_os_error().kind(),std::io::ErrorKind::WouldBlock); break; }
        accepted += n as usize;
        assert!(accepted < 4*1024*1024);
    }
    {
        let cancel = pty.write_input_cancellable(b"LATE");
        tokio::pin!(cancel);
        assert!(futures_util::poll!(&mut cancel).is_pending());
    }
    control.write_all(&[1]).await.unwrap();
    pty.write_input_cancellable(b"!").await.unwrap();
    let received = tokio::time::timeout(Duration::from_secs(10),control.read_u64_le()).await.unwrap().unwrap();
    let correct = received == accepted as u64+1;
    assert_eq!(pty.pid(),Some(pid));
    eprintln!("A10 saturation original_pid={pid} cwd={} accepted={accepted} received={received}",root.path().display());
    assert!(correct,"cancelled bytes reached original child");
    }).catch_unwind().await;
    manager.close_session(&id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10),drain).await.unwrap().unwrap();
    assert!(pty.is_reaped());
    drop(listener);root.close().unwrap();
    eprintln!("A10 input cleanup: original child reaped, output drain joined, root removed");
    if let Err(panic) = result { std::panic::resume_unwind(panic); }
}
