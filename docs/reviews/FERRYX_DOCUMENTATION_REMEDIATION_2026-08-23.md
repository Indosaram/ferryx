# Ferryx Documentation & Pages Remediation Review (2026-08-23)

## Summary

This review documents the verification and remediation of stale public URLs, development/validation commands, deploy workflow path triggers, and non-existent license links across documentation and static site assets.

## Findings, Remediation & Evidence

### 1. Stale Public GitHub Repository URLs and Clone Snippets

- **Scope / Paths**:
  - `site/astro.config.mjs`
  - `site/src/components/CTA.tsx`
  - `site/src/components/Hero.tsx`
  - `site/src/components/Navbar.tsx`
  - `site/src/components/Footer.tsx`
- **RED Evidence**:
  - `git grep -n 'github.com/ferryx/ferryx' -- site README.md docs .github` identified stale URLs across 5 files:
    - `site/astro.config.mjs:20: social: { github: 'https://github.com/ferryx/ferryx' }`
    - `site/src/components/CTA.tsx:29: code="git clone https://github.com/ferryx/ferryx && cd ferryx && bun dev"`
    - `site/src/components/CTA.tsx:35: href="https://github.com/ferryx/ferryx/releases"`
    - `site/src/components/CTA.tsx:41: href="https://github.com/ferryx/ferryx"`
    - `site/src/components/Footer.tsx:19: href="https://github.com/ferryx/ferryx"`
    - `site/src/components/Footer.tsx:28: href="https://github.com/ferryx/ferryx/blob/main/LICENSE"`
    - `site/src/components/Hero.tsx:22: code="git clone https://github.com/ferryx/ferryx && cd ferryx && bun dev"`
    - `site/src/components/Hero.tsx:40: href="https://github.com/ferryx/ferryx"`
    - `site/src/components/Navbar.tsx:42: href="https://github.com/ferryx/ferryx"`
- **Remediation**:
  - Replaced all stale `https://github.com/ferryx/ferryx` instances with canonical `https://github.com/Indosaram/ferryx`.
  - Replaced release targets with `https://github.com/Indosaram/ferryx/releases`.
  - Updated clone snippets in Hero and CTA components to `git clone https://github.com/Indosaram/ferryx.git && cd ferryx && cd src-tauri && cargo tauri dev`, matching the Tauri configuration's automatic UI dev-server startup while preserving visual layout and styling.
  - Removed 404 License anchor link (`https://github.com/Indosaram/ferryx/blob/main/LICENSE`) from `Footer.tsx` entirely since no repository license file exists.
- **GREEN Validation**:
  - `git grep -nE 'github.com/ferryx/ferryx|Indosaram/ferryx/blob/main/LICENSE' -- site README.md docs .github` returns 0 results.
  - Built static HTML contains no stale `ferryx/ferryx` namespace or dead LICENSE link.

### 2. Missing Pages Workflow Trigger for UI Directory

- **Scope / Paths**:
  - `.github/workflows/deploy-pages.yml`
- **RED Evidence**:
  - Workflow push trigger was limited to:
    ```yaml
    paths:
      - 'site/**'
      - '.github/workflows/deploy-pages.yml'
    ```
  - `site` resolves aliases into `ui/src` (e.g. `@ui`), meaning changes in `ui/**` affect the static site build without triggering deployment.
- **Remediation**:
  - Added `- 'ui/**'` under `push.paths` in `.github/workflows/deploy-pages.yml`.
- **GREEN Validation**:
  - `grep -n -F "'ui/**'" .github/workflows/deploy-pages.yml` returns line 9 with `- 'ui/**'`.

### 3. Portable Workspace Commands and License Text in README

- **Scope / Paths**:
  - `README.md`
- **RED Evidence**:
  - README contained relative `./LICENSE` link that points to a non-existent file.
  - Development and validation commands relied on non-portable scripts rather than explicit workspace directory commands.
- **Remediation**:
  - Updated README development instructions to use `cd src-tauri && cargo tauri dev`; the Tauri configuration starts the UI dev server automatically.
  - Updated README validation commands to: `bun run --cwd ui test`, `bun run --cwd site build`, and `cd src-tauri && cargo check`.
  - Removed dead relative file link `./LICENSE` from the License section while keeping the MIT/Apache 2.0 dual license statement.
- **GREEN Validation**:
  - README uses portable explicit commands.
  - No dead `./LICENSE` link in README.

### 4. Delivery Evidence Portable Commands

- **Scope / Paths**:
  - `docs/reference/FERRYX_DOCUMENTATION_DELIVERY_EVIDENCE.md`
- **RED Evidence**:
  - Prior evidence referenced root alias `bun run ui:test` instead of portable workspace command `bun run --cwd ui test`.
- **Remediation**:
  - Updated delivery evidence table to use `bun run --cwd ui test` and `cd src-tauri && cargo check`.
- **GREEN Validation**:
  - Fully aligned with portable workspace execution paths.

## Remaining Risk

- Live GitHub Pages deployment occurs on push to `main`; this remediation validates local site build and static HTML output, but public deployment will execute via the GitHub Actions runner upon commit push.
