use super::*;
use super::super::super::helper::{read_frame, write_frame, Request, Runtime};
use serde_json::{json, Value};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

const BOUND: Duration = Duration::from_secs(10);

fn private_tempdir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    super::super::super::private_file(dir.path()).unwrap();
    dir
}

struct ClientInput(Receiver<Vec<u8>>, VecDeque<u8>);

impl Read for ClientInput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(bytes) => self.1.extend(bytes),
                Err(_) => return Ok(0),
            }
        }
        let n = self.1.len().min(buf.len());
        for (slot, byte) in buf.iter_mut().zip(self.1.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

struct ClientOutput(Sender<MuxFrame>, Vec<u8>);

impl Write for ClientOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.1.extend_from_slice(bytes);
        loop {
            if self.1.len() < 4 {
                break;
            }
            let len = u32::from_be_bytes([self.1[0], self.1[1], self.1[2], self.1[3]]) as usize;
            if self.1.len() < 4 + len {
                break;
            }
            let bytes: Vec<u8> = self.1.drain(..4 + len).collect();
            let frame = read_mux_frame(&mut std::io::Cursor::new(bytes)).unwrap().unwrap();
            let _ = self.0.send(frame);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Client {
    input: Option<Sender<Vec<u8>>>,
    output: Receiver<MuxFrame>,
    partial: HashMap<u32, Vec<u8>>,
}

impl Client {
    fn send(&self, channel: u32, kind: Kind, payload: Vec<u8>) {
        let mut bytes = Vec::new();
        write_mux_frame(&mut bytes, &MuxFrame { channel, kind, payload }).unwrap();
        self.input.as_ref().unwrap().send(bytes).unwrap();
    }

    fn open(&self, channel: u32) {
        self.send(channel, Kind::Open, Vec::new());
    }

    fn request(&self, channel: u32, request: &Value) {
        let mut frame = Vec::new();
        write_frame(&mut frame, request).unwrap();
        self.send(channel, Kind::Data, frame);
    }

    fn next(&self) -> MuxFrame {
        self.output.recv_timeout(BOUND).expect("mux frame within the bound")
    }

    fn reply(&mut self) -> (u32, Value) {
        loop {
            let frame = self.next();
            match frame.kind {
                Kind::Data => {
                    let buffer = self.partial.entry(frame.channel).or_default();
                    buffer.extend_from_slice(&frame.payload);
                    if buffer.len() >= 4 {
                        let len =
                            u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
                        if buffer.len() == 4 + len {
                            let bytes = self.partial.remove(&frame.channel).unwrap();
                            let value =
                                read_frame(&mut std::io::Cursor::new(bytes)).unwrap().unwrap();
                            return (frame.channel, value);
                        }
                    }
                }
                Kind::Pong => {}
                other => panic!("unexpected {other:?} on channel {}", frame.channel),
            }
        }
    }
}

fn daemon(name: &str) -> (tempfile::TempDir, std::sync::Arc<Runtime>) {
    let dir = private_tempdir();
    let bound = super::super::bind_runtime(dir.path(), name.into()).unwrap();
    let runtime = bound.runtime.clone();
    std::thread::spawn(move || {
        while let Ok((stream, _)) = bound.listener.accept() {
            let runtime = bound.runtime.clone();
            std::thread::spawn(move || {
                let _ = super::super::serve(stream, runtime);
            });
        }
    });
    (dir, runtime)
}

fn start_mux(root: &Path, idle: Duration) -> (Client, std::thread::JoinHandle<Result<(), String>>) {
    let (input_tx, input_rx) = channel();
    let (output_tx, output_rx) = channel();
    let root = root.to_path_buf();
    let handle = std::thread::spawn(move || {
        bridge_mux_with_idle_timeout(
            &root,
            ClientInput(input_rx, VecDeque::new()),
            ClientOutput(output_tx, Vec::new()),
            idle,
        )
    });
    let client = Client { input: Some(input_tx), output: output_rx, partial: HashMap::new() };
    (client, handle)
}

fn expect_hello(client: &Client) -> Value {
    let hello = client.next();
    assert_eq!((hello.channel, hello.kind), (0, Kind::Hello));
    let hello: Value = serde_json::from_slice(&hello.payload).unwrap();
    assert_eq!(hello["magic"], json!(MUX_HELLO_MAGIC));
    assert_eq!(hello["protocol"], json!(MUX_PROTOCOL));
    hello
}

#[test]
fn mux_frame_round_trips_and_rejects_bad_headers() {
    let frame = MuxFrame { channel: 7, kind: Kind::Data, payload: b"abc".to_vec() };
    let mut bytes = Vec::new();
    write_mux_frame(&mut bytes, &frame).unwrap();
    assert_eq!(read_mux_frame(&mut std::io::Cursor::new(bytes.clone())).unwrap(), Some(frame));
    assert_eq!(read_mux_frame(&mut std::io::Cursor::new(Vec::<u8>::new())).unwrap(), None);

    let mut unknown = bytes.clone();
    unknown[8] = 99;
    assert!(read_mux_frame(&mut std::io::Cursor::new(unknown)).is_err());

    let mut oversized = Vec::new();
    oversized.extend_from_slice(&((MAX_MUX_FRAME + 1) as u32).to_be_bytes());
    oversized.extend_from_slice(&[0; 5]);
    assert!(read_mux_frame(&mut std::io::Cursor::new(oversized)).is_err());
}

#[test]
fn mux_says_hello_and_serves_requests_on_independent_channels() {
    let (dir, _runtime) = daemon("qa-mux-basic");
    let (mut client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    expect_hello(&client);

    client.open(1);
    client.open(2);
    client.request(1, &json!({"protocol":1,"op":"handshake","params":{}}));
    client.request(2, &json!({"protocol":1,"op":"shell.exec","params":{}}));

    let mut replies: HashMap<u32, Value> = HashMap::new();
    while replies.len() < 2 {
        let (channel, value) = client.reply();
        replies.insert(channel, value);
    }
    assert_eq!(replies[&1]["data"]["hostId"], json!("qa-mux-basic"));
    assert_eq!(replies[&2]["error"], json!("UNSUPPORTED: operation not allowlisted"));

    client.input.take();
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn mux_long_poll_on_one_channel_does_not_block_another() {
    let (dir, runtime) = daemon("qa-mux-longpoll");
    let token = super::super::endpoint(dir.path()).unwrap().token;
    let project = tempfile::tempdir().unwrap();
    runtime
        .handle(Request {
            protocol: 1,
            token: token.clone(),
            op: "project.register".into(),
            params: json!({"id":"p","path":project.path().to_string_lossy()}),
        })
        .unwrap();
    let spawned = runtime
        .handle(Request {
            protocol: 1,
            token,
            op: "pty.spawn".into(),
            params: json!({
                "projectId":"p","cols":80,"rows":24,"clientRequestId":"mux-longpoll",
                "program": if cfg!(windows) { "cmd.exe" } else { "/bin/sh" },
                "args": if cfg!(windows) { json!(["/c","ping -n 30 127.0.0.1 >NUL"]) } else { json!(["-c","sleep 30"]) },
            }),
        })
        .unwrap();
    let target = spawned["target"].clone();

    let (mut client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    expect_hello(&client);
    client.open(1);
    client.open(2);
    client.request(
        1,
        &json!({"protocol":1,"op":"pty.read","params":{"target":target,"cursor":"999999","waitMs":8000}}),
    );
    let started = Instant::now();
    client.request(2, &json!({"protocol":1,"op":"handshake","params":{}}));
    let (channel, value) = client.reply();
    assert_eq!(channel, 2, "the handshake answered first");
    assert_eq!(value["ok"], json!(true));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "channel 2 waited {:?} behind channel 1's long poll",
        started.elapsed()
    );

    client.input.take();
    let _ = handle.join();
}

#[test]
fn mux_segments_large_replies_and_interleaves_channels() {
    let mut frame = Vec::new();
    write_frame(&mut frame, &json!({"blob": "x".repeat(300 * 1024)})).unwrap();
    let shared = Shared {
        outbox: Mutex::new(Outbox { queues: HashMap::new(), ready: VecDeque::new(), closed: false }),
        outbox_changed: Condvar::new(),
        state: Mutex::new(MuxState {
            last_frame: Instant::now(),
            end: None,
            channels: HashMap::new(),
            next_open: 0,
        }),
        state_changed: Condvar::new(),
    };
    shared.send_data(1, &frame);
    let mut small = Vec::new();
    write_frame(&mut small, &json!({"ok":true})).unwrap();
    shared.send_data(2, &small);
    shared.finish(Ok(()));

    let mut written = Vec::new();
    write_loop(&mut written, &shared).unwrap();
    let mut cursor = std::io::Cursor::new(written);
    let mut order = Vec::new();
    let mut reassembled = Vec::new();
    while let Some(frame) = read_mux_frame(&mut cursor).unwrap() {
        assert!(frame.payload.len() <= MUX_SEGMENT);
        if frame.channel == 1 {
            reassembled.extend_from_slice(&frame.payload);
        }
        order.push(frame.channel);
    }
    assert_eq!(reassembled, frame, "segments of channel 1 rebuild the helper frame");
    let small_at = order.iter().position(|&channel| channel == 2).unwrap();
    assert_eq!(small_at, 1, "channel 2 is served right after one channel 1 segment: {order:?}");
}

#[test]
fn mux_reassembles_a_request_split_across_data_frames() {
    let (dir, _runtime) = daemon("qa-mux-split");
    let (mut client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    expect_hello(&client);
    client.open(1);
    let mut frame = Vec::new();
    write_frame(&mut frame, &json!({"protocol":1,"op":"handshake","params":{}})).unwrap();
    let (head, tail) = frame.split_at(3);
    client.send(1, Kind::Data, head.to_vec());
    client.send(1, Kind::Data, tail.to_vec());
    let (channel, value) = client.reply();
    assert_eq!((channel, value["data"]["hostId"].clone()), (1, json!("qa-mux-split")));

    client.input.take();
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn mux_answers_ping_and_exits_after_idle_bound() {
    let (dir, _runtime) = daemon("qa-mux-idle");
    let idle = Duration::from_millis(300);
    let (client, handle) = start_mux(dir.path(), idle);
    expect_hello(&client);
    client.send(0, Kind::Ping, b"7".to_vec());
    let pong = client.next();
    assert_eq!((pong.channel, pong.kind, pong.payload), (0, Kind::Pong, b"7".to_vec()));

    let started = Instant::now();
    let result = handle.join().unwrap();
    assert!(
        matches!(result, Err(ref error) if error.starts_with("REMOTE_IDLE_TIMEOUT")),
        "{result:?}"
    );
    assert!(started.elapsed() < BOUND);
    drop(client);
}

#[test]
fn mux_rejects_reserved_and_duplicate_channels() {
    let (dir, _runtime) = daemon("qa-mux-reject");
    let (client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    expect_hello(&client);
    client.open(0);
    let closed = client.next();
    assert_eq!((closed.channel, closed.kind), (0, Kind::Close));
    client.open(5);
    client.open(5);
    let closed = client.next();
    assert_eq!((closed.channel, closed.kind), (5, Kind::Close));
    assert!(String::from_utf8_lossy(&closed.payload).contains("already open"));
    drop(client);
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn mux_close_releases_only_that_channel() {
    let (dir, _runtime) = daemon("qa-mux-close");
    let (mut client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    expect_hello(&client);
    client.open(1);
    client.open(2);
    client.send(1, Kind::Close, Vec::new());
    client.request(2, &json!({"protocol":1,"op":"handshake","params":{}}));
    let (channel, value) = client.reply();
    assert_eq!((channel, value["ok"].clone()), (2, json!(true)));
    client.input.take();
    assert!(handle.join().unwrap().is_ok());
}

#[test]
fn mux_fails_before_hello_without_a_daemon() {
    let dir = private_tempdir();
    let (client, handle) = start_mux(dir.path(), MUX_IDLE_TIMEOUT);
    let result = handle.join().unwrap();
    assert!(result.is_err());
    assert!(client.output.try_recv().is_err(), "no hello without a daemon");
}
