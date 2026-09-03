# Native terminal glyph compositing contract

Authoritative rules for how the WGPU renderer turns a rasterized glyph into pixels.
Read this before touching `renderer/shaders.rs`, `renderer/pipeline.rs`,
`renderer/atlas.rs`, or `renderer/coretext_raster.rs`.

## 1. The pipeline in one pass

1. `coretext_raster.rs` (macOS) draws the glyph white-on-transparent into a
   premultiplied RGBA CoreGraphics context and copies per-channel coverage into an
   atlas buffer.
2. `atlas.rs` uploads that buffer to the `Rgba8Unorm` mask texture. `Alpha` glyphs are
   expanded to `[a, a, a, a]`; `Subpixel` glyphs are uploaded verbatim.
3. The background pass writes each cell's background with `BlendState::REPLACE`.
4. The glyph pass draws one quad per cell with `BlendState::ALPHA_BLENDING`, so the GPU
   evaluates `fg * coverage + dst * (1 - coverage)` where `dst` is that cell background.

Net result per pixel:

```
bg * (1 - coverage) + fg * coverage
```

## 2. Coverage is gamma pre-compensated — do not remove it

`gpu_context.rs` deliberately prefers a **UNORM** (non-sRGB) swapchain so libghostty's
already-sRGB-encoded cell colors are not gamma-converted twice. The consequence is that
the GPU blend runs directly on sRGB-encoded numbers, which is photometrically wrong for
antialiased coverage.

With a naive linear ramp, an edge pixel at 0.5 coverage lands at byte value 127, which is
only ~21% of the intended light instead of 50%. Every stroke thins and the whole terminal
reads as soft. This was the confirmed cause of "Ferryx looks blurrier than cmux".

The glyph fragment shader therefore pre-compensates the ramp:

```wgsl
let coverage = pow(cov.a, 0.7142857) * in.color.a;
```

The exponent is `1 / 1.4`, a deliberately conservative midpoint.

- `1 / 2.2` (`0.4545`) is full linear correction — heavier, bolder strokes.
- `0.85` is closer to the old behavior — lighter, thinner strokes.

Changing the exponent is a one-line, user-visible weight decision. Removing it
reintroduces the blur.

## 3. LCD subpixel antialiasing is NOT available

`coretext_raster.rs` calls `CGContextSetShouldSmoothFonts(context, false)`. With font
smoothing off, CoreText emits **grayscale** antialiasing, so `R == G == B` in every
rasterized pixel.

Any buffer built from that output carries zero LCD information. Do not describe this
renderer as doing subpixel AA, and do not add per-channel compositing on top of it — the
channels are identical by construction, so it costs 4x memory and buys nothing.

Real LCD rendering would require enabling font smoothing **and** giving the shader the
destination color per pixel. That is a deliberate, separate project.

## 4. Two regressions this contract exists to prevent

Both shipped during the same investigation. Both are easy to reintroduce.

### Double attenuation — text too dark, edges washed out

The shader returned pre-multiplied color while the pipeline still used
`ALPHA_BLENDING`, so the GPU multiplied by alpha a second time:

```wgsl
return vec4<f32>(fg * cov.rgb * a, cov.a * a);  // WRONG: coverage applied twice
```

Symptom: glyphs darker than their configured ANSI color, low edge contrast, soft look.

### Solid color bars — terminal unusable

Switching the glyph pipeline to `BlendState::REPLACE` and compositing the background
inside the shader made every text row render as a filled rectangle.

Rule: the glyph pass stays on `ALPHA_BLENDING`, and `GlyphInstance` carries **no**
background color. The background pass already established `dst`.

## 5. The test that locks all of this

`src-tauri/tests/native_terminal_renderer_contract/offscreen_render.rs::test_glyph_pixels_blend_once_and_leave_uncovered_pixels_as_cell_background`

It renders a real cell through WGPU and compares **every** pixel against the CoreText
mask, asserting three things:

1. Covered pixels match `bg * (1 - coverage) + fg * coverage` with the corrected ramp.
2. Pixels with zero coverage equal the cell background exactly.
3. Antialiased edge pixels sit measurably above the gamma-naive ramp.

Assertion 2 is not optional padding. An earlier revision of this test skipped
zero-coverage pixels and stayed green while the entire terminal rendered as solid bars.
A glyph test that only inspects inked pixels cannot see a fill bug.
