# Lead validation of atlas commit 17fb106

## Scope and verdict

The lead independently read the renderer/prepared-raster/shelf/cache code,
new GPU tests and direct contract changes in commit
`17fb10608ad31e0e60ae565e8dc686afdc4735f9`.
The renderer/API increment passes the evidence below. This is not a native
application-window, display-switch, or main-integration approval.

Worktree: `/Users/indo/code/project/orca-lite-atlas-20260906`.
Base: `e2a67859400e80c480de42a6ec687cda6235c275`.

## Default build resources, not a test-only override

The lead installed frozen UI dependencies and built the actual UI resources:
360 packages installed; `bun run build` completed in 2.08 seconds.
Then `cargo check --manifest-path src-tauri/Cargo.toml` ran with `TAURI_CONFIG`
unset and completed with exit 0 in 6.00 seconds. Seven existing unrelated
warnings remained visible.

Evidence:
`.omo/evidence/ulw/rendering-review-20260906/atlas/main-default-cargo-check.log`;
monitor `mon_GXFHXPS29TBECWM0` completed with exit 0.

This closes the default-resource prerequisite gap in the producer's earlier
GPU-only test environment. It does not turn those earlier receipts into
packaging or GUI evidence.

## Fresh lead execution

Changed renderer-file LSP error diagnostics returned `No diagnostics found`.
With normal resources and `TAURI_CONFIG` unset, the lead ran:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::renderer::renderer::tests:: -- --nocapture --test-threads=1
cargo run --manifest-path src-tauri/Cargo.toml --example native_terminal_renderer_poc -- --headless --output /Users/indo/code/project/orca-lite/.omo/evidence/ulw/rendering-review-20260906/atlas/main-poc-headless.png
```

Shell-local build variables:
`CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite/src-tauri/target`,
`CARGO_BUILD_JOBS=8`, and a new
`ATLAS_EVIDENCE_DIR=.../atlas/main-reproduction`.
Shared caches were not cleaned; existing artifacts were not overwritten.

- Eight GPU regressions passed; 0 failed/ignored, 600 unrelated tests filtered.
  The serial setting keeps diagnostic artifact writes ordered.
- Adapter: Apple M4 Max, Metal.
- Dense: both frames have all 1,000 glyph origins.
- Larger dense frame: all 4,000 full reference tiles match; one growth to 4096.
- Repeated failure, empty-raster pressure, oversized glyph, final history frame,
  public surface entry points and payload/config-change tests all pass.
- Headless POC: 50 frames, 800x480, final 1 rebuilt / 23 reused rows, exit 0.
  Its observed p50 3.109 ms and p95 6.602 ms are this small headless run's
  measurements, not desktop latency guarantees.

Logs: `main-native-regressions.log`, `main-poc-headless.log` under the atlas
evidence directory. Monitor `mon_XBWQF6RXNWYRY0J0` completed with exit 0.

## Independent retained-data audit

The lead used the resident Bun kernel to load the saved raw RGBA8 data and
dimension metadata, extract every complete 32x32 glyph tile, and compare it
byte-for-byte to independent fresh reference batches.

- 14,000 complete tile comparisons: 12,000 GREEN tiles all equal.
- The 2,000 RED tiles differ exactly at indices 992 through 999 in each frame.
- The 1,000 reference tiles contain 1,000 distinct hashes; the 4,000 reference
  tiles contain 4,000 distinct hashes. This is not an all-identical tofu oracle.
- History-only RED differs in 822 of 4,096 bytes; GREEN differs in zero.
- Empty-pressure frame matches all 4,096,000 reference bytes.
- Both smaller-frame recoveries match all 4,096 bytes.
- Both public surface API outputs match all 4,096,000 terminal bytes.
- The translated viewport's 223,232 outside pixels are all opaque black.

GREEN SHA-256:

- 1,000-key F0/F1:
  `9ee3000f2570969a9132e82b22ca4be21b77eeb1f4e72093c16fda96bdd5193d`.
- 4,000-key F0/F1:
  `f5b3bc1c56bda685d6c1b0cd6e6c70ae08e530c5ee10078e2fad4cba9e13c97a`.

Inputs are under the atlas evidence directory's `red/` and `green-final/`.
The audit did not modify those inputs or the prior audit report.

## Cleanup and limitations

Owned build, test and POC commands exited; no desktop, daemon, browser or server
was started. Per-test GPU objects and temporary renderers belonged to exited test
processes. Build caches and requested evidence remain retained.

The model cannot inspect image attachments, and the separate vision endpoint
failed to connect. The evidence above is numerical GPU/API verification, not a
visual-review claim. Real macOS 1x/2x compositor captures, user-visible pane
transitions and combined integration remain required.
