# Full UI verification after selector dependency repair

Status: incomplete aggregate acceptance. All 219 files finished; this is not
the earlier 300-second interrupted run.

Command: `CI=1 bun run --cwd ui test`

Monitor mon_6TQT1KKGVB9PQ546, bash_60: exit 1, 114.46 seconds.
Result: 212 files passed, seven failed; 2,439 tests passed, 10 failed.
The nwsapi override changed only the manifest override and one resolved
package from 2.2.27 to 2.2.24. The settings dropdown assertions and deadlines
were unchanged; their nine previous timeouts disappeared.

## Every observed failure

- App.remote.test.tsx:375: restored SSH session assertion expects spawning;
  the actual recovery path reaches an unmocked remote-status IPC. Requires
  reconciliation against the current process-preserving restore contract.
- App.remoteHostShortcuts.test.tsx:316: the local control fixture lacks
  mandatory sessionIdsByLeafId; close accesses leaf-1 in the absent map.
  Its remote suppression assertions must remain effective.
- appearanceThemeContract.test.ts:71: SshSection.tsx:623 still uses
  text-emerald-500 instead of the semantic success token. No correction yet.
- NativeTerminalPane.exitAttach.test.tsx:124 and :190: two assertions omit
  the actual callback's added binding identity.
- TerminalPane.exitAttach.integration.test.tsx:148: same missing callback
  binding identity at the parent boundary.
- SettingsDialog.test.tsx:614: existing Remote Access prose mismatch,
  independent of the selector fix. Preserved, not deleted or waived.
- features/ferryx/push/client.test.ts:5, :11 and :16: current client.ts is
  a stub returning null/enabled/disabled without implementing its contracts.
  LSP found only the test as a caller of PushClient and secureTaskLink.
  The file was last committed by another session in 831ae2c6. This is an
  existing unconnected feature, not a demonstrated Windows wheel regression.
  No implementation or passing result is claimed for it.

## Output retention and focused recovery

The large theme-source assertion exhausted the terminal output buffer.
full-ui-post-selector-retained.log preserves the exact remaining output,
including its explicit "earlier output dropped" marker. It is not a full
raw log. Final counts and seven named file results were observed separately
through monitor events.

The two missing App failure blocks were recovered with the exact focused
command registered in C002:

`CI=1 bun run --cwd ui test src/App.remote.test.tsx src/App.remoteHostShortcuts.test.tsx --reporter=verbose`

mon_5EM8F43ER4TFPM84 / bash_61 exited 1 with two failures and 21 passes in
3.72 seconds. The complete focused output was read. A disjoint worker owns
only these two fixtures and app-remote-suite-repair.md; production App edits
require a demonstrated defect and separate registration.

## Corrected binding assertions

After C002 registration, the lead added the exact callback binding argument
to the three stale assertions. Original backend/session/reason assertions,
the actual reducer exit transition, overlay and operational-error checks
remain intact. No production behavior or test deadline changed.

`CI=1 bun run --cwd ui test src/components/NativeTerminalPane.exitAttach.test.tsx src/components/TerminalPane.exitAttach.integration.test.tsx src/lib/nativeTerminalAttachPolicy.test.ts --reporter=verbose`

mon_1CJP04V7RZPAQZ4V / bash_62 exited 0 with all 55 tests passing in
3.19 seconds. Both edited files had clean LSP diagnostics. The three
logged IO_ERROR messages are deliberate operational-error test inputs,
not real daemon connection attempts.

## Remaining acceptance

The App fixture repair subsequently passed all 23 original scenarios in
2.76 seconds, exit 0. Lead reviewed the full diff and real sshRecovery
consumer: the updated test requires reattachment without replacement,
preserved target/backend identity, and explicit closePane suppression while
remote is active. See app-remote-suite-repair.md for raw results.

The combined `bun run --cwd ui build` completed in bash_63 /
mon_MY8RTW6VCAQ88ZSR with exit 0. TypeScript succeeded; Vite transformed
1,892 modules and built in 2.18 seconds. The existing App chunk warning
remains visible (506.68 kB, above the 500 kB advisory); no threshold was
raised. This build occurred after dependency and fixture corrections, not
after any future theme or backend repair.

The full suite remains red. Native Windows input, PTY survival, remaining
backend repairs, final source qualification, cleanup, gate, PR dispositions,
atomic commits and verified origin/main delivery are still required.
