# Ferryx Microsoft Store URL remediation

Date: 2026-09-09

## Status

Published, saved in Partner Center, and resubmitted for certification.

- Product: Ferryx, Store ID `9NLHQL5JNLM4`.
- Submission: Submission 1, last modified 09/09/2026.
- Final observed status: `In certification`.
- Final observed stage: `Pre-processing`, step 2 of 4; Submission complete,
  Certification and Publishing not started.
- Existing package retained: `Ferryx_2026.905.1_x64.msix`.
- Partner Center: https://partner.microsoft.com/en-us/dashboard/products/9NLHQL5JNLM4/overview

## URLs for Partner Center

- Website: https://indosaram.github.io/ferryx/
- Privacy policy: https://indosaram.github.io/ferryx/privacy/

Both public URLs returned HTTP 200 before resubmission, including the full policy
text. Before this change, the homepage returned 200, the privacy route returned
404, and both ferryx.app URLs failed DNS resolution.

## Changes

- Added `site/src/content/docs/privacy.md` using the existing Astro Starlight
  document template. No new dependency, custom page layout, or application
  behavior was introduced.
- Added a base-path-aware Privacy Policy link to the homepage footer. Its link
  group wraps on narrow screens.
- Updated Website and Privacy Policy URLs in all four Store submission documents:
  `MICROSOFT_STORE_SUBMISSION_PACK.md`, `ALL_APPS_MICROSOFT_STORE_SUBMISSION_PACK.md`,
  `MS_STORE_SUBMISSION.md`, and `STORE_SUBMISSION_IDENTIFIERS.md`.
- Replaced README's blanket no-remote-data claim with a description of local
  storage and optional network flows, linking to the policy.

## Policy grounding

- Local browser history and deletion: `ui/src/lib/browserHistory.ts:5`,
  `ui/src/lib/browserHistory.ts:41`, and `ui/src/lib/browserHistory.ts:92`.
- Local agent conversation records: `src-tauri/src/ferryx_scope/history/mod.rs:30`.
- Provider process communication: `src-tauri/src/ferryx_scope/chat/mod.rs:25`.
- Remote access defaults to off: `src-tauri/src/remote/state.rs:27`.
- Device revocation removes its authentication tokens:
  `src-tauri/src/remote/auth.rs:214`.
- Microsoft Store versus self-update ownership:
  `src-tauri/src/ipc/updater.rs:3`.
- The website loads Google Fonts: `site/src/pages/index.astro:58`.
- The web-push module is not evidence of working push delivery: its pending
  delivery list is empty in `src-tauri/src/ferryx_scope/push/core.rs:28`.
  The policy does not promise a push delivery service.

The policy names Project Maho as publisher and uses the existing GitHub issue
tracker as the support contact. It explicitly says that issues are public and
asks users to request a private contact method before sharing sensitive details.
No private email address or fixed retention period was invented.

## Verification

- Production command:
  `BASE_URL=/ferryx/ SITE_URL=https://indosaram.github.io bun run --cwd site build`.
  Final build exited 0 and generated `/privacy/index.html` among six pages.
- The published commit also passed a clean build in an isolated checkout based
  on remote main. An initial dependency-symlink build failed with Astro compile
  metadata resolution; installing the locked site dependencies in that checkout
  resolved it without changing source or the lockfile.
- Local production preview returned HTTP 200 for `/ferryx/` and
  `/ferryx/privacy/`.
- Browser checks at 375, 768, and 1280 pixels confirmed the policy title, seven
  content sections, canonical URL, and no document-level horizontal overflow.
- At 375 and 1280 pixels, the homepage footer's Privacy Policy link was
  focusable, within the viewport, and resolved to the policy page.
- Screenshots are under `docs/evidence/privacy-policy-2026-09-09/`. These are
  viewport captures, not full-page captures. This session's model cannot view
  images, so visual appearance has not been independently certified. The checks
  above are browser DOM and geometry evidence, not a visual verdict.
- TypeScript compiler check of `Footer.tsx`, `vite-env.d.ts`, and transitive
  imports: zero diagnostics.
- Full `bun x tsc --noEmit --pretty false -p site/tsconfig.json`: exit 2,
  31 existing errors in untouched files, including missing `@ui/*`,
  `astro:content`, and `bun:test` declarations and existing demo/test type
  errors. The new Footer error found in the first pass was fixed by passing
  the base path from Astro; it is absent from the final diagnostics.
- LSP diagnostics unavailable: the shared LSP daemon could not become reachable
  at `/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`.
- `git diff --check`: clean.
- No tests were added for prose. No desktop app or background PTY daemon was
  started, stopped, or rebuilt.

## Publication and resubmission evidence

- Local implementation commit: `1612dca`.
- Published implementation commit: `f52ab8ad85603aebd97f5c1e3c83faad4da317e4`,
  `fix(site): publish privacy policy and correct Store URLs`.
- Only the eight remediation files were cherry-picked onto remote main. Other
  local commits and other sessions' uncommitted changes were not pushed.
- GitHub Pages run
  https://github.com/Indosaram/ferryx/actions/runs/34303682422 completed with
  `success`, including build and deploy jobs. This deployed the static website,
  not a desktop release.
- Used the existing signed-in Aside browser session to open Ferryx in Partner
  Center. Changed `Apps privacy policy URL` and `Apps website URL`, then clicked
  Save.
- Reopened Properties after saving and read both persisted values back. The
  support URL remained `https://github.com/Indosaram/ferryx/issues`.
- Clicked `Resubmit for certification`. A subsequent accessibility snapshot
  confirmed `In certification` and `Pre-processing`, with the previous
  certification-failure notice replaced by the active certification status.
- The progressbar-disappearance wait timed out; the final status was verified
  from the actual updated page, not inferred from that wait.

No new MSIX was uploaded. Store approval remains Microsoft's decision; successful
resubmission is not a claim that certification has passed.
