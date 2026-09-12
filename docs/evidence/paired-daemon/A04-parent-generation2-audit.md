# A04 generation 2 parent audit

Verdict: NOT ACCEPTED. The DAG returning two completed nodes does not
establish A04 completion.

## Scope reconstructed

The producer owes shared headless workspace/session services in the six
approved production paths, with spawn, canonicalization and provider claims
moved rather than duplicated. IPC and HTTP must use the same authority.
Service-less constructors must fail closed, and ownership must not require
an AppHandle or form a strong cycle. Required proof includes split-authority
and admission-guard RED, restored GREEN, real PID/CWD identity, focused and
remote tests, CLI/relay checks, cleanup, source hashes and an implementation
report. The independent node owes an artifact-based review and safe runtime
verification after the candidate and parent execution evidence exist.

## Actual inspected state

- Read both complete node prompts, the complete 143-line fixture, the new
  guard command request, the current server registration/constructor region,
  and the generation 2 independent report including its historical appendix.
- Git status reports only the server test-module registration as a tracked
  source modification. The fixture and evidence files remain untracked.
  The server diff contains four added registration lines.
- The guard request explicitly says production was not edited and marks
  cleanup hardening as a prerequisite. It contains proposed mutations, not
  execution evidence.
- The fixture still has fallible setup after IPC startup outside its cleanup
  boundary, early-return cleanup operations, and wrapper-only timeout kill.
- No service extraction, implementation report, admission mutation results,
  restored GREEN, final binary checks or complete cleanup proof was supplied.
- The independent report correctly returns NOT ACCEPTED and does not claim
  to have run new Cargo checks.

No new runtime tests were launched for this audit. The previously observed
split-authority RED is retained without rerunning it. The unsafe cleanup
precondition prevents treating the proposed mutation command as ready.
No out-of-scope source change was found in the inspected diff.

## Recovery

The parent sent a precise correction to A04 in run
`dag_ff4ef1e2-a449-4a16-abba-0a0489d3a4c8`. The tool refused continuation with
`node_not_continuable` because the run had settled. The parent therefore
amended the same run to generation 3, preserving its key and full A04 scope.

The immediate prerequisite is test-only cleanup hardening and deterministic
failure/cancellation coverage, followed by a parent execution request at
`A04-cleanup-command-request-g3.json`. Monitor
`mon_45X1G6PB5HGB7W5F` watches that artifact. Production extraction and guard
mutations remain pending. Parent and child Cargo runs must not overlap.

A04 remains in progress. No A05-A24 or whole-plan acceptance follows from
this checkpoint.
