# P18 implementation receipt

Task st_01a09a07; 2026-09-13. Implementation complete for approval-independent prerequisites; aggregate Windows objective is NOT complete. Changes remain uncommitted in the shared checkout.

## Registration and ownership

Read root/src-tauri AGENTS, programming/Rust skill, repair packet, addendum, remaining register, packaging and tooling source evidence. Inspected current status/diff: P18 source files were clean; foreign changes were preserved. Official `executeAgentToolkit` with repository resolveCwd and parent session `01a0983f-c995-753d-afa9-593f6d118788` accepted `steer/revise_criterion` for C002 before repair, appending exact checks while preserving its full existing scenario. Receipt: `p18/registration.json`.

## Delivered

- `src-tauri/build.rs`: both MSVC manifest-directive branches require Windows AND target_env=msvc. GNU/other targets retain the default Tauri manifest builder. Existing MSVC directives unchanged.
- `src-tauri/tests/windows_edge_probe_contract.rs`: preserved test name and replaced ephemeral ignored PowerShell include/text matching with checked-in portable Node fixture execution. Both fixture files are compile-time source dependencies.
- `scripts/fixtures/{run-edge-probes,probe-daemon-edges}.mjs`: stage a real driver beside no user files in a fresh owned temporary directory; execute with nonce and canonical-path receipt; remove precisely that directory and assert removal. No daemon, protocol-2 behavior, profile, settings, endpoint, socket, polling, fixed cleanup root, or PowerShell execution. This is staging/cleanup coverage, NOT daemon integration evidence.
- `.github/workflows/build-test.yml`: Windows MSVC executes two nonzero filesystem/config contract targets, `windows_edge_probe_contract` and `windows_window_opacity_contract`, single-threaded with bash continuation semantics. No broad lib/persistence/GPU targets are enabled pending other owners' repairs. No hosted releases or quiescence CI gate added.
- `scripts/release-workflow.test.mjs`: parses actual YAML matrix, runner, target and guarded commands; rejects compile/list-only execution and inappropriate persistence/lib substitutions; verifies target test presence and excludes Unix/Linux-only target gates.
- `scripts/check-tree-quiescent.sh`: GNU metadata formatting with BSD fallback; use draining awk rather than head to avoid pipefail SIGPIPE when listing more than ten files.
- `scripts/fixtures/p18-prerequisites.test.mjs`: owned clean-layout compile/run, actual build.rs control-flow probe with stubbed external builders, old/recent/invalid metadata checks using controlled mtimes and GNU-compatible stat adapter. All roots removed and removal asserted; foreign sentinel retained.
- `scripts/fixtures/run-p18-cargo.sh`: exact offline Cargo target runner; refuses without explicit lead-authorized slot.

## RED / GREEN

Commands executed on Darwin, no Cargo execution or dependency installation:

1. `node --test scripts/fixtures/p18-prerequisites.test.mjs`
   - RED exit 1: intended clean-layout missing `.omo/.../run-edge-probes.ps1` compile failure; real build-script Windows/GNU output incorrectly contained `/MANIFEST`; GNU-compatible stat recent-file output lacked metadata. Four other cases passed. `p18/prerequisites-red.log`.
   - GREEN exit 0: 7/7, no skips; same behavioral assertions plus foreign-sentinel preservation. `p18/prerequisites-green.log`.
   - The missing-source compile failure is the specified PKG-02 defect, not an unrelated dependency failure. Standalone rustc compiles and executes the actual edge contract without Cargo's large dependency graph. Build-script probe certifies emitted directives/control flow, NOT GNU linking or native MSVC loading.
2. `node --test scripts/release-workflow.test.mjs`
   - RED: new Windows case failed `Windows must execute tests, not just check/link`.
   - After fix: Windows case passes. Full suite remains exit 1, 16/17 passing: pre-existing Pages policy test references missing `.github/workflows/deploy-pages.yml` (`Target not found`). `git ls-files .github/workflows/` lists only build-test.yml. No unrelated test was deleted/skipped or workflow invented. Logs `p18/workflow-{red,green}.log` retain this failure.
3. `node scripts/fixtures/run-edge-probes.mjs`: exit 0, `EDGE_FIXTURE_STAGED_EXECUTED_CLEANED`.
4. `bash scripts/fixtures/run-p18-cargo.sh` without slot: exit 2 before Cargo, as intended.

## Diagnostics and cleanup

LSP returned no diagnostics for build.rs, windows_edge_probe_contract.rs, release-workflow.test.mjs and all three fixture .mjs files. Bash/YAML language servers unavailable; installation forbidden and not attempted. `bash -n` (both shell files), `node --check` (four JS files), `rustfmt --check --config skip_children=true` (both Rust files), and scoped `git diff --check` all exit 0. Workflow parsing also executed in the policy test. `p18/validators.log` records commands' statuses and fixture cleanup sentinel. Test teardown removes and asserts absence of every fresh test root. No unowned daemon, desktop or remote host touched; no branch/worktree/commit/push/release/install operation performed.

## Exact remaining handoffs

- Lead exclusive Darwin Cargo slot: execute `P18_CARGO_SLOT=lead-authorized bash scripts/fixtures/run-p18-cargo.sh`, capture compiler and one-test execution receipts in p18/. Runner uses existing shared target, offline dependencies, exact owned test only. No slot arrived in this child session; Cargo build is unverified. This does not replace native acceptance.
- Native runtime owner `st_01a099f8` only: with existing MSVC provisioning run `cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --test windows_edge_probe_contract --test windows_window_opacity_contract --no-run`, retain compiler-artifact executable paths/hashes, then execute both exact targets (one test each) and capture normal load/exit without TaskDialogIndirect. Node must already exist on host. No install authorized. Execute runtime only through the owner's existing isolation/provenance rules.
- GNU actual link/runtime verification unavailable: no provisioned separately authorized GNU environment supplied. Emitted-flag probe is not reported as a GNU linker RED/GREEN.
- Full workflow suite requires lead disposition of the pre-existing missing deploy-pages.yml expectation; unrelated failing assertion remains intact.
- P06/P08/P10/P19 dependent integration suites intentionally not enabled prematurely. Lead may expand Windows CI targets after those owners supply isolation and native nonzero execution receipts. Current P18 Windows step is real safe contract coverage, not whole-domain CI/native certification.
