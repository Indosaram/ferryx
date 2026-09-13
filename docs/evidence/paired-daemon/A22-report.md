# A22 report

Status: partial delivery; full A22 acceptance is NOT claimed.

Environment: isolated worktree `/Users/indo/code/project/orca-lite-wt/herdr-resume-01a097f8`, Darwin arm64, Node v22.22.3. All UI commands used `PATH=/Users/indo/.local/bin:/Users/indo/.bun/bin:/opt/homebrew/bin:/usr/bin:/bin`. No commits, daemon operations, PTY interaction, desktop automation, deployment, or release builds.

## Changes owned by A22

- `ui/src/components/settings/PairedMachinesSection.tsx`: native machine management component, PIN pairing and re-pair form, inventory refresh, generation-bound capability checks, explicit machine identity/scope/transport/auth display, safe typed explanations, confirmed credential forgetting with cancel, and a rollout switch. Add Project is explicitly disabled without a navigation callback.
- `ui/src/components/settings/RemoteAccessSection.tsx`: mounts machine management only in native desktop; browser phone-mirror controls stay separate.
- `ui/src/lib/pairedHostInventory.ts`: persisted, default-off `pairedDaemonProjectsV1` preference drives the EXISTING `machineFeaturesEnabled` flag only with native proxy support. Refresh does not override rollback intent; no project/layout mutation or mirror branch is introduced.
- `ui/src/components/settings/PairedMachinesSection.test.tsx`, `pairedDaemonGate.test.ts`: deterministic settings and gate coverage.
- `ui/src/lib/pairedHostInventory.test.ts`: explicitly enables rollout in the existing advertised-proxy test, preserving that test's capability-failure assertions.
- `docs/paired-daemon-machine-access.md`, `ui/AGENTS.md`, `src-tauri/src/remote/AGENTS.md`: operator commands, architecture, machine-only raw-path exception, safe cleanup, version/scope guidance, honest current limitations.
- `docs/evidence/paired-daemon/A22-RED.log`, `A22-GREEN.log`, this report: evidence. Intermediate build-log file was merged and removed.

No forbidden file was edited. CLI implementation already supports owner-issued `pair generate --access machine`, so no CLI change was needed.

## Evidence

RED was recorded before implementation: missing component import plus an independent behavioral test proving the existing inventory wrongly enabled machineFeaturesEnabled by default when proxy support was advertised (expected false, received true).

The GREEN log retains an intermediate failure: mounting machine management in browsers broke two existing mirror-settings assertions. The fix is native-only mounting, not weakening those tests. Final required runs:

- `bun run --cwd ui test src/components/settings/PairedMachinesSection.test.tsx src/components/settings/pairedDaemonGate.test.ts src/lib/pairedHostInventory.test.ts src/components/settings/RemoteAccessSection.test.tsx`: exit 0, 47 tests.
- `bun run --cwd ui test src/appearanceThemeContract.test.ts`: exit 0, 7 tests.
- `bun run --cwd ui build`: exit 0 (TypeScript and Vite); existing chunk-size advisory remains.
- Diagnostics requested for every changed TypeScript file. No errors on component, inventory, inventory tests, or component tests. RemoteAccessSection reports the existing deprecated execCommand hint. Initial gate-test diagnostics were clear; subsequent refresh timed out; the final TypeScript build passed.
- Whole-worktree `git diff --check` reports unrelated trailing whitespace in `ui/src/state/inactiveProjectWorktrees.ts:218`; A22 did not edit this parallel-work file.

Tests prove native command routing, confirmation/cancel, generation-bound forgetting, re-pair PIN clearing without host selection, mirror/revoked/unsupported/native-unavailable/stale-generation presentation, default-off and persisted rollback intent, and fail-closed proxy gating. The layout preservation test retains seeded storage bytes; it does NOT prove real workspace restart restoration or native session survival.

## Remaining integration and human QA

- Direct settings Add Project navigation is not wired: SettingsDialog/App routing is outside this packet's settings/feature-gate lane, and Add Project dialog/picker changes were explicitly forbidden. The button is disabled with guidance, not falsely functional. Parent/A18 integration must connect this callback and prove the full surface.
- The advertised terminal proxy is false. Real remote terminals, remote CWD/process identity, session survival, AC04/AC07/AC08, and full AC01-AC12 are not proven here.
- Human QA must inspect native Settings rendering/accessibility and complete the clean Linux operator guide: fixture daemon start, owner machine PIN, desktop pairing, compatible remote project add, and safe cleanup without SSH. No real desktop or daemon was driven.
- Human/parent integration must prove disabled-gate behavior with actual persisted paired layouts and ensure no desktop mirror branch returns. This packet introduces no mirror branch but does not claim a full App regression test.
- Relay/local/remote version refusal is represented by capability fixtures and safe UX; real mixed-version relay/daemon interoperability and revoked-stream behavior require isolated backend/native integration evidence.

Assumptions: rollout preference is per desktop profile, defaults off, survives renderer restart, and never overrides advertised native support. Existing native inventory remains credential authority. Only the daemon owner issues machine PINs.
