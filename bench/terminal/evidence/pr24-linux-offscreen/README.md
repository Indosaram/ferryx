# PR 24: actual Linux offscreen renderer QA

Measured on 2026-10-26. These are **real WGPU renders**, not mocked output or a GUI screenshot.
The desktop app was not launched: local Unix-socket binding is restricted. No Xvfb,
namespace workaround, compositor, external network, signing, or installation was used.

## Source and environment

- Baseline production source: `eab2e8e4712e60fa155cf794ad5885afff943244`.
- Head production source: `8837b2ae958d74ba23e28f6ea3e3900c79b59355` (the additional pipeline QA example is not used here).
- Published equivalent commit: `d2b3e909e78f93ee0e0da880f7e985f348d6e254`; both head commits have tree `9da99219209f2e60dcfb33d6d4e905f1f00b260f` (verified before adding QA artifacts).
- Both existing examples built from their respective checkouts with default features,
  the dev profile, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`, one build job,
  rustc 1.99.1, and the same shared toolchain/dependency cache. No renderer source edits.
- Linux x86_64; **llvmpipe (LLVM 19.1.7, 256 bits), Vulkan, CPU adapter**.
- Official Debian trixie GTK/WebKit/FreeType/Vulkan dependencies extracted into a local
  sysroot. Fresh private HOME/XDG/cache directories per revision. DISPLAY and
  WAYLAND_DISPLAY unset; `EGL_PLATFORM=surfaceless`, `LIBGL_ALWAYS_SOFTWARE=1`,
  `VK_DRIVER_FILES` and `VK_ICD_FILENAMES` select the lavapipe ICD.
- No concurrent build/benchmark ran during either measurement.

## Results

| Check | Baseline | Head |
| --- | --- | --- |
| Existing renderer POC, 50 actual offscreen frames, p50 | 2.653 ms | 3.331 ms |
| Same run, p95 | 5.211 ms | 4.942 ms |
| Last-frame dirty rows | 1 rebuilt / 23 reused | 1 rebuilt / 23 reused |
| Glyph atlas | 111 entries / 8,395,712 allocated bytes | Same |
| Renderer image | 800 × 480 pixels | Byte-identical |
| Synthetic attributes image | 600 × 200 pixels | Byte-identical |
| Live Ghostty attributes → WGPU image | 800 × 480 pixels | Byte-identical |
| Both example exit statuses | 0 | 0 |

One 50-frame run per revision is a **smoke measurement, not a statistically established
speedup/regression**. The search/history changes do not alter this rendering path.
These CPU-software-adapter numbers cannot stand in for native hardware GPU, macOS, or
full-app input-to-display latency. See the separate pipeline measurements for the changed paths.

## Visual QA matrix and known gaps

| Area | Evidence / conclusion |
| --- | --- |
| ASCII, selection, block cursor, CJK | Visible in [head](native-render.png); identical [baseline](baseline-native-render.png) |
| Bold, faint, italic, underline, strike, inverse, invisible | Inspected [synthetic](attributes.png) and [live parser](attributes-live.png) output; baseline images identical |
| Crab emoji | **Missing in baseline and head**; not a regression introduced by this PR |
| Decomposed `e` + combining acute | **Accent missing in baseline and head**; Linux `freetype_raster.rs` selects only `text.chars().next()`, so it does not rasterize the full grapheme |
| Full app, native window/compositor, IME, real PTY interaction | **Not covered** by these offscreen examples |
| macOS / Windows / hardware GPU | **Not run here** |

No font implementation was changed to hide these pre-existing gaps. Both revisions
produce byte-identical PNGs, including the missing glyphs. Noto Color Emoji is installed
on the host; absence of that font is not an adequate explanation for the blank emoji.

## Reproduce

Build each checkout's existing examples with the same toolchain/profile/environment:

```sh
cargo build --offline --locked --manifest-path src-tauri/Cargo.toml \
  --example native_terminal_renderer_poc --example terminal_attribute_evidence
# Supply a socket-free Vulkan software adapter and writable private HOME/XDG dirs.
# Choose distinct output paths and preserve logs for each source revision.
<target>/debug/examples/native_terminal_renderer_poc --headless --output native-render.png
<target>/debug/examples/terminal_attribute_evidence attributes.png
```

The second command also writes `attributes-live.png`. Raw example output is retained
in the four adjacent `.log` files. Timings include the example's actual render/readback
work; its 50 iterations are not GUI presentation timestamps.

## PNG SHA-256

- `native-render.png` and `baseline-native-render.png`: `4f66a7d616b313dbec1d0fdbaa9806ab5d323a5b24407a85b36d7041cbe19b89`
- `attributes.png` and `baseline-attributes.png`: `065cbf60404c5d5316776231331a88900cb84c2d9d574a66878d74b59ac07030`
- `attributes-live.png` and `baseline-attributes-live.png`: `33da87baec0616f9deadc1994685515a543467646a1290a1107c682494e97e7d`
