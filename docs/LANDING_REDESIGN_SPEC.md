# Ferryx Landing Redesign Spec (cursor.com-inspired)

Authoritative design contract for the `site/` landing page redesign. Every value below is
binding: implementers must use these exact tokens rather than inventing near-neighbours.

Reference: https://cursor.com (captured 2026-08-26, 1440x900). Traits we adopt:
warm off-white page, floating pill navbar, oversized tight-tracked headline, restrained
monochrome buttons, and a dark product window floating on a soft photographic backdrop.
Traits we do NOT adopt: Cursor's copy, logo cloud, or any fabricated metric.

## 1. Palette

Light page surface (landing chrome). Use these literal hexes via arbitrary Tailwind values.

| Token | Hex | Use |
|---|---|---|
| page | `#f6f4f1` | Page background (warm off-white) |
| page-raised | `#fbfaf8` | Alternating section background |
| surface | `#ffffff` | Cards, panels |
| ink | `#0d0d0e` | Primary text, primary button fill |
| ink-soft | `#56565a` | Body copy, descriptions |
| ink-faint | `#8a8a90` | Meta, captions, eyebrow labels |
| line | `#e4e0da` | Borders, dividers, table rules |
| line-strong | `#d5d0c8` | Hover borders |
| dark-surface | `#0e0e10` | Demo window chrome, dark inline code |

The embedded desktop demo (`LiveFerryxDemo`, `@ui/*` components) stays dark. Do not touch
`--sidebar*`, `--terminal`, `--worktree-sidebar*`, or any desktop token in `index.css`, and
keep `class="dark"` on `<html>` so those components keep rendering as the real app does.

## 2. Typography

- Family: existing Inter stack. Mono stays JetBrains Mono.
- Display headline (hero): `text-[clamp(2.75rem,7vw,5.25rem)] font-medium tracking-[-0.045em] leading-[0.95] text-[#0d0d0e]`
- Section headline: `text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05]`
- Card title: `text-[17px] font-medium tracking-[-0.015em]`
- Body: `text-[15px] leading-relaxed text-[#56565a]`; hero sub `text-lg sm:text-xl`
- Eyebrow / meta: `text-[11px] font-medium uppercase tracking-[0.14em] text-[#8a8a90]`
- Never use `font-extrabold` or `font-bold` for display type. Cursor's weight is medium.

## 3. Shape and depth

- Radii: buttons/pills `rounded-full`; cards `rounded-2xl`; demo window `rounded-[20px]`;
  panels `rounded-3xl`.
- Borders: 1px `#e4e0da`. Hover `#d5d0c8`.
- Shadows: cards `shadow-[0_1px_2px_rgba(13,13,14,0.04)]`; floating nav
  `shadow-[0_8px_30px_rgba(13,13,14,0.08)]`; demo window
  `shadow-[0_40px_80px_-20px_rgba(13,13,14,0.45)]`.
- Section rhythm: `py-24 sm:py-28`. Content width `max-w-6xl`, prose blocks `max-w-2xl`.

## 4. Buttons

- Primary: `rounded-full bg-[#0d0d0e] text-white hover:bg-[#26262a] px-5 h-11 text-[15px] font-medium`
- Secondary: `rounded-full bg-white text-[#0d0d0e] border border-[#e4e0da] hover:border-[#d5d0c8] hover:bg-[#fbfaf8]`
- Ghost: `text-[#56565a] hover:text-[#0d0d0e] hover:bg-[#0d0d0e]/[0.04]`
- Transitions: `transition-colors duration-150`. No scale-on-hover on buttons.

## 5. Navbar

Floating pill, not a full-width bar:

- Wrapper: `fixed top-0 inset-x-0 z-50 px-4 pt-4` (page adds top padding to compensate).
- Pill: `mx-auto flex h-14 max-w-6xl items-center justify-between rounded-full border border-[#e4e0da] bg-[#fbfaf8]/85 px-5 backdrop-blur-xl` + floating-nav shadow.
- Left: icon `h-7 w-7 rounded-lg` + wordmark `text-[15px] font-medium tracking-[-0.02em]`.
- Center: nav links `text-[14px] text-[#56565a] hover:text-[#0d0d0e]`, `gap-7`, hidden below `md`.
- Right: GitHub icon-button (ghost, `rounded-full`) + `DownloadMenu variant="navbar"`.
- Keep the existing version badge but restyle: `border-[#e4e0da] bg-white text-[#56565a]`.

## 6. Hero

- Section: `relative pt-36 pb-0 text-center` on page background. Remove the dark grid overlay;
  replace with a soft radial wash `bg-[radial-gradient(60%_50%_at_50%_0%,rgba(13,13,14,0.05),transparent)]`.
- Headline (two lines, display scale): `Parallel agentic development.` / `Zero bloat.`
- Sub (max-w-2xl, `text-[#56565a]`): keep the truthful product sentence describing native
  Ghostty engine, wgpu rendering, split panes, embedded browser, mobile remote.
- Actions row: primary `DownloadMenu variant="hero"` + secondary GitHub button.
- Below actions: the direct-download platform links, restyled as `text-[13px] text-[#8a8a90]`
  with `hover:text-[#0d0d0e]`.
- The hero no longer renders the demo placeholder or the second button row; the demo lives in
  its own stage section (§7) and the `children` prop is removed.

## 7. Demo stage (the photographic backdrop)

New section immediately after the hero, owning the background image.

- Asset: `site/public/demo-bg.jpg` (1024x683). Reference it as `${baseUrl}demo-bg.jpg`.
- Structure:
  - Outer `<section id="preview" class="relative overflow-hidden pt-12 pb-24 sm:pb-28">`
  - Background layer: absolutely positioned `inset-x-0 top-0 h-[860px]`, `bg-cover bg-center`,
    inline `backgroundImage: url(...)`, plus `scale-105 blur-[1px]` for depth. The layer is
    height-capped rather than `inset-0` so it cannot stretch with the section.
  - Wash layer over the image, same box: a hard-stopped vertical gradient
    `bg-[linear-gradient(to_bottom,#f6f4f1_0%,rgba(246,244,241,0.12)_22%,rgba(246,244,241,0.12)_58%,#f6f4f1_96%)]`.
    The 0.12-alpha plateau between 22% and 58% is what keeps the photograph readable; a
    single `via-*/45` three-stop gradient flattens it to grey haze and is not acceptable.
    Verify by sampling gutter pixels: the mid-band must show per-channel spread (colour), not
    a uniform grey around RGB 173/173/168.
  - Foreground: `relative mx-auto max-w-5xl px-4 sm:px-6` wrapping `LiveFerryxDemo`.
- The demo window itself: `rounded-[20px] ring-1 ring-black/10` + demo-window shadow.
  Keep its dark interior and its 560px height; only the frame treatment changes.
- Remove the old `-mt-[560px]` overlap hack from `index.astro`.

## 8. Features

- Section on `#fbfaf8`, `border-t border-[#e4e0da]`.
- Header: eyebrow `Why Ferryx`, section headline `Speed. Isolation. Total control.`, sub copy.
- Grid `md:grid-cols-2 lg:grid-cols-3 gap-4`.
- Card: `rounded-2xl border border-[#e4e0da] bg-white p-6` + card shadow, hover
  `border-[#d5d0c8]`. Icon in a `h-9 w-9 rounded-xl bg-[#f6f4f1] border border-[#e4e0da]`
  tile with `text-[#0d0d0e]` glyph at `h-4 w-4`. Drop the giant watermark icon and the
  per-card badge; keep the six feature titles and descriptions verbatim.

## 9. Benchmarks / architecture matrix

- Section on page background, `border-t border-[#e4e0da]`.
- Panel: `rounded-3xl border border-[#e4e0da] bg-white overflow-hidden`.
- Header row inside panel: `border-b border-[#e4e0da] bg-[#fbfaf8] p-6`.
- Table: `divide-y divide-[#e4e0da]`, header `text-[11px] uppercase tracking-[0.12em] text-[#8a8a90]`,
  feature cell `text-[15px] text-[#0d0d0e]`.
- Yes marker: `h-6 w-6 rounded-full bg-[#0d0d0e] text-white` with `Check h-3.5 w-3.5`.
  No marker: `h-6 w-6 rounded-full border border-[#e4e0da] bg-white text-[#c4bfb7]` with `X`.
  No green/emerald anywhere on the light surface.
- Keep all seven capability rows verbatim.

## 10. CTA / downloads

- Section on `#fbfaf8`, `border-t border-[#e4e0da]`, id `downloads` preserved.
- Three platform cards: `rounded-2xl border border-[#e4e0da] bg-white p-6`.
  Platform icon tile as in §8. Asset rows: recommended =
  `bg-[#0d0d0e] text-white border-transparent hover:bg-[#26262a]`; others =
  `bg-white text-[#0d0d0e] border-[#e4e0da] hover:bg-[#f6f4f1]`. Replace `text-amber-600`
  sparkles with no icon or `text-white/70` on dark rows.
- Requirement footnotes: `text-[11px] text-[#8a8a90] border-t border-[#e4e0da]`.
- Quickstart panel (`#quickstart`): `rounded-3xl border border-[#e4e0da] bg-white p-8 sm:p-10`.
- Keep every URL, asset id, file type, and requirement string exactly as-is.

## 11. Footer

- `border-t border-[#e4e0da] bg-[#f6f4f1] py-12 text-[13px] text-[#8a8a90]`.
- Links `hover:text-[#0d0d0e]`. Keep existing link targets and the copyright line.

## 12. Shared primitives

- `ui/Button.tsx`: `default` → primary (§4), `secondary` → white/bordered, `outline` →
  bordered transparent, `ghost` → §4 ghost, `link` → underline on `#0d0d0e`. Sizes keep their
  current geometry but `default`/`lg` become `rounded-full`.
- `ui/Card.tsx`: `rounded-2xl border border-[#e4e0da] bg-white text-[#0d0d0e]` + card shadow,
  hover `border-[#d5d0c8]`. Drop `backdrop-blur`.
- `ui/Badge.tsx`: `default` → `bg-[#0d0d0e] text-white`; `secondary` →
  `border-[#e4e0da] bg-white text-[#56565a]`; `outline` → `border-[#e4e0da] text-[#56565a]`.
- `ui/CopySnippet.tsx`: dark inline terminal on the light page —
  `rounded-xl border border-[#1c1c1f] bg-[#0e0e10] text-[#e7e5e2]`, prefix `text-[#8a8a90]`,
  copy button `text-[#8a8a90] hover:bg-white/10 hover:text-white`.
- `DownloadMenu.tsx`: hero/navbar triggers use §4 primary (dark fill, white text,
  `rounded-full` for hero, `rounded-lg` for navbar). Dropdown panel becomes
  `rounded-2xl border border-[#e4e0da] bg-white shadow-[0_20px_50px_rgba(13,13,14,0.14)]`;
  rows `hover:bg-[#f6f4f1]`; "Detected" chip `border-[#e4e0da] bg-[#f6f4f1] text-[#56565a]`;
  file-type chips `border-[#e4e0da] bg-[#f6f4f1] text-[#56565a]`.

## 13. Global CSS (`site/src/index.css`)

- `body`: `background-color: #f6f4f1; color: #0d0d0e;` keep Inter + `letter-spacing: -0.015em`.
- `::selection`: `background-color: #0d0d0e; color: #ffffff`.
- Scrollbar: track `#f6f4f1`, thumb `#d5d0c8`, hover `#b9b3a9`.
- Replace `.bg-grid-pattern` rgba with `rgba(13, 13, 14, 0.04)` (still used? if unused after
  the hero rewrite, leave the rule in place; do not delete tokens other files may reference).
- Do not modify the desktop token block under `:root` (`--sidebar*`, `--terminal`, etc.).

## 14. Hard constraints

- No copy invention: reuse existing product sentences, feature titles/descriptions, platform
  requirement strings, and download URLs. Only the hero headline may be re-cased to sentence
  case as specified in §6.
- No new dependencies. Tailwind arbitrary values only.
- Do not touch `site/src/pages/index.astro`'s `<head>` metadata block (OG/Twitter/canonical)
  or `site/astro.config.mjs`.
- Do not touch anything under `ui/src/` or `src-tauri/`.
- Keep all element ids (`features`, `architecture`, `downloads`, `quickstart`) and
  `data-*`/`data-testid` attributes intact.
- Accessibility: keep focus-visible rings (`focus-visible:ring-[#0d0d0e]/30`), preserve
  `aria-label`/`aria-expanded` on the download triggers, and keep text contrast at or above
  4.5:1 against its surface.
