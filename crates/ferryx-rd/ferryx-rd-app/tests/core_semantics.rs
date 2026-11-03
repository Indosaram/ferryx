use std::time::{Duration, Instant};

use ferryx_rd_app::{
    normalize_pointer, AbrController, AudioFragmentReassembler, ClipboardDecision,
    ClipboardSynchronizer, InputKey, InputKeyMap,
};
use ferryx_rd_proto::{AudioFragment, AudioFragmentHeader};

#[test]
fn input_normalization_flips_y_and_clamps_edges() {
    assert_eq!(
        normalize_pointer(50.0, 25.0, 100.0, 100.0),
        Some((0.5, 0.75))
    );
    assert_eq!(
        normalize_pointer(-10.0, 120.0, 100.0, 100.0),
        Some((0.0, 0.0))
    );
    assert_eq!(
        normalize_pointer(120.0, -20.0, 100.0, 100.0),
        Some((1.0, 1.0))
    );
    assert_eq!(normalize_pointer(0.0, 0.0, 0.0, 100.0), None);
}

#[test]
fn windows_virtual_keys_map_to_macos_key_codes() {
    let cases = [
        (InputKey::WindowsVirtualKey(0x41), 0x00), // A
        (InputKey::WindowsVirtualKey(0x5a), 0x06), // Z
        (InputKey::WindowsVirtualKey(0x30), 0x1d), // 0
        (InputKey::WindowsVirtualKey(0x39), 0x19), // 9
        (InputKey::WindowsVirtualKey(0x25), 0x7b), // left
        (InputKey::WindowsVirtualKey(0x26), 0x7e), // up
        (InputKey::WindowsVirtualKey(0x27), 0x7c), // right
        (InputKey::WindowsVirtualKey(0x28), 0x7d), // down
        (InputKey::WindowsVirtualKey(0x20), 0x31), // space
        (InputKey::WindowsVirtualKey(0x0d), 0x24), // enter
        (InputKey::WindowsVirtualKey(0x1b), 0x35), // escape
        (InputKey::WindowsVirtualKey(0x10), 0x38), // shift
        (InputKey::WindowsVirtualKey(0x11), 0x3b), // control
    ];
    for (key, expected) in cases {
        assert_eq!(InputKeyMap::to_macos(key), Some(expected), "{key:?}");
    }
}

fn audio_fragment(frame_id: u32, index: u16, count: u16, bytes: &[u8]) -> AudioFragment {
    AudioFragment {
        header: AudioFragmentHeader {
            frame_id,
            fragment_index: index,
            fragment_count: count,
        },
        data: bytes.to_vec(),
    }
}

#[test]
fn audio_reassembly_accepts_out_of_order_fragments() {
    let mut reassembler = AudioFragmentReassembler::default();
    assert_eq!(reassembler.push(audio_fragment(7, 2, 3, b"cc")), None);
    assert_eq!(reassembler.push(audio_fragment(7, 0, 3, b"aa")), None);
    assert_eq!(
        reassembler.push(audio_fragment(7, 1, 3, b"bb")),
        Some(b"aabbcc".to_vec())
    );
}

#[test]
fn audio_reassembly_skips_lost_frame_when_a_newer_frame_arrives() {
    let mut reassembler = AudioFragmentReassembler::default();
    assert_eq!(reassembler.push(audio_fragment(40, 0, 2, b"old")), None);
    assert_eq!(
        reassembler.push(audio_fragment(41, 0, 1, b"new")),
        Some(b"new".to_vec())
    );
    assert_eq!(reassembler.pending_frames(), 0);
    assert_eq!(reassembler.skipped_frames(), 1);
}

#[test]
fn abr_reduces_above_five_percent_and_probes_gradually_to_last_good_rate() {
    let start = Instant::now();
    let mut abr = AbrController::new(8_000_000, 1_000_000, 20_000_000);
    assert_eq!(abr.evaluate(0.05, start), None);
    assert_eq!(abr.evaluate(0.051, start), Some(6_000_000));
    assert_eq!(abr.evaluate(0.0, start + Duration::from_secs(1)), None);
    assert_eq!(
        abr.evaluate(0.0, start + Duration::from_secs(2)),
        Some(6_300_000)
    );
    assert_eq!(
        abr.evaluate(0.0, start + Duration::from_secs(3)),
        Some(6_615_000)
    );
    assert_eq!(
        abr.evaluate(0.0, start + Duration::from_secs(4)),
        Some(6_945_750)
    );
    assert_eq!(
        abr.evaluate(0.0, start + Duration::from_secs(5)),
        Some(7_200_000)
    );
    // After 10s of stability, ABR resumes probing toward the 20 Mbps user cap.
    assert_eq!(
        abr.evaluate(0.0, start + Duration::from_secs(31)),
        Some(7_560_000)
    );
    for second in 32..60 {
        if let Some(rate) = abr.evaluate(0.0, start + Duration::from_secs(second)) {
            assert!(rate <= 20_000_000);
        }
        assert!(abr.current_bitrate() <= 20_000_000);
    }
    assert_eq!(abr.current_bitrate(), 20_000_000);
}

#[test]
fn clipboard_suppresses_echo_duplicate_concealed_and_oversized_text() {
    let mut clipboard = ClipboardSynchronizer::new(10);
    let remote_count = clipboard.apply_remote("remote text");
    assert_eq!(
        clipboard.poll(remote_count, "remote text", &[]),
        ClipboardDecision::SuppressEcho
    );
    assert_eq!(
        clipboard.poll(remote_count + 1, "remote text", &[]),
        ClipboardDecision::Duplicate
    );
    assert_eq!(
        clipboard.poll(
            remote_count + 2,
            "secret",
            &["org.nspasteboard.ConcealedType"]
        ),
        ClipboardDecision::Concealed
    );
    assert_eq!(
        clipboard.poll(
            remote_count + 3,
            "temporary",
            &["org.nspasteboard.TransientType"]
        ),
        ClipboardDecision::Concealed
    );
    assert_eq!(
        clipboard.poll(remote_count + 4, &"x".repeat(4097), &[]),
        ClipboardDecision::Oversized
    );
    assert_eq!(
        clipboard.poll(remote_count + 5, "local text", &[]),
        ClipboardDecision::Send("local text".to_owned())
    );
    assert_eq!(
        clipboard.poll(remote_count + 5, "local text", &[]),
        ClipboardDecision::Unchanged
    );
}
