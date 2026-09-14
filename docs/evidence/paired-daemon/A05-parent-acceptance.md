# A05 parent acceptance

Accepted at the scoped durable catalog boundary. The parent reconstructed the
producer and verifier obligations, read the changed source and complete
extraction diff across the candidate corrections, traced both B1/B2 callers,
and independently executed the final candidate. No blocking A05 finding remains.
This does not accept A06-A24 or full AC01-AC12.

All seven current source hashes match `A05-final-manifest.log`. Fresh parent LSP
error diagnostics returned no diagnostics for each source/test file.
`git diff --check` returned 0.

## Parent execution

Commands ran serially from this worktree, with `CARGO_BUILD_JOBS=4` and
`CARGO_TARGET_DIR=/Users/indo/code/project/orca-lite-wt/herdr-wave1/src-tauri/target`.
All used `--locked --manifest-path src-tauri/Cargo.toml --no-default-features`.

- `cargo test ... --lib a05_compatibility_tests -- --nocapture`: 3 passed.
- `cargo test ... --test machine_catalog_persistence -- --nocapture`: 4 passed.
- `cargo test ... --lib catalog_tests -- --nocapture`: 1 passed.
- `cargo test ... --lib worktree::registry::tests -- --nocapture`: 7 passed.
- `cargo check ... --bin ferryx-cli --bin ferryx-relay`: exit 0.
- `cargo test ... --lib a04_shared_services_tests -- --nocapture`: 4 passed.
- Three separate Local selectors each passed once:
  `test_server_unregister_workspace_is_idempotent_and_revokes_registration`,
  `test_server_register_workspace_and_spawn_isolation`,
  `test_server_spawn_cwd_validation`.

The two complete parent run logs are `A05-parent-final-green.log` and
`A05-parent-final-local.log`. The child-entry tests are included in the reported
counts; they are not additional behavioral scenarios.

## Real observations and teardown

The constructor-isolation child PID 96776 left its private canonical sentinel
unchanged and removed both constructor directories. SSH unregister released
expired ownership without contacting a host or mutating the local catalog;
desktop `daemon:` rejection remains.

Owner 96861 registered plain/Git roots via the real IPC handler, exited 0 and was
reaped before owner 97001 restored those registrations in a fresh process.
Owner 97052 exercised the injected post-readiness failure and was killed/reaped.
The intentional IPC assertion was caught, and both tasks/endpoints were joined
and closed. Alias concurrency, normalized idempotency, exposure promotion,
0600/0700 permissions, missing/invalid-root preservation, corrupt/newer bytes,
real rename failure and post-rename sync fault all passed.

The parent directly confirmed all four reported A05 process IDs and six roots
absent: `.tmpynQzGD`, `.tmpZ1cmer`, `.tmp4fjEvz`, `.tmp7knirI`, `.tmpMrLqZA`,
`.tmpNhEun9`, beneath the workstation temporary directory.

The A04 regression retained real original owner 98272, PTY 98314, matching IPC
and OS cwd `/private/tmp/a04-87vdcc/project`, and HTTP200 session identity
`dfb8a6f5-7e07-4c0e-b599-8e1c7ad59977`. All normal/injected listeners joined,
PTYs and children reaped, and private roots removed. Parent directly checked all
13 reported PIDs and seven roots absent. The Local tests use existing TempDir
ownership; their unnamed resource paths were not separately observed.

## Boundaries

Existing compiler warnings remain unsuppressed. The SSH regression covers actual
unregister dispatch with expired ownership, not another real OpenSSH run. Linux,
Windows, native UI, physical ENOSPC and power-loss behavior were not executed.
The actual write-failure alternative and injected post-rename ambiguity cover
the scoped durability failure requirements. Catalog restart creates no PTYs;
later terminal survival/reattachment acceptance remains separate.

No canonical daemon, desktop, developer catalog or SSH host was modified.
V05 remains unaccepted in its separate worktree. This increment is ready for
the authorized local commit, not push, merge or deployment.
