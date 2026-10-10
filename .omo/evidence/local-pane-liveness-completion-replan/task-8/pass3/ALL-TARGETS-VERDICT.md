# Criterion 2 — `cargo check --all-targets` at abd9e890

Command (all hosts): `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
Candidate: `abd9e890` (tree contains 6c69715f + the split_journal portable-name fix)
Base: `d82b35e4`

## Verdict

| Host | Raw native exit | E0063 sites | Verdict |
| --- | --- | --- | --- |
| mac (CQFQ4P2LXK) | **101** | 2 | RAN_FAILED |
| linux (indo) | **101** | 2 | RAN_FAILED |
| windows (DESKTOP-1LAPJMP) | **0** | 0 | RAN_PASSED |

## Remaining sites (both hosts, identical)

```
error[E0063]: missing field `local_split` in initializer of `DaemonRequest`
   --> tests/daemon_handover_transfer_contract.rs:109:28
    |
109 |             .send_request(&DaemonRequest::Spawn {
    |                            ^^^^^^^^^^^^^^^^^^^^ missing `local_split`

error[E0063]: missing fields `create_only`, `prepared_local_split` and `remaining_ms` in initializer of `SpawnTerminalRequest`
   --> tests/ipc_hardening_contract.rs:110:9
    |
110 |         SpawnTerminalRequest {
    |         ^^^^^^^^^^^^^^^^^^^^ missing `create_only`, `prepared_local_split` and `remaining_ms`
```

Evidence: `pass3/{mac,linux}/logs/all-targets.log`; `pass3/{mac,linux,windows}/rustA.log`.

## Why both sites are candidate-caused compile breaks (not environment)

The candidate widened two wire types and did not update every constructor:

- `DaemonRequest::Spawn` gained `local_split` (`src-tauri/src/daemon/protocol.rs`).
- `SpawnTerminalRequest` gained `create_only`, `prepared_local_split`, `remaining_ms`
  (`src-tauri/src/ipc/terminal.rs:490-495`).

Neither test file is touched by the candidate diff:

```
$ git diff --name-only d82b35e4 abd9e890 -- src-tauri/tests/ipc_hardening_contract.rs
(empty)
```

`daemon_handover_transfer_contract.rs` **is** in the diff, but only its `run_v5_handover_case`
assertions were extended (workspace identity + cwd retention); its `Spawn` constructor at :109 was
not updated. The worker's 6c69715f commit fixed three sibling targets
(`daemon_handover_contract.rs`, `daemon_persistence_contract.rs`, `zero_config_gen4_audit.rs`) and
its message claims "the remaining all-targets fixtures" — these two are not among them.

## `--all-targets` output is not an exhaustive site list (finding)

Cargo stops scheduling new targets once one fails, so the site set reported by a single
`--all-targets` run depends on which target failed first. That is why the stale pass-3 log shows
4 sites in three other files while the fresh run shows 2. To get a definitive answer the verifier
ran a **per-target sweep** instead — see `pass3/linux/rustC-targets.log`:

- **81 test targets: 79 TARGET_OK, 2 TARGET_FAIL** (`daemon_handover_transfer_contract`,
  `ipc_hardening_contract`) — the same two sites, so no further target is broken. The repo holds 81
  `src-tauri/tests/*.rs` files, and 79 + 2 = 81, which reconciles exactly.
- **12 examples: 12 EXAMPLE_OK**, including `ssh_password_fixture` (the file the worker repaired).
  The repo holds 12 `src-tauri/examples/*.rs` files.

## Delta from the pass-3 record

Pass 3 (at d97233c1) recorded "2 remaining sites": `examples/ssh_password_fixture.rs:82` and
`tests/zero_config_gen5_regression.rs:195`. Both are now **fixed** — the example compiles
(`EXAMPLE_OK ssh_password_fixture`) and the gen5 target compiles (`TARGET_OK zero_config_gen5_regression`).
The repair also closed `daemon_handover_contract.rs`, `daemon_persistence_contract.rs` (x2) and
`zero_config_gen4_audit.rs`. The two sites now blocking are a **different pair** the worker did not
reach, so criterion 2 is still FAIL and the remaining work routes back to the fixture worker.
