//! Emits one JSON object per line to stdout: `{"case":"<name>","frame":<RemoteGridFrame>}`.
//! Consumed by ui/src/remote/gridFrameSeam.check.ts to parse real backend frames.

use ferryx_lib::remote::mirror::RemoteTerminalMirror;
use ferryx_lib::remote::protocol::RemoteGridFrame;

fn emit(case: &str, frame: &RemoteGridFrame) {
    let frame_json = serde_json::to_string(frame).expect("frame serializes");
    println!("{{\"case\":\"{case}\",\"frame\":{frame_json}}}");
}

fn fed(cols: u16, rows: u16, bytes: &[u8]) -> RemoteGridFrame {
    let mut mirror = RemoteTerminalMirror::new(cols, rows).expect("mirror");
    mirror.feed(bytes).expect("feed")
}

fn main() {
    emit("ascii", &fed(20, 3, b"hello world"));
    emit("sgr_bold_red", &fed(20, 3, b"\x1b[1;31mRED\x1b[0m"));
    emit("cjk", &fed(20, 3, "한글 테스트".as_bytes()));
    emit("cursor_addressed", &fed(20, 3, b"\x1b[2J\x1b[H\x1b[2;5Hxy"));

    let mut mirror = RemoteTerminalMirror::new(20, 3).expect("mirror");
    mirror.feed(b"first").expect("feed first");
    emit("diff", &mirror.feed(b"\r\nsecond").expect("feed second"));

    let mut mirror = RemoteTerminalMirror::new(20, 3).expect("mirror");
    mirror.feed(b"before resize").expect("feed pre-resize");
    emit("resize_full", &mirror.resize(40, 6).expect("resize"));

    let mut mirror = RemoteTerminalMirror::new(20, 3).expect("mirror");
    mirror.feed(b"resync me").expect("feed resync");
    emit("full_resync", &mirror.full_frame().expect("full frame"));
}
