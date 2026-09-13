# V05 Resume Integration and Validation Report

**Date:** 2026-09-12  
**Task ID:** `st_01a097ff`  
**Parent Session:** `01a097f8-4568-7573-897e-d61f0fe6d692`  
**Source Worktree (READ ONLY):** `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean`  
**Destination Worktree:** `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`  
**Base Commit:** `e1a00339a5339ed4d9c9634e26b86f41111d49f6`  

---

## 1. Executive Summary

The 13 preserved V05 UI regression repair files specified by `/Users/indo/code/project/orca-lite-wt/herdr-wave0-clean/docs/evidence/paired-daemon/V05-parent-source-manifest.json` have been verified and integrated into `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`.

All 13 files match their manifest SHA-256 digests exactly before and after integration. Pre-existing uncommitted work in `src-tauri` from the concurrent backend A09 worker was preserved untouched (strictly read-only). LSP diagnostics reported zero errors or warnings. Symlink hygiene was enforced: the `ui/dist` symlink to Wave1 was verified and unlinked before building, so the UI build output to a private directory without touching Wave1.

Verification confirmed:
- **Focused Vitest Suite:** 17 test files, 347 tests passed, 0 failed, 0 pending (Exit code: 0).
- **UI Production Build (`tsc && vite build`):** 1,883 modules transformed, private `ui/dist` generated (Exit code: 0).
- **Pixel Review Reference:** Scoped settings-fixture pass recorded by image-capable reviewer `st_01a09800` (`docs/evidence/paired-daemon/V05-image-capable-review.md`).
- **Residual Baseline Gates:** 3 pre-existing PushClient stub unit failures in full UI suite remain unresolved as expected baseline limitations (not weakened or modified). Native desktop manual proof remains unexecuted and unearned by unit tests.

---

## 2. Source Hash Verification Matrix

Every candidate source and test file was hashed before copying from the read-only source worktree and re-hashed after writing to the destination worktree. All 13 SHA-256 hashes matched the authoritative manifest byte-for-byte.

| # | File Path | Manifest SHA-256 | Source (`wave0-clean`) SHA-256 | Destination (`resume`) SHA-256 | Status |
|---|---|---|---|---|---|
| 1 | `ui/src/test/setup.ts` | `97dbdabc4b78b6179b07778665356047813cbc23e00f92754af88eb87b95a12e` | `97dbdabc4b78b6179b07778665356047813cbc23e00f92754af88eb87b95a12e` | `97dbdabc4b78b6179b07778665356047813cbc23e00f92754af88eb87b95a12e` | MATCH |
| 2 | `ui/src/App.test.tsx` | `2c5d4fd68e8bf46f26f55db7eb4dcf8839a4b306a820b14511cc581ec01a7bb6` | `2c5d4fd68e8bf46f26f55db7eb4dcf8839a4b306a820b14511cc581ec01a7bb6` | `2c5d4fd68e8bf46f26f55db7eb4dcf8839a4b306a820b14511cc581ec01a7bb6` | MATCH |
| 3 | `ui/src/App.remote.test.tsx` | `dd70963dfc57ed50b1d1cce602f792427581dc59a207c0f140ccb5283e18fce4` | `dd70963dfc57ed50b1d1cce602f792427581dc59a207c0f140ccb5283e18fce4` | `dd70963dfc57ed50b1d1cce602f792427581dc59a207c0f140ccb5283e18fce4` | MATCH |
| 4 | `ui/src/components/NativeTerminalPane.exitAttach.test.tsx` | `9ed0aa2871fa3b6815a3f5a0987b4f6ed9c408bb045791590ec4a815554818be` | `9ed0aa2871fa3b6815a3f5a0987b4f6ed9c408bb045791590ec4a815554818be` | `9ed0aa2871fa3b6815a3f5a0987b4f6ed9c408bb045791590ec4a815554818be` | MATCH |
| 5 | `ui/src/components/NativeTerminalPane.presentation.test.tsx` | `9721c3125e348db6baa1740ad6237c2e9bdd8d4b2e2b1dee6c44f4846d341aa9` | `9721c3125e348db6baa1740ad6237c2e9bdd8d4b2e2b1dee6c44f4846d341aa9` | `9721c3125e348db6baa1740ad6237c2e9bdd8d4b2e2b1dee6c44f4846d341aa9` | MATCH |
| 6 | `ui/src/components/NativeTerminalPane.tsx` | `3b0b2433207e7db40969ccfee35c75916ab835b02b683a76accec891cc977686` | `3b0b2433207e7db40969ccfee35c75916ab835b02b683a76accec891cc977686` | `3b0b2433207e7db40969ccfee35c75916ab835b02b683a76accec891cc977686` | MATCH |
| 7 | `ui/src/components/TerminalPane.exitAttach.integration.test.tsx` | `5e9e9b2ae07bb8a43f01e4f36094e8e1582250edecd1f253909b116bbc61d148` | `5e9e9b2ae07bb8a43f01e4f36094e8e1582250edecd1f253909b116bbc61d148` | `5e9e9b2ae07bb8a43f01e4f36094e8e1582250edecd1f253909b116bbc61d148` | MATCH |
| 8 | `ui/src/components/Sidebar.dnd.test.tsx` | `7561778b887a5e4bf2e12f17a8a99b7fc8f862130fde61957765f432b476aaf8` | `7561778b887a5e4bf2e12f17a8a99b7fc8f862130fde61957765f432b476aaf8` | `7561778b887a5e4bf2e12f17a8a99b7fc8f862130fde61957765f432b476aaf8` | MATCH |
| 9 | `ui/src/components/settings/SshSection.tsx` | `171c8564211405d76580f4c201e539614f49f81d64a6e2031d9f550ffe740998` | `171c8564211405d76580f4c201e539614f49f81d64a6e2031d9f550ffe740998` | `171c8564211405d76580f4c201e539614f49f81d64a6e2031d9f550ffe740998` | MATCH |
| 10 | `ui/src/components/settings/RemoteAccessSection.tsx` | `6d30b247b74fdd8e4f5806a18322b2b841458891e9a1fc1bd97d6d097444174f` | `6d30b247b74fdd8e4f5806a18322b2b841458891e9a1fc1bd97d6d097444174f` | `6d30b247b74fdd8e4f5806a18322b2b841458891e9a1fc1bd97d6d097444174f` | MATCH |
| 11 | `ui/src/lib/tauri.test.ts` | `d995f91ed2b5aee85d47d2dfd9b88ef30b9f0abc0ce50e4e7851bf97ca0d4c6a` | `d995f91ed2b5aee85d47d2dfd9b88ef30b9f0abc0ce50e4e7851bf97ca0d4c6a` | `d995f91ed2b5aee85d47d2dfd9b88ef30b9f0abc0ce50e4e7851bf97ca0d4c6a` | MATCH |
| 12 | `ui/src/lib/terminalTransport/terminalTransport.test.ts` | `e7f0603f86b960bb960d1cedfe518a9d0612215e665adecf4e1aef77059fcaf9` | `e7f0603f86b960bb960d1cedfe518a9d0612215e665adecf4e1aef77059fcaf9` | `e7f0603f86b960bb960d1cedfe518a9d0612215e665adecf4e1aef77059fcaf9` | MATCH |
| 13 | `ui/src/state/remoteHostStore.test.ts` | `17e69b13cc76f643de5ba702dd7782e02c90f33cc6f90cbd650639335ffb088c` | `17e69b13cc76f643de5ba702dd7782e02c90f33cc6f90cbd650639335ffb088c` | `17e69b13cc76f643de5ba702dd7782e02c90f33cc6f90cbd650639335ffb088c` | MATCH |

---

## 3. Pre-Integration Destination Audit & Concurrency Safety

1. **Clean UI State:** Prior to copying, `git diff HEAD -- ui/` confirmed zero modified tracked files in `ui/`.
2. **Backend Concurrency:** `src-tauri` contains uncommitted changes authored by the active backend A09 worker. `src-tauri` was treated as strictly READ ONLY and left untouched.
3. **Diff Scope:** After integration, `git diff --name-only HEAD -- ui/` lists strictly and exclusively the 13 manifest paths. No unrelated files in `ui/` were touched. `git diff --check HEAD -- ui/` passed with 0 whitespace issues.

---

## 4. Symlink Hygiene and Build Isolation

1. **Pre-build check:** `ui/dist` was confirmed to be a symlink pointing to `/Users/indo/code/project/orca-lite-wt/herdr-wave1/ui/dist`.
2. **Safe removal:** Only the `ui/dist` symlink was removed via `rm ui/dist`. The target directory `/Users/indo/code/project/orca-lite-wt/herdr-wave1/ui/dist` was verified untouched (mtime Sep 13 05:25 intact).
3. **Private build directory:** Production build `tsc && vite build` recreated `ui/dist` as a local private directory (`drwxr-xr-x@ 17`).
4. **Dependency link:** `ui/node_modules` remains a read-only symlink to `/Users/indo/code/project/orca-lite-wt/herdr-wave1/ui/node_modules`.

---

## 5. Verification Results

### 5.1 LSP Diagnostics
LSP diagnostics were executed across all 13 integrated files.
- **Errors:** 0
- **Warnings:** 0
- **Pre-existing Hints:** 2 (TypeScript deprecation hints on unchanged lines: `keyCode` at `NativeTerminalPane.tsx:190` and `document.execCommand` at `RemoteAccessSection.tsx:85`).

### 5.2 Focused 17-File Vitest Suite
Executed via Node 22.22.3 Vitest runner (`node node_modules/vitest/vitest.mjs run ... --maxWorkers=1`):
- **Command Exit Code:** `0`
- **Duration:** 13.13s
- **Test Files:** 17 passed / 17 total
- **Tests:** 347 passed / 347 total (0 failed, 0 pending)
- **Evidence Files:**
  - JSON report: `docs/evidence/paired-daemon/V05-resume-focused.json`
  - Execution log: `docs/evidence/paired-daemon/V05-resume-focused.log`

Files included in focused run:
1. `src/App.remote.test.tsx` (19 passed)
2. `src/App.test.tsx` (92 passed)
3. `src/appearanceThemeContract.test.ts` (7 passed)
4. `src/components/NativeTerminalPane.exitAttach.test.tsx` (4 passed)
5. `src/components/NativeTerminalPane.lifecycle.test.tsx` (30 passed)
6. `src/components/NativeTerminalPane.presentation.test.tsx` (14 passed)
7. `src/components/SettingsDialog.test.tsx` (29 passed)
8. `src/components/Sidebar.dnd.test.tsx` (7 passed)
9. `src/components/TerminalPane.exitAttach.integration.test.tsx` (1 passed)
10. `src/lib/nativeTerminalAttachPolicy.test.ts` (50 passed)
11. `src/lib/nativeTerminalLifecycle.test.ts` (7 passed)
12. `src/lib/nativeTerminalVisibility.test.tsx` (14 passed)
13. `src/lib/tauri.test.ts` (31 passed)
14. `src/lib/updater.test.ts` (20 passed)
15. `src/test/viteHmrConfig.test.ts` (1 passed)
16. `src/lib/terminalTransport/terminalTransport.test.ts` (8 passed)
17. `src/state/remoteHostStore.test.ts` (13 passed)

### 5.3 UI Production Build
Executed via `./node_modules/.bin/tsc && ./node_modules/.bin/vite build`:
- **Command Exit Code:** `0`
- **Duration:** 2.16s
- **Modules Transformed:** 1,883 modules
- **Output:** Built cleanly into private directory `ui/dist/`
- **Evidence Log:** `docs/evidence/paired-daemon/V05-resume-build.log`

---

## 6. Visual Review Status

Pixel verification was performed by image-capable reviewer `st_01a09800` (`gemini-3.8-flash-high`) and documented in `docs/evidence/paired-daemon/V05-image-capable-review.md`.
- **Verdict:** PASS for all eight captures (`ssh-desktop-light`, `ssh-desktop-dark`, `ssh-mobile-light`, `ssh-mobile-dark`, `remote-desktop-light`, `remote-desktop-dark`, `remote-mobile-light`, `remote-mobile-dark`).
- **Scope Boundary:** Confirms layout, typography, clipping absence, and presence of the green `Added` checkmark and wrapped Remote description strictly within the deterministic IPC browser QA component fixtures.
- **Explicit Limitations:** Does not measure WCAG contrast ratios and does not certify full-application or native desktop runtime rendering.

---

## 7. Baseline Limitations and Residual Unresolved Gates

1. **Full UI Suite PushClient Stub Failures (3 tests):**
   In `ui/src/features/ferryx/push/client.test.ts`:
   - `accepts only same origin exact task links`: expected URL, received null.
   - `denied permission never subscribes`: expected `denied`, received `enabled`.
   - `server unsubscribe precedes local removal and failure preserves subscription`: expected rejection, resolved `disabled`.
   These three tests fail against the unbuilt feature stub in both baseline and candidate states. They are not regressions introduced by V05, and have not been removed, skipped, or weakened. The full UI test run will reflect these 3 failures until the push client feature packet is implemented.

2. **Native Desktop Manual Verification (Unexecuted):**
   Native terminal compositor behaviors cannot be proven by Vitest JSDOM or browser fixtures:
   - Preserving the final rendered frame across shell exit even when session metadata is cleared.
   - Blocking keyboard input to exited native panes.
   - Preserving and transferring native OS dialog ownership and unmasking on drag-and-drop.
   - Same-backend-ID daemon epoch / remote generation reattachment and input fencing on live desktop surfaces.
   These gates remain unexecuted and must be validated through genuine macOS/native desktop runtime sessions.
