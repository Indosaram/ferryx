# Ferryx Design System

## 0. Research Log

- Embedded references: shortlisted `minimalist-skill`, `linear.app`, and existing Ferryx landing tokens; selected **premium utilitarian minimalism + Linear-grade precision** because the diagnostic must read as an editorial research instrument, not a playful personality test. Research lane `design-embedded` returned a decision-ready primitive and route brief.
- Lazyweb real-product screens: six surfaces were analyzed across questionnaire, diagnostic triage, comparison matrix, parameter modeler, and recommendation patterns. The useful grammar is a narrow step flow, data-dense mono metadata, live evidence context, and direct before/after differences; decorative scan animations and ROI theatrics were rejected.
- StyleGallery spatial pattern: selected [`supporting-pane`](https://github.com/changeroa/StyleGallery/blob/38ecef0e5fa9eb83e865e71f969e0c004992a8f9/patterns/split-sidebar/supporting-pane.md). The document owns vertical scrolling; the evidence rail is sticky on desktop and becomes an in-flow disclosure on mobile. Only matrix tables own local horizontal scrolling.
- Imagen concept drafts: attempted once through the available image generator; connection failed before any artifact was produced. Per the generation workflow, no blind retry or synthetic reference is treated as evidence. The code, shipped-product, and spatial-pattern lanes define the contract.
- Existing visual truth: `site/src/index.css`, `site/tailwind.config.js`, `site/src/components/ui/`, and `site/src/components/Navbar.tsx` establish the warm paper/obsidian palette, Inter/JetBrains Mono type pairing, hairline borders, floating nav, and restrained elevation.

## 1. Direction

**Warm Precision Minimalism.** The diagnostic is a technical editorial index with interactive questions. It should feel closer to a carefully annotated systems paper than a marketing quiz.

- Warm paper in light mode, studio obsidian in dark mode.
- Dense evidence, sparse decoration.
- Flat bordered panels, not glossy cards or blurred gradient blobs.
- Tool strengths and weaknesses receive equal visual weight.
- One signature interaction: the question choice updates a live evidence rail. Everything else moves only to explain a state change.

## 2. Tokens

Reuse existing variables from `site/src/index.css`; no component may introduce a competing palette.

| Role | Token | Light | Dark |
|---|---|---|---|
| Canvas | `--page-rgb` | `#F6F4F1` | `#0B0B0D` |
| Raised canvas | `--page-raised-rgb` | `#FBFAF8` | `#111113` |
| Surface | `--surface-rgb` | `#FFFFFF` | `#151518` |
| Primary ink | `--ink-rgb` | `#0D0D0E` | `#F4F3F1` |
| Secondary ink | `--ink-soft-rgb` | `#56565A` | `#A8A8AE` |
| Metadata ink | `--ink-faint-rgb` | `#8A8A90` | `#75757C` |
| Border | `--line-rgb` | `#E4E0DA` | `#26262B` |
| Strong border | `--line-strong-rgb` | `#D5D0C8` | `#35353C` |
| Evidence/working | `--status-working` | `#60A5FA` | same |
| Warning | `--status-warning` | `#FBBF24` | same |
| Verified | `--status-success` | `#86EFAC` | same |

Spacing follows a 4px base: `4, 8, 12, 16, 24, 32, 48, 64, 96`. Page gutters are 20px mobile, 32px tablet, 48px desktop. Borders are 1px. Primary panel radius is 16px; choice radius 12px; code/evidence radius 8px; pills use full radius.

Elevation is reserved for floating navigation and active focus. Default cards use `shadow-card`; no decorative shadows.

## 3. Typography

- Display/body: `Inter`, `Pretendard`, `-apple-system`, `BlinkMacSystemFont`, sans-serif.
- Technical metadata: `JetBrains Mono`, monospace.
- Display headings use `clamp(2rem, 5vw, 4.5rem)`, tight `-0.035em` tracking, and balanced Korean line wrapping.
- Question titles use 28–36px desktop, 24–30px mobile, weight 600.
- Body text is 15–17px with 1.65 line height. Metadata is 11–13px mono.
- Never use uppercase tracking on Korean. English eyebrow labels may use uppercase mono at 0.08em.

## 4. Primitives

### DiagnosticShell

Desktop uses the selected `supporting-pane` contract: a dominant `minmax(0, 1fr)` task region and a 20–22rem evidence rail. Progress lives above the task rather than consuming a second rail. The document owns vertical scrolling; the evidence rail may be sticky but never introduces nested scrolling.

Mobile collapses to one column: compact sticky progress header, question, choices, expandable evidence summary, action row.

### StepProgress

Mono `03 / 07`, semantic `<progress>`, and a line fill. Announce progress to assistive technology. Do not render seven decorative dots without a text counter.

### DiagnosticOption

A real radio control wrapped by a full-width card. Anatomy: numeric keyboard hint, title, one-line concrete trade-off, selection ring. States: idle, hover, focus-visible, selected, disabled. Selected state changes border, background, icon, and accessible checked state — never color alone.

### EvidenceRail

Shows the scoring axes affected by the current choice, named tool examples, and source links. It updates after a choice with a short fade; no animated counters.

### ResultMatch

Tool name, score percentage, maturity/substrate tags, why it matched, evidence rows, weakness block, official link. The first and runner-up use the same component; rank is conveyed by heading and order, not inflated styling.

### DisclosurePanel

Required whenever Ferryx appears. Must state that Ferryx is experimental/new, the diagnostic author develops it, the weighting is public, and alternatives may fit better. Use a warning border and plain language; no promotional CTA inside this panel.

### MatrixTable

Desktop: sticky first column with horizontal overflow owned by the table wrapper. Mobile: tool-by-tool disclosure rows with axes inside `<details>`. Every scored cell exposes a source link or `UNKNOWN`.

### MethodologyPanel

Displays formula, weight table, complete distribution output, tie count, limits, and public data links. Code/data blocks use existing dark code tokens.

## 5. Route Composition

- `/diagnostic/`: editorial introduction, seven-step diagnostic, methodology and matrix links.
- `/diagnostic/r/[type]/`: result identity, co-winners/top three, choice contribution, weakness, runner-up delta, citations.
- `/diagnostic/matrix/`: complete comparison data with evidence and explicit unknowns.
- `/diagnostic/methodology/`: scoring contract and generated fairness report.
- `/diagnostic/compare/[a]-vs-[b]/`: neutral two-column comparison with source-backed differences.
- `/ko/diagnostic/...`: Korean mirror generated from shared data, not duplicated facts.

## 6. Interaction and Motion

- Numeric keys `1–3` select an answer; `Enter` advances; `ArrowLeft` or Back returns.
- Selection and step transitions use 150–180ms opacity/transform only, with `cubic-bezier(0.16, 1, 0.3, 1)`.
- The evidence rail may crossfade when its content changes. No loading theatre for synchronous local scoring.
- `prefers-reduced-motion: reduce` removes translations and collapses transition duration.
- Focus rings remain visible in both themes with at least 3:1 non-text contrast.

## 7. Responsive and Scroll Contract

- 375px: one column, 52px minimum choice targets, sticky compact progress, no clipped Korean nouns or orphaned particles.
- 768px: question body remains primary; evidence becomes an inline panel after choices.
- 1280px+: three-region grid; the page owns vertical scroll, evidence/progress rails are sticky below the floating navbar.
- Long repositories, URLs, SHAs, and unbroken tool names wrap with `overflow-wrap:anywhere` only inside metadata/evidence; body prose uses natural line breaking.

## 8. Accessibility Constraints

- WCAG 2.2 AA minimum; Lighthouse accessibility target is 100 on production preview.
- Radio groups use `<fieldset>` and `<legend>`; validation focus moves to the unanswered group.
- Dynamic question/result changes announce through a restrained `aria-live="polite"` region.
- All icon buttons have accessible names. No emoji icons; use Lucide or text labels.
- Tables retain headers and captions; mobile transformations preserve the same accessible facts.
- External links disclose that they open a new tab when they do.

## 9. Content and Tone

- Ask only factual operating questions. No MBTI metaphors, soulmates, archetype astrology, or fake psychology.
- State uncertainty and age explicitly. `UNKNOWN` is a valid rendered value.
- Use “이 도구를 쓰지 말아야 할 때” alongside every recommendation.
- Ferryx copy never hides ownership, maturity, or missing comparative benchmarks.
- tmux is described as a durable baseline and companion, never legacy.

## 10. Absolute Bans

1. No gradients, mesh blobs, glass cards, floating decorative particles, or fake terminal screenshots.
2. No one-sided green-check comparison tables.
3. No motion on non-interactive decoration and no hover state without an affordance.
4. No email capture, login gate, cookie wall, delayed result animation, or personal analytics payload.
5. No score or claim without a source or explicit `UNKNOWN`.
6. No Ferryx result without the disclosure and alternatives block.

## 11. Accepted Debt

- No exact visual reference image exists because the concept generation call failed before producing an artifact. Final quality is judged against this token/primitive contract and real-browser visual QA, not pixel fidelity to a mock.
- Dynamic per-user OG images are impossible on GitHub Pages. Static result-type OG images are the contract.
- Live GitHub star counts can drift; every displayed count must include `accessed_at` and is not presented as real-time.

## 12. Handoff and QA

- Primitive showcase must be rendered before route-specific composition is considered complete: choice states, disclosure, result card, evidence row, progress, and matrix cell.
- Capture every route at 375, 768, and 1280 where the layout differs.
- Drive selection, focus, back/next, validation, result navigation, theme switch, and external evidence links in the real browser.
- Final visual QA must inspect Korean wrapping, source metadata overflow, dark/light contrast, sticky rail behavior, and reduced motion.
