# Real terminal pipeline QA and measurements (PR #24)

These are **Linux, unoptimized dev-profile component measurements**, not release-build,
GUI responsiveness, or input-to-display benchmarks. The [harness](../../../src-tauri/examples/perf_terminal_pipeline.rs)
links the actual `ferryx_lib` module graph: `NativeTerminal`/libghostty-vt search and
`TerminalOutputHub` retention, snapshot, and Ghostty reconstruction. No copied matcher,
mock parser, daemon, socket, or PTY is used.

## Revisions and method

- Baseline: `eab2e8e4712e60fa155cf794ad5885afff943244`.
- Measured head: `8837b2ae958d74ba23e28f6ea3e3900c79b59355` (local validation commit).
- Remote PR source `d2b3e909e78f93ee0e0da880f7e985f348d6e254` and measured local
  commit share tree `9da99219209f2e60dcfb33d6d4e905f1f00b260f` (GitHub commit
  API and local Git verified). The evidence-only follow-up adds no production changes.
- Identical harness SHA-256: `c9a924b4f7178d64c2bda9b21529c3df075bb85da48d7587bc93aac0b541bd13`.
- Same `Cargo.lock`, Ghostty revision, compiler, machine, default features and Cargo dev
  profile: `opt-level=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_INCREMENTAL=0`,
  `CARGO_BUILD_JOBS=1`. Exact metadata is in [evidence/metadata.json](evidence/metadata.json).
- Five timed samples after two warmups; table reports medians, not p95. JSONL includes
  every sample and min/max. Revisions ran sequentially without competing builds or
  benchmarks, not randomized; small differences are not statistically established.
- Fresh process for each `search` / `history` mode; deterministic QA runs first.
- Wall and process CPU deltas are milliseconds. CPU is `getrusage(RUSAGE_SELF)` user
  plus system time. RSS is process lifetime high-water KiB, **including setup, all
  prior phases, and warmups**; it is not live heap, a phase delta, or a memory-savings
  measurement. Allocations, assertions, and drops inside each closure are timed.
- Baseline compilation reused the head's generated `ui/dist` because production UI
  source is unchanged (UI differences are tests only). This harness never executes UI.

## Workloads and correctness

| Path | Real-pipeline assertions / workload |
|---|---|
| Search QA | 12 expected-coordinate cases: ASCII overlaps and case sensitivity, wide CJK, combining-cell boundaries, final sigma context, expanding lowercase İ, Kelvin sign, absent/empty queries, scrollback |
| Search timing | 2,000 rows × 230 populated cells in 240 columns; 96-byte ASCII queries (missing/overlapping) or 32-character Unicode query; insensitive Unicode fallback uses 200 rows. Initial feed excluded; Ghostty cell extraction included |
| Multipart image recovery | Two-pixel RGB image split around text and resize; snapshot replay and session export/import preserve text, placement, and decoded RGBA |
| Representative mixed output | 120 text lines plus decoded 64×64 RGB image; publication in awkward 8,191-byte chunks, snapshot, actual Ghostty reconstruction; marker searches plus exact 4,096 red RGBA pixels asserted |
| Saturated history | 64 KiB and default 8 MiB image budgets; 512 placement commands (10,752 bytes) per publish sample, prefill excluded; full retained command ordering/length and text marker checked |
| Snapshot / reconstruction | Measured separately from publication. Default 8 MiB retains 399,457 command groups; reconstruction feeds actual Ghostty |

**Saturated fixtures are placement-only replay:** the initial image transmission has
been evicted. They stress retention accounting, not visible image handover. Visible
image correctness is independently asserted by the multipart and mixed-output cases.

## Results

| Phase | Baseline wall ms | Head wall ms | Baseline/head | Baseline CPU ms | Head CPU ms |
|---|---:|---:|---:|---:|---:|
| `search_ascii_missing` | 2685.335 | 187.176 | 14.35× | 2685.181 | 187.176 |
| `search_ascii_overlaps` | 2710.580 | 1009.498 | 2.69× | 2710.379 | 1009.165 |
| `search_unicode_sensitive` | 1649.108 | 413.160 | 3.99× | 1649.023 | 413.156 |
| `search_unicode_insensitive` | 608.018 | 583.101 | 1.04× | 607.995 | 583.082 |
| `mixed_hub_publish_snapshot_ghostty_reconstruct` | 22.869 | 5.425 | 4.22× | 22.872 | 5.445 |
| `hub_publish_saturated_65536` | 16.028 | 1.413 | 11.34× | 16.030 | 1.414 |
| `hub_snapshot_65536` | 0.263 | 0.294 | 0.90× | 0.264 | 0.295 |
| `ghostty_reconstruct_65536` | 0.996 | 1.066 | 0.93× | 0.997 | 1.068 |
| `hub_publish_saturated_8388608` | 1795.746 | 1.472 | 1220.22× | 1795.588 | 1.473 |
| `hub_snapshot_8388608` | 36.155 | 29.838 | 1.21× | 36.159 | 29.834 |
| `ghostty_reconstruct_8388608` | 92.435 | 73.910 | 1.25× | 92.409 | 73.910 |


Largest reported lifetime peak RSS: baseline search: 16,164 KiB; baseline history: 83,888 KiB; head search: 16,236 KiB; head history: 83,912 KiB.

All final runs exited successfully, including representative exact-RGBA assertions.
Raw [baseline search](evidence/baseline-search.jsonl), [head search](evidence/head-search.jsonl),
[baseline history](evidence/baseline-history.jsonl), and [head history](evidence/head-history.jsonl).

The 64 KiB snapshot and reconstruction medians increased by 0.031 ms (12%) and
0.070 ms (7%) respectively; this run does not establish their cause or significance.
The representative mixed timing also includes two searches, so its improvement is not
isolated to history retention. The 1,220× saturated-publication result is an intentionally
pathological many-small-group workload, not expected overall terminal speedup.

The unchanged Unicode-insensitive fallback is included as a control, not claimed as
an optimized path. Snapshot/reconstruction did not receive this PR's retention-eviction
algorithm change; small movements there should not be attributed to that optimization.

## Reproduce

Use two clean git worktrees under the host's mandated fixed worktree root, one at each
revision, initialized at the locked Ghostty submodule revision. Copy this exact example
into both revisions; verify the SHA above. Install/build prerequisites normally and
ensure `ui/dist` exists. Use identical toolchain/sysroot/cache settings in both worktrees:

```sh
export CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
# Set CARGO_TARGET_DIR to the host's shared target outside both worktrees.
cargo build --offline --locked --manifest-path src-tauri/Cargo.toml \
  --example perf_terminal_pipeline
# Copy binary outside the shared target before compiling the other revision.
cp "$CARGO_TARGET_DIR/debug/examples/perf_terminal_pipeline" "$EVIDENCE/pipeline-$REV"
"$EVIDENCE/pipeline-$REV" search > "$EVIDENCE/$REV-search.jsonl"
"$EVIDENCE/pipeline-$REV" history > "$EVIDENCE/$REV-history.jsonl"
```

Do not measure while the other build/benchmark runs. Offline builds require an already
populated dependency cache. `qa` mode runs only deterministic correctness checks.
On non-Unix platforms CPU/RSS fields are unavailable (zero with availability flag false).

## Scope and remaining acceptance

This proves the production parser/search/history paths under controlled in-memory byte
feeds, **not** actual PTY delivery, daemon recovery across processes, UI event scheduling,
GPU/compositor rendering, macOS acceptance, or end-to-end input latency. No local
socket bind or restriction bypass was attempted. See the separate
[Linux offscreen evidence](../evidence/pr24-linux-offscreen/README.md) for renderer
checks and known pre-existing combining/emoji rendering gaps. Full interactive desktop
acceptance remains distinct from this headless pipeline QA.


## Refreshed production-source measurements — 504b1b15

After the legacy-PTY route correction, the actual production library and this unchanged
harness were rebuilt in **3m03s**. Separate `qa`, `search`, and `history` processes all
exited successfully, including the exact decoded-RGBA and multipart-resize/handover
assertions. The earlier table/raw runs above are retained rather than overwritten.

- Actually compiled local commit: `8b72ee5cb50cf44d7e4a2ab2b0e28a51a93ef27c`;
  corresponding remote source: `504b1b159c2f5dbde9dc88ae643b6b45ae7d7e3a`;
  tree: `05d3ba9fe36c8bd0dfe65a16a11fb57173eea417`.
- Identical harness SHA-256:
  `c9a924b4f7178d64c2bda9b21529c3df075bb85da48d7587bc93aac0b541bd13`.
- Same machine, compiler, Cargo.lock, Ghostty revision, default features and unoptimized
  dev profile as the original runs. Baseline measurements are reused from earlier
  the same day; head was measured later, with no concurrent builds/benchmarks.
- Five samples after two warmups, per phase. These sequential runs are not a randomized
  paired experiment; ranges show noticeable variation, not statistical confidence bounds.
- Raw [search samples](evidence/head-504b1b15-search.jsonl),
  [history samples](evidence/head-504b1b15-history.jsonl), and
  [exact provenance/metadata](evidence/head-504b1b15-metadata.json).

| Phase | Baseline median ms | Refreshed median ms | Refreshed min–max ms | Baseline/refreshed | Refreshed CPU median ms |
|---|---:|---:|---:|---:|---:|
| search_ascii_missing | 2685.335 | 202.149 | 181.303–277.113 | 13.28× | 202.149 |
| search_ascii_overlaps | 2710.580 | 1240.322 | 1132.502–1306.789 | 2.19× | 1240.136 |
| search_unicode_sensitive | 1649.108 | 477.071 | 415.072–494.481 | 3.46× | 477.041 |
| search_unicode_insensitive | 608.018 | 568.472 | 546.123–624.188 | 1.07× | 568.417 |
| mixed_hub_publish_snapshot_ghostty_reconstruct | 22.869 | 4.917 | 4.831–5.320 | 4.65× | 4.918 |
| hub_publish_saturated_65536 | 16.028 | 1.312 | 1.227–1.381 | 12.22× | 1.313 |
| hub_snapshot_65536 | 0.263 | 0.249 | 0.227–0.364 | 1.06× | 0.250 |
| ghostty_reconstruct_65536 | 0.996 | 1.035 | 0.942–1.090 | 0.96× | 1.036 |
| hub_publish_saturated_8388608 | 1795.746 | 1.381 | 1.283–1.560 | 1300.09× | 1.382 |
| hub_snapshot_8388608 | 36.155 | 32.072 | 29.029–35.269 | 1.13× | 32.068 |
| ghostty_reconstruct_8388608 | 92.435 | 82.088 | 74.478–87.844 | 1.13× | 82.089 |


Wall/CPU values are milliseconds; CPU is process user+system delta. Largest lifetime
RSS was **16,264 KiB** for refreshed search and **83,780 KiB** for refreshed history
(baseline: 16,164 / 83,888 KiB). These include QA, preparation, prior phases and warmups;
they are **not phase heap measurements or evidence of memory savings**.

The 64 KiB reconstruction median increased from 0.996 to 1.035 ms (about 4%); its
small change and the unchanged Unicode-insensitive control are not attributed to an
optimization. Mixed-output timing includes two searches. The roughly 1,300× saturated
publication ratio applies only to the pathological many-small-placement-groups fixture,
not whole-terminal speed. That saturated replay no longer retains the initial image
transmission; independent decoded-image tests provide the image-correctness evidence.

These refreshed measurements still exclude real PTY delivery, sockets, GPU/compositor
rendering and interactive desktop acceptance. **They do not establish a current native
GUI pass**, on Linux or macOS. Later QA-only script/workflow changes do not change the
measured production/build/harness sources; exact revision equivalence is recorded in
metadata without relabeling the binary's actually compiled revision.
