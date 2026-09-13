# Parent cleanup ledger (owned QA resources)

The stop condition requires that QA resources this session owns are cleaned up,
and that the user's canonical daemon, PTYs, desktop and foreign changes are left
untouched. This ledger tracks both halves.

## Untouched, verified alive (must stay that way)

Checked with `ps aux`:

```
PID  680  /Applications/Ferryx.app/Contents/MacOS/ferryx            <- user's GUI
PID 1010  /Applications/Ferryx.app/Contents/MacOS/ferryx --daemon   <- canonical daemon
```

Both are running and were never restarted, killed, replaced or upgraded by this
session. The daemon owns every live PTY; terminating it would destroy the user's
active terminal sessions and agent workflows. No packet was permitted to touch
it, and none did.

Other `ferryx`-matching processes seen during the audit were `rustc` / `sccache`
compile jobs belonging to the in-flight packets, not daemons.

## Owned temp roots - RETAINED ON PURPOSE, not yet deleted

| path | size | last modified |
|---|---|---|
| `/tmp/ferryx-herdr-q4-linux-01a097f8` | 342 MB | 2026-09-13 10:25 |
| `/tmp/ferryx-herdr-q4-windows-01a097f8` | 1.8 GB | 2026-09-13 11:32 |

These carry this session's worktree id (`01a097f8`) so they are ours, not another
session's. They hold the Q4 cross-platform build outputs and capture logs
(`build.log`, `capture.log`, `capture.py`, `candidate-aggregate*.log`,
`candidate-baseline`, ...).

They are deliberately **NOT deleted yet**, because these Q4 repair items are still
open and would otherwise have to rebuild ~2.1 GB from scratch:

- `Q4 repair: prove Linux readiness signal framing`
- `Q4 repair: resolve native Windows platform failures`
- `Q4: verify frozen backend on Linux` / `on Windows`

Deleting them now would be premature cleanup that destroys still-needed work.
They must be removed once those items close, before the final report.

The durable evidence itself already lives in this directory (for example
`Q4-windows-receipt.ps1`,
`Q4-windows-platform-native-candidate-ready-relay.log`,
`Q4-windows-platform-native-candidate-close-ssh.log`), so removing the temp roots
later will not invalidate any captured proof.

## Not ours - leave alone

`/tmp/ferryx-kitty-abi`, `/tmp/ferryx-kitty-abi.rs`, `/tmp/ferryx-kitty-evidence`
carry no session id and were not created by this work. Another session owns them.
Do not delete.

## Child-created fixtures

Each packet was required to remove its own temp roots, sockets, PTYs and fixtures
and to assert the cleanup. The landed reports (A08, A09, A10, A12, A13, A14, A15,
A20, A22) each state their cleanup. Remaining lanes must do the same before the
final report is written.
