# Unicode Font Rendering Diagnosis — 2026-08-27

## Verdict

Ghostty config import is working correctly. The broken Korean/Unicode rendering
comes from the installed **Noto Sans KR Variable Font rendering with its Thin
default instance**, because Ferryx's native rasterizer (ab_glyph) cannot apply
variable-font weight coordinates. Real Ghostty renders the same config fine
because CoreText performs proper VF instancing.

## Evidence

User's `~/.config/ghostty/config`:

```
font-family = "MesloLGS NF"
font-family = "Noto Sans KR"
```

Load chain verified end to end:

1. `ghostty +show-config` emits both `font-family` lines (plus bold/italic
   variants — the doubled plain lines come from Ghostty reading both the XDG and
   App Support copies of the symlinked file; harmless).
2. `parse_ghostty_config` joins them into `"MesloLGS NF, Noto Sans KR,
   monospace"` (dedup + empty-value reset semantics, tested).
3. A standalone probe replicating
   `native_terminal/renderer/font_manager.rs` resolution (`fontdb 0.16.2 +
   ab_glyph 0.2.32`, 920 indexed system faces) confirms:
   - Latin, `·`, `✓`, powerline E0B0 / F115 → MesloLGS NF primary (correct).
   - Hangul `실/가` → the configured Noto face, but that face IS THIN (below).

## Root cause: NotoSansKR-VariableFont_wght.ttf defaults to Thin

Binary fvar/OS2 parse of `/Users/indo/Library/Fonts/NotoSansKR-VariableFont_wght.ttf`:

```
fvar axis wght: min=100 default=100 max=900   <- default = Thin(100)
OS/2 usWeightClass = 100                      <- fontdb indexes it as Weight(100)
postScriptName   = NotoSansKR-Thin
```

- ab_glyph exposes no public API to set variation coordinates (0.2.32
  `variable.rs` only declares the axis struct), so rendering uses the default
  master outlines = **Thin**.
- Because it is a single-face variable file, fontdb cannot weight-match a
  Regular instance either.
- Rasterized ink density at 13pt: `실` = **0.131** via this face.

If the Noto line were absent, the renderer sweeps all 920 faces in
Debug-string sort order; the first Hangul-capable hit is
**AppleSDGothicNeo-Light (300)** — also too thin. Missing PUA icons beyond
MesloLGS NF's subset can land on arbitrary CJK faces in that sweep, producing
wrong-glyph garbage such as the `(a真` shown in agent status lines.

## Verified working fallback

With `"Apple SD Gothic Neo"` as the second family, fontdb picks
`AppleSDGothicNeo-Regular` (Weight 400) from the static system TTC, `ab_glyph`
loads it fine, `실/가` resolve there, density = **0.266** (~2× the Thin output;
Regular weight visually matches the screenshot's expectations).

## Fix options

1. Immediate user-side (zero code): in `~/.config/ghostty/config`, change the
   second line to `font-family = "Apple SD Gothic Neo"`, or install static
   Noto Sans KR weights instead of the single variable TTF. Restart or use
   Settings re-import so `cmd_terminal_preferences` re-imports.
2. Code-level (proper): support variable-font instantiation for the fallback
   chain (wght coordinate application) or route missing-glyph fallback through
   CoreText on macOS like Ghostty does; additionally replace the all-system-face
   deterministic sweep with a curated/scored fallback list so PUA icons cannot
   resolve into arbitrary CJK faces.

## Root fix implemented 2026-08-27 (src-tauri)

1. **Variable-font weight instantiation** (`font_manager.rs`): `LoadedFace` now carries
   `Mutex<FontVec>` plus a parsed `wght_axis` (ab_glyph `VariableFont`). Faces instantiate
   `wght=400` for regular cells/metrics and `wght=700` for bold cells, clamped to axis
   `[min,max]`. The lock only covers set-coord → outline extraction into owned data;
   pixel rasterization runs outside the guard. Static faces behave identically to before.
   ⇒ The configured `"Noto Sans KR"` variable font now renders Regular weight.
2. **CoreText cascade fallback tier** (`renderer/fallback_resolver.rs`, new, macOS):
   inserted between configured families and the legacy sweep. Asks CoreText which installed
   font supplies the missing character, maps the returned PostScript name back into fontdb,
   optionally loading the file. Non-macOS keeps prior behavior (stub returning None).
3. **Sweep hygiene**: last-resort sweep is sorted by `|face_weight − 400|` ascending with the
   Debug-string tiebreak, so Light/Thin CJK faces no longer hijack arbitrary missing glyphs.
   Accessor: `FontManager::sweep_fallback_ids()`.

Tests added/rewritten (cargo test --lib, all green): VF instantiation bounds equal an
independently wght-400-instantiated clone; sweep ordering deterministic through production
code and Regular-band Hangul faces precede Light/Thin ones; CoreText resolver returns
AppleSDGothicNeo Regular/Medium/Bold (never `-Light`) for '한' given ["MesloLGS NF"].

User-side config change from earlier recommendation (swap to Apple SD Gothic Neo) is NO
LONGER needed — Ghostty config stays as-is.

Probe source kept at `/tmp/fontprobe` during investigation; safe to delete.

## FINAL STATE 2026-08-28: full CoreText font stack (option B shipped)

The ab_glyph/fontdb renderer was replaced entirely by CoreText after the emoji gap and a
mid-migration live-app flip incident:

- `coretext_font.rs`: CTFont lifecycle per style (symbolic traits), explicit
  `kCTFontVariationAttribute` wght=400/700 for variable fonts — a probe proved CoreText
  defaults to the fvar default instance (this machine's Noto Sans KR VF defaults to
  Thin(100): ink 0.098 vs Regular 0.188 without the attribute), and per-char cascade via
  `CTFontCreateForString` replacing the configured-tier + sweep + fallback_resolver.rs
  (module deleted).
- `coretext_raster.rs`: CGBitmapContext RGBA alpha extraction. CRITICAL: CTLineDraw into a
  bitmap context is ALREADY top-down — row mapping must be `src_y = y`, NO flip. A flip
  renders every glyph upside down. Orientation regression tests ('L' bottom-heavy,
  'P' top-heavy, 'g' descender, '─' centered) lock this for both raster paths.
- `color_glyph.rs`: emoji path aligned to the same no-flip mapping; the earlier flip made
  emoji render upside down while coverage-only tests passed. New '⬆️'/'⬇️' asymmetry tests
  catch vertical inversion of color glyphs.
- `font_manager.rs`: 1137 → 573 LOC facade; ab_glyph + fontdb dependencies removed from
  Cargo.toml entirely; zero references remain in the crate.
- Metrics parity locked by test: MesloLGS NF @13pt = 8x16 cells.
- Gates: cargo test --lib (335 passed) + native_terminal_renderer_contract (21 passed),
  exit 0, re-run independently.

Incident note: the dev runner watch-rebuilt the app from the live working tree mid-task and
the user saw fully flipped text from an intermediate build. See memory
`ferryx-devrunner-live-rebuild-hazard` for the pause-worktree-or-gate rule.
