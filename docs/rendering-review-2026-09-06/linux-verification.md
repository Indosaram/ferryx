# Native Linux validation audit

The independent verifier subagent (`linux-verify`) failed before starting work:
its model returned OpenAI 429 with a shared-quota cooldown until
2026-09-06T23:29:13Z. The lead therefore performed the audit directly, opening
every evidence artifact with its own tool calls on 2026-09-06 ~23:20-23:27 UTC.
Nothing in this report relies on the failed node's output.

## What was audited, artifact by artifact

Each claim in `linux-validation.md` was checked against the raw files under
`.omo/evidence/ulw/rendering-review-20260906/linux/`:

- `native/exits.tsv`: all twelve execution stages exited 0 (`cargo-check`,
  `cargo-contracts`, `cargo-wire`, `cargo-host`, `ui-install`, `ui-build`,
  clone/checkout/overlay/Ghostty stages, `compare-overlay`,
  `compare-foreign`, `compare-ghostty`, `compare-daemon`,
  `compare-hyprland`). Exactly two nonzero exits are recorded, both expected
  and disclosed: `compare-source` and `compare-staged-status` (exit 1) caused
  solely by Tauri-generated `gen/schemas/linux-schema.json` inside owned
  staging. `verification-summary.json` confirms the 805-file diff contains only
  that path with before/after hashes `db7f71ef…` and `223fd6e9…`.
- `native/runner.log`: UTC boundaries match the report (start 23:00:35Z, last
  test END `cargo-host exit=0 23:07:22Z`, cleanup compares 23:07:24Z). The
  environment block records rustc/cargo 1.98.0, host
  `x86_64-unknown-linux-gnu`, Bun 1.4.0, Zig 0.16.0, GTK 3.24.52,
  WebKit2GTK 2.52.6, 713 GiB free, `CARGO_BUILD_JOBS=4`, `TAURI_CONFIG=unset`,
  CPU affinity 0-3, and `base.bundle: OK` / `d3-sources.tgz: OK` verification.
- Test counts: `runner.log` tail shows `receipt_reports_effective_presentation_scale
  ... ok. 1 passed; 0 failed; 0 ignored; 576 filtered` for the host receipt;
  `verification-summary.json` records contract counts `[5, 7, 12]` plus wire
  `[1]` and host `[1]` = 26 executed tests, matching the report.
- Source provenance: `native/archive-inspection.log` lists the nine D3 overlay
  SHA-256 values; they are byte-identical to the local
  `local-archive.sha256` listing frozen before transfer, and to
  `parent-ledger.json` (steering entry 22:54:57Z). The lead additionally
  re-hashed the nine live files in the repair worktree after the Linux run:
  all nine still match the frozen archive hashes, so the committed delta and
  the validated sources are the same bytes.
- Native identity: `native/native-executables.txt` shows all four test
  binaries are x86-64 Linux ELF executables; `native/ui-dist.sha256` contains
  28 UI resource hashes; `native/native-ghostty.sha256` and
  `native/generated-Cargo.lock` are present as claimed.
- Cleanup: `cleanup.log` records mode `remove`, the owned staging root
  `/home/indo/.omo-d3-st_01a078ef-6egcgRrS`, `owned_processes: []`, foreign
  daemon `PID 41475` and Hyprland `PID 321320` identical before/after, and a
  verified REMOVED line. `retrieval-verification.log` lists 54 evidence files
  all `OK` (hash-verified locally before remote removal).

## Scope conformance

- Deliverables owed: native compile + 26 selected tests on frozen sources,
  numeric geometry/receipt evidence, preservation and cleanup receipts, and a
  report separating code checks from unrun GUI. All present.
- Nothing beyond scope: no production/test file was edited anywhere (only the
  generated schema inside owned staging changed, and staging was deleted); the
  original local snapshot (899 files, HEAD `37272f52`) and the existing
  `/home/indo/rel-0906` checkout (937 files) are recorded unchanged; no
  release/app/daemon launch; no toolchain or system package installed.

## Verdict

The Linux code-validation lane is verified: the exact frozen D3 delta
(base `c349e3a` + nine overlay paths) compiles and passes all 26 requested
contract/receipt tests on native x86-64 Linux with honest disclosure of the
two expected generated-schema exits, and cleanup receipts are complete.

Not proven by this lane, unchanged from the validation report: Wayland
compositor pixels, real `wl_surface` presentation, fractional-output
rendering, live pane masking, interactive IME anchor placement, and physical
desktop scale changes. Those remain GUI-runtime evidence, still pending.
