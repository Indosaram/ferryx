# Daemon handover repair journal

## Scope

Repair session continuity across automatic daemon handover in source code.
Do not modify installed apps, live daemon processes, live sockets, or the user's
session layout. Existing root `.debug-journal.md` belongs to unrelated work;
this file records this repair instead.

## Observations

- Old daemons 845 and 57449 retained 19 and 9 original session IDs.
- New canonical daemon 36119 listed none of those 28 IDs.
- `/tmp/rorca-501/handover_routes.json` contained `{"routes":[]}`.
- Direct queries of legacy sockets prove PTY ownership survived, not that the
  new canonical endpoint can route those sessions.

## Confirmed mechanisms and changes

1. A controlled occupied blocking worker reproduces old cleanup unlinking the
   replacement canonical listener. Commit now removes the old address before
   releasing ownership, in an awaited blocking operation.
2. Existing manifest updates allowed another file handle to acquire a write
   lock during a transaction and silently replaced malformed data with an
   empty manifest. All production read-modify-write paths now share a
   cross-process file lock and propagate read/parse failures.
3. Actual isolated headless handover reproduced a replacement main-thread
   panic: `failed printing to stdout: Broken pipe (os error 32)`. The original
   daemon's readiness reader had been closed, matching the GUI launch path.
   Handover children now have null stdin/stdout rather than inheriting that
   closed readiness pipe. The successor also suppresses the readiness token
   for `--handover-from`, so an unpatched parent cannot reproduce the panic.
   In-place daemon exec detaches stdin/stdout as well. Normal initial startup
   still emits its exact readiness token. Stderr remains inherited.
4. The retiring owner now persists its legacy route before committing
   ownership transfer. Registering a peer in the successor is in-memory only,
   removing the unawaited competing manifest writer. Startup does not report
   readiness if route inventory/adoption fails. The successor inventories and
   persists the predecessor route before requesting commit too, covering old
   producers that do not persist routes themselves.

These reproductions establish concrete code defects. They do not establish
every event in the historical installed-app failure, whose full stderr was
not captured.

## Safety and artifacts

- Only unique temporary test directories and sockets are permitted.
- No commands may stop, restart, write to, or resize a live user session.
- Production fix scope: daemon handover, manifest, proxy, server, and the
  headless startup readiness branch in `main.rs`.
- Regression tests use library targets; no shared-socket integration targets.
- Test/build logs under this report's evidence directory are retained evidence.
- The pre-existing debug journal is not modified.
- LSP is unavailable at `/Users/indo/.omo/lsp-daemon/v0.1.0/daemon.sock`.
- Default implementation child failed before tools due to OAuth 401.
  Read-only independent diagnosis uses the current parent provider instead.

## Verification evidence

- `evidence/handover-unlink-red.log`: old deferred cleanup removes the newly
  bound isolated canonical socket; one regression fails for that reason.
- `evidence/handover-manifest-red.log`: exclusion and malformed-data
  preservation regressions both fail before the transaction fix.
- `evidence/handover-process-red.log`: actual pre-fix handover child panics on
  Broken pipe; the replacement never becomes available. QA cleanup completes.
- `evidence/handover-green.log`: initial handover unit regression and all
  three manifest tests pass; debug binary build passes; two actual isolated
  handovers retain their original session IDs and input route.
- `evidence/handover-process-green.log`: strengthened real-process check
  passes two successive upgrades, preserving pre-upgrade history, executed
  printf output rather than terminal echo, and 100x30 PTY resize/describe.
  Both original session IDs remain reachable through the newest daemon.
- `evidence/handover-regressions-green.log`: additional persistence-failure
  ownership protection and adjacent client retry tests: 3 handover tests and
  2 client retry tests pass. Together with 3 manifest tests, 8 focused tests pass.
- `evidence/handover-final-regressions.log`: the same 8 focused tests pass on
  the final source after the compatibility changes; command exit code 0.
- `evidence/handover-mixed-version-red.log`: first upgrade from the preserved
  unpatched `2026.902.2` binary to the initial patch still loses its canonical
  connection. This revealed missing successor-side compatibility protection.
- `evidence/handover-compatibility-green.log`: final debug build succeeds;
  patched-to-patched two-generation handover passes; unpatched `2026.902.2`
  to patched `2026.908.1` then another upgrade with zero intermediate local
  PTYs also passes. The latter exercises same-PID exec and verifies the
  original owner's durable route, history, executed input and PTY resize.

The unpatched binary was run only as an isolated headless QA process from
the preserved backup. No installed GUI or existing user daemon was modified
or stopped during this repair.

Reproduction command:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --bin ferryx
node scripts/verify-daemon-handover.mjs
node scripts/verify-daemon-handover.mjs src-tauri/target/debug/ferryx \
  /Applications/Ferryx.app.bak-20260909-0523/Contents/MacOS/ferryx \
  --no-intermediate-session
```

The verifier uses unique `/tmp/fx-handover-*` directories and isolated HOME,
runtime, session, and data paths. It launches only headless QA daemons, never
the desktop application. The initial stdout reader is deliberately closed
after its exact readiness token; canonical replacement discovery uses
filesystem events, not timed polling. PTY output subscriptions precede input.

## Review

Independent review `st_01a082cb` requested changes for first-upgrade stdout
inheritance and missing successor-side route persistence. Both fixes and the
mixed-version regression are implemented and verified. The same reviewer
re-audited the final code and evidence and returned APPROVE for both blockers.

The reviewer noted that the process log does not explicitly print successor
version/PID continuity or numeric command status. The command monitor supplies
exit code 0; the verifier checks replacement epochs, session routing, durable
routes, history, executed output and PTY sizes. No broader GUI verdict is inferred.

## Final scoped verdict

Code repair complete. Final debug build, focused regressions, isolated
patched-to-patched and old-to-patched handovers passed. `git diff --check`
passed. LSP remained unavailable; Windows/Linux execution was not performed.

All recorded `/tmp/fx-handover-*` QA directories were removed, and a final
`lsof -nP -U` check found no socket holders for those runs. The root debug
journal and concurrent Sidebar work were not modified. Source and verifier
are committed in `3a6d017`. This report and the captured logs form the
companion verification-evidence commit.

No deployment, live-route repair, or GUI reattachment is claimed by this code
change. The original native desktop acceptance scenarios remain separate.
