# MahoRD Wire Protocol Version 3

This document is the implementation contract for MahoRD protocol version 3. It defines the bytes exchanged by a client and host. A conforming implementation must not depend on Swift object layout, native struct padding, or host byte order.

## 1. Conventions

- All integer fields are unsigned unless marked signed.
- All multibyte integers are little-endian unless a field explicitly says big-endian.
- `f32` means an IEEE 754 binary32 value. Its 32-bit bit pattern is encoded little-endian.
- Strings are UTF-8 and aren't NUL-terminated.
- Byte ranges use inclusive offsets. For example, `0..3` is four bytes.
- Packet payload offsets begin at zero, after the 12-byte `PacketHeader`.
- A receiver must reject unknown enum values where this document defines a closed enum.
- A receiver may accept trailing bytes unless a message-specific rule says otherwise. Senders must emit only the defined bytes.
- TCP carries control-plane packets. UDP carries media and cursor packets after session keys are installed.

Default ports are TCP 19730 and UDP 19731. Bonjour discovery uses service type `_mahord._tcp` in domain `local.`.

## 2. Common packet envelope

Every protocol packet starts with the following 12-byte header.

### 2.1 `PacketHeader`, 12 bytes

| Offset | Size | Type | Field | Value or meaning |
|---:|---:|---|---|---|
| 0..1 | 2 | `u16 LE` | `magic` | Always `0xEC1D`, encoded as bytes `1d ec` |
| 2 | 1 | `u8` | `type` | A `PacketType` value from the table below |
| 3..6 | 4 | `u32 LE` | `sequence` | Packet sequence number. UDP increments this per emitted packet. TCP callers commonly use zero |
| 7..10 | 4 | `u32 LE` | `timestamp` | Milliseconds since the Unix epoch, truncated modulo 2^32. Handshake and pairing packets commonly use zero |
| 11 | 1 | `u8` | `flags` | Reserved. Send zero. Ignore unknown bits on receipt |

A receiver must have at least 12 bytes, verify the magic, and verify that `type` is known before dispatching the payload.

Example header for a ping packet with sequence 1, timestamp 2, and flags 0:

```text
1d ec 07 01 00 00 00 02 00 00 00 00
```

### 2.2 `PacketType`

| Value | Name | Transport and direction | Payload |
|---:|---|---|---|
| 0 | `handshake` | TCP, client to host | `HandshakePayload` |
| 1 | `handshakeAck` | TCP, host to client | `HandshakePayload` describing the host |
| 2 | `frameHeader` | UDP, host to client | `FrameHeaderPayload` |
| 3 | `frameChunk` | UDP, host to client | `FrameChunkPayload` |
| 4 | `cursorUpdate` | UDP, host to client | `CursorUpdate` |
| 5 | `inputEvent` | TCP, client to host | `InputEventPayload` |
| 6 | `control` | TCP, either direction | `ControlMessage` |
| 7 | `ping` | Reserved packet-level ping | No defined payload. Session heartbeat uses control types 4 and 5 |
| 8 | `audioFrame` | UDP, host to client | Audio fragment payload |
| 9 | `pairingRequest` | TCP, client to host, bootstrap channel only | `PairingRequestPayload` |
| 10 | `pairingGrant` | TCP, host to client, bootstrap channel only | `PairingGrantPayload` |
| 11 | `pairingReject` | TCP, host to client, bootstrap channel only | `PairingRejectPayload` |

## 3. TCP transport and framing

TCP is protected by TLS-PSK. Inside TLS, each packet is framed as follows:

| Offset | Size | Type | Field | Meaning |
|---:|---:|---|---|---|
| 0..3 | 4 | `u32 LE` | `packetLength` | Number of bytes following the prefix |
| 4.. | `packetLength` | bytes | `packet` | One complete `PacketHeader || payload` packet |

`packetLength` must be from 1 through 16,777,216 inclusive. A receiver must buffer across partial TCP reads and must split coalesced frames. A zero length or a value above 16 MiB is invalid. The reference receiver drops its complete pending receive buffer when it reads an invalid length.

The reference implementation requests TCP receive chunks of at most 262,144 bytes. That read size isn't a wire requirement.

Example framing for the 12-byte sample ping header above:

```text
0c 00 00 00  1d ec 07 01 00 00 00 02 00 00 00 00
```

## 4. TLS-PSK identity and pairing flow

### 4.1 TLS requirements

TCP connections use a PSK-only TLS channel with no certificate authentication. Both peers must support TLS 1.2. They may negotiate TLS 1.3 when their TLS stacks support external PSKs compatibly.

A Rust implementation using OpenSSL must enable PSK callbacks and TLS 1.2 PSK AES-GCM suites. At minimum, configure:

```text
PSK-AES128-GCM-SHA256
PSK-AES256-GCM-SHA384
```

Equivalent IANA names are `TLS_PSK_WITH_AES_128_GCM_SHA256` and `TLS_PSK_WITH_AES_256_GCM_SHA384`. Certificate verification isn't part of peer authentication on this PSK-only channel. Authentication comes from possession of the selected PSK and the TLS transcript.

The offered identity selects the key:

| Connection kind | PSK identity | PSK bytes |
|---|---|---|
| First-time bootstrap | ASCII `maho-b1` | Bootstrap PSK derived from the 8-digit PIN |
| Previously paired | UTF-8 `maho-p1.<uuid>` | Raw 32-byte pairing key |

`<uuid>` is the exact pairing ID issued in `PairingGrantPayload`. Treat it as an opaque UTF-8 string even though the host currently generates a UUID string.

### 4.2 Bootstrap PSK derivation

The PIN is exactly eight decimal digits, including any leading zeroes. Derive the bootstrap PSK in two stages:

1. `stretched = PBKDF2-HMAC-SHA256(password=UTF8(pin), salt=UTF8("erd/bootstrap/v3"), iterations=600000, outputLength=32)`
2. `bootstrapPSK = HKDF-SHA256(ikm=stretched, salt=UTF8("erd/bootstrap/v3"), info=UTF8("erd/tls-psk"), outputLength=32)`

HKDF means RFC 5869 Extract followed by Expand. An empty HKDF salt, where used by a generic implementation, means 32 zero bytes for SHA-256 extraction.

The bootstrap PIN expires after 300 seconds. Five failed bootstrap TLS authentications within a 60-second window lock the bootstrap path for 300 seconds. Starting a fresh pairing window resets that lockout in the reference host.

### 4.3 First-time pairing sequence

1. The host opens a pairing window and offers PSK identity `maho-b1` with the PIN-derived bootstrap PSK.
2. The client connects over TLS using identity `maho-b1` and the same derived key.
3. Once TLS is ready, the client sends `PacketType.pairingRequest`.
4. The host asks its user to approve the named client. No screen, input, or media capability may be granted on the bootstrap connection before approval.
5. On approval, the host generates a random 32-byte pairing key, persists it under a new pairing ID, and sends `pairingGrant`.
6. On denial or unavailable pairing, the host sends `pairingReject`.
7. The client persists the grant. It then sends the normal v3 `handshake` using the granted pairing ID and a fresh 16-byte session salt. The current implementation can send this handshake on the still-encrypted bootstrap connection.
8. Later TCP connections use identity `maho-p1.<pairingID>` and the raw 32-byte pairing key.

The host must cross-check the `pairingID` in the application handshake against its pairing store. A successful TLS channel alone doesn't authorize screen or input traffic. The host must also receive a valid v3 handshake with a known pairing ID and a 16-byte session salt.

## 5. Handshake

A v3 sender must emit the full layout below. The variable offsets are based on `nameLen = N` and `pairingIdLen = P`.

### 5.1 `HandshakePayload`

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0..1 | 2 | `u16 LE` | `nameLen` | Number of hostname UTF-8 bytes. Receiver cap is 1024 |
| 2..`2+N-1` | N | UTF-8 | `hostname` | Display hostname |
| `2+N`..`3+N` | 2 | `u16 LE` | `screenWidth` | Logical screen width. Client sends 0 in its request |
| `4+N`..`5+N` | 2 | `u16 LE` | `screenHeight` | Logical screen height. Client sends 0 in its request |
| `6+N`..`9+N` | 4 | `f32 LE` | `scaleFactor` | Display scale. Client currently sends 1.0 |
| `10+N` | 1 | `u8` | `protocolVersion` | Must equal 3 |
| `11+N`..`18+N` | 8 | `u64 LE` | `capabilities` | Capability bit set |
| `19+N`..`20+N` | 2 | `u16 LE` | `pairingIdLen` | Pairing ID byte count, at most 256 |
| `21+N`..`20+N+P` | P | UTF-8 | `pairingID` | Client's granted pairing ID. Host acknowledgement normally sends an empty string |
| `21+N+P`..`36+N+P` | 16 | bytes | `sessionSalt` | Client-generated cryptographic random salt. The host acknowledgement may omit it |

The fixed v3 overhead is 37 bytes plus the hostname and pairing ID. A client handshake must contain exactly 16 session salt bytes. The serializer takes at most the first 16 supplied bytes, so callers must generate and pass exactly 16 bytes.

The decoder retains legacy shape tolerance: after width, height, and scale, it treats a missing v3 tail as protocol version 1 with no capabilities, pairing ID, or salt. Session establishment must reject that result because v3 requires `protocolVersion == 3`.

### 5.2 Capability bits

| Bit | Mask | Name | Meaning |
|---:|---:|---|---|
| 0 | `0x0000000000000001` | `streamConfiguration` | Peer supports stream configuration control types 7 through 10 |
| 1 | `0x0000000000000002` | `clipboardSync` | Peer supports the original clipboard synchronization capability |
| 2 | `0x0000000000000004` | `textClipboardSync` | Peer supports UTF-8 text clipboard messages, control types 11 through 13 |

Unknown capability bits must be preserved when practical and ignored when deciding known features.

## 6. Pairing payloads

### 6.1 Pairing request

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0..1 | 2 | `u16 LE` | `nameLen` | Client hostname byte count, receiver cap 1024 |
| 2..`1+N` | N | UTF-8 | `hostname` | Client display name |

The sender's storage format can represent up to 65,535 bytes, but conforming senders must stay within the receiver's 1024-byte cap.

Example for hostname `client-mac`:

```text
0a 00 63 6c 69 65 6e 74 2d 6d 61 63
```

### 6.2 Pairing grant

Let `I` be `idLen` and `N` be `nameLen`.

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0 | 1 | `u8` | `idLen` | Pairing ID byte count, at most 255 |
| 1..`I` | I | UTF-8 | `pairingID` | Opaque identifier used in future handshakes and PSK identities |
| `1+I`..`2+I` | 2 | `u16 LE` | `nameLen` | Host display-name byte count |
| `3+I`..`2+I+N` | N | UTF-8 | `hostName` | Name the client should persist |
| `3+I+N` | 1 | `u8` | `keyLen` | Must equal 32 |
| `4+I+N`..`35+I+N` | 32 | bytes | `key` | Random 256-bit pairing key |

A receiver must reject any `keyLen` other than 32.

### 6.3 Pairing reject

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0 | 1 | `u8` | `reason` | Pairing rejection reason |

| Value | Name | Meaning |
|---:|---|---|
| 0 | `deniedByHost` | Host user denied the request |
| 1 | `lockedOut` | Bootstrap authentication is locked after repeated failures |
| 2 | `pairingDisabled` | No active pairing window or no approval handler |

## 7. UDP encryption and datagram layout

After the authenticated client handshake, both peers derive direction-specific UDP ciphers from the 32-byte pairing key and the client's 16-byte `sessionSalt`.

### 7.1 Datagram bytes

An encrypted UDP datagram is:

```text
PacketHeader || nonce || ciphertext || tag
```

| Datagram offset | Size | Field | Protection |
|---:|---:|---|---|
| 0..11 | 12 | `PacketHeader` | Plaintext, authenticated as AES-GCM AAD |
| 12..23 | 12 | AES-GCM nonce | Plaintext |
| 24..`23+C` | C | Ciphertext | Encryption of the packet payload only |
| `24+C`..`39+C` | 16 | AES-GCM tag | Authentication tag |

`AAD` is exactly the serialized 12-byte header as transmitted. Any header change must make authentication fail. The decrypted result is dispatched as if the datagram were `PacketHeader || plaintextPayload`.

The protocol's nominal packet budget is 1,400 bytes for header plus plaintext payload. Thus `maxPayloadSize` is 1,388 bytes. Encryption adds 28 bytes, so an encrypted wire datagram can reach 1,428 bytes. Implementations must not subtract nonce and tag bytes from the documented video and audio plaintext chunk caps.

A one-byte UDP datagram containing `ff` is used as an unencrypted endpoint-registration ping. It isn't a protocol packet and must not enter packet dispatch.

### 7.2 Direction key derivation

First derive an intermediate key:

```text
udpIKM = HKDF-SHA256(
    ikm = pairingKey,
    salt = sessionSalt,
    info = UTF8("erd/udp-ikm/v3"),
    outputLength = 32
)
```

Then derive one traffic key and nonce prefix per direction. For client to host:

```text
c2hKey = HKDF-SHA256(udpIKM, sessionSalt, UTF8("erd/udp-c2h/v3"), 32)
c2hNoncePrefix = HKDF-SHA256(udpIKM, sessionSalt, UTF8("erd/udp-c2h/v3/nonce"), 4)
```

For host to client:

```text
h2cKey = HKDF-SHA256(udpIKM, sessionSalt, UTF8("erd/udp-h2c/v3"), 32)
h2cNoncePrefix = HKDF-SHA256(udpIKM, sessionSalt, UTF8("erd/udp-h2c/v3/nonce"), 4)
```

The positional HKDF arguments in those short forms are `ikm, salt, info, outputLength`.

### 7.3 Nonce and replay rules

| Nonce offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | bytes | Direction-specific derived nonce prefix |
| 4..11 | 8 | `u64 BE` | Packet counter |

Each cipher starts its send counter at zero and increments before sealing, so its first transmitted counter is 1. Counter wrap must never cause nonce reuse. A production implementation must end the session before wrap.

Receivers keep a sliding replay window of 4,096 counter values per direction. Reject a datagram before decryption if its counter is at least 4,096 behind the highest authenticated counter, or if that exact counter was already authenticated. Mark a counter as seen only after AES-GCM authentication succeeds.

## 8. Video media

Video is sent from host to client over encrypted UDP. A frame begins with one `frameHeader` packet followed by `frameChunk` packets. Packets can arrive out of order.

### 8.1 `FrameHeaderPayload`, 16 bytes

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0..3 | 4 | `u32 LE` | `frameId` | Frame identifier |
| 4..5 | 2 | `u16 LE` | `width` | Encoded frame width in pixels |
| 6..7 | 2 | `u16 LE` | `height` | Encoded frame height in pixels |
| 8 | 1 | `u8` | `isKeyFrame` | 0 means false, any nonzero value means true |
| 9..10 | 2 | `u16 LE` | `totalChunks` | Number of chunks, at most 1,024 |
| 11 | 1 | `u8` | `padding` | Sender writes zero. Receiver ignores it |
| 12..15 | 4 | `u32 LE` | `totalSize` | Exact assembled frame size, at most 33,554,432 bytes |

Reject `totalChunks > 1024` or `totalSize > 33,554,432`. Receivers should also reject internally inconsistent frames, such as chunk indices outside `0..totalChunks-1`, duplicate chunks with conflicting data, or an assembled byte count different from `totalSize`.

### 8.2 `FrameChunkPayload`

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0..3 | 4 | `u32 LE` | `frameId` | Must match a pending frame header |
| 4..5 | 2 | `u16 LE` | `chunkIndex` | Zero-based chunk index |
| 6.. | 0..1382 | bytes | `chunkData` | Consecutive bytes of the encoded frame |

The plaintext packet payload cap is 1,388 bytes. Subtracting the 6-byte chunk prefix leaves 1,382 bytes of video data. The sender computes:

```text
totalChunks = ceil(frameByteLength / 1382)
```

It sends indices from 0 upward and splits the frame into consecutive 1,382-byte slices. No frame may use more than 1,024 chunks. The independent assembled-frame cap is 32 MiB. Current chunking reaches the 1,024-chunk limit before the 32 MiB cap, but receivers must enforce both because wire fields can claim either value.

The reference receiver expires an incomplete frame after 1 second.

### 8.3 HEVC elementary stream format

Each assembled frame is HEVC in AVCC length-prefixed form, not Annex B start-code form. It contains one or more NAL units:

| Offset within each NAL record | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 BE` | `naluLength` |
| 4..`3+naluLength` | `naluLength` | bytes | Complete HEVC NAL unit, including its NAL header |

Records are concatenated with no padding. A decoder must bounds-check every big-endian length before reading the NAL unit.

On keyframes, the host prepends the HEVC parameter sets from the encoder format description, normally VPS, SPS, and PPS, each as its own 4-byte big-endian length plus NAL bytes. The encoded keyframe NAL records follow them. A client may use these parameter sets to create or refresh its decoder session.

## 9. Audio media

Audio is PCM at 48,000 Hz, stereo, interleaved 32-bit IEEE 754 float samples. One stereo sample frame is 8 bytes:

```text
left f32 native sample bytes || right f32 native sample bytes
```

The v3 wire contract treats those float bytes as little-endian IEEE 754. A big-endian implementation must byte-swap each sample.

Each captured audio block gets a `frameId` and is split across encrypted UDP packets of type `audioFrame`.

### 9.1 Audio fragment payload

| Offset | Size | Type | Field | Rule |
|---:|---:|---|---|---|
| 0..3 | 4 | `u32 LE` | `frameId` | Audio block identifier |
| 4..5 | 2 | `u16 LE` | `fragIdx` | Zero-based fragment index |
| 6..7 | 2 | `u16 LE` | `fragCount` | Total fragment count, at least 1 |
| 8.. | 0..1380 | bytes | `data` | Consecutive PCM bytes |

The 1,388-byte plaintext payload cap minus the 8-byte fragment prefix gives a 1,380-byte data cap. Senders compute `fragCount = ceil(audioByteLength / 1380)` and require it to fit in `u16`. Receivers require `fragCount >= 1` and `fragIdx < fragCount`, then concatenate fragments in index order.

For compatibility, the current client plays an `audioFrame` payload shorter than 8 bytes as raw PCM. New v3 senders must always use the fragment header.

## 10. Input events

Input travels from client to host over TLS/TCP as packet type `inputEvent`.

### 10.1 `InputEventPayload`, 21 bytes

| Offset | Size | Type | Field | Meaning |
|---:|---:|---|---|---|
| 0 | 1 | `u8` | `type` | `InputEventType` |
| 1..4 | 4 | `f32 LE` | `x` | Normalized horizontal position |
| 5..8 | 4 | `f32 LE` | `y` | Normalized vertical position, top is 0 |
| 9..10 | 2 | `u16 LE` | `keyCode` | Platform key code. Zero for events without a key |
| 11..12 | 2 | `u16 LE` | `modifiers` | Modifier bit set |
| 13..16 | 4 | `f32 LE` | `scrollDX` | Horizontal scroll delta, otherwise zero |
| 17..20 | 4 | `f32 LE` | `scrollDY` | Vertical scroll delta, otherwise zero |

### 10.2 Input event types

| Value | Name |
|---:|---|
| 0 | `mouseMove` |
| 1 | `leftMouseDown` |
| 2 | `leftMouseUp` |
| 3 | `rightMouseDown` |
| 4 | `rightMouseUp` |
| 5 | `scrollWheel` |
| 6 | `keyDown` |
| 7 | `keyUp` |
| 8 | `flagsChanged` |
| 9 | `leftMouseDragged` |
| 10 | `rightMouseDragged` |

### 10.3 Modifier bits

| Bit | Mask | Name |
|---:|---:|---|
| 0 | 1 | Shift |
| 1 | 2 | Control |
| 2 | 4 | Option or Alt |
| 3 | 8 | Command or Meta |
| 4 | 16 | `capsLock` or Caps Lock |

### 10.4 Coordinate normalization

Given a pointer position `(localX, localY)` in a client view of size `(viewWidth, viewHeight)`, where the local coordinate system has its origin at the bottom left, encode:

```text
x = clamp(localX / viewWidth, 0, 1)
y = 1 - clamp(localY / viewHeight, 0, 1)
```

The host maps normalized values into its logical screen coordinates:

```text
hostX = x * logicalScreenWidth
hostY = y * logicalScreenHeight
```

Clients with a top-left local origin should encode `y = clamp(localY / viewHeight, 0, 1)` instead of flipping twice. Always clamp both axes to `[0, 1]` before sending.

## 11. Control messages

Control packets travel over TLS/TCP with `PacketType.control`. Their packet payload begins with a one-byte control type:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0 | 1 | `u8` | `controlType` |
| 1.. | variable | bytes | Type-specific payload |

### 11.1 Control type table

| Value | Name | Direction | Type-specific payload |
|---:|---|---|---|
| 0 | `requestKeyFrame` | Client to host | Empty |
| 1 | `startStream` | Client to host | Empty |
| 2 | `stopStream` | Client to host | Empty |
| 3 | `disconnect` | Either | Empty |
| 4 | `ping` | Host to client | Empty |
| 5 | `pong` | Client to host | Empty |
| 6 | `bitrateAdjust` | Client to host | `BitrateAdjustPayload`, 4 bytes |
| 7 | `streamConfigRequest` | Client to host | `StreamConfigurationRequestPayload`, 18 bytes |
| 8 | `streamConfigResponse` | Host to client | `StreamConfigurationResponsePayload`, 18 bytes |
| 9 | `streamConfigReject` | Host to client | `StreamConfigurationRejectPayload` |
| 10 | `streamConfigError` | Host to client | `StreamConfigurationErrorPayload` |
| 11 | `clipboardSyncRequest` | Either | `ClipboardSyncRequestPayload`, 6 bytes |
| 12 | `clipboardSyncUpdate` | Either | `ClipboardSyncUpdatePayload` |
| 13 | `clipboardSyncError` | Either | `ClipboardSyncErrorPayload` |

A sender must not attach a payload to the empty forms. A receiver should ignore an unexpected payload on an otherwise known empty control type.

### 11.2 Bitrate adjustment

Offsets here and below are relative to the bytes after `controlType`.

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `i32 LE` | `targetBitrate` in bits per second |

The host currently supports a general update call. Stream configuration requests clamp bitrate to 1,000,000 through 20,000,000 bits per second.

### 11.3 Stream configuration

`StreamConfiguration` is 14 bytes:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `width` in pixels |
| 4..7 | 4 | `u32 LE` | `height` in pixels |
| 8..11 | 4 | `u32 LE` | `bitrate` in bits per second |
| 12..13 | 2 | `u16 LE` | `framesPerSecond` |

The request and response layouts are both 18 bytes:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `requestID` |
| 4..17 | 14 | struct | Desired configuration for request, active configuration for response |

A host rejects zero dimensions, dimensions larger than its capture size, zero FPS, or FPS above 120. It clamps requested bitrate to 1,000,000 through 20,000,000.

Reject and error payloads share this byte shape:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `requestID` |
| 4 | 1 | `u8` | `reason` for reject, `errorCode` for error |
| 5..6 | 2 | `u16 LE` | `messageLen = M` |
| 7..`6+M` | M | UTF-8 | `message` |

Stream configuration error codes are:

| Value | Name |
|---:|---|
| 0 | `invalidRequest` |
| 1 | `unsupportedDimensions` |
| 2 | `unsupportedBitrate` |
| 3 | `unsupportedFPS` |
| 4 | `rejectedByPeer` |

### 11.4 Clipboard synchronization

Clipboard directions:

| Value | Name |
|---:|---|
| 0 | `hostToClient` |
| 1 | `clientToHost` |
| 2 | `bidirectional` |

Clipboard origins:

| Value | Name |
|---:|---|
| 0 | `localPasteboard` |
| 1 | `remotePasteboard` |
| 2 | `syncedFromPeer` |

`ClipboardSyncRequestPayload`, 6 bytes:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `requestID` |
| 4 | 1 | `u8` | `direction` |
| 5 | 1 | `u8` | `origin` |

`ClipboardSyncUpdatePayload`:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `requestID` |
| 4 | 1 | `u8` | `direction` |
| 5 | 1 | `u8` | `origin` |
| 6..7 | 2 | `u16 LE` | `textLen = T` |
| 8..`7+T` | T | UTF-8 | `text`, at most 4,096 bytes |

`ClipboardSyncErrorPayload`:

| Offset | Size | Type | Field |
|---:|---:|---|---|
| 0..3 | 4 | `u32 LE` | `requestID` |
| 4 | 1 | `u8` | `direction` |
| 5 | 1 | `u8` | `origin` |
| 6 | 1 | `u8` | `errorCode`, application-defined |
| 7..8 | 2 | `u16 LE` | `messageLen = M` |
| 9..`8+M` | M | UTF-8 | `message` |

Text clipboard exchange should only start when both peers advertise capability bit 2.

## 12. Heartbeat and session liveness

The host sends control `ping` every 2 seconds after streaming starts. The client answers each one with control `pong`. The host records the time of the latest pong and disconnects when more than three heartbeat intervals have elapsed, which is more than 6 seconds.

Heartbeat messages have no payload. They use packet type `control`, not packet type `ping`.

## 13. Cursor update

Cursor updates use encrypted UDP packet type `cursorUpdate` and a 9-byte payload.

| Offset | Size | Type | Field | Meaning |
|---:|---:|---|---|---|
| 0..3 | 4 | `f32 LE` | `x` | Normalized horizontal position |
| 4..7 | 4 | `f32 LE` | `y` | Normalized vertical position |
| 8 | 1 | `u8` | `cursorType` | Cursor shape identifier. Zero is the current default |

The host currently computes `x = cursorX / logicalWidth` and `y = cursorY / logicalHeight`. The renderer expects values in `[0, 1]`.

## 14. Signaling through ntfy

Signaling exchanges TCP connection candidates before a direct or NAT-traversed TLS connection. It doesn't replace TLS authentication. Both peers must still use the PIN-derived bootstrap PSK after signaling.

### 14.1 Topic and payload key derivation

Let `pinBytes = UTF8(pin)`. Derive:

```text
topicSeed = HKDF-SHA256(
    ikm = pinBytes,
    salt = UTF8("erd/signaling/v3"),
    info = UTF8("erd/topic"),
    outputLength = 32
)

topic = "erd3-" + lowercaseHex(topicSeed[0..13])
```

The topic uses 14 derived bytes, or 28 lowercase hexadecimal characters. Including the `erd3-` prefix, its length is 33 characters. The truncation keeps it below ntfy's 64-character topic limit.

Derive the AES key independently:

```text
payloadKey = HKDF-SHA256(
    ikm = pinBytes,
    salt = UTF8("erd/signaling/v3"),
    info = UTF8("erd/payload-key"),
    outputLength = 32
)
```

### 14.2 Candidate JSON

Each candidate is a UTF-8 JSON object with these exact property names:

```json
{
  "role": "client",
  "localIP": "192.0.2.10",
  "localPort": 19730,
  "publicIP": "198.51.100.20",
  "publicPort": 40123
}
```

`role` is currently `client` or `server`. Ports are JSON integers in the `u16` range. IP fields are strings.

### 14.3 Encryption and publication

1. Encode the candidate as UTF-8 JSON.
2. Encrypt it with AES-256-GCM under `payloadKey`, using a fresh random 12-byte nonce and no AAD.
3. Form the standard combined representation `nonce || ciphertext || 16-byte tag`.
4. Base64-encode that combined byte string using standard Base64 with padding.
5. POST the Base64 text as the raw `text/plain` body to `https://ntfy.sh/<topic>`.
6. Require an HTTP status in the 200 through 299 range.

ntfy exposes that text in the `message` property of its JSON event envelope.

### 14.4 Poll loop

Poll:

```text
GET https://ntfy.sh/<topic>/json?poll=1&since=10m
```

The reference exchange has a total 10-second deadline, uses a 10-second HTTP request timeout, and waits 1 second after an unsuccessful poll before trying again. Each response is newline-delimited JSON. Search events from newest to oldest. For each event:

1. Parse the JSON envelope.
2. Read its string `message` property.
3. Base64-decode it.
4. Parse `nonce || ciphertext || tag` as an AES-GCM combined box.
5. Decrypt with `payloadKey` and no AAD.
6. Decode `SessionCandidate` JSON.
7. Ignore candidates whose `role` equals the local role.
8. Return the first valid candidate for the peer role.

Empty polls, unrelated ntfy events, malformed Base64, authentication failures, malformed candidate JSON, and same-role candidates don't end the loop. Stop only on a valid peer candidate, cancellation, or the overall deadline.

After exchange, the client races TCP connections to the server's local and public endpoints. The first TLS connection that becomes ready wins and the others are cancelled.

## 15. Required validation summary

A conforming v3 implementation must enforce at least these limits:

| Item | Limit |
|---|---:|
| TCP framed packet | 1 through 16,777,216 bytes |
| Packet header | Exactly 12 defined bytes |
| Handshake hostname accepted by reference decoder | 1,024 bytes |
| Handshake pairing ID | 256 bytes |
| Client session salt | Exactly 16 bytes |
| Pairing request hostname accepted by reference decoder | 1,024 bytes |
| Pairing key | Exactly 32 bytes |
| UDP plaintext packet budget | 1,400 bytes including header |
| UDP plaintext payload | 1,388 bytes |
| AES-GCM nonce and tag | 12 bytes and 16 bytes |
| Replay window | 4,096 counters |
| Frame chunks | 1,024 per frame |
| Assembled frame | 33,554,432 bytes |
| Video chunk data | 1,382 bytes |
| Audio fragment data | 1,380 bytes |
| Clipboard text | 4,096 UTF-8 bytes |

Reject malformed or unauthenticated data before allocating from wire-controlled sizes. Authenticate UDP ciphertext before marking replay state or delivering plaintext. Don't allow bootstrap connections to send input, control streaming, or receive media until pairing approval and the authenticated v3 handshake are complete.

## 16. Conformance vectors

The Swift test suite includes the RFC 5869 SHA-256 vectors below. They are useful for checking an independent HKDF implementation.

### 16.1 RFC 5869 Appendix A, test case 1

```text
IKM  = 0b repeated 22 times
salt = 000102030405060708090a0b0c
info = f0f1f2f3f4f5f6f7f8f9
L    = 42
OKM  = 3cb25f25faacd57a90434f64d0362f2a
       2d2d0a90cf1a5a4c5db02d56ecc4c5bf
       34007208d5b887185865
```

### 16.2 RFC 5869 Appendix A, test case 3

```text
IKM  = 0b repeated 22 times
salt = empty
info = empty
L    = 42
OKM  = 8da4e775a563c18f715f802a063c5a31
       b8a11f5c5ee1879ec3454e5f3c738d2d
       9d201395faa4b61a96c8
```

### 16.3 Fixed wire encodings

```text
PacketHeader(type=ping, sequence=1, timestamp=2, flags=0)
= 1dec07010000000200000000

TCP frame containing that header
= 0c0000001dec07010000000200000000

PairingRequestPayload(hostname="client-mac")
= 0a00636c69656e742d6d6163
```
