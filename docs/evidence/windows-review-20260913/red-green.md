# Windows remediation RED/GREEN evidence

INCOMPLETE. Transport has macOS RED and corrected-oracle GREEN; startup
focus has source-repair RED/GREEN and coordinator integration. Three
loader-only mutations are rejected. Windows and aggregate acceptance remain pending.

## TRANSPORT-TEST-01: existing expectation mismatch

- Date: 2026-09-13; runner start clock 12:40:33.
- Host: darwin arm64; Vitest 3.2.7 through the repository's Bun script.
- HEAD: `ab08b94fdeda5039982fc8a37e8bc36885426667`, foreign-dirty tree.
- Exact command:
  `bun run --cwd ui test src/lib/terminalTransport/terminalTransport.test.ts`.
- Prior registration: C002, 2026-09-13T03:30:46.402Z.
- Monitor: `mon_H54EJAPQA1W10Z7Z`; session `bash_3`.
- Exit: 1; 1 test file failed; 1 test failed and 7 passed, 8 total.
- Duration: 732ms. Not a missing-dependency, compile or zero-test failure.

The completed process output was read directly. Decisive output:

```text
FAIL src/lib/terminalTransport/terminalTransport.test.ts > TerminalTransport abstractions > TauriTerminalTransport listSessions queries tauri listTerminalSessions
AssertionError: expected [ { sessionId: 'sess-100', ...(3) } ] to deeply equal [ { sessionId: 'sess-100', ...(2) } ]

- Expected
+ Received
  [
    {
      "daemonEpoch": null,
+     "running": true,
      "sessionId": "sess-100",
      "worktreePath": "/repo/path",
    },
  ]

src/lib/terminalTransport/terminalTransport.test.ts:30:22
Test Files  1 failed (1)
Tests       1 failed | 7 passed (8)
error: script "test" exited with code 1
```

ASCII ellipses above normalize the runner's abbreviated-object typography.
The difference is the pre-registered missing `running` field, not an
inferred Windows wheel failure. The real mapper ran against a mocked
session-list IPC response. All invoked write/close/signal/listener IPC
boundaries were mocked; the WebSocket test only constructed an adapter.
No desktop, daemon, SSH connection or branch/worktree was started.

## Preservation and outstanding checks

Before and after the run, full `git diff --binary` strings were equal and
full `git status --porcelain=v1` strings were equal. No tracked change or
new untracked path appeared. The monitor reported process exit 1.
Dependency caches were not inventoried; do not treat these observations as
the task-wide cleanup receipt.

Source SHA256 receipts are in `transport-source-closure.md`.
The initial RED run made no test edits. Every other finding's RED/GREEN
obligation remains open; this single case cannot satisfy C001, C002 or C003.

## TRANSPORT-TEST-01: corrected oracle GREEN

The test file had no foreign diff before editing. The repair adds the
missing `running: true` expectation and an explicit exited-session fixture
with `running: false`, distinct path and non-null daemon epoch. The real
mapper is unchanged. Full object equality remains; no assertion was
removed or weakened. This is a test-contract repair, so the expectation
necessarily differs from the stale assertion in the initial RED.

- Same command as RED, single run after the test-only edit.
- Monitor `mon_J2A61TTPF9JZVVY6`, session `bash_4`.
- Runner start 12:43:28; duration 550ms; Vitest 3.2.7.
- Full completed output read: 1 test file passed, 8 tests passed, exit 0.
- LSP: no diagnostics on the changed test.
- `git diff --check`: no diagnostics.

The fixture now distinguishes absent-running from explicit-false and
asserts session/path/epoch preservation. Subsequent mutation receipts follow;
native Windows execution remains unverified.
No branch/worktree or production file was changed. The test edit remains
uncommitted in the shared working tree and must be included in later
verified delivery without overwriting foreign changes.

## Loader-only mutation rejection

Both invocations were registered in C002 before the temporary config was
created. A first registration attempt was rejected by the CLI's literal
word heuristic; the accepted wording retains the original scenario and
adds stricter mutation requirements. No criterion status was changed.

```bash
FERRYX_TRANSPORT_MUTATION=drop bun run --cwd ui test src/lib/terminalTransport/terminalTransport.test.ts --config vitest.transport-mutation.config.ts
FERRYX_TRANSPORT_MUTATION=force bun run --cwd ui test src/lib/terminalTransport/terminalTransport.test.ts --config vitest.transport-mutation.config.ts
```

The temporary config spread the real Vitest configuration, retained its
plugins and added an enforce-pre transform targeting only the actual
`tauriTransport.ts`. Exactly one `running: s.running ?? true,` occurrence
was required. Mode drop substituted an empty string; mode force substituted
`running: true,`. Production source and test assertions were never changed
for these runs. Initial mergeConfig type diagnostics were corrected before
execution; the final temporary config had no LSP diagnostics.

- Drop: monitor `mon_A7EB2BH1TNZTW59S`, session `bash_5`, start 12:47:19,
  duration 782ms. `TRANSPORT_MUTATION_APPLIED:drop` appeared. The only
  failed case was listSessions at line 36:22: both running fields were
  absent from Received. Seven other cases passed; exit 1.
- Force: monitor `mon_EA17EY9AYD23Y9FP`, session `bash_6`, start 12:47:39,
  duration 647ms. `TRANSPORT_MUTATION_APPLIED:force` appeared. The only
  failed case was listSessions at line 36:22: sess-exited expected
  `running: false`, Received `running: true`. Seven other cases passed;
  exit 1.

Full completed output from both sessions was read. Neither failure was
setup, import or compilation failure. The owned temporary config was
deleted after both processes exited. The normal config never references
it. The earlier 8/8 GREEN uses unchanged production bytes and unchanged
corrected assertions, so no redundant normal test run is claimed.

## SHARED-NATIVE-02 / P34: startup focus reconciliation

Partial repair; full packet acceptance remains open.
Exact pre-registered command for all three runs:
`bun run --cwd ui test src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts`.

- Existing baseline: `mon_VX6EKEETHXSTGV1Y` / `bash_7`, 12:49:49,
  41 passed, two files passed, exit 0, 1.07s. Existing tests did not cover
  subscription order or an event arriving while the snapshot is pending.
- New regression RED: `mon_F0K2C04HM21W93AZ` / `bash_8`, 12:52:01,
  1 failed and 41 passed, exit 1, 1.11s. Exact failure at
  `nativeWindowFocus.test.ts:72:34`: expected onFocusChanged once,
  received zero. The snapshot query had started before subscription.
- Same-assertion GREEN after production fix:
  `mon_MNJPN76JNDX9FV80` / `bash_9`, 12:52:51,
  42 passed, two files passed, exit 0, 1.16s.

Full completed outputs were read. No process was a desktop or daemon.
Both owned files were clean before changes. The helper now awaits focus
subscription before querying and applies the initial query only if no
focus event has arrived. A subsequent query failure also preserves an
already observed event. No OS-specific input code changed.

The regression controls registration and query Promises, verifies duplicate
start creates one subscription, emits false while the query is pending,
then resolves the stale query true and requires false to remain. The
existing positive-focus event check now awaits exact subscription rather
than polling, resets module state between cases and cleans its DOM listener
in finally. Project-target-incompatible Promise.withResolvers was replaced
with an explicitly typed Promise helper before execution; no config or
type-error suppression was used. Both files have clean LSP diagnostics.

Remaining P34 obligations: controlled rejection/null fallback coverage,
both event directions, real coordinator decision wired to the same focus
state, mutation proving the stale-query guard, affected-entry integration
and actual Windows debug startup/bell/unread observations. The separately
passing 40 coordinator unit tests do NOT prove that integration. No full
P34 closure, Windows success, build pass, commit or delivery is claimed.

### P34 follow-up: real coordinator integration and mutation

The preceding remaining-obligations paragraph describes the first 42-test
increment. The next increment closes its local rejection, bidirectional
event, coordinator-integration and stale-query mutation gaps, not Windows QA.

- The original combined command ran once on the expanded tests:
  `mon_YDSHPBMXAMCJ1KQM` / `bash_10`, start 13:00:25, 876ms,
  10 focus cases plus 40 coordinator cases passed, exit 0.
- Four integration cases instantiate the actual NotificationCoordinator
  with getNativeWindowFocused as its focus source. Both event directions
  survive an opposite snapshot or snapshot rejection. Bell acceptance,
  unread marking and notification/sound invocation are asserted together.
  Only OS IPC and the Tauri window API are mocked.
- Three no-event cases accept true/false initial snapshots or retain null
  after rejection. Subscription rejection leaves null without a query.
  Controlled Promises establish event ordering; no sleeps or polling.
- The pre-registered mutation command was
  `bun run --cwd ui test src/lib/nativeWindowFocus.test.ts src/lib/notificationCoordinator.test.ts --config vitest.focus-mutation.config.ts`.
  An enforce-pre loader transform required exactly one successful-snapshot
  guard, then replaced only that guard with unconditional assignment.
  It retained the normal Vitest config and plugins.
- Mutation `mon_Y5DQ4YB56BPNWERG` / `bash_11`, runner start 13:00:54,
  1.05s, emitted `FOCUS_MUTATION_APPLIED:stale-snapshot` and exited 1:
  exactly 3 failed, 47 passed. The pending-snapshot test read true instead
  of false at line 86. Both integration directions failed unread counts
  at line 132 (expected 1/got 0 and expected 0/got 1). The unrelated
  coordinator file still passed all 40 cases.
- Completed output was read in full. The temporary config was deleted
  after exit and existence=false was checked. Production source SHA256
  remained `63d1d65acc7e0804f610acb127e14d74ac0d9e48f9e1eebb894ec9e35e0c6650`
  before and after mutation. No production file was temporarily changed.
  Foreign tracked binary diff, excluding the three owned source/test
  paths, matched the pre-increment baseline exactly.

Test/config LSP diagnostics were clean. This demonstrates library-level
integration, not actual App wiring, Windows focus timing, audible delivery
or a full build. Native Windows acceptance and aggregate gate remain open.

## P28 Opera: lead-independent old-source reproduction

Lead read all three claimed artifacts, the real PairingPage caller and
production diff. Both TypeScript files had clean lead LSP diagnostics.
The child's reported 4-failed/4-passed RED and 8-passed GREEN remain
attributed to p28-opera-local-repair.md until independent normal verification.

The pre-registered lead command was:
`bun run --cwd ui test src/remote/deviceIdentity.opera.test.ts --config vitest.opera-mutation.config.ts`.
The loader required exactly one Opera and Safari branch and moved Opera
back after Safari, reproducing the original code without changing source.

- mon_JQPJY649YYR76S5R / bash_12, 13:10:20, 879ms, exit 1.
- Sentinel: OPERA_MUTATION_APPLIED:original-precedence.
- 4 failed / 4 passed, 8 total; all completed output read.
- Parser: expected Windows - Opera, received Windows - Chrome at line 42.
- Mounted form initial display was Chrome at line 67.
- Unedited and blank fallback actual POST JSON submitted Windows - Chrome,
  not Windows - Opera, at line 89. Edited name was preserved; its failure
  was only the independently checked initial suggestion.
- Edge and Chrome parser and mounted-form controls passed.

No timeout, setup or compiler failure caused RED. The temporary config
was deleted after exit and checked absent. Source SHA256 remained
267706caa9eb6c7dc9c6d317df089fe8f48b0d44c70aecf6d4d580107956db74.
Normal lead GREEN is pending the registered combined batch run after P04.
Actual Windows Opera display/submission and aggregate acceptance remain open.
