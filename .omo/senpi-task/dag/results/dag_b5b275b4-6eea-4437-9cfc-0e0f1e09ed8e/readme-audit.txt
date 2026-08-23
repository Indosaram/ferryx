> I read this as an audit request for the README contract - I will report the exact verified claims, URLs, manifests, and commands based on the workspace inspection.

### 1. Canonical Repository, URLs, and GitHub Metadata

- **Canonical Clone URL**: `https://github.com/Indosaram/ferryx.git`
  - *Evidence*: `git remote get-url origin` returned `https://github.com/Indosaram/ferryx.git`.
- **GitHub Pages URL**: `https://indosaram.github.io/ferryx/`
  - *Evidence*: Configured deployment path for repository `Indosaram/ferryx` via `.github/workflows/deploy-pages.yml`.
- **Current GitHub Homepage Value**: `""` (empty string / unset)
  - *Evidence*: `gh api repos/Indosaram/ferryx --jq .homepage` returned an empty response.
- **Branding Discrepancy Note**: `site/astro.config.mjs` configures `social.github` as `https://github.com/ferryx/ferryx` instead of `https://github.com/Indosaram/ferryx`. The README should use `Indosaram/ferryx`.

---

### 2. Verified Product Claims Justified by Manifests

| Claim | Manifest & Config Backing | Direct File Path |
| --- | --- | --- |
| **Product Name & Role**: Ferryx, an ultra-lightweight workspace and AI agent launcher | `name = "ferryx"`, `description = "Ultra-lightweight workspace & AI agent launcher (Tauri v2 + Rust)"` | `src-tauri/Cargo.toml`, `package.json` |
| **Desktop Core**: Tauri v2 application shell | `tauri = { version = "2" }`, `tauri-build = "2"` | `src-tauri/Cargo.toml`, `ui/package.json` |
| **Terminal & PTY**: Direct kernel pseudoterminal allocation with async runtime | `portable-pty = "0.8"`, `tokio = { version = "1.43" }`, `libc = "0.2"` | `src-tauri/Cargo.toml` |
| **UI Stack**: React 18, xterm.js terminal with WebGL, split tiling | `react = "^18.3.1"`, `@xterm/xterm = "^6.0.0"`, `@xterm/addon-webgl`, `@dnd-kit/core` | `ui/package.json` |
| **Mobile Remote Gateway**: Axum WebSocket and HTTP gateway for remote control | `axum = { version = "0.8" }`, `tower-http`, `qrcode = "^1.5.4"` | `src-tauri/Cargo.toml`, `ui/package.json` |
| **Audio & Notifications**: Audio cue playback and native desktop alerts | `rodio = "0.22"`, `tauri-plugin-notification = "2"`, macOS `objc2-user-notifications` | `src-tauri/Cargo.toml` |
| **Documentation & Landing Site**: Astro v5 and Starlight static documentation | `astro = "^5.3.0"`, `@astrojs/starlight = "^0.32.0"`, port `14173` | `site/package.json`, `site/astro.config.mjs` |

---

### 3. Verified Runnable Commands

#### Setup Commands
- Frontend dependencies: `bun install --cwd ui`
- Documentation site dependencies: `bun install --cwd site`

#### Development Commands
- Full desktop development: `bun run dev` (triggers `cargo tauri dev`)
- Frontend standalone development: `bun run ui:dev` (triggers `bun run --cwd ui dev`)
- Documentation site development: `bun run --cwd site dev` (runs `astro dev --port 14173`)

#### Build and Quality Verification Commands
- UI unit tests: `bun run ui:test` (runs `vitest run --maxWorkers=1` in `ui/`)
- UI production build: `bun run ui:build` (runs `tsc && vite build` in `ui/`)
- Site production build: `bun run --cwd site build` (runs `astro build` in `site/`)
- Site local preview: `bun run --cwd site preview` (runs `astro preview --port 14173`)
- Rust target check: `cargo check --manifest-path src-tauri/Cargo.toml`
- Rust backend tests: `cargo test --manifest-path src-tauri/Cargo.toml`
- Rust linter checks: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- Tauri release packaging: `bun run build` (triggers `cargo tauri build`)

---

### 4. Commands to Remove or Clarify

- **`cargo tauri dev` as a standalone command without global CLI note**:
  - *Reason*: Direct invocation of `cargo tauri dev` requires `cargo-tauri` or `@tauri-apps/cli` to be installed in Cargo's binary path.
  - *Recommendation*: Clarify that `bun run dev` or `cargo tauri` requires the Tauri CLI cargo plugin, or document running via `bun run dev`.
- **Root alias gaps**:
  - *Observation*: Root `package.json` defines `ui:dev`, `ui:test`, and `ui:build`, but lacks `site:dev` and `site:build`. Using `bun run --cwd site <script>` is required and works correctly. Keep the `--cwd site` syntax explicit in the README.

---

### 5. Summary of Manifest File Paths Inspected

- `README.md`
- `package.json`
- `ui/package.json`
- `site/package.json`
- `src-tauri/Cargo.toml`
- `site/astro.config.mjs`