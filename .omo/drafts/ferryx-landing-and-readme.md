---
slug: ferryx-landing-and-readme
status: ready
intent: clear
review_required: true
plan_path: .omo/plans/ferryx-landing-and-readme.md
plan_sha256: null
review_round_id: 1
review_round_limit: 5
pending-action: handoff to worker (/start-work)
review:
  momus:
    status: approved
    workspace_root: null
    runtime_home: null
    target: .omo/plans/ferryx-landing-and-readme.md
    round_id: 1
    plan_sha256: null
    launch_id: st_01a02700
    session: st_01a02700
    result: OKAY
approach: Create a standalone high-performance landing page in site/ with Bun + Vite + React + Tailwind + shadcn-style UI components, synchronized Ferryx dark squircle branding, GitHub Pages deployment workflow, and an aesthetic matching repository README.md.
---

# Draft: ferryx-landing-and-readme

## Components (topology ledger)
| component | outcome | status | evidence path |
|---|---|---|---|
| `site-scaffold` | Initialize `site/` with Bun + Vite + React + TypeScript + Tailwind CSS | active | `site/package.json`, `site/vite.config.ts` |
| `site-ui-components` | Build premium dark-mode landing UI (Hero, Interactive Demo Mockup, Feature Grid, Architecture comparison, Performance metrics, Download/CTA, Footer) with shadcn primitives | active | `site/src/components/*` |
| `site-branding-sync` | Embed canonical Ferryx dark squircle master logo and styling | active | `ui/src/assets/ferryx-icon.svg`, `BRANDING_AND_ICON_SPECIFICATION.md` |
| `gh-pages-workflow` | GitHub Actions automated build & Pages deployment pipeline | active | `.github/workflows/deploy-pages.yml` |
| `repo-readme` | Modern developer-first `README.md` with badges, hero preview, features, keybindings, architecture, and quickstart | active | `README.md` |

## Open assumptions (announced defaults)
| assumption | adopted default | rationale | reversible? |
|---|---|---|---|
| Deployment Base Path | Relative/flexible (`base: './'`) | Ensures GitHub Pages deployment works under any repo repository subpath seamlessly | Yes |
| Design Theme | Dark monochromatic terminal/IDE theme matching Ferryx app | Consistency with `BRANDING_AND_ICON_SPECIFICATION.md` | Yes |
| GitHub Actions | Automated deploy on push to `main` | Standard continuous delivery for GitHub Pages | Yes |

## Findings (cited - path:lines)
- `BRANDING_AND_ICON_SPECIFICATION.md:1-35`: App name is strictly **Ferryx** (`ferryx`, `com.ferryx.app`), dark monochrome squircle master icon with transparent margin (`ui/src/assets/ferryx-icon.svg`).
- `src-tauri/tauri.conf.json:1-42`: Desktop app metadata and windows config for Ferryx.
- `ui/package.json:1-35`: React 18 + Vite + Tailwind 3 + Lucide Icons stack in main UI.

## Decisions (with rationale)
- **Directory**: `site/` (Option A selected by user) for clear separation of concerns from Tauri desktop UI.
- **Stack**: `bun` + `vite` + `react` + `tailwindcss` + `lucide-react` + Radix/shadcn UI patterns.
- **README**: Comprehensive, high visual fidelity matching the landing page aesthetic (Hero, features, arch, quick start, shortcuts).

## Scope IN
- Standalone landing page package in `site/` with zero dependency on Tauri runtime.
- High-conversion responsive landing page sections (Header, Hero with interactive split-view mockup, Feature Grid, Benchmarks, Shortcuts, CTA, Footer).
- GitHub Pages deployment configuration (Vite base config + `.github/workflows/deploy-pages.yml`).
- Root repository `README.md` fully crafted with identical branding, features, and docs.

## Scope OUT (Must NOT have)
- Modifying `src-tauri/` or breaking desktop app `ui/` builds.
- Adding unneeded heavy runtime backend servers or paid hosting configs.
- Reverting Ferryx branding or introducing colored/AI-slop logos.

## Approval gate
status: ready
review_verdict: approved
