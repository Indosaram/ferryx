// Standalone rustc harness; no daemon, socket, PTY, or Cargo registration.
#[cfg(not(a10_wire_red))]
#[path = "../src/remote/terminal_wire.rs"]
mod terminal_wire;

// Faithful byte forwarding from the existing browser transport. This is NOT a
// claim that the browser adapter is a native proxy; it demonstrates why the
// native consumer must decode rather than reuse its pass-through contract.
#[test]
fn metadata_is_not_native_terminal_output() {
    let frame = b"\x1b]777;ferryx;{\"kind\":\"output\",\"sequence\":\"9007199254740993\"}\x07hello\x00\xff";
    #[cfg(a10_wire_red)]
    let terminal_bytes = frame.as_slice();
    #[cfg(not(a10_wire_red))]
    let terminal_bytes = terminal_wire::decode_frame(frame).unwrap().terminal_bytes;
    assert_eq!(terminal_bytes, b"hello\x00\xff");
}

#[cfg(not(a10_wire_red))]
mod corpus {
    use super::terminal_wire::*;
    fn frame(json: &str, bytes: &[u8]) -> Vec<u8> {
        [METADATA_PREFIX, json.as_bytes(), &[7], bytes].concat()
    }
    #[test]
    fn canonical_vectors() {
        let gap = Some(ReplayGap { requested_after_sequence: 5, available_from_sequence: 10 });
        let cases = [
            (Metadata::Output { sequence: 12, gap: None }, false, r#"{"kind":"output","sequence":"12"}"#, false),
            (Metadata::Output { sequence: 12, gap }, false, r#"{"kind":"replayGap","sequence":"12","requestedAfterSequence":"5","availableFromSequence":"10"}"#, true),
            (Metadata::Replay { start: Some(10), end: Some(12), gap: None }, false, r#"{"kind":"replay","sequence":"12","startSequence":"10","endSequence":"12"}"#, false),
            (Metadata::Replay { start: Some(10), end: Some(12), gap }, false, r#"{"kind":"replayGap","sequence":"12","requestedAfterSequence":"5","availableFromSequence":"10","startSequence":"10","endSequence":"12"}"#, true),
            (Metadata::Replay { start: None, end: None, gap: None }, false, r#"{"kind":"replay"}"#, false),
            (Metadata::Replay { start: None, end: None, gap: None }, true, r#"{"kind":"replay"}"#, true),
            (Metadata::Replay { start: None, end: None, gap }, false, r#"{"kind":"replayGap","requestedAfterSequence":"5","availableFromSequence":"10"}"#, true),
        ];
        for (metadata, force, json, reset) in cases {
            for payload in [b"".as_slice(), b"hello\x00\xff".as_slice()] {
                let terminal = if reset { [HARD_RESET, payload].concat() } else { payload.to_vec() };
                let expected = frame(json, &terminal);
                assert_eq!(encode_frame(metadata, payload, force).unwrap(), expected);
                let decoded = decode_frame(&expected).unwrap();
                assert_eq!(decoded.metadata, metadata);
                assert_eq!(decoded.terminal_bytes, terminal);
            }
        }
    }
    #[test]
    fn a02_u64_and_all_fragment_boundaries() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../../docs/evidence/paired-daemon/fixtures/contracts.json")).unwrap();
        let session = &fixtures.as_array().unwrap().iter().find(|f| f["kind"] == "session").unwrap()["value"];
        let values = [0, 1, 9_007_199_254_740_992, session["target"]["daemonEpoch"].as_str().unwrap().parse().unwrap(), session["endSequence"].as_str().unwrap().parse().unwrap()];
        assert_eq!(values[3], 9_007_199_254_740_993);
        assert_eq!(values[4], u64::MAX);
        let mut bytes: Vec<u8> = (0..=255).collect();
        bytes.extend_from_slice("A界🙂Z".as_bytes());
        bytes.extend_from_slice(b"\x1b]777;ferryx;{\"kind\":\"replay\"}\x07\x1bc");
        for sequence in values {
            let metadata = Metadata::Output { sequence, gap: None };
            let wire = encode_frame(metadata, &bytes, false).unwrap();
            assert_eq!(wire, frame(&format!(r#"{{"kind":"output","sequence":"{sequence}"}}"#), &bytes));
            for cut in 0..=wire.len() {
                let decoded = decode_fragments([&wire[..cut], &wire[cut..]]).unwrap();
                assert_eq!(decoded.metadata, metadata);
                assert_eq!(decoded.terminal_bytes, bytes);
            }
            assert_eq!(decode_fragments(wire.chunks(1)).unwrap().terminal_bytes, bytes);
        }
        let mut joined = Vec::new();
        for (sequence, part) in "🙂".as_bytes().chunks(1).enumerate() {
            let wire = encode_frame(Metadata::Output { sequence: sequence as u64, gap: None }, part, false).unwrap();
            joined.extend_from_slice(decode_frame(&wire).unwrap().terminal_bytes);
        }
        assert_eq!(joined, "🙂".as_bytes());
    }
    #[test]
    fn malformed_truncated_and_reset() {
        let valid = frame(r#"{"kind":"output","sequence":"1"}"#, b"");
        for cut in 0..valid.len() {
            assert_eq!(decode_frame(&valid[..cut]), Err(ParseError::TruncatedMetadata));
        }
        assert_eq!(decode_frame(b"{\"type\":\"attached\"}"), Err(ParseError::InvalidPrefix));
        for json in [r#"{"kind":"output","sequence":1}"#, r#"{"kind":"output","kind":"replay"}"#, "{", r#"{"kind":"replay","extra":1}"#] {
            assert_eq!(decode_frame(&frame(json, b"")), Err(ParseError::InvalidJson));
        }
        for value in ["", "00", "01", "-1", "+1", " 1", "1.0", "1e2", "18446744073709551616"] {
            assert_eq!(decode_frame(&frame(&format!(r#"{{"kind":"output","sequence":"{value}"}}"#), b"")), Err(ParseError::InvalidSequence("sequence")));
        }
        for json in [r#"{"kind":"wat"}"#, r#"{"kind":"output"}"#, r#"{"kind":"replay","startSequence":"2","endSequence":"1","sequence":"1"}"#, r#"{"kind":"replay","startSequence":"1"}"#, r#"{"kind":"replay","startSequence":"1","endSequence":"2","sequence":"3"}"#, r#"{"kind":"output","sequence":"1","requestedAfterSequence":"0"}"#, r#"{"kind":"replayGap","sequence":"1"}"#] {
            assert_eq!(decode_frame(&frame(json, b"")), Err(ParseError::InvalidMetadata));
        }
        let metadata = Metadata::Output { sequence: 10, gap: Some(ReplayGap { requested_after_sequence: 1, available_from_sequence: 10 }) };
        let wire = encode_frame(metadata, b"x", false).unwrap();
        assert_eq!(decode_frame(&wire).unwrap().terminal_bytes, b"\x1bcx");
        let end = wire.iter().position(|b| *b == 7).unwrap() + 1;
        for cut in end..end + 2 {
            assert_eq!(decode_frame(&wire[..cut]), Err(ParseError::MissingGapReset));
        }
        let wire = encode_frame(Metadata::Output { sequence: 1, gap: None }, b"\x1bcPTY reset", false).unwrap();
        assert_eq!(decode_frame(&wire).unwrap().terminal_bytes, b"\x1bcPTY reset");
    }
    #[test]
    fn allocation_bounds() {
        let metadata = Metadata::Output { sequence: 1, gap: None };
        let overhead = encode_frame(metadata, b"", false).unwrap().len();
        let exact = vec![0xff; MAX_FRAME_BYTES - overhead];
        assert_eq!(decode_frame(&encode_frame(metadata, &exact, false).unwrap()).unwrap().terminal_bytes, exact);
        assert_eq!(encode_frame(metadata, &vec![0; MAX_FRAME_BYTES], false), Err(ParseError::FrameTooLarge));
        assert_eq!(decode_fragments([vec![0; MAX_FRAME_BYTES].as_slice(), &[0]]), Err(ParseError::FrameTooLarge));
        assert_eq!(decode_frame(&vec![0; MAX_FRAME_BYTES + 1]), Err(ParseError::FrameTooLarge));
        assert_eq!(decode_frame(&[METADATA_PREFIX, &vec![b' '; MAX_METADATA_BYTES + 1], &[7]].concat()), Err(ParseError::MetadataTooLarge));
    }
}
