# Retained-image eviction regression harness

Runs the **production** `retained_history`, `vt_stream`, and `vt_state` source modules without
building Tauri, Ghostty, or a desktop app. Its only dependency is the serde dependency already
used by those modules.

```sh
cargo test --manifest-path bench/terminal/retention/Cargo.toml
cargo run --release --manifest-path bench/terminal/retention/Cargo.toml
```

The benchmark fills the default 8 MiB image-history budget with 13-byte Kitty placement
commands, then times 4,096 additional pushes. Filling the budget is outside the measured
interval. Every run checks retained byte accounting and the oldest surviving sequence.
Three samples are printed; there is deliberately no machine-dependent timing assertion.

## Recorded comparison

Linux x86_64, rustc 1.99.1, release build, three samples in the same cloud workspace:

| Implementation | Measured steady-state time (ms) |
| --- | --- |
| Base `eab2e8e4`, scan all survivors on each group eviction | 4933.612, 4769.842, 5001.873 |
| Pop only the evicted group's contiguous front parts | 0.493, 0.484, 0.489 |

This deliberately stresses the small-image-command case: 645,277 retained groups and
4,096 evictions. It is not an end-to-end app, renderer, or typical terminal-output benchmark.
The improvement follows from removing an O(retained groups) scan from each eviction;
large images with fewer retained groups will show a smaller absolute benefit.

The regression tests include whole multipart-group removal with interleaved text,
multiple-group eviction in one push, replay ordering, byte budgets, parser fragmentation,
image overflow/discard handling, and retained terminal state. Desktop/macOS acceptance
still requires the normal app environment.

## Reproduce the baseline

Create a temporary worktree at `eab2e8e4` under your host's worktree root (see `AGENTS.md`).
Copy this harness directory into that worktree's `bench/terminal/retention/`, excluding
`target/` and the generated `Cargo.lock`. Run the same release command from the baseline
worktree, then remove the temporary worktree. The relative source-module paths will compile
that worktree's original implementation; no production source needs to be rewritten.
Use the same Rust toolchain and machine for both measurements.
