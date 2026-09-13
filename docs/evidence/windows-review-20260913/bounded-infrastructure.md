# Bounded infrastructure source receipts - 2026-09-13

All 14 assigned paths are dispositioned below. Source-only completion, not
Windows acceptance: no production defect newly established. BI-01 is a
deterministic source mismatch in an existing Windows-reachable test target's
included prototype implementation, not shipped history behavior. Historical
fixtures remain historical; no harness was repaired or executed.

## Authority, method and moving-source boundary

Read `coverage.md`, `findings.md`, `gap-packet-addendum.md` completely and all
14 matching `inventory-reconciled.json` ledger objects, including their prior
`scope`, `bounded`, disposition and hashes. Census JSON SHA256:
`9b54c1610d0e617e51ffa1d9a09c6a8e0729a1422dca59e3e9e9efbc2b96024b`.
Every object was `bounded-source`; this receipt supplements rather than
mutates that immutable census. The exact earlier ranges are recorded below.

Read root/backend/native-terminal AGENTS and shared programming/review-work
skills. This is a leaf source audit, not post-implementation QA: no worktree,
sub-review, runtime or language implementation gate was invoked. LSP definition
lookup at the Windows rasterizer call and references for History::search
returned no results; explicit module/include and textual call chains supplied
the evidence instead. No claim of successful LSP resolution is made.

Initial and pre-write `git status --short` and `git diff --stat` showed 20
foreign tracked modifications (371 insertions, 38 deletions), including App,
remote, worktree and state code, plus untracked evidence/mobile/rescan work.
All were preserved. Observed HEAD was
`ab08b94fdeda5039982fc8a37e8bc36885426667`, newer than the addendum's baseline.
Assigned-path hashes taken before reading and once after reading all matched
the census: zero observed drift among these 14 paths. Receipts bind those
bytes, not later concurrent edits or the entire checkout. Only this previously
absent report was written, with apply_patch. It is uncommitted.

## Exact assigned receipts

Ranges are inclusive. Full means all source lines read with the read tool.
CoreText omissions are deliberate implementation-body exclusions, not full
file claims; hashes still cover each complete file.

| Assigned path | Read range; current SHA256 | Reachability and disposition |
|---|---|---|
| `docs/evidence/windows-terminal-20260912/runtime/compositor-seam/green.rs` | Full 1-96; `8fa37e3676df4522cbfc81d66af392a061cd245bf0f5abf7aa16296704553f4b` | Portable extracted enum/validator and zero-field Windows target model; true descriptor flags satisfy its modeled Windows arm. No HWND creation or OS input. Historical descriptor evidence only. |
| `docs/evidence/windows-terminal-20260912/runtime/compositor-seam/red.rs` | Full 1-96; `2270793229a242320e2168fee11635ff25fcca58c378674d4b7c518506f576cf` | Same model, false flags; validator returns layer-backed error first. This is intentional old-source RED, not a current app regression. |
| `docs/evidence/windows-terminal-20260912/runtime/compositor-seam/run.mjs` | Full 1-73; `51f6274d833dbdc4241589711ca5e0b9b8a224427d3745f05dfa9e1c6ab1b5fe` | Bun/compiler driver reads fixed historical revisions, checks descriptor/validator equality, writes extraction/logs and invokes rustc plus extracted test. No Windows-specific runtime branch or HWND consumer. Historical only; do not execute for this closure. |
| `docs/session-continuation-20260908/integration/qa-integration-runner.ts` | Full 1-452; `8e5dcafda0b9e637808b54df83f7e2dad460cf1b9ebefecf7bfa918a3da774a4` | Absolute /Users paths, fixed port 5214, Node Vite and Bun.WebView; Windows backslash scenario is browser input against mocks. Historical validator limitations below; no shipped execution route established. |
| `docs/session-continuation-20260908/integration/qa-integration.tsx` | Full 1-309; `22a0ae87fc25cf6aa1118ec8401f88e0a711bc48f3a223e95d91b81ce4000615` | Windows/Linux host IDs choose synthetic home/separator strings; native attach/bounds/input and SSH registration are mocked. Real Sidebar/TerminalSplitView/AddProjectDialog imports do not turn the mocked backend into native Windows behavior. |
| `docs/session-continuation-20260908/ssh/reproducible-harness.tsx` | Full 1-47; `fc06f7afdf1191602210243bea55f6297073473eb36207bc92a5145818bf700b` | Windows default query/host selects synthetic C: home; Linux selects POSIX home. Denial, loading, truncation and registration are local invoke fixtures, not SSH or filesystem execution. |
| `docs/session-continuation-20260908/ssh/reproducible-runner.ts` | Full 1-353; `5a3d6f0b1c111102930a188ae8cc0bb25daeaf4dfb76935d1823a348583cc8c3` | Fixed Vite port 5213 and Bun.WebView drive Windows/Linux string scenarios. Sleeps/polling and weak teardown prohibit acceptance use. Historical only. |
| `scripts/macos-dev-runner.sh` | Full 1-101; `c29635f5a920dddda93a3273fa37f7e5a3bd666a11c35b37b0ee3ae9ef0ccb77` | Lines 4-6 exec cargo unchanged for non-Darwin OR non-run. Only Darwin run reaches bundle assembly/signing. Bash/uname are prerequisites if explicitly called on Windows; deliberate fallback, not missing Windows asset staging. |
| `scripts/qa/verify-omo-fork-confirmation.mjs` | Full 1-102; `8e8c92beada5a3c8b79fc8bcaf81bd79d14c869eb0eaf4c884580a49e252b7e4` | External Senpi module supplied by CLI, not Ferryx app. HOME/USERPROFILE and session files are under mkdtemp realpath; prompt-triggered terminal answers, bounded child timeout, awaited exit before removal. Bun terminal capability and external module behavior are unverified prerequisites, not a proved Ferryx Windows defect. |
| `scripts/sync-version.mjs` | Full 1-264; `c441179cfc883f922dbbb11063820698ded6963bd8d3f23bfa16aba38b91567a` | Shared native-path Node release utility, reachable from Windows release command. Strict tags, CalVer mapping, uint16 bounds, quad .0, both-input validation, sibling UUID temp files and surfaced rollback errors. No new Windows failure proved. |
| `src-tauri/src/ferryx_scope/control/boundary_tests.rs` | Full 1-53; `ea305916c3797c27e8e245dc89e50de69c10961b37f63d885a80b3a8290bb7d3` | Included by cfg(test) in control/mod.rs, itself path-included by tests/scoped_control.rs. NoLauncher is shared test scaffolding; sole real /bin/cat test is cfg(unix). Windows never executes that body. Missing Windows PTY case alone is not a product defect. |
| `src-tauri/src/ferryx_scope/history/hardening_tests.rs` | Full 1-53; `4682a962bac2d41bb4bea0f3e391733ff26252a761c2134b1e36008a950d3708` | tests/scoped_history.rs includes this module on Windows: Claude active branch and paging/corruption/deletion cases shared; symlink case 37-53 Unix-only. Owned temp paths and completed synchronous IO avoid timing assumptions. BI-01 below. |
| `src-tauri/src/native_terminal/renderer/coretext_font.rs` | 1-18, 745-755; `ca30f938c6b1058cc952125ddc1001dddb7536f01d6c8576bb665dd214764bd2` | macos module ancestor cfg at 3-4 excludes implementation 19-744 on Windows; 754-755 exports an empty non-Mac macos module. Actual fallback is FontManager's non-Mac storage/metrics and Windows GDI, not this empty module. |
| `src-tauri/src/native_terminal/renderer/coretext_raster.rs` | 1-18, 311-323; `f6fc37a9c2f07bb8344f92b9a6335add4a6be4d508502ef0dbddbb0ee865e64e` | macos ancestor cfg at 3-4 excludes implementation 19-310; 322-323 empty non-Mac module. Gated caller and real Windows alpha path proved below. No CoreText DLL dependency inferred on Windows. |

Prior census scopes: compositor trio already 1-96/1-73; integration UI
1-50 plus 293 and runner 326-355; SSH harness 1-47 and runner 1-110;
macOS runner 1-20; Senpi verifier 1-85; sync-version 90-98; control 1-30;
history 37-51; CoreText font 750-755 and raster 318-323. Thus earlier partial
test/script observations are not silently relabeled full-source coverage.

## Immediate consumer and fallback proof

- `lib.rs:10-11` enables native_terminal only with native-terminal feature;
  `native_terminal/mod.rs:29` includes renderer; renderer/mod.rs:5-8 includes
  both CoreText wrapper files and Windows-only directwrite_raster. Read these
  ancestor ranges and renderer/mod.rs fully (1-36).
- Read font_manager.rs:1-310: CoreText import, field, constructor, metrics and
  raster calls are macOS-gated (15-16, 20-23, 63-71, 119-136, 184-267).
  Windows 269-282 calls directwrite_raster::rasterize_to_alpha_buffer with
  selected family/text/size/style and returns Alpha at 300-304. Read that
  rasterizer fully (1-191): it actually links gdi32 and draws through
  CreateFontW/TextOutW into a DIB. The filename does not mean DirectWrite.
  rasterizer.rs:1-57 routes public calls into FontManager; color_glyph.rs:
  189-205 returns None on non-Mac, then alpha fallback remains available.
- These consumers preserve existing RF-01/02/03/04/08 dispositions; the
  empty CoreText modules are not an additional defect. RF-05 remains explicit
  unsupported Windows color-emoji capability, not an authorized redesign.
- Read package.json fully and tauri.macos.conf.json fully: package dev/build
  route cargo tauri; only Mac config names macos-dev-runner. COV-TOOL-1 remains
  refuted. TOOLING-GAP-03/P29 concerns its separate signing-test path fixture,
  not this script's intentional non-Darwin branch.
- Read release-local.mjs:140-174 and release-platforms.mjs:642-676: release
  plan validates CalVer and computes app/MSIX values; Windows command invokes
  Node sync-version --tag before build and passes plan.msixVersion into MSIX
  packaging. Read sync-version.test.mjs fully (1-494): owned manifests cover
  mapping, validation, rollback and AggregateError. No test ran; shared rename
  use alone does not prove Windows replacement failure. This does not reopen
  PKG-03 resource staging or conditional COV-BLD-1 GNU ABI policy.

## BI-01: existing scoped-history active-branch contract contradicts parser

Classification: source-confirmed test-target/prototype implementation defect,
Windows-reachable and platform-independent; not an observed test failure or
shipped product finding. No alias found in the existing ledger. Do not assign
a desktop repair packet or enable a new history feature from this observation.

Source chain (all read): tests/scoped_history.rs:1-7 path-includes history/mod.rs
and hardening_tests.rs. The ungated hardening test at 5-15 writes Claude rows
a (user), b (abandoned assistant child of a), c (active assistant child of a).
It asserts search for abandoned-sentinel has zero entries and read returns
only a,c. history/mod.rs:104-129 parses every text message into messages,
retaining parent_id but never selecting an active ancestry. search at 44-47
matches against all parsed messages; read at 56-60 returns all of them.
Therefore the first zero assertion instead receives one source entry; the
later message-ID assertion would also include b. No timing, native paths or
Windows availability assumption is needed for this contradiction.

Exact proposed regression invocation, after separately authorized isolated
build prerequisites (NOT executed here):

`cargo test --manifest-path src-tauri/Cargo.toml --test scoped_history hardening_tests::claude_active_branch_excludes_abandoned_sibling -- --exact --nocapture`

Retain the assertion and make the included parser respect the fixture's
active-branch contract; do not delete/skip the test. Real-surface condition
for this scope is the native Windows scoped_history test executable reading
its actual owned JSONL fixture through History::search/read: zero abandoned
search matches and exactly a,c returned, while paging/corruption cases remain
intact. There is no established desktop/IPC route: lib.rs module declarations
do not expose ferryx_scope, and repository source references to these history
and control modules are the named integration-test path includes. Thus no
Windows user-visible history failure or desktop runtime success is claimed.

The Unix-only symlink test also demands ROOT_SYMLINK_REJECTED while the read
prototype canonicalizes configured roots before walking (history/mod.rs:34-35).
This is an out-of-Windows test observation, not another Windows defect, and
is not permission to run Unix fixtures or weaken that assertion.

## Historical harness limitations, not new product packets

- Compositor extraction executes only an asserted descriptor model. It cannot
  establish native-input-01/P01 wheel delivery, cross-thread transparency,
  child lifetime, GPU presentation or current binary provenance. Red/green
  filenames do not represent this worker's RED/GREEN receipts.
- Integration runner starts Vite and awaits readiness/fetch before entering
  its cleanup try/finally (15-56, 92). Reset/collapse/reopen waits can resolve
  at an elapsed deadline rather than completion; final kill timeout resolves
  before guaranteed child reap, and cacheCleaned is hardcoded true after a
  swallowed removal error (392-452). These are historical validator defects,
  not accepted teardown evidence. The UI mock returns null for unrecognized
  commands and hardcodes presentation success, so backend failure cannot be
  inferred from its visual assertions.
- SSH runner sleeps for startup and state changes, polls selectors, closes
  the WebView only on the success path, and finally sends SIGTERM without
  awaiting child exit. Recorded allPass=false does not itself set nonzero exit
  status. These are concrete historical validation limitations, not reasons
  to execute or modernize this harness. No SSH request occurs in its mock.
- External Senpi verification is not Windows Ferryx launch/resume coverage;
  USERPROFILE set to an owned fixture does not prove Bun terminal support or
  correctness of an arbitrary imported external module. No external source
  was supplied for audit and no import was executed.

## Verification limits and additional byte bindings

No tests, build, runtime, desktop, SSH, production/test/config edits, branch,
worktree, ref, commit, PR or push actions. Branch/worktree permission remains
pending. Source receipts do not close existing defects, unsupported capability
decisions, aliases, refutations or the lead's final foreign-diff qualification.
Lead must independently verify this report; no worker statement is runtime
success. Markdown diagnostics were requested but no .md LSP server is
configured; no configuration changed and no prose test was added. Report
readback and assigned receipt/hash reconciliation were completed.

Additional chain SHA256 receipts (ranges above, not full-file claims unless
explicitly stated):

| File | SHA256 |
|---|---|
| `src-tauri/tests/scoped_history.rs` (full 1-7) | `2e7a06206e5ab4a6070bc5494f09cf7eb63622586133abca66542274686ae974` |
| `src-tauri/src/ferryx_scope/history/mod.rs` (full 1-145) | `f8fc70d54815e168e4256ec2b2f302d53131b5a526f49fe8a88983047765cf0e` |
| `src-tauri/tests/scoped_control.rs` (full 1-35) | `f0ae2a6d81e150513b7287b265540c045de23b41d5e201926406048976513d89` |
| `src-tauri/src/native_terminal/renderer/mod.rs` | `fb3a4585c9b2913b1196d8891e6fd3e46654bc15ba434b450ce6e4f831303cc9` |
| `src-tauri/src/native_terminal/renderer/font_manager.rs` | `fef59d35cb816e395aeaf7ee0b57433e153bc5bb6598e1c1802653293aa34baa` |
| `src-tauri/src/native_terminal/renderer/directwrite_raster.rs` | `b265318717ab6595e2310fcf7ccdc8cf16656626bc47b9668d5834f2d638f810` |
