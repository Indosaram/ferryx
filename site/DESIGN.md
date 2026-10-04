# Ferryx landing - DESIGN.md

Scope: `site/` landing page (`src/pages/index.astro`) and its section components. Docs pages are styled by Starlight and are out of scope.
Branch: existing UI with implicit tokens. This file codifies what already ships (user decision 2026-10-04: keep current look, restructure content only).

## 1. Atmosphere
Warm-neutral paper page, near-black ink, one material: quiet raised surfaces with hairline borders. The only saturated color on the page lives inside product UI (the live demo and terminal-chrome visuals), so the product is the color. No accent color in landing chrome.

## 2. Color tokens (src/index.css, light + .dark)
| Token | Tailwind | Use |
|---|---|---|
| --page-rgb | bg-page | page background |
| --page-raised-rgb | bg-page-raised | alternating section band |
| --surface-rgb | bg-surface | cards, panels |
| --ink-rgb / --ink-hover-rgb | text-ink, bg-ink | headings, primary button fill |
| --ink-soft-rgb | text-ink-soft | body copy |
| --ink-faint-rgb | text-ink-faint | eyebrows, captions, meta |
| --line-rgb / --line-strong-rgb | border-line(-strong) | hairlines, hover border |
| --code-bg/border/ink-rgb | bg-code-bg ... | code snippets |
| status-working / warning / success / idle | text-status-* | ONLY inside terminal-chrome visuals (agent state dots) |

--ink-faint is tuned to >= 4.5:1 on page, page-raised and surface in both themes (light 110 110 116, dark 128 128 134); never lighten it.
Never write raw hex in landing chrome. Terminal-chrome visuals may use `bg-[#0e0e10]` + white alphas (existing FeatureVisuals Chrome).

## 3. Typography
Inter (sans) + JetBrains Mono (mono). Body letter-spacing -0.015em.
The pre-font body fallback is local Arial sized to 107.31%, measured against the
48px medium hero sentence in Google Chrome. The loaded Inter face remains the
visual contract; fallback alignment reduces line-wrap changes during font swap.
- Display h1: `text-[clamp(2.75rem,7vw,5.25rem)] font-medium tracking-[-0.045em] leading-[0.95]`
- Section h2: `text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05]`
- Card h3: `text-[clamp(1.375rem,2.2vw,1.875rem)] font-medium tracking-[-0.025em] leading-[1.15]`; small h3 `text-[17px] font-medium tracking-[-0.015em]`
- Body: `text-[15px]`-`text-[17px] leading-relaxed text-ink-soft`, max ~65ch (`max-w-2xl`)
- Eyebrow: `text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint`
- Meta: `text-[13px] text-ink-faint`; code/paths: `font-mono text-[12px]-[13px]`
- Micro: text-[10px] only for badges/chips inside menus (navbar Latest release badge, download menu file-type tags); text-[18px] only inside terminal-chrome visuals.
Sentence case for every heading.

## 4. Spacing & layout
Container `mx-auto max-w-6xl px-4 sm:px-6` (narrow sections `max-w-5xl` / `max-w-3xl`). Section rhythm `py-24 sm:py-28 border-t border-line`, alternating `bg-page` / `bg-page-raised`. Section header block `text-center max-w-2xl mx-auto mb-16` or left-aligned for content-heavy sections (FAQ, install). Radius: cards `rounded-2xl`, large panels `rounded-3xl`, chips `rounded-full`/`rounded-md`.

## 5. Primitives (and states)
- PrimaryButton: `bg-ink text-page hover:bg-ink-hover rounded-full h-11 px-5`; focus-visible ring `ring-2 ring-ink/30`. (DownloadMenu hero variant.)
- SecondaryButton: `Button variant=outline` + `rounded-full bg-surface border-line hover:border-line-strong hover:bg-page-raised`.
- Card: `rounded-2xl border border-line bg-surface p-6 sm:p-8 shadow-card`; hover only when the card is a link: `hover:border-line-strong`.
- Chip: `rounded-full border border-line bg-surface px-3 py-1 text-[13px] text-ink-soft`.
- TerminalChrome: FeatureVisuals `Chrome` (dark window, 3 dots, mono label).
- Disclosure (FAQ): native `<details>/<summary>`, summary `text-[16px] font-medium text-ink`, chevron rotates via `group-open:rotate-180` (transform only), body `text-ink-soft`.
- TextLink: `underline underline-offset-4 hover:text-ink` in meta lines; `inline-flex font-medium text-ink hover:text-ink-soft` + an aria-hidden arrow glyph for section CTAs.
- CodeSnippet: `rounded-xl bg-code-bg border border-code-border text-code-ink font-mono text-[13px] p-4 overflow-x-auto`.
- SkipLink: sr-only until focus-visible; on focus becomes a Chip-styled link at the top-left of the section it skips (focus-visible:not-sr-only focus-visible:absolute focus-visible:z-30).
- CopyCommand: code block (whitespace-pre-wrap break-all) plus a ghost icon button (lucide Copy/Check, aria-label "Copy install command") with an aria-live polite status; React island hydrated client:visible.
States: hover = border or background shift only; never colored accent borders for selection; keyboard focus-visible ring is the only colored edge.

## 6. Motion
Only existing motion: FeatureVisuals cycles (opacity), DownloadMenu chevron rotate, FAQ chevron rotate. 150-200ms `transition-colors`/`transition-transform`. No new decorative animation. Respect `motion-reduce:` on any new transform.

## 7. Responsive
Mobile-first; verify 375 / 768 / 1280. Grids collapse to one column below `sm`/`lg`. No horizontal overflow at 320px (navbar has a geometry test). Tables become stacked cards or scroll inside `overflow-x-auto`.

## 8. Accessibility constraints & accepted debt
- One h1 (hero). Each section has an h2 and an `id` for in-page nav. Landmark `<main>`.
- Contrast: body uses ink-soft (passes AA on page/surface in both themes); ink-faint only for meta text >= 11px.
- New section copy is server-rendered Astro. CopyCommand hydrates only when visible; the SSR-safe LazyLiveDemo wrapper dynamically imports the workspace preview on visibility, reserving 560px for its loading state.
- index.astro meta theme-color uses literal hex (#f6f4f1 / #0b0b0d) because the meta tag cannot read CSS variables; keep it equal to --page-rgb in each theme.
- Inline arrows in text links use the → glyph inside <span aria-hidden="true">, never ASCII "->".
- Inter and JetBrains Mono retain the existing brand typography and asynchronous Google Fonts loading. Self-hosting was measured and rejected after mobile FCP/LCP regressed; retain the original loading contract.
- The decorative preview background uses a lazy AVIF/WebP picture with the original JPEG fallback, the same centered cover crop and clipped blur. It does not contribute accessible content.
- In the website preview, terminal Tab moves to the following guide link and Shift+Tab to the preceding skip link, instead of forwarding page navigation into the mock terminal.
