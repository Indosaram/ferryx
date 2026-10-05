# Ultrawork Notepad — Task 9 WINDOWS half: three native desktop scenarios at 70eefafe
Started: 2026-10-04 (local)

Tier: HEAVY. Justification: the deliverable is an acceptance verdict on native desktop behaviour
(session-handling / native-terminal surface) that feeds the plan's Task 9 acceptance row and the
F1-F4 final wave; the user demanded a strict, multi-lane verification contract (independent image
reader, named producers, per-scenario PASS/FAIL/NOT_RUN, teardown receipts). A wrong verdict here
propagates into acceptance, so the evidence bar is set by the criterion, not by the size of my edits.

Skills considered:
- No skill body is required by this task's own workflow (it is a scripted remote verification run,
  not a coding task). `debugging` (hypothesis-driven loop) is the closest fit and is applied
  manually: each claim below is a hypothesis with a discriminating probe. `review-work` is NOT
  used (it is for post-implementation gate review of my own work; the user's own contract here is
  stricter and is followed instead).
- Memory discipline: every durable finding is being written to memory as it emerges.

## Plan (exhaustively detailed)
1. Recon (DONE): read the 4 briefs; read the runner + harness + driver + split-scenario adapters;
   read the Task 8 verdict; inventory maho-win staging; map the product's QA producer surface.
2. Stage candidate 70eefafe on maho-win: fresh owned source root, full-source transfer, ghostty
   junction, `bun install --cwd ui --frozen-lockfile`; record host-side sha256 of the tracked
   source files and the tar hash.
3. mtime trap: after extraction `touch` every extracted source file (or delete stale binaries) and
   PROVE the built binary is 70eefafe by `strings` on the daemon/app binary for the adoption-path
   fix strings (`adopt_transferred_ownership`, `Predecessor incarnation does not match export`).
4. Build the Windows scenario binary with the QA-feature delta
   (`cargo build --features local-split-qa`), record exact command, raw exit, binary sha256.
5. Run each of split-happy / split-attach-stall / split-cancel with the exact argv; capture raw
   exit, asserted line, actions.jsonl, result.json, cleanup.json, every producer receipt.
6. Screenshot lane: for each owned-window capture, hand screenshot.png to a multimodal-looker child;
   record its verdict; a crop/wrong-pane/unreadable marker fails the scenario.
7. Cross-check every cited artifact against its named producer; check host load before believing any
   timeout; teardown every spawned resource with a receipt.
8. Verdict table + mac-half remainder list + evidence bundle under task-9/win.

## Success criteria + QA scenarios
(see goal registration)

## Now
Bootstrap: goal registration, then host inventory via .ps1-over-scp (bash->ssh->powershell inline
quoting is unreliable; switched to file-based invocation).

## Todo
1. Host inventory: existing trees, target/ caches, free disk, uptime/load, interactive session.
2. Stage 70eefafe on maho-win; record source hashes.
3. Build with local-split-qa; prove provenance by strings.
4. Run split-happy / split-attach-stall / split-cancel exact argv.
5. Image-reader lane per capture.
6. Producer cross-check + teardown receipts.
7. Verdict table + mac remainder + bundle.

## Findings
- F1 (decisive): the candidate's QA barrier producer exists ONLY for the headless path
  `ferryx diagnostic-classifier --headless` (src-tauri/src/main.rs:28 ->
  ipc/qa_barrier.rs:run_diagnostic_classifier_headless -> emit_fixture_setup at :801).
  `QaBarrierChannel::from_env()` and `install()` have NO production call site outside that
  headless entry (only `#[cfg(test)]` modules in native_terminal/surface_host.rs:9706,9823 and
  ipc/native_terminal.rs:3640,3765). Therefore the GUI binary spawned by the runner
  (`spawnOwned(ctx.binary, [])`) never installs the channel and never settles `fixture-setup`.
- F2 (decisive): receipt names `split-create`, `marker-output`, `cancel-ack`, `attach-handshake`
  have ZERO emit sites anywhere in src-tauri (repo-wide grep for the literal names). Only
  `fixture-setup` (qa_barrier.rs:582) and `presentation` (qa_barrier.rs:37 constant, plus
  surface_host.rs producers) exist, and both are reachable only with an installed channel.
- F3: the runner's FIRST product interaction for every scenario is
  `awaitReceipt('fixture-setup', 0, 9000)` (pane-liveness.mjs runNativeScenario). With no producer
  the outcome is BARRIER_ACK_TIMEOUT -> verdict BLOCKED -> EXIT.barrierUnsupported = 7.
- F4: the split trigger cannot match the product either. `SPLIT_MENU_SELECTOR_WIN32 =
  { automationId: null, name: 'Split Right' }` (native-driver.mjs:20) and `windowsDriver` searches
  UIA descendants of the main window for Name == "Split Right". The literal string "Split Right"
  does not exist anywhere in the product; the real affordances are the React button
  `Split pane right` (ui/src/components/TerminalSplitView.tsx:1281) and the native popup entry
  `Split terminal right` (ui/src/components/TabBar.tsx:307). Plan line 196 already rules on this:
  "Native driver uses actual observed unique enabled Split Right selector; mismatch fails and is
  repaired narrowly, never guessed clicked."
- F5: Task 8 never executed scripts/qa/pane-liveness.mjs scenarios on ANY host; only the runner's
  Vitest unit suite (28/28 x3). So no prior scenario artifact exists to reuse.
- F6: Task 8 Windows staging mechanics (validated): root
  `C:\Users\sook\ferryx-pane-completion\<tree>`, `tar.exe -xf <archive> -C <root>`,
  `mklink /J <root>\src-tauri\vendor\ghostty C:\Users\sook\task2-ghostty-6a508fd5`,
  `bun install --cwd ui --frozen-lockfile`. Ghostty submodule pin: 6a508fd5e34c7e222c052a6d00bb3891ff3feace.
  Windows toolchain: rustc 1.97.0, cargo 1.97.0, Bun 1.4.0, Node v24.19.0.
- F7: candidate worktree is clean at 70eefafee2be8b24771ae424fd50b62a8f75cc7a on
  work/local-pane-liveness-completion-foundation. `.omo/` is not tracked, so evidence written
  under C/.omo/evidence/ keeps the tree clean.

## Learnings
- bash -> ssh -> `powershell -Command "<inline>"` mangles quoting ($_, quotes, pipes). Use a
  local .ps1 + scp + `powershell -NoProfile -File` (the brief recommends exactly this).
- The runner's marker lane is an EXTERNAL handshake: it writes <barrierDir>/capture-ready.json then
  blocks up to BUDGETS.stagePresentationMs (2000 ms) for an independent
  <barrierDir>/marker-recognition.json bound to runId/operationId/recognizer/text/paneBounds/sha256.
  The image-reader lane must be armed and fast, or the scenario dies with MARKER_RECOGNITION_UNVERIFIED.
