# Phase 2 Standalone Native Renderer Evidence

## Automated evidence

The standalone `wgpu` renderer POC was exercised on this workstation with:

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
  --bin native_terminal_renderer_poc -- \
  --headless --output bench/terminal/evidence/native-wgpu-phase2-latest.png
```

Observed result:

- Adapter: Apple M4 Max (Metal, IntegratedGpu)
- Frame: 800x480 px, 24 rows
- Dirty update: 1 rebuilt row, 23 reused rows
- Glyph atlas: 112 entries, 268,416 / 4,194,304 bytes
- Frame latency, 50 frames: p50 3.096 ms; p95 4.337 ms
- Artifact: `native-wgpu-phase2-latest.png`, 39,963 bytes

The resulting PNG is visibly nonblank and includes ANSI-colored text, CJK/Unicode
cells, a long line, a selection highlight, and a cursor.

Focused validation passed:

```text
cargo test --manifest-path src-tauri/Cargo.toml \
  --test native_terminal_renderer_contract -- --nocapture
6 passed; 0 failed

cargo check --manifest-path src-tauri/Cargo.toml
exit 0 (pre-existing unused-item warnings outside this renderer)

cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
exit 0
```

CLI QA passed:

- `--help` exits successfully and lists `--headless`, `--output`, and `--window`.
- Unknown input and `--output` without a value exit 1 before rendering.

## Required manual native-window gate

This agent did not open or automate a desktop window. Before Phase 3 may begin,
run:

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
  --bin native_terminal_renderer_poc -- --window
```

Verify that the Ferryx window appears at 800x480 with the rendered terminal
scenario, resize it, and confirm it redraws without a blank/tearing surface.
Close the window after the check. Record any visible GPU/surface error printed
to the launching terminal.

## Scope note

These frame timings measure standalone native rendering. They are not equivalent
to the Phase 0 xterm parser throughput metrics and must not be used as a direct
parser-throughput comparison.

## Desktop launch recovery

Adding the standalone POC introduced a second Cargo binary and made Tauri's
existing `cargo run --no-default-features --color always --` dev command
ambiguous. The resulting launch failure was reproduced, then fixed by setting
`default-run = "ferryx"` in `src-tauri/Cargo.toml`.

After the fix, `cd src-tauri && cargo tauri dev` started Vite, compiled Ferryx,
and launched `target/debug/ferryx`. The process remained running. The terminal
emitted macOS Text Services Manager input-method messages, but did not exit.

## Native-window visual QA

The standalone POC was launched with:

```bash
cargo run --manifest-path src-tauri/Cargo.toml \
  --bin native_terminal_renderer_poc -- --window
```

Orca's macOS accessibility provider captured the real native window rather than
an offscreen substitute:

- Initial window: `Ferryx Native Terminal POC`, 800x512 including titlebar,
  visible, focused, and not minimized.
- The captured content showed the Ferryx title, ANSI-colored text,
  CJK/Unicode cells, the long line, selection highlight, and cursor.
- A semantic click on the macOS full-screen control changed the same window to
  3840x1600. The captured full-screen surface retained the rendered content
  without a blank frame or visible tearing.
- The POC terminal produced no GPU or surface error after startup. Its test
  process was terminated after the check as the cleanup receipt; the Ferryx
  desktop application was left running.

Verdict: **Phase 2 native-window visual gate passed**. The POC intentionally
keeps its fixed 80x24 scene geometry at this stage; React geometry, DPI scaling,
and production-pane resize behavior remain Phase 3 work.
