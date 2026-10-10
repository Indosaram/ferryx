# Glyph Glossary Snapshot Extension Implementation Report

**Protocol:** Ghostty Terminal Snapshot Extension (`GLOSSSNP` v1)  
**Deliverable Patch:** `glyph-glossary-forward.patch` (SHA-256: `d5253ee9cc59936c11fa5349406ce2d75c845ff1a62407ddeea9fd8e34fc8db9`)  
**Scope:** Shared production READONLY; work isolated in `/tmp/ferryx-superlogical-glyph-state`  
**Verification:** Deterministic suite authored UNRUN (executor `st_…80d`); `git apply -p1 --check` exit code 0

---

## 1. Specification & Remediation Invariants
- **Unicode & PUA Bounds:** Explicit `cp <= 0x10FFFF` check prior to `u21` cast; strict PUA verification prevents truncation wrapping.
- **Reserved Field Validation:** Envelope `flags == 0`, fixed header `reserved0/1/2 == 0`, and `PointWire.reserved == 0` strictly enforced.
- **Strict Data Types:** `on_curve` byte restricted to `{0, 1}`; `f64` padding validated finite, non-NaN, and in range `[0.0, 1.0]`.
- **Contours Invariants:** `points_len == 0` with `contours_len > 0` rejected; contour endpoints must be strictly increasing and end at `points_len - 1`.
- **Duplicate Rejection & Strict EOF:** Duplicate codepoints rejected with `DuplicateCodepoint`; trailing bytes after finish record rejected with `MalformedPayload`.
- **Staging Ownership & Memory Safety:** Scratch `Glossary` stages all records; `ensureUnusedCapacity` precedes infallible insertion, ensuring clean rollback without double-free risk.
- **Portable Endianness:** Removed host struct `memcpy`; explicit little-endian `readInt`/`writeInt` and `f64` float bitcasts used across all fields.
- **Pending APC Capture:** In-flight APC glyph registration sequences in stream continuation reject with reachable `error.PendingRegistrationUncaptured`.

## 2. Full Caller Base + Glossary Atomic Integration
- `AtomicTerminalSnapshot`: Encapsulates base v1 snapshot (`GHOSTSNP`) and versioned glossary extension (`GLOSSSNP`).
- `encode`: Atomically emits base snapshot through `FINISH`, followed by glossary envelope through finish record.
- `decode_composite`: Decodes base snapshot via standard v1 decoder, locates `GLOSSSNP` magic in stream remainder, and transactionally applies glossary registrations.

## 3. Exact File Hashes
| File Path | Baseline SHA-256 | Work SHA-256 |
|---|---|---|
| `src-tauri/vendor/ghostty/src/terminal/snapshot/glossary.zig` | *(new)* | `1c2e1383c179bc292d7d490eaa667922f584d35d95f5f74d34196d2a11925faa` |
| `src-tauri/vendor/ghostty/src/terminal/apc/glyph/Glossary.zig` | `91db2e387dcb0e8da9c4ea81611b9e98396a3c4564f74c0e9f3bfde5d7aeb6b6` | `81fe3b0855c78b663835a8ce124e462afe5c9ea4ed0c5baf68074e8ecd41898d` |
| `src-tauri/vendor/ghostty/src/terminal/snapshot/main.zig` | `98d896cbabd9c7a76fb67bfb6b7f32a90ba329b190b16b01194ed8d174ddb29a` | `ad441db0379872b0c9bd4be78654aac1b00a0a257b93c6a0b7d65d0105dd0103` |
| `src-tauri/vendor/ghostty/src/terminal/c/snapshot.zig` | `b637a933ccdb9cfa204a4f638db4fb6d3f9644ac0747a4e9c6fed46cbec9f8c2` | `6716ad6a8b9f2c81521ce0975e6a3b070c248c2c3cf955ea39c5c9b970de2f55` |
| `src-tauri/vendor/ghostty/src/terminal/c/main.zig` | `ba1251be969e09502b4285f687aa0401c353e74f99745dfe1b1866ea24be725c` | `8019a6396f3f6c3a638ceb13ba8190637e819a5dc0125b4d85fc89d4960b587b` |
| `src-tauri/vendor/ghostty/src/lib_vt.zig` | `1b26ea2bdd948d8f941ecb7b5896e906ca20d1f8716184bfb2c364c8ead74a35` | `08c0663a75542a28a7cf727e583527bff2120b32edf9a2ba05b97eda3e0b53c8` |
| `src-tauri/vendor/ghostty/include/ghostty/vt/snapshot.h` | `6fb8e35fb09f47fa37907eb9e6b6ee1e55816437e280a001559a9c7498149597` | `881abbaead14663a1aa4a765871f4f141dd2b7fec4f98f8ed95a07292d256926` |
| `src-tauri/src/native_terminal/sys/constants.rs` | `1bd48c46e277b32d277753696e62c02e55ccc1ebf6d3c0323058d16441511b14` | `8db21bbc4d3369daa6b9b0f362e5ff15aead75605f0cd6161c9e7ceb8647c64a` |
| `src-tauri/src/native_terminal/sys/ffi.rs` | `8d90753d3cb0ebf4e1856aa04a6dc4f7716911636e14e02bc6506e1eff4df5b1` | `274c361c0890c6148e9a43b2202e953cc83af2a4ff968c77b9202b14106f7ddb` |
| `src-tauri/src/native_terminal/snapshot_codec.rs` | `4bcae1173f32fe28934bc439f15ccab5406b7ad9d649e4a0fa762ff779755dde` | `d5d414faeed415ade210a60ac602c18d11f4c16886a6de8c977e28d59c402e3d` |
| `src-tauri/tests/glyph_glossary_snapshot_contract.rs` | *(new)* | `ce02da296abd86f85e4f09d2da111e7483c0e62550e6a3a9ed2c7d96c0c9d982` |

## 4. Authored UNRUN Tests
- `test_glyph_glossary_capability_and_envelope_validation`
- `test_glyph_glossary_empty_roundtrip`
- `test_glyph_glossary_registration_roundtrip`
- `test_glyph_glossary_transactional_unchanged_on_failure`
- `test_glyph_glossary_malformed_limits_rejection`
- `test_glyph_glossary_atomic_composite_envelope_integration`
