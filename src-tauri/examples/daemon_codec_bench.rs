use ferryx_lib::daemon::DaemonStreamMessage;
use serde_json::{from_str, to_string};
use std::borrow::Cow;
use std::hint::black_box;
use std::time::{Duration, Instant};

const PAYLOAD_SIZES: [usize; 4] = [4 * 1024, 32 * 1024, 128 * 1024, 512 * 1024];
const TARGET_BYTES_PER_MEASUREMENT: usize = 128 * 1024 * 1024;
const RAW_DECODE_MIN_ITERATIONS: usize = 1_000_000;

#[derive(Clone, Copy)]
struct Measurement {
    elapsed: Duration,
    mib_per_second: f64,
}

fn synthetic_terminal_bytes(size: usize) -> Vec<u8> {
    // Deterministic xorshift64* output mapped to printable ASCII. This avoids measuring a
    // trivially repetitive payload while keeping the benchmark dependency-free.
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let mut bytes = Vec::with_capacity(size);
    for _ in 0..size {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let random = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
        bytes.push(0x20 + (random % 95) as u8);
    }
    bytes
}

fn raw_length_prefixed_encode(payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).expect("benchmark payload must fit u32");
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn raw_length_prefixed_decode(frame: &[u8]) -> &[u8] {
    assert!(frame.len() >= 4, "raw frame is missing its length prefix");
    let payload_len = u32::from_le_bytes(frame[..4].try_into().expect("four-byte prefix")) as usize;
    assert_eq!(frame.len(), payload_len + 4, "raw frame length mismatch");
    &frame[4..]
}

fn measurement(total_payload_bytes: usize, elapsed: Duration) -> Measurement {
    let mib = total_payload_bytes as f64 / (1024.0 * 1024.0);
    Measurement {
        elapsed,
        mib_per_second: mib / elapsed.as_secs_f64(),
    }
}

fn benchmark_size(size: usize) -> (Measurement, Measurement, Measurement, Measurement) {
    let payload = synthetic_terminal_bytes(size);
    let iterations = (TARGET_BYTES_PER_MEASUREMENT / size).max(1);
    let warmup_iterations = ((1024 * 1024) / size).clamp(4, 256);
    let message = DaemonStreamMessage::Output {
        session_id: Cow::Borrowed("throughput-bench-session"),
        sequence: 42,
        data: Cow::Borrowed(&payload),
        metrics_read_unix_micros: None,
    };

    // client.rs/server.rs currently call serde_json::{from_str,to_string} directly around the
    // real DaemonStreamMessage type, so the benchmark imports and invokes those exact functions.
    for _ in 0..warmup_iterations {
        let mut frame = to_string(black_box(&message)).expect("encode warmup frame");
        frame.push('\n');
        let decoded: DaemonStreamMessage<'static> =
            from_str(black_box(frame.trim())).expect("decode warmup frame");
        black_box(decoded);
        let raw = raw_length_prefixed_encode(black_box(&payload));
        black_box(raw_length_prefixed_decode(black_box(&raw)));
    }

    let mut encoded_frame = to_string(&message).expect("encode reference frame");
    encoded_frame.push('\n');
    let decoded: DaemonStreamMessage<'static> =
        from_str(encoded_frame.trim()).expect("decode reference frame");
    match decoded {
        DaemonStreamMessage::Output { data, .. } => assert_eq!(data.as_ref(), payload.as_slice()),
        other => panic!("unexpected decoded frame: {other:?}"),
    }
    let raw_frame = raw_length_prefixed_encode(&payload);
    assert_eq!(raw_length_prefixed_decode(&raw_frame), payload.as_slice());

    let start = Instant::now();
    for _ in 0..iterations {
        let mut frame = to_string(black_box(&message)).expect("encode daemon frame");
        frame.push('\n');
        black_box(frame);
    }
    let json_encode = measurement(iterations * size, start.elapsed());

    let start = Instant::now();
    for _ in 0..iterations {
        let decoded: DaemonStreamMessage<'static> =
            from_str(black_box(encoded_frame.trim())).expect("decode daemon frame");
        match decoded {
            DaemonStreamMessage::Output { data, .. } => {
                black_box(data.len());
            }
            other => panic!("unexpected decoded frame: {other:?}"),
        }
    }
    let json_decode = measurement(iterations * size, start.elapsed());

    let start = Instant::now();
    for _ in 0..iterations {
        let frame = raw_length_prefixed_encode(black_box(&payload));
        black_box(frame);
    }
    let raw_encode = measurement(iterations * size, start.elapsed());

    // The raw decoder only validates a four-byte prefix and returns a payload slice, so use a
    // fixed minimum operation count to keep the result above timer-resolution noise.
    let raw_decode_iterations = iterations.max(RAW_DECODE_MIN_ITERATIONS);
    let start = Instant::now();
    for _ in 0..raw_decode_iterations {
        let decoded = raw_length_prefixed_decode(black_box(&raw_frame));
        black_box(decoded.first());
        black_box(decoded.last());
    }
    let raw_decode = measurement(raw_decode_iterations * size, start.elapsed());

    (json_encode, json_decode, raw_encode, raw_decode)
}

fn size_label(size: usize) -> String {
    if size >= 1024 * 1024 {
        format!("{} MiB", size / (1024 * 1024))
    } else {
        format!("{} KiB", size / 1024)
    }
}

fn main() {
    println!("Daemon terminal-output codec benchmark");
    println!(
        "Each JSON/base64 and raw-encode measurement processes ~{} MiB after warmup.",
        TARGET_BYTES_PER_MEASUREMENT / (1024 * 1024)
    );
    println!(
        "Raw decode uses at least {RAW_DECODE_MIN_ITERATIONS} iterations because it is an O(1) slice baseline."
    );
    println!(
        "{:<9} {:>13} {:>11} {:>13} {:>11} {:>13} {:>11} {:>13} {:>11}",
        "payload",
        "json+b64 enc",
        "enc ms",
        "json+b64 dec",
        "dec ms",
        "raw enc",
        "enc ms",
        "raw dec",
        "dec ms"
    );
    println!(
        "{:<9} {:>13} {:>11} {:>13} {:>11} {:>13} {:>11} {:>13} {:>11}",
        "", "MiB/s", "", "MiB/s", "", "MiB/s", "", "MiB/s", ""
    );

    for size in PAYLOAD_SIZES {
        let (json_encode, json_decode, raw_encode, raw_decode) = benchmark_size(size);
        println!(
            "{:<9} {:>13.1} {:>11.3} {:>13.1} {:>11.3} {:>13.1} {:>11.3} {:>13.1} {:>11.3}",
            size_label(size),
            json_encode.mib_per_second,
            json_encode.elapsed.as_secs_f64() * 1000.0,
            json_decode.mib_per_second,
            json_decode.elapsed.as_secs_f64() * 1000.0,
            raw_encode.mib_per_second,
            raw_encode.elapsed.as_secs_f64() * 1000.0,
            raw_decode.mib_per_second,
            raw_decode.elapsed.as_secs_f64() * 1000.0,
        );
    }
}
