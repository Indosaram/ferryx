# dag-viewport-navigation - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** DAG 화면을 클릭해서 끌면 이동하고, 휠과 핀치로 원하는 지점을 확대하거나 축소할 수 있습니다. 전체 보기와 배율 복귀 버튼으로 시점을 쉽게 되돌립니다.

**Why this approach:** 카드와 연결선을 함께 움직여 정렬이 어긋나지 않게 합니다. 모달과 독립 패널에 같은 동작을 적용하고 실시간 상태 변경 중에도 시점을 유지합니다.

**What it will NOT do:** 노드 배치를 바꾸는 편집기, 새 그래프 라이브러리, 시점 저장 기능은 추가하지 않습니다. 실행 중인 터미널 세션을 종료하지 않습니다.

**Effort:** Medium
**Risk:** Medium - 실제 앱의 입력 전달과 모달·터미널 간 간섭은 브라우저 화면과 별도로 검증해야 합니다.
**Decisions to sanity-check:** 드래그는 이동, 휠·핀치는 확대/축소, 새 실행은 전체 보기, 같은 실행의 갱신은 현재 시점 유지입니다.

Your next move: 계획 검토가 끝나면 별도 구현 실행을 요청하세요. 이 문서는 구현계획이며 제품 수정 완료 보고가 아닙니다.

---

> TL;DR (machine): Medium effort, medium risk; shared DAG pan/zoom camera, real-host verification and evidence. Planning only in the authoring session.

## Scope
### Must have
- Replace scrollbar-based navigation with left-click drag pan on the background and noninteractive node cards.
- Wheel and pinch zoom about their anchor; accessible minus, percentage/100% reset, plus and Fit controls.
- Apply one local camera to the existing common parent of HTML cards and SVG edges. Preserve world coordinates, graph layout, ownership, run tabs and terminal sessions.
- Cover both real hosts: `DagPaneBadge` portal modal and the DAG leaf mounted through `TerminalSplitView`.
- Preserve the camera across same-run live updates; initialize/fit new runs and recover with Fit. Handle small, large, empty and temporarily zero-sized views.
- Capture the reported scrolling problem separately from the source-confirmed absence of camera controls. No claim of a native scroll root cause without native evidence.

### Grounded references
- `ui/src/components/dag/DagGraphView.tsx:25-55,95-178`: store/prop selection, early empty return, world bounds, scroll wrapper and shared children. Existing viewport is at line 138.
- `ui/src/components/dag/DagNodeCard.tsx:85-132`: noninteractive, select-none cards.
- `ui/src/components/dag/DagEdgeLayer.tsx:64-135` and `dagViewUtils.ts:3-8,35-40`: SVG ports/markers and world coordinate constants. A 4-column, 2-row fixture has world bounds 1160 x 214 CSS pixels.
- `ui/src/components/dag/DagPaneBadge.tsx:128-170,179-260`: modal Escape/Tab trap, backdrop, run tabs and shared graph mount.
- `ui/src/components/TerminalSplitView.tsx:1073-1076,1261-1263`: ancestor focus handling and real standalone DAG host. Re-read before editing; line offsets may drift.
- `ui/src/components/dag/DagGraphView.test.tsx:36-48,140-286`: geometry, serialization, host mounting and fallback waves.
- `ui/src/components/dag/DagPaneBadge.test.tsx`, `DagPaneBadge.ownership.test.tsx`, `dagViewUtils.test.ts`: retain existing regression scope.
- `ui/package.json:7-10`: Vitest via `bun run --cwd ui test`; `ui/activity-qa.html`, `ui/src/devtools/activityQaMain.tsx`, `ActivitySurfaceHarness.tsx`: existing real-component QA pattern.
- `ui/DESIGN.md`: incumbent tokens and IconButton; preserve appearance. `docs/DAG_VIEWER_EDGE_ROUTING_AND_TABS_2026-09-07.md`: historical edge/tab behavior, not fresh runtime evidence.
- Recent source history: `a31a76c6` edge arcs/run tabs; `a9f680e3` exact badge ownership. Current scroll failure is a user report; it has NOT been reproduced during planning.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No implementation in the planning session. Execution requires a separate explicit start.
- No graph editor, node repositioning, layout replacement, minimap, new graph/gesture library, persistent camera settings or schema changes.
- No global wheel interception, terminal rewrite, daemon restart or backend changes without a reproduced in-scope integration defect and a corresponding failing test.
- No app release, direct binary launch, ad-hoc signing, existing daemon termination or interference with user PTYs. Desktop launch is exactly `bun tauri dev`.
- No unrelated branding/design cleanup, dependency installation for polish, fixed sleeps, test suppression or restoration of foreign changes.
- Skills for execution: programming (TS/TDD), frontend interaction/layout references plus incumbent design (viewport UX), visual-qa (screenshots), debugging and ultimate-browsing (real input and runtime diagnosis), bun-1-4 (Bun/Chrome tools). Read only workflows actually executed.

### Binding camera/input contract
1. Camera is `{x,y,scale}` in CSS screen pixels, local to the graph; mapping `p = t + s*w`. Do not store it in `dagStore` or workspace persistence.
2. Use the existing world wrapper with `transform-origin:0 0` and `translate(xpx,ypx) scale(s)`. Remove its viewport-relative min sizes. Viewport is borderless/padding-free, `min-h-0 min-w-0 overflow-hidden`, `touch-action:none`, `overscroll-behavior:none`. Header/controls remain outside the transformed world. No transform transitions or inertia.
3. One-pointer pan adds client-coordinate deltas to translation without dividing by scale. Accept only left mouse or touch/pen; ignore interactive descendants (`button,a,input,textarea,select,[contenteditable],[data-no-pan]`). Capture accepted pointers on the viewport. Stop propagation only for consumed viewport input; do not swallow run-tab or modal control events.
4. Maintain fresh camera/gesture refs so multiple events before React render use the latest state. One primary drag owner for mouse; stable first two touch pointers for pinch; ignore extra contacts until selected contacts end.
5. Pointerup/cancel/lost capture removes that pointer and rebases the remaining gesture. Run switch, unmount, blur, hidden document and resize cancel active gestures. Release held captures idempotently. Subsequent unpressed moves cannot move the camera. Never synthesize a card click from a drag.
6. Wheel normalization: pixel delta unchanged, line delta *16, page delta *positive viewport height. For nonzero deltaY, `requested=s*exp(-0.002*clamp(deltaPixels,-1000,1000))`. Clamp scale first, then `tNew=p-(sNew/s)*(p-t)`. Prevent default and stop propagation for consumed wheel, including at scale limits. Pure horizontal wheel leaves camera unchanged and is not a camera gesture. Native wheel listener is viewport-scoped, `{passive:false}`, removed on cleanup.
7. Ordinary wheel and Ctrl+wheel use the same zoom path (no extra multiplier). Two-touch pinch records midpoint `m0`, distance `d0` and world anchor `(m0-t0)/s0`; movement uses `s1=clamp(s0*d1/d0)` and `t1=m1-s1*anchor`. Defer a pinch baseline if distance <1px. Rebase on 1->2->1 transitions; no jump.
8. Fit for positive W,H and nonempty graph: `margin=min(24,W/4,H/4)`, `fit=min(1,(W-2*margin)/worldWidth,(H-2*margin)/worldHeight)`, translation centers the world rectangle. Maximum scale is 3; effective minimum for explicit zoom is `min(0.1,fit)`. Bounds updates alone must not clamp existing camera. If current scale is below a new minimum, an outward request is a no-op; an inward request progresses toward the minimum without jumping. If above a newly lowered maximum, symmetric handling applies.
9. Minus/plus divide/multiply by 1.2 at viewport center. Percentage button sets 100% at center; Fit applies fit camera. Labels: `Zoom out`, `Reset zoom to 100%`, `Zoom in`, `Fit graph`. Use focusable native buttons with `aria-disabled` and no-op at limits, avoiding the existing trap's disabled-button problem.
10. All hooks must be unconditional before the current empty return. Initial nonempty run, no-run->run and same-run empty->first-nonempty fit only after positive measurement. Changed `runId` cancels gesture and fits; same-run status/topology changes preserve camera. Reopening starts fresh; no per-run persistence.
11. ResizeObserver retains last positive dimensions, ignores zero, and changes translation by half the viewport-size delta, keeping scale and center world point. Cancel gesture on resize; do not auto-fit after user navigation.
12. Preserve existing header information and theme; use a wrapping header/legend on narrow hosts, with controls outside the world and no horizontal page overflow. Browser at 390x844 must expose all controls; tab strip keeps its own scroll ownership.
13. Native pinch delivery is not assumed. Test actual WKWebView wheel/Ctrl+wheel/gesture events. If WKWebView needs gesture events, isolate that adapter and ensure exactly one source owns a physical gesture. No speculative duplicate zoom handler.

## Verification strategy
> Agent-executed browser and native checks. Unavailable native automation is an explicit unmet gate, never a Chrome proxy PASS. No product runtime is launched during planning.
- Test decision: TDD with Vitest through `bun run --cwd ui test src/components/dag`. Existing characterization tests stay green; capture new behavior assertions RED before production changes.
- New harness/selectors must load before RED: verify real graph, nodes and edges exist; report failure only when movement/zoom fails. Missing imports/selectors/fixture ownership are harness errors, not behavioral RED.
- Evidence root `E=.omo/evidence/dag-viewport-navigation/`; separate `baseline/`, `green/`, `native/`, `final/` logs, numeric results, action traces, screenshots and cleanup receipts. Write a durable verification summary under `docs/DAG_VIEWPORT_NAVIGATION_VERIFICATION.md`.
- Vitest injects exact nonzero bounds, controlled ResizeObserver callbacks and pointer-capture semantics. Assert rendered output/state, not only mock calls. No sleeps or time-based polling. Subscribe to the relevant render/DOM event before dispatch; use bounded event timeouts and screenshots after the acknowledged render.
- Build after clean LSP diagnostics on all changed TS/TSX: `bun run --cwd ui build`; final relevant tests below. Full suite once on final inputs: `bun run --cwd ui test`. Distinguish baseline failures using captured baseline, not historical memories; never suppress them.

### Harness and fixture contract
Create `ui/dag-viewport-qa.html`, `ui/src/devtools/dagViewportQaMain.tsx` and `scripts/qa/dag-viewport-navigation.mjs` during execution. Use existing Playwright-core/real Chrome tooling when available; do not add a production dependency. Server: `bun run --cwd ui dev --host 127.0.0.1 --port 5193 --strictPort`. URL: `http://127.0.0.1:5193/dag-viewport-qa.html`.

The harness imports real DagGraphView, DagPaneBadge, TerminalSplitView and dagStore. Only PTY/native IPC is stubbed in the browser harness, explicitly disqualifying it as native evidence. Fixture A has runId `qa-dag-a`, rootSessionId `qa-owner`, running status, waves `[a,b]`, `[c]`, `[d]`, `[e]` and edges a->c, b->c, c->d, a->e, d->e. Fixture B `qa-dag-b` shares owner with different dimensions. Big fixture has 100 columns, one node each, a chain plus a first-to-last edge. Tall fixture has one column and 100 nodes. Empty has no nodes/waves. Seed via real store APIs; providerSessionId is exactly `qa-owner`.

Two host selectors: `[data-testid="dag-pane-modal"] [data-testid="dag-graph-view"]` after badge click, and `[data-testid="qa-standalone-host"] [data-testid="dag-graph-view"]` inside real TerminalSplitView containing a DAG leaf and terminal sibling. Scope each query; never use an ambiguous global viewport selector. Before new test IDs exist locate the existing first graph body under `dag-graph-view` by its shared SVG/card content. Add `dag-viewport`/`dag-world` IDs in the implementation for continued measurement.

Runner interface to create: `bun scripts/qa/dag-viewport-navigation.mjs --phase baseline|green --host modal|standalone|both --evidence-dir <directory>`. Invalid arguments exit nonzero. Baseline mode records existing world/card/edge geometry then performs the same input sequences; graph existence must pass first and pan/zoom assertions fail specifically. Green mode fails nonzero on any scenario failure and writes structured results, not merely screenshots. The runner owns and closes its isolated Chrome context; server lifetime is owned by the invoking monitor/session.

### Exact scenarios and binary observables
- **S1 Pan (C1, both hosts):** Chrome `page.setViewportSize({width:1280,height:900})`; open host, read viewport rect R. `page.mouse.move(R.x+100,R.y+100); page.mouse.down(); page.mouse.move(R.x+220,R.y+180); page.mouse.up()`. PASS: first card and chosen edge endpoint each move `(120,80)` within 1 CSS px, header unchanged, no camera scrollbars. Repeat starting on a card and at 0.5x and 2x via controlled wheel input. Compare actual card bounds and SVG endpoint transformed by `getScreenCTM`, not only CSS text.
- **S2 Zoom (C2, both hosts):** move mouse to viewport center; `page.mouse.wheel(0,-120)`, then `(0,120)`. PASS: scale increases then returns within numerical tolerance, original anchor world point stays within 1px, document scroll and browser visualViewport scale unchanged. Unit variants set deltaMode 0/1/2 and Ctrl true/false with equivalent normalized deltas. At min/max repeat wheel input: both translation and scale unchanged. Horizontal-only `(120,0)` leaves camera unchanged.
- **S3 Controls/Fit (C3):** after pan far offscreen, `page.getByRole('button',{name:'Fit graph',exact:true}).click()` scoped to host. PASS: world, all cards and edge endpoints fit measured viewport. Repeat big/tall fixtures and at `390x844`; big fixture fit <0.1 is valid. Plus then minus restores scale, reset yields 1. All controls visible/in bounds, focusable at limits and click does not pan; no outer page overflow. Tiny 20x20 viewport and zero dimensions are unit cases: finite values, deferred fit at zero.
- **S4 Gesture lifecycle (C1/C3):** drag from viewport across dialog boundary to `(4,4)`, release, then move back with no button. PASS: modal remains open and camera stays fixed after release. Repeat ignored right/middle buttons. Unit-dispatch pointercancel/lost capture, blur, hidden state, unmount and run switch during drag; every later unpressed move is inert. Reopen twice and wheel once: same single zoom factor, not multiplied by leaked listeners. Interactive target never starts pan.
- **S5 Pinch (C2):** Chrome DevTools `Input.dispatchTouchEvent` with touchStart contacts `(R.x+100,R.y+100)` and `(R.x+200,R.y+100)`, then touchMove to `(R.x+80,R.y+120)` and `(R.x+240,R.y+120)`, touchEnd; construct coordinates inside the viewport. Away from limits PASS: scale =1.6*s0 and original midpoint world anchor maps to local `(160,120)` within 1px. Begin with one contact, add second, lift either then move remaining by `(20,10)`: no transition jump and translation changes exactly `(20,10)`. Cancellation and degenerate-distance cases also tested deterministically in Vitest. CDP browser proof is not native trackpad proof.
- **S6 Live state/hosts (C4):** while zoomed/panned, harness `Update status` publishes same run through dagStore; `Select run B` uses real modal tab; `page.setViewportSize({width:1100,height:800})`; press Tab/Shift+Tab then Escape. PASS: same-run update preserves camera, B fits once, center world point is stable on resize, Tab stays in modal and Escape closes it. Empty->populated fits once; hidden zero-size->positive does not produce NaN. Standalone sibling bounds and pane tree unchanged during pan; split seam still resizes when dragged directly. Preserve single run title, ownership and existing edge paths.
- **S7 Native delivery (C5):** execution preflight must discover available computer-use capabilities and read their actual tool schema. Launch exactly `bun tauri dev` in a monitored background session. Use only the dedicated QA workspace/panes in that debug app; select the actual existing DAG badge and standalone DAG leaf through accessibility labels, then perform S1/S2/S4 using the tool's real mouse/wheel actions. Record the literal tool invocation, target bounds, event delivery and before/after camera geometry in `E/native/actions.json` BEFORE declaring PASS. On macOS use supported OS automation for the debug app (not the user's browser profile). Actual trackpad pinch must record whether native wheel or gesture events arrive and prove one scale update path per gesture.
  - Baseline before camera edits: reproduce native vertical/horizontal scrolling on the overflowing fixture, record `scrollTop/Left`, wheel target/defaultPrevented path, native overlay state and event receipt. If reproduction fails, record that outcome honestly and still prove requested camera behavior after implementation; do not invent a root cause.
  - PTY isolation oracle: use a fresh QA-only sibling terminal, with input/scroll forwarding instrumented at the actual native IPC boundary (not a mock). Subscribe to writes and scroll forwards before each DAG gesture; log a pre/post count and session IDs. PASS: expected viewport events arrive, camera changes, zero PTY input or terminal-scroll forwards, unchanged split geometry. Then click the QA terminal and type a sentinel to prove instrumentation was live; this must increment its counter. Remove temporary instrumentation after evidence capture and rerun affected checks.
  - If tools cannot drive the debug app, cannot observe actual PTY dispatch, or cannot reproduce a physical pinch path, mark C5 UNVERIFIED and stop any implementation-complete claim. Ask for the precise missing capability or explicit user-run verification if OS automation is prohibited; the resulting user evidence is separately labeled, never silently counted as agent-executed QA. Browser PASS cannot satisfy C5.
- **S8 Cleanup:** close runner Chrome context and kill only the owned Vite/debug-runner process tree, preserving any pre-existing app/daemon. `lsof -nP -iTCP:5193 -sTCP:LISTEN` must show no owned listener after teardown; verify owned PIDs gone. Remove temporary IPC instrumentation, fixture workspaces/terminals and QA-only resources you created. Do not remove captured evidence. Write `E/final/cleanup.md`; no receipt means verification incomplete.

## Execution strategy
### Parallel execution waves
Planning-session tier is LIGHT because this is a prose artifact. Execution is HEAVY: a new gesture state machine and cross-host native input/lifecycle integration need explicit regression and surface coverage.

- Wave 1: task 1 establishes faithful baseline and native QA capability; no camera implementation starts before its behavioral RED.
- Wave 2: task 2 owns the complete camera implementation and tests. Do not split one component's gesture/resize/wheel state between concurrent writers.
- Wave 3: task 3 verifies the combined change through both hosts and native runtime, fixing only reproduced in-scope defects.
- Wave 4: task 4 records evidence and operating behavior after verification.
- Final wave: F1-F4 independent read-only audits run in parallel; one gate reviewer consolidates actual blockers. No per-implementation-node approval gates.
- This small single-component change cannot safely provide 5-8 independent edits per wave. Serial implementation is intentional to avoid artificial helpers/write conflicts; parallelism is used in independent discovery and final audits.

### Dependency matrix
1 -> 2 -> 3 -> 4 -> {F1,F2,F3,F4}. F1-F4 do not edit files. Task 3 owns fixes and reruns only affected scenarios; reviewers do not race it. Optional independent test commands may run in parallel only if fixtures/runtime resources do not conflict.

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [ ] 1. Establish real-host fixtures, faithful behavioral RED and native capability
  - Recommended task executor category: deep - browser/native input reproduction and host wiring.
  - What to do / Must NOT do: Inspect current git status/diff and source references; create the isolated harness/runner contract above. Run existing DAG tests as characterization. Capture S1/S2 baseline only after fixture existence is proved; capture S7 native scroll observation and tool capability. No camera production edits and no fake-red missing selectors. Restrict edits to new QA harness/runner and necessary test-only fixtures.
  - Parallelization: Wave 1 | Blocked by: none | Blocks: 2.
  - References: Scope references, harness contract, S1/S2/S7/S8, real TerminalSplitView and DagPaneBadge hosts.
  - Acceptance criteria: `bun run --cwd ui test src/components/dag` baseline saved; `bun scripts/qa/dag-viewport-navigation.mjs --phase baseline --host both --evidence-dir .omo/evidence/dag-viewport-navigation/baseline` reports graph-load PASS followed by pan/zoom behavioral failures, not setup failures. Native capability receipt explicitly says supported/unavailable and scroll reproduced/not reproduced. No unsupported complete claim.
  - QA scenarios: Happy: both host fixtures contain real nodes/edges, buttons and exact owner. Failure: wrong owner fixture must not display the badge; correct it before RED. Run S8 on spawned resources.
  - Evidence: `E/baseline/{tests.log,results.json,actions.json,*.png,cleanup.md}` and `E/native/baseline.md`.
  - Commit: Y | `test(dag): add real-host viewport navigation harness`; baseline failures are expected runner mode, never committed failing default tests.

- [ ] 2. Implement the shared viewport camera with failing-first interaction tests
  - Recommended task executor category: visual-engineering - one owner for React viewport and its gesture/state tests.
  - What to do / Must NOT do: First add new behavior tests in DagGraphView.test.tsx (or a colocated viewport test file), capture RED against current component. Implement binding camera contract in DagGraphView.tsx with small pure math functions in existing dagViewUtils.ts only if shared by event paths. Preserve DagNodeCard/DagEdgeLayer geometry. Do not create a generalized camera framework. Use existing IconButton/tokens; only update scoped design contract if required.
  - Parallelization: Wave 2 | Blocked by: 1 | Blocks: 3.
  - References: Entire camera contract; DagGraphView early return/world wrapper; dagViewUtils constants; DagPaneBadge focus trap; S1-S6.
  - Acceptance criteria: RED assertions cover visible pan, anchor zoom, controls/fit, touch transition, cancel, no-run/empty/resize/run-update cases; GREEN `bun run --cwd ui test src/components/dag`, clean changed-file LSP, `bun run --cwd ui build` exit 0. Existing tests remain intact.
  - QA scenarios: Happy: S1/S2/S3 in real Chrome after first render. Failure: S4/S5/S6, zoom at limits, zero-sized resize and large-fit fixture. Run `bun scripts/qa/dag-viewport-navigation.mjs --phase green --host both --evidence-dir .omo/evidence/dag-viewport-navigation/green`; all declared browser assertions pass. Screenshot and inspect at 1280x900 and 390x844 before any next visual change.
  - Evidence: `E/green/{red-tests.log,green-tests.log,build.log,results.json,actions.json,desktop.png,mobile.png,cleanup.md}`.
  - Commit: Y | `feat(dag): add drag panning and anchored viewport zoom`.

- [ ] 3. Prove host isolation and native input, fixing only observed integration defects
  - Recommended task executor category: deep - real native event delivery, cancellation and PTY isolation.
  - What to do / Must NOT do: Run all S1-S7 through real hosts, not a naked shared component. Exercise actual debug app with `bun tauri dev`; verify original scroll observation and new camera input independently. If a terminal/native guard prevents DAG events, first capture that failure and add the narrow regression test, then patch only the proven handler/overlay boundary. Preserve terminal input everywhere else. No speculative fallback or broad terminal changes.
  - Parallelization: Wave 3 | Blocked by: 2 | Blocks: 4.
  - References: S4-S8; DagPaneBadge modal bounds/focus; TerminalSplitView ancestor focus and native visibility boundary; current native input forwarding found by LSP during task 1.
  - Acceptance criteria: browser results pass for both hosts and desktop/mobile; native event/geometry/PTTY-counter evidence satisfies S7 with zero leakage and live counter control. Physical pinch delivery validated or C5 explicitly stays incomplete. Remove temporary instrumentation and verify final source still matches the tested behavior.
  - QA scenarios: Happy: native pan and zoom reach camera, terminal sentinel verifies counter. Failure: drag-release outside, run switch during gesture, native pinch duplicate delivery and unaffected sibling terminal input/scroll. S8 cleanup required before task completion.
  - Evidence: `E/native/{actions.json,events.json,pty-input.json,before.png,after.png,verdict.md}` and `E/final/cleanup.md`.
  - Commit: Y only if fixes required | `fix(dag): isolate viewport gestures from host input`.

- [ ] 4. Document verified controls and finalize regression evidence
  - Recommended task executor category: writing - concise behavior/verification record, no code ownership.
  - What to do / Must NOT do: Write `docs/DAG_VIEWPORT_NAVIGATION_VERIFICATION.md` with actual controls, scope, native reproduction result, exact tested revision, commands, evidence links and limitations. Do not label proposed or unrun tests as passes. Record S8 receipts; no generated screenshots in unrelated documentation.
  - Parallelization: Wave 4 | Blocked by: 3 | Blocks: F1-F4.
  - References: This plan's C1-C5/S1-S8, existing DAG docs, actual task 1-3 evidence.
  - Acceptance criteria: every success criterion links to baseline/GREEN/surface/cleanup evidence; targeted DAG suites and final full `bun run --cwd ui test` recorded once on final source, changed-file diagnostics plus `bun run --cwd ui build` clean or unrelated failures precisely explained. No hidden skips or weakening.
  - QA scenarios: Happy: read report and open every linked artifact, matching command/revision/scenario. Failure: a missing native receipt keeps C5 incomplete, and missing cleanup or stale evidence blocks completion.
  - Evidence: `E/final/{tests.log,build.log,coverage-audit.md,cleanup.md}` plus durable docs report.
  - Commit: Y | `docs(dag): record viewport navigation verification`.

## Final verification wave
> Runs in parallel after all tasks. ALL must approve based on actual evidence, not work duration or passing tests alone. No additional user approval is needed for a technical PASS; user-only native verification remains explicitly labeled where required.
- [ ] F1. Plan compliance audit
  - Recommended task executor category: deep - map C1-C5 and S1-S8 to final evidence and inspect assertion coverage.
  - References: Success criteria and `docs/DAG_VIEWPORT_NAVIGATION_VERIFICATION.md`.
  - Acceptance/QA: read all result records and reject missing scenario, host, RED or cleanup evidence. Write `E/final/F1.md`. Commit: N.
- [ ] F2. Code quality review
  - Recommended task executor category: deep - review lifecycle and event ownership in final diff.
  - References: camera contract and changed-file LSP/test/build outputs.
  - Acceptance/QA: verify cleanup, fresh camera refs, unconditional hooks, no suppressions/dependencies/global wheel intercept; reject any concrete regression. Write `E/final/F2.md`. Commit: N.
- [ ] F3. Real manual QA
  - Recommended task executor category: deep - audit agent-executed real-surface evidence; this title does not mean a unit simulation.
  - References: S1-S8 and browser/native action logs/screenshots.
  - Acceptance/QA: match final source revision, host-scoped numeric observables and visible screenshots. Rerun only stale/missing scenario evidence, with new cleanup receipt; C5 cannot be inferred from Chrome. Write `E/final/F3.md`. Commit: N.
- [ ] F4. Scope fidelity
  - Recommended task executor category: writing - compare request, boundaries and final artifact inventory.
  - References: Scope IN/OUT, initial git status, final diff and report.
  - Acceptance/QA: no user-session damage, foreign changes, product expansion or stale done claims; verify only authorized files/commits. Write `E/final/F4.md`. Commit: N.

## Commit strategy
- User ultrawork directive authorizes atomic verified commits. Re-read `git log --oneline -20` and `git log -5 -- <paths>` before each; use prevailing `type(scope): subject`.
- Each increment includes implementation plus its tests; do not commit broken default tests or temporary native instrumentation. No force push, history rewrite, branch change or unrelated edits.
- Final commit footer: `Plan: .omo/plans/dag-viewport-navigation.md`.
- Planning session itself commits only this completed/reviewed plan as `docs(dag): plan viewport pan and zoom navigation`; draft/notepad are state, not product files.

## Success criteria
- **C1 Drag navigation:** S1 and S4 pass in modal/standalone: primary drag pans world in screen pixels, no scrollbars, cancelled/unpressed moves do not pan, nodes/edges remain aligned. RED in baseline and new component tests, GREEN/surface receipts required.
- **C2 Zoom/pinch:** S2 and S5 pass: wheel, Ctrl+wheel and touch pinch anchor correctly, limits do not drift, gesture transitions do not jump. Native physical pinch additionally required by C5.
- **C3 Recovery/lifecycle:** S3/S4/S6 pass: accessible controls, 100%/Fit, large/tall/empty/zero-size states, resize and no listener leaks. Large fixture fits below 10%; no unstable refit.
- **C4 Adjacent regressions:** S6 and existing DagGraphView/DagPaneBadge/dagViewUtils/ownership/store tests pass: exact run ownership, modal focus/tabs/Escape, edge geometry, pane serialization and split/terminal independence preserved.
- **C5 Real app proof and cleanup:** S7 proves debug-app event delivery and PTY isolation, distinguishing current scroll reproduction from zoom absence; S8 receipts close owned resources. An unavailable native channel is incomplete, not passed.
- Planning completion is separate: complete executable plan, grounded claims, gap analysis, live-artifact-matched plan-reviewer approval, structural validation and delivery. No product behavior is claimed implemented by producing this plan.
