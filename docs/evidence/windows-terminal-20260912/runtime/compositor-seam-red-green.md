# Windows compositor descriptor seam: RED/GREEN

Task st_01a09658, 2026-09-12. Commands and assertion pinned before execution.

## Pinned execution

Working directory: `/Users/indo/code/project/orca-lite`.
Test name: `windows_platform_descriptor_passes_desktop_composition_validation`.
Identical assertion: invoke the extracted Windows target's `descriptor()`, then
`descriptor.validate_desktop_composition().expect("platform descriptor must pass desktop composition validation")`.
No assertion pins error prose; the actual error is retained in RED output.

```sh
bun docs/evidence/windows-terminal-20260912/runtime/compositor-seam/run.mjs red
bun docs/evidence/windows-terminal-20260912/runtime/compositor-seam/run.mjs green
```

The Bun runner monitors each child through its exit event and piped output with
a 60-second kill timeout (no sleeps/polling). Each mode compiles with:

```sh
rustc --edition=2021 --test docs/evidence/windows-terminal-20260912/runtime/compositor-seam/MODE.rs -o docs/evidence/windows-terminal-20260912/runtime/compositor-seam/MODE-test
docs/evidence/windows-terminal-20260912/runtime/compositor-seam/MODE-test --exact windows_platform_descriptor_passes_desktop_composition_validation --nocapture --test-threads=1
```

`MODE` is `red` then `green`. Expected compile exit: 0 both; expected test/runner
exit: 101 RED, 0 GREEN. Monitor receipts are `MODE-result.json`; generated Rust
and full stdout/stderr are retained alongside them. No UI tests/build rerun.

## Source selection and limits (pinned)

RED descriptor: `2a644262314cb9e3248d27c90c3fa742d1ae6b45` (`3fa25a19^`).
Fixed historical descriptor: `3fa25a1942ebb365515f8f8854a765c356bdf154`.
GREEN: current working-tree Windows descriptor (HEAD observed
`e06db23db4eefee4caecb432618cb1fc06bcfdcb`).
The runner requires GREEN descriptor bytes to equal the fixed historical method,
and requires historical/fixed/current validator method bytes to be identical.
Both runs use the current actual validator, enum body and descriptor struct body
extracted from `src-tauri/src/native_terminal/composition.rs`.

Portable scaffolding replaces the handle-owning target with a unit struct because
the descriptor does not read `self`. It supplies a Debug-only
`NativeTerminalError::GpuPipelineError(String)` and native Debug/PartialEq derives
instead of serde derives. The descriptor and validation methods are copied
byte-for-byte; no flags, conditions or errors are rewritten. This is a source
seam regression artifact, not an execution of the full Windows module or app.

Pre-test tooling deviation: the first RED runner invocation exited 1 before
compilation (`apply_patch: expected Begin/End Patch`). Bun's stdin sink `end(patch)`
did not send the patch. Corrected to `write(patch); await end()`; no Rust test had
run. The pinned commands and assertion are unchanged. `apply_patch` is the
workstation-provided helper (internally Python); all authored tooling is Bun/JS.

## Observed results

Host tools: Bun 1.4.0; rustc 1.92.0 (ded5c06cf 2025-12-08), Darwin arm64.
Both Rust compilations exited 0 without warnings. Both monitors completed without
timeout. RED test/runner exited **101**; GREEN test/runner exited **0**.

RED (full output: `compositor-seam/red-test.log`):

```text
running 1 test
test windows_platform_descriptor_passes_desktop_composition_validation ... descriptor=PlatformCompositorDescriptor { target_kind: WindowsChildWindow, pointer_transparent: false, layer_backed: false }
FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
platform descriptor must pass desktop composition validation: GpuPipelineError("Windows native terminal composition target is not layer-backed (child window clipping not established)")
```

GREEN (full output: `compositor-seam/green-test.log`):

```text
running 1 test
test windows_platform_descriptor_passes_desktop_composition_validation ... descriptor=PlatformCompositorDescriptor { target_kind: WindowsChildWindow, pointer_transparent: true, layer_backed: true }
ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

The generated Rust diff changes only the two descriptor booleans, sourced from
the actual historical/current implementations rather than supplied by the test.
The assertion and validator are identical. Validator SHA-256:
`754af718fb0ad4efc8e1e8bdf24c6e9152dab745695e1b2ef34dd613d716fb95`.
Full source fingerprints and exact command arrays are in both result JSON files.
The fixed commit's descriptor equals current byte-for-byte; the validator equals
both historical versions byte-for-byte (runner checks passed).

For replay with retained artifacts, run the pinned `rustc` and test commands with
MODE expanded to red/green. The extraction runner deliberately uses add-only
`apply_patch`: rerun extraction in a fresh evidence directory/tree, rather than
overwriting the retained receipts. Local test executables are Darwin test binaries,
not Windows executables.

## Why this is the relevant seam

`NativeTerminalSurfaceHost::new` calls `NativeSurfaceFrameTarget::new`
(`surface_host.rs:2378`); the latter creates `PlatformCompositorTarget`, obtains
its descriptor and validates it before renderer/GPU surface creation (:2523-2525).
`platform/mod.rs` forwards `descriptor()` directly to the inner Windows target.
The historical constructor retained the parent HWND; the fixed/current constructor
creates a real child with `CreateWindowExW`, WS_CHILD and transparent/noactivate
styles. This harness deliberately does not call those constructors.

The existing
`native_terminal_host_composition_target_must_be_platform_child_view_not_root_window`
contract calls `PlatformCompositorDescriptor::active_for_platform()`, whose Windows
branch still represents an unattached false/false descriptor. The composition
unit tests likewise use synthetic descriptors. Neither is the actual Windows
method regression required here; changing those tests or the validator would not
repair the installed binary. The isolated source extraction is therefore justified.

`installed-compositor-binary.md` independently supplies positive installed PE
evidence: the constructor retains the root handle, the caller supplies descriptor
bytes false/false/kind2, and the Windows validator rejects with exactly the RED
message. This source execution supports that mechanism, not exact whole-binary
source provenance.

## Adjacent existing coverage (read, not rerun)

- `native_terminal_child_surface_contract.rs`:
  `child_surface_geometry_preserves_scaled_origin` checks scaled pane origin and
  extent; `child_surface_geometry_rejects_non_positive_extent` rejects zero width,
  rounded-to-zero height and NaN; `child_surface_geometry_clamps_negative_origin_into_parent`
  clamps origin. These test geometry policy, not Win32 placement.
- Same file: `child_surface_stays_hidden_until_first_present` checks early hidden
  state and one-time reveal policy;
  `detached_child_surface_is_not_resurrected_by_a_late_present` checks detach policy.
- `surface_host.rs`:
  `attach_daemon_attachment_with_bounds_tolerates_invalid_initial_bounds` uses
  zero-by-zero early bounds and verifies fallback to daemon dimensions 80x24;
  `reattach_existing_session_with_bounds_tolerates_invalid_bounds` preserves 80x24.
  `ghostty_grid_resize_notifies_pty_with_matching_dimensions` changes width 400 to
  800 and asserts the synchronous resize sink receives exact resulting grid dimensions.
  These cover early bounds/resize, not the Windows descriptor/validator connection.
- Previously completed UI 62 tests/build remain the receipts in
  `../combined-verification.md`; this task did not rerun or newly certify them.

## Verification and boundaries

LSP: runner clean before execution; a final runner diagnostics refresh timed out
after 3000ms (not a clean post-correction diagnostics claim). The corrected runner
executed both specimens successfully. Generated Rust directory clean after execution
(2 Rust files, zero diagnostics). Rust compilation and the real local test entry
point were executed for both specimens. `git diff -- src-tauri ui` was empty both
before and after runs. All authored files are this report and the isolated
`compositor-seam/` artifacts; no product/test/foreign files were edited, no commit
created, no daemon contacted or modified, and no Windows app/binary executed.

**This proves descriptor-to-validation RED/GREEN only.** It does not exercise
Win32 creation, HWND ownership, clipping, geometry application, z-order, GPU
presentation, Tauri dispatch, shell selection, or the live installed process.
In particular, it **does not recover the installed process's raw console event**
or establish that its observed banner came from this branch rather than an earlier
failure. No speculative production fix or weakened validation is proposed.
