# Remote password browser QA - 2026-09-15

Run: `bun docs/evidence/remote-password-20260915/qa.mjs`.
Actual RemoteSection/AddMachineModal and product CSS; isolated Vite dynamic port;
Bun.WebView headless Chrome. All IPC is labeled browser-only fixtures. No deps
installed, product edits, desktop/Tauri launch, real SSH or external operations.

## Results

Final rerun after removal of the competing row-focus effect: 34 checks,
34 passing, zero failures and zero runner exceptions. Exit code 0.
Viewports 1280x800 and 390x844. See results.json and run.log.
The exact same harness was rerun without changes.
Readiness uses MutationObserver subscriptions before actions and bounded failure
timeouts, never polling or fixed sleeps.

Passing at both widths:
- Default four-row mixed inventory; old top navigation buttons absent.
- Single Add Machine entry with plus SVG; access disclosure opens.
- Basic auth selector exposes Password without Advanced; masked, labeled input.
- Failed probe: setter before probe, no save; fields/password retained.
- Cancel/reopen clears password and host fields.
- Successful add: setter -> probe -> save; secret excluded from probe/save payloads
  and fixture persisted inventory.
- Existing password-auth Dev Server Edit opens blank reauth input; saving entered
  password invokes setter -> probe -> save without secret leakage.
- Initial modal focus, forward Tab boundary wrap, success-panel focus containment,
  Escape child isolation and trigger focus restoration.
- No horizontal document overflow at final tested state.

## Final focus verification

At both widths, success keeps focus on a BUTTON inside the open modal.
After successful add, Done, existing-host edit, and reopening Add Machine,
Escape closes the child, does not reach parent/window listeners, and restores
focus to the Add Machine trigger. Both previously discovered focus failures
are resolved in this final browser run. No product edits or harness changes made.

## Screenshots and limitations

Fresh screenshots per viewport were overwritten by this rerun: default, access,
password-basic, password-error, success, edit-reauth, final. Contact sheets were
regenerated. Both fresh contact sheets were submitted to the read/image tool,
which returned
"Current model does not support images. The image will be omitted from this
request." Screenshots are captured artifacts, NOT visually inspected or approved.
Lead must inspect with an image-capable viewer before claiming visual success.

qa.mjs LSP has no diagnostics. entry.tsx inherits original Vite root-absolute /ui
imports and standalone React typing context: LSP reports unresolved modules and
consequent implicit-any/useState diagnostics. Real Vite browser compilation and
execution succeeded. No product build/test claim is made.

Real SSH authentication and native credential storage are not verified by these
browser IPC fixtures. Actual API fixture results are separate backend-worker
st_01a0a402 evidence and were not run or verified by this child. Setter
rejection, reverse Tab, edit failure/cancel retention are not covered here.
