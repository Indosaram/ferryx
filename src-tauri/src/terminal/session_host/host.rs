//! Prototype session host (Windows only): `ferryx --session-host --spec <path>` owns one ConPTY
//! shell, serves one controller at a time, survives controller exit, and reaps on Close.
//! `ferryx --session-host-client` is the contract test's stand-in for an exiting old daemon.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{split, AsyncWrite};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{broadcast, watch};

use super::fence::{Clock, ConnId, HostState, MonotonicClock, PeerFacts, ProcessHandle};
use super::protocol::{
    parse_control, parse_snapshot, ControlToHost, Epoch, Frame, HelloFrame, HostToControl,
    RejectCode, Role, Secret32, HOST_PROTOCOL_VERSION,
};
use super::registry::{take_spec, HostSpec};
use super::win_pipe::{
    client_process_id, create_server_instance, open_client, write_frame, FrameReader,
};
use super::win_spawn::{
    current_process_creation_time, launch_shell_in_job, open_identity, ArmedConsole, ConptyShell,
    ShellCommand, ShellJob, WinProcess, SPEC_FLAG,
};
use crate::terminal::output_hub::{ResizePoint, TerminalOutputHub};

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const TEARDOWN_WAIT: Duration = Duration::from_secs(10);
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
const READ_CHUNK: usize = 64 * 1024;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn flag_value<'a>(args: &'a [String], flag: &str) -> Result<&'a str, BoxError> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .ok_or_else(|| format!("missing {flag} <value>").into())
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: &HostToControl) -> Result<(), BoxError> {
    write_frame(writer, &Frame::control(message)?).await?;
    Ok(())
}

enum Step {
    Continue,
    Drop,
    Closed,
}

enum Outcome {
    Disconnected,
    Finished,
}

async fn reject<W: AsyncWrite + Unpin>(
    writer: &mut W,
    code: RejectCode,
    message: impl Into<String>,
) -> Step {
    let message = HostToControl::Rejected {
        code,
        message: message.into(),
    };
    match send(writer, &message).await {
        Ok(()) => Step::Continue,
        Err(_) => Step::Drop,
    }
}

struct Host {
    spec: HostSpec,
    hub: Arc<TerminalOutputHub>,
    clock: MonotonicClock,
    state: Mutex<HostState<WinProcess>>,
    input: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    console: Mutex<Option<ArmedConsole>>,
    size: Mutex<(u16, u16)>,
    job: ShellJob,
    shell: WinProcess,
    shell_pid: u32,
    host_creation_time: u64,
    reader_done: Mutex<Option<mpsc::Receiver<()>>>,
}

pub fn run_host(args: &[String]) -> i32 {
    match host_main(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ferryx session-host: {error}");
            1
        }
    }
}

fn host_main(args: &[String]) -> Result<(), BoxError> {
    let spec_path = PathBuf::from(flag_value(args, SPEC_FLAG)?);
    let spec = take_spec(&spec_path).map_err(|e| format!("take spec: {e:?}"))?;
    let job = ShellJob::create()?;
    let command = ShellCommand {
        program: &spec.program,
        args: &spec.args,
        cwd: &spec.cwd,
        env: &spec.env,
        cols: spec.cols,
        rows: spec.rows,
    };
    let ConptyShell {
        input,
        output,
        console,
        process,
        pid,
    } = launch_shell_in_job(&command, &job)?;
    let console = match console.arm_close() {
        Ok(console) => console,
        Err((error, console)) => {
            let _ = job.terminate(1);
            drop(input);
            drop(output);
            drop(console);
            return Err(error.into());
        }
    };

    let session_id = spec.session_id.clone();
    let hub = Arc::new(TerminalOutputHub::default());
    drop(hub.register_session(&session_id));
    hub.record_initial_size(&session_id, spec.cols, spec.rows);

    let (reader_done_tx, reader_done) = mpsc::channel::<()>();
    {
        let hub = Arc::clone(&hub);
        let session_id = session_id.clone();
        let mut output = output;
        thread::Builder::new()
            .name("ferryx-sh-reader".into())
            .spawn(move || {
                let mut buffer = vec![0u8; READ_CHUNK];
                loop {
                    match output.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            hub.publish(&session_id, buffer[..read].to_vec());
                        }
                    }
                }
                let _ = reader_done_tx.send(());
            })?;
    }

    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
    {
        let mut input = input;
        thread::Builder::new()
            .name("ferryx-sh-writer".into())
            .spawn(move || {
                while let Ok(bytes) = input_rx.recv() {
                    if input.write_all(&bytes).is_err() {
                        break;
                    }
                }
            })?;
    }

    let (exit_tx, exit_rx) = watch::channel::<Option<i32>>(None);
    {
        let watcher = process.duplicate()?;
        thread::Builder::new()
            .name("ferryx-sh-exit".into())
            .spawn(move || {
                loop {
                    match watcher.wait_timeout(Duration::from_secs(3600)) {
                        Ok(true) => break,
                        Ok(false) => continue,
                        Err(_) => return,
                    }
                }
                let code = watcher.exit_code().map(|code| code as i32).unwrap_or(-1);
                let _ = exit_tx.send(Some(code));
            })?;
    }

    let clock = MonotonicClock::new();
    let host = Arc::new(Host {
        state: Mutex::new(HostState::new(spec.token, clock.now())),
        size: Mutex::new((spec.cols, spec.rows)),
        spec,
        hub,
        clock,
        input: Mutex::new(Some(input_tx)),
        console: Mutex::new(Some(console)),
        job,
        shell: process,
        shell_pid: pid,
        host_creation_time: current_process_creation_time()?,
        reader_done: Mutex::new(Some(reader_done)),
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // teardown uses block_in_place, which needs a runtime worker thread, not the block_on thread.
    runtime.block_on(async move { tokio::spawn(async move { host.serve(exit_rx).await }).await? })
}

impl Host {
    async fn serve(&self, mut exit_rx: watch::Receiver<Option<i32>>) -> Result<(), BoxError> {
        let mut listener = create_server_instance(&self.spec.pipe_name, true)?;
        let mut next_conn = 1u64;
        loop {
            tokio::select! {
                connected = listener.connect() => connected?,
                Ok(()) = exit_rx.changed() => {
                    self.teardown();
                    return Ok(());
                }
            }
            let fresh = create_server_instance(&self.spec.pipe_name, false)?;
            let pipe = std::mem::replace(&mut listener, fresh);
            let conn = ConnId(next_conn);
            next_conn += 1;
            match self.serve_connection(conn, pipe, &mut exit_rx).await {
                Outcome::Disconnected => {}
                Outcome::Finished => return Ok(()),
            }
        }
    }

    fn disconnect(&self, conn: ConnId) -> Outcome {
        let now = self.clock.now();
        self.state.lock().on_disconnect(conn, now);
        Outcome::Disconnected
    }

    async fn serve_connection(
        &self,
        conn: ConnId,
        pipe: NamedPipeServer,
        exit_rx: &mut watch::Receiver<Option<i32>>,
    ) -> Outcome {
        let client_pid = client_process_id(&pipe).ok();
        let peer = PeerFacts {
            client_pid,
            process: client_pid.and_then(|pid| open_identity(pid).ok()),
        };
        let (read_half, mut writer) = split(pipe);
        let mut reader = FrameReader::new(read_half);
        let hello = match tokio::time::timeout(HELLO_TIMEOUT, reader.read_frame()).await {
            Ok(Ok(Some(Frame::Control(json)))) => match parse_control::<ControlToHost>(&json) {
                Ok(ControlToHost::Hello(hello)) => hello,
                _ => return Outcome::Disconnected,
            },
            _ => return Outcome::Disconnected,
        };
        let now = self.clock.now();
        let admitted = self.state.lock().admit_hello(conn, &hello, peer, now);
        let admission = match admitted {
            Ok(admission) => admission,
            Err(error) => {
                let _ = reject(&mut writer, error.reject_code(), error.to_string()).await;
                return Outcome::Disconnected;
            }
        };

        // Subscribe before exporting, so every chunk is in the snapshot, the receiver, or both.
        let session_id = &self.spec.session_id;
        let Some(attachment) = self.hub.subscribe_with_sequence(session_id, None) else {
            return self.disconnect(conn);
        };
        let mut live = attachment.receiver;
        let Some(snapshot) = self.hub.export_session_state(session_id) else {
            return self.disconnect(conn);
        };
        let mut sent_through = snapshot.chunks.last().map(|chunk| chunk.sequence);
        let (cols, rows) = *self.size.lock();
        let exited = self.state.lock().exit_code();
        let welcome = HostToControl::Welcome {
            host_protocol: HOST_PROTOCOL_VERSION,
            session_id: session_id.clone(),
            shell_pid: self.shell_pid,
            host_pid: std::process::id(),
            host_creation_time: self.host_creation_time,
            cols,
            rows,
            current_epoch: admission.current_epoch,
            role: admission.role,
            exited,
        };
        if send(&mut writer, &welcome).await.is_err() {
            return self.disconnect(conn);
        }
        let snapshot_sent = match Frame::snapshot(&snapshot) {
            Ok(frame) => write_frame(&mut writer, &frame).await.is_ok(),
            Err(_) => false,
        };
        if !snapshot_sent {
            return self.disconnect(conn);
        }

        loop {
            tokio::select! {
                frame = reader.read_frame() => {
                    let Ok(Some(frame)) = frame else {
                        return self.disconnect(conn);
                    };
                    match self.handle_frame(conn, frame, &mut writer).await {
                        Step::Continue => {}
                        Step::Drop => return self.disconnect(conn),
                        Step::Closed => return Outcome::Finished,
                    }
                }
                chunk = live.recv() => match chunk {
                    Ok(chunk) => {
                        if sent_through.is_some_and(|sent| chunk.sequence <= sent) {
                            continue;
                        }
                        sent_through = Some(chunk.sequence);
                        let frame = match chunk.replay_gap {
                            Some(gap) => Frame::control(&HostToControl::GapChunk {
                                sequence: chunk.sequence,
                                gap,
                            }),
                            None => Ok(Frame::Output {
                                sequence: chunk.sequence,
                                bytes: chunk.bytes.to_vec(),
                            }),
                        };
                        let written = match frame {
                            Ok(frame) => write_frame(&mut writer, &frame).await.is_ok(),
                            Err(_) => false,
                        };
                        if !written {
                            return self.disconnect(conn);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_))
                    | Err(broadcast::error::RecvError::Closed) => return self.disconnect(conn),
                },
                Ok(()) = exit_rx.changed() => {
                    let code = *exit_rx.borrow();
                    let exited = HostToControl::Exited {
                        code,
                        final_sequence: sent_through.unwrap_or(0),
                    };
                    let _ = send(&mut writer, &exited).await;
                    self.teardown();
                    return Outcome::Finished;
                }
            }
        }
    }

    async fn handle_frame<W: AsyncWrite + Unpin>(
        &self,
        conn: ConnId,
        frame: Frame,
        writer: &mut W,
    ) -> Step {
        let now = self.clock.now();
        match frame {
            Frame::Input {
                controller_epoch,
                bytes,
            } => {
                let checked = self.state.lock().check_controller(conn, controller_epoch, now);
                if let Err(error) = checked {
                    return reject(writer, error.reject_code(), error.to_string()).await;
                }
                let delivered = self
                    .input
                    .lock()
                    .as_ref()
                    .is_some_and(|input| input.send(bytes).is_ok());
                if delivered {
                    Step::Continue
                } else {
                    reject(writer, RejectCode::Closing, "shell input is closed").await
                }
            }
            Frame::Control(json) => match parse_control::<ControlToHost>(&json) {
                Ok(ControlToHost::Resize {
                    controller_epoch,
                    cols,
                    rows,
                    generation,
                }) => {
                    let checked = self.state.lock().check_controller(conn, controller_epoch, now);
                    if let Err(error) = checked {
                        return reject(writer, error.reject_code(), error.to_string()).await;
                    }
                    let resized = self.console.lock().as_ref().map(|c| c.resize(cols, rows));
                    match resized {
                        Some(Ok(())) => {}
                        Some(Err(error)) => {
                            return reject(writer, RejectCode::Closing, error.to_string()).await
                        }
                        None => return reject(writer, RejectCode::Closing, "console closed").await,
                    }
                    *self.size.lock() = (cols, rows);
                    let Some(sequence) = self.hub.record_resize(&self.spec.session_id, cols, rows)
                    else {
                        return Step::Drop;
                    };
                    let ok = HostToControl::ResizeOk {
                        generation,
                        point: ResizePoint {
                            sequence,
                            cols,
                            rows,
                        },
                    };
                    match send(writer, &ok).await {
                        Ok(()) => Step::Continue,
                        Err(_) => Step::Drop,
                    }
                }
                Ok(ControlToHost::Close {
                    controller_epoch, ..
                }) => {
                    let begun = self.state.lock().begin_close(conn, controller_epoch, now);
                    if let Err(error) = begun {
                        return reject(writer, error.reject_code(), error.to_string()).await;
                    }
                    let code = self.teardown();
                    let _ = send(writer, &HostToControl::CloseOk { code }).await;
                    Step::Closed
                }
                Ok(ControlToHost::Ping { nonce }) => {
                    match send(writer, &HostToControl::Pong { nonce }).await {
                        Ok(()) => Step::Continue,
                        Err(_) => Step::Drop,
                    }
                }
                Ok(_) => {
                    reject(
                        writer,
                        RejectCode::ProtocolUnsupported,
                        "not supported by the prototype host",
                    )
                    .await
                }
                Err(_) => Step::Drop,
            },
            Frame::Output { .. } | Frame::Snapshot(_) => Step::Drop,
        }
    }

    /// Close order (design section 5): drop the input writer, terminate the job, wait for the
    /// shell, then close the console on its armed thread while the reader keeps draining.
    fn teardown(&self) -> Option<i32> {
        tokio::task::block_in_place(|| {
            drop(self.input.lock().take());
            let _ = self.job.terminate(1);
            let exited = self.shell.wait_timeout(TEARDOWN_WAIT).unwrap_or(false);
            let code = if exited {
                self.shell.exit_code().ok().map(|code| code as i32)
            } else {
                None
            };
            let now = self.clock.now();
            self.state.lock().mark_exited(code, now);
            let console = self.console.lock().take();
            if let Some(console) = console {
                let _ = console.close().wait(TEARDOWN_WAIT);
            }
            let reader_done = self.reader_done.lock().take();
            if let Some(done) = reader_done {
                let _ = done.recv_timeout(TEARDOWN_WAIT);
            }
            code
        })
    }
}

pub fn run_probe_client(args: &[String]) -> i32 {
    match probe_client_main(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ferryx session-host client: {error}");
            1
        }
    }
}

fn probe_client_main(args: &[String]) -> Result<(), BoxError> {
    let pipe = flag_value(args, "--pipe")?.to_string();
    let token = Secret32::parse_hex(flag_value(args, "--token")?)
        .map_err(|e| format!("token: {e:?}"))?;
    let epoch = Epoch(flag_value(args, "--epoch")?.parse()?);
    let min_lines: usize = flag_value(args, "--min-lines")?.parse()?;
    let out = PathBuf::from(flag_value(args, "--out")?);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    // The timeout's Sleep must be created inside the runtime, or it panics with "no reactor running".
    let result = runtime.block_on(async {
        tokio::time::timeout(PROBE_TIMEOUT, probe(&pipe, token, epoch, min_lines)).await
    })??;
    std::fs::write(&out, serde_json::to_vec(&result)?)?;
    Ok(())
}

async fn probe(
    pipe: &str,
    token: Secret32,
    epoch: Epoch,
    min_lines: usize,
) -> Result<serde_json::Value, BoxError> {
    let client = open_client(pipe, tokio::time::Instant::now() + OPEN_TIMEOUT).await?;
    let (read_half, mut writer) = split(client);
    let mut reader = FrameReader::new(read_half);
    let hello = ControlToHost::Hello(HelloFrame {
        token,
        host_protocol: HOST_PROTOCOL_VERSION,
        controller_epoch: epoch,
        controller_pid: std::process::id(),
        role: Role::Claim,
        grant_nonce: None,
    });
    write_frame(&mut writer, &Frame::control(&hello)?).await?;
    let (session_id, shell_pid, host_pid) = match reader.read_frame().await? {
        Some(Frame::Control(json)) => match parse_control::<HostToControl>(&json)? {
            HostToControl::Welcome {
                session_id,
                shell_pid,
                host_pid,
                ..
            } => (session_id, shell_pid, host_pid),
            other => return Err(format!("expected Welcome, got {other:?}").into()),
        },
        other => return Err(format!("expected a Welcome frame, got {other:?}").into()),
    };
    let mut chunks: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
    match reader.read_frame().await? {
        Some(Frame::Snapshot(json)) => {
            for chunk in parse_snapshot(&json)?.chunks {
                chunks.insert(chunk.sequence, chunk.bytes.to_vec());
            }
        }
        other => return Err(format!("expected a Snapshot frame, got {other:?}").into()),
    }
    let line = regex::bytes::Regex::new(r"L[0-9]+E")?;
    let count = |chunks: &BTreeMap<u64, Vec<u8>>| {
        let joined: Vec<u8> = chunks.values().flatten().copied().collect();
        line.find_iter(&joined).count()
    };
    while count(&chunks) < min_lines {
        match reader.read_frame().await? {
            Some(Frame::Output { sequence, bytes }) => {
                chunks.insert(sequence, bytes);
            }
            Some(Frame::Control(_)) => {}
            Some(other) => return Err(format!("unexpected frame {other:?}").into()),
            None => return Err("host closed the pipe".into()),
        }
    }
    let chunks: Vec<serde_json::Value> = chunks
        .iter()
        .map(|(sequence, bytes)| serde_json::json!([sequence, bytes]))
        .collect();
    Ok(serde_json::json!({
        "sessionId": session_id,
        "shellPid": shell_pid,
        "hostPid": host_pid,
        "chunks": chunks,
    }))
}
