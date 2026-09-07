### WGPU CompositeAlphaMode fallback triggers unreachable panic on Linux Wayland
- **ID**: L4-RENDERER-FONT-1
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/gpu_context.rs:15` - `        .unwrap_or(wgpu::CompositeAlphaMode::Auto)`
- **Why it breaks**: When `Opaque` compositing is not supported (standard for Wayland Vulkan subsurfaces which only expose `PreMultiplied`), `opaque_composite_alpha_mode` defaults to `CompositeAlphaMode::Auto`. In `wgpu-core` 30.0.1 (`device/surface_config.rs`), `Auto` only checks `[Opaque, Inherit]` and hits an `unreachable!("Fallback system failed to choose alpha mode...")` panic, crashing the entire application during surface configuration.
- **Fix**: In `src-tauri/src/native_terminal/renderer/gpu_context.rs::opaque_composite_alpha_mode`, inspect `alpha_modes` and fall back to `alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)` so `PreMultiplied` or any driver-supported mode is chosen when `Opaque` and `Inherit` are missing.
- **Status**: OPEN

### Windows GDI rasterizer lacks font fallback cascade causing CJK and Unicode to render as tofu
- **ID**: L4-RENDERER-FONT-2
- **Severity**: BLOCKER
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:144` - `        let font = CreateFontW(`
- **Why it breaks**: `directwrite_raster.rs` rasterizes glyphs via legacy Win32 GDI `CreateFontW` rather than DirectWrite, passing only the primary font face with zero font fallback cascade logic. When rendering Korean, Japanese, Chinese, mathematical symbols, or box-drawing characters absent from the primary font (e.g. Consolas), GDI produces missing-glyph tofu rectangles or blank cells, leaving East Asian text unreadable.
- **Fix**: Replace Win32 GDI with DirectWrite (`IDWriteFactory::CreateTextLayout` and `IDWriteFontFallback::MapCharacters`) or implement a font fallback cascade that maps unhandled codepoints to fallback faces such as "Malgun Gothic" / "Meiryo" / "Microsoft YaHei" / "Segoe UI Symbol" before rasterization.
- **Status**: OPEN

### Linux FreeType rasterizer lacks CJK fallback cascade and drops non-monospace scripts
- **ID**: L4-RENDERER-FONT-3
- **Severity**: BLOCKER
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:182` - `    let Some(path) = resolve_font_path(family, ch, bold, italic)`
- **Why it breaks**: `freetype_raster.rs` attempts to match glyphs against only the user-configured font family and the literal fallback `"monospace"`. Standard Linux monospace fonts (e.g. DejaVu Sans Mono, Liberation Mono) do not contain CJK glyphs, and because no system fallback cascade is queried when "monospace" lacks the character, `resolve_font_path` returns `None` and Korean/Japanese/Chinese text fails to rasterize, displaying as blank tofu cells.
- **Fix**: In `src-tauri/src/native_terminal/renderer/freetype_raster.rs::resolve_font_path`, query Fontconfig without restricting family (e.g. `format!(":charset={:x}", ch as u32)`) when both the configured family and "monospace" lack coverage, letting Fontconfig resolve the system CJK font (e.g. Noto Sans CJK).
- **Status**: OPEN

### Color emoji rasterization is completely unimplemented on Windows and Linux
- **ID**: L4-RENDERER-FONT-4
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/color_glyph.rs:191` - `    pub fn rasterize_color_glyph(`
- **Why it breaks**: `color_glyph.rs` stubs out `rasterize_color_glyph` on `not(target_os = "macos")` to unconditionally return `None`, while the macOS implementation rasterizes via CoreText and Apple Color Emoji into RGBA buffers. On Windows and Linux, all emoji codepoints fail the color path and fall through to monochrome alpha rasterization, where GDI renders empty/corrupt glyphs and FreeType renders flat uncolored shapes.
- **Fix**: Implement `rasterize_color_glyph` for Windows using DirectWrite with `DWRITE_GLYPH_IMAGE_FORMATS_COLR | DWRITE_GLYPH_IMAGE_FORMATS_PNG | DWRITE_GLYPH_IMAGE_FORMATS_SVG` targeting "Segoe UI Emoji", and for Linux using FreeType with `FT_LOAD_COLOR` targeting "Noto Color Emoji".
- **Status**: OPEN

### Non-macOS cell metrics hardcode arbitrary 0.6 and 1.25 ratios instead of querying font metrics
- **ID**: L4-RENDERER-FONT-5
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:133` - `                width_px: (fs * 0.6).round().max(1.0) as u32,`
- **Why it breaks**: On Windows and Linux, `FontManager::cell_metrics_for_scale` ignores the configured font and returns hardcoded synthetic metrics `(fs * 0.6)` by `(fs * 1.25)`. Actual monospace fonts on Windows (Consolas, Cascadia Code) and Linux (DejaVu Sans Mono) have advance widths and line height ratios differing from 0.6 / 1.25, leading to severe cell-to-glyph misalignment, clipped ascenders/descenders, and broken box-drawing continuous lines.
- **Fix**: In `src-tauri/src/native_terminal/renderer/font_manager.rs`, derive real font metrics for Windows via Win32 `GetTextMetricsW` / DirectWrite `GetMetrics`, and for Linux via FreeType `FT_FaceRec` (`max_advance_width`, `ascender`, `descender`, `height`).
- **Status**: OPEN

### Comma-separated font stack passed unparsed to Windows GDI exceeding LF_FACESIZE
- **ID**: L4-RENDERER-FONT-6
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:115` - `    let face = wide(family);`
- **Why it breaks**: Ferryx's default font stack (`'MesloLGS NF, "Noto Sans KR", monospace'`) is passed unparsed as a raw string to `directwrite_raster.rs`. Win32 GDI `CreateFontW` accepts only a single typeface name up to 32 characters (`LF_FACESIZE`); because the 39-character comma-separated string exceeds `LF_FACESIZE` and does not match any single font, GDI rejects it and falls back to the proportional system GUI font (e.g. MS Sans Serif), breaking fixed-width terminal rendering entirely.
- **Fix**: Parse comma-separated font stacks in `FontManager` or `directwrite_raster.rs`, strip quotes, and resolve the first locally installed font name before invoking Win32 GDI `CreateFontW`.
- **Status**: OPEN

### Linux FreeType rasterizer reloads fontconfig and opens font files from disk per glyph
- **ID**: L4-RENDERER-FONT-7
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:134` - `        let config = FcInitLoadConfigAndFonts();`
- **Why it breaks**: For every single glyph cluster rasterized, `resolve_font_path` calls `FcInitLoadConfigAndFonts()` to re-parse all fontconfig XML files from disk, followed by `FT_New_Face` to read and parse the font binary, and `FT_Done_Face` to discard it. This causes extreme disk I/O, file descriptor churn, and massive frame stuttering during terminal scrolling and viewport resizes.
- **Fix**: Use `FcConfigGetCurrent()` instead of `FcInitLoadConfigAndFonts()`, and maintain a thread-safe cache of open `FT_Face` handles keyed by font path and size rather than opening and destroying faces per glyph.
- **Status**: OPEN

### Missing ntdll link dependency for Zig static library on Windows MSVC
- **ID**: L4-RENDERER-FONT-8
- **Severity**: HIGH
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/native_terminal/build_ghostty.rs:369` - `    println!("cargo:rustc-link-lib=static={}", config.link_lib_stem);`
- **Why it breaks**: `GhosttyLibVt.zig` configures Windows static builds to link against NT system libraries (`lib.root_module.linkSystemLibrary("ntdll", .{})`), but `build_ghostty.rs` only instructs cargo to link `config.link_lib_stem`. When linking with MSVC (`x86_64-pc-windows-msvc`), `ntdll.lib` is not linked by default by `rustc`, causing unresolved external symbol linker errors for NT runtime APIs.
- **Fix**: In `src-tauri/native_terminal/build_ghostty.rs`, add `if config.target.contains("windows") { println!("cargo:rustc-link-lib=ntdll"); }`.
- **Status**: OPEN

### Missing pkg-config probing and linker search paths for FreeType and Fontconfig on Linux
- **ID**: L4-RENDERER-FONT-9
- **Severity**: HIGH
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:70` - `#[link(name = "freetype")]`
- **Why it breaks**: `freetype_raster.rs` declares direct `#[link(name = "freetype")]` and `#[link(name = "fontconfig")]` bindings without any `pkg-config` probe in `src-tauri/build.rs`. On Linux distributions where development libraries reside in multiarch paths (e.g. `/usr/lib/x86_64-linux-gnu`) or custom prefixes, the linker fails to locate `-lfreetype` or `-lfontconfig`, breaking native terminal compilation.
- **Fix**: In `src-tauri/build.rs`, add a Linux target branch that probes `pkg_config::Config::new().probe("freetype2")` and `pkg_config::Config::new().probe("fontconfig")` to emit `cargo:rustc-link-search` and `cargo:rustc-link-lib` directives.
- **Status**: OPEN

### WGPU adapter request with compatible_surface: None fails on hybrid GPU Windows and Linux systems
- **ID**: L4-RENDERER-FONT-10
- **Severity**: HIGH
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/gpu_context.rs:59` - `            compatible_surface: None,`
- **Why it breaks**: `GpuContext::new` initializes the GPU adapter requesting `HighPerformance` with `compatible_surface: None`. On hybrid-graphics laptops (e.g. Intel/Nvidia Optimus on Windows, or Prime on Linux), the discrete GPU adapter selected may not be compatible with the display window surface created later, resulting in `surface.get_capabilities(&self.adapter).formats` returning an empty slice and `configure_surface` failing with `GpuPipelineError`.
- **Fix**: In `src-tauri/src/native_terminal/renderer/gpu_context.rs`, validate surface compatibility in `configure_surface` and fall back to requesting an adapter compatible with the actual `wgpu::Surface` if `cap.formats.is_empty()`.
- **Status**: OPEN

### Linux FreeType rasterizer drops combining characters in multi-codepoint grapheme clusters
- **ID**: L4-RENDERER-FONT-11
- **Severity**: MEDIUM
- **Platforms affected**: Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/freetype_raster.rs:176` - `    let Some(ch) = text.chars().next().filter(|c| !c.is_whitespace()) else {`
- **Why it breaks**: `freetype_raster::rasterize_to_alpha_buffer` extracts only `text.chars().next()`, completely discarding all trailing characters in `text`. Any terminal cell containing a combining mark (such as accent marks like `e` + `\u{0301}`, Hangul jamo sequences, or ZWJ sequences) renders only the base character with all combining diacritics stripped.
- **Fix**: Iterate through all characters in `text`, resolve and render each glyph into the buffer using additive blending and horizontal offset tracking, matching the macOS combining-mark loop in `FontManager::rasterize_glyph_for_scale`.
- **Status**: OPEN

### Windows GDI rasterizer lacks vertical baseline positioning and draws glyphs at cell top
- **ID**: L4-RENDERER-FONT-12
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:168` - `        TextOutW(dc, 0, 0, glyphs.as_ptr(), glyph_len);`
- **Why it breaks**: Win32 GDI `TextOutW` draws glyphs at `(0, 0)` with default top-left alignment rather than positioning along a typographic baseline. Glyphs with descenders ('g', 'y', 'p') are pushed downward and clipped at the bottom cell boundary, while uppercase characters and box-drawing lines appear shifted upward and misaligned with adjacent horizontal rows.
- **Fix**: Call `GetTextMetricsW(dc, &mut tm)` in `directwrite_raster.rs` to query `tmAscent`, set `SetTextAlign(dc, TA_TOP | TA_LEFT)`, and calculate vertical offset `(cell_height - (tm.tmAscent + tm.tmDescent)) / 2` to vertically center characters within the cell.
- **Status**: OPEN

### Subpixel coverage rendering and subpixel positioning are macOS-exclusive
- **ID**: L4-RENDERER-FONT-13
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:299` - `        if subpixel_buffer.iter().any(|&b| b != 0) {`
- **Why it breaks**: On Windows and Linux, `rasterize_glyph_for_scale` invokes only the monochrome `rasterize_to_alpha_buffer` path, leaving `subpixel_buffer` all zeroes and forcing `FontManager` to always return `RasterizedGlyph::Alpha`. Subpixel positioning and RGBA subpixel antialiasing implemented via CoreGraphics on macOS are completely unavailable on Windows and Linux, resulting in noticeably blurrier text on standard-DPI LCD monitors.
- **Fix**: Implement subpixel RGBA coverage rendering in `directwrite_raster.rs` (using DirectWrite `IDWriteBitmapRenderTarget` with ClearType) and in `freetype_raster.rs` (using `FT_RENDER_MODE_LCD`), populating `subpixel_buffer`.
- **Status**: OPEN

### Variable font weight instantiation and synthetic styles missing on Windows and Linux
- **ID**: L4-RENDERER-FONT-14
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:21` - `    system: Mutex<CoreTextFontSystem>,`
- **Why it breaks**: `FontManager` encapsulates `CoreTextFontSystem` solely on macOS; on Windows and Linux, `system` is replaced by a bare `Mutex<f32>` font size. Features implemented in `coretext_font.rs` and `coretext_raster.rs`—such as variable font `wght` axis interpolation via `kCTFontVariationAttribute`, synthetic bold horizontal dilation, and synthetic italic row shearing—have no Windows or Linux equivalent, causing bold/italic text in fonts lacking separate face files to render as plain regular text.
- **Fix**: Introduce a cross-platform font system trait implemented by `CoreTextFontSystem`, DirectWrite on Windows (supporting `IDWriteFontFace5` variable axes), and FreeType on Linux (supporting `FT_Set_Var_Design_Coordinates`, `FT_Outline_Embolden`, and `FT_Outline_Transform`).
- **Status**: OPEN

### Per-glyph GDI allocation thrashing risks process quota exhaustion on Windows
- **ID**: L4-RENDERER-FONT-15
- **Severity**: MEDIUM
- **Platforms affected**: Windows
- **Evidence**: `src-tauri/src/native_terminal/renderer/directwrite_raster.rs:125` - `        let dc = CreateCompatibleDC(std::ptr::null_mut());`
- **Why it breaks**: `directwrite_raster::rasterize_to_alpha_buffer` creates and releases a device context (`CreateCompatibleDC`), a DIB section (`CreateDIBSection`), and a font (`CreateFontW`) for every single glyph rasterized. Under heavy atlas filling or during rapid terminal streaming, this per-glyph GDI object thrashing incurs high kernel transition latency and risks triggering Windows' 10,000 GDI object per-process limit if any allocation fails to clean up cleanly.
- **Fix**: Cache and reuse a thread-local memory DC and DIB section, and retain an LRU cache of `HFONT` handles keyed by font family, size, weight, and slant.
- **Status**: OPEN

### Secondary color glyph fallback path is gated exclusively to macOS
- **ID**: L4-RENDERER-FONT-16
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:255` - `            if !rendered_base && is_secondary_color_candidate(first_ch) {`
- **Why it breaks**: `FontManager::rasterize_glyph_for_scale` implements a secondary fallback for arrows and miscellaneous technical symbols (e.g. `is_secondary_color_candidate`), rasterizing them as color glyphs if the primary alpha font produces no ink. This entire fallback check is enclosed inside `#[cfg(target_os = "macos")]`, so on Windows and Linux, arrow and technical symbols that lack glyphs in the primary face are never routed to secondary color rendering.
- **Fix**: Move the `is_secondary_color_candidate` check outside the `#[cfg(target_os = "macos")]` block in `src-tauri/src/native_terminal/renderer/font_manager.rs` and provide a cross-platform color rasterizer implementation.
- **Status**: OPEN

### Font manager test assertions assume 4-byte subpixel buffers and panic on Windows and Linux
- **ID**: L4-RENDERER-FONT-17
- **Severity**: MEDIUM
- **Platforms affected**: Windows+Linux
- **Evidence**: `src-tauri/src/native_terminal/renderer/font_manager.rs:354` - `            (metrics.width_px * metrics.height_px * 4) as usize`
- **Why it breaks**: Test-only: `font_manager::tests` (`test_font_manager_derives_nonzero_metrics_and_rasterizes`, `test_glyph_orientation_regression`, etc.) assert that `rasterize_glyph` returns a buffer of length `width * height * 4` and index bytes at `[idx + 3]`. Because Windows and Linux only generate 1-byte-per-pixel `RasterizedGlyph::Alpha` buffers, these assertions fail and slice indexing panics with index-out-of-bounds on non-macOS test runs.
- **Fix**: Update `font_manager.rs` unit tests to inspect `RasterizedGlyph::is_subpixel()` and scale the expected buffer length and stride dynamically (4 for subpixel, 1 for alpha).
- **Status**: OPEN

### Default terminal font stack hardcodes macOS and Nerd Font families without Windows/Linux fallbacks
- **ID**: L4-RENDERER-FONT-18
- **Severity**: LOW
- **Platforms affected**: Windows+Linux
- **Evidence**: `ui/src/lib/tauri.ts:3` - `export const DEFAULT_TERMINAL_FONT_STACK = 'MesloLGS NF, "Noto Sans KR", monospace';`
- **Why it breaks**: The default terminal font stack hardcodes `MesloLGS NF` and `Noto Sans KR`, neither of which is bundled or pre-installed on standard Windows or Linux installations. Windows users lack "Cascadia Mono" or "Consolas", and Linux users lack "DejaVu Sans Mono" or "Liberation Mono" in the default font stack, causing Ferryx to immediately hit fallbacks on vanilla OS setups.
- **Fix**: In `ui/src/lib/tauri.ts`, update `DEFAULT_TERMINAL_FONT_STACK` to include cross-platform monospace families: `'MesloLGS NF, "Cascadia Mono", Consolas, "DejaVu Sans Mono", "Noto Sans KR", monospace'`.
- **Status**: OPEN
