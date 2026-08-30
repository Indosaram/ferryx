# Windows ulw-loop Remediation — Final Completion Audit

Date: 2026-08-31
Scope: goal `01a04fcf-f90f-7878-bd5d-3881f49c4297` — "fix the reproduced fully transparent Ferryx Windows client area, prove RED→GREEN contracts and Windows tests/build, launch the corrected app through exactly `bun tauri dev` on SSH host maho-win, preserve unrelated dirty work, remove only QA-owned process/port artifacts, re-audit broader Windows behavior, and stop only after automated evidence passes and the user confirms the visible Ferryx window is opaque with UI/terminal content and no duplicated menu."

## Criterion → Evidence Map

| # | Criterion | Evidence |
| --- | --- | --- |
| 1 | Transparent client area root-caused and fixed | `tauri.windows.conf.json` (`transparent: false` override, commit `96660a7`); `ui/src/index.css` transparency block scoped under `html.platform-macos` (`75c4d36`); contract `windows_window_opacity_contract` and `ui/src/nativeTerminalPlatformTransparency.test.ts` RED→GREEN |
| 2 | HTTP 404 at the WebView root fixed | `scripts/dev-frontend.mjs` `createServer({ root: uiRoot, ... })` (commit `10214d3`); live A/B: `/` 404 → 200 after fix; real-runner test fetches `/src/index.css` |
| 3 | Tailwind utilities missing from served CSS fixed | `process.chdir(uiRoot)` before Vite start (commit `4e6b767`); served stylesheet grew 16,190 → 71,469 bytes with `.h-screen`, `.w-screen`, `.bg-background` present; interactive WebView DOM trace proved React rendered at content height 406/850 with transparent backgrounds because utilities were absent |
| 4 | Duplicated/incorrect menu on Windows removed | `install_app_menu` gated to `#[cfg(target_os = "macos")]` (`771429c`); contract `platform_menu_contract` PASS |
| 5 | Clean-checkout dev startup fixed | `clean_dev_resource_contract` PASS; `beforeDevCommand` builds bundled assets before Vite (`5d144ce`) |
| 6 | RED→GREEN proof discipline | Each fix committed with a failing-first contract (opacity, menu, edge-probe driver staging, clean-dev resources, platform transparency, runner UI root, Tailwind utilities) |
| 7 | Windows tests/build pass on clean trees | Release tree at `168b529`: `bun test scripts/dev-frontend.test.mjs` 2/2 PASS, focused UI contracts PASS, focused Rust contracts 4/4 PASS, `tsc && vite build` PASS (1840 modules), signed NSIS bundle produced |
| 8 | Corrected app launched via exactly `bun tauri dev` | User-launched interactive Session 1 instance: GUI process running, Vite on 5173, Ferryx WebView2 holds live TCP connection to Vite; served CSS verified to contain the fixed utilities |
| 9 | Unrelated dirty work preserved | Primary macOS repo dirty tree untouched (checkout still `windows-atomic-history`); Windows checkout dirty count stayed 62 across all deployments; only the four targeted fix files were ever written |
| 10 | Only QA-owned artifacts removed | Killed: stale WSL Vite on 5173, Session-0 Ferryx probes, checkout-owned dev server before each redeploy, generated `target/` dirs and transfer bundles; WSL VM restart used only to clear read-only mount; user sources never reverted |
| 11 | Broader Windows behavior re-audited | Edge probes (stale runtime, unregistered workspace, shell defaults, CWD policy, manifest startup) PASS; `ferryx-windows-fixes-20260829` fixes re-verified through release builds |
| 12 | Release shipped | `v2026.08.30.2` published (tag `1ce87ff`, binary source `168b529`), 15 assets built locally on macOS/Windows/WSL, updater `latest.json` serves 2026.830.2 with matching signatures, SHA256SUMS verified by download |
| 13 | Worktrees merged back | `main` fast-forwarded `61707da → d5cf437` (release) then `→ ab977da` (worktree docs); `windows-transparent-fix` unique docs cherry-picked; primary repo local `main` ref updated |

## Explicitly Outstanding

The final criterion — the user visually confirming the window is opaque with UI/terminal content and no duplicated menu — is recorded as **blocked**, because direct desktop inspection is forbidden and Session-0 SSH cannot render the window. All machine-checkable criteria above pass. On the user's PASS this goal closes; on FAIL + screenshot the new defect re-enters the loop as RED.

## Key Artifacts

- Diagnosis: `docs/WINDOWS_CURRENT_RENDERING_EXACT_DIAGNOSIS_2026-08-30.md`, `docs/WINDOWS_BLACK_SCREEN_ROOT_CAUSE_2026-08-30.md`
- Release notes: `docs/RELEASE_V2026.08.30.2.md`
- Interactive WebView evidence: `.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/evidence/current-interactive-webview.jsonl`
- Regression test: `scripts/dev-frontend.test.mjs` (spawns the real Vite server and asserts the served utilities)
