# P31 local repair receipt - 2026-09-13

Task st_01a0993a. Local implementation only; Windows acceptance remains open.
Owned writes: Sidebar.tsx, WorktreeList.tsx, Sidebar.sortableIdentity.test.tsx
(all under ui/src/components), and this receipt. Initial owned production paths
were clean. Foreign changes were surveyed and left untouched. Final observed
HEAD: da6eec06d65551f67bbc43f09910cde470c3478d; shared checkout is moving.

## Repair

Sidebar's existing qualified SortableContext IDs now also register through the
real WorktreeList useSortable hook. Drag data retains rowWorkspaceId separately
from the containing group workspaceId. Drop indexing, overlay member lookup,
order application and pruning use the same worktreeSortableId. New persisted
orders store these IDs; old path-only and previously readable member:path keys
remain readable. A legacy equal path selects the first unseen natural member,
then other members append, preserving the prior deterministic ambiguity policy.
No grouping redesign, menus, visuals, agent policy, nativeMenu, store or types
changes. Local rows without explicit workspaceId keep their existing group/path
ID shape; qualified rows keep the existing group/member/path ID shape.

## Registered regression and actual execution history

Every test invocation was exactly:

```sh
bun run --cwd ui test src/components/Sidebar.sortableIdentity.test.tsx
```

This package script executes `vitest run --maxWorkers=1` (not Bun's test runner).
Four invocations, in order:

1. Production RED: exit 1; 1 file failed, 2 failed / 1 passed (3 tests).
   Both registration and keyboard tests fail the intended assertion
   `expect(new Set(ids).size).toBe(3)`: received 2, expected 3.
   Legacy path-only reading passed. Before production edits, diagnostics found
   an extra repoPath test-fixture property; it was removed without changing any
   assertion.
2. After production fix: exit 1; 1 failed / 2 passed. Registration passed.
   Keyboard displacement expected `translate3d(0px, 40px, 0)` but received only
   `transition: transform 200ms ease;`. This was a fixture geometry defect:
   DragOverlay had the default header rectangle instead of its source rectangle.
3. Instrumented diagnosis: exit 1; same 1 failed / 2 passed. Real hook outputs
   showed activeIndex 2 and overIndex -1 with correctly spaced member rectangles.
   Removed temporary logging/output instrumentation and made overlay measurement
   use the active row's deterministic rectangle. No production change for this.
4. GREEN: exit 0; 1 file passed, 3 passed / 0 failed; tests 109ms, total 1.28s.
   All original assertions are unchanged. No retries to obtain a lucky pass.

Tests retain actual Sidebar, grouping, WorktreeList, DndContext, SortableContext,
KeyboardSensor, collision detection, transforms, storage and remount behavior.
The sortable module wrapper calls the real hook and only exposes its outputs as
DOM attributes. Nonessential host switcher/notification UI and host lookup are
stubbed; identity and ordering are not mocked. Three rendered rows include two
SSH members on distinct hosts with identical /repo paths, plus a Windows-style
local root. Assertions cover unique context-matching registration IDs, exact
nonnegative indexes, distinct drag data, keyboard sibling displacement, intended
member order, saved IDs, exact remount order and legacy path-only reads.

Keyboard readiness subscribes to the sensor's actual deferred document keydown
registration before activation, with a bounded failure timeout. No polling,
waitFor, fixed sleeps or arbitrary clock advancement. Drop storage and React
commit are checked after the synchronous event/act boundary. afterEach unmounts,
clears fixture storage and restores spies; keyboard subscription timeout/spies
are disposed in finally. No desktop, daemon, network, SSH or child runtime was
started. No branch/worktree, commit, remote action, build or broad test suite.

## Diagnostics and checks

- `functions.lsp_diagnostics` severity=all on Sidebar.tsx: no diagnostics.
- Same on WorktreeList.tsx: only pre-existing TS6385 hint at 44:69 for
  navigator.platform; no errors/warnings. That line was not changed.
- Same on the new test after fixture type correction: no diagnostics (before
  overlay-geometry correction). Two final requests timed out at 3000ms, so a
  TypeScript compiler API check was used rather than claiming fresh LSP success.
- `git diff --check -- ui/src/components/Sidebar.tsx ui/src/components/WorktreeList.tsx`
  exited 0 with no output.
- Final compiler command (exit 1):

```sh
bun -e 'const ts = require("./ui/node_modules/typescript"); const config = ts.readConfigFile("ui/tsconfig.json", ts.sys.readFile); const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, "ui"); const program = ts.createProgram(parsed.fileNames, {...parsed.options, noEmit:true}); const owned = ["/components/Sidebar.tsx", "/components/WorktreeList.tsx", "/components/Sidebar.sortableIdentity.test.tsx"]; const diagnostics = ts.getPreEmitDiagnostics(program).filter(d => d.file && owned.some(p => d.file.fileName.endsWith(p))); console.log(ts.formatDiagnosticsWithColorAndContext(diagnostics, {getCanonicalFileName:p=>p, getCurrentDirectory:ts.sys.getCurrentDirectory, getNewLine:()=>"\n"})); console.log(`Owned-file TypeScript diagnostics: ${diagnostics.length} (3 files)`); process.exit(diagnostics.length ? 1 : 0);'
```

Output: 1 diagnostic across 3 owned files: existing Sidebar.tsx:613:79 TS2550,
`Property 'at' does not exist on type 'string[]'` (target lib predates ES2022).
The unchanged project-label `.at(-1)` was present in the initial full read and
is outside this repair. No diagnostics in the new test or changed code.
Not suppressed or repaired. Lead owns combined build/typecheck verification.
An initial read-only `find .. ...` discovery command timed out after 10 seconds;
subsequent bounded reads succeeded. No persistent fixture files to remove.

## Exact remaining Windows runtime limits

Executed on Darwin arm64 in Vitest/jsdom with explicit non-Mac Sidebar props,
not a Windows WebView or current Windows debug desktop. Still required on the
owned Windows debug runtime: grouped member checkouts including equal absolute
paths on two distinct SSH hosts; pointer drag AND keyboard reorder of the
intended member; actual sibling displacement/geometry, correct selected member
and persistence after reopening. This local fixture does not establish Windows
native keyboard routing, pointer hit testing, rendering/animation, actual host
checkout availability, OS storage persistence, native menu/clipboard behavior,
package provenance or desktop/daemon behavior. No runtime acceptance claimed.
All four owned files remain uncommitted and subject to concurrent shared-tree
changes; lead must independently inspect and verify the combined batch.
