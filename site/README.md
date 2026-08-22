# Ferryx Web & Documentation Architecture

This repository hosts the unified Ferryx public web presence and documentation engine under `site/`.

## Architecture Overview

- **Host & Framework**: [Astro v5](https://astro.build/)
- **Documentation Engine**: [@astrojs/starlight](https://starlight.astro.build/)
- **Interactive Components**: [@astrojs/react](https://docs.astro.build/en/guides/integrations-guide/react/) (React 18)
- **Styling**: Tailwind CSS with desktop token bridging (`../ui/src`)
- **Port**: `14173` for the local dev server (`bun run dev`)

## Deployment

GitHub Pages injects the production base path in Actions. Local development keeps the site at `http://localhost:14173/`.

## Route Structure

- `/` : Custom Ferryx Landing Page
  - Includes interactive `<LiveFerryxDemo client:only="react" />` mounting the genuine desktop `Sidebar`, `TabBar`, and `TerminalSplitView` against a client-side mocked Tauri IPC layer.
  - Truthful product feature sections (Features, Architecture comparison matrix, CTA, Footer).
  - Zero fabricated benchmarks or unverifiable numeric claims.
- `/docs/introduction/` : Starlight Documentation Intro
- `/docs/shortcuts/` : In-app verified keyboard shortcuts matching `ui/src/lib/shortcuts.ts`.

## Content Authoring

Documentation pages live in `site/src/content/docs/docs/*.md`.
Adding new Markdown/MDX files automatically integrates them into:
1. Starlight left sidebar navigation
2. Full-text search (Pagefind)
3. Table of Contents (TOC)
4. Expressive Code syntax highlighting
