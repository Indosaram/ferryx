use super::*;
use ferryx_rd_app::{AssembledFrame, FrameAssembler};
use ferryx_rd_decode::Nv12Frame;
use ferryx_rd_proto::{FrameChunk, FrameHeader};

fn fixture() -> Vec<AssembledFrame> {
    include_str!("../../../tests/fixtures/hevc-continuity.hex")
        .lines()
        .enumerate()
        .map(|(id, hex)| {
            let data: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            let key = ferryx_rd_decode::parse_length_prefixed_nalus(&data)
                .unwrap()
                .iter()
                .any(|nalu| matches!(nalu.nal_type, 19 | 20));
            AssembledFrame {
                header: FrameHeader {
                    frame_id: id as u32,
                    width: 32,
                    height: 32,
                    is_key_frame: key,
                    total_chunks: 2,
                    total_size: data.len() as u32,
                },
                data,
                timestamp_ms: id as u32,
            }
        })
        .collect()
}

fn assemble(
    assembler: &mut FrameAssembler,
    frame: &AssembledFrame,
    complete: bool,
    now: Instant,
) -> Option<AssembledFrame> {
    assert!(assembler
        .push_header(frame.header.clone(), frame.timestamp_ms, now)
        .unwrap()
        .is_none());
    let split = frame.data.len() / 2;
    assert!(assembler
        .push_chunk(
            FrameChunk {
                frame_id: frame.header.frame_id,
                chunk_index: 0,
                data: frame.data[..split].to_vec()
            },
            now
        )
        .unwrap()
        .is_none());
    if complete {
        assembler
            .push_chunk(
                FrameChunk {
                    frame_id: frame.header.frame_id,
                    chunk_index: 1,
                    data: frame.data[split..].to_vec(),
                },
                now,
            )
            .unwrap()
    } else {
        None
    }
}

fn drain(
    queue: &FrameQueue,
    decoder: &mut HevcDecoder,
    submitted: &mut Vec<u32>,
    pixels: &mut Vec<Nv12Frame>,
) {
    while let Ok((frame, _)) = queue.recv_timeout(Duration::ZERO) {
        submitted.push(frame.header.frame_id);
        pixels.extend(
            decoder
                .decode(&frame.data, frame.timestamp_ms as i64)
                .unwrap(),
        );
    }
}

#[test]
fn real_hevc_recovers_after_missing_chunk_overflow_and_completion_inversion() {
    // Given: a real two-GOP HEVC stream and its decoded pixel baseline.
    let frames = fixture();
    assert_eq!(frames.len(), 16);
    let mut baseline_decoder = HevcDecoder::from_keyframe(&frames[0].data).unwrap();
    let mut baseline = Vec::new();
    for frame in &frames {
        baseline.extend(
            baseline_decoder
                .decode(&frame.data, frame.timestamp_ms as i64)
                .unwrap(),
        );
    }
    baseline.extend(baseline_decoder.flush().unwrap());
    assert_eq!(baseline.len(), 16);
    for mode in ["missing_chunk", "overflow", "inversion"] {
        let mut assembler = FrameAssembler::default();
        let queue = FrameQueue::new();
        let mut decoder = HevcDecoder::from_keyframe(&frames[0].data).unwrap();
        let now = Instant::now();
        let mut requests = 0;
        let mut submitted = Vec::new();
        let mut pixels = Vec::new();
        // When: one reference is lost or delayed before the next real IDR.
        for (id, frame) in frames.iter().enumerate() {
            let complete = !(id == 3 && mode != "overflow");
            if let Some(assembled) = assemble(&mut assembler, frame, complete, now) {
                requests += usize::from(queue.push((assembled, now)).unwrap());
            }
            if mode == "inversion" && id == 4 {
                let late = &frames[3];
                let assembled = assembler
                    .push_chunk(
                        FrameChunk {
                            frame_id: 3,
                            chunk_index: 1,
                            data: late.data[late.data.len() / 2..].to_vec(),
                        },
                        now,
                    )
                    .unwrap()
                    .unwrap();
                requests += usize::from(queue.push((assembled, now)).unwrap());
            }
            if mode != "overflow" || !(3..=6).contains(&id) {
                drain(&queue, &mut decoder, &mut submitted, &mut pixels);
            }
        }
        pixels.extend(decoder.flush().unwrap());
        // Then: no dependent AU crosses the gap and retained pixels are exact.
        let expected_ids: Vec<u32> = (0..3).chain(8..16).collect();
        assert_eq!(submitted, expected_ids, "{mode}");
        assert_eq!(requests, 1, "{mode}");
        let expected: Vec<_> = baseline
            .iter()
            .filter(|p| p.timestamp_ms < 3 || p.timestamp_ms >= 8)
            .cloned()
            .collect();
        assert_eq!(pixels, expected, "{mode}");
        assert_eq!(
            assembler.loss_ratio(now),
            0.0,
            "headers alone cannot identify chunk loss"
        );
    }
}
