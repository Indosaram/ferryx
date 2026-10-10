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
| `text-[9px]` | Tertiary metadata (status line, agent task) |
| `text-[10px]` | Badges (`primary`), footer runtime label |
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
| `--chat-foreground-secondary` `#838383` | `text-chat-foreground-secondary` | Secondary labels, timestamps, metadata |
| `--chat-foreground-tertiary` `#818181` | `text-chat-foreground-tertiary` | Tertiary/de-emphasized text and icons |
| `--chat-border` `#191919` | `border-chat-border` | Standard card and separator border |
| `--chat-primary` `#346bf1` | `bg-chat-primary` / `text-chat-primary` | Primary action buttons and focus accents |
| `--chat-primary-foreground` `#ffffff` | `text-chat-primary-foreground` | Text and icons on `chat-primary` fills (white on #346bf1 = 4.65:1) |
| `--chat-link` `#4a7cf5` | `text-chat-link` | Markdown links in chat prose (5.18:1 on the chat screen; the primary blue is 4.26:1 and fails AA at text size) |
| `--chat-code` `#4bb8f0` | `text-chat-code` / `bg-chat-code` | Code block highlighting and inline code text |
| `--chat-danger` `#ff6467` | `text-chat-danger` / `bg-chat-danger` | Destructive actions, errors, cancellation |
| `--chat-row-hover` `rgba(255,255,255,0.04)` | `bg-chat-row-hover` | Subtle white-alpha hover on chat rows |
| `--chat-composer-panel` `rgba(10,10,10,0.92)` | `bg-chat-composer-panel` | Backdrop panel for mobile composer |
| `--chat-composer-surface` `rgba(26,27,27,0.9)` | `bg-chat-composer-surface` | Translucent input editor surface |
| `--chat-composer-border` `rgba(25,25,25,0.8)` | `border-chat-composer-border` | Translucent composer border |

## 2. Primitives

Reusable, already shipped. Compose these; don't hand-roll equivalents.

### App Primitives
- `ui/IconButton` (`ui/src/components/ui/IconButton.tsx`): Square icon control (`size-6` sm / `size-7` md), `aria-label` and `title` required.
- `ui/SectionHeader` (`ui/src/components/ui/SectionHeader.tsx`): `h-8` section title with optional count pill and trailing actions.
- `ui/StatusDot` (`ui/src/components/ui/StatusDot.tsx`): Maps `AgentState` to a status glyph; `working` pulses, `motion-reduce` safe.
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
Projects                        [+ add project]  <- SectionHeader, count = projects.length
+- v maho-workspace             <- project row (folder icon, active)
|    +- o main        [primary] <- worktree row, nested under its project
|    \- o feature/api           <- worktree row
\- > content-intel-dashboard    <- collapsed project row
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
   1px `border-worktree-sidebar-border` vertical rail, which brightens to
   `border-worktree-sidebar-ring` for the active row. Indentation step is `pl-3`.
5. **The `primary` badge** marks the repository's root worktree (the one whose branch is not an
   `orca/<ws>/<slug>` worktree branch). It is a token-colored pill, never a colored emoji.

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
- The chat terminal drawer opens and closes with a composited opacity/transform transition only (its 45vh/85vh height change is instant, never a layout animation); the chat's looping motion (animate-ping, animate-spin) carries motion-reduce:animate-none; the scroll-to-latest affordance has no looping animation; no hover transform on non-interactive elements.

## 6. Responsive Behavior

The sidebar is user-resizable and persisted (`orca.sidebar.width`), clamped 220-420px, default 236px.
Every row must survive the 220px floor: names `truncate`, badges and action icons `shrink-0`,
metadata lines truncate rather than wrap.

### Mobile chat surface constants
- Drawer heights: `45vh` collapsed / `85vh` expanded.
- Control dimensions: `2.25rem` (`size-9`) composer controls and `1.75rem` (`size-7`) header controls.
- Corner radii: `20px` user-bubble radius, `12px` card radius.
- Typography: `16px` composer editor font (prevents iOS zoom-on-focus).
- Safe-area rule: `ui/index.html` sets `viewport-fit=cover` and the composer uses `.pb-safe`, because `env(safe-area-inset-*)` is 0 without `viewport-fit=cover`.

## 7. Accessibility Constraints

- The tree uses semantic disclosure: project rows expose `aria-expanded`; the worktree group is
  labelled by its project row via `aria-label`.
- Active worktree is announced with `aria-current="true"`, not color alone.
- Status is never encoded by color alone; the dirty/clean state also carries text.
- All icon-only controls carry `aria-label` (enforced by `IconButton`).
- Focus is visible on every control: `focus-visible:ring-1 focus-visible:ring-ring`.
- Nested action buttons must not be DOM descendants of the row button (no nested interactives).
- Every icon-only chat control carries `aria-label`, every chat control carries `focus-visible:ring-1 focus-visible:ring-ring`, the running activity indicator is `role="status"`, a closed terminal drawer is inert and `aria-hidden`.

## 8. Accepted Debt

- **Inactive projects show no worktree count.** The client only holds the active workspace's
  worktrees, so a count for collapsed projects would require a backend fan-out (`cmd_worktree_list`
  per project). Deferred; the collapsed chevron communicates "expandable" without promising a number.
- Worktree rows re-derive display names from branch strings on each render. Cheap at realistic
  worktree counts; revisit only if a project exceeds a few hundred worktrees.
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
