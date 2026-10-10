# Handover Live-State Integration Map: Unlinked Producers

**Verdict:** BLOCK (Unlinked live producers: VT parser, resize lease, writer-paste, glyph/Kitty)
**Authority:** Read-only architecture deliverable | **Verifier:** Parent sole remote verifier (old8b3 cancelled; tests UNRUN)
**Workcopy Baseline:** `/tmp/ferryx-superlogical-handover-state/workcopy` (Base `9c07425a`, zero commits/builds)

## 1. Actual Export & Import Handover Call Sites
- **Predecessor Export Site:** `src-tauri/src/daemon/server.rs:4131` (`handle_transfer_sessions`) -> `pty_manager.export_session_for_handover_async(&session_id, &transfer_id)` (`pty.rs:913`) -> `session.rs:1776` (`export_for_transfer_with_freeze`) -> `session.rs:1897` (`quiesce_input_output_async`). Builds `PtySessionSnapshot`.
- **Successor Import Site:** `src-tauri/src/daemon/server.rs:2663` (`accept_and_receive_transferred_sessions`) -> `handover_socket.rs:395-459` (`receive_and_validate_transferred_sessions`) -> `server.rs:2828` -> `pty_manager.adopt_transferred_session(master, snapshot)` (`pty.rs:1072`) -> `session.rs:604` (`PtySession::adopt`). Atomic activation unpauses reader/writer fences at `server.rs:2852` (`session.activate_adopted()`).

## 2. Unlinked Live Producer Integration Matrix

### Producer 1: Live Ghostty VT Parser State
- **Wiring Ownership & Source:** Native Terminal / Wave 0 (`snapshot_codec.rs` baseline SHA-256: `4bcae1173f32fe28934bc439f15ccab5406b7ad9d649e4a0fa762ff779755dde`, wave0: `49bcd413d1206bf814f410cb4c394471791fb5160b7de53bf22c4c39d1c49f5a`, v4: `7e5ceb1781cd715b7936014bab60e9b6c02bd4b8d758d95a867115c5acc666ee`).
- **Required Owner APIs:**
  - *Export:* `encode_terminal_snapshot_to_writer(&term, writer)` -> emits `GHOSTSNP` magic bytes capturing dual screens, lines, cursor, continuation bounds.
  - *Import:* `decode_terminal_snapshot(&bytes, opts)` with `GhosttyAllocator` vtable enforcing 32 MiB heap cap (`allocator.h`) and `ghostty_terminal_set` scrollback overrides.
- **Schema Mapping:** `AuthoritativeVtSnapshot` (`session.rs:227`) -> `base_snapshot` (`GHOSTSNP` wire), `primary_history_rows`, `alternate_history_rows`, `is_alternate_screen`, `cursor_position`, `continuation_max_bytes` (clamped <=32 MiB). DTO holds wire bytes; live state requires binding to `DecodedTerminal`.
- **Wiring Call Sites:** Export: `session.rs:1785` must extract live parser bytes instead of `None`. Import: `pty.rs:1095` must construct `DecodedTerminal` and attach to adopted `PtySession`.

### Producer 2: Authenticated Resize Lease Coordinator
- **Wiring Ownership & Source:** Owner `87d` (`resize-lease-forward.patch` `d5d945a1200d71d7db7423d4ec54de78a17cc6d06fb73ce15d2eb297d9e3cf2a`; `resize_lease.rs` `8e99516a0688328b0117b28b6ccd391cc69104e37d0463e7344f1a9941418137`; `service.rs` `9141c5bd7cd06b7829a3578849b648fb9e28a3da5abe19938d288c2904874e75`).
- **Required Owner APIs:**
  - *Export:* `ResizeLeaseManager::export_lease_snapshot(&session_id) -> Option<ResizeLeaseSnapshot>` querying monotonic `Instant` TTL, client class, active geometry.
  - *Import:* `ResizeLeaseManager::adopt_lease_snapshot(snapshot: ResizeLeaseSnapshot)` reseeding monotonic clock and restoring debounced geometry without PTY resize storm.
- **Schema Mapping:** `ResizeLeaseSnapshot` (`session.rs:308`) -> `lease_id`, `session_id`, `client_id`, `client_class` (`DesktopNativeWindow`=100 > `InteractiveRemoteWeb`=50 > `MobileWeb`=20), `lease_epoch`, `expires_in_millis` (`Instant` delta), `active_geometry` (`cols`, `rows`).
- **Wiring Call Sites:** Export: `pty.rs:920` query `TerminalService::resize_lease_manager`. Import: `server.rs:2840` invoke `adopt_lease_snapshot` prior to output pump.

### Producer 3: Single-Writer Sequence & Staged Paste Coordinator
- **Wiring Ownership & Source:** Owner `writer931` (`writer-paste-forward.patch` `390df2ee4c9553318fed12811f07ff0a49a5166b703880987924dd1a766c75d3`; `writer_fenced_input.rs` `8920b9daf71e5e78e0db5ec7472fd20c2f73b8d385339371997f5e18d8debb70`).
- **Required Owner APIs:**
  - *Export:* `WriterCoordinator::export_fenced_state(&session_id) -> Option<WriterFencedStateSnapshot>` capturing active writer/epoch, sequence, gap frames, uncommitted staged paste.
  - *Import:* `WriterCoordinator::adopt_fenced_state(snapshot: WriterFencedStateSnapshot)` restoring writer epoch, sequence barrier, and staged paste buffer without de-staging or PTY drop.
- **Schema Mapping:** `WriterFencedStateSnapshot` (`session.rs:341`) -> `session_id`, `active_writer`, `active_epoch`, `last_accepted_seq`, `gap_frames` (<=512 KiB); `active_paste: Option<StagedPasteSnapshot>` (`session.rs:374`) -> `paste_id`, `staged_bytes` (<=512 KiB), `bracketed`.
- **Wiring Call Sites:** Export: `session.rs:1897` (`quiesce_input_output_async`) extract coordinator state. Import: `server.rs:2845` install before `activate_adopted()`.

### Producer 4: Supplementary Glyph Glossary & Kitty Graphics
- **Wiring Ownership & Source:** Native Terminal / Glyph State (`glyph-glossary-forward.patch` `d5253ee9cc59936c11fa5349406ce2d75c845ff1a62407ddeea9fd8e34fc8db9`; `glossary.zig` `1c2e1383c179bc292d7d490eaa667922f584d35d95f5f74d34196d2a11925faa`; `snapshot.zig` lines 1208-1270).
- **Required Owner APIs:**
  - *Export:* `AtomicTerminalSnapshot::encode` -> emits `GLOSSSNP` v1 envelope (<=1024 entries). Kitty images: intentionally omitted by upstream Zig encoder; grid preserves `U+10EEEE`.
  - *Import:* `decode_composite` -> validates `GLOSSSNP` v1 magic/endianness, unpacks PUA glyphs into `Glossary`. Kitty image placeholders preserved without crashing.
- **Schema Mapping:** `SupplementaryTerminalState` (`session.rs:271`) -> `glyph_snapshot` (`GLOSSSNP` wire <=1024 entries), `kitty_snapshot` (`GHOSTIMG` wire <=4 MiB).
- **Wiring Call Sites:** Export: `session.rs:1795` serialize glossary state. Import: `pty.rs:1105` restore glossary via `import_glossary_snapshot`.

## 3. Exact Sourcechain Hashes (SHA-256)
- `handover-state-forward.patch`: `c2bb5d4380946001a588b30bbefdd43656a4681079bf4b144830ad36d8edd424`
- `workcopy/terminal/session.rs`: `db235db173aebd686e93f12509f2febf273afaf2f4fe342e54c9b51d046d57ed`
- `workcopy/terminal/pty.rs`: `80f72823ae9bf08a8ebdaa765c930272eaf5c6d753a03fa6b7abf20f81d86965`
- `workcopy/terminal/service.rs`: `441d9e085d8ae70da93109efbceac732c98e906a2cb6fc272920868e47570c2e`
- `workcopy/daemon/server.rs`: `164e28996b8d2674ae58103085b9619b225ba9f7d08018cee4d66cdb6e479568`
- `workcopy/daemon/handover_socket.rs`: `b0c9181aefa102dbcd6d9aa903207111b026f2c4547b5eeb7d5466de39547c2d`
- `native_terminal/snapshot_codec.rs`: `4bcae1173f32fe28934bc439f15ccab5406b7ad9d649e4a0fa762ff779755dde`
- `resize_lease.rs` (Task 3): `8e99516a0688328b0117b28b6ccd391cc69104e37d0463e7344f1a9941418137`
- `writer_fenced_input.rs` (Wave 1): `8920b9daf71e5e78e0db5ec7472fd20c2f73b8d385339371997f5e18d8debb70`
- `ghostty/snapshot/glossary.zig`: `1c2e1383c179bc292d7d490eaa667922f584d35d95f5f74d34196d2a11925faa`

## 4. Wiring Ownership & Source-Overlap Constraints
- **Three-Way Collision in `terminal/service.rs`:** Handover (`441d9e08`), Resize (`9141c5bd`), and Writer/Paste (`e77f921b`) concurrently mutate `TerminalService`. Direct edits forbidden; Handover must consume exported APIs via `Arc<dyn ...>` trait boundaries or sequenced overlay.
- **Two-Way Collision in `terminal/mod.rs` & `daemon/server.rs`:** Module declarations (`resize_lease`, `writer_fenced_input`) and server request dispatch must be integrated via non-overlapping enum branches.
- **Codec & PTY Overlap in `native_terminal/snapshot_codec.rs` & `pty.rs`:** Wave 0 codec (`49bcd413`), Codec v4 (`7e5ceb17`), and Handover PTY (`80f72823`) must coordinate via `DecodedTerminal` FFI contract without inlining conflicting method definitions.
- **Zero Local Mutation Policy:** Shared tree remains untouched. No builds/tests/GUI/commits executed. All tests UNRUN pending sole remote verifier.
