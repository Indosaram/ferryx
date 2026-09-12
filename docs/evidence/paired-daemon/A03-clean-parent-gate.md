# A03 clean candidate: parent execution gate

Candidate: `herdr-cloud-wave0-clean`, based on `cd16c90`.
Date: 2026-09-12.

## Outcome

The candidate builds independently of the foreign overlay and all 18 focused
A03 tests pass, including the freshly built owner CLI against a private daemon
and relay. The full required headless remote regression is NOT green:
196 tests pass and five existing grid tests fail. An untouched `cd16c90`
worktree independently reproduces the identical five failures (179 pass).
These failures are not introduced by A03. V04 tracks their repair separately;
this report does not accept the whole Wave 0 or the overall objective.

## Scope and ownership

Eleven Rust files implement owner-issued Mirror/Machine grants, authenticated
capabilities, typed admission errors, and their tests. The clean producer
manifest and independent report enumerate those files and SHA-256 values.
The parent compared every hash before execution and after completion: all
eleven match. Parent diff inspection confirms the base three-field pairing
response, three-element client result and original state constructors.

Foreign relay URL response changes, terminal service sharing, auto-spawn,
grid implementation changes, UI and release changes are excluded. Existing
tests are retained. Historical failing-first and mutation evidence lives in
`overlay/`; its README explicitly limits that evidence to its original context.

## Actual clean execution

- `A03-clean-build-prerequisite.log`: first invocation fails because generated
  `ui/dist` does not exist. This is not a behavioral RED.
- `A03-clean-ui-build.log`: candidate `bun run --cwd ui build` passes, 1883
  modules, exit 0. Dependencies and Rust caches are private APFS copies, not
  shared mutable targets. No desktop or application bundle is launched.
- `A03-clean-runtime.log`: CLI/relay headless check and fresh CLI build pass.
  `cargo test --locked --manifest-path src-tauri/Cargo.toml
  --no-default-features --lib a03_ -- --nocapture` passes all 18 tests.
  The subsequent complete `remote::` run fails five tests, enclosing exit 101.
- `A03-clean-baseline-remote.log`: identical remote command on unchanged
  tracked source at `cd16c90` reproduces the same five failures, exit 101.
- A separate parent `git diff --check` passes. The chained check did not run
  after the failed full suite.

The fixture demonstrates mirror and machine owner CLI issuance, returned
scope, Control permission, stable machine identity, UDS protocol 3 and legacy
Ping, authenticated HTTP capabilities, anonymous/malformed/revoked denial,
zero identity probes after revocation, Mirror machine-route denial and
unavailable machine services. Capabilities advertise no unimplemented services.
The capabilities requests use the direct private gateway; this is not proof
of the later forced-relay capability route or remote execution.

## Cleanup and limits

Fixture PID 63655 and CLI PIDs 63676 and 63891 exit and are reaped. The fixture
reports runtime shutdown, socket removal and private-root removal. A parent
`ps -p 63655,63676,63891 -o pid=,command=` returns no rows.

The owner CLI fixture isolates HOME/data/runtime in a subprocess. The full
remote suite was run with the normal toolchain HOME, not an additional
suite-wide private HOME; its existing per-test isolation is not strengthened
by this report. Existing warnings are retained in complete logs. Independent
review must distinguish source review from commands actually run by that
reviewer.

No canonical daemon was restarted, no user PTY was intentionally controlled,
and no deployment, push or merge was performed. The baseline comparison
worktree and private build caches remain local QA resources pending cleanup.

## Remaining gate

Complete the updated independent A03 review, then commit only the scoped A03
increment with the known baseline regression disclosed. V04 must repair and
verify the mandated headless grid suite without skipping or weakening tests.
A04-A24 and the full acceptance matrix remain outstanding.
