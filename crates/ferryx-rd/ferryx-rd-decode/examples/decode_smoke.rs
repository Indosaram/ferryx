use std::{env, fs, process};

use ferryx_rd_decode::HevcDecoder;

fn main() {
    let mut arguments = env::args_os().skip(1);
    let Some(extradata_path) = arguments.next() else {
        eprintln!("usage: decode_smoke <extradata.avcc> <access-unit.avcc>");
        process::exit(2);
    };
    let Some(access_unit_path) = arguments.next() else {
        eprintln!("usage: decode_smoke <extradata.avcc> <access-unit.avcc>");
        process::exit(2);
    };
    let extradata = fs::read(extradata_path).expect("read extradata");
    let access_unit = fs::read(access_unit_path).expect("read access unit");
    let mut decoder = HevcDecoder::new(&extradata).expect("create HEVC decoder");
    let mut frames = decoder
        .decode(&access_unit, 1234)
        .expect("decode access unit");
    frames.extend(decoder.flush().expect("flush decoder"));
    assert!(
        !frames.is_empty(),
        "synthetic stream produced no decoded frames"
    );
    let frame = &frames[0];
    assert_eq!((frame.width, frame.height), (64, 64));
    assert_eq!(frame.y_plane.len(), 64 * 64);
    assert_eq!(frame.uv_plane.len(), 64 * 32);
    println!(
        "decoded={} acceleration={:?} first={}x{} pts={}",
        frames.len(),
        decoder.acceleration(),
        frame.width,
        frame.height,
        frame.timestamp_ms
    );
}
