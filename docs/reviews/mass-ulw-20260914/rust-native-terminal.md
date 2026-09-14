# Native terminal engine (Rust) review — `src-tauri/src/native_terminal/`

Scope reviewed under a hard read budget: FFI pointer handling around `libghostty-vt`
(`lifecycle.rs`, `bell.rs`, `guards.rs`, `queries.rs`, `selection.rs`, `cell_extractor.rs`,
`png_decoder.rs`), platform parity (`composition.rs`, `platform/mod.rs`, `platform/macos.rs`,
`platform/windows.rs`, `platform/linux.rs`) and lock ordering between render and input
(`surface_host.rs`, `terminal.rs`). Only findings whose decisive lines I opened and read are listed.

### [P2] `active_for_platform()` reports a descriptor that can never pass its own validation on Windows and Linux

- Location: `src-tauri/src/native_terminal/composition.rs:275`
- Observed: the per-platform "active" descriptor hard-codes both composition flags to `false` off macOS:

  ```rust
  #[cfg(target_os = "windows")]
  {
      Self {
          target_kind: CompositorTargetKind::WindowsChildWindow,
          pointer_transparent: false,
          layer_backed: false,
      }
  }
  ```

  (same shape for `#[cfg(target_os = "linux")]` at `composition.rs:283`), while
  `validate_desktop_composition` rejects exactly that combination for the same kind at
  `composition.rs:326-331`: `CompositorTargetKind::WindowsChildWindow => { if !self.layer_backed { return Err(... "is not layer-backed" ...) } }`.
  The real targets disagree with it: `platform/windows.rs:284-290` returns
  `pointer_transparent: true, layer_backed: true`, and `platform/linux.rs:344-350` returns
  `pointer_transparent: has_child, layer_backed: has_child`. `surface_host.rs:2003-2009`
  (`target_descriptor`) hands this bogus descriptor out whenever no host is currently attached:
  `if let Some(host) = hosts.values().next() { host.descriptor() } else { PlatformCompositorDescriptor::active_for_platform() }`.
  The unit test at `composition.rs:468-500` cements the mismatch by asserting
  `active.validate_desktop_composition().unwrap_err()` on Windows and Linux.
- Why it is wrong: the project mandates cross-platform parity, but the single "what does this platform
  support" query answers "unsupported/invalid" on two of the three shipped platforms, and a test locks
  that answer in. Any caller that gates on `target_descriptor()` before a host exists (startup probe,
  capability report, diagnostics surface) will disable or error out the native terminal on Windows and
  Linux while the actual compositor targets are fully valid. I found no shipped caller of
  `target_descriptor()` today (`rg target_descriptor src-tauri/src ui/src` returns only the definition),
  which is why this is P2 and not P1: it is a latent parity trap, not a current user-visible break.
- Minimal fix: make `active_for_platform()` return the same flags the real targets report
  (`pointer_transparent: true, layer_backed: true` for `WindowsChildWindow` and `LinuxChildWindow`) and
  flip the two `unwrap_err()` assertions at `composition.rs:468-500` to `is_ok()`. Do not relax
  `validate_desktop_composition`; the validator is the honest half of this pair.

### [P2] VT replies are dropped silently when the write-PTY callback cannot take either lock, and when the buffer cap is reached

- Location: `src-tauri/src/native_terminal/bell.rs:122`
- Observed: the C write-PTY callback tries both sinks non-blockingly and has no final fallback:

  ```rust
  if let Some(guard) = ctx.pty_write_tx.try_lock() {
      if let Some(tx) = guard.as_ref() { let _ = tx.send(record); return; }
  }
  if let Some(mut guard) = ctx.write_pty_buffer.try_lock() {
      const MAX_WRITE_PTY_BUFFER: usize = 16 * 1024;
      let current_bytes: usize = guard.iter().map(|r| r.data.len()).sum();
      if current_bytes + record.data.len() <= MAX_WRITE_PTY_BUFFER { guard.push(record); }
  }
  ```

  If `write_pty_buffer.try_lock()` fails, or the 16 KiB cap is exceeded, the record is discarded with
  no `tracing` record at all. `terminal.rs:158-166` (`set_pty_write_sender`) holds *both* of those
  mutexes simultaneously while draining, which is precisely the window in which both `try_lock`s fail.
- Why it is wrong: these records are terminal query responses (DSR cursor-position reports, device
  attributes, mode reports). An application that writes `ESC[6n` and blocks on the reply hangs forever
  when its response is dropped, and there is no log line for an operator to correlate the hang with.
  The drop is silent by construction, so the failure presents as "the pane froze" with a clean log.
- Minimal fix: add a `tracing::warn!(len = record.data.len(), "dropping terminal PTY write: sink unavailable")`
  on both discard paths (the `try_lock` miss and the cap rejection) so the loss is observable; the
  buffering policy itself can stay as-is.

### [P3] Blocking lock inside an FFI callback that every other sink in the same callback avoids

- Location: `src-tauri/src/native_terminal/bell.rs:116`
- Observed: `let generation = *ctx.remote_generation.lock();` — a blocking `parking_lot::Mutex::lock()`
  executed on the libghostty callback thread, immediately before the two deliberately non-blocking
  `try_lock()` calls at `bell.rs:122` and `bell.rs:128`. The comment discipline elsewhere in this file
  ("Prevents unwinding across the foreign ABI", `bell.rs:68-70`) shows re-entrancy is a known hazard here.
- Why it is wrong: it is a re-entrancy foot-gun rather than a live bug. Today every caller of
  `NativeTerminal::set_remote_generation` (`terminal.rs:171`, used at `surface_host.rs:1348`, `1428`,
  `1791`) takes and releases the guard within a single statement, so no caller can be holding it while
  `feed()` drives the callback. The moment one does, the callback deadlocks the calling thread with no
  timeout and no diagnostic — an asymmetry a future edit will not notice.
- Minimal fix: use `ctx.remote_generation.try_lock().map(|g| *g).unwrap_or(None)` so the whole callback
  is non-blocking and consistent with its two neighbouring sinks.

## Checked and found clean

- Lock ordering between render and input is consistent: every nesting acquires `hosts` before
  `sessions` (`surface_host.rs:979-989` `lock_attached_hosts`, `surface_host.rs:374-376`
  `dispatch_scheduled_render`, `surface_host.rs:2188/2204` `render`, `surface_host.rs:2262/2264`
  `render_current_with_focus`). I found no inverted `sessions` → `hosts` nesting; the two sites that
  touch both in the other direction (`surface_host.rs:1164-1167`, `1199-1200`) release the first guard
  before taking the second.
- FFI ownership is guarded correctly: every foreign handle is wrapped in a `Drop` guard
  (`guards.rs:17-107`), `create_native_terminal` frees the handle on every registration failure
  (`lifecycle.rs:52`, `81`, `93`, `105`, `117`, `129`, `144`, `157`), all extracted foreign buffers are
  null- and length-checked before `from_raw_parts` (`queries.rs:203-207`, `selection.rs:153-161`,
  `cell_extractor.rs:70-110`), and the PNG callback wraps decoding in `catch_unwind` and transfers
  ownership through Ghostty's allocator (`png_decoder.rs:62-82`).
- No `unwrap()`/`expect()`/`panic!()` on OS-controlled values in non-test code. The only non-test
  occurrences are two `unreachable!()` over internal, fully-enumerated state
  (`surface_host.rs:232`, `composition.rs:351`).

Summary: P0=0, P1=0, P2=2, P3=1
