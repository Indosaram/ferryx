use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::CodecError;

type HmacSha256 = Hmac<Sha256>;

pub const RELAY_FRAME_HEADER_LEN: usize = 17;
pub const RELAY_CHANNEL_TCP: u8 = 0x01;
pub const RELAY_CHANNEL_UDP: u8 = 0x02;
pub const RELAY_MAX_PAYLOAD_LEN: usize = 2 * 1024 * 1024;
pub const RELAY_MAX_SESSIONS_PER_HOST: usize = 4;
pub const RELAY_HEARTBEAT_SECS: u64 = 30;
pub const RELAY_IDLE_TIMEOUT_SECS: u64 = 60;
pub const RELAY_MAX_SESSION_SECS: u64 = 24 * 60 * 60;
pub const RELAY_CONNECTS_PER_MINUTE: u32 = 100;
pub const RELAY_HOST_ID_MAX_LEN: usize = 128;

/// Public routing metadata only. The PIN is entered separately and never belongs
/// in this code or in a relay control frame. This adds no session wire feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConnectCode {
    pub host_id: String,
    pub url: String,
}

impl RelayConnectCode {
    pub const PREFIX: &str = "ferryx-rd-relay:";

    pub fn parse(code: &str) -> Option<Self> {
        let (host_id, url) = code.trim().strip_prefix(Self::PREFIX)?.split_once('@')?;
        if !relay_host_id_ok(host_id)
            || !["ws://", "wss://", "http://", "https://"]
                .iter()
                .any(|prefix| url.starts_with(prefix) && url.len() > prefix.len())
            || url.chars().any(char::is_whitespace)
            || url.contains('@')
            || url.contains('#')
        {
            return None;
        }
        Some(Self {
            host_id: host_id.to_owned(),
            url: url.to_owned(),
        })
    }

    pub fn encode(&self) -> String {
        format!(
            "{}{host}@{url}",
            Self::PREFIX,
            host = self.host_id,
            url = self.url
        )
    }
}

#[must_use]
pub fn relay_host_id_ok(host_id: &str) -> bool {
    if host_id.is_empty() || host_id.len() > RELAY_HOST_ID_MAX_LEN {
        return false;
    }
    host_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

#[must_use]
pub fn relay_token(secret: &[u8], host_id: &str) -> String {
    let mut mac = match HmacSha256::new_from_slice(secret) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    mac.update(host_id.as_bytes());
    let result = mac.finalize().into_bytes();
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in result.as_slice() {
        out.push(HEX_CHARS[(b >> 4) as usize] as char);
        out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    out
}

#[must_use]
pub fn relay_token_matches(secret: &[u8], host_id: &str, presented: &str) -> bool {
    if presented.len() != 64 {
        return false;
    }
    let expected = relay_token(secret, host_id);
    if expected.len() != 64 {
        return false;
    }
    let mut diff = 0u8;
    let mut non_hex = 0u8;
    for (a, b) in expected.bytes().zip(presented.bytes()) {
        if !b.is_ascii_hexdigit() {
            non_hex |= 1;
        }
        diff |= a ^ b.to_ascii_lowercase();
    }
    diff == 0 && non_hex == 0
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayFrame {
    pub session_id: [u8; 16],
    pub channel: u8,
    pub payload: Vec<u8>,
}

pub fn encode_relay_frame(
    session_id: &[u8; 16],
    channel: u8,
    payload: &[u8],
) -> Result<Vec<u8>, CodecError> {
    if channel != RELAY_CHANNEL_TCP && channel != RELAY_CHANNEL_UDP {
        return Err(CodecError::UnknownRelayChannel(channel));
    }
    if payload.len() > RELAY_MAX_PAYLOAD_LEN {
        return Err(CodecError::LengthLimit {
            field: "relay payload",
            actual: payload.len(),
            max: RELAY_MAX_PAYLOAD_LEN,
        });
    }
    let mut out = Vec::with_capacity(RELAY_FRAME_HEADER_LEN + payload.len());
    out.extend_from_slice(session_id);
    out.push(channel);
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode_relay_frame(input: &[u8]) -> Result<RelayFrame, CodecError> {
    if input.len() < RELAY_FRAME_HEADER_LEN {
        return Err(CodecError::Truncated {
            field: "relay frame header",
            needed: RELAY_FRAME_HEADER_LEN,
            remaining: input.len(),
        });
    }
    let channel = input[16];
    if channel != RELAY_CHANNEL_TCP && channel != RELAY_CHANNEL_UDP {
        return Err(CodecError::UnknownRelayChannel(channel));
    }
    let payload_len = input.len() - RELAY_FRAME_HEADER_LEN;
    if payload_len > RELAY_MAX_PAYLOAD_LEN {
        return Err(CodecError::LengthLimit {
            field: "relay payload",
            actual: payload_len,
            max: RELAY_MAX_PAYLOAD_LEN,
        });
    }
    let mut session_id = [0u8; 16];
    session_id.copy_from_slice(&input[..16]);
    let payload = input[RELAY_FRAME_HEADER_LEN..].to_vec();
    Ok(RelayFrame {
        session_id,
        channel,
        payload,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayControl {
    Register { host_id: String, token: String },
    Registered { host_id: String },
    Connect { host_id: String, token: String },
    Incoming { session_id: String },
    Accept { session_id: String },
    Connected,
    Ping,
    Pong,
    Error { code: String, message: String },
}

pub type RelayControlMessage = RelayControl;
pub type RelayMessage = RelayControl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_token_matches_golden_vector() {
        let secret = b"test-secret";
        let host_id = "host-1";
        let token = relay_token(secret, host_id);
        assert_eq!(
            token,
            "a8c4175edecd9485186f266e17dd0670bc5f5f13799b361e7b32fa80a6532862"
        );
        assert!(relay_token_matches(secret, host_id, &token));
    }

    #[test]
    fn relay_token_rejects_wrong_secret() {
        let secret = b"test-secret";
        let wrong_secret = b"wrong-secret";
        let host_id = "host-1";
        let token = relay_token(secret, host_id);

        assert!(!relay_token_matches(wrong_secret, host_id, &token));
        assert!(!relay_token_matches(secret, "host-2", &token));
        assert!(!relay_token_matches(secret, host_id, "invalid-token"));
    }

    #[test]
    fn relay_frame_roundtrip_tcp_and_udp() {
        let session_id = [42u8; 16];
        let payload = b"hello relay world";

        for channel in [RELAY_CHANNEL_TCP, RELAY_CHANNEL_UDP] {
            let encoded =
                encode_relay_frame(&session_id, channel, payload).expect("encoding succeeds");
            assert_eq!(encoded.len(), RELAY_FRAME_HEADER_LEN + payload.len());
            assert_eq!(&encoded[..16], &session_id);
            assert_eq!(encoded[16], channel);
            assert_eq!(&encoded[17..], payload);

            let decoded = decode_relay_frame(&encoded).expect("decoding succeeds");
            assert_eq!(decoded.session_id, session_id);
            assert_eq!(decoded.channel, channel);
            assert_eq!(decoded.payload, payload);
        }
    }

    #[test]
    fn relay_frame_rejects_short_header_bad_channel_and_oversize_payload() {
        let session_id = [1u8; 16];
        let bad_channel = 0x03;

        assert!(encode_relay_frame(&session_id, bad_channel, b"test").is_err());

        let short_bytes = vec![0u8; 16];
        assert!(decode_relay_frame(&short_bytes).is_err());

        let mut bad_channel_frame = vec![0u8; 17];
        bad_channel_frame[16] = bad_channel;
        assert!(decode_relay_frame(&bad_channel_frame).is_err());

        let oversize_len = RELAY_MAX_PAYLOAD_LEN + 1;
        let oversize_payload = vec![0u8; oversize_len];
        assert!(encode_relay_frame(&session_id, RELAY_CHANNEL_TCP, &oversize_payload).is_err());

        let mut oversize_encoded = vec![0u8; RELAY_FRAME_HEADER_LEN + oversize_len];
        oversize_encoded[16] = RELAY_CHANNEL_TCP;
        assert!(decode_relay_frame(&oversize_encoded).is_err());
    }

    #[test]
    fn relay_control_json_roundtrip() {
        let cases = vec![
            RelayControl::Register {
                host_id: "host-1".to_string(),
                token: "tok-1".to_string(),
            },
            RelayControl::Registered {
                host_id: "host-1".to_string(),
            },
            RelayControl::Connect {
                host_id: "host-1".to_string(),
                token: "tok-1".to_string(),
            },
            RelayControl::Incoming {
                session_id: "sess-1".to_string(),
            },
            RelayControl::Accept {
                session_id: "sess-1".to_string(),
            },
            RelayControl::Connected,
            RelayControl::Ping,
            RelayControl::Pong,
            RelayControl::Error {
                code: "host_not_found".to_string(),
                message: "Host not found".to_string(),
            },
        ];

        for msg in cases {
            let json = serde_json::to_string(&msg).expect("serialize json");
            let parsed: RelayControl = serde_json::from_str(&json).expect("deserialize json");
            assert_eq!(parsed, msg);
        }

        let reg = RelayControl::Register {
            host_id: "h1".to_string(),
            token: "t1".to_string(),
        };
        let json = serde_json::to_string(&reg).expect("serialize");
        assert!(json.contains("\"type\":\"register\""));
        assert!(json.contains("\"host_id\":\"h1\""));
        assert!(json.contains("\"token\":\"t1\""));
    }

    #[test]
    fn public_connect_code_roundtrip_and_legacy_absence() {
        let route = RelayConnectCode {
            host_id: "my-host_1".into(),
            url: "https://relay.example:443".into(),
        };
        assert_eq!(RelayConnectCode::parse(&route.encode()), Some(route));
        for legacy in ["192.168.1.2", "host.local:19730", ""] {
            assert_eq!(RelayConnectCode::parse(legacy), None);
        }
        for invalid in [
            "ferryx-rd-relay:@https://relay.example",
            "ferryx-rd-relay:bad/id@https://relay.example",
            "ferryx-rd-relay:host@file:///etc/passwd",
            "ferryx-rd-relay:host@https://user:secret@relay.example",
            "ferryx-rd-relay:host@https://relay.example#12345678",
            "ferryx-rd-relay:host@https://relay.example\nsecret",
        ] {
            assert_eq!(RelayConnectCode::parse(invalid), None);
        }
    }

    #[test]
    fn relay_host_id_rejects_empty_and_illegal_chars() {
        assert!(!relay_host_id_ok(""));
        assert!(relay_host_id_ok("valid-host_123.test"));
        assert!(!relay_host_id_ok("has space"));
        assert!(!relay_host_id_ok("has/slash"));
        assert!(!relay_host_id_ok("has@at"));
        assert!(!relay_host_id_ok(&"a".repeat(129)));
        assert!(relay_host_id_ok(&"a".repeat(128)));
    }
}
