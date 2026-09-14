# A04 parent acceptance

Accepted at the scoped shared headless service boundary, after generation 5
independent review and a fresh parent execution on the final eight source hashes
in `A04-parent-green-result.json`. This is not acceptance of A05-A24, native
paired-host rendering, Windows execution or AC01-AC12 in full.

The parent reconstructed both node prompts, read their actual implementation and
independent reports, checked all changed source bodies and the complete extraction
diff, and found no unapproved production scope. The eighth file is the explicitly
documented existing SSH QA wire-contract repair. Historical rejected reports and
failed runs remain evidence, not current verdicts.

The parent's final exact invocation from the wave1 root was:

```sh
env -u A04_PRIVATE_ROOT -u A04_SPLIT_AUTHORITY -u A04_INJECTION CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib daemon::server::a04_shared_services_tests::a04_shared_authority_runtime -- --exact --nocapture
```

It exited 0 with one outer test and its child passing.
`A04-parent-final-runtime.log` records owner PID 41995, original PTY PID 42033,
session `bd2b80f4-6273-4abf-8d3c-46ceb8e5d882`, IPC and OS cwd
`/private/tmp/a04-LpPel2/project`, and HTTP `127.0.0.1:49965` returning that
session/workspace with status 200. The backend and session authority pointers
match; the fixture also asserts workspace authority identity. Both listeners
joined, the original PTY and child were reaped, and there were no cleanup errors.
The parent then directly confirmed both numeric PIDs and the private root absent.

Earlier parent runs cover focused construction/drop and six cleanup injections
(4 top-level tests), Local sessions (34), remote (212), SSH survival (4), handover
(3), actual isolated OpenSSH/PTY/gateway transport (1 outer test), and headless
CLI/relay compilation. Exact logs and source chronology are retained in the
parent GREEN receipt. The controlled split-authority and two admission mutation
REDs failed at their intended assertions and were restored before extraction.
No source mutation remains.

The existing 16 test-build warnings and 17 headless-check warnings remain
unsuppressed. Empty-construction drop and acknowledged-PTY cancellation checks
do not establish every live task lifetime or cancellation during blocking spawn.
No desktop or canonical daemon was launched. Future machine APIs remain gated.
V05 is still unaccepted in its separate worktree.

The extraction is ready for its authorized local atomic commit. No push, merge
or deployment is authorized by this acceptance.
