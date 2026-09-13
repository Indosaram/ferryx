# Watchdog daemon logging fix evidence

Date: 2026-09-13. Worktree: `sa-watchdog`. Scope: P2 in
`FOUR_TRACK_FINAL_CODE_REVIEW.md`; no GUI or live daemon operations.

## Production change

`run_daemon_headless` installs the production subscriber before constructing the
server or spawning its tasks. A narrowly filtered subscriber captures the existing
agent-state release INFO events, including session, reason and previous state.
It does not enable unrelated daemon/network tracing or write records to stderr or
stdout. The existing `FERRYX_DAEMON_READY` branch is unchanged.

Sink resolution:

- Explicit: `$FERRYX_DATA_DIR/logs/daemon.log`.
- macOS/Linux: `$HOME/.ferryx/logs/daemon.log`.
- Windows: `%LOCALAPPDATA%/Ferryx/logs/daemon.log`, falling back to
  `%USERPROFILE%/.ferryx/logs/daemon.log`.
- No shared temporary-directory fallback. Unix log directory/file permissions
  are 0700/0600; final log symlinks are rejected with `O_NOFOLLOW`. Windows uses
  the per-user directory's inherited ACL, matching existing remote-data practice.

No dependencies added. Existing Tokio, tracing-subscriber and standard-library
file locking suffice. A nonblocking 256-record queue limits pending data; records
larger than 8192 bytes and records rejected by a full queue are counted. The worker
writes `daemon_log_dropped_records=N` on the next write or orderly finish. Disk
retention is one file capped at 1 MiB, truncated before the next write would exceed
the cap (no archives). File locking serializes size-check/truncate/append across
overlapping daemon processes, including handover. Opening, permissions, locking,
metadata, truncation, writes and flushes use `crate::ipc::run_blocking`.

Initialization errors disable optional file logging without preventing server
startup or readiness. Worker write errors disable the writer without aborting,
restarting or signalling the server. Either path emits one stderr report prefixed
`FERRYX_DAEMON_LOGGING_DISABLED`, with error detail capped at 256 characters.
There is no retry loop or continuous stderr fallback. The failed receiver closes,
so subsequent records are discarded rather than accumulating. Existing-global-
subscriber initialization failure takes the same nonfatal path before any writer
task is started.
Normal server return sends a FIFO stop message and awaits the writer, draining
already-enqueued records. The existing explicit IPC Shutdown uses `process::exit`,
and Unix upgrade uses exec: neither runs Rust cleanup; queued tail records can be
lost on those paths or on crashes/forced termination. This fix does not claim
crash-durable or lossless audit logging and does not change those lifecycle paths.

## RED, captured before production implementation

Added a test-only branch to the real `run_daemon_headless` entry point. In a fresh
test subprocess it calls the actual `AgentStateHub::release_manual` and
`release_foreground` for `logging-fixture-session`, then returns before constructing
a server/listener. The branch does not install a test subscriber. Production
logging initialization now runs before this branch, and production finish drains
the queue before exit. Thus disabling production initialization breaks the test.

Command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  cli::tests::headless_release_reasons_reach_private_bounded_sink -- --exact
```

Original runtime result (not a compilation failure):

```text
headless release log must exist: Os { code: 2, kind: NotFound, message: "No such file or directory" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1044 filtered out
```

An earlier `--no-default-features` attempt failed to compile existing native
references; it is not the RED evidence. Default features were used thereafter.
During implementation, three new IpcError-to-anyhow conversion errors were fixed
in the owned logging/CLI code before GREEN.

## GREEN and boundedness

Same runtime regression after the production change:

```text
test cli::tests::headless_release_reasons_reach_private_bounded_sink ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1047 filtered out
```

Assertions read the actual persisted sink after bounded child exit and require
the session field and each distinct reason on matching records:
`manual_reset` and `foreground_agent_to_shell`. They also check 0600 permissions,
the size ceiling, and absence of reason records on stdout/stderr. A separate
cap-boundary test pre-fills the file to 1 MiB, appends a record using the production
writer, and checks the resulting length:

```text
test daemon::logging::tests::full_log_discards_old_records_before_appending ... ok
test result: ok. 1 passed; 0 failed
```

`cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx` passed. Existing
warnings remain (17 in lib tests, 16 in the binary build); no warnings suppressed.
Diagnostics found no errors in the three changed Rust files; the final CLI
diagnostic request was cancelled by the server after an earlier clean result.
`git diff --check` passed.

## Historical real executable failure surface (superseded)

Before the lifecycle correction below, ran `ferryx --daemon` with `FERRYX_DATA_DIR` pointing to a regular
file inside a unique worktree fixture. Other data/runtime/home/tmp variables and
cwd were overridden inside that fixture. This fails before any daemon connection
or listener and produced:

```text
exit=1
stdout bytes=0
Ferryx daemon error: IpcError { code: InternalError, message: "daemon log initialization: Not a directory (os error 20)", details: None }
```

This exit behavior was rejected in parent review because optional diagnostics
must not make terminal service unavailable. It is historical evidence, not the
current contract. The corrected executable was built but this full daemon launch
was not repeated: it would now proceed to service startup.

All fixture subprocesses use isolated data/runtime/home/tmp locations. The test
awaits exact child exit with a 20-second timeout and kill-on-drop; no sleeps,
polling, signals, restarts, live sockets, GUI or commits. Temporary runtime fixtures
were removed. Command transcripts remain in ignored `target/logging-*.txt`.

## Integration limits

This verifies the real production initialization and release methods, not the
watchdog process observer or full desktop spawn/readiness handshake. Windows and
Linux runtime behavior, ACLs and cross-process locking were not executed here.
The cap test does not simulate concurrent handover writers. Write-failure
injection at the actual writer boundary is covered below; a real full disk was
not created.
The test-only early exit deliberately prevents daemon listener/startup effects.

The parent should update the P2 disposition and shifted line citations in
`FOUR_TRACK_FINAL_CODE_REVIEW.md` after combined review; that document was not
rewritten by this child. Historical missing pre-production watchdog evidence and
manual GUI limitations remain unchanged. P1 files and pre-existing frontend/docs
changes were preserved and are owned separately.

## Lifecycle correction: optional logging must preserve primary service

Parent review identified that the first implementation's logging-task select
aborted the server on a diagnostic disk failure, and opening failure prevented
startup. Both behaviors have been removed. The CLI now awaits the primary server
task independently; the writer reports a failure once and returns. Finishing a
writer with a closed receiver is normal after its already-reported failure.

Added `cli::tests::headless_primary_service_survives_logging_failures` before
changing production failure behavior. It runs `run_daemon_headless` in fresh
subprocesses for initialization and write failure. The test replaces only the
primary service task with a stdin-gated task; it retains production logging
initialization, readiness handling and primary-task lifecycle await. It opens no
listeners and does not construct a daemon server. Initialization failure uses a
real regular file at `logs`; write failure injects `StorageFull` at the production
`append_bounded` boundary after subscriber initialization and a real release event.
The parent reads the exact error report before releasing the primary task through
stdin. Then it requires successful exit, readiness, a post-failure primary-service
sentinel, and exactly one disabled report. No polling or sleeps. This is lifecycle
seam coverage, not a claim that actual PTYs were created.

Failing-first command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  cli::tests::headless_primary_service_survives_logging_failures -- --exact
```

```text
test cli::tests::headless_primary_service_survives_logging_failures ... FAILED
init_failure:
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1048 filtered out
```

The original init path exited unsuccessfully before readiness. With the lifecycle
correction, the same command passed both subprocess cases. Final combined command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib cli::tests::headless -- --nocapture
```

```text
test cli::tests::headless_logging_child ... ok
test cli::tests::headless_release_reasons_reach_private_bounded_sink ... ok
test cli::tests::headless_primary_service_survives_logging_failures ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1046 filtered out
```

`cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx` passed after this
correction (16 existing warnings). Final diagnostics on cli/logging/mod returned
no errors; diff checking passed. No P1 files were edited. The earlier statement
that diagnostic failure terminates the service is explicitly superseded. Sink
location, target filter, permissions, queue bounds and file cap are unchanged.
