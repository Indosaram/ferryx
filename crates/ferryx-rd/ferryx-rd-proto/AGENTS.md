# ferryx-rd-proto Knowledge Base

<!-- Score: 20 | Domain: Rust wire protocol v3 definitions, ChaCha20-Poly1305 AEAD, packet codecs, reassembly -->

## OVERVIEW
Pure protocol codec library for MahoRD version 3 wire formats, framing, packet envelopes, control payloads, and input events without I/O dependencies.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| Codec Trait Contract | `src/lib.rs` | `WireCodec`, `CodecError` |
| Packet Envelopes & Types | `src/packet.rs` | `PacketHeader`, `PacketType`, `MAGIC` |
| Length-Prefixed Framing | `src/framing.rs` | `FrameDecoder`, `FrameEncoder` |
| Handshake & Salt Exchange | `src/handshake.rs` | `HandshakeState`, `HandshakePayload` |
| Video/Audio Payloads | `src/media.rs` | `VideoChunk`, `AudioChunk`, `FrameHeader` |
| Input Event Encoding | `src/input.rs` | `InputEvent`, `MouseEvent`, `KeyEvent` |
| TCP Control Signaling | `src/control.rs` | `ControlMessage`, `QualityChange`, `PingPong` |
| Pairing & Identity | `src/pairing.rs` | `PairingRequest`, `PairingResponse` |

## CONVENTIONS
- Pure calculation and codec logic: no network sockets, filesystem I/O, or system timers inside this crate.
- All numbers encoded to wire format must use explicit Big-Endian / Network Byte Order.
- Decoders must validate size bounds and slice boundaries before allocating or slicing data.

## ANTI-PATTERNS
- Never add I/O dependencies (`tokio::net`, `std::net`, file handles) to `ferryx-rd-proto`.
- Do not perform unbounded vector allocations from untrusted packet length headers.
