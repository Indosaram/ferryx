# Disposable-worktree test instrumentation

This tool is for a separate, disposable verification worktree. **Do not apply it to
production/development sources.** It does not run tests or change Cargo features.
The ordinary, uninstrumented test command remains unchanged.

```
cargo run --locked --offline --manifest-path tools/test-shard-instrumenter/Cargo.toml -- \
  plan --root /absolute/canonical/disposable/worktree --shards 8 > plan.json
# The maintained runner must establish ownership of the disposable worktree first.
ferryx-test-shard-instrumenter apply --root /absolute/canonical/disposable/worktree --shards 8 > applied.json
```

The CLI writes its JSON manifest to stdout. It parses every `.rs` file below
`src-tauri/src` and `src-tauri/tests/support` using `syn`, including separately
stored `include!` inputs and shared `#[path]` test modules. For each
free function carrying `#[test]` or `#[tokio::test(...)]`, it inserts one `cfg`
attribute **before the first existing attribute**, preserving all original bytes,
comments, attributes (including `ignore`/Tokio options), and function bodies.
Shared modules, helper functions, and test hooks remain untouched.

Ordinary buckets use FNV-1a over the relative source path plus original `fn` byte
offset. Files containing self-executable/libtest dispatch are indivisible cohorts;
the shared SSH supervisor files are explicitly joined. Cohorts are placed in stable
largest-first order into the least-loaded bucket, preserving subprocess helper
availability without overloading one shard. The runner additionally validates
literal targets and SHA-pinned reviewed dynamic selectors against compiler names.
With no `ferryx_test_sharding` cfg, every original test remains enabled. A shard
requires both `--cfg ferryx_test_sharding` and `--cfg 'ferryx_test_shard="N"'`.
The runner must validate N and matching `--check-cfg` declarations. Do not use this
mechanism as permission to execute tests requiring unavailable sockets or devices.

The manifest contains original/instrumented SHA-256 hashes, paths, original byte
offsets, local inline-module function names, and bucket counts. **It is not the
compiled test inventory:** inactive cfg/platform/bin files are deliberately included,
and out-of-line module paths may differ from source paths. The runner must verify
compiled `--list` output, disjoint shards, and exact inventory union separately.

The tool fails on parse errors, pre-existing instrumentation, unsupported test
attributes, conditional test registration, and recognizable test-generating macro
patterns. Newly introduced arbitrary procedural test generators require explicit
support. Root and source paths must be canonical, symlinks are refused, and all
files are validated before applying. Use a quiescent owned worktree: application is
not a filesystem transaction and does not defend against concurrent hostile path
replacement. A failed apply must be discarded, not partially used.

Tests: `cargo test --locked --offline --manifest-path tools/test-shard-instrumenter/Cargo.toml`.
