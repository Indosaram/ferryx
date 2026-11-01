# Orca Lite: UI Design System

Extracted from the shipped Tailwind theme (`tailwind.config.js`) and token layer (`src/index.css`).
This file is the implementation contract: no color, size, spacing, or motion value may appear in a
component unless it traces back to a token named here.

## 1. Tokens

All colors are CSS custom properties in `:root` (`src/index.css`), exposed to Tailwind as semantic
utilities. Components reference the **Tailwind semantic name**, never a hex value.

### Surfaces
| Token | Utility | Use |
|---|---|---|
| `--background` `#23262d` | `bg-background` | App shell behind panes |
| `--card` `#171717` | `bg-card` | Titlebars, inactive tabs, tab strips, dialogs |
| `--accent` `#404040` | `bg-accent` | Active tab and interactive hover surfaces |
| `--terminal` `#282c34` | `bg-terminal` | Terminal viewport only; never application chrome |
| `--terminal-divider` `#5c6068` | `var(--terminal-divider)` | Opaque 1px separator between split terminal panes |
| `--worktree-sidebar` `#2a2a2a` | `bg-worktree-sidebar` | Sidebar surface (the project tree lives here) |
| `--worktree-sidebar-accent` `#353535` | `bg-worktree-sidebar-accent` | Selected / active row fill |
| `--popover` `#171717` | `bg-popover` | Floating surfaces |

### Text
| Token | Utility | Use |
|---|---|---|
| `--foreground` `#fafafa` | `text-foreground` | Primary labels (project + worktree names) |
| `--muted-foreground` `#a1a1a1` | `text-muted-foreground` | Secondary lines, branch names, icons at rest |
| `--worktree-sidebar-foreground` | `text-worktree-sidebar-foreground` | Sidebar-local text; `/45` to `/65` alpha for de-emphasis |

### Lines and focus
| Token | Utility | Use |
|---|---|---|
| `--worktree-sidebar-border` (white @ 7.06%) | `border-worktree-sidebar-border` | Section rules, tree guide rails |
| `--worktree-sidebar-ring` `#737373` | `border-worktree-sidebar-ring` | Active worktree rail accent |
| `--ring` `#737373` | `ring-ring` | Keyboard focus ring (`focus-visible:ring-1`) |

### Status
| Token | Utility | Meaning |
|---|---|---|
| `--status-working` `#60a5fa` | `bg-status-working` | Agent running |
| `--status-warning` `#fbbf24` | `text-status-warning` | Dirty tree, locked worktree, starting |
| `--status-success` `#86efac` | `bg-status-success` | Healthy / exited cleanly |
| `--status-idle` `#737373` | `bg-status-idle` | No agent attached |

`StatusDot` encodes state by shape as well as colour: starting is a hollow `--status-warning` ring (pulsing, `motion-reduce:animate-none`), waiting a filled `--status-warning` dot, working a spinner. No halo rings.

### Radius and spacing
- `--radius: 0.5rem` -> `rounded-lg` / `rounded-md` (`calc(radius - 2px)`) / `rounded-sm`.
  Sidebar rows use `rounded-md`.
- Named spacing: `sidebar: 14.75rem`, `titlebar: 2.25rem`, `tabbar: 2rem`.
- Grid: Tailwind's 0.25rem scale. Sidebar rows are `h-7` (1.75rem); dense sub-rows use `py-1.5`.

### Typography
Single family: **Geist** variable (`font-sans`), self-hosted, weights 100 to 900.
Sidebar scale, smallest to largest:
| Size | Use |
|---|---|
| `text-[9px]` | Reserved; sidebar rows carry no second line (§4 rule 6) |
| `text-[10px]` | Sidebar state slot labels, footer status line |
| `text-[11px]` | Tree guide labels, empty states |
| `text-[12px]` | Worktree name, project name, search field |
| `text-[13px]` | Workspace switcher row |
Branch names and other git identifiers always render `font-mono`.

### Mobile chat surface tokens
These are the remote phone client's only palette. Chat components reference the **Tailwind semantic name**, never a hex value. Chat surfaces may additionally use the shared semantic tokens --status-working / --status-warning / --status-success (agent status), --ring (focus rings) and --terminal (the embedded terminal viewport); no other colour value may appear in a chat component.

| Token | Tailwind utility | Use |
|---|---|---|
| `--chat-screen` `#0a0a0a` | `bg-chat-screen` / `text-chat-screen` | Root viewport/screen background |
| `--chat-surface` `#111111` | `bg-chat-surface` / `text-chat-surface` | Default surface / card background |
| `--chat-surface-raised` `#1a1b1b` | `bg-chat-surface-raised` | Elevated card / modal background |
| `--chat-surface-hover` `#141414` | `bg-chat-surface-hover` | Hover state for interactive chat rows/buttons |
| `--chat-user-bubble` `#161616` | `bg-chat-user-bubble` | User message bubble fill |
| `--chat-foreground` `#f5f5f5` | `text-chat-foreground` | Primary text and headings |
| `--chat-foreground-secondary` `#a1a1a1` (light `#52525b`) | `text-chat-foreground-secondary` | Secondary labels, timestamps, metadata (>= 6.6:1 on every chat surface) |
| `--chat-foreground-tertiary` `#858585` (light `#63636d`) | `text-chat-foreground-tertiary` | Tertiary/de-emphasized text and icons; >= 4.5:1 on screen and surface, at least 15% luminance away from secondary |
| `--chat-border` `#363636` (light `#bdbdc3`) | `border-chat-border` | Card, sheet and separator boundary (>= 1.5:1 on the screen); not for input outlines |
| `--chat-primary` `#346bf1` | `bg-chat-primary` / `text-chat-primary` | Primary action buttons and focus accents |
| `--chat-primary-foreground` `#ffffff` | `text-chat-primary-foreground` | Text and icons on `chat-primary` fills (white on #346bf1 = 4.65:1) |
| `--chat-link` `#4a7cf5` | `text-chat-link` | Markdown links in chat prose (5.18:1 on the chat screen; the primary blue is 4.26:1 and fails AA at text size) |
| `--chat-code` `#4bb8f0` | `text-chat-code` / `bg-chat-code` | Code block highlighting and inline code text |
| `--chat-danger` `#ff6467` | `text-chat-danger` / `bg-chat-danger` | Destructive actions, errors, cancellation |
| `--chat-row-hover` `rgba(255,255,255,0.04)` | `bg-chat-row-hover` | Subtle white-alpha hover on chat rows |
| `--chat-composer-panel` `rgba(10,10,10,0.92)` | `bg-chat-composer-panel` | Backdrop panel for mobile composer |
| `--chat-composer-surface` `rgba(26,27,27,0.9)` | `bg-chat-composer-surface` | Translucent input editor surface |
| `--chat-composer-border` `#6b6b6b` (light `#85858e`) | `border-chat-composer-border` | Composer and text-input outline (>= 3:1 on the screen, WCAG 1.4.11) |

Inside `.remote-app-root` the shared `--border` follows `--chat-border`, `--input` is `#6b6b6b` / light `#85858e` (3:1 for input outlines) and `--muted-foreground` is `#a1a1a1`. The light remote root also overrides `--status-working/-warning/-success/-idle` so every status colour reaches 4.5:1 on white (status used as text needs 4.5:1, as a glyph 3:1).

**Font (V-03, brand decision).** Geist stays the single family on the phone as well as the desktop. It is the shipped brand face, self-hosted and variable; the phone gets its own decisions through size, weight and the rem phone scale (§6), not a second family. Revisit only with a brand brief, never as a drive-by swap.

## 2. Primitives

Reusable, already shipped. Compose these; don't hand-roll equivalents.

### App Primitives
- `ui/IconButton` (`ui/src/components/ui/IconButton.tsx`): Square icon control (`size-6` sm / `size-7` md for desktop; `size-11` `touch` for phone surfaces, per §6), `aria-label` and `title` required.
- `ui/SectionHeader` (`ui/src/components/ui/SectionHeader.tsx`): `h-8` section title with optional count pill and trailing actions.
- `ui/StatusDot` (`ui/src/components/ui/StatusDot.tsx`): Maps `AgentState` to a status glyph; `working` pulses, `motion-reduce` safe.
- `ui/SegmentedControl` (`ui/src/components/ui/SegmentedControl.tsx`): `role="tablist"` of equal-width `role="tab"` buttons with `aria-selected` and arrow-key roving. Track `bg-card rounded-md p-0.5`; segment `text-[11px] rounded-sm text-muted-foreground`; selected segment `bg-worktree-sidebar-accent text-foreground`. An optional count renders `text-status-warning` beside the label. Selection is a fill, never a border.
- `sidebar/SidebarStateSlot` (`ui/src/components/sidebar/SidebarStateSlot.tsx`): The single trailing status element of a sidebar row. `waiting` renders "needs input" + `StatusDot`; `working` renders "running" (or "N running") + `StatusDot`; `unread` and `done` render a `StatusDot` only; remote renders lucide `Server` `size-2.5` + host text truncated at `max-w-[88px]`. `text-[10px] text-muted-foreground shrink-0`. A `button` only when it has an action, otherwise a non-interactive `span`. Renders nothing for idle.
- `lib/cn` (`ui/src/lib/cn.ts`): `twMerge(clsx(...))` for conditional classes. All conditional styling goes through it.

### shadcn Base Primitives (`ui/src/components/ui/`)
- `alert` (`alert.tsx`): Status banners and inline callouts with `default` and `destructive` variants.
- `badge` (`badge.tsx`): Metadata tags and status pills.
- `button` (`button.tsx`): CVA button component supporting `default`, `destructive`, `outline`, `secondary`, `ghost`, and `link` variants.
- `card` (`card.tsx`): Structured container with `CardHeader`, `CardTitle`, `CardDescription`, and `CardContent`.
- `input` (`input.tsx`): Accessible text, number, and search field.
- `label` (`label.tsx`): Radix-backed form label.
- `progress` (`progress.tsx`): Radix-backed progress indicator bar.
- `select` (`select.tsx`): Accessible Radix select menu with custom trigger, popover viewport, and items.
- `separator` (`separator.tsx`): Full-alpha horizontal or vertical divider.
- `slider` (`slider.tsx`): Radix-backed range slider for numeric inputs.
- `switch` (`switch.tsx`): Accessible Radix toggle (`button[role="switch"]`).

### Settings Layout Primitives (`ui/src/components/settings/primitives.tsx`)
- `SettingsHeading`: Section header with icon, title, and description.
- `SettingRow`: Standard configuration row with label, secondary description, and right-aligned control slot.
- Note: Legacy paths `ui/src/components/ui/SettingsPrimitives.tsx` and `ui/src/components/BrowserSettingsPanel.tsx` have been deleted.

### Keycap styling recipe
Keycap pills in `ShortcutsSection` retain muted background tones (`rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[10px]`, `border-border/60 bg-muted/40`).

### Agent tab identity
- A terminal tab with an agent type in `SUPPORTED_AGENT_LOGOS` displays that local brand SVG at `size-4`.
- Brand SVGs are decorative (`alt=""`) because the tab label supplies the accessible name.
- Any unknown, unsupported, or terminal-only type uses `TerminalSquare`; never substitute a generic bot or a guessed logo.
- The separate `StatusDot` remains adjacent to the identity icon, so working/waiting/done is not encoded by the logo.

### Tab chrome states
- The tab strip and inactive tabs use `bg-card`.
- The active tab uses `bg-accent text-foreground`, so it follows light/dark appearance settings.
- `bg-terminal` is reserved for the terminal viewport and its pixel-filling wrappers. A tab must never inherit the terminal theme background.

### Terminal split separators
- Horizontal and vertical terminal splits use the opaque `--terminal-divider` color directly on a line exactly 1px thick.
- The visible line remains 1px; a separate transparent hit area extends 4px around it for resizing.
- Do not reuse translucent application `border` tokens here: their built-in alpha makes a 1px line disappear against terminal backgrounds.

### Native terminal scrollbar
- Native terminal panes reserve a `w-3` (12px) right-side strip outside the compositor viewport so the DOM scrollbar remains visible above the native surface.
- The track uses `bg-terminal`; its narrow thumb uses `bg-muted-foreground/45` and only changes color on hover. It has no decorative motion beyond the existing `transition-colors` contract.
- The scrollbar is rendered only when Ghostty reports retained scrollback (`total > len`) and exposes the semantic vertical `scrollbar` role for assistive technology.

## 3. shadcn/ui Adoption

The settings system uses shadcn/ui components built on Radix UI primitives and Tailwind CSS.

### Configuration (`components.json`)
The project configures shadcn with the following settings:
- Schema: `https://ui.shadcn.com/schema.json`
- Style: `default`
- Base color: `neutral`
- CSS variables: enabled (`true`)
- Aliases: `@/components` for components, `@/lib/cn` for utils, and `@/components/ui` for primitives.
- Icon library: `lucide`

### Dependencies
The adoption added Radix UI packages, styling helpers, and animation plugins:
- Radix primitives: `@radix-ui/react-label`, `@radix-ui/react-progress`, `@radix-ui/react-select`, `@radix-ui/react-separator`, `@radix-ui/react-slider`, `@radix-ui/react-slot`, `@radix-ui/react-switch`.
- Utility packages: `class-variance-authority` (CVA) and `tailwindcss-animate`.

### Token Alignment
Zero CSS token changes were needed during shadcn adoption. The existing `:root` variables in `src/index.css` and color mappings in `tailwind.config.js` already supply the standard shadcn convention set (`--background`, `--foreground`, `--card`, `--popover`, `--primary`, `--secondary`, `--muted`, `--accent`, `--destructive`, `--border`, `--input`, `--ring`). Custom hex values and the Geist font family remain fully preserved.

### Settings Composition Rules
- **Standalone Section Modules**: Each settings tab is an isolated component in `ui/src/components/settings/` (`GeneralSection`, `AppearanceSection`, `TerminalSection`, `ShortcutsSection`, `WorkspaceSection`, `AgentsSection`, `BrowserSection`, `NotificationsSection`, `RemoteAccessSection`).
- **Dialog Shell**: `SettingsDialog.tsx` hosts tab navigation and renders the active section inside its content viewport.
- **Toggles**: Boolean settings use shadcn `<Switch>` (`button[role="switch"]`). Native checkboxes are no longer the settings standard.
- **Selects**: Radix `<Select>` controls replace native `<select>` dropdowns across all panels.
- **Sliders**: Radix `<Slider>` handles numeric ranges (such as volume levels and scale settings).
- **Density Overrides**: Desktop settings rows require compact controls. Actions inside `SettingRow` use `size="sm"` or explicit `h-7` button height overrides with `text-[11px]` typography.

### Historical Design Rules & Superseded Patterns
The settings redesign originated from the audit documented in `ui/docs/settings-panel-redesign.md` (rules S1 through S8). The shadcn implementation supersedes older manual recipes where they conflict:
- **S1 (Headings & Accessibility)**: Standard preserved. Each section provides `SettingsHeading`, an accessible `sr-only` heading, and an enclosing `section` with `aria-labelledby`.
- **S2 (Boolean Controls)**: Superseded. shadcn `<Switch>` replaces the legacy native checkbox standard.
- **S3 (SettingRow API)**: Standard preserved. `SettingRow` expects a `label` prop (with fallback to `title`).
- **S4 (Shared Primitive Location)**: Updated. `SettingsHeading` and `SettingRow` now live at `ui/src/components/settings/primitives.tsx`.
- **S5 (Button Hierarchy)**: Updated. Standardized on shadcn `<Button>` variants with `h-7` / `size="sm"` density adjustments inside settings rows.
- **S6 (Border Alpha Discipline)**: Standard preserved. Section wrappers and cards use full-alpha `border-border`. Internal row dividers use `border-border/40`.
- **S7 (Layout & Surfaces)**: Standard preserved. Preference rows live inside `border-y border-border` groups via `SettingRow`; management blocks live inside `Card` containers (`rounded-lg border border-border bg-card`).
- **S8 (Inputs, Selects & Sliders)**: Superseded by shadcn primitives. Native inputs, selects, and range sliders are replaced by shadcn `<Input>`, `<Select>`, and `<Slider>`.

## 4. Sidebar Information Architecture

The sidebar presents **one nested tree**, not two disjoint lists:

```
[ Worktrees | Inbox 3 ]                  <- SegmentedControl
[ Filter worktrees      ]                <- Input
Projects 4                               <- SectionHeader, count = project groups
+- v ferryx                    [+] [...] <- project row; + and ... on hover only
|  |  main                  running o   <- worktree row, one 28px line
|  |# sidebar-redesign      running o   <- active row: accent fill + bright rail
|  |  relay-latency     needs input o   <- state slot
|  |  daemon-lifetime                    <- idle: nothing after the name
|  |  omarchy               [] omarchy  <- remote host slot
+- > maho-ws                    [] daas  <- collapsed project row with status slot
+- > piratetalk                      o   <- unread
\- > site
2 running · 1 needs input                <- footer
```

Rules:
1. **Every registered project renders as a project row.** Rows are disclosure controls
   (`aria-expanded`), not plain links.
2. **Worktrees nest under their owning project.** Only the active project has loaded worktrees.
   `useWorkspaceStore` and `useWorkspaceRuntime` are scoped to a single `workspaceId`, so only the
   active project expands. This is an honest reflection of loaded state, not a limitation to hide.
3. **Selecting a collapsed project switches the active project**, which loads its worktrees and
   expands it. Selecting a worktree activates that worktree, switching project first if needed.
4. **Depth is expressed with a guide rail**, not indentation alone. Nested worktrees sit behind a
   1px `border-worktree-sidebar-border` vertical rail. Indentation step is `pl-3`.
5. **Active worktree = fill + rail, never a border.** The active row is
   `bg-worktree-sidebar-accent text-foreground`, and its 1px rail segment brightens to
   `worktree-sidebar-ring`. No raw hex classes; no state borders on rows.
6. **A worktree row is one `h-7` (28px) line**: name (`text-[12px] font-semibold truncate`), then
   at most one `SidebarStateSlot`. No git data: no branch, dirty count, `primary` badge, or remote
   path line. Root-worktree status is logic only (no delete action), never display.
7. **State-slot priority** (one slot per row, highest wins): needs input > running > unread >
   remote host. Idle shows nothing; no grey idle dot. A project row uses the same slot; a listing
   failure ("Unavailable", reason in `title`) outranks all, and paired machine state ranks last,
   with text from the shared remote status vocabulary (see Remote settings). A not-connected
   paired slot is a button that opens Settings > Remote on that machine.
8. **Project name click selects and expands; it never collapses.** Only the chevron collapses.
9. **Project hover actions are exactly `+` and `...`** (`IconButton` sm). `...` opens the native
   project menu (Past conversations, Remove Project); no DOM dropdown in the sidebar.
10. **Anatomy, top to bottom**: `SegmentedControl` Worktrees / Inbox (Inbox carries the attention
    count), a filter `Input` ("Filter worktrees", Escape clears, not persisted), `SectionHeader`
    "Projects N", the tree, and a footer reading `<n> running · <m> needs input` in
    `text-[10px] text-muted-foreground`. Each footer part is omitted at zero; both zero = empty footer.
    A non-empty filter forces projects with a matching worktree expanded.

## 5. Motion

- Only `transition-colors` on hover/active state changes (GPU-composited; no layout animation).
- Disclosure chevrons rotate via `transition-transform` (`rotate-90` when expanded).
- `animate-enter` (140ms `translateY(2px)` + fade) is available for newly revealed groups.
- `StatusDot`'s ping/pulse and pending SSH connection spinners are the only looping motion;
  both are disabled under `motion-reduce`.
- SSH initialization replaces the empty workspace with a centered `Loader2` (`size-5`),
  a `text-sm` phase label and `text-xs` host label on `bg-background`. Use muted foreground,
  `gap-3`, `p-6`, wrapping text, and a polite status region with `aria-busy`.
  Failed connections stop the spinner and show the structured error plus a small retry button.
  Ready workspaces with no tabs retain the existing empty state.
- No hover state may change anything other than color/opacity on a non-interactive element.
- The chat's looping motion (animate-ping, animate-spin, animate-pulse) carries `motion-reduce:animate-none`; the scroll-to-latest affordance has no looping animation; no hover transform on non-interactive elements.
- The "working" label (`.work-shimmer-text`) is solid `--chat-foreground-secondary` text with a 1.6s opacity pulse that only runs under `prefers-reduced-motion: no-preference`. Gradient text is banned.
- `.pane-inbox-flash` uses `--status-warning`; under reduced motion it holds and then disappears without a fade.
- Buttons animate colour only (`transition-colors`); `transition-all` and press `scale` are not used on phone controls. Every `animate-*` or `transition-transform` carries a `motion-reduce:` guard.

## 6. Responsive Behavior

The sidebar is user-resizable and persisted (`orca.sidebar.width`), clamped 220-420px, default 236px.
Every row must survive the 220px floor: names `truncate`; the state slot and action icons are
`shrink-0`; the host label truncates at `max-w-[88px]`.

### Mobile chat surface constants
- There is no chat terminal drawer: the chat and the terminal are two modes of the Session screen, switched from the top bar.
- Corner radii: `20px` user-bubble radius, `12px` card radius.
- Safe-area rule: `ui/index.html` sets `viewport-fit=cover`, because `env(safe-area-inset-*)` is 0 without it. Bottom-anchored bars use `.pb-safe`, headers `.pt-safe-header`, sheets `.pb-safe-sheet`; full-width roots and bottom bars also use `.px-safe` (or `.px-safe-4` where they already pad 1rem) so landscape notches never cover content.

### Phone touch density (all remote surfaces below 768px)
These rules override every compact desktop size in this file for the phone. The desktop keeps its
compact scale; phone changes are gated on the remote root or the existing touch/mobile prop.
- **Targets.** Every interactive control has a hit area of at least 44x44 CSS px (`min-h-11`,
  `size-11`, or an invisible hit-slop covering 44x44 on both axes), with at least 8px between
  neighbouring targets. Glyphs may stay `size-4`/`size-5`; the hit area grows, never the icon.
  `IconButton` `sm`/`md` (24/28px) are desktop sizes and are not used bare on a phone.
- **Header controls.** Phone header and top-bar controls (back, terminal/chat switch, overflow,
  context picker) follow the same 44x44 rule (`size-11` / `min-h-11`); the compact `size-7` (28px)
  header buttons are desktop-only and never ship on a phone header.
- **Text.** Body text is at least 14px (`text-sm` / `text-phone-body`); nothing is smaller than
  12px (`text-xs` / `text-phone-meta`). `text-[10px]`, `text-[11px]` and `text-[10.5px]` are
  banned on phone surfaces. Use rem sizes (Tailwind's scale or `text-phone-*`), never `text-[Npx]`,
  so the density setting and 200% text scaling reach every label.
- **Inputs.** Inputs and textareas render at 16px (`text-base` / `text-phone-input`) so iOS does
  not zoom on focus; their outline uses `border-input` / `border-chat-composer-border` (3:1).
- **Contrast.** Body text >= 4.5:1, non-text UI (outlines, icons that carry meaning, focus) >= 3:1,
  in dark and light. Only semantic tokens: no hex, no `bg-white/x`, no `bg-black/x`, no palette
  colours (`rose-`, `amber-`).
- **No generated-UI tells.** No coloured side stripes (`border-l-*` accents, `before:w-[3px]`
  bars), no decorative `ring-2` halos (status uses shape: hollow vs filled), no accent outline on a
  rounded card, no gradient text, no sparkles icon, no icon-in-tinted-tile alerts, no
  `backdrop-blur` on opaque surfaces, no decorative shadows on the dark opaque UI.
- **Copy.** Sentence case. Plain language; machines are "computer". No raw codes, enums, session
  ids or CLI commands on screen. Every user-visible error goes through
  `ui/src/remote/userFacingError.ts`. An empty state always offers its next action as a button.
  Actionable failures stay visible inline; a toast is a confirmation, never the only record.
- **Toasts.** On phones the Toaster sits top-center below `safe-area-inset-top`, clear of the tab
  bar, composer and keyboard.
- **History.** Switching a tab *replaces* the history entry; opening a pushed screen (a session)
  or an overlay (sheet, drawer, dialog) *pushes* one; the platform back gesture closes the topmost
  overlay first, then returns from a pushed screen, then leaves the app. Back never walks through
  tab taps.

## 7. Accessibility Constraints

- The tree uses semantic disclosure: project rows expose `aria-expanded`; the worktree group is
  labelled by its project row via `aria-label`.
- Active worktree is announced with `aria-current="true"`, not color alone.
- Status is never encoded by color alone; a running/needs-input slot always carries its text label
  next to the dot, and dots without text (unread/done) carry an accessible name.
- All icon-only controls carry `aria-label` (enforced by `IconButton`).
- Focus is visible on every control: `focus-visible:ring-1 focus-visible:ring-ring`.
- Nested action buttons must not be DOM descendants of the row button (no nested interactives).
- Every icon-only chat control carries `aria-label` (which includes any visible label text), every chat control carries `focus-visible:ring-1 focus-visible:ring-ring`, decorative icons are `aria-hidden`, the running activity indicator is `role="status"`, and one message is announced by one live region only.
- Reading content inside `.remote-app-root` (paragraphs, list items, code, alerts, status text) is selectable; the desktop shell's `user-select: none` applies to controls only.

## 8. Accepted Debt

- **Inactive projects show no worktree count.** The client only holds the active workspace's
  worktrees, so a count for collapsed projects would require a backend fan-out (`cmd_worktree_list`
  per project). Deferred; the collapsed chevron communicates "expandable" without promising a number.
- Worktree rows re-derive display names from branch strings on each render. Cheap at realistic
  worktree counts; revisit only if a project exceeds a few hundred worktrees.
- **The sidebar filter only matches loaded worktrees.** Inactive projects match by project label
  only, because their worktrees are not loaded (same constraint as the missing count above).
- **Mobile composer attachment staging:** The mobile composer can stage attachments but sending is blocked until remote upload exists, so the composer states the block in a polite status region.

## 9. Onboarding wizard

Added for the first-run Welcome onboarding modal (`ui/src/components/onboarding/`).

### Primitives (`WizardPrimitives.tsx`)
- `WizardRowList`: Bordered list container (`border-y border-border divide-y divide-border/40`) grouping wizard items.
- `WizardRow`: Flexible item row with title (`text-[13px] font-medium text-foreground`), optional description (`text-[12px] text-muted-foreground`), status glyph/label, and trailing action slot (`shrink-0`).
- `WizardStatus`: Compact status indicator (`text-[11px] font-medium`) pairing semantic tokens (`text-status-success`, `text-status-warning`, `text-muted-foreground`) with a non-color glyph (`Check`, `AlertCircle`, `Minus`).
- `Keycap`: Standard keyboard shortcut badge (`rounded border border-border bg-muted/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground`).

### Dialog & Step States
- Dialog shell: Two-column desktop grid (`sm:grid-cols-[13rem_1fr]`) with left linear step rail and right content viewport.
- Step transitions: Step content bodies mount with `animate-enter motion-reduce:animate-none`.
- Event-driven sync: Hardware and OS permission statuses refresh on window `focus` and `visibilitychange` rather than periodic polling intervals.
- Action hierarchy: Single primary action in footer ("Continue" / "Done") with secondary actions ("Skip setup", "Back"); per-row interactions use compact controls (`size="sm" h-7 text-[11px]`).

### Surface patterns (round 2)
- **Empty workspace (`GettingStartedChecklist`)**: no `Card` boxes. A centered `max-w-md w-full px-4` column holds a left-aligned header, then full-width choice rows that reuse the `FirstProjectStep` anatomy (icon tile + `text-[13px]` title + `text-[12px]` description + `ChevronRight`), then one quiet setup row (`border-t border-border pt-4`) pairing the permissions line with a ghost `Open Welcome Setup` action.
- **`WhatsNewDialog`**: same shell as the wizard — `bg-background/80 backdrop-blur-sm` scrim, `rounded-lg border border-border bg-card shadow-lg` panel, `Updated` eyebrow + `text-[17px]` heading named by `aria-labelledby`, `IconButton` close, footer `border-t` with a focused-on-mount `Got it` primary action. Escape is handled on the panel, never a window listener. Markdown renders at the `text-[13px]` scale; links stay `text-foreground` with `decoration-border` (never `text-primary`).
- **`DaemonConnectionBanner`**: a calm inline notice — `border-b border-border bg-card`, one `text-status-warning` `AlertCircle` glyph, `text-[13px]`/`text-[12px]`/`font-mono text-[11px]` text block, compact `h-7 text-[11px]` secondary retry. No amber slabs, tinted borders, or state colours beyond the status token.
- **Default agent picker (Agents step)**: a `role="radiogroup"` of choice-row buttons (`rounded-md`, `hover:bg-accent`, `focus-visible:ring-1`); selection reads as `bg-accent` plus a trailing `Check`, never a coloured border. Options carry the brand icon from `lib/agentIcon` (`agent-tab-logo--monochrome` when monochrome, `TerminalSquare` when unknown) and the resolved command in `font-mono`. Roving tabindex, arrow-key selection, immediate `saveAgentSettings` persistence; agents that are not installed collapse into one muted `Not installed: …` line, and an all-empty PATH renders the `WizardRow` empty message instead of the group.

### Round 3 patterns
- **Intro step (`IntroStep`)**: a setup overview, not a feature pitch. One `text-[13px]` sentence on what Ferryx is, then a `WizardRowList` of what this setup covers, driven by the wizard's actual `steps` (System access appears only when the permissions step is present). Product explanations live only in the Features step.
- **Features step (`FeaturesStep`)**: a `grid gap-3 sm:grid-cols-2` of `<figure>` cards (`rounded-md border border-border bg-card/40 p-3`). Each opens with an `aria-hidden` illustration panel (`h-24`, `rounded-md bg-background/60`, no border of its own, so a card carries at most two framed levels: the card and the miniature's own chips) holding a token-only UI miniature (panes, tabs, status dots, a command palette row built from `bg-card`, `bg-accent`, `bg-muted`, `--terminal-divider`, status tokens; `bg-terminal` only where the miniature depicts a terminal pane; no images, no raw colours), then a `<figcaption>` with a plain muted icon glyph + `text-[13px]` title and a `text-[12px]` muted description. Keycaps appear once, in the keyboard card's caption, never repeated in its illustration.
- **Permissions all-set line**: when every visible permission is granted, one `text-[13px] font-medium` line with a `text-status-success` `Check` sits above the rows. No banner, no tinted slab.
- **Inline custom agent (Agents step)**: "Add custom agent" expands an inline `<form>` (Name, Command, optional Arguments inputs, ghost Cancel, primary Add) inside the step; it never navigates out of the wizard. Command is a bare program name looked up on PATH; flags go in Arguments, matching Settings > Agents. Opening it focuses Name, Enter submits, Escape cancels the draft without closing the wizard, and closing the form (Escape, Cancel, Add) returns focus to the "Add custom agent" button. Input ids come from `useId()`. A validation error renders as `role="alert"` referenced by the inputs' `aria-describedby`. The new agent and the default are saved in one settings write.
- **Rail done rule**: a rail row shows the `Check` when its step is before the current one or already satisfied (`doneSteps`); the current step always shows its number and `aria-current="step"`, never the check.
- **Command-line step (`CliStep`)**: an optional step of its own after Agents, dropped from the wizard when the launcher is unsupported on the platform. A `text-[13px]` sentence on what `ferryx` does, then example commands as `font-mono text-[11px]` code lines on `bg-muted` with a `text-[12px]` muted caption each (only subcommands the launcher routes), then one `WizardRow` for the launcher: `WizardStatus` Installed / Not installed, the launcher path in the description, and a compact secondary `Install CLI` action. An install failure is a `role="alert"` `text-status-warning` line; an unsupported or unknown status is one neutral `text-muted-foreground` line, never an error. Continue works whether or not it is installed.

- **Settings (`PermissionsSection`)**: the onboarding shortcut is a `SettingRow` (label + description + `h-7 text-[11px]` secondary control), not a floating ghost button.

## 10. Remote Account Login Surface

Added for web/mobile remote authentication (`ui/src/remote/AccountLoginPage.tsx`).

### Design Contract & Pattern Lineage
- **Layout Pattern**: Adopts the StyleGallery `cover` viewport-shell pattern (`https://raw.githubusercontent.com/changeroa/StyleGallery/main/patterns/viewport-shell/cover.md` via `every-layout.dev`).
- **Spatial Structure**: The root viewport shell uses `grid grid-rows-[1fr_auto] min-h-dvh w-full p-4 sm:p-6 lg:p-8 bg-background text-foreground`.
  - Main: Centered `w-full max-w-sm` container hosting the unified brand lockup and authentication card without artificial scroll traps.
  - Footer: Quiet bottom metadata anchor (`text-xs text-muted-foreground`) providing authentic application identification without monospace costume or arbitrary opacity filters.
- **Scroll Ownership**: Strict zero internal scroll container (`overflow-visible` on card; document root owns all natural viewport overflow). No nested scrolling or height-capped containers (`min-h-dvh` instead of fixed viewport clamps).

### Brand Lockup & Visual Anatomy
- **Unified Lockup Placement**: The 64px app icon sits directly inside the central main column above the heading, forming an integrated brand unit with the card rather than floating disconnected at the top of the viewport.
- **App Logo**: Actual Ferryx app icon asset `/icon-192.png` rendered with explicit `width={64} height={64}` (`size-16 rounded-2xl shadow-lg`) for crisp multi-DPI display.
- **Brand Typography**:
  - Heading: Strong `text-[28px] font-semibold tracking-tight text-foreground leading-tight` using standard copy `Sign In to Ferryx` (preserving contract and avoiding prose-pinned test breakages).
  - Subheading/Description: Calm `text-sm text-muted-foreground leading-relaxed mt-2`.
- **Card Container**: `rounded-2xl border border-border bg-card p-6 sm:p-8 shadow-xl space-y-6`.
  - Restrained tonal depth: charcoal dark palette with clean solid card background (`bg-card`), strictly avoiding unnecessary `backdrop-blur` on solid backgrounds, glowing halos, or decorative clutter.

### Controls & Input Ergonomics
- **Touch Target & Font Sizing**: Spacious `h-11` (44px) inputs and primary actions. Inputs explicitly use `text-base` (16px) to eliminate iOS Safari viewport auto-zoom on focus while remaining visually compact via controlled vertical padding.
- **Input Attributes & Ergonomics**: Form field carries `autoComplete="email"`, `autoCapitalize="none"`, and `spellCheck={false}` for mobile and desktop browser ergonomics.
- **Placeholder Contrast**: `placeholder:text-muted-foreground` without opacity division (`/60`), maintaining clean readability and WCAG AA contrast.
- **Focus Rings**: Standard visible focus rings on all interactive elements: `focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none` on inputs and buttons.
- **Interactive Feedback**:
  - Primary button: `bg-primary text-primary-foreground hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:pointer-events-none`.
  - Input field: `bg-background border border-input text-foreground transition-colors focus:border-border`.

### State Handling Discipline
- **Error State**:
  - Per frontend design discipline and skill directives, errors must NEVER use colored borders (`border-destructive` is forbidden).
  - Clean calm callout: `p-3.5 text-xs text-destructive bg-destructive/10 rounded-lg space-y-1` without tinted borders, referencing `role="alert"` and preserving `data-testid="account-login-error"`.
- **Magic Link Waiting State**:
  - Clean, unnested status presentation without decorative nested bordered boxes or pulsating animations.
  - Features the static Lucide `MailCheck` icon (`size-5 text-muted-foreground shrink-0`) paired with clear informational copy.
  - Email highlight: `<strong className="font-medium text-foreground break-all">{email}</strong>`.
  - Secondary reset action: `h-10 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors rounded-lg flex items-center justify-center focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none` to change target email.

## 11. Remote Account Direct Worktree Selection Architecture

Updated for post-magic-link unified top picker selection (`ui/src/remote/useAccountWorktrees.ts`, `ui/src/remote/RemoteApp.tsx`).

### Design Contract & Architecture
- **Elimination of Intermediate Machine and Selection Screens**: After magic-link authentication, users are not forced into any separate intermediate page (no machine connect screen and no standalone centered worktrees card list).
- **Direct Fallthrough to Main Remote Shell**:
  - Authenticated account sessions fall directly through to the common `RemoteApp` shell.
  - The content area is initially empty (no premature terminal, chat messages, or browser view mounted).
  - The existing top header trigger (`aria-label="Change workspace context"`) is present in its collapsed state, displaying the default context text.
- **Top Picker Inventory Integration (`RemoteWorkspaceMirror`)**:
  - Enrolled online machines are discovered in the background via `useAccountWorktrees`.
  - Machine inventories are loaded over read-only tunnels (`GET /api/v1/workspace/state` on declared `projects[].worktrees`), strictly excluding synthetic fallback context options to prevent phantom "default worktree" entries.
  - Worktrees from every machine populate the existing desktop-style worktree list, grouped by `workspaceId` exactly like the desktop sidebar. No machine name, machine group, machine status row, or machine choice is ever rendered. The option keeps the raw desktop `workspaceId`; the owning machine travels in a hidden `machineId` used only for row keys, active-row matching, and dispatch, so identical workspaces on two machines stay two distinct rows.
  - Only aggregate inventory state renders inside the opened picker ("Loading worktrees...", a relay error alert, and one "Retry loading worktrees" action for failed probes); offline machines simply contribute no rows. Before a choice the body is blank and the header shows only the collapsed trigger.
  - In account mode the picker footer omits the "Machines" (host drawer) action entirely; its left slot is a neutral "Sign out" icon action instead. Paired-device mode keeps "Machines".
- **Explicit User Selection & Context Gate**:
  - The client NEVER issues `POST /api/v1/workspace/select` or manipulates desktop focus during discovery.
  - When the user opens the top picker and explicitly clicks an option:
    1. The target machine's connection and token are committed to `RemoteApp`.
    2. All unselected exploratory tunnels are cleanly closed.
    3. The selection request is sent to the daemon over the target tunnel.
    4. Upon confirmation matching the target workspace and worktree slug, the gate opens and the active surface mounts.
  - Subsequent cross-machine switches from the top picker reuse this same connection acquisition and tunnel lifecycle discipline.
- **Failure Isolation & Non-blocking Degraded States**:
  - Offline machines and partial host discovery failures only remove rows from the worktree list, without failing online machines or blocking the UI shell.
  - Inflight selection failures provide explicit "Retry Selection" and "Back to Worktrees" recovery actions without leaving the interface stalled.

## 12. Remote settings

Desktop Settings > Remote (`ui/src/components/settings/RemoteSection.tsx`). Machine-centric, not a
flat stack of panels.

### Information architecture
Sections render in this order, each with `SettingsHeading` semantics (S1):
1. **Account bar**: one row. Signed-in email or a Sign in action, plus account actions.
2. **My machines**: paired and SSH machines in one list, one row per machine. Never merged
   automatically.
3. **Discovered**: machines found but not yet set up (for example Tailscale peers). Their only
   primary action is "Set up". They never appear under My machines until set up.
4. **This machine**: this device's enrollment code, devices, gateway and relay settings.
5. **Advanced**: collapsed by default. Native inventory, credential migration, account server URL
   and other diagnostics live only here.

### Status vocabulary
Paired and SSH machines share one vocabulary, produced by one pure status function that the
sidebar slot also uses. The label is always text; color never carries status alone.
| Label | Token | Meaning |
|---|---|---|
| Connected | `text-status-success` | Reachable and usable |
| Offline | `text-muted-foreground` | Probe says unreachable |
| Checking… | `text-muted-foreground` | Not probed yet; never shown as Connected |
| Needs access | `text-status-warning` | Grant, re-pair or credentials required |
| Setting up | `text-status-working` | Provisioning or preparing in progress |
| Error | `text-destructive` | Last action failed; reason shown |
| Disabled | `text-muted-foreground` | Turned off by the user |

### Row rules
- A machine row is name, status label, and **exactly one primary action** (`Button size="sm" h-7
  text-[11px]`); every other action lives in the row's overflow menu.
- **A disabled primary action shows its reason as visible text** in the row
  (`text-[11px] text-muted-foreground`), never in `title` alone.
- **Details and edit open in a right-side Sheet** (`bg-popover`, `border-l border-border`) with a
  focus trap and Escape to close. Opening it never reflows the list; no inline expand.
- Rows follow S6/S7: `border-y border-border` groups with `border-border/40` dividers, no
  tinted borders for state.
- No new tokens. `--destructive` and `--border` are the existing shadcn set (§3 Token
  Alignment); every other value is a §1 token.

## 13. Phone home (supervisor-first)

Applies to the remote web client below 768px when signed in to an account
(`ui/src/remote/home/`). Desktop (>= 768px) and paired-host mode are unchanged.

### Navigation

- Three home destinations behind a bottom tab bar: Supervisor (default after sign-in),
  Inbox, Worktrees. `MobileHomeTabBar`: `role="tablist"`, 56px (`h-14`) plus `.pb-safe`,
  icon above a one-word text label, roving tabindex with Arrow keys. Inbox shows a count
  badge on `bg-status-warning` when sessions wait; the count is also in the tab's
  accessible name.
- Opening a session from any destination shows the Session screen (the existing
  `MobileTopBar` plus chat/terminal body). The tab bar is hidden there so the composer and
  keyboard keep the full height. `MobileTopBar` gains a back control (`mobile-session-back`).
- `usePhoneNavigation` *replaces* the history entry on a tab switch and *pushes* one only on
  entering a session or opening an overlay (§6 History). `popstate` closes the topmost overlay,
  then returns from a session to the destination it came from; back from a home tab leaves the
  app instead of replaying earlier tab taps.

### Surfaces

- Supervisor: header card (`bg-chat-surface-raised`) with a sentence-case status whose unit
  matches the list below it (for example "Watching 3 lanes"), never a bare enum such as
  `off` or `watching 4`, and one sentence. Then one section per machine, with a state note when it is loading, offline,
  not connected, unsupported ("Update Ferryx on this computer to see its supervisor") or
  has the hub stopped. Lane cards show the name (cwd leaf · title), one status slot (paused
  > typing > needs you > hub status), Goal, "Stopped: <reason>", and a 44px Pause/Resume
  button. A typing lane uses `bg-chat-surface-raised` plus a text label and icon (never
  `bg-accent`, which is a desktop token with no light-theme value on the chat palette). Then a
  "Recent" list of the last 3 actions with "Show all"; each row names its computer, opens its
  session, and shows humanised text (no op names, refusal codes or session-id prefixes).
- Inbox: needs-you sessions from every machine. Waiting and supervisor-held come first,
  then finished; each row states why in text ("Waiting for your answer", "Supervisor paused
  this lane", "Finished"). The dot colour only repeats that text.
- Worktrees: the drawer's inventory rendered inline (`RemoteWorkspaceMirror inline`) with
  New terminal and New worktree, plus an Account button. Sign out lives only in the Account
  sheet (one place, confirmed, out of the thumb zone); the sheet holds the email, Sign out and
  Delete account. Deletion asks the person to type their email; the
  server's `CONFIRMATION_MISMATCH` / `SUBSCRIPTION_ACTIVE` answers are shown in text on
  `--chat-danger`.
- Session strip (`SupervisorSessionStrip`) sits under the top bar only for a supervised
  session. Priority follows `paneSupervision`: paused (Resume) > left to you > held >
  acting (quoted sent text on `bg-chat-surface-raised`, "Take over" pauses the lane, then focuses the
  composer or terminal input) > watching (Pause here).

### Test ids

`mobile-home` (`data-destination`), `mobile-home-tabs`, `mobile-home-tab-{supervisor,inbox,
worktrees}`, `mobile-home-inbox-count`, `mobile-home-resume`, `supervisor-home`,
`supervisor-home-headline`, `supervisor-machine-<machineId>`, `supervisor-lane-card`
(`data-lane-key`, `data-machine-id`), `supervisor-lane-pause`, `supervisor-recent-action`,
`inbox-home`, `inbox-row` (`data-machine-id`, `data-session-id`), `worktrees-home`,
`remote-worktrees-inline`, `account-open`, `account-delete-open`, `account-delete-email`,
`account-delete-confirm`, `account-delete-error` (`data-code`), `supervisor-session-strip`
(`data-state`), `supervisor-take-over`, `supervisor-resume`, `mobile-session-back`.

No new tokens: every colour is an existing `chat-*`, `status-*` or `accent` token.
