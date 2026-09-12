# A05 parent candidate review

Not accepted. The first independent reviewer failed with provider HTTP 502 and
`getaddrinfo ETIMEOUT chatgpt.com`; that is not a code verdict. The parent read
the original prompts, complete new catalog/service/test bodies, and every
remaining changed-file diff, then ran the candidate fixture.

`A05-parent-candidate-green.log` records the locked, four-job, private-target
`machine_catalog_persistence` test passing. The parent directly confirmed its
private root `.tmpB860KP` absent after completion. This proves service
reconstruction in one test process, not an OS daemon restart.

## Corrections required before acceptance

- `tests/machine_catalog_persistence.rs` uses `tokio::net::UnixStream` without a
  Unix conditional. The test target cannot compile on Windows. Isolate that
  transport appropriately and retain platform-neutral catalog behavior checks.
- The fixture does not run distinct owner processes for initial registration and
  restoration. Extend the existing scoped fixture to perform actual isolated
  process restart with exact readiness and wait/reap receipts. Do not start the
  canonical daemon or create replacement PTYs.
- A panic or timeout in the expanded IPC block bypasses the explicit task join;
  runtime drop is not its claimed failure-path teardown. Ensure owned sockets
  and task are closed/joined on assertion failure and test that path.
- `workspace_service.rs::register` discards the normalized ID returned by
  `validate_workspace_id` and publishes the original string. The previous
  registry implementation used the normalized value, so surrounding-space IDs
  can now be acknowledged but not found by normalized lookup. Add a focused
  failing compatibility regression and preserve the old normalized identity.
- The same registration now always persists and replaces the manager for an
  unchanged existing binding, unlike the previous no-op behavior. Preserve
  idempotent re-registration while allowing an intentional exposure promotion.
- Exercise explicit invalid-root restoration and post-rename directory-sync
  failure, covering mutation fencing without asserting rollback of an
  ambiguous disk outcome. The existing real rename/write failure is already
  useful evidence; actual disk exhaustion is not additionally required by the
  plan's disk-full/write-failure alternative.
- Trace production construction callers and place newly introduced catalog
  reads and Git probes off Tokio executor threads before gateway readiness.
  Existing unrelated constructor I/O is not an excuse for adding blocking work.

Preserve existing passing alias, write-failure, missing-root, corrupt/newer,
permissions, and reserved-ID evidence. The private catalog DTO ownership issue
is resolved and must not be reopened. No A06-A24 capability or HTTP API is
authorized by this correction.
