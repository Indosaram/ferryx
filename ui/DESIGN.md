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
- `StatusDot`'s ping/pulse is the only looping motion and is disabled under `motion-reduce`.
- No hover state may change anything other than color/opacity on a non-interactive element.

## 6. Responsive Behavior

The sidebar is user-resizable and persisted (`orca.sidebar.width`), clamped 220-420px, default 236px.
Every row must survive the 220px floor: names `truncate`, badges and action icons `shrink-0`,
metadata lines truncate rather than wrap.

## 7. Accessibility Constraints

- The tree uses semantic disclosure: project rows expose `aria-expanded`; the worktree group is
  labelled by its project row via `aria-label`.
- Active worktree is announced with `aria-current="true"`, not color alone.
- Status is never encoded by color alone; the dirty/clean state also carries text.
- All icon-only controls carry `aria-label` (enforced by `IconButton`).
- Focus is visible on every control: `focus-visible:ring-1 focus-visible:ring-ring`.
- Nested action buttons must not be DOM descendants of the row button (no nested interactives).

## 8. Accepted Debt

- **Inactive projects show no worktree count.** The client only holds the active workspace's
  worktrees, so a count for collapsed projects would require a backend fan-out (`cmd_worktree_list`
  per project). Deferred; the collapsed chevron communicates "expandable" without promising a number.
- Worktree rows re-derive display names from branch strings on each render. Cheap at realistic
  worktree counts; revisit only if a project exceeds a few hundred worktrees.
