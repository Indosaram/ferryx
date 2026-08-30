# Windows Remediation Completion Audit — 2026-08-30

## Objective

Fix the reproduced transparent/blank Ferryx Windows client, prove the fixes with Windows tests and builds, launch the actual checkout through exactly `bun tauri dev` on `maho-win`, preserve unrelated dirty work, remove only QA-owned process and port artifacts, re-audit broader Windows behavior, and finish only after the user confirms the visible window is opaque, renders Ferryx UI and terminal content, and has no duplicated native menu.

## Prompt-to-artifact checklist

| Requirement | Evidence | Status |
| --- | --- | --- |
| Windows client must be opaque | `src-tauri/tauri.windows.conf.json` overrides the shared transparent window with `transparent: false`; `windows_window_opacity_contract` passes on Windows | PASS |
| macOS-style native menu must not appear on Windows | `install_app_menu` and its setup call are gated by `#[cfg(target_os = "macos")]`; `platform_menu_contract` passes on Windows | PASS |
| Clean checkout must launch in development | `beforeDevCommand` invokes `bun scripts/dev-frontend.mjs`; `clean_dev_resource_contract` passes | PASS |
| Vite must not become an orphan | The runner uses Vite's `createServer()` in-process. Windows lifecycle probe observed port 5173 owned by `bun scripts/dev-frontend.mjs`, then `VITE=0` and `RUNNER=0` immediately after terminating that process | PASS |
| Windows users must be able to choose terminal kinds | New-tab popover exposes Default shell, PowerShell, Windows PowerShell, Command Prompt, and WSL; the selection is forwarded through `TabBar`, `App`, and `workspaceStore` to `SpawnTerminalRequest.shell` | PASS |
| Shell picker behavior must be tested | `NewTabPopover.test.tsx` and `workspaceStore.test.tsx`: 46/46 tests pass on the actual Windows checkout | PASS |
| Frontend must type-check and build | `bun run --cwd ui build` passes on the actual Windows checkout | PASS |
| Corrected app must launch through the exact required command | From `C:\Users\sook\ferryx-winbuild\orca-lite`, `bun tauri dev` completed the dev build in 25.69 seconds and launched `target\debug\ferryx.exe` | PASS |
| Runtime components must be live | Final snapshot: GUI=1, daemon=1, Vite=1; port 5173 owner is `bun scripts/dev-frontend.mjs` | PASS |
| Existing dirty work must be preserved | Actual checkout had 46 pre-existing dirty entries. Verified remediation added 13 intended file entries, yielding 59; target patches applied cleanly without reverting unrelated files | PASS |
| QA artifacts must be scoped | Only Ferryx processes from the actual checkout and the checkout-owned port-5173 process tree were terminated during cleanup; final lifecycle probe leaves no runner or Vite after termination | PASS |
| Broader Windows behavior must be re-audited | Current branch retains prior passing daemon/PTTY, shell/CWD, native-surface, edge-probe, launcher, path, WSL CLI, and clean-build evidence recorded in the durable ulw-loop ledger | PASS (automated scope) |
| Visible client must be opaque and show UI/terminal content | Requires direct observation of the newly launched Windows desktop window; desktop manipulation is prohibited | WAITING FOR USER |
| Visible client must have no duplicated native menu | Requires direct observation of the newly launched Windows desktop window; code and contract prove the Windows menu is not installed | WAITING FOR USER |

## Completion verdict

The automated and runtime portions are complete. The durable goal is not complete until the user inspects the currently launched `bun tauri dev` window and confirms both remaining visible criteria.
