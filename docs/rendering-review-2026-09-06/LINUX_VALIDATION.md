# Native Linux code validation on omaki

## Runner and preservation boundary

Use the user-designated omaki bench, `ssh indo@100.91.254.71`. WSL was retired
and has no distribution; do not install or restore it.

Read-only preflight confirmed Arch Linux 7.1.9, Rust/Cargo 1.98.0, Bun 1.4.0,
Zig 0.16.0, GTK 3.24.52 and WebKit2GTK 2.52.6. The user has an active Hyprland
session and a foreign Ferryx AppImage daemon PID 41475. Do not stop, attach to,
replace or manipulate that daemon or the user's desktop.

## Frozen source

The lead supplies the exact verified D3/receipt commit after its local acceptance.
Use a uniquely owned remote staging checkout of that commit, not the live remote
release/build directory. Record commit and scoped file hashes. Preserve the
existing source, working changes, apps and build processes. Reuse existing caches
only non-destructively. The pinned Ghostty source remains
`6a508fd5e34c7e222c052a6d00bb3891ff3feace`.

## Required native commands

With ordinary generated UI resources and no `TAURI_CONFIG` resource override:

```sh
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --test native_terminal_wayland_subsurface_contract --test native_terminal_composition_contract --test native_terminal_child_surface_contract -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::native_terminal::tests::wire_receipt_reports_effective_presentation_scale -- --exact --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib native_terminal::surface_host::tests::receipt_reports_effective_presentation_scale -- --exact --nocapture
```

Record native target/toolchain, source identity, exact command, executed counts,
numeric geometry/receipt values and exit codes. Source tests must execute; a
zero-match or unavailable dependency is not a pass. Do not alter production code
or test assertions merely to make this validation green. Report a concrete
failure to the lead for a separately verified in-scope fix.

This phase proves Linux compilation and portable code behavior. It does not
launch a GUI and cannot prove Wayland compositor pixels, interactive IME or
native pane masking. Those remain separate acceptance work.

## Artifacts and cleanup

Write `linux-validation.md` and raw logs/hashes under the rendering evidence
directory. Retrieve evidence before deleting owned remote staging. Confirm all
owned test/build commands exit and remove only owned staging/source artifacts;
retain user caches and the existing daemon/session. Any source retained for
further explicitly authorized GUI QA must be named and accounted for.
