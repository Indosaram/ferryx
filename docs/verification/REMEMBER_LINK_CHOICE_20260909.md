# Remember terminal link choice

Date: 2026-09-09

## Result

The existing terminal link chooser now includes an unchecked-by-default
`Remember this choice` checkbox. After a successful browser open, checking it
saves the selected destination using the existing browser settings:

- `openLinksInBuiltInBrowser` reflects the selected destination.
- `showTerminalLinkActions` becomes false, so subsequent links open directly.
- Other browser preferences, including the Shift override, remain unchanged.

An unchecked one-time choice or closing the chooser does not save preferences.
The chooser explains where to change the decision:
Settings > Browser > Link Routing. Enabling `Show terminal link actions` restores
the chooser. No new storage key or dependency was introduced.

Production change: `ui/src/components/TerminalLinkActions.tsx`.
Regression tests: `ui/src/components/TerminalLinkActions.remember.test.tsx`.

## Verification

Before implementation, the new tests failed in four cases because the checkbox
was absent; the unchecked one-time-choice case passed.

The final command passed 38 tests in four files, followed by a successful
TypeScript check and Vite build, exit code 0:

```sh
bun run --cwd ui test \
  src/components/TerminalLinkActions.remember.test.tsx \
  src/components/TerminalLinkActions.test.tsx \
  src/lib/linkRouting.test.ts \
  src/lib/browserSettings.test.ts
bun run --cwd ui build
```

New coverage exercises both remembered destinations, next-link automatic
routing, chooser remount with persisted settings, enabling the chooser again,
unchecked choices, dismissal, and Shift-click after remembering the built-in
browser. The real settings and routing modules run; only the OS browser opener
and toast hosting boundary are mocked.

An isolated headless WebView rendered the actual component, Sonner toaster, and
built application CSS, without restarting the desktop development app:

- Desktop 1000 x 700: dialog rectangle `(560, 510, 416, 166)`, no horizontal overflow.
- Mobile 390 x 844: dialog rectangle `(16, 662, 358, 166)`, checkbox visibly checked,
  no horizontal overflow.
- Clicking the checked built-in action emitted the settings-saved event with
  `openLinksInBuiltInBrowser: true` and `showTerminalLinkActions: false`.
- Reloading the page and opening another link called the registered opener
  directly and left zero chooser dialogs.

Fresh screenshots:

- `link-choice-desktop-20260909.png`
- `link-choice-mobile-20260909.png`

A visual reviewer inspected both images and reported PASS: checkbox, labels,
helper text, close button, and destination buttons were visible and did not
overlap or clip. The long URL was intentionally ellipsized.

LSP diagnostics could not run because its daemon socket was unreachable.
The successful TypeScript build supplied the type check. `git diff --check`
passed. The temporary headless harness was removed after verification.

## Development shutdown

Sent Ctrl+C to the existing `bun tauri dev` session. Its monitor completed with
exit code 0. A process check confirmed the development launcher and debug GUI
were gone. The previously running installed app (PID 666) and its daemon
(PID 827) remained. The debug daemon was no longer present after shutdown;
no separate daemon-kill command was issued.

No desktop mouse or keyboard automation was performed. Actual native browser
opening was not retested after shutdown; the tests isolate that OS boundary.

Verification was captured before the feature commit. Existing unrelated changes
in the shared working tree were preserved and excluded from this commit.
