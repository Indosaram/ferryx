# Windows GUI Manual Checklist — Permissions Port (2026-09-06)

Automated verification for the permissions port is complete on macOS, Linux
(omaki), and Windows (maho-win) — see
`docs/PERMISSIONS_ONBOARDING_2026-09-06.md` for the evidence matrix. The one
thing automation cannot prove is a **visible** Windows GUI: OpenSSH-launched
builds run in Session 0 where `ferryx.exe` windows are not user-visible
(2026-08-30 incident). Please run the following checks yourself in an
**interactive** PowerShell session on maho-win.

## Setup

The QA worktree is already prepared at `C:\Users\sook\ferryx-permqa`
(commit `e404e76`, own `node_modules`, `ui\dist` built, cargo permissions tests
6/6 green). In an interactive PowerShell (not SSH):

```powershell
cd C:\Users\sook\ferryx-permqa
bun tauri dev
```

Wait for the Ferryx window to appear.

## Checks

1. Window visibility: the Ferryx window renders normal content (not a black
   screen). If black, stop and report — that is the known WebView2 class of
   failure, not a permissions issue.
2. Open Settings → System Permissions section. Expect on Windows:
   - Only ONE permission card: "Desktop Notifications".
   - NO "Full Disk Access" card, NO "Accessibility" card.
   - Badge on the notifications card reads "Managed by OS".
   - NO "Enable Notifications" button (Windows notification permission is
     OS-managed; the app must not claim it can request a prompt).
   - One button: "Open Windows Settings".
3. Click "Open Windows Settings": Windows Settings must open on
   System → Notifications (`ms-settings:notifications`). PASS if the
   Notifications page opens; FAIL otherwise (note what opened instead).
4. No macOS text anywhere in the section (no "System Settings > Notifications"
   Apple-style wording, no Full Disk Access guidance).
5. Onboarding is macOS-only: NO "Welcome to Ferryx" permissions modal should
   appear on first launch of this build (and no "Re-run Welcome Setup" button
   in the Windows section).

Report back: window visible Y/N, which cards/badge you see, whether the
ms-settings deep link opened the Notifications page, and any deviation from
the above.

## Optional Linux desktop check

On any Linux desktop machine, the same section should show a notifications
informational text with no buttons at all (no request, no settings deep link).
omaki is headless, so this needs a desktop Linux session to check visually;
backend/UI behavior is already covered by the omaki native test run.
