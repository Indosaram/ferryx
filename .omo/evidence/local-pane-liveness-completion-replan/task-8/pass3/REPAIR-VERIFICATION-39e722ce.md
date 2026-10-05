# Repair verification by execution — HEAD `39e722ce`

Candidate tree bytes verified on every host by sha256 before running:

| File | sha256 | mac | linux | windows |
| --- | --- | --- | --- | --- |
| `daemon/session_service.rs` | `7d1a1aaa…` | ✓ | ✓ | applied |
| `ipc/terminal.rs` | `3946a80b…` | ✓ | ✓ | applied |
| `terminal/paired_runtime.rs` | `46344a52…` | ✓ | ✓ | applied |
| `ipc/tests.rs` | `f7393930…` | ✓ | ✓ | applied |
| `native_terminal/surface_host.rs` | `98d35560…` | ✓ | ✓ | applied |

## Compile gates — all clean

| Gate | mac | linux | windows |
| --- | --- | --- | --- |
| `cargo check --all-targets` | **0**, errors=0 | **0**, errors=0 | **0**, errors=0 |
| `cargo test --lib -- --list` (lib test target compiles) | **0**, 2693 selected | **0**, 2675 selected | — |

The E0308 blocker is fixed on every host.

## The two NEW paired-incarnation tests — PASS

| Test | mac | linux |
| --- | --- | --- |
| `terminal::paired_runtime::tests::paired_session_incarnation_is_stable_per_actor_and_distinct_across_reincarnation` | **0** — 1 passed | **0** — 1 passed |
| `daemon::session_service::tests::paired_describe_reports_the_incarnation_the_attach_fence_proves` | **0** — 1 passed | **0** — 1 passed |

## The attach pair — state CHANGED from FAILED to PASS

| Test | `abd9e890` | `39e722ce` mac | `39e722ce` linux |
| --- | --- | --- | --- |
| `ipc::tests::tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences` | **FAILED** | (rerun in flight) | **0 — 1 passed** |
| `ipc::tests::test_p13_attach_routes_through_descriptor_and_reinstalls_proxy` | **FAILED** | **0 — 1 passed** | **0 — 1 passed** |

## The four `surface_host` presentation tests — state CHANGED from FAILED to PASS

| Test | `abd9e890` (all 3 hosts) | mac | linux | windows |
| --- | --- | --- | --- | --- |
| `bounds_ipc_presents_when_browser_child_is_open` | FAILED | (rerun in flight) | **0 — 1 passed** | **0 — 1 passed** |
| `deferred_bounds_retry_does_not_restore_obsolete_width` | FAILED | (rerun in flight) | **0 — 1 passed** | **0 — 1 passed** |
| `synchronized_output_bounds_ipc_finishes_on_detach_or_stream_end` | FAILED | (rerun in flight) | **0 — 1 passed** | **0 — 1 passed** |
| `synchronized_output_bounds_ipc_waits_for_actual_presentation` | FAILED | (rerun in flight) | **0 — 1 passed** | **0 — 1 passed** |

## `pane_liveness_` and split reliability

| Gate | mac | linux | windows |
| --- | --- | --- | --- |
| `--lib pane_liveness_ -- --list` | (rerun) | 0, **47** selected | 0, **43** selected |
| `--lib pane_liveness_ -- --nocapture --test-threads=1` | (rerun) | **0 — 47 passed; 0 failed** | **0 — 43 passed; 0 failed** |
| `--lib local_split_reliability_ -- --list` | (rerun) | 0, **15** selected | 0, **14** selected |
| `--lib local_split_reliability_ -- --nocapture --test-threads=1` | (rerun) | **0 — 15 passed; 0 failed** | **0 — 14 passed; 0 failed** |
| `--lib split_journal` | — | — | **0 — 7 passed; 0 failed** |
| `--test ipc_hardening_contract` | (rerun) | (in flight) | **0 — 0 passed; 0 failed** |
| `--test daemon_handover_transfer_contract --list` | (rerun) | (in flight) | 0, 0 selected |

## Two harness/contamination findings (mine, not the product's)

1. **mac disk exhausted mid-run.** `df` hit **167 MiB free (100%)**, and three gates failed with
   `No space left on device (os error 28)` while linking build scripts, plus one test failed with
   `Timed out waiting for daemon response (15s)`. The mac disk is full because of **foreign**
   directories (`maho-workspace` 114G, `ferryx-monitor-2026.1003.2` 27G,
   `ferryx-input-diag-9297` 23G) that are not this dispatch's to delete; I reclaimed only my own
   regenerable `source-21dea3c0/target` (15G) and re-ran. **Those mac results were discarded.**
2. **Concurrent scripts contended.** I left `rgateG` running when I launched `rgateH` and then
   overwrote the tree, so G's later gates ran against mixed bytes. G was killed and its log preserved
   as `rustG-gates.STALE.log`; only H's results are cited.
