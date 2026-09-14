#![cfg(windows)]
// Compile the exact production session, not a replica. Only unrelated metrics
// and its enclosing error enum are supplied by this small platform harness.
pub mod terminal {
    #[derive(Debug)]
    pub enum PtyError { IoError(String), ResizeError(String), KillError(String), Other(String) }
    pub mod metrics { pub fn record_pty_read(_: &str, _: usize) {} }
    pub use crate::session::*;
}
#[path = "../../src/terminal/session.rs"]
mod session;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{io::{Read,Write},time::Duration};
use futures_util::FutureExt;

#[link(name="kernel32")]
extern "system" { fn GetStdHandle(n:u32)->*mut std::ffi::c_void; fn SetConsoleMode(h:*mut std::ffi::c_void,mode:u32)->i32; }

#[test]
fn child() {
    let Ok(addr)=std::env::var("A10_NATIVE_CONTROL") else{return};
    let mut control=std::net::TcpStream::connect(addr).unwrap();
    unsafe { assert_ne!(SetConsoleMode(GetStdHandle(-10i32 as u32),0),0); }
    control.write_all(&std::process::id().to_le_bytes()).unwrap();
    let mut go=[0];control.read_exact(&mut go).unwrap();
    let mut count=0u64;
    loop {let mut b=[0;4096];let n=std::io::stdin().read(&mut b).unwrap();assert_ne!(n,0);count+=n as u64;if b[..n].contains(&b'!'){break}}
    control.write_all(&count.to_le_bytes()).unwrap();
    // Parent EOF is the explicit teardown signal after the count receipt.
    let _ = control.read(&mut go).unwrap();
}

#[tokio::test]
async fn native_saturation_drop_and_deadline() {
    use tokio::io::{AsyncReadExt,AsyncWriteExt};
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let pair=native_pty_system().openpty(PtySize{rows:24,cols:80,pixel_width:0,pixel_height:0}).unwrap();
    let input=session::windows_input::WindowsInput(pair.master.try_clone_input_handle().unwrap());
    let fill=session::windows_input::WindowsInput(pair.master.try_clone_input_handle().unwrap());
    let writer=pair.master.take_writer().unwrap();
    let cursor=session::windows_input::WindowsInput(pair.master.try_clone_input_handle().unwrap());
    let reader=pair.master.try_clone_reader().unwrap();
    let mut command=CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact","child","--nocapture"]);
    command.env("A10_NATIVE_CONTROL",listener.local_addr().unwrap().to_string());
    command.cwd(std::env::var("HOME").unwrap());
    let child=pair.slave.spawn_command(command).unwrap();drop(pair.slave);
    let (tx,mut rx)=tokio::sync::mpsc::channel(1024);
    let pty=std::sync::Arc::new(session::PtySession::new(session::PtySessionConfig{id:"private-proof".into(),input,master:pair.master,child,writer,reader,cols:80,rows:24,tx,worktree_path:None}));
    pty.mark_running();
    let drain=tokio::spawn(async move{while let Some(bytes)=rx.recv().await{
        println!("PTY_OUTPUT {:?}",String::from_utf8_lossy(&bytes));
        if bytes.windows(4).any(|w|w==b"\x1b[6n"){cursor.try_write(b"\x1b[1;1R").unwrap();}
    }});
    let outcome=std::panic::AssertUnwindSafe(async{
        let (mut control,_)=tokio::time::timeout(Duration::from_secs(15),listener.accept()).await.unwrap().unwrap();
        let pid=control.read_u32_le().await.unwrap();assert_eq!(pty.pid(),Some(pid));
        let mut accepted=0u64;
        let bytes=vec![b'x';65536];
        // Conhost drains its pipe into the console input queue independently of
        // the child. Saturate both queues with a bounded deadline, not one
        // transient PIPE_NOWAIT zero return.
        tokio::time::timeout(Duration::from_secs(10),async {
            while accepted < 16*1024*1024 {
                let n=fill.try_write(&bytes).unwrap(); accepted+=n as u64;
                if n==0 {tokio::task::yield_now().await;}
            }
        }).await.ok();
        // Exact zero-byte native write is the saturation observation.
        println!("SATURATED pid={pid} accepted={accepted} cwd={}",std::env::var("HOME").unwrap());
        if std::env::var_os("A10_BLOCKING_RED").is_some() {
            let writer=pty.clone();
            let (entered,observed)=tokio::sync::oneshot::channel();
            let mut worker=tokio::task::spawn_blocking(move||{entered.send(()).unwrap();writer.write_input(&vec![b'x';16*1024*1024])});
            observed.await.unwrap();
            let blocked=tokio::time::timeout(Duration::from_millis(100),&mut worker).await.is_err();
            control.write_all(&[1]).await.unwrap();
            if blocked {tokio::time::timeout(Duration::from_secs(30),&mut worker).await.unwrap().unwrap().unwrap();}
            pty.write_input_cancellable(b"!").await.unwrap();
            let count=tokio::time::timeout(Duration::from_secs(30),control.read_u64_le()).await.unwrap().unwrap();
            println!("RED_RECEIPT pid={pid} blocking={blocked} writer_joined=true received={count}");
            assert!(!blocked,"legacy synchronous production input did not yield under real ConPTY saturation");
            return;
        }

        tokio::time::pause();
        {
            let cancelled=vec![b'x';65536];
            let pending=pty.write_input_cancellable(&cancelled);tokio::pin!(pending);
            assert!(futures_util::poll!(&mut pending).is_pending());
            let timed=pty.write_input_cancellable(b"TIMEOUT");tokio::pin!(timed);
            assert!(futures_util::poll!(&mut timed).is_pending());
            println!("DEADLINE_ARMED");
            tokio::time::advance(Duration::from_secs(11)).await;
            println!("CLOCK_ADVANCED");
            let ready=futures_util::poll!(&mut timed);
            assert!(matches!(ready,std::task::Poll::Ready(Err(_))),"deadline did not fire: {ready:?}");
            println!("DEADLINE_PASSED");
        }
        tokio::time::resume();
        control.write_all(&[1]).await.unwrap();
        println!("CHILD_RELEASED");
        pty.write_input_cancellable(b"!").await.unwrap();
        println!("SENTINEL_WRITTEN");
        let received=tokio::time::timeout(Duration::from_secs(30),control.read_u64_le()).await.unwrap().unwrap();
        assert!((accepted+1..=accepted+65536+1).contains(&received));assert_eq!(pty.pid(),Some(pid));
        assert!(pty.begin_closing());
        assert!(pty.write_input_cancellable(b"AFTER_CLOSE").await.is_err());
        println!("GREEN original_pid={pid} accepted={accepted} received={received} deadline=10s-checked-at11s post_close=denied");
    }).catch_unwind().await;
    pty.kill().unwrap();pty.wait_and_reap().unwrap();pty.close_io();pty.close_output();
    if let Some(reader)=pty.take_reader_task(){tokio::time::timeout(Duration::from_secs(15),reader).await.unwrap().unwrap();}
    tokio::time::timeout(Duration::from_secs(15),drain).await.unwrap().unwrap();
    assert!(pty.is_reaped());drop(pty);drop(fill);drop(listener);
    println!("CLEANUP original child reaped; reader/drain joined; listener/handles dropped");
    if let Err(panic)=outcome{std::panic::resume_unwind(panic)}
}
