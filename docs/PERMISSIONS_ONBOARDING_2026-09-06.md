# Permissions First-Run Onboarding (2026-09-06)

## Outcome
Replaced the 15-second permissions guidance toast with a deliberate first-run macOS
onboarding modal that walks the user through granting Full Disk Access, Accessibility,
and Notifications.

## Behavior
- Desktop start: after ~1.2s, if localStorage `ferryx.permissions.onboarding-dismissed`
  is unset and `get_system_permissions_status` reports platform=macos with any missing
  permission, the modal opens. No backdrop/Escape dismiss; explicit footer choices only.
- Rows: FDA -> "Open System Settings"; Accessibility -> "Request Access" + settings;
  Notifications -> "Enable Notifications" + settings fallback. Live status via 2s poll
  + window focus refetch; progress "N of 3 granted".
- All granted -> "Get Started" (marks dismissed). Otherwise "Don't show again" (marks
  dismissed) / "Remind Me Later" (re-shows next launch).
- Settings > Permissions section unchanged. Old toast module deleted.

## Files
- Added: ui/src/lib/permissionsOnboarding.ts(+test), ui/src/components/onboarding/PermissionsOnboardingDialog.tsx(+test)
- Modified: ui/src/App.tsx (toast effect -> onboarding gate), ui/src/lib/storageKeys.ts
  (ferryx.permissions.onboarding-dismissed), ui/src/App.test.tsx (tauri mock stub)
- Deleted: ui/src/lib/permissionsToast.ts, ui/src/lib/permissionsToast.test.ts

## Verification
- vitest (`bun run --cwd ui test`): 1627 passed, 7 failed -- all 7 pre-existing at
  pristine HEAD (reproduced in temp worktree): App "signed update" check,
  tauri.test.ts x3, push/client.test.ts x3. Unrelated to this change.
- `bun run --cwd ui build`: exit 0; dialog ships as its own lazy chunk.
- NOT verified on real desktop UI yet -- visual modal check pending (owner).

## Dev-run verification (2026-09-06, this machine)
- `bun tauri dev` initially failed: `scripts/macos-dev-runner.sh` expects
  `target/debug/Contents/Info.plist`, but nothing regenerates it after target/ is
  rebuilt, so the run aborted at `cp: .../Contents/Info.plist: No such file or directory`.
- Fix: runner now synthesizes a minimal dev Info.plist when missing (contract test
  `development_runner_synthesizes_missing_dev_info_plist` added; 3/3 pass).
- Relaunch succeeded: `FERRYX_FRONTEND_READY`, dev GUI pid 36843 from
  `target/debug/Ferryx.app/Contents/MacOS/ferryx`, dev daemon on `/tmp/rorca-501-dev/`
  (release app untouched), Vite on 5173. Log shows terminal spawns (UI booted).
- Dev bundle is ad-hoc signed with identifier com.ferryx.app but a distinct path, so its
  TCC permissions are tracked separately from /Applications/Ferryx.app -- ideal for
  onboarding QA because nothing is granted for the dev app.
- Layout fix after owner screenshot: buttons lived on the header line and the
  Accessibility row's two buttons overflowed the card edge. All three cards now share one
  vertical anatomy (title+badge / full-width description / bottom-right action row shown
  only while ungranted). Dialog tests 11/11 pass; change hot-reloaded into the running dev
  app via Vite HMR.
- Single-button rule (owner 2026-09-06): each ungranted onboarding row shows exactly one
  action -- the request API when macOS offers one (AX prompt, notification prompt),
  otherwise only the System Settings deep-link. Mirrors PermissionItemStatus.canRequest.
- Settings > Permissions completions (owner 2026-09-07): Notifications card gained the same
  XOR single-button rule with an "Enable Notifications" request action (it previously only
  deep-linked to settings); new subordinate "Re-run Welcome Setup" ghost button resets the
  onboarding dismissal and re-opens the modal immediately via the
  `ferryx:open-permissions-onboarding` window event (OPEN_PERMISSIONS_ONBOARDING_EVENT in
  ui/src/lib/permissionsOnboarding.ts, subscribed in App.tsx). Suites: PermissionsSection 5,
  dialog 4, lib 7 -- all green.

## Windows/Linux port and real-device verification (2026-09-06, commit 049c673 + e404e76)

Port: non-mac backend now maps notification permission to `Unknown` with
`canRequest=false` (no requestable-prompt claim) and exposes `canOpenSettings`;
UI is platform-conditional -- macOS 3 cards, Windows notifications-only with
"Managed by OS" badge and an "Open Windows Settings" deep link
(`ms-settings:notifications` via `cmd_permissions_open_settings`), Linux
informational text with no buttons; onboarding stays macOS-only. TS wire union
extended with `unknown`/`canOpenSettings`; web stub returns `canOpenSettings:false`.

Real-device evidence (all commands run natively on each host at HEAD e404e76):

- omaki (Arch Linux x86_64, ssh indo@100.91.254.71): `cargo test --lib permissions`
  6/6 pass (RC=0); vitest 3 suites 17/17 (RC=0, node v22.23.2); `bun run build` RC=0;
  ghostty pin 6a508fd5 verified; code synced via incremental git bundles
  (e2a6785..049c673 then 049c673..e404e76).
- maho-win (Windows x86_64 msvc, ssh maho-win, C:\Users\sook\ferryx-permqa):
  `cargo test --lib permissions` 6/6 pass (RC=0, ghostty-vt-static.lib built from
  junctioned source, sha 6a508fd5); `bun run build` (tsc + vite) RC=0 after giving the
  QA checkout its own node_modules (the junction to the winbuild clone was stale --
  pre-sonner -- and tauri build.rs requires ui/dist to exist before cargo test);
  vitest 3 suites 17/17 (RC=0, node v22.23.2 direct).
- macOS (local): 17/17 under both `bun run test` and node-run vitest; cargo
  permissions 6/6; vite build exit 0.

Host-portability lessons baked into the tree/tests:

- vitest under the Bun runtime silently breaks `vi.mock` interception (mocks never
  install; the real tauri module's web stub rendered instead). Both remote hosts had
  no real Node, so `bun run` shimmed node->bun. Fix: user-local Node v22 + direct
  `node node_modules/vitest/vitest.mjs run` invocation. Vitest requires Node.
- `detectMacPlatform()` reads host navigator.platform/userAgent/process.platform, so
  mac-gated UI tests must pin the host (the rerun-onboarding test now pins
  navigator.platform/userAgent to macOS and restores in finally; commit e404e76).
  Delegation note: the gemini quota gateway rate-limited (150min retry) so this
  minimal test-only fix was implemented directly.
- Windows GUI cannot be verified over OpenSSH (Session 0 windows are not
  user-visible); interactive checklist handed to the user separately.
