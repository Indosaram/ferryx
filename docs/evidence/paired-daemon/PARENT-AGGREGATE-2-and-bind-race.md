# Parent aggregate #2 and the LAN bind race

Run after the A14 protocol arms landed and the lib became compile-stable again.

## Compile gate

- `cargo check --locked --no-default-features --lib` → **exit 0**
  (`A12-session-metadata-parent-compile-lib.log`). The earlier
  `PairedHostOperation` non-exhaustive errors in `daemon/server.rs` and
  `daemon/client.rs` are resolved.
- An earlier gate with `--tests` failed (`exit 101`) on
  `native_terminal_wayland_subsurface_contract`, which imports
  `native_terminal::{child_surface, renderer, composition, surface_host}`.
  That test file is tracked and **unmodified** (`git diff --name-only HEAD` and
  `ls-files --others` are both empty for it), so its incompatibility with
  `--no-default-features` is pre-existing and the gate scope, not the code, was
  wrong. Parent-side mistake, corrected to `--lib`.

## Aggregate result

`A12-session-metadata-parent-aggregate-2.log` (exit 101):

```
test result: ok. 1 passed; ... (machine_worktrees section)
test result: FAILED. 282 passed; 1 failed; 0 ignored; 735 filtered out
failures:
    remote::tests::test_active_desktop_terminal_contract_and_safe_selection_bridge
panicked at src/remote/tests.rs:759:10:
start server: "Failed to bind to 192.168.0.34:60286: Address already in use (os error 48)"
```

## Classification: pre-existing parallel bind race, not a regression

Evidence gathered by the parent:

1. `src/remote/tests.rs` is tracked and **unmodified** by this work.
2. The production bind site (`src/remote/server.rs:2497`,
   `Failed to bind to {bind_addr}`) is **not** in the diff. The only
   bind-related added lines in `server.rs` are inside a new test fixture
   (`TcpListener::bind("127.0.0.1:0")`).
3. The test configures `port: 0`, yet the panic names a concrete port, so the
   gateway discovers a port and then binds `192.168.0.34:<port>` — a
   discover-then-bind window that another parallel test can win.
4. `lsof -nP -iTCP:60286` showed nothing listening afterwards, so no canonical
   daemon or user process owned it. Nothing of the user's was touched.
5. Isolated rerun
   (`--lib remote::tests::test_active_desktop_terminal_contract_and_safe_selection_bridge -- --exact --test-threads=1`)
   → **exit 0**, `ok. 1 passed`
   (`A12-session-metadata-parent-desktop-contract-isolated.log`),
   private root removed (`removal_exit=0`).

The defect predates this plan. This work does increase collision pressure,
because it adds a large number of new `remote::machine_*` tests to the same
parallel binary, so the pre-existing race is now more likely to be observed.

Recommended follow-up (outside the plan's A01-A24 scope, needs authorization):
bind the LAN listener directly on `<lan-ip>:0` and read `local_addr()` instead
of discovering a port and re-binding it.

## Status

- 282 of 283 executed `remote::` tests pass; the single failure is the race above.
- `machine_worktrees` integration target passed in the same run.
- The headless `ferryx-cli` / `ferryx-relay` check did not execute because the
  shell chain stopped at the failing test; it is re-run separately.
- A repeat full `remote::` run is in flight to measure reproduction frequency.
