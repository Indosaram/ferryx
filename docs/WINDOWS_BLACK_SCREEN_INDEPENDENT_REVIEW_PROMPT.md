# Independent Review Prompt: Ferryx Windows Black Screen

Copy the prompt below verbatim into a fresh coding-agent session.

---

You are performing an **independent, adversarial root-cause review** of a Ferryx Windows desktop failure. Do not trust prior completion claims. The latest and highest-priority user evidence is:

> "여전히 그냥 까만 화면만 나오는데" — the Ferryx window still displays only a black screen.

Your job is to reproduce, diagnose, and report the real cause. This is a **review and investigation task only**: do not edit files, create commits, reset the checkout, or attempt a speculative fix. Return a decision-ready report for the implementing session.

## Observable failure

- Host: SSH alias `maho-win`.
- Actual Windows checkout: `C:\Users\sook\ferryx-winbuild\orca-lite`.
- Required desktop launch command, from that directory: exactly `bun tauri dev`.
- The user sees a Ferryx window whose client area is still entirely black. Earlier it was transparent and showed the desktop through the client area; a Windows opacity override removed the transparency but did not make the UI render.
- Earlier the window also had a duplicated macOS-style native menu. That code was changed to macOS-only, but the current review must independently verify whether the visible menu problem is truly gone.
- Process existence is not proof. Prior runs observed `GUI=1`, `daemon=1`, and Vite listening on port 5173 while the user still saw a black window.
- HTTP 200 responses for `/`, `/src/main.tsx`, and `/src/App.tsx` are not proof that WebView2 navigated, evaluated modules, mounted React, or painted.

## Current checkout constraints

- The actual checkout is intentionally dirty. The latest audit counted **59 dirty entries**: 46 pre-existing user changes plus 13 intended Windows-remediation files.
- Preserve every unrelated change. Do not use destructive Git commands, restore files, overwrite the checkout from another branch, or commit anything.
- Do not directly manipulate the user's desktop, mouse, keyboard, windows, or UI. If a visible check is required, provide an exact manual checklist for the user.
- Only terminate processes demonstrably owned by this checkout, and only if necessary for a faithful reproduction:
  - `C:\Users\sook\ferryx-winbuild\orca-lite\src-tauri\target\debug\ferryx.exe`
  - port-5173 processes whose command line contains the exact checkout path.
- Desktop execution must use the debug app and exactly `bun tauri dev`. Do not launch the binary directly, use `cargo tauri dev`, build a release bundle, create/copy an `.app`, or invent another launch path.

## Prior changes to review skeptically

An isolated review branch exists at:

```text
/private/tmp/ferryx-windows-transparent-fix
branch: windows-transparent-fix
```

Relevant commits there:

```text
96660a7 fix(windows): keep the main window opaque
3b88a43 test(windows): preserve platform opacity split
5d144ce fix(dev): prepare bundled remote assets
771429c fix(windows): omit the macOS application menu
f9bf842 fix(dev): own the frontend server lifecycle
9eb46a6 fix(dev): use Bun command arrays
9d190d1 feat(windows): choose shell for new terminals
b2e3064 fix(dev): bind Vite to the runner lifecycle
5b47f5c docs(windows): audit remediation completion
```

These changes and their tests passed, but the user-visible black screen proves that the completion conclusion was wrong or incomplete. Treat every prior diagnosis as a hypothesis, not as fact.

Important current files include:

```text
src-tauri/tauri.conf.json
src-tauri/tauri.windows.conf.json
src-tauri/src/lib.rs
src-tauri/src/native_terminal/platform/windows.rs
src-tauri/src/native_terminal/surface_host.rs
scripts/dev-frontend.mjs
ui/vite.config.ts
ui/index.html
ui/src/main.tsx
ui/src/App.tsx
ui/src/index.css
ui/src/lib/tauri.ts
ui/src/state/workspaceStore.ts
```

Prior reports that may contain useful evidence but must not be accepted uncritically:

```text
docs/WINDOWS_BUILD_EXECUTABLE_LOCK_2026-08-30.md
docs/WINDOWS_REMEDIATION_COMPLETION_AUDIT_2026-08-30.md
.omo/ulw-loop/01a04fcf-f90f-7878-bd5d-3881f49c4297/reports/current-windows-transparency-completion-audit.md
```

## Required investigation

### 1. Reproduce faithfully

1. Record branch, HEAD, and the dirty-file list without modifying it.
2. Check for checkout-owned Ferryx/Vite processes and port 5173 ownership.
3. Clean only matching QA-owned processes if required.
4. Launch from the actual checkout with exactly:

   ```powershell
   cd C:\Users\sook\ferryx-winbuild\orca-lite
   bun tauri dev
   ```

5. Capture the complete build/runtime log, process tree, executable paths, command lines, Vite owner, and relevant timestamps.
6. Ask the user for a fresh screenshot or precise visible description if needed. Do not infer rendering success from process state.

### 2. Determine which rendering layer is black

Distinguish with evidence among:

- top-level Win32 window background;
- WebView2 controller/composition surface;
- successfully loaded HTML with an empty `#root`;
- React bootstrap or dynamic-import failure;
- fully mounted React tree covered by a native terminal child HWND;
- native terminal child surface incorrectly sized/z-ordered to cover the whole client;
- transparent/opaque composition interaction;
- stale Vite content or wrong dev-server instance;
- WebView2 runtime/profile/cache/data-directory failure;
- CSP, module, asset, HMR, or Tauri protocol failure.

Do not reject native HWND occlusion solely by reading intended bounds. Verify real runtime geometry, parent HWND, visibility, z-order, and timing.

### 3. Instrument WebView2/React rather than guessing

Find the cheapest non-destructive way to obtain direct evidence for all of the following:

- Did WebView2 begin and complete navigation to `http://127.0.0.1:5173`?
- What is the final document URL?
- Did `ui/index.html` execute?
- Does `#root` exist?
- Did `ui/src/main.tsx` execute?
- What value did the Tauri-runtime detection use?
- Did `await import("./App")` resolve or reject?
- Did `ReactDOM.createRoot(...).render(...)` run?
- Are there `console.error`, `window.error`, or `unhandledrejection` records?
- What are the computed client background color and root bounding rectangle?
- Is the React root populated after startup?

Prefer temporary runtime tracing or a minimal diagnostic driver over speculative source changes. If collecting this evidence requires a source edit, stop and describe the exact proposed instrumentation patch instead of applying it.

### 4. Audit Windows native surfaces

Inspect and correlate runtime behavior in:

```text
src-tauri/src/native_terminal/platform/windows.rs
src-tauri/src/native_terminal/surface_host.rs
```

At minimum verify:

- native child HWND parent;
- actual rectangle after every `update_viewport`;
- whether it is shown before React layout establishes valid bounds;
- whether `SetWindowPos(..., HWND_TOP, ...)` can place it over app chrome;
- whether zero, negative, stale, DPI-scaled, or full-window geometry is possible;
- whether multiple restored terminal surfaces overlap the complete WebView;
- whether feature-off or delayed-reveal runs change the black-screen symptom.

### 5. Audit config merging and actual runtime inputs

Verify, from the running Windows build rather than assumptions:

- whether `tauri.windows.conf.json` is automatically merged;
- the effective window `transparent`, decorations, shadow, and background settings;
- the exact `devUrl` used by the built app;
- that the Vite server belongs to the actual checkout;
- that `scripts/dev-frontend.mjs` resolves the intended Vite config and root;
- that the frontend assets loaded by WebView2 match the current checkout contents;
- whether any environment variable, cached WebView profile, or process started from another checkout changes behavior.

### 6. Form and test at least three competing hypotheses

For each hypothesis provide:

- predicted evidence;
- observation or experiment;
- actual evidence;
- accepted/rejected status.

Do not settle on “orphaned Vite” merely because it happened previously. A live correct Vite server coexisted with the black window, so that hypothesis alone is insufficient.

## Required deliverable

Create a report in the repository, without modifying product code:

```text
docs/WINDOWS_BLACK_SCREEN_INDEPENDENT_REVIEW_2026-08-30.md
```

The report must contain:

1. **Verdict first** — the confirmed root cause, or a precise statement that it remains unconfirmed.
2. **Reproduction evidence** — exact command, checkout, process paths, timestamps, and user-visible result.
3. **Layer diagnosis** — which layer is black and how that was proven.
4. **Hypothesis matrix** — at least three hypotheses with accepted/rejected evidence.
5. **Prior-fix audit** — which previous fixes are correct, insufficient, or unrelated.
6. **Smallest recommended fix** — exact files/symbols and why it addresses the observed root cause.
7. **Failing-first regression test or diagnostic contract** — a test that would fail for this black-screen defect, not merely for config text.
8. **Verification plan** — exact automated commands plus the required user-visible manual check.
9. **Dirty-work preservation evidence** — before/after status proving the review changed only the report file.
10. **Confidence and unresolved risks**.

## Success condition

Stop only when the report provides direct runtime evidence identifying the black rendering layer and either:

- confirms a root cause with a minimal, testable fix recommendation; or
- proves exactly what evidence is still unavailable and gives one narrow next diagnostic action.

Do not claim the issue is fixed. Do not mark any visual criterion PASS. The current user-visible state is black-screen RED until the user explicitly confirms otherwise.

---
