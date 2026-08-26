# Native terminal migration gate audit

**Audit date:** 2026-08-25  
**Scope:** implementation-plan gates through the current Phase 3 boundary.  
**Rule:** a passing unit test or build is supporting evidence, not a substitute
for the documented real-surface gate.

## Objective-to-artifact matrix

| Plan deliverable | Evidence | Verdict |
| --- | --- | --- |
| Phase 0 reproducible xterm baseline and feature freeze | `baseline-summary.md`, `baseline-latest.json`, `xterm-feature-baseline.json`, and `bun test bench/terminal/workloads.test.ts` (11 passed) | Passed |
| Phase 1 safe pinned libghostty-vt engine boundary | `native-terminal-phase1-safety-abi-audit.md`; Ghostty build contract (13 passed), engine contract (22 passed), macOS `leaks` result, and guarded allocator run | Passed, with Miri limitation explicitly recorded |
| Phase 2 standalone renderer | Native WGPU renderer and offscreen contract suites exist; this audit does not treat their existence as a Phase 3 pass | Supporting evidence only |
| Phase 3 static composition | `native-surface-phase3-composition-check.md` and macOS screenshot show clipping below app chrome and beside the sidebar | Partial: static geometry only |
| Phase 3 native click/text/render path | `native-surface-phase3-input-prototype.md`; user macOS receipt shows `abc` and a block cursor in the right Ghostty/WGPU pane | Passed for ordinary text input |
| Phase 3 focus retention | Pointer-default regression contract and one successful click-to-text receipt | Partial: leave-and-return focus round trip remains unrecorded |
| Phase 3 dynamic resize and split | Ghostty-state-preserving bounds test passes | Partial: no live tear-free resize/split receipt |
| Phase 3 React modal/overlay stacking | None | Open |
| Phase 3 IME and candidate anchor | Cursor receipt + CSS anchor contracts, including 2x DPI conversion | Partial: no live macOS CJK composition/candidate receipt |
| Phase 3 multi-display/DPI transition | 2x DOM contract only | Open: no live display transition |
| Phase 4 onward | No work may begin until the Phase 3 architecture gate is accepted | Not entered |

## Native Daemon-PTY Bridge Contract (2026-08-25)

### RED Evidence

`cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract` failed because `NativeTerminalSurfaceHostState` lacked the daemon attachment and stream bridge:

```
error[E0599]: no method named `attach_daemon_attachment` found for struct `NativeTerminalSurfaceHostState` in the current scope
  --> tests/native_terminal_surface_host_contract.rs:89:10
   |
89 |         .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
   |          ^^^^^^^^^^^^^^^^^^^^^^^^ method not found in `NativeTerminalSurfaceHostState`

error[E0599]: no method named `snapshot_for_session` found for struct `NativeTerminalSurfaceHostState` in the current scope
   --> tests/native_terminal_surface_host_contract.rs:105:10
    |
105 |         .snapshot_for_session(session_id)
    |          ^^^^^^^^^^^^^^^^^^^^ method not found in `NativeTerminalSurfaceHostState`
```

### GREEN Evidence

The production bridge connects `DaemonClient::attach` to `NativeTerminalSurfaceHostState`:
- Daemon attachment replay history and live `DaemonStreamMessage::Output` chunks feed directly into the session's native Ghostty VT emulator.
- Replay gaps and session exit/detach are handled deterministically without leaked background tasks.
- `cmd_native_terminal_send_input` encodes browser key and text payloads via the Ghostty key encoder and routes them directly to `DaemonClient::write_terminal`.
- React `NativeTerminalPane` explicitly attaches its daemon `backendSessionId` on mount before reporting bounds, and detaches on unmount/replacement. Its frontend-local session ID remains DOM-only.

Verification results:
- `cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract`: 4/4 passed (0.05s).
- `cargo test --manifest-path src-tauri/Cargo.toml native_terminal`: 10/10 passed.
- `cargo check --manifest-path src-tauri/Cargo.toml`: passed with only 4 pre-existing dead-code warnings in `session.rs` and `manager.rs`.
- `bun run --cwd ui test src/components/NativeTerminalPane.test.tsx`: 16/16 passed (250ms).
- `bun run --cwd ui build`: passed (`tsc && vite build` in 1.69s).

### Deterministic stream-proof hardening

The first bridge contract used `Notify::notified()` without registering an
active waiter before output was triggered, which could lose a
`notify_waiters()` event under a different scheduler interleaving. The
contract now subscribes to a per-session `watch::Receiver<()>` before daemon
output or gap recovery, then awaits `changed()` with a two-second bound. The
producer advances the watch version only after the native terminal processes
the stream message. The test fixture's inert daemon reader uses
`std::future::pending()` and is aborted during deterministic detach; it has no
wall-clock delay.

RED:

```text
error[E0599]: no method named `changed` found for struct `Arc<Notify>`
```

GREEN:

```text
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_surface_host_contract
4 passed; 0 failed; finished in 0.00s
```

### Active desktop prompt smoke scenario

Status: **in progress; not a passing claim**.

The current `cargo tauri dev` run launched the main Ferryx window and Vite
hot-reloaded the two fixes below:

1. `NativeTerminalPane` now uses `backendSessionId` rather than the
   frontend-only tab ID for attach, bounds, focus, input, and detach. A
   distinct-ID regression test initially failed against the former mapping and
   now passes.
2. The native terminal's WebView ancestry is transparent only while a native
   pane is present, allowing the root-window WGPU surface to show through the
   terminal rectangle while browser chrome and overlays remain opaque.

Required observation in this exact running surface:

```text
select terminal tab -> shell prompt visible
printf 'native bridge ok\n' -> command output visible -> next prompt visible
```

The UI machine gates after these fixes are clean:

```text
bun run --cwd ui test   # 81 files, 615 tests passed
bun run --cwd ui build  # tsc + Vite build passed
```

The smoke scenario is deliberately left pending until its actual on-screen
result is observed and recorded; it is the prerequisite for the remaining
Phase 3 focus, resize, overlay, IME, and DPI scenarios.



**Do not begin Phase 4.** The Phase 3 architecture gate remains open because
the required real-surface focus round trip, dynamic resize/split behavior,
overlay layering, CJK IME candidate placement, and display-transition
scenarios have not been captured.

The web/mobile xterm remote path remains out of scope for the native POC and
must remain intact through the later rollout gate.

## Remaining manual evidence

1. Click from the right native pane to the left xterm pane and back; confirm
   each receives its own subsequent input.
2. Resize the window and drag a split while the native pane contains text;
   capture no tearing, blanking, or reset.
3. Open a React modal, menu, and palette over the native pane; capture correct
   stacking.
4. Compose and commit CJK text while a candidate UI is visible; capture that
   it is anchored at the native cursor, then repeat after moving/resizing the
   pane.
5. Move between displays with distinct scale factors when available; capture
   geometry and text sharpness remain correct.
