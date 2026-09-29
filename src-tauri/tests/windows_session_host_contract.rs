#![cfg(windows)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ferryx_lib::terminal::session_host::protocol::{
    parse_control, parse_snapshot, ControlToHost, Epoch, Frame, HelloFrame, HostToControl,
    RejectCode, Role, Secret32, HOST_PROTOCOL_VERSION,
};
use ferryx_lib::terminal::session_host::registry::{pipe_name, write_spec, HostSpec};
use ferryx_lib::terminal::session_host::win_pipe::{
    open_client, secure_private_dir, write_frame, FrameReader,
};
use ferryx_lib::terminal::session_host::win_spawn::WinProcess;
use tokio::io::{split, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::NamedPipeClient;

const LINES: u32 = 600;
const STEP: Duration = Duration::from_secs(60);
const EXIT_WAIT: Duration = Duration::from_secs(20);

struct Cleanup {
    host: Child,
    shell_pid: Option<u32>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(pid) = self.shell_pid {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.host.kill();
        let _ = self.host.wait();
    }
}

fn exited_within(process: &WinProcess, timeout: Duration) -> bool {
    process.wait_timeout(timeout).expect("wait on process")
}

fn numbers(chunks: &BTreeMap<u64, Vec<u8>>) -> Vec<u32> {
    let joined: Vec<u8> = chunks.values().flatten().copied().collect();
    regex::bytes::Regex::new(r"L([0-9]+)E")
        .unwrap()
        .captures_iter(&joined)
        .map(|c| std::str::from_utf8(&c[1]).unwrap().parse().unwrap())
        .collect()
}

fn contains(chunks: &BTreeMap<u64, Vec<u8>>, needle: &[u8]) -> bool {
    let joined: Vec<u8> = chunks.values().flatten().copied().collect();
    joined.windows(needle.len()).any(|w| w == needle)
}

struct Controller {
    reader: FrameReader<ReadHalf<NamedPipeClient>>,
    writer: WriteHalf<NamedPipeClient>,
    chunks: BTreeMap<u64, Vec<u8>>,
}

impl Controller {
    async fn send(&mut self, message: &ControlToHost) {
        write_frame(&mut self.writer, &Frame::control(message).unwrap())
            .await
            .expect("send control");
    }

    async fn next_control(&mut self) -> HostToControl {
        tokio::time::timeout(STEP, async {
            loop {
                match self.reader.read_frame().await.expect("read frame") {
                    Some(Frame::Output { sequence, bytes }) => {
                        self.chunks.insert(sequence, bytes);
                    }
                    Some(Frame::Control(json)) => return parse_control(&json).unwrap(),
                    other => panic!("unexpected frame {other:?}"),
                }
            }
        })
        .await
        .expect("control message within bound")
    }

    async fn read_until(&mut self, done: impl Fn(&BTreeMap<u64, Vec<u8>>) -> bool) {
        tokio::time::timeout(STEP, async {
            while !done(&self.chunks) {
                match self.reader.read_frame().await.expect("read frame") {
                    Some(Frame::Output { sequence, bytes }) => {
                        self.chunks.insert(sequence, bytes);
                    }
                    Some(Frame::Control(json)) => {
                        panic!("unexpected control {:?}", parse_control::<HostToControl>(&json))
                    }
                    other => panic!("unexpected frame {other:?}"),
                }
            }
        })
        .await
        .expect("output condition within bound")
    }
}

fn read_probe(path: &Path) -> (String, u32, u32, BTreeMap<u64, Vec<u8>>) {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let chunks = value["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            let bytes = pair[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect();
            (pair[0].as_u64().unwrap(), bytes)
        })
        .collect();
    (
        value["sessionId"].as_str().unwrap().to_string(),
        value["shellPid"].as_u64().unwrap() as u32,
        value["hostPid"].as_u64().unwrap() as u32,
        chunks,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_survives_controller_exit_and_resumes_on_a_new_controller() {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_ferryx"));
    let dir = tempfile::tempdir().unwrap();
    secure_private_dir(dir.path()).unwrap();
    let session_id = format!("wsh{}", std::process::id());
    let pipe = pipe_name(&session_id, &rand::random::<[u8; 16]>()).unwrap();
    let token = Secret32::generate();
    let root = std::env::var("SystemRoot").unwrap();
    let script = format!(
        "for($i=1;$i -le {LINES};$i++){{ Write-Output ('L'+$i+'E'); Start-Sleep -Milliseconds 20 }}; \
         Write-Output ('DO'+'NE'); while($true){{ $l=[Console]::ReadLine(); Write-Output ('GOT<'+$l+'>') }}"
    );
    let spec = HostSpec {
        session_id: session_id.clone(),
        pipe_name: pipe.clone(),
        token,
        cwd: dir.path().to_path_buf(),
        program: format!(r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"),
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script,
        ],
        env: std::env::vars().collect(),
        cols: 120,
        rows: 40,
    };
    let spec_path = write_spec(dir.path(), &spec).unwrap();
    let host_log = std::fs::File::create(dir.path().join("host.stderr")).unwrap();
    let host = Command::new(&exe)
        .args(["--session-host", "--spec"])
        .arg(&spec_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(host_log)
        .spawn()
        .expect("spawn host");
    let host_process = WinProcess::open(host.id()).unwrap();
    let mut cleanup = Cleanup {
        host,
        shell_pid: None,
    };

    let probe_out = dir.path().join("client-a.json");
    let mut client_a = Command::new(&exe)
        .args(["--session-host-client", "--pipe", &pipe, "--token", &token.to_hex()])
        .args(["--epoch", "1", "--min-lines", "20", "--out"])
        .arg(&probe_out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn client A");
    let client_a_process = WinProcess::open(client_a.id()).unwrap();
    assert!(exited_within(&client_a_process, STEP), "client A did not exit");
    assert!(client_a.wait().unwrap().success(), "client A failed");
    let (a_session, a_shell_pid, a_host_pid, a_chunks) = read_probe(&probe_out);
    cleanup.shell_pid = Some(a_shell_pid);
    assert_eq!(a_session, session_id);
    assert_eq!(a_host_pid, cleanup.host.id());
    let a_numbers = numbers(&a_chunks);
    let a_max = *a_numbers.iter().max().expect("client A saw lines");
    assert!(a_max < LINES, "shell finished before client A exited; no resume was exercised");
    let shell_process = WinProcess::open(a_shell_pid).expect("shell survives client A exit");
    assert!(!exited_within(&host_process, Duration::ZERO), "host exited with client A");

    let client = open_client(&pipe, tokio::time::Instant::now() + STEP).await.unwrap();
    let (read_half, writer) = split(client);
    let mut b = Controller {
        reader: FrameReader::new(read_half),
        writer,
        chunks: BTreeMap::new(),
    };
    b.send(&ControlToHost::Hello(HelloFrame {
        token,
        host_protocol: HOST_PROTOCOL_VERSION,
        controller_epoch: Epoch(2),
        controller_pid: std::process::id(),
        role: Role::Claim,
        grant_nonce: None,
    }))
    .await;
    match b.next_control().await {
        HostToControl::Welcome {
            session_id: sid,
            shell_pid,
            host_pid,
            current_epoch,
            ..
        } => {
            assert_eq!(sid, session_id);
            assert_eq!(shell_pid, a_shell_pid, "same shell after controller change");
            assert_eq!(host_pid, a_host_pid);
            assert_eq!(current_epoch, Epoch(2));
        }
        other => panic!("expected Welcome, got {other:?}"),
    }
    match tokio::time::timeout(STEP, b.reader.read_frame()).await.unwrap().unwrap() {
        Some(Frame::Snapshot(json)) => {
            for chunk in parse_snapshot(&json).unwrap().chunks {
                b.chunks.insert(chunk.sequence, chunk.bytes.to_vec());
            }
        }
        other => panic!("expected Snapshot, got {other:?}"),
    }
    b.read_until(|chunks| contains(chunks, b"DONE")).await;

    for (sequence, bytes) in &a_chunks {
        assert_eq!(b.chunks.get(sequence), Some(bytes), "sequence {sequence} changed on resume");
    }
    let expected: Vec<u32> = (1..=LINES).collect();
    assert_eq!(numbers(&b.chunks), expected, "numbered output has a gap or duplicate");

    write_frame(
        &mut b.writer,
        &Frame::Input {
            controller_epoch: Epoch(2),
            bytes: b"MARKER\r".to_vec(),
        },
    )
    .await
    .unwrap();
    b.read_until(|chunks| contains(chunks, b"GOT<MARKER>")).await;

    write_frame(
        &mut b.writer,
        &Frame::Input {
            controller_epoch: Epoch(1),
            bytes: b"STALE\r".to_vec(),
        },
    )
    .await
    .unwrap();
    match b.next_control().await {
        HostToControl::Rejected { code, .. } => assert_eq!(code, RejectCode::StaleEpoch),
        other => panic!("stale-epoch input was not rejected: {other:?}"),
    }

    b.send(&ControlToHost::Resize {
        controller_epoch: Epoch(2),
        cols: 100,
        rows: 30,
        generation: 7,
    })
    .await;
    match b.next_control().await {
        HostToControl::ResizeOk { generation, point } => {
            assert_eq!(generation, 7);
            assert_eq!((point.cols, point.rows), (100, 30));
        }
        other => panic!("expected ResizeOk, got {other:?}"),
    }

    b.send(&ControlToHost::Close {
        controller_epoch: Epoch(2),
        grace_ms: 0,
    })
    .await;
    match b.next_control().await {
        HostToControl::CloseOk { .. } => {}
        other => panic!("expected CloseOk, got {other:?}"),
    }
    assert!(exited_within(&shell_process, EXIT_WAIT), "shell outlived Close");
    // The shell is gone; its pid may be reused, so the Drop safety net must not taskkill it.
    cleanup.shell_pid = None;
    assert!(exited_within(&host_process, EXIT_WAIT), "host outlived Close");
    assert!(exited_within(&client_a_process, Duration::ZERO));
    assert!(cleanup.host.wait().unwrap().success(), "host exit code");
    assert!(!contains(&b.chunks, b"GOT<STALE>"), "stale input reached the shell");
    cleanup.shell_pid = None;
    drop(dir);
}
