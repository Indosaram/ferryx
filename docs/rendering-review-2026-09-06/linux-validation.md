# Frozen D3 native Linux validation

## Result

**PASS for native Linux compilation and the requested code contracts.** Normal
`cargo check` passed, and **26 tests executed and passed**: 5 child-surface,
7 composition, 12 Wayland-subsurface, one exact wire receipt and one exact host
receipt regression. No failed, ignored or zero-match test run is counted as a
pass. **Wayland GUI acceptance remains unrun.** No application, daemon, release
binary, compositor interaction or desktop input was launched by this lane.

Execution: 2026-09-06 23:00:35-23:07:24 UTC, task `st_01a078ef`.
Evidence root (`E` below):
`.omo/evidence/ulw/rendering-review-20260906/linux/`.

## Source and native identity

- User-designated bench: `indo@100.91.254.71`, hostname `indo`, Arch Linux
  `7.1.9-arch1-2`, native `x86_64-unknown-linux-gnu`.
- Rust `1.98.0` (`88d9e12ae`, LLVM 22.1.8), Cargo `1.98.0`, Bun `1.4.0`, Zig
  `0.16.0`, GTK `3.24.52`, WebKit2GTK `2.52.6`. Actual staging preflight had
  713 GiB free on `/home`; 12 CPUs and 15 GiB RAM. Cargo used four jobs and the
  runner's CPU affinity was restricted to CPUs 0-3, inherited by Zig.
- Exact base: `c349e3a6ffa1ff36cef30ac4d66dd78250e3f428`; detached independent
  checkout plus exactly nine archive paths. This is an immutable input overlay,
  **not a new commit** and not the concurrently dirty local working tree.
- `base.bundle` SHA-256:
  `ee0e602c7aa674097e33b03bf35d64f1382a011f5eef5369419de1209ce8076b`.
- `d3-sources.tgz` SHA-256:
  `275c570e402b8fa76971e8c2fc8de6e538d2a92c2704270c05e65118f45dabee`.
- All nine regular-file paths and hashes were checked before extraction, locally
  against the parent's ledger and independently on Linux. Duplicate, unexpected,
  non-regular and escaping paths were rejected by the staging script.
  `E/parent-ledger.json`, `local-archive.sha256`, and
  `native/archive-inspection.log` retain this provenance; `native/staged-overlay.patch`
  retains the actual base-to-overlay diff.
- Ghostty `6a508fd5e34c7e222c052a6d00bb3891ff3feace` was cloned with
  `--no-hardlinks --no-checkout` from the existing
  `/home/indo/rel-0906/src-tauri/vendor/ghostty`, then checked out only inside
  owned staging. Both Ghostty source snapshots stayed identical. Native build
  outputs record that SHA, Zig 0.16.0, Cargo target `x86_64-unknown-linux-gnu`,
  and Zig target `x86_64-linux-gnu`.
- `native/native-executables.txt` identifies all four test binaries as x86-64
  Linux ELF executables. Their SHA-256 values and the freshly built Ghostty
  libraries' hashes are retained in `native/native-executables.sha256` and
  `native/native-ghostty.sha256`.

### Ordinary UI and dependency preparation

`bun install --cwd ui --frozen-lockfile` and `bun run --cwd ui build` each exited
0. The actual package script ran `tsc && vite build` (1,863 modules), producing
28 resource files, including `ui/dist/index.html`; all resource hashes are in
`native/ui-dist.sha256`. The Tauri build outputs enumerate those same resource
paths. `TAURI_CONFIG` was unset: no resource bypass or empty fixture was used.

The base does not track a Cargo lockfile. The requested normal Cargo command
resolved 751 compatible packages; this is **not a frozen Rust dependency claim**.
The exact generated lockfile is retained as `native/generated-Cargo.lock`, SHA-256
`e9009087faa862b51beae7d377fd0aac51ae85fb2670a13ae7b7399f040b385b`.
Existing Cargo/Bun caches were reused non-destructively; build targets and Zig
build caches were fresh and owned. No toolchain/system package was installed,
no WSL was used, and no lock holder was killed.

## Executed commands and counts

All commands ran from the owned source checkout, with default features and no
cross-target override. `native/runner.log` records exact commands, UTC boundaries
and exits; `native/exits.tsv` is the machine-readable exit ledger.

```sh
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract --test native_terminal_composition_contract --test native_terminal_child_surface_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal::tests::wire_receipt_reports_effective_presentation_scale -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::receipt_reports_effective_presentation_scale -- --exact --nocapture
```

| Evidence under `E/native/` | Executed result | Exit |
| --- | --- | --- |
| `cargo-check.log` | Native dev check finished, 2m45s | 0 |
| `cargo-contracts.log` | Child 5/5, composition 7/7, Wayland 12/12; none filtered | 0 |
| `cargo-wire.log` | Exact wire regression 1/1; 576 filtered | 0 |
| `cargo-host.log` | Exact host regression 1/1; 576 filtered | 0 |

Each requested command ran once. No unrelated all-tests gate, UI test suite or
GUI test was substituted. Test compilation/linking took 3m21s for the contract
targets and 25.74s for the library target. Existing source warnings remain in
the raw output: nine library warnings and eleven library-test warnings, including
unused imports/variables and dead-code warnings. No warnings were suppressed or
code/assertions changed to make these commands pass.

## Numeric geometry and receipt evidence

The unchanged contract tests print actual production geometry/state values in
`native/cargo-contracts.log`:

- Fractional logical extent: integer buffer scale **2**, buffer **802x600**,
  divisible by the scale; logical origin **(0,0)**.
- Raw DPR **1.5**: child logical position **(10,20)**, scale **2**, child buffer
  **800x600**; host physical bounds **(20,40,800,600)** and grid **40x15**.
- Shared-edge case, first pane: logical **(10,20,401,301)** at raw DPR **1**
  gives scale **1**, buffer **401x301**, cells **7x14**, grid **57x21**.
  Raw **1.5** and **2** give scale **2**, buffer **802x602**, cells **13x28**,
  grid **61x21**. Raw **2.5** gives scale **3**, buffer **1203x903**, cells
  **20x41**, grid **60x22**. Adjacent panes share logical edges **x=411** and
  **y=321**; all twelve pane/DPR state records are in the raw log.

The two exact receipt tests do not print numeric values. Their successful
assertions in the hash-verified frozen source establish raw DPR **1.5** becoming
stored effective scale **2.0**, physical cells staying **16x32**, the actual host
getter returning `Some(2.0)`, and the real getter/converter/serialization chain
emitting JSON `effectiveScaleFactor: 2.0`, `cellWidthPx: 16`, `cellHeightPx: 32`.
These are executed assertion values, **not claimed console prints or observed
compositor measurements**. Their input logical extent is **800x480**. Tauri's
MockRuntime supplies only the unused window boundary; no native window is opened.

## Preservation, generated output and cleanup

Owned staging was `/home/indo/.omo-d3-st_01a078ef-6egcgRrS`, with an explicit
`st_01a078ef` ownership marker. Scripts were created locally through `apply_patch`
and transferred. No remote source text edit or production/test fix was made.

- All nine overlay hashes are equal before/after. Both input archives remained
  unchanged. The original local source snapshot (899 non-doc/non-evidence files,
  HEAD `37272f52dd4adf55d02d6b9f87fbf048c41acd04`) is byte-identical before/after.
- The existing `/home/indo/rel-0906` source snapshot (937 files) and its Ghostty
  source snapshot (5,864 files) are unchanged. No existing checkout was built in
  or cleaned. Existing applications/session were not manipulated.
- **The full owned-checkout comparison did not pass unchanged:**
  `compare-source` and `compare-staged-status` each exited **1** because normal
  Tauri generation updated tracked `src-tauri/gen/schemas/linux-schema.json`.
  The 805-file snapshot diff contains only that path: added process/updater
  permission schemas. This is generated output, not Rust/UI/test source drift.
  Before hash `db7f71ef0dc1889e37b1a95b03ec71b36b797429845bb8f4d3b7f8fb5482e288`;
  after hash `223fd6e9b6f563223374f0bd9b075f566e27ade097ebb3c475abae666e71c5ab`.
  Both files and `native/generated-schema.patch` are retained. The nonzero
  comparisons were preserved, not hidden or "fixed" by restoring source.
- Foreign daemon **PID 41475**, original start time and executable/arguments
  `/tmp/.mount_FerryxppnJnn/usr/bin/ferryx --daemon` remained identical.
  Hyprland **PID 321320** remained identical, including its pre-existing
  `--safe-mode` flag. These facts are in before/after receipts and `E/cleanup.log`.
- All native commands exited. The `/proc` cleanup inspection found **zero owned
  remaining processes**, excluding only its own inspector/SSH ancestors. Logs
  were retrieved and all **54 remote evidence-file hashes** verified locally
  (`retrieval-verification.log`, exit 0) **before removal**.
- Cleanup then removed the entire owned staging tree, source, node_modules,
  native test binaries, target and Zig outputs; verified the path was absent;
  and rechecked daemon and Hyprland survival (`cleanup.log`, exit 0). No source
  or process was retained for GUI QA. Shared user caches were retained.

`E/verification-summary.json` checks the source comparison exception, counts,
command exits, input hashes and cleanup receipts. Evidence Python scripts have
clean LSP diagnostics. Shell scripts pass `bash -n`; shell LSP was unavailable
and was not installed. These script checks are not additional product tests.

## Acceptance boundary

This closes the requested **native Linux code-validation** lane without a
dependency/code blocker. It does not prove Wayland compositor pixels, real
`wl_surface` presentation, fractional-output rendering, live native pane masking,
interactive IME composition/anchor placement or physical desktop scale changes.
Those GUI checks remain explicitly unrun. No claim is made for a full app/release
build or for a different commit, overlay or Cargo dependency resolution.
