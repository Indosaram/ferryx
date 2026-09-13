# DAG PR2 native pointer delivery handoff

2026-09-13, task st_01a099f6. **BLOCKED at Windows execution, not complete.**
For the lead and Windows runtime owner. No production/test edits, branch or
worktree operations, desktop launch, commit, push, or GitHub writes occurred.

## Fresh external and local state

Executed from `/Users/indo/code/project/orca-lite` on macOS arm64:

- `gh pr view 2 --json state,headRefOid,mergeStateStatus,statusCheckRollup`
  exited 0: OPEN, head `79b02ab6ea4753bf87080dd567368a870c018057`,
  mergeStateStatus UNKNOWN, statusCheckRollup `[]`.
- `gh pr diff 2` and `gh pr diff 2 --name-only` exited 0. Exactly
  `src-tauri/src/native_terminal/platform/windows.rs` and new sibling
  `windows_pointer_tests.rs`. Full patch read, not just its file list.
- `gh api graphql` with repository Indosaram/ferryx, pullRequest(number:2),
  reviewThreads(first:100), resolution/comment author/body/url and pageInfo
  exited 0. Independently repeated count query returned totalCount 0 and
  hasNextPage false. No unresolved discussion to resolve or additional page.
- `gh pr checks 2` exited 1 with `no checks reported on the
  'fix/windows-drag-interactions' branch`. No running check exists to
  subscribe to; this is not a CI pass. UTC command: 2026-09-13T08:53:26Z.
- `git rev-parse HEAD` exited 0:
  `da6eec06d65551f67bbc43f09910cde470c3478d`.
- `git diff -- src-tauri/src/native_terminal/platform/windows.rs` exited 0
  with no output. `test -f .../windows_pointer_tests.rs` exited 1 (absent).
  Initial status showed neither PR2 path dirty. Numerous foreign files
  are dirty, including P15 Cargo.toml and remote/state.rs; none were edited.

## Actual missing source delta

Current constructor still passes `WS_CHILD | WS_CLIPSIBLINGS` to
CreateWindowExW. Reveal shows and raises that renderer above its input
sibling. WNDPROC answers HTTRANSPARENT, whose thread-local delegation does
not establish cross-thread WebView2 delivery. The public descriptor nevertheless
advertises pointer_transparent=true.

PR2 adds WS_DISABLED (0x08000000) to the **normal** style, extracts the existing
allocation unchanged into from_parent_hwnd for the production constructor
and regression, updates the invariant comment, and includes the new test.
No wheel normalization, frontend handler, focus hook, IPC, or capture change
is in this PR. The narrow fix remains appropriate; no additional source
defect in this patch was established by this refresh.

The test constructs the real compositor, reveals it over a STATIC input
child, and queries WindowFromPoint from another thread on an owned hidden
desktop. Both HWNDs themselves belong to the owner thread. It checks visible
renderer state and exact input HWND equality. It does not dispatch native
input, create WebView2, draw actual terminal pixels, or prove selection.

Existing pr-review.md, audit-native-input.md, wheel-regression-seam.md and
acceptance-status.md were reread. Their P01 missing-execution assessment
still holds. Completed P02 normalization was not repeated or modified.

## Registered exact next executable proof

The installed official toolkit exports executeAgentToolkit from
`/Users/indo/.bun/install/global/node_modules/omo-ai/plugin/extensions/omo-agent-toolkit.js`.
Invoked via Bun, using resolveSessionId bound explicitly to original session
`01a0983f-c995-753d-afa9-593f6d118788` and resolveCwd to this repository.
Operation steer / kind annotate_ledger exited 0, returned ok=true,
accepted=true, no rejected reasons. It registered the following command,
unchanged oracle, old/new styles and runtime limits before any repair.
It did not mark a criterion passing or hand-edit goal/ledger JSON.

On the allocated Windows source tree, stage the PR's constructor extraction,
test module declaration and identical test, retaining the original enabled
style for RED. Execute through the lead's long-run monitor with a bounded
process deadline:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::platform::windows::pointer_tests::presented_terminal_yields_cross_thread_pointer_hit_testing_to_input -- --exact --nocapture --test-threads=1
```

Require exactly one discovered/executed test. Intended RED is the final
post-reveal HWND equality failure, not compilation, desktop allocation,
timeout or zero tests. Then add only WS_DISABLED to production creation
style and run the **identical command and assertion** for GREEN. Record
source hashes before each execution. The query thread join is unbounded
inside this existing PR fixture: the outer monitor must bound the run.
Run existing native lifetime tests and native build afterward; they are
additional obligations, not results obtained here.

Do not invent a portable style-bit assertion as a replacement. A macOS
cargo invocation excludes this cfg(target_os="windows") module. Installed
Windows Rust targets (observed via rustup target list --installed, exit 0)
do not provide a Windows runtime, user32 desktop, or WebView2 delivery.

## Native delivery boundary for the Windows owner

The same-assertion HWND run is only a prerequisite. Real-surface acceptance
requires an owned current-source interactive Windows debug application,
launched **only with `bun tauri dev`**, and source/binary identity receipts.

Before physical input, subscribe to DOM pointerdown/move/up/cancel plus
actual backend selection/release completion for an owned pane containing
a unique known terminal-output sentinel. Use native SendInput down/move/up
over the visible rendered sentinel, not dispatchEvent or an IPC mouse call.
Await the exact subscribed release and read the selected terminal text;
assert the intended sentinel, ordered delivery, no stale dragging, and
unchanged sibling session. Capture visible terminal pixels and action logs.
Repeat the same native action/oracle with the old production style and the
corrected style. Missing event deadlines are failures, never polling luck.
Record release-outside-root/cancel and re-entry separately for capture
acceptance; WindowFromPoint cannot close those requirements. Wheel and OLE
drop remain separate acceptance, not inferred from drag success.

No executable native drag observer was added: that requires the actual
allocated GUI/session and agreed observation endpoint. Any new fixture or
literal runtime invocation must be registered before its associated fix.

## Exact dependency and preservation

This child has no monitor, task_send, computer-use or Windows execution
tool. It can run short local bash commands but must not bypass the required
monitor for Windows builds/runs. Runtime owner is st_01a099f8; PR3 owner is
st_01a099f7. No isolated old/new Windows tree or interactive session was
acquired by this child, and no fix was applied ahead of the missing RED.

User authorization clarification: reversible source synchronization into an
EXISTING proven QA-owned isolated checkout needs no separate approval.
Runtime owner can execute the registered seam there now after proving
ownership and preserving unrelated bytes. Only branch/worktree creation or
deletion remains pending; main must not switch. This is not a blanket block
on Windows execution. Record destination, before/after source hashes and
restore receipts; transfer only the two PR2 paths at the inspected PR head,
retaining the old style for RED before adding WS_DISABLED for GREEN.

Runtime owner supplies monitored command execution and the actual WebView2
observation surface described above; no mocked geometry substitutes for it.
User reports P15 local GREEN finished and its exclusive reservation of
/private/tmp/p32-adjacent-st_01a099cd-ZyJhzW/target is now released. P15 is
no longer a build blocker. Coordinate exclusive use with any current Cargo
consumer before acquiring this shared target; release is not ownership by
this child. This child has not launched Cargo or acquired a build lock.
Direct task_send to st_01a099f8 is unavailable in this child's exposed tools;
the shared report is the handoff, not a claim that a task message was sent.
Reuse windows-preflight-current.md as preservation history, not current
PID/port allocation: verify installed app and both user daemons anew and
leave them untouched. Do not reuse the old installed binary or launch it.

The handoff is the next registered executable boundary and exact dependency,
**not implementation delivery, RED/GREEN, native drag proof, or PR disposition**.
Only this report and the toolkit ledger annotation were authored.
