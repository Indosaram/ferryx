# Remote Machines Unification - Browser-Only Visual QA Deliverable

**Date:** 2026-09-15  
**Deliverable Directory:** `docs/evidence/remote-machines-20260915`  
**Test Runner:** `docs/evidence/remote-machines-20260915/qa.mjs`  
**Environment:** Bun v1.4.0, Bun.WebView (Chrome backend, headless), macOS Darwin 25.6.0 arm64  
**Target Components:** `RemoteSection` (`ui/src/components/settings/RemoteSection.tsx`) and `AddMachineModal` (`ui/src/components/settings/AddMachineModal.tsx`) with real Tailwind CSS (`ui/src/index.css`, `ui/src/settings-runtime.css`).

---

## 1. Executive Summary

This deliverable provides comprehensive visual and interaction QA evidence for the **Remote Machines UX Unification** in Orca-Lite. Testing was conducted in an isolated headless browser (`Bun.WebView`) served on an ephemeral, non-5173 port (`port: 53092`) with mock Tauri IPC at the system boundary. No desktop manipulation, live daemon pairing, or Tauri process launches were executed.

### Key Verification Metrics
- **Total Assertions Executed:** 54 checks across 2 viewports (1280x800 Desktop and 390x844 Mobile).
- **Passed Checks:** 54 / 54 (100% pass rate).
- **Failed Checks:** 0.
- **Screenshots Captured:** 20 PNG artifacts across 10 distinct interactive states at both viewports.

---

## 2. Test Matrix & Scenario Coverage

| # | Scenario | Desktop (1280x800) | Mobile (390x844) | Key Verification Points |
|---|---|:---:|:---:|---|
| 1 | **Empty List** | `1280-empty-list.png` | `390-empty-list.png` | Single "Add Machine" button in toolbar; zero legacy filter tabs (All/Paired/SSH); no horizontal overflow. |
| 2 | **Mixed List** | `1280-mixed-list.png` | `390-mixed-list.png` | Unified list combining Paired and SSH machines; type badges (`Paired`, `SSH`); status indicators (`Ready`, `Needs Grant`, `Not checked`); enabled/disabled "Add Project" buttons. |
| 3 | **Paired Details** | `1280-paired-details.png` | `390-paired-details.png` | Expandable details conceal Machine ID, Relay Origin, Transport, Generation, Grant Scope, and Auth Status; actions for "Check capabilities", "Re-pair", and "Forget credentials". |
| 4 | **SSH Details** | `1280-ssh-details.png` | `390-ssh-details.png` | Concealed technical metadata (Hostname, Port, Username, Auth method, Identity file, Jump host, Runtime diagnostics); actions for "Test connection", "Prepare agent integration", "Edit", "Disable", and "Delete". |
| 5 | **Modal: Pair with PIN** | `1280-modal-pin.png` | `390-modal-pin.png` | Default tab on modal open; single 6-digit PIN input with helper text and hardcoded relay; cancel and pair buttons. |
| 6 | **Modal: SSH Advanced** | `1280-modal-ssh-advanced.png` | `390-modal-ssh-advanced.png` | Connect with SSH form; basic inputs (Label, Hostname, Username, Port); collapsible advanced options (Auth Method, Key, Jump Host) cleanly revealed. |
| 7 | **Modal: Import Config** | `1280-modal-import.png` | `390-modal-import.png` | Import SSH Config tab with system config summary and host count preview, plus paste configuration sub-mode. |
| 8 | **Modal: Failure State** | `1280-modal-failure.png` | `390-modal-failure.png` | Invalid PIN verification error displayed in accessible alert container (`role="alert"`); user PIN preserved in the input without clearing. |
| 9 | **Modal: Success State** | `1280-modal-success.png` | `390-modal-success.png` | Verified machine screen offering direct "Add Project" action button; closing modal auto-selects the new row. |
| 10 | **Escape & Isolation** | `1280-modal-escape-isolated.png` | `390-modal-escape-isolated.png` | Escape key closes the modal without bubbling to the parent Settings Dialog wrapper (`parentEscapeCount: 0`); trigger focus properly restored to the "Add Machine" button. |

---

## 3. Focused Behavioral Analysis

### 3.1 Escape Parent Isolation
- **Observed Behavior:** When `AddMachineModal` is opened from within a parent dialog container (simulating the settings dialog with an active Escape handler), pressing the `Escape` key closes the `AddMachineModal` cleanly.
- **Isolation Verification:**
  - `parentEscapeCount: 0`
  - `windowEscapeCount: 0`
  - Synthetic event stopping in `handleKeyDown` (`e.stopPropagation(); e.preventDefault();`) successfully prevented the event from reaching parent React handlers.
- **Trigger Focus Restoration:** Upon modal dismissal, keyboard focus is reliably returned to the triggering `<Button aria-label="Add Machine">`.

### 3.2 Modal Focus Lifecycle
- **Observed Behavior:** When `AddMachineModal` opens, focus is programmatically set via:
  ```typescript
  const input = modalRef.current?.querySelector<HTMLElement>(
    'input:not([disabled]), textarea:not([disabled]), [role="tab"][aria-selected="true"]',
  );
  input?.focus();
  ```
- **Finding:** Because `[role="tab"][aria-selected="true"]` precedes `<input id="add-machine-pin" />` in document DOM order, `querySelector` resolves to the **active tab button** (`<button role="tab" aria-selected="true">Pair with PIN</button>`) rather than the PIN text input.
- **Impact:** While keyboard accessibility is preserved (Tab navigation smoothly enters the PIN input on next keystroke), users cannot immediately begin typing without pressing Tab or clicking the input field.

### 3.3 Form Controls & Input Preservation
- **Form Controls:**
  - All form controls (`input`, `select`, `button`, `textarea`) render with accessible labels, visible focus rings, and proper keyboard activation.
  - Submitting empty or whitespace-only inputs is prevented; the "Pair Machine" button is disabled until valid input is entered.
- **Error Handling & Input Preservation:**
  - When connection verification fails (e.g., bad PIN or unreachable SSH host), the modal displays an inline error alert banner.
  - Entered values (PIN, SSH hostname, port, credentials) are **strictly preserved** in state, allowing the user to correct typos without re-entering configuration.

### 3.4 Narrow Layout (390px Mobile Viewport)
- **Viewport Measurement:** 390px width x 844px height (iPhone 14/15 profile).
- **Containment:**
  - Top navigation tabs (`Machines`, `Access to This Machine`, `Connection Details`) wrap gracefully onto separate lines.
  - Toolbar buttons ("Add Machine", "Refresh") and Search Input stack vertically on small viewports without overlapping.
  - Mixed list rows stack Machine Name, Type Badge, and Status indicator cleanly above the Action buttons ("Add Project", "Details").
  - Expanded technical metadata grids collapse to 1 column with `break-all` wrapping for long Machine IDs and Relay URLs.
  - Dialog container is constrained by `max-w-lg w-full max-h-[90vh] overflow-y-auto`, ensuring full modal content accessibility on short/narrow screens without clipping.

---

## 4. Defect & Observation Ledger

| ID | Severity | Category | Description | Recommendation |
|---|---|---|---|---|
| **OBS-001** | Low / UX | Focus Management | Initial modal focus targets the active tab button rather than the primary input field because `querySelector` matches `[role="tab"][aria-selected="true"]` before form inputs in DOM order. | Refine selector to prioritize `'input:not([disabled]):not([type="hidden"]), textarea:not([disabled])'` before falling back to tab buttons. |
| **OBS-002** | Low / Polish | Layout | At exactly 390px, toolbar buttons ("Add Machine" and "Refresh") sit side-by-side; on viewports below ~340px, the search bar and buttons may wrap onto 3 lines. | Current responsive breakpoints (`flex-col gap-3 sm:flex-row`) handle 390px well. No immediate change needed. |

---

## 5. Artifact Catalog

All screenshot artifacts are committed under `docs/evidence/remote-machines-20260915/screenshots/`:
- `1280-empty-list.png`
- `390-empty-list.png`
- `1280-mixed-list.png`
- `390-mixed-list.png`
- `1280-paired-details.png`
- `390-paired-details.png`
- `1280-ssh-details.png`
- `390-ssh-details.png`
- `1280-modal-pin.png`
- `390-modal-pin.png`
- `1280-modal-ssh-advanced.png`
- `390-modal-ssh-advanced.png`
- `1280-modal-import.png`
- `390-modal-import.png`
- `1280-modal-failure.png`
- `390-modal-failure.png`
- `1280-modal-success.png`
- `390-modal-success.png`
- `1280-modal-escape-isolated.png`
- `390-modal-escape-isolated.png`

Full machine-readable run results: `docs/evidence/remote-machines-20260915/results.json`.

---

## 6. How to Re-Run QA Verification

```bash
bun docs/evidence/remote-machines-20260915/qa.mjs
```
The script will allocate an ephemeral port, launch Vite, execute the full suite in headless Bun.WebView, regenerate screenshots, and assert all 54 checks.
