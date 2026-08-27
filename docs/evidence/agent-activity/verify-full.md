# Full Verification Report: Ferryx Agent-Activity Fix

## Git Status & Diff Surface

Command:
```bash
git status --porcelain
```

Output:
```
 M scripts/macos-dev-runner.sh
 M src-tauri/src/native_terminal/surface_host.rs
 M src-tauri/src/native_terminal/terminal.rs
 M src-tauri/tests/macos_dev_bundle_contract.rs
 M ui/src/lib/agentTitle.ts
 M ui/src/state/workspaceStore.ts
?? docs/evidence/agent-activity/
?? src-tauri/scripts
?? ui/activity-qa.html
?? ui/src/devtools/
?? ui/src/state/activityRenderChain.test.tsx
?? ui/src/state/activityStatePersistence.test.ts
```

Detailed file listing (`git status --porcelain -uall`):
- Tracked modified files:
  - `scripts/macos-dev-runner.sh` (**OUTSIDE** `ui/src` and `src-tauri/src/native_terminal`)
  - `src-tauri/src/native_terminal/surface_host.rs` (INSIDE `src-tauri/src/native_terminal`)
  - `src-tauri/src/native_terminal/terminal.rs` (INSIDE `src-tauri/src/native_terminal`)
  - `src-tauri/tests/macos_dev_bundle_contract.rs` (**OUTSIDE** `ui/src` and `src-tauri/src/native_terminal`)
  - `ui/src/lib/agentTitle.ts` (INSIDE `ui/src`)
  - `ui/src/state/workspaceStore.ts` (INSIDE `ui/src`)
- Untracked files / artifacts:
  - `docs/evidence/agent-activity/probe-rust-title.md` (**OUTSIDE**)
  - `docs/evidence/agent-activity/probe-ui-render.md` (**OUTSIDE**)
  - `docs/evidence/agent-activity/surface-green/GREEN-log.txt` (**OUTSIDE**)
  - `docs/evidence/agent-activity/surface-green/GREEN-working.json` (**OUTSIDE**)
  - `docs/evidence/agent-activity/surface-red/RED-log.txt` (**OUTSIDE**)
  - `docs/evidence/agent-activity/surface-red/RED-working.json` (**OUTSIDE**)
  - `src-tauri/scripts` (**OUTSIDE**)
  - `ui/activity-qa.html` (**OUTSIDE**)
  - `ui/src/devtools/ActivitySurfaceHarness.tsx` (INSIDE `ui/src`)
  - `ui/src/devtools/activityQaMain.tsx` (INSIDE `ui/src`)
  - `ui/src/devtools/captureActivitySurface.sh` (INSIDE `ui/src`)
  - `ui/src/state/activityRenderChain.test.tsx` (INSIDE `ui/src`)
  - `ui/src/state/activityStatePersistence.test.ts` (INSIDE `ui/src`)

### Files Outside `ui/src` and `src-tauri/src/native_terminal`
Yes, files outside `ui/src` and `src-tauri/src/native_terminal` were modified or added:
1. `scripts/macos-dev-runner.sh` (modified)
2. `src-tauri/tests/macos_dev_bundle_contract.rs` (modified)
3. `src-tauri/scripts` (untracked symlink/dir)
4. `ui/activity-qa.html` (untracked QA harness file)
5. `docs/evidence/agent-activity/*` (untracked test evidence artifacts)

---

## Gate 1: UI Vitest Test Suite

Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && npx vitest run
```

Captured Output Tail:
```
 Test Files  98 passed (98)
      Tests  845 passed (845)
   Start at  01:17:30
   Duration  53.49s (transform 851ms, setup 6.25s, collect 10.58s, tests 7.87s, environment 17.68s, prepare 2.68s)
```

Result:
- Test Files: 98 passed (98 total)
- Tests: 845 passed (845 total)
- Failed: 0
- Skipped: 0

---

## Gate 2: TypeScript Compilation Check

Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && npx tsc --noEmit; echo "TSC_EXIT_CODE: $?"
```

Captured Output:
```
TSC_EXIT_CODE: 0
```

Result:
- Exit Code: 0
- Diagnostic Output: None (clean compilation)

---

## Gate 3: UI Production Build

Command:
```bash
cd /Users/indo/code/project/orca-lite && bun run --cwd ui build; echo "BUILD_EXIT_CODE: $?"
```

Captured Output:
```
$ tsc && vite build
vite v6.4.3 building for production...
transforming...
✓ 1724 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                              1.18 kB │ gzip:  0.55 kB
dist/assets/geist-variable-CrgPqtmy.woff2   69.44 kB
dist/assets/ferryx-icon-OXRkkUvz.png       805.82 kB
dist/assets/index-D5S6x2Qk.css              47.92 kB │ gzip:  8.85 kB
dist/assets/check-By0xOxoC.js                0.29 kB │ gzip:  0.24 kB
dist/assets/x-CYBgYtGw.js                    4.49 kB │ gzip:  1.22 kB
dist/assets/browser-DkdnF056.js             25.78 kB │ gzip: 10.13 kB
dist/assets/RemoteApp-Zzr4mufJ.js           30.96 kB │ gzip:  9.47 kB
dist/assets/SettingsDialog-Bp0Zh8BJ.js      55.08 kB │ gzip: 13.12 kB
dist/assets/index-BjM7q95g.js              173.40 kB │ gzip: 55.86 kB
dist/assets/App-BKe72okQ.js                323.17 kB │ gzip: 91.53 kB
✓ built in 1.56s
BUILD_EXIT_CODE: 0
```

Result:
- Exit Code: 0
- Build succeeded cleanly

---

## Gate 4: Rust Unit Tests (`native-terminal` feature)

Command:
```bash
cargo test --manifest-path /Users/indo/code/project/orca-lite/src-tauri/Cargo.toml --lib --features native-terminal
```

Captured Output Tail:
```
test result: ok. 285 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.86s
```

Result:
- Passed: 285
- Failed: 0
- Ignored: 0
- Measured: 0
- Filtered out: 0

---

## Gate 5: Specific Activity Test Verification

### 5.1 `ui/src/state/activityStatePersistence.test.ts`
Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && npx vitest run src/state/activityStatePersistence.test.ts
```
Captured Output:
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/state/activityStatePersistence.test.ts (3 tests) 4ms

 Test Files  1 passed (1)
      Tests  3 passed (3)
   Start at  01:18:52
   Duration  511ms (transform 105ms, setup 82ms, collect 110ms, tests 4ms, environment 202ms, prepare 29ms)
```
Count: 1 test file passed, 3 tests passed.

### 5.2 `ui/src/state/activityRenderChain.test.tsx`
Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && npx vitest run src/state/activityRenderChain.test.tsx
```
Captured Output:
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/state/activityRenderChain.test.tsx (2 tests) 49ms

 Test Files  1 passed (1)
      Tests  2 passed (2)
   Start at  01:18:52
   Duration  814ms (transform 126ms, setup 83ms, collect 367ms, tests 49ms, environment 201ms, prepare 30ms)
```
Count: 1 test file passed, 2 tests passed.

### 5.3 `ui/src/state/workspaceNativeActivity.test.tsx`
Command:
```bash
cd /Users/indo/code/project/orca-lite/ui && npx vitest run src/state/workspaceNativeActivity.test.tsx
```
Captured Output:
```
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/state/workspaceNativeActivity.test.tsx (3 tests) 19ms

 Test Files  1 passed (1)
      Tests  3 passed (3)
   Start at  01:19:06
   Duration  501ms (transform 97ms, setup 78ms, collect 112ms, tests 19ms, environment 160ms, prepare 25ms)
```
Count: 1 test file passed, 3 tests passed.

---

VERDICT: ALL_GATES_GREEN
