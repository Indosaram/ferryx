# Rendering review and repair loop

## Outcome and scope

Resolve confirmed intermittent native-terminal rendering defects with regression
and real-surface evidence. Review current working-tree code, including foreign
uncommitted atlas fixes, without changing their ownership.

Loop state and append-only notepad:
`.omo/ulw-loop/rendering-review-20260906/ledger.jsonl`.

HEAVY: the user requested review; renderer cache invalidation and surface
lifecycle concurrency are in scope. Font sharpness is not the remaining symptom
according to `reference/project/ferryx-pane-rendering-review-scope.md`.

## Skills and delegation

- `mass-ulw`: one dependency-ordered workflow per phase.
- `ulw-loop`: goals, criterion evidence, checkpoints and durable continuation.
- `bun-1-4`: orchestration and process execution from the Bun kernel.
- `coding-agent-sessions`: recover the September 5 rendering review.
- `git-master`: preserve foreign work and commit verified owned increments.
- `debugging`: workers distinguish source hypotheses from reproductions.
- `programming`, `ast-grep`: workers use language contracts, LSP and structural
  searches for code review and fixes.
- `frontend`, `impeccable`: frontend worker audits lifecycle and geometry without
  redesigning application chrome.
- `visual-qa`, `computer-use`: exact debug-app scenarios and screenshot evidence.
- `review-work`: final evidence review when its gate applies; initial source
  diagnosis is not an approval of completed work.
- `memory-discipline`: retain durable verified findings, not transient progress.

Topology: three parallel diagnosis lanes followed by one verification/synthesis
lane. Atlas and native host lanes use `deep` for GPU/backend reasoning; frontend
lifecycle uses `deep` because asynchronous session ownership, not styling, is
the task. Synthesis uses `deep` to falsify cross-lane race and rendering claims.
All lanes write separate reports only. No team is needed: diagnosis read scopes
may overlap, but write scopes do not. The lead owns scope decisions, CLI ledger,
finding acceptance, integration and final surface verification.

## Ordered plan

1. Review atlas and row-cache frame coherence; write `atlas.md` with exact
   references, executable reproductions and limitations.
2. Review native surface scheduling, ownership and presentation; write
   `surface.md` with exact references, reproductions and limitations.
3. Review frontend pane lifecycle, geometry and visibility; write `frontend.md`
   with exact references, reproductions and limitations.
4. Recheck all three reports against source and callers; write `review.md` with
   confirmed defects, rejected hypotheses and per-defect RED/surface scenarios.
5. Register one atomic defect task and criterion per accepted finding before
   production changes. Create an isolated worktree for each repair phase;
   account explicitly for foreign uncommitted baseline changes.
6. Capture right-reason RED, apply the smallest owned fix, capture GREEN and
   affected entry-point evidence. Record cleanup and commit each verified unit.
7. Run real debug desktop scenarios for glyph pressure/density and
   split/move/resize/tab/overlay transitions. Refine concrete actions before
   execution and capture screenshots. Tests alone do not close a scenario.
8. Run affected diagnostics, tests and builds. Resolve failures caused by this
   work; explain unrelated pre-existing failures without expanding scope.
9. Audit the final diff and every criterion's evidence; integrate only with
   required approval. Persist final review, commit list and cleanup receipts.
   Complete the aggregate only after observable rendering criteria pass.

## Success criteria

- C1: Each of the three source-review reports exists; dependent synthesis
  rechecks the findings using source/caller references and classifies each as
  confirmed, refuted or unproven. Prose review needs QA-by-read, not text tests.
- C2: Each confirmed rendering defect has its exact failing-first test/scenario
  captured before implementation and the same proof GREEN after. Dense mixed
  Korean/ASCII content stays correct through repaint and 1x/2x density changes
  on the debug app started only by `bun tauri dev`.
- C3: Distinct session content remains correct through split, resize,
  pane-to-tab move, tab switch and overlay visibility. No blank visible surface
  or hidden surface over chrome; PTYs survive. Screenshots, action logs and
  resource cleanup are required.

Supporting commands:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal
bun run --cwd ui test -- src/components/NativeTerminalPane.test.tsx src/components/NativeTerminalPane.lifecycle.test.tsx src/lib/nativeTerminalLifecycle.test.ts
cargo check --manifest-path src-tauri/Cargo.toml
bun run --cwd ui build
```

## Initial evidence

- Shared tree contains foreign edits in atlas.rs, row_cache.rs, Cargo.toml,
  vendor/wgpu-hal, macOS dev runner, onboarding and supporting docs/tests.
- Existing atlas patch defers overflow invalidation to the next frame, grows
  the texture to 1024 and skips empty masks. Its remaining behavior must be
  reviewed, not assumed correct from the previous 111-test report.
- Previous review session:
  `01a07093-abde-7b1f-b61f-ff34447cb572`.
- Current source contains `RenderScheduleCoordinator`, ownership guards and
  lifecycle tests. Recheck these guards before alleging a race already fixed.

## Stop condition

Stop when every confirmed in-scope defect has a verified fix and all registered
regressions and real-surface scenarios pass with artifacts and cleanup receipts.
Missing runtime access or conflicting foreign edits remain explicit blockers,
not a basis for declaring completion.

## Execution additions

- Run independent baseline QA while diagnosis is active. The baseline worker
  writes `baseline.md`, captures the four supporting commands above, and records
  HEAD plus the working diff before and after. It edits no source or tests.
- Baseline revision observed by the lead:
  `b8f82d707f0cb99907e3d79c0c9cdc75053ef931` on `main`.
- Current criterion evidence directory:
  `.omo/evidence/ulw/rendering-review-20260906/G001-review-and-resolve-ferryx-intermitte/a1`.
- Before finishing, stop the lead's one-shot foreign-atlas modification monitor
  (`mon_XV5PZ1ERHS51FPGB`, `watch_1`) if it has not already fired. Do not stop
  any pre-existing Ferryx, cargo-tauri or user terminal process.
- Native QA preflight currently fails: the selected `orca` CLI reports
  `runtime_unavailable` because its app/runtime metadata is absent. This is not
  a rendering failure and not evidence of rendering success. Source and baseline
  work continue independently; native screenshot scenarios remain unresolved.
