    #[test]
    fn inspect_captured_wire_msg1_against_responder() {
        // This test harness is designed to decode a captured wire msg1 (100 bytes: 4-byte BE length prefix + 96-byte Noise msg1)
        // against responder parameters to determine whether rejection occurs at AEAD/prologue decrypt vs authorize callback.
        // It reads from environment variables if present, otherwise no-ops.
        let Ok(msg1_hex) = std::env::var("FERRYX_PROBE_MSG1_HEX") else {
            return;
        };
        let Ok(prologue_str) = std::env::var("FERRYX_PROBE_PROLOGUE") else {
            eprintln!("FERRYX_PROBE_PROLOGUE required");
            return;
        };
        let Ok(priv_hex) = std::env::var("FERRYX_PROBE_RESPONDER_PRIV_HEX") else {
            eprintln!("FERRYX_PROBE_RESPONDER_PRIV_HEX required");
            return;
        };
        let expected_initiator_pub_b64 = std::env::var("FERRYX_PROBE_EXPECTED_INITIATOR_PUB_B64").ok();

        let raw_wire = decode_hex(&msg1_hex);
        assert!(raw_wire.len() >= 4, "wire frame must have 4-byte prefix");
        let wire_len = u32::from_be_bytes(raw_wire[0..4].try_into().unwrap()) as usize;
        let msg1 = if raw_wire.len() == wire_len + 4 {
            &raw_wire[4..]
        } else if raw_wire.len() == 96 {
            &raw_wire[..]
        } else {
            panic!("unexpected wire length: {} (expected 100 or 96)", raw_wire.len());
        };

        let responder_priv = decode_hex(&priv_hex);
        let params = noise_params().expect("valid NoiseParams");
        let mut responder = Builder::new(params)
            .local_private_key(&responder_priv)
            .prologue(prologue_str.as_bytes())
            .build_responder()
            .expect("build snow responder");

        let mut payload = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        match responder.read_message(msg1, &mut payload) {
            Ok(read) => {
                eprintln!("[PROBE_RESULT]: AEAD_DECRYPT_OK payload_len={}", read);
                if let Some(remote_static) = responder.get_remote_static() {
                    let remote_static_b64 = base64::engine::general_purpose::STANDARD.encode(remote_static);
                    eprintln!("[PROBE_RESULT]: INITIATOR_STATIC_DECODED_LEN={}", remote_static.len());
                    if let Some(expected) = expected_initiator_pub_b64 {
                        if remote_static_b64 == expected {
                            eprintln!("[PROBE_RESULT]: AUTHORIZE_MATCH=true (matches expected attach_public_key)");
                        } else {
                            eprintln!("[PROBE_RESULT]: AUTHORIZE_MATCH=false (MISMATCH: decoded does not match expected attach_public_key)");
                        }
                    }
                } else {
                    eprintln!("[PROBE_RESULT]: REMOTE_STATIC_MISSING");
                }
            }
            Err(e) => {
                eprintln!("[PROBE_RESULT]: AEAD_DECRYPT_FAILED error={:?}", e);
            }
        }
    }
