# Ferryx bounded UI contract

Existing-system extraction, 2026-09-05. This is not a redesign. Read alongside
`ui/DESIGN.md`; current source wins where that older document differs. Shape follows
the frontend `references/design/design-system-architecture.md` (eight sections,
existing-project extraction only). No new UI/dev dependencies, tokens, fonts, or
primitive library are authorized.

## 1. Atmosphere & Identity

Preserve the dense desktop workspace shell: compact project/worktree navigation,
tab chrome, terminal/browser panes, and restrained semantic status indicators.
The signature is quiet tonal separation around the working pane, not decorative
cards or a new mobile brand. `ui/src/main.tsx` selects `App` inside Tauri and
`RemoteApp` otherwise; browser-only rendering does not demonstrate desktop parity.

## 2. Color

Authority: `ui/src/index.css`, `settings-runtime.css`, `tailwind.config.js`, and
`lib/appearanceSettings.ts`. The runtime stylesheet overrides the bootstrap palette.

| Role / utility | Token | Runtime light | Runtime dark |
|---|---|---|---|
| Shell / `bg-background` | `--background` | `#f6f7f9` | `#0a0a0a` |
| Card / `bg-card` | `--card` | `#ffffff` | `#141414` |
| Popover / `bg-popover` | `--popover` | `#ffffff` | `#141414` |
| Primary text / `text-foreground` | `--foreground` | `#171717` | `#fafafa` |
| Secondary / `text-muted-foreground` | `--muted-foreground` | `#666b74` | `#a1a1a1` |
| Active / `bg-accent` | `--accent` | `#e0e3e8` | `#404040` |
| Border / `border-border` | `--border` | `#17171718` | `#ffffff12` |
| Focus / `ring-ring` | `--ring` | `#737373` | `#737373` |

Use the existing primary, secondary, destructive, sidebar, worktree-sidebar,
input, and status semantic utilities, including their foreground counterparts.
Do not copy hex values into components. Tailwind's translucent mapping multiplies
the token alpha; `border-border/40` is intentionally an internal divider, not a
replacement for a full border. Terminal colors remain terminal-preference-owned;
`bg-terminal` is never new application chrome. Preserve working/warning/success/
idle tokens and destructive errors; communicate status in text as well as color.

## 3. Typography

Keep self-hosted Geist variable, `font-sans` system fallbacks, and `font-mono` for
identifiers/code. Retain the existing compact scale rather than importing the
reference's illustrative 14px minimum: 10px navigation overlines, 11px settings
descriptions, 12px explanatory text, 13px row labels, 15px semibold settings
headings (`settings/primitives.tsx`, `SettingsDialog.tsx`). Use nearby component
recipes; no display typography. Read-only history and paths use `selectable`
because the application body otherwise disables text selection.

## 4. Spacing & Layout

Keep Tailwind's 4px scale and named `sidebar: 14.75rem`, `titlebar: 2.25rem`,
`tabbar: 2rem`; radius is `0.5rem` with existing md/sm derivatives. Desktop's
configured initial window is 1280x850, minimum 800x600. Settings is the existing
full-window `z-50` shell, 280px navigation, independently scrolling main content,
896px maximum inner width, `px-8 pt-10 pb-16`; not a new centered modal.
Compose within existing panes and remote layout. Narrow rows truncate names and
retain actions; mobile QA must exercise wrapping, scroll, and keyboard visibility
without transplanting desktop fixed navigation into the phone layout.

## 5. Components

All paths below are under `ui/src/components/`; compose these existing controls.

| Primitive / structure | Variants and states | Layout / accessibility |
|---|---|---|
| `ui/button.tsx`: `Button` (button or Radix Slot) | default, destructive, outline, secondary, ghost, link; default/sm/lg/icon sizes; hover and disabled styles | Inline cluster; visible focus ring; supply button type in forms. Loading is caller-owned, not a built-in variant. |
| `ui/IconButton.tsx`: native button | sm 24px / md 28px; hover, focus, disabled | `label` supplies both title and accessible name; preserve compact desktop sizing. |
| `ui/input`, `label`, `select`, `switch`, `slider` | Existing text/selection/boolean/range controls and Radix states | Associate labels, retain keyboard semantics; no hand-built replacements. |
| `ui/alert`, `badge`, `card`, `progress`, `separator`, `sonner` | Existing callouts, metadata, containers, progress, rules, toast wrapper | Error/empty/loading content belongs to the consuming feature; actionable failures must remain visible, not only transient toasts. |
| `ui/SectionHeader`, `ui/StatusDot` | Header/count/actions; working spinner, starting pulse, waiting warning ring, unread primary dot, done/exited success, fallback failure | Status glyphs are aria-hidden; parent supplies status text. Do not assume unknown states render idle. |
| `settings/primitives.tsx`: `SettingsHeading`, `SettingsGroup`, `SettingRow` | Heading/description; group/action slot; row label/description/control slot | Vertical stacks, row `gap-4 py-3`, internal `border-border/40`; scroll owned by SettingsDialog main. |

Bounded feature placement and acceptance contract (required behavior, not a claim
that these features already ship):

| Included feature | Reuse / boundary | Required visible states |
|---|---|---|
| Waiting inbox | Existing Sidebar/TabBar activity and `StatusDot`; target exact workspace/worktree/tab/pane | Empty, waiting, unread, acknowledged; navigation alone must not erase another target's attention. |
| SSH / Run on | Existing agent launch surface (`AgentCards`, App), SSH command model; existing settings controls | Local versus named host, connecting, connected, actionable failure; never silently run locally after remote failure. |
| Conversation history | Existing shell navigation and selectable content, not terminal screenshot history | Loading, empty, readable result, unavailable provider/file, resume target; distinguish browser URL history. |
| Browser Design Mode | Existing `BrowserPane`/`BrowserToolbar` and native webview lifecycle | Off/on, selected element/context, unavailable page, cancel; ordinary browsing and pane/tab drag remain intact. |
| Mobile public API / chat / approvals / independent remote / push | Existing `RemoteApp`, settings Remote Access and shared controls | Pairing/auth failure, connection loss, selected context, chat pending/error, pending/resolved/stale approval, notification permission/subscription/error. Remote selection must not require changing desktop focus. |

Explicitly excluded: PR/GitHub/Linear integrations, editors/Git GUI, schedules,
dev service, usage, hibernation, plugins, voice, CloudVM, and relay. Do not add
navigation or placeholder screens for those domains.

## 6. Motion & Interaction

Retain existing color transitions, disclosure transforms, and `animate-enter`
(140ms ease-out, 2px translation plus fade). `StatusDot` uses reduced-motion-safe
spin/pulse; no new motion system. Preserve focus on return from settings, Escape
behavior, native-surface masking during overlays/drag, and stable terminal sessions
across pane movement. Subscribe to exact async state changes before actions in QA;
do not use fixed sleeps to make assertions pass.

## 7. Depth & Surface

Existing mixed strategy: tonal shell/card/active surfaces, semantic 1px borders,
and the shipped button shadow variants. Keep these recipes rather than declaring
the app shadow-free. Native terminal split separators remain opaque 1px
`--terminal-divider`, independent of transparent resize hit areas. New feature
content does not justify changing terminal compositing or app-level z-index rules.

## 8. Accessibility Constraints & Accepted Debt

Require semantic labels, keyboard reachability, visible focus, non-color status,
selectable conversation content, reduced-motion support, and recoverable error
states. Verify light/dark, desktop minimum width, and a narrow phone viewport;
this extraction does not certify contrast or WCAG conformance.

Existing discrepancies (recorded, not permission for new debt): `ui/DESIGN.md`
still lists old sidebar colors and describes working as a pulse; source now uses
runtime theme overrides and a spinner. Settings is full-window with
`aria-modal={false}` rather than a conventional modal. Compact typography and
icon controls are existing desktop behavior, not a claim of mobile touch-target
compliance. LSP is globally unreachable per task context; symbols were verified
by direct source reads/searches. No product behavior or styling changed here.
