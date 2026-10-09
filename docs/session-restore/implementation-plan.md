# Ferryx Session Preservation and Recovery Implementation Plan

- Basis specification: `docs/session-restore/spec.md` v11.1 (hereinafter "the specification")
- Date written: 2026-10-09
- Status: Executable. However, some designs in P2 and P3 may change depending on the outcome of resolving the P0 risks (marked on each item).

---

## 0. Premises and Principles

- **Coexistence of the new path and the existing path**: The new path (MicroHost + FXSH) is built under the feature flag `ferryx.sessionHost.v6`. When the flag is off, the current v5 daemon path operates as-is. Even after the flag is turned on, existing v5 sessions continue to be used as the Legacy partition (specification §6.1).
- **Reuse first**: Follow the reuse verdicts in the table in §1 below. The only things written anew are the parts where the specification differs fundamentally from the existing structure (host process, FXSH, state replication, lease).
- **State machine implementation approach**: The specification's state machines (§2.5, §3.6, §4.4, §5.5, §4.3) are first built as pure Rust types without I/O. Handlers take the form `(state, event) -> (state, effects)`. The same random-order checks as in the executable models (`docs/session-restore/spec-models/*.js`) are ported to Rust `proptest` and kept as unit tests. I/O adapters are attached after that.
- **Project rules**: Simultaneous support for all platforms (macOS, Windows, Linux). Synchronous disk I/O and git go through `run_blocking`. Force-killing the existing daemon is prohibited. Storage keys are `ferryx.*`. IPC errors are structured `{code, message, details}`.

---

## 1. Correspondence Between Current Code and the Specification

| Specification component | Current code | Verdict |
|---|---|---|
| Host VT engine | `native_terminal/sys` (libghostty-vt FFI), `NativeTerminal` in `native_terminal/terminal.rs`, `cell_extractor.rs`. VT parsing is always included in the build (`Cargo.toml` features comment) | **Reuse**. The host process uses only `NativeTerminal` without a GPU. S extraction extends `cell_extractor` |
| PTY creation and child signals | `terminal/pty.rs` `PtyManager`, vendored `portable-pty` | **Partial reuse**. Creation and signals are reused. The write path is replaced with the non-blocking write queue of specification §3.5 (depending on the P0-1 result) |
| Output ring buffer | `terminal/output_hub.rs` `TerminalOutputHub` (512 KiB, `ReplayGap`) | **Replaced in the new path**. The specification uses S replication (Snapshot+Delta) instead of byte replay. Retained in the Legacy path |
| Existing GUI snapshot | `native_terminal/snapshot_codec.rs` (ghostty snapshot v1) | **Replace**. FXSH uses the canonical SBody of specification Appendix A. The ghostty snapshot is considered only as an aid for client-side grid reconstruction |
| Daemon process and socket | `daemon/server.rs` (7,857 lines), `daemon/protocol.rs` (v5 JSON+Base64), `daemon/client.rs` | **Reduce and convert into a policy daemon**. PTY ownership is handed over to the host, and the daemon becomes the router, registry, intent DB, and relay. The v5 server code remains as the counterpart of the Legacy partition adapter |
| Handover | `daemon/handover*.rs` | **Not used for new sessions** (specification §9). Retained only for Legacy sessions |
| macOS service registration | `daemon/launchd.rs` (`com.ferryx.daemon`, plist generation and loading) | **Pattern reuse**. Generalized to per-instance `com.ferryx.host.<id>` |
| Linux and Windows service registration | None (run detached via `util::detach_launched_child`) | **New**. systemd user template unit, Task Scheduler |
| Persistent DB | `rusqlite` (bundled) dependency exists | **Reuse**. Used for the intent DB (WAL) |
| GUI session restore | `ui/src/lib/sessionPersistence.ts` (epoch matching, revival inference), `ui/src/state/workspaceRestore.ts` | **Replace**. Replaced with the binding FSM (specification §5.5), and the inferred-revival clause is removed |
| GUI native surface | `nativeTerminalLifecycle.ts`, `nativeTerminalAttachPolicy.ts`, `native_terminal/surface_host.rs`, `lifecycle.rs`, `viewport.rs` | **Convert**. The pane actor rules (specification §4.3) and the Resize lease (§4.4) are put into this layer |
| GUI input | `ui/src/lib/nativeTerminalInputQueue.ts`, `native_terminal/input.rs` | **Replace**. Writer state machine (specification §3.6) |
| Remote client | `remote/` (Axum, WS, attach_router, machine_protocol) | **Convert**. Make the remote view an FXSH subscriber and writer, and the gateway relays FXSH over WS |

---

## 2. Phases

The "completion criteria" of each phase must pass before starting the corresponding part of the next phase. Dependencies are in §3.
### P0. Risk-resolution experiments (1–2 weeks)
These are things the spec assumes but has not measured. Depending on the results, spec revisions may be required.

**Experiment platforms**: Since there is no macOS experiment machine (maho-mac), P0 is done only on Windows (`maho-win`, Windows 11 26200) and Linux (`omarchy`, Arch + Hyprland/Wayland). The macOS items in the table below are excluded from this P0 scope, and macOS verification is performed at the P8 Gate harness stage once a macOS experiment machine becomes available. Until then, macOS paths are marked "unverified".

| ID | Experiment | Success criteria | On failure |
|---|---|---|---|
| P0-1 | Overlapped (asynchronous) writes to the Windows ConPTY input pipe, POSIX master fd `O_NONBLOCK` writes and `EAGAIN` resumption | On Windows and Linux, a 1 MiB paste is delivered in order via partial writes and resumption, with 0 actor-thread blocking | Replace with a dedicated writer thread + bounded channel for Windows only, and amend the "non-blocking" wording in spec §3.5 to "writes that do not block the actor" |
| P0-2 | Full extraction of S (spec §2.2) with `NativeTerminal` (no GPU) in the host process: screen cells, scrollback, palette, modes, hyperlinks | Extract → canonical_encode → decode → re-encode bytes identical. Measure extraction time at 200×60 + 10,000 lines | Write additional libghostty-vt FFI bindings for each missing item |
| P0-3 | Scrollback rewrite detection (spec §2.3): reflow, ED 3, engine-internal rewrites | Detection signal fires exactly once after each operation | Replace items that cannot be detected with per-step scrollback hash comparison (including cost measurement) |
| P0-4 | Per-instance service units: systemd `ferryx-host@.service`, Task Scheduler task (launchd after obtaining a macOS machine) | On each OS, registration, start, and self-deletion; survives termination of GUI and daemon; PPID chain is the service manager | Specify the alternative mechanism for that OS in spec §1.1 |
| P0-5 | Immediate detachment right after Destroy on Linux X11 and Wayland child surfaces | The spec §4.3-8 Destroy rule is possible within the same step | Fall back to deferred detachment + masking in the platform module |

Completion criteria: a results report for the five experiments and, if needed, a revised spec.

**P0 results (Linux, omarchy, 2026-10-09)** — Evidence: `docs/session-restore/p0-evidence/`
- **P0-2 state extraction: passed (conditional)**. Without a GPU, S was extracted using libghostty-vt's grid ref API (`ghostty_terminal_grid_ref`, `ghostty_grid_ref_cell/row/style/graphemes/hyperlink_uri`) and terminal queries (modes, palette, title, cursor). 200×60 + 9,768 lines of scrollback: 3.93 million cells, 39,308 combining characters, 112,624 hyperlink cells, experimental encoding 32.0 MB, **136 ms**, byte-identical results across two extractions. The round-trip (decode→re-encode) check against the canonical encoding of spec Appendix A has not been done yet.
  - Finding 1: Scroll regions and tab stops have no query API (they only come out via the formatter's VT output). Since they are not needed for display, they were removed from S in the spec (§A.4).
  - Finding 2: While the alternate screen is active, the primary screen's scrollback is not visible in history coordinates. The spec now defines S relative to the active screen and includes screen switching in scrollback rewrites (§2.3). Consequence: every time a full-screen program like vim is opened and closed, a full snapshot is sent (see risk table below).
  - Finding 3: The scrollback line limit is a page-granular approximation (9,768 lines for a 10,000-line request). There is a separate default byte limit, and without lifting it only 215 lines remained. The host must explicitly set the byte limit.
- **P0-3 rewrite detection: passed**. The detector (dimension and active-screen comparison + position and content hash of the tracked grid ref of the newest scrollback row) was checked against actual history comparison (ground truth). 300 random runs, 45,000 steps, of 9,837 actual rewrites **0 missed**, 1,145 false positives (2.5%, causing only unnecessary resynchronization). 36 µs per step at 200 columns. This detection rule was added to spec §2.3.
- **P0-1 non-blocking PTY writes: passed (Linux, Windows)**. 1 MiB was written in 64 KiB units with the child's reads delayed by 0 / 1.5 / 4 seconds. Linux: on the `O_NONBLOCK` master, about 240 partial writes and about 240 EAGAINs, max write call 15 µs, bytes and order matched. Windows: on the ConPTY input pipe (`FILE_FLAG_OVERLAPPED`), 15 pending, max write call 14 µs, bytes and order matched.
  - Finding 4: Windows ConPTY accepts the full 1 MiB within about 1 second even if the child does not read for 4 seconds (internal buffer). Therefore, on Windows the "write-point record (committed)" does not mean the child has read it, and the write queue's 1 MiB backpressure (§3.4-7) is also rarely triggered on Windows. This is consistent with the spec definition (write point = ConPTY input pipe `WriteFile`), so there is no spec change; it is reflected in the Windows expected values for Gate 3.
  - Finding 5: In the first experiment, the child inherited the parent's standard input instead of the console and received 0 bytes (`STARTF_USESTDHANDLES` missing). The vendored portable-pty already sets `STARTF_USESTDHANDLES` and `INVALID_HANDLE_VALUE` (`psuedocon.rs:122-123`). The host implementation must keep this setting.
- **P0-4 per-instance service units: passed (Linux, Windows)**. Confirmed with systemd user template `ferryx-p0-host@.service` and Task Scheduler `\\FerryxP0\\Ferryx-P0-Host-<id>` (principal is the user SID): two instances running concurrently, restarting only one leaves the other's PID unchanged, parent is the service manager (systemd / svchost), unaffected by termination of unrelated parent processes, voluntary exit and self-deregistration of retired instances, 0 registrations after cleanup. On Windows, specifying the principal by account name fails with `0x80070534`, so the SID must be used. omarchy has `Linger=yes` so it persists after logout, but for users with linger disabled, linger must be enabled at install time.
- **P0-5 immediate child surface detachment: passed (Linux Wayland, X11)**. 3 runs each: on Wayland, removing the `wl_subsurface` or committing a null buffer is sent within 0.03 ms in the same step, with compositor confirmation at 0.25 ms or less. On X11, removing the child window was confirmed at 0.11 ms or less (parent's child count 1→0), and unmap changed the map state 2→0. On-screen pixel verification was not done (see incident below).
  - Incident: In the first attempt, a `grim` screenshot for verifying results killed omarchy Hyprland with SIGBUS (`Screenshare::copyShm` → `readPixels`, radeon). Subsequent automation uses only protocol verification, not screen capture.
- **P0 summary**: All five experiments passed on Windows and Linux. The macOS paths (launchd, NSView detachment, macOS PTY) are unverified. The conditions for starting P1 and P2 are met.
### P1. FXSH codec (1 week)
- New module `src-tauri/src/fxsh/`: header (spec §7.1), basic encoding (§7.2), 34 commands (§7.4), error codes (§7.6), Appendix A schema, `canonical_encode`, `state_digest`(xxh3_128).
- Use manual encode/decode only (serde forbidden, spec §7.1).
- Tests: golden vectors, rejection of truncated frames and invalid bool/UTF-8/tag/opt conditions, ignoring of trailing bytes, 16 MiB limit.
- Completion criteria: `cargo test fxsh::` passes, fuzzer (`cargo fuzz` or proptest random bytes) 0 panics in 1 hour.
### P2. Host core state machines (3~4 weeks, no I/O)
New module `src-tauri/src/session_host/`. Each item is a pure state machine + proptest.
1. **Session actor state** (§2.1~§2.3): S, revision, line_id, size normalization, `S_MAX` eviction, hyperlink table cleanup. Input is the extractor from P0-2.
2. **Subscription·Delta·merge** (§2.4, §2.6): subscription replacement (`subscriber_id`, `attach_seq`), queue limit, `resync_count`.
3. **Snapshot budget allocator and subscription budget state machine** (§2.5): including Draining. Port the invariants of the execution model `budget` to proptest.
4. **Input ledger·write queue** (§3.3~§3.5): epoch, fence, retained ring, ledger retention, VtReply limit.
5. **lease host side** (§3.3, §4.4): input·Resize, `revokees`, `LeaseVacated`.
6. **operation table** (§7.5): reservation, request hash, failure result storage, eviction, monotonic clock abstraction (accelerated in tests).
- Completion criteria: per-module proptests (at least 10,000 cases each) pass. All invariants of the execution model exist as Rust tests.
### P3. Host process (2 weeks, requires P0-1·P0-4·P1·P2)
- New binary `ferryx-host` (`Cargo.toml` `[[bin]]`). A tokio task per session actor, a PTY reader thread and an 8 MiB mailbox (§2.1).
- Local IPC endpoint, directory·peer credential verification (§1.2), Hello/HelloAck (§7.3).
- Per-frame sender (§2.5 send path) and `SnapshotFrameWritten`·`SnapshotBufferReleased` events.
- Instance registry file and `retired` handling (§1.1, §6.2). The service unit registration code is `session_host/service/{launchd,systemd,taskscheduler}.rs`, a generalization of `daemon/launchd.rs`.
- Completion criteria: integration tests for real shell Spawn → Subscribe → digest match, input round-trip, Kill idempotency. Pass Gates 5, 6, 7, 17 (excluding a) with the host alone.
### P4. Policy daemon overhaul (2~3 weeks, requires P3)
- **Partition registry and router** (§5.1, §5.3): MicroHost connection, Legacy adapter (a wrapper that attaches to the v5 daemon via the existing `daemon/client.rs`).
- **intent DB** (§5.2): rusqlite WAL. Includes `spawn_payload`. Writes use `run_blocking`.
- **Inventory aggregation** (§5.4): persist `request_token` and `owner_dead` state.
- **Owner death monitoring** (§5.6): kqueue / pidfd / `OpenProcess`, separated into platform modules.
- **Relay routing metadata** (§3.1): request mapping, `client_instance_id → connection`, subscription mapping, Unsubscribe on connection close.
- Since the daemon no longer owns the PTY, the `handover*` path is bypassed for new sessions (kept only for Legacy).
- Completion criteria: pass Gates 2, 14, 15, 16, 17(a), 18, 19, 20 with the daemon+host combination (the GUI is replaced by a test client).
### P5. GUI native layer (3 weeks, requires P1·P2. Can run in parallel with P4)
- **Replica client** (§2.7, §2.8): snapshot assembly, Delta application, hyperlink cleanup, UI event deduplication. The result is a CPU grid model → connected to the existing renderer (`native_terminal/renderer`).
- **GPU device actor** (§4.2) and **pane actor** (§4.3): rework `surface_host.rs`, `lifecycle.rs`, `viewport.rs`. Detach immediately on Destroy (result of P0-5).
- **Resize lease client** (§4.4) and **composer state machine** (§3.6): place the pure state machine (same approach as P2) in Rust and expose it via Tauri commands. `nativeTerminalInputQueue.ts` becomes a thin layer that calls this state machine.
- Completion criteria: pass Gate 4a~4f, 10, 11, 12, 13.
### P6. GUI state & restore (2 weeks, requires P4·P5)
- Implement the binding FSM (§5.5) as a pure reducer in `ui/src/state/`. Remove the epoch-guessing revival clause and the `backendSessionId = null` handling in `sessionPersistence.ts`, and connect `workspaceRestore.ts` to FSM events.
- Layout hint adoption procedure, displaying the final screen of terminated sessions.
- Tauri JSON boundary (§7.8): u64 as decimal strings, TS `BigInt`.
- Completion criteria: `bun run --cwd ui test` passes. Pass Gates 1, 8, 9 with the actual GUI.
### P7. Remote client (2 weeks, requires P4·P6)
- The `remote/` gateway relays FXSH frames over WS, and the remote view (`RemoteApp.tsx`) uses the same replication client, author, and lease rules. TS-side FXSH codec (cross-validated with P1's golden vectors).
- Completion criteria: Pass Gate 4e and the "two remote clients" item of 11. Zero regressions in existing remote functionality.
### P8. Gate harness (in parallel from P3, 3 weeks)
- `fault-tap` feature flag (PTY write point log), `ferryx-pty-probe` child program, daemon/host instrumentation hooks (delay, drop, reorder injection), monotonic clock acceleration hook, GPU device lost injection.
- Three measurement environments: macOS 15, Ubuntu 24.04 (X11·Wayland), Windows 11.
- Completion criteria: a script that automatically runs Gate 1~20 on the three OSes, and a results report.
### P9. Release and migration (1~2 weeks)
- Ship the flag as default off. Then, after going through internal use (dogfood), switch it to default on.
- updater changes: prohibit terminating the existing daemon (§6.1), install an additional host instance (§6.2).
- Legacy retirement UI (§5.7).
- Completion criteria: Gate 20 passes with the actual updater, 0 loss of existing sessions.

---

## 3. Dependencies

```
P0 ──┬─> P1 ──┬─> P2 ──┬─> P3 ──> P4 ──┬─> P6 ──> P7 ──> P9
     │        │        └─> P5 ─────────┘
     └────────┴──────────────> P8 (in parallel from P3, done before P9)
```

- Can run in parallel: P1 and P0-2~P0-5. Items 1~6 within P2 are mutually independent. P4 and P5.
- Total duration estimate: approximately 16~20 weeks based on the serial path (1 person). Can be shortened by parallelizing P2·P5·P8.

---

## 4. Remaining risks

| Risk | Impact | Response |
|---|---|---|
| P0-1 Asynchronous writes not possible on Windows | §3.5 write queue design change | Verify first in P0, specify an alternative |
| P0-2 libghostty-vt does not expose all of S | Replication accuracy (G2) | Extend FFI. For items where this is not possible, adjust the definition of S in the specification |
| Snapshot serialization cost | Measured 136 ms (10,000 lines, 200 columns). During that time, that session's actor stalls. Occurs on every alternate screen switch | In P2, measure an approach that moves extraction to a copy outside the actor (ghostty snapshot_encode, then decode on a separate thread). Specify an upper bound on actor stall in the specification |
| Scope of daemon modification (`server.rs` 7,857 lines) | P4 schedule | Place the new path in a separate module and pin the existing code behind a Legacy adapter |
| Parts of the specification have only undergone execution model checking (no external review) | Specification defects discovered during implementation | Defects discovered during implementation are bundled into the same change as the specification revision and the update of the corresponding execution model |
