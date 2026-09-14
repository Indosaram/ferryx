# Q4 Linux frozen verification - NOT ACCEPTED

Task st_01a0984c, parent 01a097f8-4568-7573-897e-d61f0fe6d692; 2026-09-13.
This is independent Linux evidence expansion for the frozen Wave1 dependency,
not implementation approval, complete packet acceptance, or native acceptance.

## Outcome

The frozen backend compiles on real x86_64 Linux, and the worktree suite and all
five additional integration targets pass. Four existing child-process readiness
tests fail in this invocation. The aggregate gate is **NOT ACCEPTED**. No
production or test source was repaired and no failed test was retried to green.

| Execution | Result | Full evidence |
| --- | --- | --- |
| Required `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib remote:: -- --nocapture` | exit 101; 251 passed, 3 failed, 674 filtered | Q4-linux-remote.log |
| Required six-target integration command | exit 101; catalog 3 passed, 1 failed; Cargo did not execute subsequent targets | Q4-linux-integration.log |
| Required `cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib worktree:: -- --nocapture` | exit 0; 44 passed, 884 filtered | Q4-linux-worktree.log |
| Required `cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --lib --bin ferryx-cli --bin ferryx-relay` | exit 0 | Q4-linux-build.log |
| Only previously unexecuted integration targets, with the same flags | exit 0; legacy bounds 2, transports 2 outer tests, machine worktrees 2, relay 6, safety 9 | Q4-linux-remaining-integration.log |

Exact argv, PIDs and exits are in Q4-linux-results.json and
Q4-linux-remaining-results.json. The continuation removes only the already-run
catalog target from the original argv; it neither suppresses its failure nor
repeats it. Helper tests that return without their child environment are not
independent coverage; the transport outer test actually executed its selected
child (one passed). Counts are harness results, not unique scenario counts.

## Identity and isolation

Read the full approved 833-line plan, applicable root/backend/daemon AGENTS,
WAVE1-resume-acceptance-gaps.md and PLATFORM-resume-preflight.md. Source was only
`/Users/indo/code/project/orca-lite-wt/herdr-wave1`, inherited HEAD
`e1a00339a5339ed4d9c9634e26b86f41111d49f6` plus actual dirty/untracked source.
The active resumed src-tauri source was not copied or edited.

- All 34 A08 frozen hashes match before capture, after capture, in the private
  snapshot, after transfer, after Linux execution, and in the original source
  at final recheck. See Q4-linux-source-before.json, source-after-capture.json,
  snapshot-baseline.json, source-final.json, transfer.log and cleanup.log.
- Full snapshot: 6,827 regular files; manifest SHA-256
  `f5c4eab343d595b88c83b31a29412f8ad3c4acc3008951283fb8bee71bc8b4bf`.
  Archive SHA-256
  `9f0941dc85bce511efe622ed8cf87e8d84185f1e127b57a79e8e5f3576864978`.
  Q4-linux-snapshot-manifest.json binds source, fixtures, scripts, assets and
  private UI dist input, including untracked implementation and Cargo.lock.
- No targets/node_modules/canonical configuration or credentials were copied.
  Dependency symlinks were not traversed. Ghostty's clean pinned
  `6a508fd5e34c7e222c052a6d00bb3891ff3feace` source was archived and its own Git
  history bundled to satisfy the build's revision check in a private directory.
- Capture initially omitted ignored Cargo.lock and failed before archive/build;
  finish-capture.py explicitly copied the required frozen lock and revalidated
  all hashes. Both capture outputs are preserved, not erased.
- Local staging was absent before creation; remote root absence was checked
  before `mkdir -m 700`. Retained roots are
  `/tmp/ferryx-herdr-q4-linux-01a097f8` and
  `/home/indo/ferryx-herdr-q4-linux-01a097f8` on indo@100.91.254.71 (omaki,
  kernel hostname `indo`), Linux 7.1.9-arch1-2, 12 CPUs; never WSL.
- Cargo/rustc 1.98.0, Zig 0.16.0; GTK 3.24.52, WebKit/JSC 2.52.6, ALSA
  1.2.16.1 available. rust-analyzer's installed shim reports Unknown binary;
  Q4-linux-diagnostics.log discloses unavailable LSP before compiler execution.
- Q4-linux-environment.json sets HOME, runtime/data/session, all relevant XDG,
  TMP/TEMP/TMPDIR before library initialization. QA roots are outside checkouts.
  Cargo/Rustup homes explicitly retained; target private; jobs=3, wrapper empty,
  dev/test debug=0, incremental=0. Git global/system config excluded. Tests ran
  with RUST_TEST_THREADS=1; that inherited child setting is relevant below.
- Reviewed constructors, fixture paths and subprocess setup before execution;
  private explicit config/auth paths or tempfile-backed constructors are used.
  Existing transport and typed-owner fixtures explicitly allocate unique
  `/tmp/a08-*` roots rather than honoring TMPDIR. These remain private/outside
  Git, are named in cleanup receipts and were not redirected by source edits.

Tauri build generated a private-copy delta in
`src-tauri/gen/schemas/linux-schema.json`; all other manifested input hashes
remain unchanged and all 34 frozen hashes still match. This is explicitly
returned as Q4-linux-generated-source-delta.patch/json, not silently described
as byte-identical output source. No authored baseline source delta exists.

## Actual Linux behavior proved

- `remote::filesystem_tests::a06_directory_http_home` passes real native HTTP
  browsing. Linux cfg creates an actual directory with byte 0xff on disk;
  returned listing remains valid and truncated. The fixture also covers home,
  parent/outside-home, tilde, hidden toggle, accessible symlink, spaces/quotes/
  Unicode, directories-only, 1,000 entries, and native mode-000 requested-root
  403. Malformed/relative/NUL/invalid UTF-8 requests and missing/file targets
  receive their asserted errors. This is Linux filesystem evidence, not only
  Darwin Git-index output. Its invalid-entry omission assertion is indirect
  through valid listing/truncation, not a dedicated collision adversary.
- `worktree::git::local_budget_tests::a08_git_native_byte_fidelity` creates the
  actual Linux file ` leading <ff> trailing `, also inserts it in the Git index,
  and observes strict `Invalid UTF-8 in Git stdout`. The real legacy worktree
  path fixture preserves leading/trailing spaces, quote and U+FFFD. There is
  no dedicated invalid-byte *linked worktree directory* fixture in this run.
- `git_hook_descendant_containment` passes cancel/revoke/deadline/output/
  success/failure/injected modes with real Linux Git, Python hook, intermediate
  and leaf processes. Each PGID equals the owned Git PID, differs from the
  test PGID; sibling remains alive until explicit cleanup. Logs record every
  worker/root cleanup with errors=[]; final /proc receipts show all logged
  process IDs absent. Linux-selected implementation uses /proc membership and
  pidfd_open/poll, not Darwin kqueue. No syscall tracer was attached.
- Exceptional drain JoinError test records explicit child wait before error;
  real revocation/deadline/cancel blocked Git children are killed/reaped. This
  does not prove exceptional OS attach/signal/wait failures or deliberate
  process-group escape containment.
- Real owner HTTP/relay/private UDS worktree lifecycle passes, including
  zero-byte lost reply, replay, branch choices, dirty refusal and socket/root
  teardown. Transport fixture explicitly reports PTY_spawned=false.
- Separate shared-owner busy fixture records real PTY PID 310454 and actual
  managed-worktree cwd, explicit close, absent/already_reaped=true. Typed-owner
  fixture records original PTY 307301 and exact locked/busy/partial UDS/native
  adapter error equality. Worktree safety executes real PTY lifecycle tests.
- Existing relay terminal WS bridge and gateway socket ticket/replay/revocation
  tests pass (exact names/results in remote.log). These are real wire fixtures
  with existing fixture backends, not the later machine terminal-stream/native
  proxy contract, and not proof of an end-to-end relay PTY input/resize/interrupt
  workflow. R3 staged wire/full-body-injection contract passes with its historical
  eager-upload limitations unchanged.

## Retained failures and diagnosis boundary

These are executed test failures, not compiler RED:

1. `followthrough_owner_crash_while_git_active`: line 627 requires observed
   readiness marker; first owner 306921 and owned Git 306943/hook 306946 were
   cleaned, replay owner 306947 exited 0, but marker assertion failed.
2. `interrupted_git_transaction_restart_never_repeats`: same marker assertion
   at worktree_authority_tests.rs:627; owner 307064 killed/reaped; root removed.
3. `r12_real_kill_windows` (name means crash windows, not Windows OS):
   workspace_api_tests.rs:593, first afterCatalog register barrier timed out
   (`signal=Err(Elapsed(())) kill=Ok(())`); owner 307411 killed/reaped.
4. `isolated_owner_restart_and_failure_cleanup`: machine_catalog_persistence.rs:103
   expected readiness/completion result; register child 308043 was killed/reaped
   after 20 seconds, root removed. Remaining restore/injected child phases did
   not execute.

The parent fixtures parse exact/start-of-line stdout sentinels from Rust test
children. Children inherit RUST_TEST_THREADS=1 and invoke libtest with only
`--exact ... --nocapture`; libtest can place its test-name prefix on the same
line as the first marker. This is a source-backed suspected harness cause,
especially given the successful replay child's exit, **not independently proven
causality**: child stdout lines are consumed rather than retained by these
fixtures. No marker/parser change, environment retry or production repair was
made. Parent review should resolve this fixture framing issue before treating
these failures as durable-journal or runtime implementation defects.

Build retains 10 existing warnings (unused imports/variables/dead code), with
full compiler output. No warning suppression, skipped failing test, or green
retry was used.

## Cleanup and orchestration receipts

Q4-linux-run.py owns/waits each Cargo command under background monitor 296751;
remaining.py owns/waits continuation under monitor 309350. Existing monitor
completion was awaited via Linux pidfd subscription with bounded waits, never
by duplicate builds or sleeps. Shell launch SSH timed out while the first
monitor remained alive; its existing PID was inspected and followed.

The user-reported transient provider HTTP 400 interrupted orchestration before
snapshot/build. Resumption found an empty existing local staging directory and
remote root still absent; it did not restart a command. This is an **API
orchestration failure**, not a compiler/test failure. An early overly broad
AGENTS find also timed out; targeted reads replaced it.

Final Q4-linux-cleanup.log records no owned running process, all 54 extracted
logged process IDs absent, both explicit /tmp fixture roots absent, and deletion
of the entire private qa tree including generated test credentials/locks.
Fixtures record listener joins/refusals and PTY waits. No canonical daemon,
installed app, desktop input, remote deployment or privileged change occurred.
Source/archive/vendor bundle/target/logs/scripts are retained only in the two
assigned staging roots for later composition; no owned runtime remains there.
The initial full-source cleanup check correctly stopped on generated schema
drift before removal; Q4-linux-cleanup-first.log preserves it. The second check
accepts only that exact disclosed generated delta, then performs cleanup.

Evidence files are uncommitted Q4-linux-prefixed additions only. Resumed repo
`git status` itself reported an existing vendor symlink/submodule incompatibility;
it was not repaired. No source change or Git commit was made in any existing
worktree.

## Explicit remaining exclusions

No Q1 budget-exhaustion additions or Q3 rich prunable-preview implementation
were present: prunable preview retains explicit 422. No Windows/macOS build,
desktop/Tauri command invocation, GUI/native-menu acceptance, machine session
CRUD/controller stream, A12 events, forced-relay OS network exclusion, deployed
relay compatibility, production log audit, rollback, exceptional OS-failure
drain or automatic parent-death cleanup is accepted here. Failed restart/crash
phases remain unproved. The active-Git owner crash harness itself kills surviving
Git; that is not automatic production orphan containment. Keep full-plan and
native acceptance manual and separate.
