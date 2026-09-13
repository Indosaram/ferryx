# Native terminal image rendering

## Outcome

Ferryx now connects libghostty-vt's existing Kitty image storage and placement
API to its native WGPU renderer. This is an integration of Ghostty's image
support, not a new terminal or a new image protocol implementation.

The original failure was reproduced through actual VT input and GPU readback:
the red image's pixel was `[0, 0, 0, 255]`, rather than `[255, 0, 0, 255]`.
The same regression now passes.

## Changes

- `src-tauri/src/native_terminal/png_decoder.rs`: installs the process-wide PNG
  decoder using the existing `png` dependency and Ghostty's allocator.
- `src-tauri/src/native_terminal/sys/kitty.rs`: binds the pinned image,
  placement, generation and geometry C APIs.
- `src-tauri/src/native_terminal/images.rs`: copies foreign image data into
  owned, generation-keyed shared pixel storage.
- `src-tauri/src/native_terminal/snapshot.rs`: carries native image placements.
  Image pixels are not included in the existing remote text serialization.
- `src-tauri/src/native_terminal/renderer/images.rs` and `renderer/pass.rs`:
  upload cached textures and composite them with clipping, source coordinates,
  straight alpha, and Kitty z layers.
- `src-tauri/src/terminal/pty.rs`: defaults `PI_IMAGE_PROTOCOL=kitty` for native
  PTYs unless explicitly configured. The installed Senpi client otherwise
  returns `images: null` for the current `TERM=xterm-256color`, absent
  `TERM_PROGRAM` environment. The override makes its actual capability
  detection return `images: "kitty"` without impersonating Ghostty.

Image storage is bounded to 64 MiB in Ghostty; this is not a total process-memory
limit, since decoded CPU snapshots, GPU textures and transient buffers also
consume memory. File, temporary-file and shared-memory transmissions remain
disabled.

Review found an accepted-image/GPU-limit mismatch: Ghostty accepts a 9000x1
PNG, but WGPU's requested device texture limit is 8192. The first implementation
returned `LimitExceeded` and prevented the entire terminal frame from being
presented. A real PNG regression reproduced that failure. The renderer now
omits placements beyond the actual device texture limit and continues drawing
text and supported images. Oversized images are not tiled or downscaled.

## Verification

Lead-run combined command:

```sh
FERRYX_IMAGE_EVIDENCE_DIR=docs/evidence/terminal-images-20260913 \
  cargo test --manifest-path src-tauri/Cargo.toml \
  --test native_terminal_images --test terminal_image_environment -- --nocapture
```

Exit code: 0.

- 10 real GPU tests passed: RGB; fragmented PNG; orientation and color;
  retransmission under the same ID; deletion; scrolling; reset; session
  isolation; viewport origin/clipping; oversized-image frame isolation; and a composed PNG evidence frame
  (some tests cover more than one property).
- 2 isolated real PTY tests passed: default image capability and explicit
  opt-out preservation.
- Lead reran 4 Kitty unit tests after the review fix: all passed.
- Implementation-worker checks: 171 native-terminal unit tests and 18
  surface-host contract tests passed before the narrow oversized-image fix.
- Lead `cargo build --manifest-path src-tauri/Cargo.toml --lib` passed after
  the review fix (exit 0).
- Independent review closed its single P1 finding after inspecting the fix
  and the real-PNG regression; no remaining finding in that narrow review.
- New module diagnostics reported no errors; `git diff --check` passed.
- Existing renderer contract target: 24 passed and 2 failed. The failures
  require `target/debug/native_terminal_renderer_poc`, while the manifest
  declares an example at `target/debug/examples/native_terminal_renderer_poc`.
  That unrelated harness was not changed.

Cargo executes these tests with `src-tauri` as the working directory. The
saved input and actual 640x384 WGPU viewport readback are:

- `src-tauri/docs/evidence/terminal-images-20260913/quadrants.png`
- `src-tauri/docs/evidence/terminal-images-20260913/native-png.png`

The readback is checked numerically for red/green above blue/yellow.
This session's image reader reported that its model cannot inspect image
attachments, so the numeric evidence is not described as human visual QA.

## Desktop confirmation

The running application and daemon were not restarted or replaced. The tests
exercise the real native renderer offscreen, not an installed application window.
Use the project's debug launch command from the repository:

```sh
bun tauri dev
```

In that debug application's terminal, from the repository root:

```sh
bun scripts/terminal-image-probe.mjs
bun scripts/terminal-image-probe.mjs \
  src-tauri/docs/evidence/terminal-images-20260913/quadrants.png
```

Both commands should show a rectangle with red/green on top and blue/yellow
below. Scroll it and split the pane: the image should move with terminal
content and remain inside the pane.

The already-running daemon and existing agent processes keep their old
environment. To check an existing Senpi installation in a current shell
without restarting the daemon, start the client with:

```sh
PI_IMAGE_PROTOCOL=kitty senpi
```

The native renderer still needs to be the newly built debug app. No forced
daemon restart is necessary for that manual capability override.

## Boundaries

This change covers native direct Kitty RGB/RGBA/PNG output. It does not add
Sixel, iTerm2 inline images, web/mobile image transport, or resolve Kitty
Unicode/virtual placements. The pinned C render-info API marks virtual
placements invisible and offers no placeholder resolver. Windows/Linux source
paths are portable, but their GPU runtime was not executed in this macOS
session. External Zig FFI and WGPU cannot be validated through Miri here.

Changes are uncommitted in the shared working tree.
