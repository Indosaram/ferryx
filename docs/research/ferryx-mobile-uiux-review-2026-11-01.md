# Ferryx mobile UI/UX review (2026-11-01)

Scope: the phone client, i.e. `ui/src/remote/**` (`RemoteApp`, `home/`, `chat/`, login/pairing, drawer, terminal, browser) and the Tauri shell in `mobile/` that loads the same bundle (`ui/vite.mobile.config.ts`, `VITE_FERRYX_MOBILE=1`).
Revision: `1f1e1fde` (main, HEAD).
Method:
- Apple HIG was read in full across three parallel lanes: Foundations 18/18 pages, Patterns 25/25, Components + Inputs + iOS/iPadOS 79/79.
- The rules from impeccable (GitHub plus the local skill, 92 rules and the detector) and the frontend skill (176 rules plus the lints) were extracted.
- Three lanes read the code: the shell, the home, and chat/terminal.
- I took my own screenshots: the QA harness was built on noye and rendered with Bun.WebView at 390x844, 375x667 and in light theme. DOM measurements came from the same renders.
- The lints were run on noye.

Evidence root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/mobile-uiux-review-20261101/`
- `lanes/*.md`: source extraction lanes (HIG x3, impeccable, frontend, code x3)
- `shots/*.png`: screenshots (`<state>-<width>.png`, `-light-` for light theme, `sheet-*.png` for contact sheets)
- `dom-metrics-390.json`: measured hit targets, font sizes under 12px, input font sizes
- `contrast-390.json`: elements below 4.5:1, from rendered colors
- lint output: written into the report body (on noye `~/fx/wt/mobileux-20261101/{draft-guard*.txt,detect*.json}`)

Labels: **[measured]** means an observation from a render or DOM measurement. **[source]** means read from code at file:line. **[inferred]** means a judgment with no direct measurement. Of the code lanes' findings, two that I could check by rendering or code turned out wrong and were dropped (see Appendix B).

---

## 1. Universal interface principles extracted from HIG (Apple-specific items removed)

Apple-only material (Liquid Glass, SF Symbols, Dynamic Type APIs, the specific iOS bar heights) was removed, keeping only rules that apply to any mobile web or hybrid app. Each source rule has an ID in `lanes/hig-*.md` (F-xx Foundations, P-xx Patterns, C-xx Components).

### 1.1 Perception and accessibility
| ID | Principle | Source |
|---|---|---|
| U1 | Every touch target is at least 44x44 CSS px. Only secondary controls may go down to 28px, and then with at least 8px of spacing. Enlarge hit areas with padding or a pseudo-element. | F-03, C-01 |
| U2 | Body text is 16-17px, and nothing is smaller than 11px. Inputs are at least 16px, because anything smaller makes iOS zoom in on focus. | F-25, C-13 |
| U3 | Text contrast is at least 4.5:1, or 3:1 for large text and essential glyphs. Check both dark and light themes. | F-01 |
| U4 | Status is never shown by colour alone. Always pair colour with text, shape or an icon. | F-02, P-04, C-14 |
| U5 | The UI survives text scaled to 200% and reflows instead of clipping. | F-04, F-23 |
| U6 | Every gesture has a visible single-tap alternative. Don't fight system edge gestures. | F-05, C-17 |
| U7 | Respect `prefers-reduced-motion`. Motion has a cause, is short and interruptible, and is never the only carrier of information. | F-10, F-33..35 |
| U8 | Respect the safe areas: notch, home indicator, keyboard, and in landscape the left and right edges. | F-21 |

### 1.2 Structure and navigation
| ID | Principle | Source |
|---|---|---|
| U9 | Bottom tabs switch top-level sections only (at most 5, one-word labels, never hidden). Actions don't go in the tab bar. | C-03, C-23 |
| U10 | Always show where the person is: a short title, the selected tab, the highlighted selection. | C-04 |
| U11 | Back closes the topmost overlay first, then pops the screen. A tab switch is not a back-history step. | C-05, C-06 |
| U12 | One modal at a time. On a phone, use sheets instead of popovers. A sheet has a grabber, swipe-down dismiss, and a discard confirmation when it holds unsaved input. | C-06..08, P-08 |
| U13 | Only one scroll container per axis. No nested scrolling on the same axis. | C-19 |
| U14 | Primary controls sit in the thumb zone (bottom and middle). | C-21 |
| U15 | Progressive disclosure: the essentials first, the details one step down. | F-18, F-19 |

### 1.3 State and feedback
| ID | Principle | Source |
|---|---|---|
| U16 | Paint something right away (a skeleton or the last-known state). Never show an empty screen. Loading text names what is loading. | P-01, C-12 |
| U17 | Reopening, reconnecting or switching apps returns the person exactly where they were: screen, scroll, draft. | P-02 |
| U18 | Feedback scales with importance. Ordinary state is shown inline; interrupt only for critical or irreversible events. | P-03 |
| U19 | Errors say the cause, what to do next, and where it happened. No raw codes. | F-37, P-05 |
| U20 | Empty states say why they are empty and offer the next action. | F-38 |
| U21 | Destructive actions are red, never the default, placed last, and paired with Cancel. Common deletes prefer undo. | F-11, C-09, C-10, P-06 |
| U22 | Live state and historical records are distinct at a glance. Running work stays reachable in one tap. | P-15 |
| U23 | Every tap gives immediate pressed feedback. Async buttons show a spinner and a changed label. | C-02 |
| U24 | Notifications and badges are honest: urgency reflects reality, badges count only unread items, and tapping deep-links to the exact item. | P-09, C-20 |

### 1.4 Language and input
| ID | Principle | Source |
|---|---|---|
| U25 | Buttons are verbs. Use the person's vocabulary, with no internal jargon, and one term per concept. | F-36, F-40 |
| U26 | Minimise input: suggest, prefill, choose instead of type. Validate inline. Paste always works. | P-12 |
| U27 | Ask for permissions (notifications, microphone) at the moment they are needed, with the reason. | F-45, P-10 |
| U28 | Inputs have a persistent label beyond the placeholder, and the right keyboard type, autocomplete and return-key label. | C-13, F-39 |

### 1.5 Principles specific to remote control (from the HIG Patterns checklist, mapped to the dev-agent domain)
| ID | Principle | Source |
|---|---|---|
| U29 | An approval card shows what will run (command or diff, directory, agent, risk), with Approve and Deny as verbs. The safe choice is the default. | P-08, P-05 |
| U30 | An approval that was dismissed without a decision stays pending and stays visible. Its outcome (approved, running, result) is shown inline. | P-03, P-05 |
| U31 | A disconnect shows inline as reconnecting or offline, with the time of the last good state, never as a modal. Reconnecting loses no scrollback or draft. | P-03, P-14 |
| U32 | During streaming the working state is visible, and stop/interrupt is always reachable. | P-15, P-19 |
| U33 | In terminal mode, Esc, Ctrl, Tab and the arrow keys plus an exit are always reachable. | P-19 |

---

## 2. Personas, experience, journeys, and the ideal state

### 2.1 Inferred brief (designpowers R-164)
- **What it is:** the phone companion for Ferryx, a desktop terminal, worktree and agent manager. The phone runs nothing itself; it watches and steers sessions running on the person's computers through the relay.
- **Core tasks (by frequency):** (1) see which agents are waiting on me, (2) answer or approve, (3) check that a run has finished, (4) send a short instruction to a session, (5) intervene in the supervisor (pause or take over), (6) run a command directly in a terminal (rare).
- **Mode:** impeccable "Operate". Scannability, consistency and native expectations beat expression.
- **Quality bar:** the user benchmarks against Orca and native mobile apps (memory: human.md). Chat is the default; the terminal is an on-demand switch.

### 2.2 Personas (permanent, temporary, situational; Casey is mandatory)
| Persona | Situation | Abilities and constraints | Goal | What breaks them |
|---|---|---|---|---|
| **P1 Minho: lead dev away from the desk (primary)** | Phone in one hand on the subway or in a café, LTE dropping in and out. Glances at it 20-40 times a day for 10-30 seconds each. | One thumb, mostly outdoors, divided attention | "Does anything need me?", then approve or answer in one tap, then put the phone away | Small targets, raw codes, three-step navigation, reconnects that wipe what was on screen |
| **P2 Casey: one-handed and interrupted (impeccable mandatory)** | Holding something else, interrupted by a notification halfway through | Thumb zone only, short focus | Finish whatever was cut off right where it stopped | State lost after an interruption, controls at the top of the screen, 24px targets |
| **P3 Sora: on-call operator (stress)** | 2 a.m., a failed alert, slow 3G, in a rush | Tired, error-prone | Find the failing session, see why it stopped, retry or pause | Lists that look live but aren't, "something went wrong", destructive buttons next to the safe ones |
| **P4 Jun: low vision / 200% zoom (permanent)** | Larger text and stronger contrast settings | Can't read 10px gray, needs reflow | Read the same information | 10-11px meta text, #838383 on a dark background, fixed-width grids |
| **P5 Ari: screen reader (permanent)** | VoiceOver or TalkBack | Relies on focus order, names and live regions | Understand the state, approve | Live regions that chatter, sheets that lose focus, icon buttons without names |
| **P6 Hana: first-time user (learning)** | Just signed up, desktop isn't linked yet | Doesn't know the vocabulary (host, machine, worktree, lane) | Connect a computer and see the first session | An empty screen with no reason, jargon, CLI instructions on the phone |

### 2.3 Key journeys and their emotional curve
| # | Journey | Ideal flow (tap count) | Emotional peak |
|---|---|---|---|
| J1 | Notification → approve | Push tap → that approval card is already open → Approve (2) → result shown inline | Relief: "it's handled" |
| J2 | Glance check | App opens → home shows "2 waiting · 3 running · 1 done" right away, with the last-known state as the first paint (0) | Confidence |
| J3 | Short instruction | Session → composer (thumb zone) → send → working state shows immediately (2-3) | Sense of control |
| J4 | Supervisor intervention | Lane card → Pause (1); or the session strip → Take over → keyboard (2), with nothing handed over if the pause failed | Safety |
| J5 | First-time connection | Sign in → "Finding your computers…" → if none, "Connect from the desktop app" + QR/link → first session | Hope, not confusion |
| J6 | Recovery after disconnect | Background → return: same screen, scroll and draft; an inline "Reconnecting… last updated 2 min ago" | No anxiety |
| J7 | Direct terminal work | Session → Terminal toggle → 44px key dock (Esc/Ctrl/Tab/arrows) → back to chat | Not frustrated |

### 2.4 Ideal state: one line per item; the gaps in section 3 are measured against these
- **I1 Home = an action queue.** The first screen is a list of what needs me. Each row says who (session title), what (the question or command), where (computer · worktree) and when. The primary action (Approve / Answer / Open) is a 44px or larger text button on the row.
- **I2 One source of truth.** Inbox, badge and supervisor counts come from the same model and agree. No information repeats on a single screen.
- **I3 Live vs stale vs loading are always distinct.** Each machine row has a text state (Live / Reconnecting · 2 min ago / Offline / Loading); unknown is never shown as "off" or "none".
- **I4 Touch density is consistent.** Every interactive element is at least 44px, body text 15-17px, meta at least 12-13px, inputs 16px. Desktop sidebar components are not shown on the phone unchanged.
- **I5 Navigation follows the platform.** Tab switches are replace, sessions and overlays are push, and back closes the topmost overlay first. One Inbox and one Supervisor surface.
- **I6 Approvals are safe.** The card shows command, directory, agent and risk. The safe choice is the default, Approve and Deny are visually separated, and the outcome is shown inline.
- **I7 The composer is reachable and protected.** It sits in the thumb zone, survives the keyboard and safe area, keeps its draft, and Stop is always reachable while running.
- **I8 Plain language.** "Computer" consistently; no raw enums, error codes or session-id prefixes; empty states always offer the next action.
- **I9 Tokens, colour and motion are consistent.** Only chat-* / status-* tokens, with light and dark verified. Status is shown with text plus shape. Looping motion is stopped under reduced-motion.
- **I10 No AI-slop tells.** No coloured side stripes, decorative rings, gradient text, emoji icons, hype copy, or tiles that nobody acts on.
- **I11 The terminal is first-class.** A key dock, the tab strip, a connection state, and a plain explanation when it is unavailable (no raw placeholder strings).
- **I12 Restore after an interruption.** Screen, scroll, draft and selected session come back the same after backgrounding or reconnecting.

---

## 3. Product vs ideal state: the full gap list

Severity: P0 = blocks the task or hides an error/safety problem, P1 = a primary journey is hard, P2 = consistency or quality defect, P3 = polish.
Evidence labels: [measured] = screenshot or DOM measurement, [source] = code reading, [verify] = needs device confirmation.
Path abbreviations (relative to `ui/src/`): WS=remote/chat/MobileChatWorkspace.tsx, CMP=remote/chat/MobileChatComposer.tsx, MSG=remote/chat/MobileChatMessage.tsx, CC=remote/chat/MobileChatComponents.tsx, TOP=remote/chat/MobileTopBar.tsx, SH=remote/chat/MobileSheet.tsx, RPC=remote/chat/ReferencePromptCard.tsx, OVF=remote/chat/MobileOverflowSheet.tsx, SW=remote/chat/MobileSessionSwitcherSheet.tsx, RT=remote/RemoteTerminal.tsx, TAB=remote/RemoteTerminalTabStrip.tsx, DOCK=components/MobileKeyDock.tsx, RB/RBC/RBW=remote/RemoteBrowser{,Controls,Workspace}.tsx, AC=remote/agentConversation.ts, APP=remote/RemoteApp.tsx, CSS=index.css, QA=devtools/MobileChatQaHarness.tsx.

### 3.1 Key gaps by ideal-state item (summary; full list in 3.2)

**I1 Home = an action queue**
- The Supervisor tab is a monitoring dashboard: a decorative header card, a Goal repeated on every lane card, and a Recent log of raw enums (`input`, `held`). Pending approvals have no primary action on Home. `home/SupervisorHome.tsx:207-216` [measured `home-supervisor-390.png`, `home-supervisor-long-390.png`]
- The Inbox inline answer input is `h-6 text-[11px]` (24px, 11px) `features/ferryx/attention/AttentionInbox.tsx:443` [source]. Filter chips are 24px [measured `dom-metrics-390.json`].

**I2 One source of truth**
- The Inbox tab badge shows 2 while the list shows All 3 [measured `home-inbox-390.png`]. Status is shown three times: stripe + pill + group dot (`AttentionInbox.tsx:52-53, 256`).
- Sign-out appears twice: the Worktrees footer icon (28px, no confirm) and Account/menu "Sign out" [measured `home-worktrees-390.png`, `menu-open-390.png`] (H-lane cross-reference).
- Supervisor "off" shows the same fact in three places (`SupervisorHome.tsx:34, 230-234, 268-270`) [measured `home-supervisor-off-390.png`, `home-supervisor-unsupported-390.png`].

**I3 Live / stale / loading are distinct**
- **P0** The page-level empty state says "No computers are linked" while discovery is still loading (`SupervisorHome.tsx:236-240`, H-03) [source, confirmed].
- **P0 [verify]** The relay drops an idle account tunnel after about 2 minutes, and the shell only notices on the next worktree selection, so after the phone sleeps actions fail with STREAM_CLOSED (`APP:4695-4705, 2916, 2955`, `attachTunnel.ts:326,1262`, S-02).
- There is no offline/reconnecting state anywhere in the shell (S-06, `APP:2224-2412, 3302-3328`). "Connecting to…" has no spinner and no cancel (S-14, `APP:5180-5186`).
- The headline is computed only over readable machines, so "off" is claimed when nothing is reachable (H-04, `home/phoneHomeModel.ts:20-31`).

**I4 Touch density is consistent**
- [measured `dom-metrics-390.json`] Top bar icons 32x32 (`TOP:13-17`), terminal tab strip 24px (`TAB:98-99,165`), composer +/mic/send 32x32 (`CMP:443,455,467,481`), Copy 16x16, Approve/Decline about 28-30px (`RPC:289-311`, `CC:520-548`), drawer close/new 28x28, project/worktree rows 29px, Inbox chips 24px.
- 10-11px text is everywhere: status, meta, timestamps, tab bar labels 11px [measured]. Inputs under 16px trigger iOS zoom: "Filter worktrees" 14px [measured], pairing inputs 14px (S-12, `PairingPage.tsx:98-122`), prompt answer, approval note and held-message edit fields 12px (T-06, `RPC:414`, `CC:505`, `CMP:341`).
- At 200% text the session screen scrolls sideways at 468px and top bar icons are clipped [measured `stream-text200-390.png`, `sheet-text200.png`].
- At 768px and wider (tablets, landscape phones) the desktop header and sidebar are shown with 20-28px controls (S-10, `APP:4966-5059`).
- The design doc itself sets sub-44px controls (`ui/DESIGN.md` §6 `size-7` header controls, `docs/DESIGN.md` §3 10-13px scale; S-54, T-122), so the defects are by policy.

**I5 Navigation follows the platform**
- Tab switches push history and back doesn't close sheets (`home/usePhoneNavigation.ts:31-35`, S-09, H-lane). Android Back is not handled for sheets, the drawer, the picker or PIN-paired mode.
- The bottom sheet grabber suggests drag-to-dismiss but has no drag handling (`SH:212-218, 237-245`, T-88). The session switcher's close button is a 56px circle while its rows have weak affordance [measured `switcher-open-390.png`].
- A notification tap does nothing on the account phone home until a worktree has been opened (S-08, `APP:4559-4580`, `public/sw.js:99-114`).
- The browser view mode has no entry point and `RemoteDesignOverlay` has no importer (S-40, S-46).

**I6 Approvals are safe**
- **P0** In multi-option approvals, one tap on a 32px row sends the answer immediately, including "always allow" style options. `RPC:362-378` (button `onClick={() => void answer({optionIndex})}`), `space-y-1` = 4px spacing (`RPC:331`) [source, confirmed].
- In two-option approvals, Approve/Decline are about 28px, adjacent with `gap-2`, and Approve is a filled gold button; "Add note…" is a 52x17 link [measured `stream-390.png` "Action Required" card]. The prompt card and ApprovalActionCard are nested double borders (T-03). Type scale is inverted: the title is 12px and the command under review is 11px (T-05, `RPC:255-259, 325`).
- Stop sits 4px from Send and stopping has no confirm (T-15/T-16, `CMP:448-489`).

**I7 Composer reachable and protected**
- On every keyboard open/close the focused field is forced to the middle of the screen, the visualViewport offset is ignored, and the host terminal is resized each time (T-38..T-40, `main.tsx:110-117`, `APP:1267-1273`, `RT:770-858`).
- Banners, held messages and the prompt card stack above the composer without a height limit, so the composer can be pushed off screen (T-42, `CMP:250-402`).
- The mic button is shown when voice is unsupported and has an empty aria-label/title (`CMP:449-458`, T-93) [measured `stream-390.png`].

**I8 Plain language**
- The login error prints the provider's raw JSON (`MAIL_FAILED: Resend API error 422 … {"message":…}`), and the card overflows the screen (scrollWidth 400 > 390) [measured `login-error-390.png`; `AccountLoginPage.tsx` setError path].
- Internal codes and session UUIDs reach users ("daemon epoch changed", "STREAM_CLOSED: …"; S-03, `APP:2484-2710, 3256, 4733`). Supervisor toasts use "(CODE)" and "CODE: message" (`SupervisorHome.tsx:49, 200`).
- The phone tells the user to run a CLI command: "Start it on a computer with ferryx hub start" [measured `login-account-390.png`, `home-supervisor-off-390.png`].
- host / machine / computer / worktree / lane are used interchangeably (`MobileHostDrawer.tsx:258,276`, `SupervisorHome.tsx:43`). The drawer's empty state "No selectable desktop worktrees are available." has no next action [measured `drawer-open-390.png`]. "New worktree" uses `window.prompt()` (`APP:4496`, S-11).
- Terminal mode with no running session renders a blank body (S-05, `APP:5348-5369`).

**I9 Tokens, colour and motion are consistent**
- **P0** Pairing errors use `text-destructive-foreground` = #0a0a0a on a `bg-destructive/15` dark red tint, about 1.3:1 and with no `role="alert"` (`PairingPage.tsx:91`, `index.css:59`; S-01) [source, confirmed].
- [measured `contrast-390.json`] Worktree meta 10px #838383 at 3.99:1, "Projects" 12px at 3.40:1, Inbox counts 4.17:1. The Ctrl-C key label is nearly invisible (T-73, `DOCK:142-147`). In light theme "Waiting for input" is success green at about 1.4:1 (T-59, `CC:74-82`).
- The terminal and browser use fixed colours that ignore light theme (T-74..T-76). Hex colours are hard-coded in `.work-shimmer-text` (`CSS:405-416`), plus `bg-white/[0.035]`, `bg-black/50` and similar (section 4).

**I10 No AI-slop tells** — see section 4 (V-01..V-08).

**I11 The terminal is first-class**
- Retry and the tab strip are 24px (T-32..T-34). The key dock's modifier state is unclear (T-45, `DOCK:136-180`). The keyboard resizes the host PTY (T-40). The browser toolbar has no button over 44px (T-35, T-110).

**I12 Restore after an interruption**
- The service worker never caches the JS bundles, so the "offline shell" can't boot offline (S-07, `public/sw.js:5-10, 53-57`).
- The tunnel is not detected as dead after backgrounding (S-02).
- History load and scroll restore are unstable (T-56, `MSG:402-492`, `AC:276-285`).

### 3.2 Full gap list (every lane finding; one line each)
Each line is "ID - severity - file:line :: problem summary". Full text, principles and suggested direction are in the matching `lanes/*.md` file.

#### 3.2.1 Shell, login, pairing, PWA (lanes/code-shell.md, 55 items)
- S-01 — P0 — `PairingPage.tsx:91` :: Problem:** the error banner is `p-3 mb-4 text-xs bg-destructive/15 border border-destructive text-destructive-foreground rounded`.
- S-02 — P0 (verify) — `RemoteApp.tsx:4695-4705`, `2916`, `2955`, `attachTunnel.ts:326,1262` :: Problem:** the active account tunnel is known to be dropped by the relay after about 2 minutes idle (comment at
- S-03 — P1 — `RemoteApp.tsx:2484, 2489, 2503, 2535, 2540, 2595, 2623, 2702, 2709-2710, 3256, 4733` :: Problem:** selection failures go straight into `creationError` and render as user copy:
- S-04 — P1 — `RemoteApp.tsx:4614-4620, 4644-4648`, `useAccountWorktrees.ts:541-556` :: Problem:** the picker failure line renders `${status.errorCode}: ${status.error}` joined with " / ". The result is
- S-05 — P1 — `RemoteApp.tsx:5348-5369` :: Problem:** in terminal mode with `effectiveSessionId == null` (a worktree that has no running terminal, an
- S-06 — P1 — `RemoteApp.tsx:2224-2412` (events socket), `3302-3328`, `3707-3720` :: Problem:** the shell has no connectivity model visible to the user. Socket backoff is silent, `navigator.onLine` is
- S-07 — P1 — `ui/public/sw.js:5-10, 53-57` :: Problem:** the precache holds only `/`, `/index.html`, the manifest and the icon. The cache-first branch
- S-08 — P1 — `RemoteApp.tsx:4559-4580`, `sw.js:99-114` :: Problem:** a notification tap is acted on only when `token` is set (4559). On the account phone home before a
- S-09 — P1 — `home/usePhoneNavigation.ts:31-35`, `RemoteApp.tsx:5374-5482`, `mobile/README.md` ("Android back navigation") :: Problem:** only phone-home tab switches and session entry push history. The following do not:
- S-10 — P1 — `RemoteApp.tsx:4966-5059` (wide header; `REMOTE_DESKTOP_SIDEBAR_QUERY = "(min-width: 768px)"`, `RemoteSessionList.tsx:91`) :: Problem:** at 768px and above the shell switches to a desktop chrome. This includes iPads and large phones in
- S-11 — P1 (verify) — `RemoteApp.tsx:4496` :: Problem:** "New worktree" uses `window.prompt("New worktree slug (letters, numbers and dashes)")`. On mobile it is an
- S-12 — P1 — `PairingPage.tsx:98-122, 129` :: Problem:**
- S-13 — P1 — `ui/index.html:10,14`; `AccountLoginPage.tsx:189`; `PairingPage.tsx:83`; `ApprovePage.tsx:133,162`; `PlanLimitNotice.tsx:80`; `main.tsx:105` :: Problem:** `viewport-fit=cover` plus `apple-mobile-web-app-status-bar-style=black-translucent` makes content draw
- S-14 — P1 — `RemoteApp.tsx:5180-5186, 4710-4716, 2420-2436` :: Problem:** "Activating Selected Worktree / Connecting to X..." has no progress indicator and no cancel or back
- S-15 — P2 — `AccountLoginPage.tsx:90, 96-100, 151, 177-179` :: Problem:** raw `err.message` and `typed.code` are shown. Examples: "Failed to fetch" (Chrome), "Load failed"
- S-16 — P2 — `chat/MobileTopBar.tsx:13-21`; `ui/DESIGN.md` §6 "`1.75rem` (`size-7`) header controls" :: Problem:** the top-bar icon buttons are `size-8` (32px). `HIT_SLOP` extends only vertically
- S-17 — P2 — `RemoteApp.tsx:4636-4660` :: Problem:** picker status texts are `text-[11px]` ("Loading worktrees...", the error, the empty reason). The Retry
- S-18 — P2 — `RemoteApp.tsx:5093-5122`, `relayE2E.ts:12-13` :: Problem:**
- S-19 — P2 — `RemoteApp.tsx:5183-5226` :: Problem:** "Retry Selection" and "Back to Worktrees" are `px-2.5 py-1 text-[11px] font-medium ... rounded`, about
- S-20 — P2 — `PlanLimitNotice.tsx:79-81, 87-130, 151-162` :: Problem:**
- S-21 — P2 — `ApprovePage.tsx:164-205` :: Problem:**
- S-22 — P2 — `AccountLoginPage.tsx:258-279, 102` :: Problem:** the waiting state has no "Resend link", no expiry countdown (it expires silently at 10 minutes, 63-67),
- S-23 — P2 — `ui/index.html:59-64`, `index.css:345-347`, `manifest.webmanifest:7-8`, `mobile/README.md` (Android splash `#23262D`) :: Problem:** four background colors show during launch: splash or manifest `#09090b`, html `#23262d`, remote root
- S-24 — P2 — `main.tsx:101-108` :: Problem:** the boot failure says "Reload the app; if this persists, check the console for details." Phones have no
- S-25 — P2 — `RemoteApp.tsx:1267-1274, 4924`; `main.tsx:112-119`; `index.html:10` :: Problem:** the root height is forced to `visualViewport.height` but ignores `visualViewport.offsetTop`. On iOS
- S-26 — P2 — `components/ui/sonner.tsx:74-84`, `RemoteApp.tsx:4925, 4862-4867, 4940-4944` :: Problem:** toasts sit at `bottom-right` with `mobileOffset.bottom = 16+36` (52px) and no safe-area term. On the
- S-27 — P2 — `RemoteApp.tsx:4559-4580` :: Problem:** if the tapped session no longer exists (ended or on another machine), `alertSessionId` is kept forever
- S-28 — P2 — `RemoteApp.tsx:4811-4833, 5009-5019` :: Problem:** the attention press always focuses `textarea[data-testid="remote-terminal-input-sink"]`, even in chat
- S-29 — P2 — `RemoteApp.tsx:683-701, 965, 987, 4178` :: Problem:** chat copy relies on internal vocabulary:
- S-30 — P2 — `RemoteApp.tsx:412-416, 4801-4809, 4984-5005` :: Problem:** connection labels "Relay (Proxy)", "LAN (Direct)" and "Tailscale (Direct)" are network jargon and say
- S-31 — P2 — `RemoteApp.tsx:5173-5179, 4620-4630` :: Problem:** the pre-selection body reads "Choose a worktree to open its terminal." at `text-xs`.
- S-32 — P2 — `RemoteApp.tsx:1594-1617` :: Problem:** auto-restore of the last target silently waits when its machine is offline or erroring. The user is not
- S-33 — P2 — `RemoteApp.tsx:2049-2054, 1493-1515` :: Problem:** a 401/403 from `/workspace/state` calls `disconnect()`, which clears the token and marks the host
- S-34 — P2 — `main.tsx:121`, `RemoteApp.tsx:5445, 5458-5469` :: Problem:** in the mobile companion the service worker is not registered and no native push plugin exists
- S-35 — P2 — `index.css:203-210` :: Problem:** `body { user-select:none }` applies app-wide, so the remote client inherits desktop-shell behavior. Chat
- S-36 — P2 — `index.css:132` (`--chat-border:#191919`), `index.css:345-361` (`.remote-app-root --border:#191919; --input:#111111`) :: Problem:** `#191919` borders on `#0a0a0a` are about 1.2:1, and `#111111` input borders are about 1.05:1. Input,
- S-37 — P2 — `mobile/src-tauri/capabilities/mobile.json:7-12`, `tauri.conf.json` :: Problem:** the companion has no deep-link or universal-link capability. The desktop QR (`/pair`), approval links
- S-38 — P2 — `RemoteApp.tsx:2895, 5062-5082`; `ui/DESIGN.md` §13 ("paired-host mode are unchanged") :: Problem:** a phone paired by PIN gets a completely different shell than an account phone: no bottom tabs, no
- S-39 — P2 — `RemoteApp.tsx:5075` :: Problem:** the Inbox button in `MobileTopBar` exists only when `model.context.workspaceId` is set. Before a worktree
- S-49 — P2 — `PairingPage.tsx:83-88, 112-122`; `AccountLoginPage.tsx` (no link) :: Problem:**
- S-54 — P2 — `ui/DESIGN.md` §6 (`size-7` header controls), `docs/DESIGN.md` §3 (10-13px compact scale), §8 accepted debt :: Problem:** the design contract itself encodes sub-44px controls and 10-13px text for the phone client, and
- S-40 — P3 — `RemoteDesignOverlay.tsx:120-133, 163-206` :: Problem:** this file is dead code (no importer), but it would be inaccessible if reused:
- S-41 — P3 — Title Case drift :: Problem:** the remote surfaces mix Title Case with sentence case:
- S-42 — P3 — `AccountLoginPage.tsx:208, 286`; `manifest.webmanifest:4`; `index.html:11` :: Problem:** marketing and filler copy: "Access and control your remote machines securely over the encrypted tunnel."
- S-43 — P3 — `index.css:405-420` (`.work-shimmer-text`), `index.css:388-401` (`.pane-inbox-flash`) :: Problem:** `work-shimmer-text` is animated gradient text built from hard-coded `#838383`/`#f5f5f5` with
- S-44 — P3 — `AccountLoginPage.tsx:193, 201, 204`; `ui/DESIGN.md` §10 :: Problem:** the phone login is a floating `rounded-2xl ... shadow-xl` card inside `p-4`, with a 64px icon and a
- S-45 — P3 — `RemoteApp.tsx:4769-4774` :: Problem:** the lazy fallback is a `text-[11px] ... animate-pulse` pill with no `motion-reduce:animate-none`.
- S-46 — P3 — `RemoteApp.tsx:5284-5346`, `1103, 1280, 5072` :: Problem:** browser mode cannot be reached: no UI calls `setViewMode("browser")` and `MobileTopBar` casts the mode
- S-47 — P3 — `RemoteApp.tsx:5034-5058, 4978, 4972` :: Problem:** the Chat/Terminal toggles have no `aria-pressed` and no radiogroup/tablist semantics, so the selection is
- S-48 — P3 — `manifest.webmanifest:2-26`; `index.html` :: Problem:** `"purpose": "any maskable"` on a single icon is discouraged because the maskable crop clips the logo.
- S-50 — P3 — `PlanLimitNotice.tsx:27-29` :: Problem:** `toLocaleString()` renders a full date-time with seconds (for example "11/1/2026, 3:04:05 PM").
- S-51 — P3 — `index.css:128-130` :: Problem:** `--chat-foreground-secondary:#838383` and `--chat-foreground-tertiary:#818181` are visually identical, so
- S-52 — P3 — `settings-runtime.css:153-159` :: Problem:** the density settings change the root `font-size` (16/17px), but most remote text uses arbitrary pixel
- S-53 — P3 — `useScreenWakeLock.ts:50-54` :: Problem:** a refused wake lock (battery saver or permissions policy) is silent, so a "keep screen on" preference
- S-55 — P3 — `sw.js:70-81` :: Problem:** the notification badge uses the full-color `/icon-192.png` (Android requires a monochrome alpha

#### 3.2.2 Phone home: Supervisor, Inbox, Worktrees, drawer (lanes/code-home.md, 72 items)
- H-01 · P0 · Light theme: the typing lane card and the acting session strip are unreadable :: `SupervisorHome.tsx:87`, `:112`; `SupervisorSessionStrip.tsx:54`; tokens `index.css:53` (`--accent: #404040`, never overridden under `[data-theme="light"]`), `index.css:1 **[refuted by measurement, Appendix B]**
- H-02 · P0 · An offline machine with a cached snapshot shows live controls and no offline state :: `SupervisorHome.tsx:243-249` (`snapshot ? null : <MachineStateNote/>`), `usePhoneSupervisor.ts:104`. **[refuted by measurement, Appendix B]**
- H-03 · P0 · The page-level empty state lies while discovery is loading :: `SupervisorHome.tsx:236-240`.
- H-04 · P1 · The headline is computed over readable machines only, so "off" is claimed when nothing is reachable :: `phoneHomeModel.ts:20-31`, `:34`; `SupervisorHome.tsx:210-215`.
- H-05 · P1 · Stacked contradictory empty notices :: `SupervisorHome.tsx:230-234` plus `:249` / `:250-272`.
- H-06 · P1 · The Inbox tab badge counts something different from what the Inbox shows :: `PhoneHomeView.tsx:30` (counts `row.state === "needs-you"` over `inbox.rows`), `remoteInbox.ts:322-341`, `MobileHomeTabBar.tsx:49,61-68`.
- H-07 · P1 · Inbox rows on the phone use desktop-sidebar typography: 10.5-12.5px :: `AttentionInbox.tsx:186` (filter chip `h-6 … text-[11px]`, count `text-[10px]`), `:202` (group header `text-[11px]`), `:205` (`text-[10px]`), `:262` (`text-[12.5px]` who)
- H-08 · P1 · Inbox answer controls are 24px tall :: `AttentionInbox.tsx:186` (`h-6` filter chips), `:421`, `:429`, `:448` (`h-6 … px-2.5` approve/deny/send buttons), `:443` (`h-6` free-text input, `text-[11px]`, which also
- H-09 · P1 · Inbox rows carry a coloured left accent stripe :: `AttentionInbox.tsx:256-257` (`before:w-[3px] before:bg-status-warning|success`), `:254` (`border-white/[0.06] bg-white/[0.035]`).
- H-10 · P1 · The Worktrees tab footer actions are 28px icon-only buttons with tooltips as the only label :: `RemoteSessionList.tsx:669-693` (`h-8` bar), `IconButton.tsx:28` (`size-7` = 28px), `:23` (`title=`).
- H-11 · P1 · OS back closes nothing: sheets and drawers write no history :: `usePhoneNavigation.ts:57-73`; `MobileSheet.tsx:148-155` (Escape only); `RemoteSessionList.tsx:721-738` (drawer).
- H-12 · P1 · Tab switches push history: back walks through tab taps :: `usePhoneNavigation.ts:75-84` (`writeHistory(next, "push")`).
- H-13 · P1 · "Take over" silently continues when Pause fails :: `SupervisorSessionStrip.tsx:41-45`.
- H-14 · P1 · The Session strip "acting" state uses `role="status"` on a container that holds a button and live-updating text :: `SupervisorSessionStrip.tsx:51`, `:61-62`.
- H-15 · P1 · The host drawer is desktop-scale on a phone sheet :: `MobileHostDrawer.tsx:68` (`text-[10px]`, icon `size-2.5`), `:105`, `:113` (`text-[10px] font-mono`), `:247` (`text-[10px]` build stamp), `:281` (`text-xs`), `:313` (`tex
- H-16 · P1 · The host drawer uses hard-coded `white/[0.06]` and the desktop palette inside a chat sheet :: `MobileHostDrawer.tsx:68` (`bg-white/[0.06]`), `:192-230` (`hover:bg-accent`, `text-destructive`, `text-muted-foreground`), `:271-272`, `:297-298` (`bg-accent/70`).
- H-17 · P1 · The account phone home has no host switcher; "Switch host" and "Local Machine" are paired-mode concepts that leak into account mode on the Session screen :: `RemoteApp.tsx:5374-5380` (MobileHostDrawer always mounted, `isAccountSession={Boolean(activeTunnelConnection)}`), `MobileHostDrawer.tsx:258` ("Switch host"), `:276` ("Lo
- H-18 · P1 · Sign out exists in two places on the Worktrees tab, one of them unconfirmed in the thumb zone :: `RemoteSessionList.tsx:670-673` (footer icon, `onSignOut` wired at `RemoteApp.tsx:4895`), `AccountSheet.tsx:103-111`.
- H-19 · P1 · The resume bar and the tab bar compete for the bottom; the resume bar has no safe-area or separation and steals list height :: `PhoneHomeView.tsx:42-58`.
- H-20 · P1 · Worktree rows on the phone: stale and pending rows are disabled with no reason :: `RemoteSidebar.tsx:250` (`disabled={locked || node.stale}`), `SidebarWorktreeRow.tsx:47` (`opacity-60`), `RemoteSidebar.tsx:193-196` (whole list locked while one row is p
- H-21 · P2 · Lane status pill exposes raw hub enums :: `phoneHomeModel.ts:52-53` (`lane.status.replace(/_/g, " ")`), QA fixture `"waiting_on_review"` (`devtools/phoneHomeQaStates.tsx:98`).
- H-22 · P2 · The headline word is a raw machine token: "off", "paused", "typing", "watching 4" :: `SupervisorHome.tsx:211-213`, `phoneHomeModel.ts:17`.
- H-23 · P2 · Lane name concatenates cwd leaf and title; the Pause aria-label uses the title only :: `phoneHomeModel.ts:40-43`, `SupervisorHome.tsx:80`, `:118` vs `:130`.
- H-24 · P2 · "Goal" plus a truncated single line, with "Stopped: <reason>" also truncated; no way to read either in full :: `SupervisorHome.tsx:96-104`.
- H-25 · P2 · Lane cards: two controls with no visual separation; the Pause icon is 16px in a 44px box with no label :: `SupervisorHome.tsx:112`, `:127-137`.
- H-26 · P2 · The "Recent" audit list is a log nobody acts on, rendered as a dense 12px mono grid :: `SupervisorHome.tsx:306-327`.
- H-27 · P2 · Error copy shows raw codes in toasts and notes :: `SupervisorHome.tsx:49` ("(CODE)"), `:178`, `:200` (`${result.code}: ${result.message}`), `SupervisorSessionStrip.tsx:34`.
- H-28 · P2 · Machine state notes are dead ends :: `SupervisorHome.tsx:38-57`.
- H-29 · P2 · Hub-start state: the label stays "Start" while busy, and the CLI fallback puts a terminal command on a phone :: `SupervisorHome.tsx:255-265`, `:268-270`; `phoneHomeModel.ts:34`.
- H-30 · P2 · The header card is a decorative tile :: `SupervisorHome.tsx:207-216`.
- H-31 · P2 · Inconsistent page headers across tabs :: `SupervisorHome.tsx:207-210` (h1 inside a raised card, py-3.5), `InboxHome.tsx:21` (bare h1, `px-1` inside `px-4`, in the scroll), `WorktreesHome.tsx:15-16` (fixed `h-14`
- H-32 · P2 · Scroll ownership on Worktrees: nested scrollers, and sections capped at 40% :: `WorktreesHome.tsx:27` (`overflow-hidden`), `RemoteSidebar.tsx:209` (`max-h-[40%] flex-none` for offline/loading sections), `SidebarView.tsx:203` (inner `overflow-y-auto`
- H-33 · P2 · Worktrees sidebar internals stay at desktop sizes under touch density :: `RemoteSidebar.tsx:97,102,212` (`text-[11px]` status, machine heading and "Not synced" caption), `SidebarStateSlot.tsx:43` (`text-[10px]` state slot even in touch), `Side
- H-34 · P2 · Worktrees tab: the sidebar's own palette (`bg-worktree-sidebar` #2a2a2a) sits inside the chat palette shell (`bg-chat-screen` #0a0a0a) :: `RemoteSessionList.tsx:703`, `WorktreesHome.tsx:14`.
- H-35 · P2 · Tab bar labels at 11px; the badge at 10px; the selected state signalled mostly by a faint pill :: `MobileHomeTabBar.tsx:53` (`text-[11px]`), `:65` (`text-[10px]`, `h-4 min-w-4`), `:58` (`bg-chat-surface-raised` #1a1b1b on `bg-chat-header` ≈#0a0a0a).
- H-36 · P2 · Tab bar keyboard model: Arrow keys select immediately (automatic activation), and Home/End are missing :: `MobileHomeTabBar.tsx:23-31`.
- H-37 · P2 · `pt-safe-header` padding on the home also adds 8px, and the Worktrees header adds its own h-14 :: `PhoneHomeView.tsx:32`, `index.css:255-257`, `WorktreesHome.tsx:15`, `SupervisorHome.tsx:206` (`pt-4`), `InboxHome.tsx:20` (`pt-4`).
- H-38 · P2 · Landscape: no left/right safe area :: `index.css:251-261` (only top/bottom utilities exist), `MobileHomeTabBar.tsx:33`, `PhoneHomeView.tsx:32`.
- H-39 · P2 · Terminal tab strip (Session screen) is entirely sub-touch :: `RemoteTerminalTabStrip.tsx:98-99` (`size-6` step buttons, 24px), `:117` (`h-8` strip), `:165` (`h-6` tabs, 24px), `:182` (`text-[11px]` labels), `:131` (`text-[11px]` or
- H-40 · P2 · The waiting dot uses `ring-2` (accent-ring decoration) and colour alone :: `RemoteTerminalTabStrip.tsx:90` (`ring-2 ring-status-warning/20`), `:93`.
- H-41 · P2 · AccountSheet: Delete account sits directly under Sign out at the same size, with no separation :: `AccountSheet.tsx:103-120`.
- H-42 · P2 · AccountSheet confirm: the placeholder shows the email to type :: `AccountSheet.tsx:138` (`placeholder={email ?? "you@example.com"}`).
- H-43 · P2 · AccountSheet: the confirm step changes the sheet title, but focus doesn't move :: `AccountSheet.tsx:90-91`, `:115`; `MobileSheet.tsx:159-171` (focus set only on open).
- H-44 · P2 · AccountSheet email can be wrong :: `AccountSheet.tsx:9-27`, `:100`.
- H-45 · P2 · MobileSheet backdrop and close: the panel is not swipe-dismissable, though the grabber implies it :: `MobileSheet.tsx:232-234` (drag handle `h-1 w-9`), whole component (no pointer handlers).
- H-46 · P2 · MobileSheet: `bg-black/50` backdrop is a hard-coded colour; the footer sticks inside the scroll container :: `MobileSheet.tsx:217`, `:247-253`.
- H-47 · P2 · The phone home never calls `reset()`; after sign-out/in the screen state can persist through history :: `usePhoneNavigation.ts:102-105`; `RemoteApp.tsx` (no call); `:57-62` resets only when `enabled` flips.
- H-48 · P2 · Opening a session from home gives no immediate feedback :: `RemoteApp.tsx:4837-4841` (`selectAccountOption(...).then(ok => openSession())`).
- H-49 · P2 · Inbox: a non-openable row is discovered only after tapping (toast) :: `RemoteApp.tsx:4858-4867`; `remoteInbox.ts:211` (`readable` known up front).
- H-50 · P2 · Inbox uses `who` = agent type as the row title :: `remoteInbox.ts:99-102`; `AttentionInbox.tsx:262`.
- H-51 · P2 · "Supervisor paused this lane" overrides "Waiting for your answer" :: `remoteInbox.ts:209`, `AttentionInbox.tsx:276-278`, `phoneHomeModel.ts:107-111`.
- H-52 · P2 · Two Inboxes on the Session screen: a sheet and a drawer tab :: `RemoteApp.tsx:5075` (MobileTopBar inbox → sheet), `RemoteSessionList.tsx:661` (the drawer's SidebarViewTabs Inbox, built from `attentionRows`, a *different* row source: 
- H-53 · P2 · The Session-screen drawer exposes a Supervisor tab too, giving three supervisor surfaces :: `RemoteSessionList.tsx:662`, `SidebarView.tsx:187,192`, plus the home Supervisor tab and the Session strip.
- H-54 · P2 · The drawer is `role="dialog"` without `aria-modal`, focus trap or initial focus; there is no swipe-to-close :: `RemoteSessionList.tsx:723-736`.
- H-55 · P2 · "No focused terminal" empty state uses desktop typography and wording :: `RemoteSessionList.tsx:756-772` (`text-sm` h2, `text-xs` body).
- H-56 · P3 · Ellipsis characters and curly quotes mixed with ASCII :: `SupervisorHome.tsx:41` ("Loading…"), `SupervisorSessionStrip.tsx:62` (curly “”), `phoneHomeModel.ts:90`; `InboxHome.tsx:25`.
- H-57 · P3 · Eye icon used for "Supervisor" everywhere: the tab, the header, and three strip states :: `MobileHomeTabBar.tsx:10`, `SupervisorHome.tsx:209`, `SupervisorSessionStrip.tsx:59,87,94`.
- H-58 · P3 · "Left to you. The supervisor would not answer this one." gives no next step :: `SupervisorSessionStrip.tsx:85-91`.
- H-59 · P3 · Held-state copy hard-codes "3s" :: `SupervisorSessionStrip.tsx:71`.
- H-60 · P3 · The strip's "Pause here" vs the lane card's Pause icon vs "Take over": three verbs for one action :: `SupervisorSessionStrip.tsx:100-102`, `:64-66`; `SupervisorHome.tsx:130`.
- H-61 · P3 · "Pause here" button uses secondary-colour text with no fill: weakest affordance in the strip :: `SupervisorSessionStrip.tsx:100` (`text-chat-foreground-secondary`, no bg).
- H-62 · P3 · Tab bar icon `size-[18px]` is an arbitrary value :: `MobileHomeTabBar.tsx:59`; `AccountSheet.tsx:97,118`; `MobileSheet.tsx:284`.
- H-63 · P3 · Hard-coded radii: `rounded-2xl`, `rounded-xl`, `rounded-t-[20px]` :: `SupervisorHome.tsx:207` (2xl), `:53,112,237,251,274` (xl), `InboxHome.tsx:23` (2xl), `PhoneHomeView.tsx:48` (xl), `MobileSheet.tsx:228` (`rounded-t-[20px]`).
- H-64 · P3 · `SECTION_LABEL` machine headings at 12px secondary carry the most important grouping (which computer) :: `SupervisorHome.tsx:36`, `:248`.
- H-65 · P3 · The Recent grid uses fixed `grid-cols-[40px_52px_…]` :: `SupervisorHome.tsx:317`.
- H-66 · P3 · `formatAttentionTime` is used in the Inbox while `relativeTime` (phoneHomeModel) is used in the strip, and the Recent list uses absolute `clock` :: `AttentionInbox.tsx:242`, `phoneHomeModel.ts:56-69`.
- H-67 · P3 · The build stamp is shown to end users in the host drawer footer :: `MobileHostDrawer.tsx:245-250`.
- H-68 · P3 · Transport labels "mDNS", "Tailscale", "SSH", "Relay" are shown as badges :: `MobileHostDrawer.tsx:18-23`.
- H-69 · P3 · The QA harness doesn't cover the states this audit flags :: `devtools/phoneHomeQaStates.tsx:18-29`.
- H-70 · P3 · `WorktreesHome` Account pill: `rounded-full` pill style is used nowhere else on the home :: `WorktreesHome.tsx:21`.
- H-71 · P3 · `PhoneHomeView` `<main>` sits inside a container with `data-testid="mobile-home"` but there is no landmark for the resume bar, which floats between main and nav :: `PhoneHomeView.tsx:33-57`.
- H-72 · P3 · DESIGN §13 bakes in several of the defects above :: `ui/DESIGN.md:430-440` (push on every tab switch → H-12), `:444-451` (`bg-accent` typing fill → H-01; the "Recent" list → H-26; the "watching N" headline → H-22), `:455-4

#### 3.2.3 Chat, approvals, composer, terminal, browser (lanes/code-chat.md, 124 items)
- T-01 - P0 - RPC:362-378, RPC:331 (`space-y-1`) :: Problem: A single tap on any option of a multi-option approval sends the answer immediately. Codex's three-option approval and Claude's "Yes, and don't ask again" both go
- T-02 - P1 - RPC:289-311, CC:520-548 :: Problem: In the two-option approval, Approve and Decline are `px-3 py-1.5 text-xs` (about 28px tall), sit `gap-2` apart, and are right-aligned in a footer. Approve uses a
- T-03 - P1 - RPC:289-310 :: Problem: The two-option path nests an ApprovalActionCard inside the prompt card. Both are `rounded-xl border-status-warning/30 ... backdrop-blur-md p-3.5`. `prompt.title`
- T-04 - P1 - RPC:300-301, CC:528-546 :: Problem: Button labels come straight from option text (`prompt.options[...].label`). Long labels ("Yes, and don't ask again for commands in /path/...") sit in a `shrink-0
- T-05 - P1 - RPC:255-259, CC:488-492, RPC:293, RPC:325 :: Problem: The type scale is inverted. The prompt title is `text-xs` (12px), the question/description is `text-xs` secondary grey, and the command/diff the user must review
- T-06 - P1 - RPC:414, CC:505, CMP:341 :: Problem: Three inputs use `text-xs` (12px): the prompt custom-answer input, the approval feedback textarea, and the held-message edit input. iOS Safari zooms on focus for
- T-07 - P2 - RPC:421-430 :: Problem: Enter in the custom answer sends without `preventDefault()` and without a `press` argument, so focus hand-off logic (`referenceFocusFollowsAnswer`) treats it as 
- T-08 - P2 - RPC:65-68 :: Problem: The queued-prompt copy hard-codes "Codex keeps working meanwhile..." for every agent (Claude, OmO and others).
- T-09 - P2 - RPC:449-455 :: Problem: The typed-answer confirm box is `role="alert"`, so it interrupts the screen reader every time it renders. It also re-runs `scrollIntoView` on every `typedAnswer`
- T-10 - P2 - RPC:235, CC:479, RPC:346, RPC:371, RPC:454 :: Problem: State is expressed through tinted borders: `border-status-warning/30`, `/40` selected rows, and a tinted confirm box. DESIGN.md §10 and §12 forbid tinted or colo
- T-11 - P2 - RPC:235 :: Problem: The whole prompt card is `select-none`, so the user cannot select and copy the question or the option text. Only the `pre` bodies opt back in.
- T-12 - P2 - RPC:355 :: Problem: The multi-select checkbox is `size-3.5` (14px) with `accent-chat-primary`, and the row is `min-h-[32px]`. The label row is clickable, but the visual control is t
- T-13 - P2 - CC:454, CC:518 :: Problem: The legacy approval default title is "Action Required" (Title Case, alarmist) and the default note link reads "Add note...". The note affordance is a `text-[11px
- T-14 - P2 - CC:505, RPC:414 :: Problem: Focus styling uses `focus:ring-status-warning/30` and `focus:border-status-warning/50` instead of the contract `focus-visible:ring-1 ring-ring`.
- T-15 - P1 - CMP:448-489 :: Problem: While a prompt is open, Stop (`bg-chat-danger`, 32px) and Send (32px) sit side by side with `gap-1` (4px). One mis-tap kills the running turn when the user meant
- T-16 - P1 - CMP:467 :: Problem: The Stop icon is `text-chat-foreground` (#f5f5f5) on `bg-chat-danger` (#ff6467). That is about 2.6:1, below the 3:1 minimum for non-text UI. In light theme the d
- T-17 - P2 - OVF:81-90 :: Problem: "Sign out" is a plain sheet row that runs immediately with no confirmation, directly under "Phone alerts". When `onSignOut` exists it also hides "Machines" (OVF:
- T-18 - P1 - RBC:551-592, RBC:573-580 :: Problem: "Advanced JS Eval" runs arbitrary JavaScript with session privileges on a single tap of "Run Eval". The warning is `text-[11px]` amber. The textarea is `text-xs`
- T-19 - P2 - RBC:357-378 :: Problem: "Take Control" is a saturated amber `#ffb900` fill. "Release Control" is neutral and has no confirm. Claiming control from another driver (state `occupied`) show
- T-20 - P1 - TOP:13-17 :: Problem: All top-bar icon buttons are `size-8` (32px). `HIT_SLOP` extends only vertically (`before:inset-x-0 before:-inset-y-1.5`), so the width stays 32px, with 2px gaps
- T-21 - P1 - CMP:443, CMP:455, CMP:467, CMP:481 :: Problem: The composer attach, mic, stop and send buttons are `size-8` (32px). DESIGN §6 itself specifies `size-9` (36px), and both are under 44px. Send is the most-used c
- T-22 - P1 - CMP:309 :: Problem: The remove-staged-file button is `size-5` (20px) with a `size-3` icon.
- T-23 - P1 - CMP:364, CMP:378, CMP:388 :: Problem: The held-message Send/Edit/Remove buttons are `size-6` (24px) and spaced `gap-1`. Remove (Trash) sits next to Edit with no confirm.
- T-24 - P1 - MSG:332-338 :: Problem: The message copy button is `p-0.5` with a `size-3` icon (about 16px), placed right after the timestamp.
- T-25 - P1 - MSG:115-120 :: Problem: The code-block Copy button is `p-1 text-[10px]` (about 20px tall).
- T-26 - P2 - MSG:418-423 :: Problem: The "Worked for ..." toggle is `px-1.5 py-0.5 text-xs` (about 22px).
- T-27 - P2 - CC:278-296 :: Problem: Work rows (tool calls, thinking) are `min-h-[32px]` and are tappable to expand.
- T-28 - P2 - CC:855, CC:886, CC:916, CC:945, CC:971 :: Problem: `<summary>` disclosures (compaction, abandoned branch, notice, skill, task result) are `text-[11px]` single lines of about 16-28px with no padding target.
- T-29 - P2 - CC:1325-1328 :: Problem: The "Earlier messages" button is `py-1 text-[11px]` (about 24px).
- T-30 - P2 - CC:1153 :: Problem: "Show the whole output" is `px-2 py-1 text-[11px]`.
- T-31 - P2 - WS:317, WS:368 :: Problem: The WS header back and terminal buttons are `size-7` (28px). DESIGN §6 codifies 28px header controls.
- T-32 - P1 - RT:1931, RT:1983 :: Problem: The terminal back and options buttons are `size-7` (28px) and have no `focus-visible` ring.
- T-33 - P1 - RT:2135, RT:2140, RT:2145 :: Problem: The "Retry" buttons in the terminal outcome banners are bare `underline` text inside a `text-[10px]` pill (about 14px tall). Retry is the only recovery action wh
- T-34 - P1 - TAB:98-99, TAB:165 :: Problem: Tab-strip step buttons and the new-terminal button are `size-6` (24px). The tabs are `h-6` (24px) with `text-[11px]` labels. Swipe gestures (RT:1636-1641) are th
- T-35 - P1 - RBC:272-301, RBC:319-326, RBC:386-411, RBC:472-498, RBC:505-512, RBC:536-545 :: Problem: Every browser control uses `p-1`, `px-1.5 py-0.5` or `px-2 py-1`, so they range from 20px to 28px tall. The toolbar is a desktop devtool UI shipped to the phone.
- T-36 - P2 - RB:410-418, RBW:339-361 :: Problem: The refusal dismiss "×" is `p-1 text-sm`. The "IME Input" toggle is `px-1.5 py-0.5 text-[11px]`. "← Back to Sessions" is a text link.
- T-37 - P3 - SW:55 :: Problem: The switcher footer "New terminal" (`h-11`) is fine in size but lacks `focus-visible:ring`.
- T-38 - P1 - `main.tsx:110-117` :: Problem: On every `visualViewport` resize (keyboard open/close, URL bar collapse, pinch), the focused input is forced to `scrollIntoView({block:"center"})`. In chat, the 
- T-39 - P1 - APP:1267-1273, APP:4924 :: Problem: Root height follows `visualViewport.height`, but `offsetTop` is ignored. On iOS the layout viewport scrolls when the keyboard opens, so the header and composer c
- T-40 - P1 - RT:852-858, RT:770-846 :: Problem: The terminal geometry comes from a ResizeObserver on the grid. When the soft keyboard opens and the root shrinks to `visualViewport.height`, the rows collapse an
- T-41 - P2 - CMP:247 :: Problem: The composer panel is `rounded-2xl` with only `border-t`. On a phone, rounding the bottom corners against the screen edge plus `pb-safe` creates a visual gap abo
- T-42 - P2 - CMP:250-402 :: Problem: Banners (draft unsaved, warning, attach error), staged files, held messages and the prompt card all stack *above* the textarea inside the composer footer (CMP:25
- T-43 - P2 - WS:476-490 :: Problem: The scroll-to-latest pill is absolutely positioned at `-top-12` relative to the footer. When the footer grows (held messages, prompt card) it still floats correc
- T-44 - P2 - DOCK:114 :: Problem: The dock uses `pb-[env(safe-area-inset-bottom)]` inline while chat uses `.pb-safe` (CSS:251). When the keyboard is open, the iOS inset still applies, so a home-i
- T-45 - P1 - DOCK:136-180 :: Problem: The primary dock row has 13 or more 44px keys (Ctrl, Alt, Esc, Tab, Ctrl-C, 4 arrows, Shift, Enter, More, Direct/Line), about 600px wide, inside `overflow-x-auto
- T-46 - P2 - RT:2289-2330 :: Problem: The line-mode form sits between the grid and the dock. With the keyboard open, three stacked bars (line input, dock, keyboard) leave only a few terminal rows. Th
- T-47 - P2 - RT:763-766 :: Problem: On coarse pointers, the terminal never auto-focuses, so the user must tap the grid. A tap also triggers `focusInput` (RT:1642-1644), but a 40px horizontal drift 
- T-48 - P1 - MSG:139 :: Problem: Fenced code renders at `text-[11px]` with `overflow-x-auto` and no wrap toggle, so long lines require horizontal scrolling inside a vertically scrolling list (di
- T-49 - P2 - CC:776, MSG:69, CC:781 :: Problem: Code renders at 11-12px in some places (rich-path `pre` at `text-[11px]`, inline code at `text-xs`) but prose is `text-base`. Inline code in a 16px sentence drop
- T-50 - P1 - MSG:216-254, MSG:249 :: Problem: The work list is a nested scroller (`max-h-64 overflow-y-auto`) with a mask fade, inside the main scroller, and each expanded tool output is another nested scrol
- T-51 - P2 - CC:344, CC:353, CC:1125, CC:1134 :: Problem: Tool input and output render at `font-mono text-[12px] text-chat-foreground-secondary`, which is grey. Output is often the evidence the user needs.
- T-52 - P2 - CC:399, CC:407, CC:417 :: Problem: Thinking and prose-thinking render in `italic opacity-80` / `italic text-[12px]` grey. Italic plus reduced opacity plus small size is hard to read and drops the 
- T-53 - P2 - CC:259-265 :: Problem: Work-row labels use `truncate leading-none`. The tool command's first line is cut to one line with no way to see it short of expanding, and `leading-none` clips 
- T-54 - P3 - MSG:112 :: Problem: The code language label is `uppercase text-[10px] tracking-wider` tertiary text, which is unreadable decoration.
- T-55 - P2 - CC:985, CC:976 :: Problem: The task-result summary row puts the title (truncate), meta in `text-[10px]` mono, the status word in `text-[10px]` and a chevron on one line. At 375px the meta 
- T-56 - P1 - MSG:402-492, AC:276-285 :: Problem: `role: "system"` messages (every non-assistant, non-tool item, AC:276-285) fall through to the *assistant* rendering, which is unbubbled 16px prose. System and r
- T-57 - P2 - MSG:351-399 :: Problem: There is no accessible role label per turn: no "You" or "Assistant" heading and no `article`/`aria-label`. A screen-reader user hears an undifferentiated stream.
- T-58 - P2 - MSG:484-488, WS:353-359, TOP:139-146 :: Problem: Running state shows in up to three places at once: the TOP dot, the WS header pill (desktop) and a per-message ActivityIndicator. Each ActivityIndicator is its o
- T-59 - P1 - CC:74-82 :: Problem: `waiting_for_input` (the agent needs the user) is coloured `status-success` green with an AlertCircle. The DESIGN status table maps waiting/needs-input to `statu
- T-60 - P2 - CC:57-58, CC:68, CC:76 :: Problem: The copy is "Thinking...", "Running tool...", "Waiting for input..." with three ASCII dots, in `tracking-wide`, inside a pulsing pill with a spinner *and* a ping
- T-61 - P2 - WS:323-324, WS:340-347 :: Problem: The fallback header title is "Agent Workspace". The fallback subtitle shows a hard-coded green `bg-status-success` dot with "ferryx remote" regardless of connect
- T-62 - P2 - WS:95-99, WS:428-430 :: Problem: The starter prompts ("Summarize the current state", "What changed since my last message?", "Run the tests") are generic and fixed regardless of the agent or repo
- T-63 - P3 - MSG:375, MSG:405 :: Problem: The user bubble is `max-w-[88%]` and the assistant is `max-w-[94%]` with `px-5` stream padding (WS:388). That leaves about 300px for code on a 375px phone, which
- T-64 - P3 - AC:204-206 :: Problem: When no timestamps and no durations exist, the label becomes `formatWorkedDuration(0)`, which renders "Worked for 0s". It is a misleading claim.
- T-65 - P3 - AC:186-194 :: Problem: The live "Worked for" duration uses `Date.now()` at derivation time, so it only ticks when a poll re-derives. It appears frozen, then jumps.
- T-66 - P2 - WS:287-291 :: Problem: Auto-follow calls `scrollIntoView({behavior:"smooth"})` on every `messages` change while at the bottom. A poll that replaces the array causes repeated smooth ani
- T-67 - P2 - WS:223-240 :: Problem: `isAtBottom` uses a 48px threshold but the pill appears at more than 100px, which leaves a 48-100px dead zone where the view neither follows nor offers the pill.
- T-68 - P2 - CMP:180-187 :: Problem: Send clears the field immediately, but no optimistic user bubble or "sending" state appears in WS. Until the next poll the message just disappears, and on a held
- T-69 - P3 - MSG:95-107, MSG:312-323 :: Problem: Copy feedback is a 2s icon swap with no live announcement. `aria-label` changes to "Copied" but is not announced on most screen readers.
- T-70 - P2 - RBW:399-418, RBW:166-171 :: Problem: The browser IME bar tracks composition in React *state* (`setIsComposing`). Safari fires `compositionend` before the Enter keydown, so the form submits mid-commi
- T-71 - P3 - CMP:416-431 :: Problem: The composer IME handling is good (CMP:77-85). However, `enterKeyHint="send"` combined with Enter-to-send on mobile means a Korean user who wants a newline must 
- T-72 - P3 - RT:2202-2221 :: Problem: The sink textarea is 1x1px at (0,0). The OS caret and candidate window for Korean/Japanese IME anchor at the top-left of the terminal rather than at the cursor; 
- T-73 - P1 - DOCK:142-147 :: Problem: The Ctrl-C key uses `text-destructive-foreground` (#0a0a0a) on `bg-destructive/20` over the dark card, so near-black text sits on a dark-red tint at about 1.5:1 
- T-74 - P2 - RT:1921, RT:1949-1972, RT:2133-2188, RT:1988-1989 :: Problem: The terminal chrome uses raw colours: `text-[#f5f5f5]`, `bg-[#111111]`, `border-[#191919]`, `text-[#838383]`, and the Tailwind palette `rose-*` / `amber-*` / `ro
- T-75 - P2 - RBC:266-590, RB:367-480, RBW:338-441 :: Problem: The entire browser surface is hard-coded hex (`#111111`, `#191919`, `#346bf1`, `#ffb900`, `#ff6467`, `#4bb8f0`, `#838383`, `#818181`), including `#ffb900`, which
- T-76 - P2 - TAB:99, TAB:166; TOP:234; RT:2367 :: Problem: More raw colours: `bg-white/[0.06]` and `bg-white/[0.08]` (invisible in light theme), `text-black` on the inbox badge, and `bg-white` on the switch thumb.
- T-77 - P2 - CSS:128, CSS:130 (DESIGN §1 table) :: Problem: `--chat-foreground-tertiary` #818181 and `--chat-foreground-secondary` #838383 are visually identical, so the "tertiary" level used for de-emphasis throughout (t
- T-78 - P2 - CSS:132, CSS:146-148 :: Problem: `--chat-border` #191919 on `--chat-screen` #0a0a0a is about 1.1:1, and the composer border is rgba(25,25,25,.8). Cards, starters, code blocks and attachments are
- T-79 - P2 - CSS:409-418 :: Problem: `.work-shimmer-text` hard-codes `#838383` / `#f5f5f5` in its gradient, so in light theme it is a light-grey shimmer on white (unreadable).
- T-80 - P2 - CMP:484 :: Problem: The disabled Send button uses `text-chat-foreground-secondary/50` on `bg-chat-surface-raised/40`, giving about 2:1. The disabled state is acceptable at low contr
- T-81 - P3 - MSG:110, CC:136-182, CC:1058, CC:1076 :: Problem: Opacity-suffixed surfaces (`bg-chat-screen/90`, `bg-chat-surface/60`, `border-chat-foreground/10`) and decorative `shadow-xs` / `shadow-sm` / `shadow-md` / `shad
- T-82 - P2 - RT:1960, RT:2184, RT:2264 :: Problem: `animate-pulse` on the buffered-input dots and on the terminal cursor has no `motion-reduce:animate-none`. DESIGN §5 states that looping motion must be motion-re
- T-83 - P3 - CMP:467, CMP:481, WS:482, DOCK:245, RT:2325 :: Problem: `active:scale-95 transition-all` on Send, Stop, the pill, every dock key and line-send. `transition-all` animates layout properties, and there is no reduced-moti
- T-84 - P3 - MSG:427, RT:2367 :: Problem: The chevron rotation (`transition-transform duration-200`) and the switch thumb `transition-transform` have no `motion-reduce` guard.
- T-85 - P3 - RB:381 :: Problem: The refusal banner uses `animate-in fade-in slide-in-from-top-2` with no `motion-reduce` guard.
- T-86 - P2 - SH:148-155 :: Problem: Escape closes the sheet through a window *capture* listener. If two sheets are open (for example the terminal options sheet over the overflow sheet), one Escape 
- T-87 - P2 - SH:205-258 :: Problem: The sheet does not lock body scroll and does not make the background `inert`. Screen-reader virtual cursors can wander behind `aria-modal`, and on iOS the page b
- T-88 - P2 - SH:212-218, SH:237-245 :: Problem: The backdrop is a full-screen `<button>` with the same accessible name as the close button ("Close X"), so a touch screen-reader user meets two identical "Close"
- T-89 - P2 - TAB:148-186 :: Problem: `role="tablist"` / `role="tab"` without `tabpanel` / `aria-controls` and without arrow-key roving (DESIGN SegmentedControl requires roving). Tabs also set both `
- T-90 - P2 - TOP:156-163 :: Problem: `role="group"` on a text span exists only so that it can carry an `aria-label`. That misuses a landmark-like role for static text. TOP:291 adds a stray sr-only "
- T-91 - P2 - CC:115, CMP:445, CMP:457, CMP:469, CMP:487, MSG:133, MSG:345, WS:487, CC:484 :: Problem: Lucide icons inside labelled buttons are missing `aria-hidden`, so some screen readers announce "image" twice.
- T-92 - P2 - CMP:361, CMP:375, CMP:385 :: Problem: Held-message button labels concatenate the *full message text* ("Send held message <entire paragraph>"), so long messages produce extremely verbose names.
- T-93 - P2 - CMP:449-458 :: Problem: The mic button is permanently `disabled`, with the label "Voice input is not supported". It is a dead control occupying prime composer space, and disabled button
- T-94 - P3 - CMP:118, CMP:423 :: Problem: The placeholder says "Ask the agent…" but the accessible name is "Ask the repo agent". During a prompt the placeholder changes (APP:4490) while the label stays t
- T-95 - P2 - RB:439-447, RBW:437-443 :: Problem: Browser status text (connecting, paused, disconnected) has no `role="status"`. The driver error banner has no `role="alert"`. The reconnect overlay (RB:468-481) 
- T-96 - P2 - RBC:272-301, RBC:36-40, RBC:543 :: Problem: Navigation uses Unicode glyph icons (←, →, ↻, ⌫, ×) as visible labels. The keys row buttons (⌫, arrows) have no `aria-label`, so they announce as "leftwards arro
- T-97 - P3 - DOCK:169-178 :: Problem: The input-mode toggle has `aria-label="Line input mode"` with `aria-pressed`, but its visible text alternates "Direct"/"Line", so the visible and accessible name
- T-98 - P3 - DOCK:186-205 :: Problem: The locked modifier is shown by `underline` plus `ring-2 ring-primary/60 ring-offset-1`, which is a ring-accent outline. The one-shot and locked states are hard 
- T-99 - P2 - DOCK:233-253, RT:1931, RT:1983, RT:2318, RBC:all buttons :: Problem: These controls have no `focus-visible:ring`. The dock keys and RBC inputs use `focus:border-[#346bf1]` only.
- T-100 - P1 - RT:2009-2016, RT:2128 :: Problem: The grid sets `touchAction: none` and handles touch itself. There is no way to select or copy terminal output on a phone (no long-press, no select mode), and no 
- T-101 - P2 - RT:1569-1578 :: Problem: Pinch changes the font size live with no on-screen size indicator and no reset. The minimum size of 10px (RT:92) is unreadable on a phone. Each step changes geom
- T-102 - P2 - RT:1636-1641 :: Problem: A horizontal swipe of 40px or more switches terminal tabs silently, with no animation or toast. When `followHostSize` uses `pan-x`, horizontal scrolling of a wid
- T-103 - P2 - RT:1922, RT:1988-1997 :: Problem: With `showTitleBar=false` on phones (APP:5351), the terminal header shrinks to an `h-7` strip showing the build stamp (`text-[10px]`, `#838383/70`) and "Live" in
- T-104 - P2 - RT:1946-1972 vs RT:2130-2188 :: Problem: The overflow, buffered and output-gap indicators render twice when embedded (header plus overlay) and are inconsistent in shape (rounded vs rounded-full). The ov
- T-105 - P2 - RT:2134 :: Problem: The outcome text shows raw machine strings: `terminalOutcome.state` (for example "affirmative_exit" in the fallback branch at RT:2144) and "Recovery outcome unkn
- T-106 - P2 - RT:1974, RT:2340-2370 :: Problem: The terminal options menu (Keep screen on) exists only when the Wake Lock API is supported. On iOS versions without it there is no options button at all, so futu
- T-107 - P3 - DOCK:84-92 :: Problem: `navigator.vibrate(15)` fires on every key press and modifier tap, with no setting. It is Android-only and the iOS gap is silent, which makes feedback inconsiste
- T-108 - P3 - TAB:130-135 :: Problem: The ordinal "? / N" appears when the active tab is not a pane, which is a cryptic glyph.
- T-109 - P3 - DOCK:114, DOCK:245-250 :: Problem: The dock uses desktop tokens (`bg-card`, `border-border`, `bg-primary`, `bg-secondary`) while the rest of the phone UI uses `chat-*` tokens, so the dock's active
- T-110 - P1 - RBC:263-596 :: Problem: The phone gets a desktop devtools toolbar: two wrapping rows of controls (nav, URL, badge, claim, Ref, Fill, Point Click, 9 keys, custom key, eval toggle) with a
- T-111 - P1 - RB:421-466 :: Problem: The viewport is a scaled `object-contain` screenshot with `cursor-crosshair`, and taps become point-clicks. There is no zoom or pan, no tap-position feedback (no
- T-112 - P2 - RBC:306-317, RBC:484-491, RBW:392-423 :: Problem: The URL input lacks `inputMode="url"`, `autoCapitalize="none"`, `autoCorrect="off"` and `enterKeyHint="go"`, so iOS capitalises the first letter of URLs. The cus
- T-113 - P2 - RBC:331-354, RBC:519-534, RB:384-388 :: Problem: Status and refusal codes are shown as UPPERCASE `tracking-wider` badges with raw codes (`BROWSER_INVALID_REFERENCE`, `LETTERBOX_MARGIN`, `REFUSED`). This is deve
- T-114 - P2 - RBC:420-466 :: Problem: "Point Click" is shown struck-through with an "Unsupported" or "Refused" pill. A permanently struck-through feature on the main toolbar is noise.
- T-115 - P3 - RBW:338-348 :: Problem: The header shows the raw `browserId` in mono, plus "← Back to Sessions" (glyph arrow) and "IME Input" (jargon).
- T-116 - P3 - CC:208 :: Problem: The `Sparkles` icon is used for task/agent/workpool tool calls. That is a cliché "AI" glyph.
- T-117 - P3 - CC:483-485, RPC:250-252 :: Problem: A warning icon in a tinted, bordered tile (`p-1 rounded-lg bg-status-warning/10 border border-status-warning/20`) is a typical template "alert tile".
- T-118 - P3 - CC:339, CC:414, CC:1120, MSG:204 :: Problem: Expanded tool and thinking panels and blockquotes use a left border rail (`ml-7 border-l pl-3`, `border-l-2`), a side-stripe accent pattern. It is acceptable for
- T-119 - P3 - WS:308, CC:93, CMP:247, RPC:235, CC:479 :: Problem: `backdrop-blur-*` is used on opaque or near-opaque surfaces (header `bg-chat-screen` is fully opaque). It does nothing visually, costs GPU on phones, and DESIGN 
- T-120 - P3 - TOP:98, TOP:185 :: Problem: The "Island" pills (`rounded-full bg-chat-surface-raised shadow-sm p-[7px]`) were ported from paseo. Two floating capsules with `shadow-sm` on a dark header are 
- T-121 - P3 - MSG:381, WS:388 :: Problem: The user bubble is 16px while the stream is `select-text` but the root is `select-none` (WS:301). Selection in metaRow, the starters and the composer chrome is b
- T-122 - P2 - `ui/DESIGN.md` §6 "Mobile chat surface constants" :: Problem: The contract itself codifies sub-44px targets (`size-9` composer controls, `size-7` header controls), and the code then undershoots even those (`size-8` composer
- T-123 - P2 - QA:1, QA:257-397 :: Problem: The QA harness has no seeded state for: an open ReferencePromptCard (any kind), held messages, staged attachments or an attach error, draft unsaved, a composer w
- T-124 - P3 - QA:206, WS:460, MSG:478-482 :: Problem: `approvalAction` (the legacy ApprovalActionCard per message) is only produced by the harness. The production approval path is the ReferencePromptCard in the comp

---
## 4. Violations against impeccable and the frontend skill (including AI-slop tells)

### 4.1 Automated tools (run on noye at revision 1f1e1fde)
| Tool | Command | Result |
|---|---|---|
| frontend `check-draft-guard.ts` | `bun check-draft-guard.ts src/remote src/features src/components/MobileKeyDock.tsx src/components/ui src/index.css src/settings-runtime.css` | **exit 1, 3 findings** (234 files) |
| frontend `check-accent-stripes.ts` | same targets (src/remote, …) | **exit 1, 2 findings** |
| impeccable `detect.mjs --json` | `src/remote src/index.css src/settings-runtime.css src/components/MobileKeyDock.tsx` | **exit 2, 3 findings** |
| impeccable `detect.mjs --json` | `src/features src/components/ui` | 0 findings |

Each finding:
- **V-01 accent-outline (decorative ring)**: `ring-2 ring-status-warning/20`. Locations: `ui/src/remote/RemoteApp.tsx:5023`, `ui/src/remote/RemoteTerminalTabStrip.tsx:90`, `ui/src/components/ui/StatusDot.tsx:40`. The draft guard forbids it as an accent outline. Use a shape difference instead (hollow vs filled) plus text.
- **V-02 gradient-text**: the `.work-shimmer-text` rule in `ui/src/index.css:405-416` uses `linear-gradient(...)` + `background-clip:text` + `color:transparent`, with **hard-coded hex values (#838383/#f5f5f5)**. It is a "Working…" shimmer. This hits the impeccable ban on gradient text and the token rule. Under light theme the same hex values are used, so the contrast intent breaks there too [source].
- **V-03 overused-font**: Geist (`ui/src/index.css:4`, `:208`) is one of the overused fonts in impeccable's list. It is the existing brand font, so this is a judgment call (the brief wins), but there is no evidence the phone surface was given its own type decision.

### 4.2 AI-slop tells the lints missed (found by me)
- **V-04 coloured side stripe on cards**: `ui/src/features/ferryx/attention/AttentionInbox.tsx:256` `before:absolute before:left-0 before:top-2 before:bottom-2 before:w-[3px] before:bg-status-warning|success`. [measured] it shows as the yellow/green bar on the left of each Inbox card in `shots/home-inbox-390.png` and `home-inbox-light-390.png`. The lint has a `pseudo-bar` rule but **missed it because the class string is assembled by composition** (lint blind spot). Status is shown three times: stripe + pill + group dot.
- **V-05 decorative header tile**: the Supervisor header card (`ui/src/remote/home/SupervisorHome.tsx:207-216`) has no action, and its h1 repeats the tab name ("stat/decor tile nobody acts on"). [measured] `home-supervisor-390.png`
- **V-06 badge with unclear meaning**: the top-right lowercase enum word "watching 5" / "off" / "typing" (`SupervisorHome.tsx:211-213`). The unit doesn't match anything visible.
- **V-07 filler copy**: the login subtitle "Access and control your remote machines securely over the encrypted tunnel." and the footer "Ferryx Remote Access" (`ui/src/remote/AccountLoginPage.tsx`) [measured `login-390.png`]. They are promotional or descriptive copy that doesn't help sign-in.
- **V-08 one fact in several places**: Inbox stripe/pill/dot (above). The Supervisor "off" state shows the header sentence, the page-level note and the per-computer CLI hint (`SupervisorHome.tsx:34, 230-234, 268-270`; [measured] `home-supervisor-off-390.png` and `home-supervisor-unsupported-390.png` show "Start it on a computer with ferryx hub start" and "None of your computers reports…" together).

### 4.3 Rule-by-rule violations (frontend R-ids, impeccable I/N-ids)
| Rule | Content | Violation evidence |
|---|---|---|
| R-13..20 hit targets / impeccable N "44pt" | 44x44 | [measured `dom-metrics-390.json`] top bar icons 32x32 (5), terminal tab strip 24px high (8), prev/next 24x24, copy message 16x16, Copy code 48x24, Approve/Decline 30px high, "Add note..." 52x17, composer attach/mic/send 32x32, Inbox filters 24px, project rows 29px, drawer close/new 28x28, Worktrees footer 28x28 |
| R-119..132 mobile / I "tiny text" | body ≥16, nothing <12 | [measured] tab strip, status slots, metadata, file sizes and timestamps at 10-11px; Inbox meta 10.5px; tab bar labels 11px |
| impeccable N "input ≥16px" | prevents iOS zoom | [measured] `Filter worktrees` input **14px** (`home-worktrees`, `sidebar-parity`); [source] Inbox inline answer input `h-6 text-[11px]` (`AttentionInbox.tsx:443`) |
| R-149..162 a11y / U3 contrast | 4.5:1 | [measured `contrast-390.json`] worktree meta 10px #838383 ≈ **3.99:1**, "Projects" **3.40:1**, Inbox counts 4.17:1 |
| U5 / impeccable "text scaling" | 200% reflow | [measured `shots/*-text200-390.png`] the session screen overflows sideways (scrollWidth 468 > 390); top bar icons clipped at the right edge; `text-[Npx]` fixed-px text doesn't scale, so hierarchy breaks |
| R-21..35 states | empty / loading / error / offline | (correction: the `[terminal grid placeholder (socket offline)]` string in `terminal-mode-390.png` is a QA harness placeholder (`ui/src/devtools/MobileChatQaHarness.tsx:349`), not product copy, so it is excluded from the findings); the drawer shows only "No selectable desktop worktrees are available." in 11px with no next action (`drawer-open-390.png`); the login error shows the provider's raw JSON (`login-error-390.png`) |
| R-93..104 copy / impeccable clarify | no raw codes | [measured] `MAIL_FAILED: Resend API error 422 Unprocessable Entity: {"message":…}` printed to the user as-is; [source] `SupervisorHome.tsx:49,200` "(CODE)", "CODE: message" toasts |
| R-105..118 scroll ownership | one scroller | [source] Worktrees has a scroller per computer section plus `max-h-[40%]` (`RemoteSidebar.tsx:209`, `SidebarView.tsx:203`) |
| R-57..68 surface/token | tokens only | [source] `bg-white/[0.035]`, `border-white/[0.06]` (`AttentionInbox.tsx:254`), `bg-black/50` (`MobileSheet.tsx:217`), `bg-white/[0.06]` (`MobileHostDrawer.tsx:68`), the `.work-shimmer-text` hex values |
| R-43..56 motion | caused, compositor-only, reduced-motion | the shimmer is guarded by `prefers-reduced-motion: no-preference` (OK). [source] the bottom sheet grabber has no drag-dismiss, a false affordance (`MobileSheet.tsx:232-234`) |
| impeccable "label = name" (WCAG 2.5.3) | visible label is included in the accessible name | [source] resume bar shows "Open session" but its name is "Back to X" (`PhoneHomeView.tsx:42-57`) |
| impeccable critique: Nielsen H2/H4 | user's language, consistency | the terms host / machine / computer / worktree / lane are mixed (`MobileHostDrawer.tsx:258,276`, `SupervisorHome.tsx:43`) |
| designpowers R-166/170 | Casey (one hand) | the main actions (worktree switching, session switching, more menu) sit in the 32px top bar, outside the thumb zone [measured `stream-390.png`] |

### 4.4 Nielsen 10 scores (impeccable critique rubric; based on screenshots + code)
| # | Heuristic | Score | Reason |
|---|---|---|---|
| H1 | System status | 2 | The working state and the session strip are good. "No computers are linked" during loading (`SupervisorHome.tsx:236`) and the raw placeholder in the terminal are not |
| H2 | Real-world match | 1 | `MAIL_FAILED` JSON, `ferryx hub start`, "watching 5", "lane", "Local Machine", session-id prefixes |
| H3 | Control and freedom | 2 | Back gestures step through tab history; sheets don't close on back; no undo |
| H4 | Consistency | 2 | Three different header treatments across the 3 tabs; the Worktrees tab uses the desktop sidebar palette; three Inbox surfaces |
| H5 | Error prevention | 3 | Account deletion requires typing the email (though the placeholder shows it); Approve/Decline sit next to each other at 30px |
| H6 | Recognition over recall | 2 | Icon-only buttons in the top bar and footer; `title` tooltips don't work on touch |
| H7 | Flexibility and efficiency | 3 | Session strip, resume bar, quick prompts |
| H8 | Minimal design | 2 | Decorative header card, Recent log, duplicated statuses |
| H9 | Error recovery | 1 | No retry; raw codes; toasts only |
| H10 | Help | 2 | Some empty states explain themselves, but there is no next-action button |
| | **Total** | **20/40 (Acceptable, low end)** | |

---

## 5. Screenshot evidence (taken myself, viewed)

Generation: the QA harness (`mobile-chat-qa.html`) and the production entry point (`index.html`) at revision 1f1e1fde were built on noye with `vite build`, served locally with Bun.serve, and captured with Bun.WebView (WebKit).

| Surface | Files (`shots/`) |
|---|---|
| Login (real RemoteApp first screen) | `login-390.png`, `login-375.png`, `login-error-390.png`, sheet `sheet-login.png` |
| Phone home: Supervisor | `home-supervisor-390.png`, `-acting-`, `-off-`, `-unsupported-`, `-long-` (390), `home-supervisor-375.png`, `-light-390`, `-acting-light-390`, `-text200-390` |
| Phone home: Inbox | `home-inbox-390.png`, `-375`, `-light-390`, `-text200-390` |
| Phone home: Worktrees / account deletion | `home-worktrees-390.png`, `-375`, `-light-390`, `-text200-390`, `account-delete-390.png` |
| Chat | `stream-390.png`, `-375`, `running-`, `long-`, `empty-`, `attachments-`, `topbar-attention-`, `topbar-long-title-`, `stream-light-390`, `stream-text200-390` |
| Sheets / menus | `switcher-open-390.png`, `menu-open-390.png`, `-375` |
| Drawer | `drawer-open-390.png`, `-375`, `sidebar-parity-390.png`, `sidebar-parity-inventory-390.png` |
| Terminal | `terminal-mode-390.png`, `-375` |
| Session strip | `session-strip-acting-390.png`, `session-strip-paused-390.png`, `session-strip-acting-light-390.png` |
| Contact sheets | `sheet-chat1/2/3.png`, `sheet-home1/2/3.png`, `sheet-light.png`, `sheet-light-accent.png`, `sheet-text200.png`, `sheet-login.png` |

Measurement data: `dom-metrics-390.json` (hit targets, text under 12px, input font sizes), `contrast-390.json` (below 4.5:1). Horizontal overflow: none on any harness state at 390 or 375 (scrollWidth equals the viewport). Overflow only in `login-error-390` (400 > 390) and in the 200% chat session (468 > 390).

---

## Appendix A. Evidence files
Root: `/Volumes/T9-Mac/project/ferryx-wt/.evidence/mobile-uiux-review-20261101/`
- `lanes/hig-foundations.md` (F-01..F-49, 18 Foundations pages), `lanes/hig-patterns.md` (P-01..P-22, 25 Patterns pages), `lanes/hig-components.md` (C-01..C-24, 79 Components pages): full HIG reading lanes
- `lanes/impeccable-rules.md` (I-01..I-92, N-01..N-26, critique rubric, detector usage), `lanes/frontend-refs.md` (R-01..R-176; design README, redesign, taste, layout, interaction, designpowers, ui-ux-db, perfection): rule extraction
- `lanes/code-home.md` (H-01..H-72), `lanes/code-shell.md` (S-01..S-55), `lanes/code-chat.md` (T-01..T-124): code audits
- `shots/` screenshots, `dom-metrics-390.json`, `contrast-390.json`

Tally: 251 code findings. P0: 6 raised, 2 refuted (H-01, H-02), so 4 live P0 (T-01, S-01, S-02, H-03); see each lane file for P1-P3 counts + V-01..V-08 + measured items.

## Appendix B. Lane findings refuted or corrected by measurement
- **H-01 (light accent unreadable) refuted**: in light theme `bg-accent` renders as rgb(224,227,232) with #171717 text, which is readable [measured `home-supervisor-acting-light-390.png`, `session-strip-acting-light-390.png`]. The `index.css:53` value is overridden by the `settings-runtime.css` light/accent rules.
- **H-02 (offline machine shows stale live lanes) refuted**: `remoteSupervisor.ts:239-273` nulls the snapshot when the machine goes offline, and `usePhoneSupervisor.ts:66-75` marks lastSeen as disconnected.
- **Terminal placeholder string**: `[terminal grid placeholder (socket offline)]` in `terminal-mode-390.png` is QA harness text (`devtools/MobileChatQaHarness.tsx:349`), so it is excluded from product findings. The real terminal's no-session/empty states are S-05 and T-100.
- Spot checks confirmed in source: S-01 (`PairingPage.tsx:91` + `index.css:59`), S-11 (`RemoteApp.tsx:4496` `window.prompt`), T-01 (`ReferencePromptCard.tsx:331, 362-378`), H-03 (`SupervisorHome.tsx:236`).

## Appendix C. Side effect during this work
- While capturing the login error screenshot, the QA page's account origin fell back to the default (`relay.ferryx.dev`) and sent one magic-link request for `dev@example.com`. Resend rejected it with 422, so no email was sent and no account was created.


## Fix ledger (appended 2026-11-01)

All 259 findings are accounted for: 254 fixed, 3 deferred, 2 refuted. Commits are on `fix/mobile-uiux-20261101` (head cd4f96e7), merged into local main. Lane ledgers with per-finding notes are in `ferryx-wt/.evidence/mobile-uiux-review-20261101/fix-ledger-*.md`.

### Not fixed

| ID | Status | Reason |
|---|---|---|
| H-01 | refuted | `settings-runtime.css` sets a light `--accent` (#e0e3e8 with dark text), matching the report's Appendix B. The lane card's folder line now uses `chat-foreground` to address your 4.01:1 contrast measurement (b8394d71). |
| H-02 | refuted | `remoteSupervisor` nulls the snapshot when a computer is offline or errors. A QA state `home-offline-cached` now shows this. |
| S-37 | deferred | No deep-link plugin is a dependency (checked `mobile/src-tauri` Cargo, package and tauri.conf); I added in-app "Pair with a code instead" (S-49) as the in-app entry |
| S-38 | deferred | Phone home needs account discovery and the supervisor feed, which paired mode lacks; the home views and navigation are lane A's. Back now closes overlays in paired mode too (S-09) |
| T-72 | deferred | Needs real-device IME QA. b279339f deliberately pinned a stationary 1x1 sink after iOS/Android IME cancellation and layout scroll bugs; moving it to the cursor cell changes Korean/Japanese input on hardware. This run has no device and no IME test rig; jsdom cannot show candidate-window placement. The preedit overlay already follows the cursor. |

### Fixed

| ID | Commit(s) |
|---|---|
| H-03 | c262af21 |
| H-04 | c262af21 |
| H-05 | c262af21 |
| H-06 | 960f6f88 |
| H-07 | 960f6f88 |
| H-08 | 960f6f88 |
| H-09 | 960f6f88 |
| H-10 | 21b62cc4 |
| H-11 | 960f6f88, 21b62cc4 |
| H-12 | 960f6f88 |
| H-13 | c262af21 |
| H-14 | c262af21 |
| H-15 | 21b62cc4 |
| H-16 | 21b62cc4 |
| H-17 | d27ae0e4 |
| H-18 | 21b62cc4 |
| H-19 | 960f6f88 |
| H-20 | 21b62cc4 |
| H-21 | c262af21 |
| H-22 | c262af21 |
| H-23 | c262af21 |
| H-24 | c262af21 |
| H-25 | c262af21 |
| H-26 | c262af21 |
| H-27 | c262af21 |
| H-28 | c262af21 |
| H-29 | c262af21 |
| H-30 | c262af21 |
| H-31 | 960f6f88 |
| H-32 | 21b62cc4 |
| H-33 | 21b62cc4, b8394d71 |
| H-34 | 21b62cc4 |
| H-35 | 960f6f88 |
| H-36 | 960f6f88 |
| H-37 | 960f6f88 |
| H-38 | d1e57db5 |
| H-39 | 320d268f |
| H-40 | 0f76db93 |
| H-41 | 960f6f88 |
| H-42 | 960f6f88 |
| H-43 | 960f6f88 |
| H-44 | 960f6f88 |
| H-45 | dbe17c58 |
| H-46 | dbe17c58 |
| H-47 | 502e2c6b |
| H-48 | 99cef84a, fade6ef3 |
| H-49 | 99cef84a |
| H-50 | 960f6f88 |
| H-51 | 960f6f88 |
| H-52 | d27ae0e4, 21b62cc4 |
| H-53 | 21b62cc4 |
| H-54 | 21b62cc4 |
| H-55 | 21b62cc4 |
| H-56 | c262af21 |
| H-57 | c262af21, 960f6f88 |
| H-58 | c262af21 |
| H-59 | c262af21 |
| H-60 | c262af21 |
| H-61 | c262af21 |
| H-62 | 960f6f88 |
| H-63 | c262af21, 960f6f88 |
| H-64 | c262af21 |
| H-65 | c262af21 |
| H-66 | c262af21 |
| H-67 | 21b62cc4 |
| H-68 | 21b62cc4 |
| H-69 | 83320434 |
| H-70 | 960f6f88 |
| H-71 | 960f6f88 |
| H-72 | 8af7b282 |
| S-01 | d33337d7 |
| S-02 | a8993f69 |
| S-03 | 99cef84a |
| S-04 | 99cef84a |
| S-05 | 5cd427ca |
| S-06 | a8993f69 |
| S-07 | 189eab44 |
| S-08 | 5cd427ca |
| S-09 | 502e2c6b |
| S-10 | f22a740c |
| S-11 | 5cd427ca |
| S-12 | d33337d7 |
| S-13 | d33337d7, 189eab44 |
| S-14 | 99cef84a |
| S-15 | d33337d7 |
| S-16 | e2578549 |
| S-17 | 99cef84a |
| S-18 | a8993f69 |
| S-19 | 99cef84a |
| S-20 | d27ae0e4 |
| S-21 | d33337d7 |
| S-22 | d33337d7 |
| S-23 | 97b6a681 |
| S-24 | 189eab44 |
| S-25 | 189eab44 |
| S-26 | 57819077 |
| S-27 | 5cd427ca |
| S-28 | 5cd427ca |
| S-29 | 997e6e01 |
| S-30 | a8993f69 |
| S-31 | 5cd427ca |
| S-32 | d27ae0e4 |
| S-33 | d33337d7 |
| S-34 | f22a740c |
| S-35 | d1e57db5 |
| S-36 | d1e57db5 |
| S-39 | d27ae0e4 |
| S-40 | d27ae0e4 |
| S-41 | d492577b, dbe17c58, 2a58f430 |
| S-42 | d33337d7, 189eab44 |
| S-43 | d1e57db5 |
| S-44 | d33337d7 |
| S-45 | 5cd427ca |
| S-46 | f22a740c, 430ae34c |
| S-47 | 7d88ced7 |
| S-48 | 189eab44 |
| S-49 | d33337d7 |
| S-50 | d27ae0e4 |
| S-51 | d1e57db5 |
| S-52 | 2c0fdf6c |
| S-53 | d27ae0e4 |
| S-54 | 8af7b282, e1564866 |
| S-55 | 189eab44 |
| T-01 | d492577b |
| T-02 | d492577b |
| T-03 | d492577b |
| T-04 | d492577b |
| T-05 | d492577b |
| T-06 | d492577b, 4b5c1799 |
| T-07 | d492577b |
| T-08 | d492577b |
| T-09 | d492577b |
| T-10 | d492577b |
| T-11 | d492577b |
| T-12 | d492577b |
| T-13 | d492577b |
| T-14 | d492577b |
| T-15 | 4b5c1799 |
| T-16 | 4b5c1799 |
| T-17 | dbe17c58 |
| T-18 | d7c9dd5d |
| T-19 | d7c9dd5d |
| T-20 | e2578549 |
| T-21 | 4b5c1799 |
| T-22 | 4b5c1799 |
| T-23 | 97b6a681 |
| T-24 | c391bf78 |
| T-25 | c391bf78 |
| T-26 | c391bf78 |
| T-27 | c391bf78 |
| T-28 | c391bf78 |
| T-29 | c391bf78 |
| T-30 | c391bf78 |
| T-31 | cd4d2683 |
| T-32 | 0df5119d |
| T-33 | 0df5119d |
| T-34 | 320d268f |
| T-35 | d7c9dd5d |
| T-36 | d7c9dd5d |
| T-37 | dbe17c58 |
| T-38 | 189eab44 |
| T-39 | 189eab44 |
| T-40 | 0df5119d |
| T-41 | 4b5c1799 |
| T-42 | 5dfd9f3c |
| T-43 | 2a58f430 |
| T-44 | e0b4b6d6 / 0df5119d |
| T-45 | e0b4b6d6 |
| T-46 | 0df5119d |
| T-47 | 0df5119d |
| T-48 | c391bf78 |
| T-49 | 8cd93eb3 |
| T-50 | c391bf78 |
| T-51 | c391bf78 |
| T-52 | c391bf78 |
| T-53 | c391bf78 |
| T-54 | c391bf78 |
| T-55 | c391bf78 |
| T-56 | c391bf78 |
| T-57 | c391bf78 |
| T-58 | c391bf78 |
| T-59 | c391bf78, d1e57db5 |
| T-60 | c391bf78 |
| T-61 | 2a58f430 |
| T-62 | fc50b62a |
| T-63 | 2a58f430 |
| T-64 | ded14e41 |
| T-65 | ded14e41 |
| T-66 | 2a58f430 |
| T-67 | 2a58f430 |
| T-68 | ded14e41 |
| T-69 | c391bf78 |
| T-70 | d7c9dd5d |
| T-71 | 4b5c1799 |
| T-73 | e0b4b6d6 |
| T-74 | 0df5119d |
| T-75 | d7c9dd5d |
| T-76 | 320d268f / 0df5119d |
| T-77 | d1e57db5 |
| T-78 | d1e57db5 |
| T-79 | d1e57db5 |
| T-80 | 4b5c1799 |
| T-81 | c391bf78 |
| T-82 | 0df5119d |
| T-83 | 4b5c1799, 2a58f430 |
| T-84 | c391bf78 |
| T-85 | d7c9dd5d |
| T-86 | dbe17c58, 5b00c62e |
| T-87 | dbe17c58 |
| T-88 | dbe17c58 |
| T-89 | 320d268f |
| T-90 | e2578549 |
| T-91 | 4b5c1799, c391bf78 |
| T-92 | 4b5c1799 |
| T-93 | 4b5c1799 |
| T-94 | 4b5c1799 |
| T-95 | d7c9dd5d |
| T-96 | d7c9dd5d |
| T-97 | e0b4b6d6 |
| T-98 | e0b4b6d6 |
| T-99 | d7c9dd5d / e0b4b6d6 / 0df5119d |
| T-100 | 0df5119d |
| T-101 | 0df5119d |
| T-102 | 0df5119d |
| T-103 | 0df5119d |
| T-104 | 0df5119d |
| T-105 | 0df5119d |
| T-106 | 0df5119d |
| T-107 | e0b4b6d6 |
| T-108 | 320d268f |
| T-109 | e0b4b6d6 |
| T-110 | d7c9dd5d |
| T-111 | da9a63c8 |
| T-112 | d7c9dd5d |
| T-113 | d7c9dd5d / 40ed24b3 / e570f84a |
| T-114 | d7c9dd5d |
| T-115 | d7c9dd5d |
| T-116 | c391bf78 |
| T-117 | d492577b |
| T-118 | c391bf78 |
| T-119 | 2a58f430, c391bf78, 4b5c1799, d492577b, d7c9dd5d |
| T-120 | e2578549 |
| T-121 | 2a58f430 |
| T-122 | 8af7b282 |
| T-123 | 6dd02bd6 |
| T-124 | 6dd02bd6 |
| V-01 | 57819077 |
| V-02 | d1e57db5 |
| V-03 | 8af7b282 |
| V-04 | 960f6f88 |
| V-05 | c262af21 |
| V-06 | c262af21 |
| V-07 | d33337d7 |
| V-08 | c262af21, 960f6f88 |

### Verification (noye, cd4f96e7)

- vitest full suite: 8950 tests, 8900 pass, 50 fail. All 55 failing keys were already failing on the pre-fix baseline d3acc1c7; 0 new failures. tsc --noEmit exit 0.
- draft-guard and accent-stripes over the mobile scope (232 files): exit 0. impeccable detect: only overused-font (Geist, kept per V-03); no gradient text, no side stripes.
- DOM metrics on the QA build, 36 states x 390/375 x dark/light/200% text (216 runs): 0 targets under 44px, 0 inputs under 16px, 0 text under 12px, 0 contrast failures, 0 horizontal overflow.
- P0 behaviours: T-01 (ReferencePromptCard tap selects, Send sends), S-01 (pairing error is a role=alert, checked live on /pair), H-03 (SupervisorHome "Checking your computers…" vs empty), S-02 (RemoteApp.tunnelResume) pass.
- Screenshots: `ferryx-wt/.evidence/mobile-uiux-review-20261101/shots/final/`.
