# Windows tooling: exact eight-path source closure

2026-09-13; child st_01a0988b. **8/8 assigned paths have final bounded source dispositions. Source review is not runtime success.** Four source-confirmed tooling findings below are proposals for lead verification, not implemented repairs. Isolation approval remains pending.

## Snapshot and limits

Lead adjudication: all eight reported hashes match current bytes; direct
reads confirm the four mechanisms. TOOLING-GAP-01/02 are defects in the
legacy Homebrew-bound mock, which is not a Windows acceptance harness and
will not be executed or credited by this task. Retain these observations
without expanding Windows remediation into rehabilitating that mock.
TOOLING-GAP-03/04 are Windows-reachable test portability repairs, pending
their registered RED/GREEN and artifact checks. No runtime success follows
from this adjudication.

- Observed HEAD: `ab08b94fdeda5039982fc8a37e8bc36885426667`. `git ls-files -s -z | shasum -a 256` gives the lead's `b5c0ab6db7f9e928b5b5133a4adb066b5d7baa88f2745ffec3e80bd963e926c8`; 1,854 indexed entries. The newline-form listing has a different hash (`0abb3b2c...`); that is serialization, not index drift.
- All eight complete named files were read. Their working-byte SHA256 values equal the inventory ledger. The scoped `b7ad4516..ab08b94f` diff is empty for all eight. The full name-status diff confirms that **none of this assignment is a newly committed image path**: `script/qa/terminal-image-paste-scenario.mjs` is the old, singular-script mock harness, not new `scripts/terminal-image-probe.mjs`. New native image files belong to the separately assigned image closure; no repeat image/renderer audit is claimed here.
- Necessary caller/dependency reads: root/site package manifests; `scripts/macos-dev-runner.sh` in full; `scripts/sync-version.mjs` in full; `ui/src/lib/terminalThroughput.bench.ts` in full; `bench/terminal/workloads.ts:250-304`; site Astro configuration in full; CI workflow through site-test selection. Prior inventory/findings/gap-verification/addendum supply settled-domain context, not a fresh broad audit.
- Only this report was created, using apply_patch. No project scripts, tests, builds, desktop/browser/daemon/remote operations, source/test edits, Git refs, worktrees or commits were executed. Bun snippets only read/hash/compare inventory and source bytes. No runtime, typecheck, render or cleanup success is inferred.

## Exact path ledger

Hashes are SHA256 of working bytes; coordinates below refer to these snapshots. All eight paths are unchanged from HEAD and from the assigned inventory. Each row supersedes only its `pending-bounded` source disposition, not runtime gates or the JSON inventory itself.

| Path | Lines | SHA256 | Final source disposition |
|---|---:|---|---|
| `bench/terminal/runner.ts` | 523 | `6cb931246fbac1d065cfec80f56f20752e9318150527a00bcc77f947b95fbdad` | Reviewed shared synthetic/parser benchmark; not a Windows desktop validator. No new Windows defect assigned. |
| `script/qa/terminal-image-paste-scenario.mjs` | 353 | `43d6bd82fc14d046a1072de7d6f91968836adc6be07192f299c1a7df671b0324` | Reviewed host-bound mock harness; unsuitable for current Windows/Ferryx acceptance; TOOLING-GAP-01/02. |
| `scripts/macos-dev-runner.test.mjs` | 58 | `0dd0beda11bcaad4013c98788edda9f61f9191a79ed42998939a18bbb2a3503a` | Reviewed signing fixture; native-Windows fixture portability defect TOOLING-GAP-03, not production runner fallback defect. |
| `scripts/sync-version.test.mjs` | 494 | `b65265090edc022840e9f64d6912b5720146764342f12a995ec0b27fc4f51a27` | Reviewed portable, owned manifest fixtures and actual replacement-failure assertions; no new Windows defect. |
| `scripts/terminal-render-probe.sh` | 123 | `aeb7da415cba2895501fbc25e3f48c4ac0510fc2ae01e9bb3d9cdd5ac55f9413` | Reviewed POSIX-shell byte emitter; explicit prerequisite, no native Windows/render success claim. |
| `site/src/components/Features.tsx` | 114 | `7955b0ebba0cb399d5f9e3042b8c35a559faabdbefd38bcb66cb3a3c5b5186a5` | Reviewed presentation and base-path link; platform wording is not an executable OS branch. |
| `site/src/pages/index.astro` | 124 | `f08c80a56b43e5a10fff40f8e1753c5c583af1036ed9fbd5912a277101220927` | Reviewed site entry, structured metadata, theme/hydration branches; no Windows implementation defect. |
| `site/src/seo.test.ts` | 442 | `ae58ba83ea664f55ccf9509027b4d9d8b22b7ba43a482002874660619b8f965a` | Reviewed real-build validator; native Windows route-key mismatch TOOLING-GAP-04. |

## Per-path mechanisms, prerequisites and cleanup

### 1. Benchmark runner

`import.meta.main` at 514-523 calls exported `executeBaselineSuite` (287 onward); there is no root package runner alias, and the bounded bench caller search found only that self-entry. `runWrappedThroughputBenchmark` (36-97) executes Bun with an argv array, checks child status, then parses machine-generated numeric tables. Its actual child computes matching direct/legacy checksums and throws on mismatch. Paths use node:path, and OS at 390 is report metadata, not a platform branch. Bun and resolvable `@xterm/xterm` plus the child file are prerequisites; the root manifest does not itself declare xterm. A dependency-resolution failure would be blocked execution, not Windows behavioral RED.

Parser writes await each xterm completion callback (100-105); multipane waits round promises (181-203), not fixed sleeps. Successful runs dispose terminals. Rejection lacks a try/finally disposal guarantee; callback waits and child spawn have no bounded timeout. It writes/overwrites three files in `bench/terminal/evidence` (417-427), with no rollback/removal. Therefore this is not a safe no-write inspection command and was not run. `assertHonestMeasurementSurface` checks surface labels, not native/PTTY/renderer parity. Its summary explicitly leaves real desktop input/visual scenarios unmeasured.

Bounded non-Windows observation: multipane `runChecksum` is only accumulated for r=0, but `finalChecksum = runChecksum` runs for every iteration (178-206), so default three-run output ends at zero. This synthetic report bookkeeping does not establish a Windows defect; no benchmark repair packet or new Windows regression is proposed. Likewise fallback xterm version metadata is not proof of installed version. Lexical regex/newline/OS matches do not justify turning this benchmark into a desktop test.

### 2. Old image-paste scenario

Unconditional `main()` (350) and top-level `node-pty` require mean **importing is not a safe inspection seam**. Main resolves UI xterm bundle/CSS, validates only the PNG signature (84-99), makes an evidence directory, spawns the same executable in a real PTY, then starts an ephemeral loopback HTTP server and external agent-browser for transcript replay. No Ferryx DOM paste event, IPC, native terminal binary or clipboard API is called anywhere in this file. The child reads the fixture directly through `FIXTURE_PNG_PATH`; emitting SYN itself is not Ferryx transport verification.

Readiness is data-event driven before the trigger write; normal completion awaits PTY exit, with a ten-second failure timer. Missing-trigger mode kills on timeout; wrong-trigger sends 0x15 and child exits 1. Browser readiness instead polls every 50ms (75-82,284-288), which is existing nondeterministic fixture debt, not accepted evidence. Hardcoded `/opt/homebrew/bin/agent-browser` (15), local node-pty ABI, PNG fixture and xterm dependencies are prerequisites, not supplied Windows support. The limitation text acknowledges a mock, but metadata `scenario` and limitation's claim to prove Ferryx transport exceed the actual call graph. Do not change paste policy under native-input-09/WIN-UI-04 to satisfy this mock.

The fixed browser session and finally behavior are TOOLING-GAP-01; the inactive hash fault is TOOLING-GAP-02. Output artifacts remain on disk intentionally, and can be overwritten; they are not owned temporary roots by default.

### 3. macOS signing test

The two Bun tests execute the **real copied** shell runner in mkdtemp roots (6-53). They fake uname=Darwin, cargo, codesign and the launched executable with shell scripts; seed plist/icon; assert actual signing argv identity, exact 0/7 exit and launch iff signing succeeded. This is meaningful behavior, not signature prose pinning or real signing. Both pipes and exit are awaited together; finally removes the owned root (54-56). The 30-second framework timeout is a failure bound, not sleep-based synchronization, but there is no explicit child kill/reap guard if that bound fires.

The runner's production non-Darwin/non-run branch (4-5) remains `exec cargo "$@"`, consistent with the settled **COV-TOOL-1 refutation**. This test deliberately takes the Darwin branch on every host; absence of Windows fallback coverage alone is not a bug. Its incompatible fixture construction is TOOLING-GAP-03. Existing Bash/POSIX tools must be provisioned before any future execution; do not install them or invoke a real signer/cargo as a test fallback.

### 4. Version synchronization test

`runScript` (43-45) uses process.execPath plus actual script argv, not a shell command; every successful/malformed-tag CLI case supplies owned conf/cargo paths. Missing-tag CLI returns usage before manifest access in production `main` (224-228). Dynamic imports reach a guarded CLI entry (`sync-version.mjs:244-264`), so API tests do not run main. Callers in release-local/release-contract import these exports; release-platforms contains native node invocations, including the PowerShell command at 661. Those release operations were not executed or reopened as a domain audit.

Fixtures cover exact CalVer/MSIX numeric values, dependency preservation, order/newline/idempotence, invalid dates and bounds, dry-run bytes, no temporary leftovers and second-rename rollback. Failure injection patches the fs object production really uses, allows the first real replacement, verifies its new version before throwing EIO, then checks original bytes restored (375-419). AggregateError case preserves EIO/EACCES identity. `mock.reset` and `syncBuiltinESMExports` run in finally; roots are removed. No shared repository manifest mutation or sleep/polling is required. Node's node:test/mock API is the intended prerequisite, not a claim that arbitrary Bun compatibility is green. Existing error-regex assertions are diagnostic-text debt; add no prose-pinning tests. The year>65535 case is rejected earlier by the four-digit format, so it does not prove that internal numeric branch was exercised. This bounded coverage limitation does not prove an invalid Windows version escapes. Native filesystem replacement/ACL behavior remains unexecuted.

### 5. Render probe

The direct `#!/bin/sh` entry uses set -eu, shell printf, arithmetic and finite loops only: attributes (14-24), palette ramps (29-63), truecolor (65-89), three 200-column lines (92-123). No spawn of an app/daemon, network, filesystem output, environment mutation, raw input or alternate-screen mode. Every styled fragment resets SGR on the normal path. No tests or screenshot/assertion/cleanup receipt is present: a successful emitter exit cannot prove hidden text remains hidden, glyphs render, or wrap is correct. A POSIX shell (e.g. already-authorized Git Bash/WSL) is required on Windows; native PowerShell/cmd interpretation is not promised. The lexical `ramp:\n` matches are not Windows path branches. No extra feature implementation follows from this prerequisite.

### 6-7. Site component and entry

`index.astro` imports Features (5) and mounts it `client:visible` (117); Features maps static descriptions/visual components, alternates layout with index parity and emits the Architecture link only for that data row (75-109). BASE_URL trailing-slash normalization (Features10/index12) supplies deployment-relative hrefs. Its WebView2/WebKit/macOS-first sentence (46) is presentation, not OS detection, installer selection or daemon startup. Reuse **GAP-UI-08's illustrative-site distinction**; this is not an additional variant of that alleged product defect and no prose test is warranted.

The `Windows` string in index26 is SoftwareApplication.operatingSystem metadata. It does not dispatch native capabilities or certify parity. Canonical/social URLs resolve against Astro.site/Astro.url (20-21); JSON-LD serializes structuredData (74). Theme bootstrap chooses stored light/dark or media preference (37-44). Navbar/Hero load early; Features/Benchmarks/Footer are visibility hydrated; demo is client-only with an explicitly mocked backend. Astro config aliases native Tauri calls to site mocks. Site build dependencies and browser localStorage/media-query access are prerequisites; this path does not own desktop processes needing cleanup. Font URLs are external browser assets, not a source-review network action. Actual browser rendering/theme/storage behavior remains unverified, with no source-proven Windows-specific defect here.

### 8. SEO build tests

CI ui-check is Ubuntu and calls `bun test --cwd site` (workflow17-45), not Windows. The suite itself is platform-ungated. BeforeAll spawns actual `bun run build` in site with explicit origin/base env, drains both pipes and rejects nonzero (18-39). There is no stale-dist fallback. Root-origin case runs another actual build into `site/node_modules/.cache/seo-root-dist` (422-441). Both have 300-second framework bounds; neither explicitly kills the child on timeout, removes build directories nor uses a private checkout. Future runs must use an approved isolated workspace; never run this as a read-only test in the shared tree.

Assertions inspect production HTML, JSON-LD nodes, resolved files, image dimensions/sizes, required page containment, links/counts and deployment URLs. They are generally meaningful shipped-artifact assertions, not mere source greps. The macOS array containment at 206 asserts only that member, not Windows support. Builds succeeding on Ubuntu do not validate host filesystem-to-route conversion; TOOLING-GAP-04 explains the actual Windows failure. No new fixed prose equality, skips or optional-prerequisite PASS should be introduced.

## Confirmed findings and exact future regression contracts

These IDs are unique within this closure. GT/P aliases below are explicit relations, not invitations to count an existing defect twice. Commands are **future proposals, none run**. Proposed new `.test.mjs` files do not exist by this report's action and must be allocated before implementation. Import-safe execution seams and owned process/browser roots must exist before exercising the old image script. All tests must run nonzero cases, fail on missing prerequisites rather than report PASS, register events before triggers and use bounded failure deadlines only. No sleeps/polling or skipped tests constitute GREEN.

### TOOLING-GAP-01: image harness can close an unowned browser session and conceal failed cleanup

Source: image script212,281,329-346. `oldBrowserSession` is saved, but the fixed session name is assigned only after PTY assertions/transcript writes. Any exception inside try before line281 still invokes agent-browser close in finally using the caller's inherited session (or default session). After line281 it instead reuses a global fixed name, which may already belong to another run. Close errors are swallowed, successful main already prints PASS, and failed metadata update is swallowed too. Thus ownership/cleanup cannot be certified even on its intended Homebrew host. Server closure is awaited when listening and env restoration exists, but neither establishes browser ownership.

Alias: same harness-safety **family** as GT-01/P25, **new path and distinct mechanism**, not DS-10/default-daemon termination and not automatically within P25's ownership.

Future exact invocation: `bun test script/qa/terminal-image-paste-scenario.test.mjs -t 'TOOLING-GAP-01'` after allocating an import-safe command/event recorder seam. Register failing PTY completion before trigger, inherit a sentinel browser session, and independently fail browser close. **RED:** any close addressed to sentinel/default/unacquired session, or success after owned close failure. **GREEN:** zero unowned browser commands, unique owned session only after acquisition, env restored on both paths, failure surfaced and artifacts retained on cleanup failure. Binary acceptance after separate approval: same instrumented CLI with owned PTY/server/browser, sentinel session remains intact, all owned children emit exit/close before a successful result. Missing agent-browser is INCOMPLETE, not the intended RED.

### TOOLING-GAP-02: wrong-fixture-hash negative control cannot cause its advertised mismatch

Source: image script157 sets `FIXTURE_SHA256` to zeros for `wrong-fixture-hash`; embedded child116-142 never reads that variable, recomputes the hash from the same file as the parent, and main249 compares against unchanged fixtureMeta.sha256. With a valid stable fixture and other prerequisites successful, the negative mode follows the same passing checks as normal mode. Source proves a dead fault injection, not observed Windows clipboard/image corruption.

Alias: **none** in findings/gap-verification/addendum; not native-input-09, RF-05 or the new native image renderer.

Future exact invocation: `bun test script/qa/terminal-image-paste-scenario.test.mjs -t 'TOOLING-GAP-02'`. Preserve real hash computation and PTY receipt validation; only browser rendering may be replaced by an owned recorder. **RED:** deliberately mismatched expected digest is accepted or reaches screenshot/PASS. **GREEN:** same stable valid PNG succeeds normally; mismatched expected digest produces nonzero outcome before screenshot and still completes owned cleanup. Binary acceptance: separately authorized CLI `bun script/qa/terminal-image-paste-scenario.mjs --simulate-red wrong-fixture-hash` on an explicitly owned configured fixture/session returns nonzero for digest mismatch, while the identical fixture normal command returns zero with matching machine-consumed digest. This verifies the mock only; no Ferryx IPC/DOM claim is permitted. Missing fixture/dependency/browser is blocked, not mismatch RED. Replace existing readiness polling with exact readiness signaling before such acceptance.

### TOOLING-GAP-03: signing fixture mixes native Windows paths with POSIX environment/target parsing

Source: macos test8-11 uses native `join`; line38 strips only `/debug`, so native Windows `...\\debug` remains and runner37 appends another `/debug`. The executable/plist/icon are seeded in the single-debug location; real runner cannot find that target executable. Line37 constructs PATH with `:` despite native Windows delimiter/drive letters, making fake tool interception unreliable across native Bun/Bash namespaces. POSIX executable mode assertion (7) also cannot certify executable-bit semantics on Windows. No platform gate prevents these fixtures from being discovered there. Installing bash alone does not repair the path contract.

Alias: **not COV-TOOL-1** (production fallback remains refuted); related fixture portability family DS-09/FSSH-T01 but new JS path/owner.

Future exact invocation on an already provisioned native Windows Bun + approved Bash environment: `bun test scripts/macos-dev-runner.test.mjs`. Retain both 0/7 cases and copied production shell, adding a transport recorder that prevents ambient cargo/codesign fallback. **RED:** wrong double-debug lookup or inability to reach owned mock tools; **GREEN:** single owned target, exact fake signing argv, exit0 launches only the fixture and exit7 never launches, zero real signer/build calls, all owned child exits before root removal. Also run the identical command on macOS to preserve intended signing behavior. No skipping the Windows cases, no real Developer ID prerequisite and no production asset-staging repair implied.

### TOOLING-GAP-04: native Windows filesystem keys are compared with URL-path literals

Source: SEO test43-61 builds `full` with native `path.join`, then stores `pages[full.slice(DIST.length)]` (52). Windows yields `\\index.html` and `\\docs\\...`, whereas REQUIRED_PAGES (64-81) and downstream `pages[route]` (262) require `/index.html` and `/docs/...`. `builtPages()` is called by multiple ordinary tests, so a successful Windows Astro build still fails required containment before those content checks. This is host path serialization, not an actual broken published website URL.

Alias: **none** in prior accepted ledgers; not COV-CI-1 (current Ubuntu site job is separate from missing Windows Rust tests).

Future exact invocation on native Windows in approved isolated workspace: `bun test --cwd site src/seo.test.ts`. Keep actual build and required-page/content assertions, add a deterministic win32/POSIX path projection case through the production route-key helper if extracted. **RED:** real built page is absent under required slash key. **GREEN:** required route keys use URL separators on either host; all required pages and subsequent metadata/link assertions still execute and pass; a deliberately removed required page still fails. Exit zero/zero tests or missing build deps is not GREEN. Binary/artifact acceptance: the same Windows-generated fresh dist maps all required files to slash routes and resolves links under both `/ferryx/` and root origin; preserve failing build exit as failure, not stale-artifact success. No desktop launch is needed or claimed.

## Foreign drift for lead frozen-diff verification

At the bounded comparison with inventory working SHA256, six tracked files differ: `src-tauri/src/remote/tests.rs`, `ui/src/App.tsx`, `ui/src/remote/MobileHostDrawer.tsx`, `ui/src/remote/RemoteApp.tsx`, `ui/src/remote/RemoteSessionList.tsx`, `ui/src/remote/RemoteUI.test.tsx`. Inventory postCensusDrift already lists App and RemoteApp; the other four are newly reported relative to that annotation. These are foreign bytes, not read/qualified behavior by this tooling closure. Initial/final pre-write status also includes foreign remote protocol/server/lib/worktree/UI/state changes and untracked remote/rescan/mobile evidence; none was touched. All eight assigned source hashes remained equal to the inventory at the pre-write check. No subsequent concurrent edits were chased.

The lead must independently reopen these exact mechanisms and validate report membership/hashes before merging source dispositions. This report is uncommitted in the shared tree. Closure is limited to eight supported source receipts; source repairs, regression execution, Windows binaries and runtime acceptance remain pending.
