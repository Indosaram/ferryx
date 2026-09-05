# Frontend rendering session re-review - 2026-09-05

## Scope and session reconstruction

The user requested review, commit, and closure of session `01a07093-abde-7b1f-b61f-ff34447cb572`.
The raw session log was inspected, including the original frontend rendering request, implementation, verification, commit results, and the final unexecuted re-review request at 14:00 UTC.

The original work was already committed on main:

- `9c07825`: Refresh worktree activity selectors and deduplicate focused-terminal publication.
- `83a3296`: Share native preference requests/cache and preserve startup background.
- `2aef606`: Mask native browsers for semantic overlays and reject stale search responses.
- `076e429`: Update native bounds on display-density changes without reattachment.
- `3496f54`: Preserve the original audit, reviewer verdict, and runtime evidence.

These commits contain 24 files. No session-owned implementation remained uncommitted at the end of that session. Its last request to re-review and commit had no implementation or tool activity afterward.

A detached, locked review worktree at `3496f54` isolated these commits from concurrent main-worktree changes. The main index was empty. Before writing review evidence, 242 foreign dirty paths were inventoried and hashed. None were selected for this review commit. Main has subsequent input/IME and protocol commits; App, workspaceStore, and NativeTerminalPane have later changes, so the checks below do not certify the latest combined main tree.

## Code assessment

The lead reviewed the production diff and regression tests across cache ownership, search response ordering, browser visibility, workspace selector dependencies, focused-terminal publication, and display-density lifecycle. No introduced blocking defect was found in the scoped commits. No production code or tests were changed during this re-review.

- Browser visibility is already serialized by `enqueueBrowserLifecycle` in `ui/src/lib/browserTauri.ts`; an additional ordering mechanism is not needed for this change.
- Search generations are invalidated on newer queries, clear, teardown, and target changes. Terminal target changes reissue the retained query.
- Superseded pending preference requests adopt the current shared promise; new consumers use the resolved cache without replacing the persisted startup background.
- Workspace activity dependencies include session rebinding and parked-layout inputs.
- Density listeners rearm for the new resolution and are removed on teardown, preserving the native attachment.
- Added race tests use deferred results and explicit React/MutationObserver delivery rather than fixed sleeps. Restore fixture changes retain their assertions and register an actual project root.

Independent gate review: **APPROVE**, with no introduced blocking defect or criterion violation. The reviewer checked the exact committed source, tests, and lead-captured verification evidence. It confirmed browser masking, search response invalidation, shared preference ownership, activity dependencies, publication deduplication, and density-listener cleanup. Native compositor pixels and the pre-existing search-navigation gap remain the two non-blocking notes below.

The reviewer did not rerun the commands; execution results in this report belong to the lead. LSP requests used the error filter, so the evidence establishes zero reported errors, not an exhaustive warning audit.

## Fresh automated verification

Executed in the isolated review worktree, not the mixed main tree:

- `CI=1 bun run --cwd ui test --reporter=dot`: 145 test files passed; 1,492 tests passed in one run.
- `bun run --cwd ui build`: TypeScript and Vite passed; 1,860 modules transformed; Vite finished in 5.29 seconds.
- The combined test/build monitor exited with code 0.
- `git diff ea27a71..3496f54 --check`: exit 0.
- LSP diagnostics on the scoped production files reported no errors. A separate directory scan also reported no errors but was capped at 50 files; the TypeScript build is the full type check.
- The new JSON evidence has no LSP diagnostics. Markdown diagnostics are unavailable because no Markdown language server is configured; the report receives content and whitespace review instead.

The original session reported 146 files and 1,498 tests on a mixed working tree that included concurrent pane-to-tab-row tests. This re-run uses the committed tree, where that foreign file is absent. All 145 discovered files and 1,492 tests ran; no test was skipped or assertion reduced. The historical mixed-tree count is not presented as the isolated commit count.

## Fresh real-surface checks

The lead mounted the actual BrowserPane, CommandPalette, and TerminalSearchOverlay in isolated system WebKit at 1100 by 750 pixels, using the committed React source and CSS. Only the Tauri IPC boundary was faked and recorded. No user desktop window was operated.

- Modal masking: opening the real command palette emitted browser visibility false; dismissing emitted true. PASS.
- Owner visibility regression: dismissing the palette with the browser owner hidden left final visibility false. PASS.
- Layout: the palette bounds were x=214, y=92, width=672, height=363, with no horizontal overflow. PASS.
- Search race: queries `err` and `error` were issued; resolving the newer request with 2 matches and then the older with 5 left visible `1/2`. PASS.
- Clear: clearing the query removed the result counter. PASS.
- The lead inspected a fresh tool screenshot: the search input, `1/2` counter, and controls were visible and unclipped. The blank native-content area is expected because native compositor output is not simulated.

Runtime artifact: [session-rereview-runtime.json](../evidence/rendering-review-2026-09-05/session-rereview-runtime.json).
Original broader WebKit evidence for settings, worktree activity, and density remains in [runtime-checks.json](../evidence/rendering-review-2026-09-05/runtime-checks.json). Those rows were not rerun in WebKit this turn; their committed regression tests passed in the fresh full suite.

## Remaining boundaries

- Physical WGPU/native child-view pixels, monitor-to-monitor sharpness, and Windows/Linux desktop compositor output were not verified. These are not implied by browser IPC or build success.
- Please manually verify with exactly `bun tauri dev`: open and close modals over a browser/terminal, switch tabs, resize split panes, and move between monitors with different pixel densities. Check for overlap, black areas, stale content, or blur.
- Terminal search previous/next changes the result index but does not scroll to or highlight an exact native match. This is a pre-existing capability gap, not added by the reviewed commits.
- Later mixed-tree notification, drag/drop, Rust, packaging, and scoped-feature work is outside this session and remains untouched.

## Delivery

This re-review adds only this report and its runtime JSON artifact. Existing rendering commits are retained; they are not recreated, amended, or rewritten. No push is requested or performed.
