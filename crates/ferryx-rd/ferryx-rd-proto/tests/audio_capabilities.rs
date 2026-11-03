use ferryx_rd_proto::{Capabilities, Handshake, WireCodec, PROTOCOL_VERSION};

#[test]
fn opus_capability_survives_handshake_and_old_peers_do_not_opt_in() {
    let handshake = Handshake {
        name: "audio".into(),
        width: 1,
        height: 1,
        scale: 1.0,
        version: PROTOCOL_VERSION,
        capabilities: Capabilities::AUDIO_OPUS,
        pairing_id: String::new(),
        session_salt: [0; 16],
    };
    let decoded = Handshake::decode(&handshake.encode().unwrap()).unwrap();
    assert!(decoded.capabilities.contains(Capabilities::AUDIO_OPUS));
    let old = Capabilities::from_bits_retain((1 << 16) - 1);
    assert!(!old.contains(Capabilities::AUDIO_OPUS));
    assert_eq!(Capabilities::AUDIO_OPUS.bits(), 1 << 19);
}
