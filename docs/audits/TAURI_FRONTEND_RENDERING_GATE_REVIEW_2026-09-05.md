# Final gate review: Tauri frontend rendering

## recommendation

**APPROVE**

No specific failure of the requested rendering-fix outcome was found in the bounded session changes. The declared native compositor pixel boundary remains unverified by design and is not a blocker under the supplied criteria.

## blockers

None.

## originalIntent

Find, review, and fix Tauri desktop frontend rendering defects, specifically the session-owned changes in `App.tsx`, `BrowserPane.tsx`, `TerminalSearchOverlay.tsx`, `lib/terminalSettings.ts`, `lib/nativeTerminalVisibility.tsx`, `state/workspaceStore.ts`, the density-listener block in `NativeTerminalPane.tsx`, and their associated tests. Concurrent TabBar, TerminalSplitView, drag types, shortcuts, RemoteTerminal, Rust, packaging, and unrelated NativeTerminalPane input/IME work are foreign and excluded.

## desiredOutcome

- Native browser/terminal surfaces yield to real dialogs/search surfaces and restore according to owner visibility.
- Late browser and terminal search responses cannot overwrite current state.
- Terminal preference consumers share startup fetch/cache state, preserve the startup background, and reject stale refresh results.
- Worktree activity recomputes after session rebinding.
- Unchanged focused-terminal payloads are not republished and settings callbacks remain stable.
- Density-only changes update native terminal scale without reattaching.
- Automated and isolated real-WebKit evidence supports these outcomes; physical native compositor pixels remain explicitly manual.

## userOutcomeReview

The inspected production changes implement the requested behaviors at their real state/effect boundaries:

- `ui/src/App.tsx:1042-1053` compares serialized focused-terminal payload content before IPC; `ui/src/App.tsx:1779` and `:1838` pass the stable callback directly.
- `ui/src/components/BrowserPane.tsx:81-88, 182-224` combines owner and semantic-surface visibility; `:226-242` generation-guards find results/errors, with invalidation on browser change/unmount/close.
- `ui/src/components/TerminalSearchOverlay.tsx:65-130, 160-170` generation-guards success/error, clears on session change, reissues the retained query, and invalidates on clear/unmount.
- `ui/src/lib/terminalSettings.ts:205-244, 275-277` shares the native preference promise, tracks the latest resolved preferences, prevents stale request publication, and avoids replacing persisted startup background before preferences resolve.
- `ui/src/lib/nativeTerminalVisibility.tsx:10-29, 51-70` evaluates every matching surface independently and honors an opt-out on the surface or an ancestor.
- `ui/src/state/workspaceStore.ts:1138-1160` includes the state read by the affected memoized selectors.
- `ui/src/components/NativeTerminalPane.tsx:1748-1763` rearms a resolution media query, updates bounds only after attachment, and removes both listeners on cleanup.

The runtime artifact independently records shared settings fetch/race behavior, worktree badge transfer, and density 2-to-1 bounds updates with no lifecycle calls. The audit records real-component WebKit modal hide/show and stale terminal-search behavior. Native compositor pixels are not claimed as verified.

## direct remove-ai-slops / programming pass

No blocking slop, overfit, or maintenance regression was found in the scoped diff.

- The new tests assert behavioral transitions and adversarial races rather than requested text/removal, implementation constants, or tautologies.
- Deferred promises and event delivery are deterministic; no new fixed sleeps or timing-luck polling were introduced in scoped tests.
- Production additions are localized refs/effects/cache state needed by the concrete races; no speculative parsing, normalization, extraction, or abstraction was added.
- Comments are somewhat detailed but explain native-compositor and race invariants; they do not create a criterion failure.
- Foreign IME/shortcut/RemoteTerminal and Rust test volume was excluded from this assessment.

## checked artifact paths

- `docs/audits/TAURI_FRONTEND_RENDERING_REVIEW_2026-09-05.md`
- `docs/evidence/rendering-review-2026-09-05/runtime-checks.json`
- `ui/src/App.tsx`
- `ui/src/App.test.tsx`
- `ui/src/components/BrowserPane.tsx`
- `ui/src/components/BrowserPane.test.tsx`
- `ui/src/components/TerminalSearchOverlay.tsx`
- `ui/src/components/TerminalSearchOverlay.test.tsx`
- `ui/src/components/NativeTerminalPane.tsx` (density listener only)
- `ui/src/components/NativeTerminalPane.lifecycle.test.tsx`
- `ui/src/lib/terminalSettings.ts`
- `ui/src/lib/terminalSettings.test.tsx`
- `ui/src/lib/nativeTerminalVisibility.tsx`
- `ui/src/lib/nativeTerminalVisibility.test.tsx`
- `ui/src/state/workspaceStore.ts`
- `ui/src/state/workspaceStore.test.tsx`
- Current scoped `git diff`; foreign files were identified and excluded.

## evidence assessment

Executor claims are treated as historical evidence, not rerun verification, per the bounded instruction not to run tests/build:

- Audit reports `CI=1 bun run --cwd ui test --reporter=dot`: 146 files / 1,498 tests passed.
- Audit reports `bun run --cwd ui build`: TypeScript + Vite exit 0.
- These checks precede later foreign IME work and therefore do not verify the latest combined working tree; no broader claim is made.
- Runtime JSON directly contains settings startup/race, worktree activity transfer, and density scale 2 then 1 with an empty lifecycle list.

## exact evidence gaps / notes

1. **NOTE - native pixels:** Physical WGPU/WKWebView/WebView2/Linux compositor pixels remain unverified. Evidence pointer: `docs/audits/TAURI_FRONTEND_RENDERING_REVIEW_2026-09-05.md`, “Native desktop verification boundary.” This is explicitly allowed and is not a blocker.
2. **NOTE - latest combined tree:** The reported full test/build run predates foreign NativeTerminalPane IME edits. Evidence pointer: task brief and audit final verification. The reviewed session changes have historical passing evidence, but the current mixed tree is not claimed green.
3. **NOTE - missing separate review artifact:** No session-specific code-review report artifact was found, so its required explicit skill-perspective/overfit coverage could not be confirmed. This gate performed that pass directly. This is an input/reporting gap, not a stated user outcome criterion and therefore not a blocker.
4. **NOTE - manual QA matrix:** No standalone manual-QA matrix artifact was found; the audit contains the manual checklist and runtime observations. This does not contradict a stated success criterion.
5. **NOTE - BrowserPane unit coverage:** The scoped `BrowserPane.test.tsx` diff only adds cleanup; modal masking is supported by the audit's actual-component WebKit exercise, while browser-find generation behavior is supported primarily by code inspection rather than a dedicated changed test artifact. No failing behavior is evidenced, so this is not grounds for rejection.
