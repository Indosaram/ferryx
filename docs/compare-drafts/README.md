# Draft Comparison Pages (Unpublished)

This directory contains draft comparison pages evaluating Ferryx against four agent-orchestration and terminal tools:

1. `herdr.md` — Ferryx vs Herdr
2. `cmux.md` — Ferryx vs cmux
3. `orca.md` — Ferryx vs Orca
4. `vibe-kanban.md` — Ferryx vs Vibe Kanban

## Status and Publishing Warning

These files are **drafts only** awaiting maintainer review.
**Do NOT touch or move anything into `site/src/content/docs/compare/`** until explicitly approved. Moving any of these draft files into `site/src/content/docs/compare/` immediately publishes them on the public site.

## Permissible Sources and Editorial Rules

The only factual basis permitted for competitor claims in these drafts is:
- `docs/FERRYX_COMPETITIVE_USER_SENTIMENT_TOP10_2026-09-12.md` (dated 2026-09-12).

Every factual statement about a competitor must follow these strict rules:
1. **Verifiable citation**: Every competitor claim traces to its exact section and citation code (e.g. `§7 Herdr, source [S31]`) in a trailing `## Sources` section.
2. **Zero invention**: No invented numbers, star counts, dates, licenses, prices, benchmarks, or feature lists. Only literal numbers and facts from the source document are cited.
3. **Honest advantage section**: Each page includes an explicit `## What <tool> does better` section containing solely features and strengths documented in the source document.
4. **Draft frontmatter**: Every draft includes `draft: true` and a `verified:` audit line referencing the 2026-09-12 research document cross-checked on 2026-09-25.
5. **Ferryx repo facts**: Ferryx claims are constrained strictly to documented repository architecture (`.orca-worktrees/wt-<slug>`, `orca/<workspace-id>/<slug>`, headless Rust PTY daemon, 512 KiB sequenced ring buffer with replay, WGPU/libghostty-vt rendering, SUL-1.0 license, and Tauri v2 + Rust desktop stack) as stated in existing published documentation (`site/src/content/docs/compare/crystal.md`).
6. **No unmeasured performance claims**: Strict adherence to site house style.
7. **Line limit**: Kept under 120 lines per document.
