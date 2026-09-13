# A10 output budget parent review

Status: candidate source reviewed; runtime acceptance and socket integration open.

## Scope and source checks

The original packet owns an additive machine output subscription, its budget
tests, an owned real-PTY test, and a caller-contract report. It does not own
routed-owner plumbing, socket integration, controller cleanup, or reconnect
proof. The follow-up specifically owns teardown after a progress-future panic
and an injected failure proving that teardown, without production API changes.

The parent read the complete machine_output.rs, both test files, the caller
contract and its cleanup correction. The parent separately inspected the
output_hub.rs diff: module declaration, sender field/initialization, and fanout
for both gap boundaries and ordinary output are additive. Legacy broadcast
types and capacities are untouched by this diff.

All four current SHA-256 hashes match the worker's recorded candidate:

- src-tauri/src/terminal/output_hub.rs:
  2647eb6e9d80680669349e2b270eb08f1dcbc5f9e4856326fa9d6718bf2aabfc
- src-tauri/src/terminal/machine_output.rs:
  72ad1a9ca5bf874dab494af015751e565675372177e23836c237d69b9d7b15c4
- src-tauri/tests/a10_output_budget.rs:
  40148b7081f6933c4e43f2fcd48b9ebf2f72077b251bc97689b704cb7f4d8c04
- src-tauri/tests/a10_output_budget_pty.rs:
  672b3b2da135c1ae752a6870096989c37f90c3f0137723334063ef89dc9aef0d

The panic correction catches the bounded progress future, drops its output
receiver, attempts production session close and TempDir close, checks reaped
and reader-finished state, then either accepts the specific injected panic or
resumes an unexpected panic. This fixes the previously identified path where
an inner assertion could bypass teardown. It does not prove every conceivable
failure before the progress future is constructed.

Parent LSP diagnostics on the corrected PTY fixture returned no errors.

## Runtime blocker, not a passing result

The parent read the full cleanup attempt log. Despite the GREEN filename,
compilation failed with E0004 in daemon/client.rs:118: the request name match
does not yet cover MachineSessionDetail. Neither PTY test executed.
The parent independently read that current match and confirmed the missing
arm. The routed-owner worker is still active and has been asked to signal a
compilable increment; its files were not changed or reverted.

The owner subsequently supplied the two exhaustive client entries and read-only
retry classification. Parent inspected the resulting source and ran the focused
combined commands against composed code:

- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --test a10_output_budget --test a10_output_budget_pty -- --nocapture`:
  seven budget tests passed; ordinary PTY test passed with PID 62280,
  2097152 sibling bytes, peak charge 1047560, original session reaped and
  reader finished. Injected-panic cleanup FAILED with
  `Timed out reaping killed PTY session '5455c4a5-6328-44bf-aa9b-a446ebbd3217'`.
  Log: A10-budget-parent-composed.log, exit 101.
- `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib terminal::output_hub::tests`:
  13 passed, exit 0. Log: A10-hub-parent-regression.log.

The parent read both results, not only the monitor exit markers. The fixture
repair catches the panic but does not yet establish successful production
close under that failure. Worker st_01a0986b is diagnosing the actual reaper
path and checking owned leftover resources. No production lifecycle edit has
been authorized to that worker yet. Supervisor /tmp/herdr-journal-follow.SkBPnb
is retained pending that cleanup assessment. Do not accept A10 from the
passing budget cases or the normal PTY case alone.

The fixture now drains its sole PTY output receiver concurrently with
production close instead of dropping that receiver first. Parent read the
changed source and independently reran the two-test command:
A10-budget-parent-drain.log, exit0, two passed. Injected PID64974 and normal
PID64975 were reaped, their readers finished, and fixture roots closed.
Normal delivery again totaled 2097152 bytes with peak charge1047560.

This accepts the corrected fixture's cleanup, not the broader behavior of
production close after its output receiver disappears. That concern is
tracked as `A10 followthrough: adjudicate dropped PTY receiver cleanup`.
The production lifecycle watcher may own the reader JoinHandle; this fixture
asserts reader completion and EOF, not that close_session joined that handle.

The failed injected session 5455c4a5-6328-44bf-aa9b-a446ebbd3217 did not
record its PID before failure. PID62280 belongs to the successful sibling.
The worker could not recover an identity-specific PID mapping or reaping
receipt; a later generic process listing is not proof. Later successful
cleanup of injected PIDs63879/64974 does not close that historical evidence
gap. No broad kill was performed and the earlier supervisor is retained.

## Integration remains required

The machine receiver's byte budget includes guards held in flight and a
reserved control allowance. These guarantees require the socket to retain
each guard through flush, verify actual serialized overhead, and select the
independent termination signal against every write. Reading the module alone
cannot establish those obligations.

Remaining proof includes authoritative owner subscription, real blocked
socket write cancellation, the ten-second deadline, controller release,
independent sibling progress, and reconnect to the original PTY PID with an
explicit replay/gap cursor. The full A10 output-budget repair remains open.

Git status initially failed because the vendor submodule is a symbolic link.
Read-only status/diff with --ignore-submodules=all succeeded without changing
that link. The worktree contains many other active producers' changes;
this review does not attribute them to the output-budget packet.

All candidate changes remain uncommitted.
