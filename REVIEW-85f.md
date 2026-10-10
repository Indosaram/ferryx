# REVIEW 85f — mobile input (B64 + single-writer fence) & viewport-selection follow-up patches

Sole verifier runtime: **8b3**. Report 842 is obsolete. Every runtime claim below is **UNRUN**.

## Final artifact (fence round, supersedes receipt b0ae99e6 — stale)

| Patch | Files | sha256 |
|---|---|---|
| `mobile-input-b64-followup.patch` (A) | mirror.rs, protocol.rs, server.rs, RemoteTerminal.tsx, terminalGestureRouter.test.ts, terminalInputTransport.ts | `4e9b3711ac2d6b66e2ccf9886ebaff056be7b3a60d870479f6311818815c650e` |
| `mobile-viewport-selection-followup.patch` (B) | RemoteTerminal.tsx, mobileTextSelection{,.test}.ts, terminalViewportKeyboard{,.test}.ts | `00fede50c7cb11f52146806d22d7169476407c504f7d8aa22bdcec2ec5790c68` |
| `mobile-generation-controllane-followup.patch` (C, layered after A) | server.rs only (bounded control lane + 3 contract tests) | `086b48419ef72ed5083c4edc6c8ddb4adcb3fb120a18722b11032ad6411586d3` |

Chain: `pre-a` → A-tree (`.genA5b`) → `work` (`.genA5b` ⊂ work; B is exactly `work − .genA5b`). Zero overlap: A carries the `inputSeq` hunks in RemoteTerminal so they cancel out of B's diff.

Apply proof (UNRUN, `git apply --check` only, then byte-diff of applied tree vs A-tree/work):

- `A5_CHECK_CLEAN` + `PROOF_A5_BYTE_IDENTICAL`
- `B5_CHECK_CLEAN` + `PROOF_B5_BYTE_IDENTICAL`
- `C_CHECK_CLEAN` + `SRC_TAURI_BYTE_IDENTICAL` + `UI_MATCHES_A_TREE` (pre-a + A + C `src-tauri` byte-equal to `work`; C touches no ui).

Parse checks only (no builds, no tests, no dev server): `rustfmt --emit stdout` exit 0 on server/protocol/mirror; `bun build --no-bundle` PARSE_OK on terminalInputTransport.ts, RemoteTerminal.tsx, terminalGestureRouter.test.ts.

## Round-1/2 receipt lineage (for provenance)

`728e2e89d35e288cb13f62d69aa342d50e75a3f506c170d88e6479c04ea0ae09` (round 1) → `6c4d5636a954c49e80fa89863b39ff3743fe601cc4c5385d6051407d6d89814f` (round 2 supersedes round 1) → this fence round (final). `b0ae99e6…` receipt is stale.

## Patch A — input fence (what changed since round 2)

Single-writer coordinator wired at the two real socket callers (plain + render-grid handlers):

- Registration via `TerminalService::register_terminal_writer(..., None)` per connection; server binds its own epoch; new `ServerControlMessage::WriterEpoch { epoch, client_id }` sent as the registration barrier (clients may ignore it; tests await it).
- B64 arms run `authenticate_client` **first** — viewer (`remote_input_viewer_denied`), stale epoch (`remote_input_stale_epoch`), superseded writer (`remote_input_not_active_writer`) denied with typed status **before decode and before any backend write**; the old `can_control` gate was removed from these arms (fence is sole authority). `remote_input_control_denied` = 0 occurrences.
- Sequence contract: client sends `inputSeq >= 1` (`#[serde(default)] input_seq: u64`, per-variant `rename_all = "camelCase"` so the wire key is `inputSeq`); 0/absent → `remote_input_invalid_sequence` fail-closed at the fence.
- `RemoteTerminal`: `inputSeqRef` increments only **after** encode succeeds (no burned-seq gap), reset on socket open.
- SSH control arm keeps its contract: pattern `RemoteWriteB64 { generation, data, .. }` (seq deliberately ignored there — fixed to match the extended variant).
- mirror `render_modes`: SGR/DECCKM query error → whole `tracking_mode = Unknown` (fail-closed unknown mode; the inert `.unwrap_or(false)` bools fill only after — not parity, accepted debt).
- B64 decode: generation-typed errors + predecode caps retained (bytes + decoded caps before allocation).

Tests added/extended (all real gateway + PTY, no sleeps — WriterEpoch barrier / bounded deadline loops):

- `test_b64_socket_viewer_denied_before_backend_write` — viewer frame denied typed, zero bytes reach the recorded PTY.
- `test_b64_socket_superseded_writer_denied_before_backend_write` — writer A registered, writer B registration supersedes, A denied typed with zero bytes; active B still lands byte-exact input.
- matrix now 8 rejection cases (`zero_seq` added; expected `remote_input_invalid_sequence`); delivery/fail-closed/positive-control frames carry `inputSeq: 1`; pre-existing high-byte `0xff` byte-exact fixture retained.

## Patch B — viewport + selection (unchanged from receipt 86cbace1 round)

visualViewport-driven keyboard resize (no canonical resize on keyboard-only visibility), DOM Selection extension + handle-based grapheme clamping (Intl.Segmenter), mobileTextSelection + terminalViewportKeyboard with tests.

## Notes / debts carried

- Generation on the wire is a **string** (frontend canonical DTO is string) — deliberate; not a u64 number.
  Issuance is conditional: `session_backend.recovery()` → `recovery_message()` → `RemoteStatus { generation: status.generation.to_string() }` is sent **only when `recovery()` returns `Some`** (paired/ssh-backed sessions). For local sessions `recovery()` returns `None`, so **no initial frame is issued at all** (see "Missing producer API"). The client only ever stores what the server issued (`generationRef.current = parsed.generation`, RemoteTerminal L776-781); `encodeInputFrame` throws `RemoteInputGenerationMissingError` otherwise and `sendInput` drops typed (L499-518) — fail-closed KEPT, never a raw fallback. The machine path does issue generation independently (`machine_protocol::Attached { generation }`, server ~L2039); the local plain/grid path has only recovery messages.
- Stale report 842 claims (superseded by this file): "14-file followup" — actual **10 unique files** (A=6, B=5, RemoteTerminal.tsx shared); "retained hashes" — actual final hashes below; "verifier" routing — **8b3 is sole runtime, 842 obsolete**.

## Missing producer API (BLOCKING — reported, not claimed)

- **Gap:** local sessions never receive an issued generation → `generationRef` stays null → `sendInput` drops **all** local input fail-closed. There is no producer to consume: no initial `RemoteStatus` on local connect, no per-reconnect re-issue, and `write_input_operation` ignores `generation` for the local PTY (read-contract asymmetry).
- **Contract to coordinate (owners: writer812 = `write_generation` authority, output7db = output/attachment generation):** per-attach generation issuance for **every** session kind (local + paired), fresh on each attach/reconnect, emitted as the initial control frame; stale (pre-reconnect) generations must be denied at the fenced write; read side is the EXISTING consumer (`remoteStatus` parse at RemoteTerminal L776-781 — present, unmodified; no new consumer code needed).
- The three new tests in patch C are authored to this contract and fail **naming the missing API** until it lands — they never pass on a hardcoded generation "1".

## Patch C — bounded control lane (fixes patch A's own defect)

- Patch A introduced `mpsc::unbounded_channel` for client-triggered typed input rejections (a flooding client could grow it without bound). C replaces the plain-handler lane with `mpsc::channel(CONTROL_LANE_BUDGET = 8)` + `queue_control_frame()` using a fixed `CONTROL_SEND_DEADLINE = 100ms` (backpressure, then drop-with-warn; the input itself was already denied server-side).
- Grid handler still uses an unbounded lane (server ~L3080) — **pre-existing base infrastructure** also carrying client-controlled mirror ops; left untouched to preserve other owners, flagged here as inherited debt.
- Tests added (UNRUN, real gateway + PTY, bounded deadline loops, no sleeps): `test_production_initial_control_frame_issues_generation_for_local_consumer`, `test_local_keystroke_and_x10_bytes_derive_issued_generation_real_pty` (X10 high bytes 0xE8/0xD4 byte-exact), `test_stale_generation_after_reconnect_denied` (fresh per-attach generation asserted + stale deny before write).

## Gesture scope split (explicit dependent handoff)

- **This node (transport + router):** `terminalGestureRouter.ts` is complete and byte-identical to `pre-a` (routeWheel/routePrimary/routeTwoFinger, encodeMouseReport/encodeArrowKey, X10/wheel caps, CellCoordinate) — it is base, not a patch delta. Patch A carries the helper-level tests (per-mode wheel byte counts, zero-report for tracking-off/unknown, fail-closed no-generation) plus the inputSeq/transport fence.
- **Dependent node (caller integration, NOT optional):** RemoteTerminal has **zero** `route*`/`applyRoute` calls (grep-confirmed). The dependent must wire the real JSX handlers — `handleWheel` (L882), `handleTouchStart/Move/End/Cancel` (L974–1162, bound at L1417–1421) — through `routeWheel`/`routePrimary`/`routeTwoFinger` + the encode helpers with cell coordinates and the `wheelRemainderRowsRef` remainder coordinated against `MAX_WHEEL_TICKS`, replacing the inline wheel arithmetic (L892–895). Helper-only tests do **not** satisfy the dependent: mounted-caller tests against real RemoteTerminal handlers are required. Generation stays server-issued per the contract above; no raw fallback.
- `GapQueued` is not nacked outbound (no `ServerControlMessage::Nack` variant exists) — server-side bound only; distance rejection `remote_input_gap_distance_exceeded` remains.
- ssh control path: no epoch/sequence fence (scope: the two real socket callers).
- Mode-query failure → Unknown whole mode is fail-closed correctness, not parity; typed unknown *encoding* is not modeled.
- Unrun by mandate: no cargo/bun/tsc anywhere ("No ownerbuilds"); children not run; shared tree read-only; no production daemon/GUI/commit; no full recapture; same owner/release/cancel receipt lineage as round 1.
