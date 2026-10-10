# Repair verification by execution — HEAD `426d1b27`

The author's verdicts were **UNVERIFIED** until run. These are the raw results.

## Verified: the four `surface_host` presentation tests now PASS on all three hosts

Command (each test, exact):
```
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  native_terminal::surface_host::tests::<name> -- --exact --nocapture --test-threads=1
```

| Test | mac | linux | windows |
| --- | --- | --- | --- |
| `bounds_ipc_presents_when_browser_child_is_open` | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed |
| `deferred_bounds_retry_does_not_restore_obsolete_width` | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed |
| `synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed |
| `synchronized_output_bounds_ipc_waits_for_actual_presentation` | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed | **0** — 1 passed; 0 failed |

**State change vs `abd9e890`:** all four were **FAILED on all three hosts** there
(`receipt.presented` / `latest.presented` / `result.unwrap().presented` assertion failures at
`surface_host.rs:5961, 6072, 6183, 6269`). At `426d1b27` all four are **passed**. The author's
"the tests were wrong, production untouched" verdict is therefore **VERIFIED by execution**, and I
independently confirmed every hunk sits inside `mod tests` (`git show` hunk headers all read
`@@ … mod tests {`).

## Verified: `pane_liveness_` filter unchanged and green

| Host | list selected | run | native |
| --- | --- | --- | --- |
| mac | 47 | (see `rustG-gates.log`) | 0 |
| linux | 47 | `ok. 47 passed; 0 failed; 2626 filtered out` | 0 |
| windows | 43 | `ok. 43 passed; 0 failed; 2237 filtered out` | 0 |

## Verified: the attach pair now compiles and passes on linux

| Test | linux |
| --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | **0** — `ok. 1 passed; 0 failed` |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | **0** — `ok. 1 passed; 0 failed` |

**State change vs `abd9e890`:** both were **FAILED** there (`UnsupportedCapability: "Attach requires
the persisted seven-field pane binding; pass attachTuple"` at `src/ipc/tests.rs:313:6` and `:3426:6`).
Both now **pass**. The p13 pass is the **settling experiment for the paired-fence question**: the fence
**accepts** a proxy session when the incarnation is provable, which isolates the real-world failure to
the daemon answering `None` rather than to the fence's own logic.

## Verified: compile gates clean

| Gate | mac | linux |
| --- | --- | --- |
| `cargo check --all-targets` | **0** (errors=0) | **0** (errors=0) |
| `cargo test --lib -- --list` (lib test target compiles) | **0**, 2691 selected | **0**, 2673 selected |
| windows `cargo check --all-targets` | — | — | **0** |

The E0308 blocker at `src/ipc/tests.rs:309` is **fixed** — the lib test target compiles on all three
hosts.

## Lead's `daemon_epoch` correction — CONFIRMED in my own reading

The caveat that a `daemon_epoch` clause could block attach is **NOT OPEN**. Reading
`src-tauri/src/ipc/terminal.rs`:

- `grep` for any comparison (`==` / `!=`) against `daemon_epoch` → **no match**.
- First attach path, `:2702`: `if description.incarnation != binding.incarnation` — incarnation only;
  then `:2709` `authoritative.daemon_epoch = attachment.epoch.to_string()` (**overwrite**).
- Second attach path, ``:2820`: `description.incarnation.is_none() || description.incarnation != binding.incarnation`
  — incarnation only; then `:2839` `binding.daemon_epoch = attachment.epoch.to_string()` (**overwrite**).

Both paths **overwrite** the epoch from the attachment and never gate on it, so the epoch clause was
never a blocker. Confirmed independently.

## Third fence string found

`"Attach incarnation differs from authoritative owner"` (first attach path, `:2703`) is also
**0 occurrences at base `d82b35e4` and 1 at candidate** — a third candidate-introduced fence string,
alongside the two reported.
