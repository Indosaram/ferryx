# ferryx-landing-and-readme - Work Plan

## TL;DR (For humans)
**What you'll get:** A standalone, high-performance landing page for Ferryx built in `site/` with Bun + React + Tailwind + shadcn-style UI components, synchronized Ferryx dark monochrome branding, GitHub Pages deployment workflow, and a developer-first repository `README.md`.

**Why this approach:** Keeping the landing page in a dedicated `site/` subproject isolates web marketing assets and GitHub Pages builds from the Tauri desktop app runtime (`ui/` and `src-tauri/`), preventing bundle bloat and dependency conflicts while maintaining 100% aesthetic and brand parity.

**What it will NOT do:** It will not modify core desktop app code in `src-tauri/` or `ui/`, will not introduce external hosted database dependencies, and will strictly avoid non-canonical or colored branding assets.

**Effort:** Short
**Risk:** Low - Isolated web subproject and root README addition with zero blast radius on desktop runtime.
**Decisions to sanity-check:** Relative base path (`base: './'`) in Vite for universal GitHub Pages subpath compatibility; automated GitHub Actions workflow for continuous deployment.

Your next move: High-accuracy Momus review execution and plan authorization. Full execution detail follows below.

---

> TL;DR (machine): Short effort, Low risk, delivers `site/` landing app + `.github/workflows/deploy-pages.yml` + `README.md`.

## Scope
### Must have
- Standalone landing page application in `site/` powered by Bun, Vite, React, TypeScript, Tailwind CSS, Lucide React, and shadcn-style components.
- Canonical Ferryx dark monochrome squircle branding (`ui/src/assets/ferryx-icon.svg`, transparent outer margin, zero colored alterations).
- High-fidelity interactive UI sections:
  1. Top Navigation Bar (Logo, Nav links, GitHub Stars / Fork / Release CTA).
  2. Hero Section (Tagline, quick installation/run snippet with copy button, interactive terminal + browser split-view mockup).
  3. Feature Showcase Grid (Rust native daemon, zero Electron overhead, multi-agent workspace, tab persistence, embedded browser).
  4. Performance & Memory Comparison Matrix (Ferryx vs Electron-based AI IDEs).
  5. Interactive Keyboard Shortcuts Cheat Sheet.
  6. Call to Action (Download macOS DMG/app, clone repo) & Footer.
- GitHub Pages automated build and deployment pipeline (`.github/workflows/deploy-pages.yml`).
- Beautiful, comprehensive repository `README.md` at root matching the landing page aesthetic.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No edits or breaking changes to `ui/` or `src-tauri/` desktop application code.
- No colorful, AI-slop, or non-canonical logos (strictly obey `BRANDING_AND_ICON_SPECIFICATION.md`).
- No heavy server-side rendering or backend database requirements (pure static SPA for GitHub Pages).

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after (Vite production build + DOM unit test where applicable)
- Evidence: `.omo/evidence/task-<N>-ferryx-landing-and-readme.txt`

## Execution strategy
### Parallel execution waves
- **Wave 1**: 1 (Scaffold `site/` subproject & dependencies)
- **Wave 2**: 2, 3, 4 (UI Primitives, Hero Section, Feature Grid & Benchmarks)
- **Wave 3**: 5, 6, 7 (Shortcuts & CTA Footer, GitHub Actions Workflow, Root README)
- **Wave 4**: 8 (Full build & cross-project verification)

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | None | 2, 3, 4, 5, 6, 7, 8 | None |
| 2 | 1 | 3, 4, 5 | None |
| 3 | 1, 2 | 8 | 4, 6, 7 |
| 4 | 1, 2 | 8 | 3, 6, 7 |
| 5 | 1, 2 | 8 | 6, 7 |
| 6 | 1 | 8 | 3, 4, 5, 7 |
| 7 | 1 | 8 | 3, 4, 5, 6 |
| 8 | 1, 2, 3, 4, 5, 6, 7 | F1..F4 | None |

## Todos
> Implementation + Test = ONE todo. Never separate.

- [x] 1. Initialize `site/` subproject with Bun, Vite, React, TypeScript, and Tailwind CSS
  Recommended task executor category: quick
  What to do / Must NOT do:
  - Scaffold `site/package.json` with scripts (`dev`, `build`, `preview`), dependencies (`react`, `react-dom`, `lucide-react`, `clsx`, `tailwind-merge`), and devDependencies (`vite`, `@vitejs/plugin-react`, `tailwindcss`, `postcss`, `autoprefixer`, `typescript`, `@types/react`, `@types/react-dom`, `@types/node`).
  - Configure `site/tsconfig.json`, `site/vite.config.ts` (with `base: './'`), `site/tailwind.config.js`, and `site/postcss.config.js`.
  - Copy and sync canonical Ferryx icon assets (`ui/src/assets/ferryx-icon.svg`, favicons) into `site/public/` and `site/src/assets/`.
  - Create `site/index.html` with accurate metadata, OpenGraph tags, and dark theme defaults.
  - Run `bun install` inside `site/`.
  Parallelization: Wave 1 | Blocked by: None | Blocks: 2, 3, 4, 5, 6, 7, 8
  References: `BRANDING_AND_ICON_SPECIFICATION.md:1-35`, `ui/package.json:1-35`
  Acceptance criteria: `cd site && bun run build` creates valid `site/dist/` assets with exit code 0.
  QA scenarios:
    - Happy: `bun run --cwd site build` completes with exit code 0 and generated index.html.
    - Failure: Missing dependency or syntax error in vite.config.ts causes build failure.
    - Evidence: `.omo/evidence/task-1-ferryx-landing-and-readme.txt`
  Commit: Y | feat(site): scaffold standalone landing page subproject with bun and tailwind

- [x] 2. Build shadcn-style UI primitives and design system for landing page
  Recommended task executor category: visual-engineering
  What to do / Must NOT do:
  - Create utility helper `site/src/lib/utils.ts` (`cn` helper using `clsx` + `tailwind-merge`).
  - Create core UI components in `site/src/components/ui/`: `Button`, `Badge`, `Card`, `Tabs`, `Tooltip`, and `Container`.
  - Set up dark modern monochrome styling, glowing border gradients, glassmorphic card effects, and subtle grid background patterns in `site/src/index.css`.
  - Must NOT introduce colorful themes or arbitrary third-party bloated libraries.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 3, 4, 5
  References: `BRANDING_AND_ICON_SPECIFICATION.md:1-25`, `ui/src/index.css`
  Acceptance criteria: UI components export cleanly and compile in TypeScript without errors.
  QA scenarios:
    - Happy: `bun run --cwd site build` compiles all UI primitives with 0 type errors.
    - Failure: Invalid CSS variable or unexported component causes build failure.
    - Evidence: `.omo/evidence/task-2-ferryx-landing-and-readme.txt`
  Commit: Y | feat(site): add shadcn-style UI primitives and dark workspace theme

- [x] 3. Implement Navigation Bar and interactive Hero Section with split-view mockup
  Recommended task executor category: visual-engineering
  What to do / Must NOT do:
  - Build `site/src/components/Navbar.tsx`: Ferryx logo badge, Navigation links (Features, Architecture, Benchmarks, Shortcuts), GitHub Star link, Download CTA button.
  - Build `site/src/components/Hero.tsx`:
    - Bold badge: "Ultra-lightweight Rust Native AI Workspace & Launcher".
    - High-impact headline: "Parallel Agentic Development. Zero Bloat."
    - Quick start install snippet with one-click copy button (`git clone ... && bun install && cargo tauri dev`).
    - Interactive Split-view Mockup: Realistic terminal tab + embedded browser tab side-by-side preview with simulated agent output, status indicators, and tabs.
  Parallelization: Wave 2 | Blocked by: 1, 2 | Blocks: 8
  References: `BRANDING_AND_ICON_SPECIFICATION.md`, `ui/src/components/TabBar.tsx`, `ui/src/components/TerminalSplitView.tsx`
  Acceptance criteria: Hero section renders responsively across desktop, tablet, and mobile breakpoints without layout overflow.
  QA scenarios:
    - Happy: `bun run --cwd site build` compiles Hero and Navbar with 0 errors.
    - Failure: Broken asset path or unclosed JSX tag triggers build failure.
    - Evidence: `.omo/evidence/task-3-ferryx-landing-and-readme.txt`
  Commit: Y | feat(site): implement navbar and interactive hero section with split-view demo

- [x] 4. Implement Feature Grid and Performance Comparison Matrix
  Recommended task executor category: visual-engineering
  What to do / Must NOT do:
  - Build `site/src/components/Features.tsx`:
    - Native Rust PTY Daemon (instant terminal sessions, persistent background tasks).
    - True Multi-Agent Workspace (run parallel AI agent coding sessions in dedicated tabs/worktrees).
    - Embedded Browser Companion (side-by-side web preview, zero app switching).
    - Zero Electron Overhead (lightweight Tauri v2 + Webview2/WebKit architecture).
    - Resilient Session Persistence (daemon keeps shells and workspaces alive across restarts).
  - Build `site/src/components/Benchmarks.tsx`:
    - Interactive comparison table / visual bars: Memory Usage (Ferryx ~80MB vs Electron IDEs ~800MB-1.5GB), Startup Time (<150ms vs >3.5s), Binary Size.
  Parallelization: Wave 2 | Blocked by: 1, 2 | Blocks: 8
  References: `BRANDING_AND_ICON_SPECIFICATION.md`, `FERRYX_PARITY_FULL_SWEEP.md`
  Acceptance criteria: Feature cards and benchmark charts render cleanly with clear typography and dark aesthetic.
  QA scenarios:
    - Happy: `bun run --cwd site build` compiles Features and Benchmarks successfully.
    - Failure: Missing metric definition or type mismatch causes build error.
    - Evidence: `.omo/evidence/task-4-ferryx-landing-and-readme.txt`
  Commit: Y | feat(site): implement feature grid and performance comparison section

- [x] 5. Implement Keyboard Shortcuts Cheat Sheet, CTA, and Footer
  Recommended task executor category: visual-engineering
  What to do / Must NOT do:
  - Build `site/src/components/Shortcuts.tsx`: Interactive keyboard shortcut cards (e.g. `⌘T` New Terminal, `⌘⇧B` New Browser Tab, `⌘W` Close Pane, `⌘1-9` Switch Tabs, `⌘\` Split Horizontal/Vertical).
  - Build `site/src/components/CTA.tsx`: Final call-to-action banner for downloading releases, viewing documentation, and contributing on GitHub.
  - Build `site/src/components/Footer.tsx`: Copyright, GitHub link, MIT/Apache license notice, Ferryx brand mark.
  - Assemble full page in `site/src/App.tsx`.
  Parallelization: Wave 3 | Blocked by: 1, 2 | Blocks: 8
  References: `BRANDING_AND_ICON_SPECIFICATION.md:27-33`, `ui/src/App.tsx`
  Acceptance criteria: Full landing page renders end-to-end with smooth scrolling navigation and responsive footer.
  QA scenarios:
    - Happy: `bun run --cwd site build` completes with 0 errors and valid bundle output.
    - Failure: Broken link handler or missing shortcut entry causes build error.
    - Evidence: `.omo/evidence/task-5-ferryx-landing-and-readme.txt`
  Commit: Y | feat(site): add keyboard shortcuts guide, cta banner, and footer

- [x] 6. Configure GitHub Pages Deployment Workflow
  Recommended task executor category: quick
  What to do / Must NOT do:
  - Create `.github/workflows/deploy-pages.yml`:
    - Trigger on `push` to `main` (paths in `site/**` or manual `workflow_dispatch`).
    - Setup Bun environment (`oven-sh/setup-bun@v2`).
    - Install dependencies and run `bun run build` inside `site/`.
    - Upload artifact to `actions/upload-pages-artifact@v3` pointing to `site/dist/`.
    - Deploy to GitHub Pages with `actions/deploy-pages@v4` and standard permissions (`contents: read`, `pages: write`, `id-token: write`).
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 8
  References: GitHub Pages official action specs
  Acceptance criteria: `.github/workflows/deploy-pages.yml` is valid YAML and follows GitHub Pages deployment standards.
  QA scenarios:
    - Happy: YAML parses cleanly with no syntax errors.
    - Failure: Invalid indentation or missing required action step fails validation.
    - Evidence: `.omo/evidence/task-6-ferryx-landing-and-readme.txt`
  Commit: Y | ci: add github actions workflow for automated github pages deployment

- [x] 7. Craft developer-first repository `README.md`
  Recommended task executor category: writing
  What to do / Must NOT do:
  - Create root `README.md` structured with:
    - Ferryx Dark Squircle logo header and badges (Build status, License, Bun, Tauri, Rust, Discord/GitHub).
    - Clear elevator pitch and value proposition.
    - Key Features overview with aesthetic ASCII/Markdown layout.
    - Architecture overview (Rust Native Daemon + Tauri v2 + React Frontend).
    - Quick Start guide (Prerequisites: Rust, Bun, Tauri CLI; Step-by-step dev setup: `bun install`, `bun tauri dev`).
    - Landing Page link and GitHub Pages deployment instructions.
    - Keyboard Shortcuts cheat sheet table.
    - Performance & Benchmark summary.
    - Contributing & License guidelines.
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 8
  References: `BRANDING_AND_ICON_SPECIFICATION.md`, `package.json`, `src-tauri/tauri.conf.json`
  Acceptance criteria: `README.md` exists, has accurate links/code blocks, and passes markdown linting with 0 broken internal references.
  QA scenarios:
    - Happy: `README.md` renders cleanly with valid syntax and comprehensive instructions.
    - Failure: Outdated legacy naming or broken markdown links present.
    - Evidence: `.omo/evidence/task-7-ferryx-landing-and-readme.txt`
  Commit: Y | docs: create modern ferryx readme with architecture and quickstart

- [x] 8. Execute full static build and cross-repo integrity checks
  Recommended task executor category: quick
  What to do / Must NOT do:
  - Run `bun run --cwd site build` and verify `site/dist/index.html` and bundled CSS/JS are present and optimized.
  - Run `bun run --cwd ui test` to ensure desktop app tests remain 100% passing.
  - Verify that `BRANDING_AND_ICON_SPECIFICATION.md` rules remain fully intact.
  Parallelization: Wave 4 | Blocked by: 1, 2, 3, 4, 5, 6, 7 | Blocks: F1, F2, F3, F4
  References: `site/package.json`, `ui/package.json`
  Acceptance criteria: Both `site/` build and `ui/` test suite pass cleanly with exit code 0.
  QA scenarios:
    - Happy: `bun run --cwd site build` && `bun run --cwd ui test` pass with 0 errors.
    - Failure: Regression in desktop UI or failure in site build fails step.
    - Evidence: `.omo/evidence/task-8-ferryx-landing-and-readme.txt`
  Commit: Y | chore: verify site build and repository integrity

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [x] F1. Plan compliance audit (Verify all 8 implementation items completed against specifications)
- [x] F2. Code quality review (TypeScript 0 errors, clean component boundaries, zero slop/bloat)
- [x] F3. Real manual QA (Verify site build output index.html and README.md formatting)
- [x] F4. Scope fidelity (Verify zero regressions on desktop app `ui/` and `src-tauri/`)

## Commit strategy
- Atomic conventional commits per completed task (`feat(site): ...`, `ci: ...`, `docs: ...`, `chore: ...`).

## Success criteria
- `site/` is a functional standalone React + Tailwind + Bun landing page with high-fidelity dark monochrome UI.
- `.github/workflows/deploy-pages.yml` delivers automated GitHub Pages deployment.
- `README.md` provides complete, developer-ready documentation for Ferryx.
- Core desktop app (`ui/` and `src-tauri/`) remains 100% stable and fully passing tests.
