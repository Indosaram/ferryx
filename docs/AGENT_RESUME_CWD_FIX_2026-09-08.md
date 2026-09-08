# OMO session restore CWD and confirmation repair

## Changes

- The Ferryx agent-state extension now reports the session manager's
  `getSessionFile()` result as `providerSession.transcriptPath`, alongside the
  agent-generated session ID. Existing protocol and persistence fields carry it;
  no daemon protocol bump or new session ID is involved.
- `terminal/resume_cwd.rs` resolves OMO's original CWD from the exact transcript
  header before a reconnect spawns the process. It verifies the header type, ID
  and absolute CWD, with a bounded header read.
- Legacy ID-only snapshots are repaired by exact-ID lookup in the configured
  agent session stores, default OMO stores and named OMO profiles. Missing,
  mismatched or conflicting metadata fails rather than guessing another project.
- `daemon/server.rs` runs the new disk lookup through `ipc::run_blocking`. The
  resolved CWD still passes the existing workspace/worktree jail before spawn.
  The daemon returns the corrected CWD, and the existing frontend rebind and
  persistence flow saves it without changing the stable workspace/worktree root.
- Other agents retain their existing directory resolution behavior.

The independent OMO confirmation defect was fixed in the Senpi source worktree
`/Users/indo/code/senpi-ferryx-confirm-fix`:
`packages/coding-agent/src/main.ts` settles the answer before `rl.close()` can
emit the EOF/cancellation event. The same two-line ordering correction was
applied to the installed global Senpi `dist/main.js`.

## Verification on macOS

| Gate | Result |
| --- | --- |
| Senpi confirmation regression before the fix | 4 affirmative cases failed; 5 negative/EOF cases passed |
| Senpi confirmation regression after the fix | 9 passed |
| Ferryx transcript reporting before/after | Missing `transcriptPath` reproduced; real loopback TCP test passed after fix |
| Ferryx CWD resolver before the fix | 3 failed |
| Ferryx resolver and workspace-jail tests after re-review | 10 passed, including FIFO rejection and a real process CWD check |
| Existing production spawn CWD validation | 1 passed |
| Related frontend session suites | 81 passed across 5 files |
| Senpi root TypeScript check | Passed |
| Ferryx frontend build and final TypeScript check | Passed |
| Ferryx default-feature debug executable build | Passed |
| Senpi CLI smoke using `/opt/homebrew/bin/node` | 8/8 passed; real auth unchanged |
| Source and installed OMO session bootstrap in real PTYs | Each: `y`/`yes` fork, `N`/Enter/EOF cancel, matching project resumes exact original ID |
| Isolated Ferryx daemon and real PTY child | Both transcript-backed and legacy ID-only requests launched in the nested CWD with the exact resume argv and unchanged workspace |

The daemon QA uses a temporary HOME, `FERRYX_RUNTIME_DIR`, `FERRYX_SESSION_DIR`,
and a tiny local agent executable that reports its actual CWD and argv. It does
not connect to or restart the user's daemon. OMO's own bootstrap is verified
separately using the installed/source implementation, real readline and a real
PTY, with isolated session files and no provider calls.

Repeatable commands:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib resume_cwd
cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::server::tests::test_server_spawn_cwd_validation
bun run --cwd ui test src/lib/sshAgentState.test.ts src/lib/agentReconnect.integration.test.ts src/lib/agentReconnect.test.ts src/lib/agentResume.test.ts src/lib/sessionPersistence.test.ts
bun run --cwd ui build
cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx
bun scripts/qa/verify-ferryx-resume-cwd.mjs src-tauri/target/debug/ferryx
bun scripts/qa/verify-omo-fork-confirmation.mjs /absolute/path/to/senpi/dist/main.js
bun scripts/qa/verify-omo-fork-confirmation.mjs /absolute/path/to/senpi/packages/coding-agent/src/main.ts
```

## Pre-commit re-review

Two availability defects were reproduced and corrected during the requested
second review:

- A FIFO supplied as a transcript was accepted if it delivered a valid header,
  and could block the global spawn lock if no writer connected. The regression
  failed before the fix. Unix now opens nonblocking and checks the opened file's
  type; non-Unix targets also reject non-file paths before opening.
- A regular file in `.omo/profiles` was treated as a profile directory. Adding
  a `.DS_Store` fixture caused the real daemon QA to fail with
  `agentResumeInvalid: Not a directory (os error 20)`. Profile enumeration now
  includes directory entries only. Both real-daemon restore scenarios pass
  with the metadata-file fixture present.

After these corrections, the 10 Rust regressions, debug executable build and
isolated daemon QA passed. The Senpi confirmation regression was also rerun:
9 passed. Previous frontend and OMO real-PTY evidence remains applicable because
those production paths did not change during re-review.

The independent final reviewer returned **APPROVE**, with no blocking findings.
The full verdict and its nonblocking evidence limitations are preserved in
[`AGENT_RESUME_CWD_REVIEW_2026-09-08.md`](AGENT_RESUME_CWD_REVIEW_2026-09-08.md).

## Limitations and rollout state

- LSP diagnostics were attempted but the shared LSP daemon was unreachable.
  Compiler/type checks were used; no clean LSP result is claimed.
- `--no-default-features` hit existing missing native-terminal Tauri command
  macros. The supported default-feature tests and executable build passed.
- The new Senpi worktree initially lacked package-local dependencies. Reusing
  existing local dependency directories resolved its typecheck errors. The
  default Node launcher also failed CLI smoke; explicit system Node passed.
- No Windows or Linux execution was performed. The product resolver uses
  portable filesystem APIs and supports HOME/USERPROFILE; the standalone daemon
  QA driver currently uses the Unix socket.
- The running release Ferryx app and its daemon were not replaced or restarted.
  Ferryx's source/debug executable is fixed, but the active release process
  still needs a normal, session-safe rollout.
- New OMO processes load the installed confirmation fix. An already-running
  process may still have its old module in memory. A future global package
  update can overwrite the local installed patch until the source fix ships.
- Commit scope is restricted to these session-restore changes and their tests,
  QA drivers and evidence. Unrelated shared-tree changes are excluded.
- No PR, package publication, live session-state rewrite or release deployment
  was performed. The Senpi source change is maintained in its dedicated worktree.
