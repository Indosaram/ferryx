# ferryx-rd-net Knowledge Base

<!-- Score: 16 | Domain: Rust async network transport (Tokio TCP/UDP, STUN/TURN/ICE, congestion control) -->

## OVERVIEW
Asynchronous network transport layer implementing TLS-PSK authenticated TCP signaling, AES-GCM encrypted UDP datagrams, STUN NAT traversal, and rendezvous signaling.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| TLS-PSK TCP Transport | `src/tls_psk.rs` | `TlsPskStream`, `TlsPskListener`, `TlsPskServer` |
| Encrypted UDP Channels | `src/udp_gcm.rs` | `DatagramCipher`, `DatagramError`, `Direction` |
| STUN Server Query & NAT | `src/stun.rs` | `StunClient`, `StunError` |
| Rendezvous Signaling | `src/signaling.rs` | `SignalingClient`, `SessionCandidate` |
| Security Bootstrap & Lockout | `src/tls_psk.rs` | `bootstrap_psk`, `BootstrapLockout` |

## CONVENTIONS
- Network I/O is built entirely on `tokio::net` and async streams.
- UDP datagrams employ authenticated AES-GCM encryption with direction-separated keys and replay attack protection.
- Bootstrap authentication utilizes ephemeral PINs with exponential rate-limiting lockout against brute-force attacks.

## ANTI-PATTERNS
- Do not perform synchronous/blocking socket calls; all network paths must be async `await`.
- Never reuse cryptographic nonces across datagram packets.
- Avoid raw unauthenticated UDP transmissions; all media and control traffic requires AES-GCM datagram encryption.
