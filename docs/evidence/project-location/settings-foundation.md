# Evidence Report: Shared SSH Inventory & Mounted SSH Machines Settings Section

**Date:** 2026-09-06  
**Document:** `docs/evidence/project-location/settings-foundation.md`  
**Task ID:** `st_01a07778` (resolving verification blockers from `st_01a07775`)  
**Status:** Complete, Verified Deterministic (RED -> GREEN), Zero Polling, LSP Clean

---

## 1. Executive Summary

We resolved the concrete foundation verification blockers from `settings-foundation-verification.md` across the seven foundation TypeScript files and updated this evidence report with authenticated, verified results.

Key achievements:
1. **Zero Polling & Deterministic Test Harness**: Replaced all 15 interval-backed `waitFor` call sites across `ui/src/lib/sshHosts.test.tsx` (4 call sites) and `ui/src/components/settings/SshSection.test.tsx` (11 call sites) with deterministic deferred IPC promises established before user action and resolved or rejected synchronously inside `await act(...)`. All assertions execute directly against settled state. Removed all unused `waitFor` imports. Audited zero `waitFor`, `findBy`, `setTimeout`, `setInterval`, or `sleep` calls across all foundation tests.
2. **Type Cleanliness**: Replaced test `any` usages (`(reason?: any) => void`) with `unknown` (`(reason?: unknown) => void`). Navigated with real `SectionId` `"ssh"` directly in tests (`initialSection="ssh"`).
3. **Clean Seam Contract**: Removed the unused `SshSection as SshSettings` legacy compatibility alias from `ui/src/components/SettingsDialog.tsx`, retaining clean canonical export `export { SshSection } from "./settings/SshSection";`.
4. **Structured IPC Error Extraction**: Exported `extractIpcErrorMessage` from `ui/src/lib/sshHosts.ts` and integrated it into `ui/src/components/settings/SshSection.tsx` for all five mutation and probe operations: host saving (`updateSshHost`), OpenSSH config importing (`importSshConfig`), host deletion (`deleteSshHost`), enabled/disabled state toggling (`updateSshHost`), and connection testing (`testSshConnection`). Structured Tauri IPC error objects (`{ code, message, details }`) now faithfully surface their human-readable `message` in UI alert banners instead of falling back to generic placeholder text.
5. **Authenticated RED Regression Proof**: Added 5 behavioral regression tests for structured mutation rejections in `SshSection.test.tsx` *before* modifying `SshSection.tsx`. Executed the suite and captured authenticated RED failure output (`exit code 1`, 5 failed / 9 passed) proving that generic fallback messages were previously rendered, followed by verified GREEN results post-fix (`exit code 0`, 14 passed).
6. **Preserved Wire and Hook Contracts**: The `useSshHosts()` hook interface remains strictly `{ hosts, loading, error, refresh }`. All registered Tauri commands (`cmd_ssh_list_hosts`, `cmd_ssh_update_host`, `cmd_ssh_delete_host`, `cmd_ssh_import_config`, `cmd_ssh_test_connection`) continue to operate against authoritative backend storage. Epoch-based stale-read discarding, `isTauri()` write checks, and port validation (1–65535) remain intact.
7. **Strict Scope Discipline**: Changes are strictly confined to the original seven foundation files plus this report. No edits were made to `App.tsx`, `ProjectDialogs.tsx`, `remoteProject`, backend Rust code, or any concurrent worktree lanes.

---

## 2. Authoritative Wire Contracts & Data Models

The TypeScript DTOs in `ui/src/lib/sshHosts.ts` directly mirror the authoritative Rust models in `src-tauri/src/ssh/mod.rs:9-24` and `src-tauri/src/ipc/ssh.rs:18-35`:

```ts
export type SshHostSource = "config" | "manual";
export type SshAuthMethod = "agent" | "key";

export interface SshHost {
  id: string;
  label: string;
  hostname: string;
  username?: string | null;
  port?: number | null;
  identityFile?: string | null;
  jumpHost?: string | null;
  source: SshHostSource;
  authMethod: SshAuthMethod;
  disabled?: boolean | null;
}

export interface SshTargetSummary {
  host: SshHost;
  reachable: boolean;
  lastError?: string | null;
  checkedAt: number;
}
```

### Tauri IPC Bridge Operations

| Function | Tauri IPC Command | Request Payload | Return Type |
|---|---|---|---|
| `listSshHosts()` | `cmd_ssh_list_hosts` | None | `Promise<SshHost[]>` |
| `updateSshHost(host)` | `cmd_ssh_update_host` | `{ host: SshHost }` | `Promise<SshHost[]>` |
| `deleteSshHost(id)` | `cmd_ssh_delete_host` | `{ id: string }` | `Promise<SshHost[]>` |
| `importSshConfig(configText)` | `cmd_ssh_import_config` | `{ configText: string }` | `Promise<SshHost[]>` |
| `testSshConnection(host)` | `cmd_ssh_test_connection` | `{ host: SshHost }` | `Promise<SshTargetSummary>` |

---

## 3. Exact Exports for Consumers

The Chooser implementer (`ui/src/components/ProjectDialogs.tsx`) and settings components import from `ui/src/lib/sshHosts.ts`:

### Exported Symbols

```ts
import {
  useSshHosts,
  listSshHosts,
  updateSshHost,
  deleteSshHost,
  importSshConfig,
  testSshConnection,
  formatSshTarget,
  formatSshKey,
  subscribeSshHosts,
  getCachedSshHosts,
  resetSshHostsCache,
  extractIpcErrorMessage,
  type SshHost,
  type SshHostSource,
  type SshAuthMethod,
  type SshTargetSummary,
  type UseSshHostsResult,
} from "../lib/sshHosts";
```

### Consumption Pattern in Location Chooser

```tsx
import { useSshHosts, formatSshTarget, type SshHost } from "../lib/sshHosts";

export function RemoteProjectForm({ onRegistered }: { onRegistered: (project: RegisteredProject) => void }) {
  const { hosts, loading, error } = useSshHosts();

  const activeHosts = hosts.filter((host) => !host.disabled);

  if (loading && hosts.length === 0) {
    return <div>Loading SSH hosts…</div>;
  }

  if (activeHosts.length === 0) {
    return (
      <div data-testid="remote-empty">
        <p>No active SSH hosts configured.</p>
        <button onClick={() => openSettings("ssh")}>Configure SSH Hosts in Settings</button>
      </div>
    );
  }

  return (
    <select data-testid="remote-host-select">
      {activeHosts.map((host) => (
        <option key={host.id} value={host.id}>
          {host.label} ({formatSshTarget(host)}:{host.port ?? 22})
        </option>
      ))}
    </select>
  );
}
```

---

## 4. Settings UI Component Architecture

### 1. `ui/src/components/settings/types.ts`
- `SectionId` union includes `"ssh"`:
  ```ts
  export type SectionId =
    | "general"
    | "appearance"
    | "terminal"
    | "shortcuts"
    | "agents"
    | "browser"
    | "notifications"
    | "remote"
    | "permissions"
    | "ssh";
  ```

### 2. `ui/src/components/SettingsDialog.tsx`
- Registered `"ssh"` in `VALID_SECTIONS`.
- Rendered `<NavButton active={section === "ssh"} icon={<Server />} label="SSH Machines" onClick={() => setSection("ssh")} />`.
- Mounted section body: `{section === "ssh" ? <SshSection /> : null}`.
- Export cleaned: `export { SshSection } from "./settings/SshSection";` (unused `SshSettings` alias removed).

### 3. `ui/src/components/settings/SshSection.tsx`
- Layout conforms to Ferryx design tokens (`SettingsHeading`, `SettingsGroup`, typography scale).
- **Empty State**: Renders clear banner with "No SSH machines configured" and direct action buttons to add or import.
- **Form**: In-card add/edit form with validation for required labels, hostnames, and valid integer port ranges (1–65535). Prevents duplicate submissions during async requests.
- **Import**: Pasted OpenSSH config parser panel with validation.
- **Test Connection**: Inline asynchronous probe triggering `cmd_ssh_test_connection`, rendering spinner during execution, green badge upon verification, or red alert banner with server error detail upon failure.
- **Toggle Switch**: Accessible switch allowing immediate enable/disable toggling of individual machines with async busy lock.
- **Mutation Error Surfacing**: Rejections from `updateSshHost`, `importSshConfig`, `deleteSshHost`, and `testSshConnection` pass through `extractIpcErrorMessage(err, fallback)` so structured backend messages are displayed directly in the UI.

---

## 5. Verification Evidence: RED Artifacts & Historical Captures

### A. Authenticated RED Regression Proof: Structured IPC Mutation Rejections

Before applying the error extraction fix to `ui/src/components/settings/SshSection.tsx`, five regression tests were added to `ui/src/components/settings/SshSection.test.tsx` verifying that structured backend rejection messages (`{ code: "...", message: "..." }`) surface in the UI for save, import, delete, toggle, and connection test operations.

**Command executed:**
```bash
bun run --cwd ui test src/components/settings/SshSection.test.tsx
```

**Observed RED Failure Output (Exit Code: 1):**
```text
$ vitest run --maxWorkers=1 src/components/settings/SshSection.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ❯ src/components/settings/SshSection.test.tsx (14 tests | 5 failed) 334ms
   ✓ SshSection Settings Component > renders empty state when no SSH machines are configured 51ms
   ✓ SshSection Settings Component > renders mounted inventory with badges, endpoints, and status 15ms
   ✓ SshSection Settings Component > validates form input when adding a machine manually 73ms
   ✓ SshSection Settings Component > saves a new machine, disables duplicate submit while pending, and mounts it into inventory 42ms
   ✓ SshSection Settings Component > edits an existing machine and updates the inventory 25ms
   ✓ SshSection Settings Component > toggles enabled/disabled state via switch with async busy guard 7ms
   ✓ SshSection Settings Component > deletes a machine from the inventory with async guard 10ms
   ✓ SshSection Settings Component > imports pasted SSH configuration and updates inventory 27ms
   ✓ SshSection Settings Component > tests host connection and renders visible success and error results 16ms
   × SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when saving a machine fails with structured IPC error 29ms
     → expect(element).toHaveTextContent()

Expected element to have text content:
  Failed to persist SSH host to disk: permission denied
Received:
  Failed to save SSH machine.
   × SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when importing config fails with structured IPC error 16ms
     → expect(element).toHaveTextContent()

Expected element to have text content:
  Configuration file contains invalid directive at line 2
Received:
  Failed to import SSH configuration.
   × SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when deleting a machine fails with structured IPC error 8ms
     → expect(element).toHaveTextContent()

Expected element to have text content:
  Cannot delete host: entry is locked by active session
Received:
  Failed to delete SSH machine.
   × SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when toggling a machine fails with structured IPC error 6ms
     → expect(element).toHaveTextContent()

Expected element to have text content:
  Cannot toggle host: machine state is locked
Received:
  Failed to update machine state.
   × SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when testing connection rejects with structured IPC error 8ms
     → expect(element).toHaveTextContent()

Expected element to have text content:
  Failed: Network unreachable: host is down
Received:
  Failed: Connection test failed.

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 5 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/components/settings/SshSection.test.tsx > SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when saving a machine fails with structured IPC error
Error: expect(element).toHaveTextContent()
Expected: "Failed to persist SSH host to disk: permission denied"
Received: "Failed to save SSH machine."

 FAIL  src/components/settings/SshSection.test.tsx > SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when importing config fails with structured IPC error
Error: expect(element).toHaveTextContent()
Expected: "Configuration file contains invalid directive at line 2"
Received: "Failed to import SSH configuration."

 FAIL  src/components/settings/SshSection.test.tsx > SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when deleting a machine fails with structured IPC error
Error: expect(element).toHaveTextContent()
Expected: "Cannot delete host: entry is locked by active session"
Received: "Failed to delete SSH machine."

 FAIL  src/components/settings/SshSection.test.tsx > SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when toggling a machine fails with structured IPC error
Error: expect(element).toHaveTextContent()
Expected: "Cannot toggle host: machine state is locked"
Received: "Failed to update machine state."

 FAIL  src/components/settings/SshSection.test.tsx > SshSection Settings Component > Structured IPC mutation error surfacing > surfaces backend error message when testing connection rejects with structured IPC error
Error: expect(element).toHaveTextContent()
Expected: "Failed: Network unreachable: host is down"
Received: "Failed: Connection test failed."

 Test Files  1 failed (1)
      Tests  5 failed | 9 passed (14)
   Start at  01:08:39
   Duration  1.68s (transform 128ms, setup 161ms, collect 689ms, tests 334ms, environment 350ms, prepare 44ms)

error: script "test" exited with code 1
```

### B. Historical Reported Captures (st_01a0775a)

*(Note: The following two captures are preserved from historical task run `st_01a0775a`. As documented in verification report `st_01a07775`, standalone log files and numeric exit-code captures were not recorded at that time.)*

1. **Initial Seam Navigation Failure (Historical capture, no numeric exit artifact):**
   ```text
   ⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯
    FAIL  src/components/SettingsDialog.ssh.test.tsx > SettingsDialog SSH Navigation (seam verification) > renders an 'SSH Machines' nav item and switches to SSH Machines section
   TestingLibraryElementError: Unable to find an accessible element with the role "button" and name "SSH Machines"

    FAIL  src/components/SettingsDialog.ssh.test.tsx > SettingsDialog SSH Navigation (seam verification) > mounts directly to 'ssh' section when initialSection is 'ssh'
   TestingLibraryElementError: Unable to find an accessible element with the role "button" and name "SSH Machines"

    Test Files  1 failed (1)
         Tests  2 failed (2)
   ```

2. **Stale-Read Race Condition Failure (Historical capture, no numeric exit artifact):**
   ```text
    FAIL  src/lib/sshHosts.test.tsx > sshHosts library and useSshHosts hook > IPC command wrappers > prevents in-flight list from overwriting newer mutation inventory (stale read discard)
   AssertionError: expected [ { id: 'host-1', …(7) } ] to deeply equal [ { id: 'host-1', …(7) }, …(1) ]
   ```

---

## 6. Verification Evidence: GREEN Implementation

Following the removal of all 15 `waitFor` calls, deferred IPC promise wiring, export of `extractIpcErrorMessage`, removal of `SshSettings` alias, and structured error handling in `SshSection.tsx`, the full foundation test suite passed cleanly and deterministically.

### Primary Test Command
```bash
bun run --cwd ui test src/components/SettingsDialog.ssh.test.tsx src/components/settings/SshSection.test.tsx src/lib/sshHosts.test.tsx
```

### Captured GREEN Output (Exit Code: 0)
```text
$ vitest run --maxWorkers=1 src/components/SettingsDialog.ssh.test.tsx src/components/settings/SshSection.test.tsx src/lib/sshHosts.test.tsx

 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui

 ✓ src/components/settings/SshSection.test.tsx (14 tests) 301ms
 ✓ src/components/SettingsDialog.ssh.test.tsx (2 tests) 110ms
 ✓ src/lib/sshHosts.test.tsx (16 tests) 26ms

 Test Files  3 passed (3)
      Tests  32 passed (32)
   Start at  01:09:51
   Duration  2.68s (transform 224ms, setup 238ms, collect 1.08s, tests 437ms, environment 554ms, prepare 77ms)

TEST_EXIT=0
```

### Zero-Polling & Zero-`any` Audit

Audited test sources with `rg` to confirm no polling or arbitrary timer functions remain:

```bash
# Polling and timer functions audit:
rg 'waitFor|findBy|setTimeout|setInterval|sleep' ui/src/lib/sshHosts.test.tsx ui/src/components/settings/SshSection.test.tsx ui/src/components/SettingsDialog.ssh.test.tsx
# Result: 0 matches (exit 1)

# Any-type audit across all 7 foundation files:
rg '\bany\b' ui/src/lib/sshHosts.ts ui/src/lib/sshHosts.test.tsx ui/src/components/settings/SshSection.tsx ui/src/components/settings/SshSection.test.tsx ui/src/components/settings/types.ts ui/src/components/SettingsDialog.tsx ui/src/components/SettingsDialog.ssh.test.tsx
# Result: 0 matches (exit 1)
```

### Non-Regression Suite

1. **Design Tokens**:
   ```bash
   bun run --cwd ui test src/components/settings/designTokens.test.ts
   # Result: 3 passed (3) in 771ms; exit 0
   ```
2. **Settings Dialog Baseline**:
   ```bash
   bun run --cwd ui test src/components/SettingsDialog.test.tsx
   # Result: 29 passed (29) in 4.02s; exit 0
   ```
3. **TypeScript Compiler**:
   ```bash
   cd ui && bunx tsc --noEmit
   # Result: clean output; exit 0
   ```

---

## 7. LSP Diagnostics & Compiler Audit

All seven foundation files were audited using the `lsp_diagnostics` tool with severity filter `all`. In this session, every file returned fresh, clean diagnostics without timeout:

| File Path | LSP Diagnostics Result | Status |
|---|---|---|
| `ui/src/lib/sshHosts.ts` | `No diagnostics found` | Clean |
| `ui/src/lib/sshHosts.test.tsx` | `No diagnostics found` | Clean |
| `ui/src/components/settings/SshSection.tsx` | `No diagnostics found` | Clean |
| `ui/src/components/settings/SshSection.test.tsx` | `No diagnostics found` | Clean |
| `ui/src/components/settings/types.ts` | `No diagnostics found` | Clean |
| `ui/src/components/SettingsDialog.tsx` | `No diagnostics found` | Clean |
| `ui/src/components/SettingsDialog.ssh.test.tsx` | `No diagnostics found` | Clean |

*(Note: In the prior verification run `st_01a07775`, requests for `sshHosts.ts` and `sshHosts.test.tsx` timed out at 3000ms and were noted as unverified rather than clean. In this run, all seven files completed within tool timeout and returned zero errors, warnings, or hints.)*

Independent compiler verification (`cd ui && bunx tsc --noEmit`) completed with exit code 0 and zero output.

---

## 8. Working Tree State & Blast Radius Check

Running `git status --short` confirms tracked changes are strictly confined to the settings shell seam and type definition, with no unauthorized modifications to foreign code:

```text
Tracked foundation diffs:
 M ui/src/components/SettingsDialog.tsx (NavButton, VALID_SECTIONS, mount, clean re-export)
 M ui/src/components/settings/types.ts (added "ssh" to SectionId)

Untracked foundation files:
 ui/src/lib/sshHosts.ts
 ui/src/lib/sshHosts.test.tsx
 ui/src/components/settings/SshSection.tsx
 ui/src/components/settings/SshSection.test.tsx
 ui/src/components/SettingsDialog.ssh.test.tsx
```

All other dirty files present in the shared workstation tree (`src-tauri/*`, `docs/*`, `ui/src/components/NativeTerminalPane.*`, `ui/src/lib/linkRouting.*`, `ui/src/lib/remoteProject.*`) were preserved without modification.

Implementation is complete, deterministic, zero-polling, authenticated RED -> GREEN, LSP clean, and ready for integration.
