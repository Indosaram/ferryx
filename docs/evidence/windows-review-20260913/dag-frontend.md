# DAG Frontend Acceptance and Aggregate Suite Disposition

Task: st_01a099f9  
Session: 01a0983f-c995-753d-afa9-593f6d118788  
Date: 2026-09-13  
Scope: Resolve remaining in-scope frontend aggregate failures, verify full frontend test suite and build, and provide precise source-based disposition for each residual failure without implementing unrelated features.

---

## 1. Executive Summary

- **UI Test Suite (`CI=1 bun run --cwd ui test`)**:
  - Total Files: **219** (217 passed, 2 failed).
  - Total Tests: **2,449** (2,445 passed, 4 failed).
  - Duration: **105.83s**, exit code `1`.
  - Retained log: [`dag-frontend-test.log`](dag-frontend-test.log).
- **UI Production Build (`bun run --cwd ui build`)**:
  - TypeScript (`tsc`): **Passed with 0 errors**.
  - Vite: **Transformed 1,892 modules**, generated production bundle in **2.23s**, exit code `0`.
  - Retained log: [`dag-frontend-build.log`](dag-frontend-build.log).
- **In-Scope Defect Status**:
  - All verified prior repairs confirmed passing in aggregate (Binding fixtures 55 pass, App fixtures 23 pass, SSH semantic token 33 pass).
  - Residual failures: exactly **4 tests across 2 files**, both comprehensively dispositioned with commit provenance and contract analysis below.
  - Zero unrelated feature code was invented (push client remains an unconnected stub).
  - Zero tests were deleted or weakened; no production prose was changed merely to satisfy arbitrary stale wording.

---

## 2. Verification of Prior In-Scope Repairs

The aggregate test execution independently verified that earlier focused repairs remain fully green and integrated:

1. **Terminal Binding Fixtures (55/55 passed)**:
   - `src/components/NativeTerminalPane.exitAttach.test.tsx` (4/4 passed)
   - `src/components/TerminalPane.exitAttach.integration.test.tsx` (1/1 passed)
   - `src/lib/nativeTerminalAttachPolicy.test.ts` (50/50 passed)
   - *Seam*: Reconciled stale 3-argument callback expectations to include full daemon binding identity (`backendSessionId`, `daemonEpoch`, `generation`, `lifecycleState`), preserving exit transition and operational-error handling.

2. **App Remote Fixtures (23/23 passed)**:
   - `src/App.remote.test.tsx` (19/19 passed)
   - `src/App.remoteHostShortcuts.test.tsx` (4/4 passed)
   - *Seam*: Aligned SSH reattachment with process-preserving restore contract, mocking `cmd_remote_status` during recovery without invalid re-spawn, and initialized `sessionIdsByLeafId` in local control fixtures.

3. **SSH Section Appearance Theme Token (33/33 passed)**:
   - `src/appearanceThemeContract.test.ts` (7/7 passed)
   - `src/components/settings/SshSection.test.tsx` (26/26 passed)
   - *Seam*: `ui/src/components/settings/SshSection.tsx:623` replaced hardcoded `text-emerald-500` with semantic `text-status-success` (`ui/DESIGN.md`).
   - *Visual Limitation Note*: 4 render artifacts exist (`1280-light.png`, `1280-dark.png`, `390-light.png`, `390-dark.png` under `ssh-added-render/`), but child agent image tooling is unavailable (`Current model does not support images`). Computed SVG styles (`#15803d` / `#86efac`) and 12px bounds are verified by automated headless DOM inspection; parent reviewer can inspect the retained PNG files.

---

## 3. Residual Failure Dispositions

Across all 219 test files in `ui`, exactly two files fail, containing a total of 4 failing tests. Each has a precise source-based disposition:

### Failure A: `src/components/SettingsDialog.test.tsx:614` (1 Failure)

- **Test**:
  `states that authorized browser profiles reconnect while Remote remains enabled and only require re-pairing when storage cleared, revoked, or profile changed`
- **Assertion Failure**:
  ```text
  FAIL  src/components/SettingsDialog.test.tsx > SettingsDialog > states that authorized browser profiles reconnect while Remote remains enabled and only require re-pairing when storage cleared, revoked, or profile changed
  Error: expect(element).toHaveTextContent()
  Expected element to have text content:
    /authorized browser profiles reconnect/i
  Received:
    Remote AccessAccess desktop terminal sessions from your phone. One switch turns remote access on; one QR code pairs any device, connecting through the relay and upgrading to a direct LAN or Tailscale path whenever it is reachable.Remote AccessRemote AccessServe live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on.Relay / Signaling Server URLPublic relay that carries pairing and traffic when a device is off your network. Leave empty to stay local-network only.Paired DevicesNo paired devices.
  ```
- **Provenance & Product Contract**:
  - The failing test was authored in commit `5ca3201cc44ae5c5457e746b1603fd87bc3926fe` (Aug 27, 2026) when `RemoteAccessSection` had a long multi-sentence description.
  - In commit `b3883d4f316a9622b5a45d48172381761daedff4` (*feat(remote): implement phase 4 ui and settings simplification*), the remote settings UI was intentionally overhauled:
    - Dedicated mode toggles were replaced with a single master toggle and universal QR pairing with relay fallback hints.
    - The section description was tightened to:
      `description="Access desktop terminal sessions from your phone. One switch turns remote access on; one QR code pairs any device, connecting through the relay and upgrading to a direct LAN or Tailscale path whenever it is reachable."`
    - The row description became:
      `description="Serve live terminal sessions to paired devices. Authorized browsers reconnect automatically while this stays on."`
    - `RemoteAccessSection.test.tsx` was updated to reflect the simplified contract, but `SettingsDialog.test.tsx:614` was not updated at that time.
- **Disposition**: **STALE TEST ASSERTION**.
  - The actual product contract is the simplified Phase 4 copy in `RemoteAccessSection.tsx`.
  - Reverting or expanding the product prose to include the deprecated August 27 re-pairing sentences would violate the design system and the explicit mandate: *"do not change prose merely to satisfy arbitrary stale wording"*.
  - Deleting or weakening the test in this pass is barred by mandate: *"do not delete/weaken tests"*.
  - Therefore, this failure is preserved and dispositioned as a known stale assertion awaiting lead/spec reconciliation.

---

### Failure B: `src/features/ferryx/push/client.test.ts:5, 11, 16` (3 Failures)

- **Tests**:
  1. `accepts only same origin exact task links` (line 5):
     `expected null to be 'https://ferryx.test/#task=YWJj'`
  2. `denied permission never subscribes` (line 11):
     `expected 'enabled' to be 'denied'`
  3. `server unsubscribe precedes local removal and failure preserves subscription` (line 16):
     `promise resolved "'disabled'" instead of rejecting`
- **Provenance & Production Contract**:
  - The files `ui/src/features/ferryx/push/client.ts` and `ui/src/features/ferryx/push/client.test.ts` were committed together in commit `831ae2c67da8143294be4526dd417bb07531b897` (*chore: consolidate in-flight dag, terminal, and packaging work*).
  - `client.ts` is an incomplete stub implementation:
    ```ts
    export type PushState = "loading" | "unsupported" | "insecure" | "denied" | "disabled" | "enabled" | "busy" | "error";
    export function secureTaskLink(_value: unknown, _origin: string): string | null { return null; }
    export interface PushApi { request(path: string, body?: unknown): Promise<unknown> }
    export class PushClient {
      constructor(readonly api: PushApi, readonly registration: ServiceWorkerRegistration) {}
      async enable(_showBody = false): Promise<PushState> { return "enabled"; }
      async disable(): Promise<PushState> { return "disabled"; }
    }
    ```
  - LSP workspace analysis confirms **zero production callers** exist for `PushClient`, `PushApi`, `PushState`, or `secureTaskLink`. Only `client.test.ts` references them.
  - This is an unconnected in-flight feature prototype committed by another branch/session, completely unrelated to Windows wheel navigation, PTY management, terminal rendering, or desktop layout.
- **Disposition**: **OUT-OF-SCOPE INCOMPLETE PROTOTYPE STUB**.
  - Per task instructions and `full-ui-post-selector.md`: *"full-ui-post-selector.md describes existing push stub committed831ae2c6 with only tests as callers, do not invent full unrelated push feature"*.
  - No new feature implementation is introduced to fabricate a green test.
  - No test cases are skipped, deleted, or weakened.
  - Preserved as an isolated prototype failure outside the Windows review acceptance gate.

---

## 4. Aggregate Execution Receipts

### A. UI Test Suite Command

```sh
CI=1 bun run --cwd ui test
```

- **Exit Code**: `1`
- **Duration**: `105.83s`
- **Summary**:
  - Test Files: `2 failed | 217 passed (219)`
  - Tests: `4 failed | 2445 passed (2449)`
- **Log**: Full stdout and stderr captured in [`docs/evidence/windows-review-20260913/dag-frontend-test.log`](dag-frontend-test.log).

### B. UI Production Build Command

```sh
bun run --cwd ui build
```

- **Exit Code**: `0`
- **Duration**: `2.23s`
- **Summary**:
  - Step 1: `tsc` (TypeScript compiler) — passed cleanly with zero diagnostics.
  - Step 2: `vite build` — 1,892 modules transformed and bundled into `ui/dist/`.
  - Advisory chunk warning: `App-C9DH7TVm.js` is 506.68 kB (existing baseline advisory; no threshold altered).
- **Log**: Full stdout and stderr captured in [`docs/evidence/windows-review-20260913/dag-frontend-build.log`](dag-frontend-build.log).

---

## 5. Working Tree and Boundary Integrity

- **Foreign dirty files**: All foreign working tree modifications (`.omo/plans/`, `docs/`, `script/qa/`, `src-tauri/Cargo.toml`, `src-tauri/src/remote/state.rs`, etc.) were strictly preserved untouched.
- **Exclusive ownership rispett**: P15 (`remote/state.rs`, `Cargo.toml`) and P14 notification evidence were not touched.
- **Git operations**: No branch creation/deletion, worktree operations, checkouts, commits, or pushes were performed.
- **Runtime operations**: No live Tauri desktop app was launched. No mock shims were introduced into production code.
