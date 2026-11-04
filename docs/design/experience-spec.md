# Ferryx experience spec for the Rex adoption UI work

Date: 2026-11-04. Status: input contract for plan `rex-adoption-20261104` todos 8, 10, 11, 19,
20 and 21 (and the UI parts of 14, 18, 22 and 24 that follow them). Source: adoption plan v4
section 3.5a-c and 3.1(b), the draft finding F-UX/WIN, and the current source listed under
each section. This document changes no code. Where it and `docs/DESIGN.md` / `ui/DESIGN.md`
disagree, those documents win for existing screens and this one wins for the new screens
and states it defines.

Rules that apply to every section:

- A new screen or state ships with its i18n keys in `ui/src/i18n/en.json` under the prefix
  named here and its file listed in `ui/i18n-enforced.txt` (todo 19 check script).
- No new hex literal, font, or animation outside `ui/src/tokens/`.
- Desktop (`App`) and phone (`RemoteApp`) share tokens and copy keys; only layout differs.
- Every state table below is complete: a screen that can be empty, loading or failed shows
  the listed copy, never a blank region or a raw error string.

## Screen list

| Screen | Surface | Owning todo | i18n key prefix | States |
| --- | --- | --- | --- | --- |
| Welcome wizard shell (step rail, footer buttons) | desktop | 19, 21 | `onboarding.wizard.*` | resuming, step, error |
| Onboarding step bodies (one per step id) | desktop | 19, 21 | `onboarding.step.<stepId>.*` | loading, ready, error, skipped |
| Onboarding resume banner | desktop | 21 | `onboarding.resume.*` | shown, dismissed |
| Onboarding cross-device notice | desktop | 22 | `onboarding.sync.*` | synced, offline, conflict-merged |
| Getting started checklist | desktop | 21 | `onboarding.checklist.*` | empty, partial, done |
| Size owner badge on pane header | desktop | 8 | `sizeOwner.badge.*` | desktop-owned (hidden), phone-owned, other-desktop-owned |
| Size owner control on phone terminal header | phone | 8 | `sizeOwner.phone.*` | owner, not-owner, viewer, error |
| Phone view switch (Chat / Terminal / Browser) | phone | 19 | `remote.viewMode.*` | chat, terminal, browser |
| Phone chat view | phone | 19 | `remote.chat.*` | loading, empty, no-agent, error, ready |
| Phone terminal view | phone | 8, 19 | `remote.terminal.*` | connecting, ready, disconnected |
| Layout sync status (conflict toast) | desktop | 10 | `layout.sync.*` | applied, conflict-replaced, daemon-older |
| Detached sessions sidebar section | desktop | 11 | `layout.detached.*` | empty (hidden), list, attaching, error |
| Detached marker in phone inventory | phone | 11 | `remote.inventory.detached.*` | detached |
| Deep link toasts and confirm dialog | desktop | 20 | `deepLink.*` | unknown-link, unknown-target, confirm, opened |
| Keyboard shortcuts cheat sheet | desktop | 20 | `help.shortcuts.*` | list, filtered-empty |
| In-product docs search | desktop | 20 | `help.search.*` | idle, results, no-results, index-missing |
| Settings > Import | desktop | 28 | `settings.import.*` | idle, preview, skipped-items, error |

Key naming: `<area>.<screen>.<element>` in lowerCamelCase segments, for example
`sizeOwner.badge.takeBack`. Accessible names use the suffix `.label`, helper text `.hint`,
error text `.error.<code>` where `<code>` is the lowerCamelCase form of the structured backend
error code (for example `layout.sync.error.layoutConflict` for `LAYOUT_CONFLICT`); copy is
selected by `{ code }`, never by matching message text.

## Tokens

Single source: `ui/src/tokens/` (todo 19) with `colors.ts`, `spacing.ts`, `motion.ts` and
`index.ts`. `ui/scripts/gen-token-css.ts` writes the `:root` variable block consumed by
`ui/src/index.css` and `ui/src/settings-runtime.css`.

- Color tokens keep the existing CSS variable names and values (`--background`,
  `--foreground`, `--card`, `--primary`, `--muted-foreground`, `--destructive`, the
  `--chat-*` family, `--status-*`, `--terminal`, `--terminal-divider`). The migration is
  value-for-value: the light/dark screenshots of todo 19 differ only where a hex literal was
  wrong before. Theme (`:root[data-theme]`), accent (`:root[data-accent]`) and density
  (`:root[data-density]`) selectors stay; only their values come from tokens.
- New semantic tokens introduced by this spec, each with a light and dark value that meets
  the Accessibility section:
  - `--size-owner-badge` / `--size-owner-badge-foreground`: the "Sized by phone" badge.
    Derived from `--status-warning` at 15% fill with full-strength text, same recipe as the
    phone "waiting" chip in `RemoteApp.tsx`.
  - `--detached-marker`: the detached session dot; reuses `--status-idle` hue.
  - `--onboarding-step-done`, `--onboarding-step-current`: step rail glyphs; map to
    `--status-success` and `--primary`.
- Spacing: the Tailwind scale stays; `spacing.ts` exports the named values that TS code
  needs (`touchTarget = 44`, `phoneHeader = 56`, `desktopHeader = 28`, `gap = 8`).
- Hex literals in `ui/src/lib/terminalSettings.ts` (21) and `ui/src/lib/tauri.ts` (21) become
  token imports; terminal theme palettes are data tokens (`colors.terminalPalettes`), not CSS
  variables.
- Contract tests `appearanceThemeContract.test.ts`, `workspaceThemeContract.test.ts` and
  `MobileChatTheme.contract.test.ts` stay green and are the guard for value drift.

## i18n

Framework (todo 19): `i18next` + `react-i18next`, resources in `ui/src/i18n/en.json`, setup in
`ui/src/i18n/index.ts`, English only in this plan. Language detection is off; the locale is
`en` until a second resource file exists.

- Enforcement is per file: a file listed in `ui/i18n-enforced.txt` may not contain JSX text
  literals or hex literals (`bun ui/scripts/check-hardcoded.ts`). The first enforced file is
  `ui/src/components/onboarding/WelcomeWizard.tsx`; each later todo adds the files it creates
  or rewrites.
- Allowed literals in enforced files: punctuation-only text, product names passed as
  interpolation values (`Ferryx`, agent names), keyboard glyphs rendered by the shortcut
  formatter, and `data-testid` values.
- Interpolation uses named values (`{{device}}`, `{{machine}}`, `{{step}}`, `{{count}}`), never
  string concatenation. Plurals use i18next `_one` / `_other` suffixes.
- Copy style: sentence case, no trailing period on buttons and badges, a period on full
  sentences, plain verbs ("Take back", "Attach", "Resume setup"). Error copy says what happened
  and what the user can do, in that order.
- Existing copy that stays English-literal outside enforced files is not a regression; it
  moves to keys only when its file is touched by a todo listed above.

Copy keys defined by this spec (English source text):

| Key | Text |
| --- | --- |
| `onboarding.wizard.title` | Welcome to Ferryx |
| `onboarding.wizard.stepsLabel` | Setup steps |
| `onboarding.wizard.next` | Continue |
| `onboarding.wizard.back` | Back |
| `onboarding.wizard.skip` | Skip for now |
| `onboarding.wizard.finish` | Finish |
| `onboarding.wizard.close.label` | Close setup |
| `onboarding.resume.title` | Pick up where you left off |
| `onboarding.resume.body` | You finished {{count}} of {{total}} setup steps. |
| `onboarding.resume.action` | Resume setup |
| `onboarding.resume.dismiss` | Not now |
| `onboarding.sync.synced` | Setup progress from your other devices was applied. |
| `onboarding.sync.offline` | You are offline. Progress is saved on this computer and syncs later. |
| `onboarding.step.<stepId>.title` | one per step id, existing titles moved verbatim |
| `onboarding.step.enroll.title` | Link this computer |
| `onboarding.step.skills.title` | Agent skills |
| `onboarding.step.skills.body` | Install the Ferryx skill and hooks for the agents found on this computer. |
| `onboarding.step.<stepId>.error.load` | This step could not load. Try again or skip it. |
| `onboarding.checklist.title` | Getting started |
| `sizeOwner.badge.phone` | Sized by phone |
| `sizeOwner.badge.device` | Sized by {{device}} |
| `sizeOwner.badge.takeBack` | Take back |
| `sizeOwner.badge.takeBack.label` | Resize this terminal to fit this window again |
| `sizeOwner.phone.take` | Take size |
| `sizeOwner.phone.take.label` | Resize this terminal to fit this phone |
| `sizeOwner.phone.owner` | Sized for this phone |
| `sizeOwner.phone.viewer` | View only: size follows the desktop |
| `sizeOwner.phone.error.forbidden` | This device can view but not resize. |
| `remote.viewMode.label` | View |
| `remote.viewMode.chat` | Chat |
| `remote.viewMode.terminal` | Terminal |
| `remote.viewMode.browser` | Browser |
| `remote.chat.loading` | Loading the conversation |
| `remote.chat.empty` | No messages yet. Send one to start. |
| `remote.chat.noAgent` | No agent is running in this terminal, so there is no chat to show. Open the terminal to see its output. |
| `remote.chat.error.refresh` | Couldn't refresh the chat. Showing the last messages received. |
| `remote.chat.error.unavailable` | This session's chat can't be shown on this phone. |
| `remote.chat.openTerminal` | Open terminal |
| `remote.terminal.connecting` | Connecting |
| `remote.terminal.disconnected` | Disconnected. Reconnecting |
| `layout.sync.conflictReplaced` | Layout updated from another window. |
| `layout.sync.daemonOlder` | Layout is saved by this app until the background service is updated. |
| `layout.detached.title` | Detached |
| `layout.detached.attach` | Attach |
| `layout.detached.attach.label` | Attach {{name}} to the current window |
| `layout.detached.error.sessionNotFound` | That session has ended. |
| `remote.inventory.detached.label` | Detached |
| `deepLink.unknownLink` | Ferryx can't open that link. |
| `deepLink.unknownWorkspace` | Unknown workspace |
| `deepLink.confirm.title` | Open this link? |
| `deepLink.confirm.body` | A link wants to {{action}}. |
| `deepLink.confirm.accept` | Open |
| `deepLink.confirm.cancel` | Cancel |
| `help.shortcuts.title` | Keyboard shortcuts |
| `help.shortcuts.filter.label` | Filter shortcuts |
| `help.shortcuts.empty` | No shortcut matches "{{query}}". |
| `help.search.placeholder` | Search the docs |
| `help.search.empty` | No results for "{{query}}". |
| `help.search.error.indexMissing` | Docs search is not available in this build. |
| `settings.import.title` | Import from another terminal |
| `settings.import.error.importParseError` | That file could not be read. Nothing was changed. |

The `remote.chat.*` texts are the strings already shipped in `RemoteApp.tsx`
(`CHAT_REFRESH_WARNING`, `CHAT_UNAVAILABLE_WARNING`, the no-agent message); they move to keys
unchanged.

## Motion

- One hook: `ui/src/lib/usePrefersReducedMotion.ts` (todo 19) subscribes to
  `matchMedia("(prefers-reduced-motion: reduce)")` change events and replaces the six ad hoc
  calls (`MobileChatWorkspace.tsx`, `useDragDismiss.ts`, `useEdgeSwipeBack.ts`,
  `phoneTransition.ts`, `MobileKeyDock.tsx`, `usePullToRefresh.ts`). CSS keeps the media
  queries in `index.css`.
- `motion.ts` tokens: `durationFast = 120ms`, `durationEnter = 140ms` (the existing
  `animate-enter`), `durationSheet = 220ms`, `easeOut = cubic-bezier(0.2, 0, 0, 1)`,
  `enterOffset = 2px`.
- Animate only transform, opacity and filter. Every looping or translating animation has a
  reduced-motion branch that becomes an opacity change of at most `durationFast` or no
  animation.
- New motion in this plan, all optional under reduced motion:
  - Size owner badge: fade in with `durationEnter`; no pulse, no loop.
  - Terminal resize after a take/return: no animated reflow; the grid snaps.
  - Detached section: rows enter with `animate-enter`; attach removes the row without a
    collapse animation.
  - Onboarding steps: step change crossfades with `durationEnter`; the step rail never animates
    progress fill.
  - Phone view switch: no slide between Chat and Terminal; content swaps in place so the
    composer and terminal keep their scroll positions.

## Onboarding

Current state: `ui/src/lib/onboarding.ts` step ids `account, intro, features, agents, cli,
project, supervisor, try, phone`, order `intro, features, agents, cli, project, supervisor,
try, phone, account`, state `{ version: 1, completedSteps, dismissed }` in localStorage key
`ferryx.onboarding.v1`; `WelcomeWizard.tsx` renders it on desktop only.

Target step order (todo 21): `intro, features, agents, skills, enroll, cli, project,
supervisor, try, phone, account`. `skills` and `enroll` come right after `agents`; `import`
(todo 28, optional) comes right after `features`.

| Step id | Key prefix | Scope | Satisfied when |
| --- | --- | --- | --- |
| `intro` | `onboarding.step.intro.*` | account | marked completed |
| `features` | `onboarding.step.features.*` | account | marked completed |
| `import` | `onboarding.step.import.*` | machine | completed or skipped |
| `agents` | `onboarding.step.agents.*` | machine | a default agent is chosen or none is installed |
| `skills` | `onboarding.step.skills.*` | machine | every detected agent reports installed or the user skips |
| `enroll` | `onboarding.step.enroll.*` | machine | this computer is enrolled (`accountLinked`) |
| `cli` | `onboarding.step.cli.*` | machine | CLI launcher installed |
| `project` | `onboarding.step.project.*` | machine | at least one project |
| `supervisor` | `onboarding.step.supervisor.*` | account | `supervisorReady` true |
| `try` | `onboarding.step.try.*` | account | marked completed |
| `phone` | `onboarding.step.phone.*` | account | a paired device exists |
| `account` | `onboarding.step.account.*` | account | signed in |

Resume rules (todo 21):

1. Storage tiers, highest authority first: daemon state (`onboardingStateV1` capability,
   `<data>/onboarding-daemon.json`), the local file `<data>/onboarding.json` (written through
   `cmd_onboarding_set`, readable before any daemon exists), then localStorage
   `ferryx.onboarding.v1` as a cache. A write goes to every available tier; a read takes the
   highest available tier and refreshes the lower ones.
2. Migration runs once: when the daemon first reports the capability and has no state, the
   GUI sends the file state; afterwards the daemon copy wins.
3. On launch the wizard opens at the first step in order that is neither completed nor
   satisfied by context. If at least one step is completed and the wizard was not dismissed,
   the desktop shows the resume banner instead of opening the wizard modally.
4. A corrupt or unreadable file is treated as absent: the wizard starts at the first
   unsatisfied step and the file is rewritten; no error dialog is shown.
5. Onboarding state never enters terminal snapshots or layout files.
6. `ferryx://onboarding/<step>` (Deep links) opens the wizard at that step even if it is
   completed; it does not mark anything completed.

Cross-device rules (todo 22):

- Only account-scope steps sync: `account, intro, features, supervisor, phone, try`. Machine
  steps (`enroll, cli, skills, project, agents, import`) never leave the computer; the account
  service rejects them with `MACHINE_STEP_NOT_SYNCABLE`.
- Merge: union of `completedSteps`, `dismissed` is OR, newest `updatedAt` wins for ordering.
  Push is debounced 2 s after each change; pull on sign-in and app start. Offline keeps the
  local copy and shows `onboarding.sync.offline` once per session.
- When a pull completes steps the user is currently looking at, the wizard advances to the
  next unsatisfied step and announces `onboarding.sync.synced` in the live region.

States per step body: loading (skeleton rows, `aria-busy="true"`), ready, error
(`onboarding.step.<stepId>.error.load` with Retry and Skip), skipped (step rail shows the
skipped glyph and the step remains reachable).

## Deep links

Scheme `ferryx://` (todo 20), registered on macOS, Windows (installer and MSIX
`windows.protocol`) and Linux (AppImage `register_all`). Router: `ui/src/lib/deepLinks.ts`.

| Route | Effect | Confirm |
| --- | --- | --- |
| `ferryx://settings/<section>` | open Settings at a `SectionId` from `ui/src/components/settings/types.ts` (`general`, `appearance`, `terminal`, `shortcuts`, `agents`, `browser`, `notifications`, `remote`, `plan`, `permissions`, `ssh`) | no (navigation only) |
| `ferryx://onboarding/<step>` | open the wizard at a step id from Onboarding | no (navigation only) |
| `ferryx://open?workspace=<id>` | focus the window showing that workspace | no (navigation only) |

- An unknown route or section shows the toast `deepLink.unknownLink`; an unknown workspace
  shows `deepLink.unknownWorkspace`; in both cases no state changes.
- Any future route that changes state (creates, closes, sends, installs, enrolls) shows the
  confirm dialog `deepLink.confirm.*` naming the action, with Cancel focused by default. No
  route may act on a terminal session, close anything, or send input.
- A link that arrives while another instance runs is forwarded through single-instance; the
  existing window comes to front. A link arriving during onboarding for a settings section
  opens Settings above the wizard; closing Settings returns focus to the wizard heading.

In-product docs (todo 20):

- Help > Keyboard shortcuts: cheat sheet generated from `ui/src/lib/shortcuts.ts`, honouring
  user overrides; grouped by the existing shortcut categories; filter input on top.
- Help > Search docs: `ui/public/search.json` generated from docs headings; results open the
  docs page in the default browser. States idle, results, no-results, index-missing.
- Help > For agents: links to `llms.txt` (todo 3).

## Mobile chat-first

Policy (owner decision 2026-10-06, paired with size ownership): on a phone the chat is the
default view of a session, and the terminal is an explicit switch.

- `RemoteApp` starts every session in `viewMode = "chat"` (current behaviour,
  `RemoteApp.tsx` `useState<"chat" | "terminal" | "browser">("chat")`). Opening a session from
  the inventory, a notification, or the inbox lands in Chat.
- The view switch (`remote.viewMode.*`) is a `role="group"` labelled `View` in the phone
  header with Chat, Terminal and, when available, Browser; each is a toggle button with
  `aria-pressed`, 44x44 px target. The choice is per session and in memory only; it is not
  persisted, so a reload returns to Chat.
- Chat states: loading, empty, ready, no-agent (copy `remote.chat.noAgent` with an
  `remote.chat.openTerminal` button), error (`remote.chat.error.refresh` keeps the last
  messages; `remote.chat.error.unavailable` offers Open terminal).
- Switching to Terminal does not resize the session. The phone becomes size owner only by the
  rules in Size ownership (first Resize from a Control device or Take size). Switching back
  to Chat keeps ownership until the phone detaches or the desktop takes it back; the phone
  shows `sizeOwner.phone.owner` in the terminal header while it owns.
- Detached sessions (todo 11) appear in the phone inventory with the `Detached` marker and
  open in Chat like any other session.
- Desktop is unaffected: panes are terminals first; chat is not a desktop default.

## Size ownership

Behaviour of record is todo 8 (`SizeOwnership { owner, applied, desktop }` per session,
capability `sizeOwnershipV1`, event `SizeOwnerChanged`). This section fixes the UX.

Rules visible to the user:

1. The desktop owns the size by default. No badge is shown while the desktop owns it.
2. A phone with Control permission becomes owner on its first terminal Resize after attach
   or when the user taps Take size. A phone in Chat view does not send Resize.
3. A view-only device never changes the size; its Resize is recorded as a preference only.
4. When the owning phone detaches (closes the tab, loses the connection past the reconnect
   window, or switches to another session), ownership returns to the desktop and its
   remembered size is applied.
5. The desktop takes ownership back on any local resize from the focused window, or when the
   user presses Take back.
6. Two Control phones: the last explicit taker wins; automatic resizes from a non-owner never
   flip ownership, so the size cannot oscillate.

Desktop pane header (`sizeOwner.badge.*`):

| State | Badge | Action |
| --- | --- | --- |
| desktop owns | none | none |
| phone owns | `Sized by phone` | `Take back` button |
| another desktop or named device owns | `Sized by {{device}}` | `Take back` button |
| take back in flight | badge stays, button disabled with `aria-busy` | none |
| take back failed | badge stays; toast with the structured error copy | button re-enabled |

The badge sits right of the pane title, uses `--size-owner-badge`, has text (never color
only), and the button has accessible name `sizeOwner.badge.takeBack.label`. While the phone
owns, the desktop pane renders the phone grid top-left at the phone size with the remaining
area in `--terminal` backing; it does not scale the font.

Phone terminal header (`sizeOwner.phone.*`):

| State | Shown | Action |
| --- | --- | --- |
| this phone owns | `Sized for this phone` | none |
| not owner, Control permission | `Take size` button | sends `takeSize` |
| view-only device | `View only: size follows the desktop` | none |
| take failed with permission error | `sizeOwner.phone.error.forbidden` | none |

The phone renders a non-owned grid fit-to-width with horizontal scroll, never by resizing.
Ownership changes are announced through a polite live region once per change, using the
badge or header text as the announcement.

## Accessibility

Applies to every screen in the Screen list, desktop and phone, light and dark.

- Contrast: text >= 4.5:1, large text and non-text UI (icons that carry meaning, outlines,
  focus rings, badge borders) >= 3:1 against their actual background, in both themes and all
  five accents. New tokens are added to the contrast contract test of their surface
  (`MobileChatTheme.contract.test.ts` on phone, a new token contrast test in todo 19 on
  desktop).
- Targets: 44x44 CSS px on phone for every control including the view switch, Take size and
  sheet buttons; desktop keeps its compact density but every icon-only control has an
  accessible name and a tooltip.
- Focus order:
  - Welcome wizard: dialog heading (focused on open and on every step change, as today),
    step body controls in DOM order, then Back, Skip, Continue; Escape closes and returns
    focus to the control that opened it. The step rail is an ordered list with
    `aria-current="step"` and is not in the tab order.
  - Resume banner: Resume setup, then Not now; it does not steal focus on launch.
  - Pane header: pane title, size badge Take back, then the existing pane actions.
  - Detached section: section heading, then one row per session with its Attach button.
  - Deep link confirm dialog: Cancel focused first, then Open; focus returns to the previously
    focused element on close.
  - Phone header: back, context picker, waiting chip, view switch buttons, overflow.
- Focus is always visible (`focus-visible` ring with the `--ring` token, >= 3:1). No focus
  traps outside modal dialogs.
- Reduced motion: every animation in Motion has a reduced branch driven by
  `usePrefersReducedMotion` or the CSS media query; verified by tests that stub
  `matchMedia` and assert the reduced class or zero duration.
- Screen readers: status changes (size owner, onboarding sync, layout conflict replace,
  attach result) go through one polite live region per surface; errors that block an action
  use `role="alert"`. Status is never conveyed by color alone (badge text, detached label,
  step rail glyph plus `aria-label`).
- Automated checks: axe-core under vitest + jsdom for WelcomeWizard and the RemoteApp shell
  (todo 19), extended by each later todo to the screen it adds (size badge, phone size
  control, detached section, deep link confirm, cheat sheet). Zero violations of impact
  serious or critical.
- Manual checks per todo QA: keyboard-only walk through the new screen on desktop; VoiceOver
  or TalkBack spot check of the phone header; light and dark at 390 px and 1440 px.

## Handoff to dependent todos

| Todo | Uses sections |
| --- | --- |
| 8 | Size ownership, Mobile chat-first, Accessibility, keys `sizeOwner.*` |
| 10 | Screen list (`layout.sync.*`), Motion |
| 11 | Screen list (`layout.detached.*`, `remote.inventory.detached.*`), Mobile chat-first |
| 19 | Tokens, i18n, Motion, Accessibility |
| 20 | Deep links, keys `deepLink.*`, `help.*` |
| 21 | Onboarding, keys `onboarding.*` |
| 22 | Onboarding cross-device rules |
