# Combined verification — shell selection + retained-presentation batch (BOUNDED)

**Date:** 2026-09-12
**Task ID:** st_01a09591 (verification lane, parent `dag_0af60f86-ac23-45f9-93dd-2b918f67c2e0`)
**Cross-referenced receipts:** `lead-verification.md` (lead direct runs, mon_2SB9XKK748G4B4NZ / mon_YT8XX0MFJ7429MA / mon_B0HRDDWFJBPSKZG4), `shell-implementation.md` (st_01a09586), `presentation-implementation.md`, `baseline.md` (mon_6J2JG08TYR85MD42)
**Bounded verdict:** on the merged shared worktree, the combined shell + retained-presentation batch is green at unit, typecheck, and build level, and the targeted historical Rust bounds regression passes. **The only failing tests are the two pre-existing `NativeTerminalPane.exitAttach.test.tsx` cases (exact IDs below). Live Windows GUI proof is explicitly UNPROVEN** — no startup screenshot, no shell-spawn observation, no HWND geometry, no runtime artifacts exist. No task-wide completion or commit is claimed.
**Runs in flight:** none. This lane's three executions (sections 3–4) all completed before this update was written. This update performed **no new test/build executions** — it added source reads (exitAttach line mapping) and this report only, per the no-rerun boundary.

---

## 1. Pinned commands and test IDs used by this lane (recorded before its runs)

Combined suite, run from `ui/` with the real Node runner (not Bun):

```sh
cd /Users/indo/code/project/orca-lite/ui
node node_modules/vitest/vitest.mjs run --maxWorkers=1 \
  src/components/TabBar.test.tsx \
  src/components/TerminalSplitView.shell.test.tsx \
  src/components/NativeTerminalPane.lifecycle.test.tsx \
  src/components/NativeTerminalPane.presentation.test.tsx
```

Build, run from repo root:

```sh
bun run --cwd ui build        # resolves to `tsc && vite build`
```

Pinned test IDs, verified in source before the run (`rg -n "it("`) — expected totals 19 + 4 + 30 + 9 = 62:

- `src/components/TabBar.test.tsx` (19); shell-lane cases:
  - `offers Windows terminal shell options in the new-tab menu on Windows and forwards selected shell to onAdd` (line 500)
  - `does not offer Windows shell options in the new-tab menu on non-Windows platforms` (line 556)
- `src/components/TerminalSplitView.shell.test.tsx` (4), describe `TerminalSplitView Windows shell selection forwarding`:
  - `forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows` (117)
  - `forwards pwsh, powershell, and wsl shells from TabBar through TabGroupView to onAddTab on Windows` (142)
  - `forwards generic default New Terminal action without shell to onAddTab` (175)
  - `forwards shell selection to onAddTab when all tabs are closed (empty layout fallback)` (198)
- `src/components/NativeTerminalPane.lifecycle.test.tsx` (30 cases: 17 plain `it` + `it.each` expansions 4 + 2 + 4 + 3), describe `NativeTerminalPane compositor ownership lifecycle`
- `src/components/NativeTerminalPane.presentation.test.tsx` (9), describe `native terminal presentation retention`; the three baseline-RED cases:
  - `retains a shown final frame on exit, blocks input, and releases it on unmount` (138)
  - `never reattaches a dead PTY when retained-frame geometry recovery is requested` (173)
  - `holds the final frame until a reconnected replacement is presented` (206)

## 2. Inspected diffs (worktree vs HEAD `b5c97127`)

`git status --porcelain` at diff-inspection time: 14 modified files + untracked `TerminalSplitView.shell.test.tsx`, `docs/evidence/windows-terminal-20260912/`, `site/wrangler.jsonc`. The worktree is under concurrent foreign edits (later snapshot shows additional `src-tauri/src/daemon/protocol.rs`, `daemon/server.rs`, `ipc/tests.rs`, `remote/tests.rs` modifications by other lanes); every foreign edit is preserved and this lane touched only this report. The findings below are bounded to the snapshot inspected.

### Shell-selection lane — faithful RED/GREEN assessed against actual diff

- `ui/src/components/TabBar.tsx` (+43/−11): `WINDOWS_SHELL_OPTIONS` (`pwsh`/`powershell`/`cmd`/`wsl`, stable ids `new-terminal:*`), private `isWindowsPlatform()` reading `navigator.platform`/`userAgent` (runtime seam; no test-only production props), `New Terminal Profile` submenu on Windows only, `actions[option.id] = () => onAdd(option.shell)`. Non-Windows item order/content unchanged.
- `ui/src/components/TerminalSplitView.tsx` (+12/−6): `onAddTab` retyped `(shell?: string) => void` on `TerminalSplitViewProps` and `TabGroupViewProps`; `TabGroupView` forwards `onAdd={(shell) => { focusGroup(); shell !== undefined ? onAddTab(shell) : onAddTab(); }}` — exactly replacing the dropped-argument defect (`onAdd={() => { focusGroup(); onAddTab(); }}`) cited in the RED log.
- `ui/src/components/TabBar.test.tsx` (+119): recursive submenu traversal, Windows forwarding test, non-Windows absence test.
- `ui/src/components/TerminalSplitView.shell.test.tsx` (new, 4): real `TerminalSplitView`, `../lib/nativeMenu` seam, asserts `onAddTab("cmd"|"pwsh"|"powershell"|"wsl")` + shell-less default + empty-layout fallback; navigator stubs (`Win32` / `Windows NT 10.0`) restored in `afterEach`.

Upstream seams intact, no edit needed: `App.tsx` carries `shell` into `cmd_terminal_spawn`.

### Retained-presentation lane — diff matches producer causal claims

`ui/src/components/NativeTerminalPane.tsx` (+42/−3), three coordinated changes consistent with the root cause (cleanup detached a retained surface because live-ownership was the only signal):

1. **Retained-owner seeding**: `isRetainedPresentation = retainedOwner !== null && !retainedOwner.live && retainedOwner.sessionId === targetSessionId`; seeds `lastGeometry = measureGeometry()`, `isAttached = true`; `attemptAttach`'s `!owner.live` early return still blocks dead-PTY reattach.
2. **Retained recovery reroute**: `retryBoundsRef` calls `reportBounds()` when the owner is not live instead of the guaranteed no-op `attemptAttach`; live-path bytes unchanged.
3. **Cleanup handoff guard**: passive cleanup skips `detachNativeTerminalLifecycle` when the incoming render re-armed the same session as a retained owner, emitting `terminal.surface.detach.retained_handoff`; true unmount and live replacement still detach exactly once.

Windows behavior untouched: retained `surfaceSessionId` branch gated on `isMacShortcutPlatform()`; consistent with diff inspection and with the lead's independent review of the same diffs.

### Foreign / other-lane edits (read-only, preserved)

- `ui/src/App.tsx` (+10/−1): `adopted` flag; a successful spawn is no longer closed when only persistence fails.
- `ui/src/state/workspaceStore.ts` (+11/−2): new sessions start `reconnectLifecycle: "spawning"`; `REBIND_SESSION_BACKEND` only rebinding null-backend sessions; `SESSION_BACKEND_UNAVAILABLE` matches `backendSessionId === null` too and resets `reconnectLifecycle: "idle"`.
- `ui/src/components/TerminalPane.tsx` (+11/−6): `spawning`/`validating` treated as reconnecting/pending, not disconnected.
- `src-tauri/*` (7 files, +459/−89 incl. `surface_host.rs` +219): `ipc/terminal.rs` marks pending startup/remote session on spawn; `ipc/native_terminal.rs` marks remote generation at attach. These Rust changes and the App/store/TerminalPane changes above belong to other sessions, not this goal's startup/bounds lanes. This goal's runtime lane owns no production files. Read-only here; no Rust build or broad integration run in this lane.

## 3. This lane's captured runs (retained receipts; not rerun after the lead's)

**Combined four-suite vitest** — **exit 0**, run 21:25:05, single `--maxWorkers=1` pass:

```text
 ✓ src/components/NativeTerminalPane.lifecycle.test.tsx (30 tests) 794ms
 ✓ src/components/TabBar.test.tsx (19 tests) 591ms
 ✓ src/components/TerminalSplitView.shell.test.tsx (4 tests) 140ms
 ✓ src/components/NativeTerminalPane.presentation.test.tsx (9 tests) 89ms

 Test Files  4 passed (4)
      Tests  62 passed (62)
   Start at  21:25:05
   Duration  15.41s (transform 1.87s, setup 779ms, collect 9.00s, tests 1.62s, environment 1.92s, prepare 968ms)
```

(stderr lines in the full log are deliberate fixture IPC failures exercising error paths.)

**exitAttach probe** (separate one-shot run to verify the pre-existing-failure claim) — **exit 1, 2 failed | 2 passed (4)**; exact failures in section 5.

**Build** — `bun run --cwd ui build` from repo root, **exit 0**: `tsc && vite build`, 1892 modules transformed, built in 3.86s (full asset list in run log; only the pre-existing >500 kB chunk informational warning).

## 4. Lead verification cross-reference (receipts in `lead-verification.md`; no reruns)

- **Six-suite direct run** (21:21:54, exit 1): TabBar 19, shell forwarding 4, lifecycle 30, presentation 9 (all three baseline failures green), `src/lib/nativeTerminalLifecycle.test.ts` 7, exitAttach 2 passed / 2 failed → **71 passed, 2 failed**. Lead explicitly notes this is not an overall green run and assertions were not weakened.
- **Targeted Rust regression** (`cargo test --lib browser_child -- --test-threads=1`, exit 0): `bounds_ipc_presents_when_browser_child_is_open` and `output_presents_when_browser_child_is_open` passed, 1030 filtered out. Bounded: historical multi-webview lookup fix only, on the shared foreign tree — **not** the current Windows startup defect, **not** a pristine baseline.
- **LSP + build**: four shell files clean; NativeTerminalPane no errors + one pre-existing `keyCode` hint; build exit 0 (1892 modules, 3.19s).
- **Scope review**: shell source diff, forwarding test, presentation diff + layout cleanup inspected; popup IPC supports the submenu and emits the selected ID app-wide.

## 5. Coverage review and the exact two failures

Concordance matrix (all retained receipts; nothing duplicated):

| Scope | This lane | Lead | Concordant? |
| --- | --- | --- | --- |
| TabBar + shell forwarding + lifecycle + presentation (62) | exit 0, 62/62 @21:25:05 | all pass inside six-suite run | yes |
| exitAttach suite | 2 fail / 2 pass, exit 1 | same 2 fail / 2 pass | yes, two independent runs |
| `nativeTerminalLifecycle` module (7) | not run | 7 passed | covered by lead |
| Targeted Rust `browser_child` bounds regression | not run | exit 0, 2 passed | covered by lead |
| `bun run --cwd ui build` | exit 0, 3.86s | exit 0, 3.19s | yes, two independent runs |
| LSP diagnostics | 6 changed files, no errors | shell files + NativeTerminalPane, no errors | yes, hints only |
| Diff inspection | all 14 modified files read | shell + presentation diffs, popup IPC | complementary |
| Live Windows GUI | unproven | unproven | concordant gap |

### The exact two exitAttach failures (both runs agree; pre-existing, outside this batch)

1. **`suppresses error badge and cancels retry when cmd_native_terminal_attach returns SESSION_NOT_FOUND with typed details`** (defined at `NativeTerminalPane.exitAttach.test.tsx:90`); failing assertion at **:124** —
   `expect(onUnavailable).toHaveBeenCalledWith("dead-backend-session", "daemon-attach-not-found")`.
   Production now invokes the callback with a third binding key: `("dead-backend-session", "daemon-attach-not-found", "dead-backend-session::0:")`.
2. **`suppresses error badge for legacy INTERNAL_ERROR with exact Session not found message`** (defined at :162); failing assertion at **:190** —
   `expect(onUnavailable).toHaveBeenCalledWith("legacy-dead-session", "legacy-internal-error")`.
   Production now passes `("legacy-dead-session", "legacy-internal-error", "legacy-dead-session::0:")`.

Root cause: the optional third `bindingKey` parameter in the callback contract (`NativeTerminalPane.tsx:49`, call site :1994 — verified by this lane) landed in commit `7404a44f`; the two assertions still pin two arguments. This batch did not change the contract or these tests. **Action for the owning lane:** update the two assertions (e.g. expect the three-argument form or use `expect.any(String)`/`expect.stringMatching(/::0:$/)` for the key). No pristine-tree rerun of the baseline claim was performed by lead or this lane; the presentation worker's stash-based claim is corroborated but not independently reproduced.

## 6. GUI evidence: explicitly UNPROVEN

Unit/typecheck/build green does **not** establish any Windows user-facing behavior. Missing, per `COMPLETION-CHECKLIST.md` (still NOT COMPLETE) and the lead's scope review:

- A live `bun tauri dev` run occurred in the isolated Windows checkout. The lead read its actual stdout: the debug build completed, then WebView2 creation failed with `HRESULT(0x8007139F)` before terminal attachment. See `runtime/lead-startup-observation.md`. This is not a successful GUI run or RED evidence for the native-bounds defect. Screenshots, successful CDP capture, HWND geometry and cleanup receipts remain unproven.
- Windows startup cause unconfirmed; actual shell spawning unobserved (no menu screenshot, no `echo FERRYX_WIN_SHELL_OK` output).
- No startup screenshot with `echo FERRYX_WIN_START_OK` and no bounds-failure-free log evidence; no resize check with `echo FERRYX_WIN_RESIZE_OK`.
- Local test/build runs used the shared merged worktree. The initial Windows debug launch used the committed `fa429aac` QA checkout, but failed before a terminal appeared. Neither establishes a passing pristine Windows terminal baseline.

## 7. Bounded conclusions

1. Established (merged shared worktree): shell menu + faithful forwarding and retained-presentation repairs pass all pinned unit tests (62/62 this lane; 71/73 incl. exitAttach and the lifecycle module under lead), typecheck and build pass (two independent runs), targeted historical Rust bounds regression passes (lead).
2. Failing: exactly the two exitAttach cases above, pre-existing, assertion-drift vs. the three-argument callback contract; owner action defined.
3. Unproven: all live Windows GUI criteria (startup, shell choice, resize), the startup root cause, and pristine-baseline behavior. These remain gated on the runtime lane; no completion or commit is claimed by this report.
4. Boundaries honored: no reruns in this update; no source edits or fixes by this lane; foreign work (14 modified files, `site/wrangler.jsonc`, all other reports) preserved; no commits, pushes, release builds, desktop launches, or daemon actions.
