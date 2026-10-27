# Local full-library test compilation in bounded shards

`scripts/ci-test-shards.py` is an opt-in escape hatch when the monolithic
`cargo test --lib --no-run` compiler process cannot complete. It does not modify
normal Cargo builds, production sources, CI workflows, features, assertions, or
ignored-test policy. By default it **compiles and inventories** tests only; this is not a replacement
for the runtime test gate. `--run-tests` explicitly enables that complete gate.

## Prerequisites and invocation

Use the repository's supported Rust, Bun, Zig, Ghostty and native dependencies.
Build `ui/dist` with Bun first. Dependencies must already be available offline.
Use a clean committed source revision and the host's prescribed worktree root.
The runner requires Python 3, Git and the same Rust compiler for both inventory
and executable builds. Explicit profile environment settings are recorded;
external Cargo configs, Rust flags and compiler wrappers are refused.

```sh
# Example: use your host's actual canonical checkout/worktree paths.
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
python3 scripts/ci-test-shards.py \
  --source-root /absolute/path/to/checkout \
  --worktree-root /absolute/path/to/ferryx-wt \
  --evidence /absolute/path/to/ferryx-wt/.evidence/new-run \
  --target x86_64-unknown-linux-gnu --shards 8
```

The evidence directory must not exist. `--stage-timeout` bounds each compilation
(default 1800 seconds); the existing process-group supervisor records five-second
resource samples and cleans up the stage's children. Compilation is sequential.
This tool is currently validated on Linux, not macOS or Windows.

## Completeness and semantic safeguards

1. Obtain the unmodified compiler-resolved test inventory by macro expansion with
   the **same installed compiler**. This diagnostic invocation alone opts the
   root crate into `RUSTC_BOOTSTRAP=ferryx_lib` and `-Zunpretty=expanded`; it does
   not produce or claim a compiled executable. Normal shard compilation uses
   neither option. The version-sensitive parser fails closed on unexpected
   descriptor/registration structure.
2. Create an owned disposable Git worktree at the exact source commit. Reuse only
   the verified pinned local Ghostty checkout; no submodule network fetch occurs.
3. The `syn`-based instrumenter inserts cfg attributes only on complete test
   function items, preserving their original bytes, attributes and assertions.
   Modules and shared helper functions remain intact. Both `src-tauri/src` and
   shared `src-tauri/tests/support` inputs are scanned. Every compiler descriptor
   must map to exactly one planned AST function by original source path/line
   before compilation starts; unknown external inputs fail closed. Platform/feature-disabled
   source tests do not become part of the target's compiler inventory.
4. Keep self-executing parent/helper tests in the same binary. Source files with
   `current_exe`/libtest dispatch form cohorts; the shared SSH supervisor fixtures
   form one cross-file cohort. Deterministic greedy placement balances these groups
   without splitting them. A source-aware preflight resolves literal/constant test
   selectors against the compiler inventory and fails on cross-cohort targets.
   Dynamic selectors and compiler-inactive exceptions require reviewed source-SHA
   mappings in `tools/test-shard-instrumenter/self-dispatch-reviews.json`; any source
   edit invalidates that review. This is conservative repository-specific checking,
   not a general Rust data-flow proof. Integration test sharding is not supported.
5. Compile each shard with ordinary Cargo, default features and the same profile.
   A root-test-only wrapper selects one deterministic bucket. Unique workspace
   wrapper paths separate Cargo fingerprints while reusing dependency artifacts.
   The actual compiler feature/profile/target/extern contract must match baseline.
6. Ask every actual executable for its full and ignored test lists. Require the
   **disjoint union of exact names and ignored metadata** to equal the original
   compiler inventory. Missing, duplicate, extra or reclassified tests fail the
   run. Merely matching a count is insufficient.
7. Retain executable hashes, lists, source/lock/compiler provenance, compiler
   arguments, resource logs, instrumentation manifest and generated patch. Remove
   only the generated worktree owned by this invocation; retain shared cache.

An optional `--baseline` accepts an existing verified inventory only when its
source/compiler/target/profile snapshot is identical. Prefer generating a new
baseline after any source or configuration change.

## Runtime is a separate gate

`COMPILE_AND_EXACT_INVENTORY_PASS` means every target-resolved library test was
compiled once, not that any test passed at runtime. On a machine legitimately permitting all test capabilities, add `--run-tests`
to execute every shard with no name filters or added skips. This occurs before
the generated worktree is removed, preserving compile-time manifest paths. The
runner checks each runtime summary against the complete inventory, including
normal ignored-test semantics; only then records `FULL_RUNTIME_PASS`. Tests involving sockets,
PTYs, processes, display or network require the corresponding legitimate runtime
capabilities; never bypass sandbox denials. Restricted environments must report
only the specifically audited tests actually executed, not full-suite success.

## Tool regression tests

```sh
python3 -m unittest discover -s scripts -p '*shard*test.py'
cargo test --offline --locked --manifest-path tools/test-shard-instrumenter/Cargo.toml
```


## Test prerequisites and conditional coverage

Font rasterization tests require the configured MesloLGS NF font. Install test
fonts only in an owned test-data directory and record their upstream revision/hash;
missing fonts must not be treated as a renderer assertion pass. Headless GPU tests
need an available hardware or software adapter, not a desktop window. The eight
optional-adapter tests print `FERRYX_GPU_TEST_READY` or
`FERRYX_GPU_TEST_UNAVAILABLE`; use `--nocapture` when collecting coverage. A harness
pass after `UNAVAILABLE` is not proof that GPU assertions ran. Environment-gated
subprocess helpers may be no-ops when directly selected; their parent scenarios
provide the actual coverage and must remain co-located.

Linux cluster/color-emoji regressions also require DejaVu Sans, DejaVu Sans Mono and Noto Color Emoji
(e.g. Debian/Ubuntu `fonts-dejavu-core` and `fonts-noto-color-emoji`). PangoCairo,
Pango, Cairo and fontconfig are native dependencies already supplied by the GTK
stack. The `--run-tests` runner checks exact font families with `fc-match` before
compilation and records resolved font-file hashes. A fallback font or unavailable
fontconfig is an actionable preflight failure, never a skipped passing test.
Compile/inventory-only mode does not require runtime fonts. Use the same owned
HOME/XDG/font configuration for baseline/head rendering and measurements.

## Optional constrained-host optimized build

Test sharding does not change production compilation. If the optimized root crate
cannot finish on a constrained host, an explicitly different code-generation
partition can be measured without changing the committed release profile:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --release --bin ferryx \
  --features tauri/custom-protocol --locked --offline -j1 \
  --config 'profile.release.package.ferryx.codegen-units=16'
```

This keeps optimization level 3, thin LTO, stripped symbols and no debug info,
but partitions Ferryx into 16 code-generation units rather than the repository's
single unit. Dependencies retain their original profile settings. Record this
override, compiler, source and executable hashes with any QA result: it is not
the same binary or proof that the unmodified release profile passed. Codegen
partitioning can affect runtime performance; do not infer equivalence from build
success. A killed compiler alone does not prove an out-of-memory condition.
This command builds locally; it neither signs nor publishes nor launches the app.
