# Remote Machines UX Unification Report

**Date:** 2026-09-15
**Component:** Settings -> Remote
**Author:** hephaestus (omo senpi-task `st_01a0a395`)

---

## 1. Executive Summary

We have implemented, hardened, and verified the approved Remote Machines UX unification in `orca-lite`. The redesign consolidates outbound machine connectivity (both Paired Daemons and SSH Hosts) into a single, cohesive user experience while maintaining strict separation of underlying transports, credentials, and inbound gateway services.

All 9 initial review blockers, the `AddMachineModal` persistent unmount/reopen lifecycle bug, and the full `SettingsDialog` test suite failures have been resolved and verified with targeted regression tests.

---

## 2. Review Blocker & Bug Resolutions

### (1) Config Import Result Separation
- **Defect:** Config import previously invoked `onSuccess`, presenting "connected and verified" and offering `Add Project` before any connection test was performed.
- **Resolution:** Config import now transitions into an explicit **Imported Result View** (`Imported N machines into inventory. Imported machines are recorded as unchecked. Test the connection from machine details before adding remote projects.`). It provides a single `Done` button that closes the modal and selects the imported host. No readiness status and no project offer are made until an explicit connection test is executed.

### (2) Verified State Propagation & Live Generation Recheck
- **Defect:** The modal's `onSuccess` callback only updated row selection, leaving paired rows in `UNCHECKED` state and dropping SSH environment test results.
- **Resolution:**
  - `AddMachineModal` passes a structured `VerifiedMachineResult` to `RemoteSection`.
  - For Paired Daemons: `RemoteSection` stores `{ generation, code: "READY" }` in `checks`, immediately transitioning the row to `Ready`.
  - For SSH Hosts: `RemoteSection` stores the tested reachability and runtime environment in `sshTestResults`, transitioning the row to `Reachable` / `Ready`.
  - In `AddMachineModal`, after `await negotiate(pairedContext)`, the live store state is re-fetched. If `generation` drifted or `authStatus` was revoked during capability negotiation, the modal catches it, reports `STALE_HOST_GENERATION` or `MACHINE_GRANT_REQUIRED`, retains user input, and suppresses project offers.

### (3) Full SSH Discovery & Advanced Field Editing
- **Defect:** SSH configuration import was paste-only, and SSH editing only exposed 4 fields, regressing key, auth method, and jump host controls.
- **Resolution:**
  - In `AddMachineModal`: Restored system SSH config discovery (`readSystemSshConfig`), displaying config path, host counts, an `Import All (N hosts)` action, and Tauri custom file picker (`Choose file...`), alongside the paste option.
  - In `RemoteSection` SSH edit form: Restored editing of all fields—`Label`, `Hostname`, `Username`, `Port` (with integer bounds validation), `Auth Method` (`agent` | `key`), `Identity File`, and `Jump Host`.

### (4) Connection Invalidation & Stale Async Result Guard
- **Defect:** `sameConnection` invalidation and async race guards from `SshSection` were missing in `RemoteSection`.
- **Resolution:**
  - Restored `sameConnection(left, right)` and `hostsRef`. When SSH host credentials change, previous test summaries and prepared flags are purged.
  - In `handleTestSsh` and `handlePrepareSsh`, added `if (!isCurrentHost(host)) return;` guards after async resolution to prevent stale completions from updating mutated hosts.
  - Added port integer validation (1–65535) before saving SSH host edits.

### (5) Exactly One "Add Machine" Button
- **Defect:** When the machine list was empty, an extra "Add Machine" button appeared inside the empty state card, creating duplicate controls.
- **Resolution:** Removed the redundant button from the empty state card. The top toolbar hosts the single, persistent `Add Machine` action.

### (6) Modal Focus Trap, Escape Isolation, Reopen Lifecycle & Focus Restore
- **Defect:**
  - When `isOpen=false`, `AddMachineModal` remained mounted in `RemoteSection`. `handleClose` set `isDismissedRef=true`, but the initial `useEffect([], ...)` only ran on mount, causing any subsequent reopening of the modal to permanently discard async callbacks.
  - Initial focus had a fixed 20ms timeout and was dependent on `activeTab`, causing tab switching to overwrite `triggerRef` with elements inside the modal.
- **Resolution:**
  - `isDismissedRef.current = false` is now explicitly reset on every `isOpen === true` transition.
  - External trigger element (`triggerRef.current = document.activeElement`) is captured exactly once per modal open. It is never overwritten by tab switching.
  - Focus is applied immediately via effect without any fixed timeout or sleep.
  - When `isOpen` transitions to `false`, focus is restored to `triggerRef.current`.
  - Added keyboard focus trap cycling for Tab / Shift+Tab.
  - Handled `Escape` with `e.stopPropagation()` and `e.preventDefault()`, closing only the modal without bubbling to `SettingsDialog`.
  - Disabled method switching, close (X) button, and Escape dismissal while `busy` is true.
  - Cleared PIN upon successful verification and modal close.

### (7) Namespaced Machine Keys
- **Defect:** Paired and SSH host IDs could collide if identical IDs were assigned across domains.
- **Resolution:** Namespaced all row keys, DOM attributes (`data-machine-id`), selection state, focus refs, and details sets with `${item.kind}:${item.id}` (e.g., `paired:https://...` vs `ssh:dev-1`).

### (8) Paired "Add Project" Action Isolation
- **Defect:** If only `onOpenSshProject` was passed to `RemoteSection`, paired machine rows could appear with active `Add Project` buttons.
- **Resolution:** Explicitly required `onOpenProject` for Paired rows (`disabled={busy || code !== "READY" || !onOpenProject}`). SSH rows continue to support either `onOpenProject` or `onOpenSshProject`.

### (9) Ergonomic Layout, Concise Status & Collapsible Advanced Options
- **Concise Row Status:** Replaced multi-line error/explanation text on rows with concise indicators (`Ready`, `Offline`, `Needs Grant`, `Unchecked`, `Desktop Required`, `Incompatible`, `Unavailable`). Full explanation paragraphs are presented within the expanded Details pane.
- **Long Details Wrap:** Added `break-all` / `break-words` on Machine IDs, relays, and hostnames to prevent horizontal overflow.
- **Responsive Methods:** Method selector tabs in `AddMachineModal` use `flex-wrap gap-1` for clean wrapping on narrow viewports.
- **Collapsed Advanced Options:** Folded `Auth Method`, `Identity File`, and `Jump Host` into a collapsible section ("Show Advanced Options") in both the Add Machine SSH form and the SSH host editor.

---

## 3. Settings Navigation Baseline & Test Suite Audit

### 3.1 Root Cause & Baseline Evidence
In commit `415497480cf93b5c757bafceac152e410a8f904f` (`feat(settings): redesign remote access and ssh into unified remote section`):
1. Top-level navigation in `SettingsDialog.tsx` was unified:
   - `<NavButton label="Remote Access" ... />` and `<NavButton label="SSH Machines" ... />` were replaced with `<NavButton label="Remote" ... />`.
   - `sanitizeSectionId` mapped legacy `"ssh"` requests directly to `"remote"` (`if (candidate === "ssh") return "remote"`).
   - Inbound controls (QR code, pairing PIN, device revocation) were placed under the `"Access to This Machine"` subpage inside `RemoteSection`.
2. However, the existing tests in `SettingsDialog.test.tsx` and `SettingsDialog.ssh.test.tsx` were not updated in commit `41549748`:
   - `SettingsDialog.test.tsx` (lines 465, 588, 611, 642, 679) still attempted to click a non-existent button with name `"Remote Access"`.
   - `SettingsDialog.ssh.test.tsx` still asserted a standalone top-level `"SSH Machines"` button.

### 3.2 Test Updates to Match Redesign Specification
Per task instructions, obsolete tests were updated to assert the approved navigation architecture:
- In `SettingsDialog.test.tsx`: Navigates to `Remote` and then clicks `Access to This Machine` before asserting inbound device listing, QR code generation, and PIN copy actions.
- In `SettingsDialog.ssh.test.tsx`: Asserts that `Remote` is the unified navigation item, and that mounting with `initialSection="ssh"` activates `Remote` (`aria-current="page"`) and displays the mixed machine list (`"Connect to another machine."`).

### 3.3 Full SettingsDialog Suite Execution Results
Command:
```bash
bun run --cwd ui test \
  src/components/SettingsDialog.escape.test.tsx \
  src/components/SettingsDialog.update.test.tsx \
  src/components/SettingsDialog.cli.test.tsx \
  src/components/SettingsDialog.escapeUsable.test.tsx \
  src/components/SettingsDialog.test.tsx \
  src/components/SettingsDialog.ssh.test.tsx
```

Output:
```text
 ✓ src/components/SettingsDialog.test.tsx (29 tests) 2170ms
 ✓ src/components/SettingsDialog.update.test.tsx (5 tests) 521ms
 ✓ src/components/SettingsDialog.cli.test.tsx (6 tests) 429ms
 ✓ src/components/SettingsDialog.escape.test.tsx (5 tests) 216ms
 ✓ src/components/SettingsDialog.escapeUsable.test.tsx (2 tests) 140ms
 ✓ src/components/SettingsDialog.ssh.test.tsx (2 tests) 116ms

 Test Files  6 passed (6)
      Tests  49 passed (49)
   Duration  8.70s
```

All 6 test files (49 tests) are **100% green**.

---

## 4. Verification Evidence (Targeted Machine Suites)

Executed Vitest runner across all settings machine suites:
```bash
bun run --cwd ui test \
  src/components/settings/RemoteSection.test.tsx \
  src/components/settings/PairedMachinesSection.test.tsx \
  src/components/settings/SshSection.test.tsx
```

Output:
```text
 ✓ src/components/settings/SshSection.test.tsx (27 tests) 529ms
 ✓ src/components/settings/RemoteSection.test.tsx (15 tests) 367ms
 ✓ src/components/settings/PairedMachinesSection.test.tsx (7 tests) 107ms

 Test Files  3 passed (3)
      Tests  49 passed (49)
   Duration  2.91s
```

All 15 tests in `RemoteSection.test.tsx` passed:
1. `renders ONE mixed machine list without All/Paired/SSH tabs or separate headings`
2. `renders EXACTLY ONE Add Machine button even when the inventory is empty`
3. `renders NO permanent add or PIN forms on the page body`
4. `common rows display concise status, Add Project, and Details toggle with technical metadata hidden`
5. `propagates verified generation-bound state to row status and offers project upon PIN pairing success`
6. `rechecks live paired generation/auth after await negotiate; fails if credentials revoked or changed`
7. `config import separates imported-not-verified result with NO ready/project offer`
8. `restores system SSH config discovery, card, and import options inside Add Machine modal`
9. `restores all SSH edit fields including port validation, authMethod, identityFile, and jumpHost`
10. `restores sameConnection invalidation: clears stale test results when host config changes`
11. `disables Paired Add Project without onOpenProject even if onOpenSshProject is provided`
12. `namespaces machine row IDs and prevents ID collision between Paired and SSH machines`
13. `handles Escape in Add Machine modal without propagating to parent SettingsDialog`
14. `restores focus to trigger element when Add Machine modal closes`
15. `resets dismissed state on reopen: open-close-reopen pairing succeeds and is not discarded`

### TypeScript Compilation
Command:
```bash
./ui/node_modules/.bin/tsc --noEmit -p ui/tsconfig.json
```
Output:
```text
(clean - 0 errors, 0 warnings)
```

---

## 5. Modified & Created Files

- **`ui/src/components/settings/RemoteSection.tsx`**: Unified mixed machine list, concise status display, namespaced row tracking, restored `sameConnection` invalidation, full SSH editor with port validation and advanced options, and verified state propagation.
- **`ui/src/components/settings/AddMachineModal.tsx`**: Unified modal with PIN, SSH, and Config Import methods, pre-readiness verification with live generation recheck, focus trap, external trigger capture once per open, no fixed timeout, Escape propagation isolation, imported-not-verified separation, and system SSH config card.
- **`ui/src/components/settings/RemoteSection.test.tsx`**: 15 unit and regression tests covering all unified UX behaviors, reopen lifecycle, and focus restoration.
- **`ui/src/components/SettingsDialog.test.tsx`**: Updated 5 tests to navigate to `Remote` -> `Access to This Machine`.
- **`ui/src/components/SettingsDialog.ssh.test.tsx`**: Updated 2 tests to verify `Remote` nav button and legacy `ssh` normalization.
- **`docs/REMOTE_MACHINES_UX_UNIFICATION_2026-09-15.md`**: This technical implementation, audit, and verification report.

---

## 6. Limitations & Handoff Notes

1. **Visual QA & Desktop Execution:** Desktop runtime execution (`bun tauri dev`) remains delegated to the lead for visual verification.
2. **Identity-Checked Re-Pair:** Backend support for in-place generation/token identity replacement remains pending; `REPAIR_REQUIRES_IDENTITY_SUPPORT` guides users to forget credentials and pair anew.
3. **Helper Provisioning:** Terminal helper binary installation remains isolated from SSH connection testing and agent extension preparation.
