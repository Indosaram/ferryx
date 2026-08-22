# Orca Lite — UI Design System

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
| `--worktree-sidebar-foreground` | `text-worktree-sidebar-foreground` | Sidebar-local text; `/45`–`/65` alpha for de-emphasis |

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
- `--radius: 0.5rem` → `rounded-lg` / `rounded-md` (`calc(radius - 2px)`) / `rounded-sm`.
  Sidebar rows use `rounded-md`.
- Named spacing: `sidebar: 14.75rem`, `titlebar: 2.25rem`, `tabbar: 2rem`.
- Grid: Tailwind's 0.25rem scale. Sidebar rows are `h-7` (1.75rem); dense sub-rows use `py-1.5`.

### Typography
Single family: **Geist** variable (`font-sans`), self-hosted, weights 100–900.
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

Reusable, already shipped — compose these; do not hand-roll equivalents.
- `ui/IconButton` — square icon control (`size-6` sm / `size-7` md), `aria-label` + `title` required.
- `ui/SectionHeader` — `h-8` section title with optional count pill and trailing actions.
- `ui/StatusDot` — maps `AgentState` to a status glyph; `working` pulses, `motion-reduce` safe.
- `lib/cn` — `twMerge(clsx(...))` for conditional classes. All conditional styling goes through it.

### Tab chrome states
- The tab strip and inactive tabs use `bg-card`.
- The active tab uses `bg-accent text-foreground`, so it follows light/dark appearance settings.
- `bg-terminal` is reserved for the terminal viewport and its pixel-filling wrappers. A tab must never inherit the terminal theme background.

### Terminal split separators
- Horizontal and vertical terminal splits use the opaque `--terminal-divider` color directly on a line exactly 1px thick.
- The visible line remains 1px; a separate transparent hit area extends 4px around it for resizing.
- Do not reuse translucent application `border` tokens here: their built-in alpha makes a 1px line disappear against terminal backgrounds.

## 3. Sidebar information architecture (this feature)

The sidebar presents **one nested tree**, not two disjoint lists:

```
Projects                        [+ add project]  ← SectionHeader, count = projects.length
├─ ▾ maho-workspace             ← project row (folder icon, active)
│    ├─ ● main        [primary] ← worktree row, nested under its project
│    └─ ● feature/api           ← worktree row
└─ ▸ content-intel-dashboard    ← collapsed project row
```

Rules:
1. **Every registered project renders as a project row.** Rows are disclosure controls
   (`aria-expanded`), not plain links.
2. **Worktrees nest under their owning project.** Only the active project has loaded worktrees —
   `useWorkspaceStore` / `useWorkspaceRuntime` are scoped to a single `workspaceId` — so only the
   active project expands. This is an honest reflection of loaded state, not a limitation to hide.
3. **Selecting a collapsed project switches the active project**, which loads its worktrees and
   expands it. Selecting a worktree activates that worktree, switching project first if needed.
4. **Depth is expressed with a guide rail**, not indentation alone: nested worktrees sit behind a
   1px `border-worktree-sidebar-border` vertical rail, which brightens to
   `border-worktree-sidebar-ring` for the active row. Indentation step is `pl-3`.
5. **The `primary` badge** marks the repository's root worktree (the one whose branch is not an
   `orca/<ws>/<slug>` worktree branch). It is a token-colored pill, never a colored emoji.

## 4. Motion

- Only `transition-colors` on hover/active state changes (GPU-composited; no layout animation).
- Disclosure chevrons rotate via `transition-transform` — `rotate-90` when expanded.
- `animate-enter` (140ms `translateY(2px)` + fade) is available for newly revealed groups.
- `StatusDot`'s ping/pulse is the only looping motion and is disabled under `motion-reduce`.
- No hover state may change anything other than color/opacity on a non-interactive element.

## 5. Responsive behavior

The sidebar is user-resizable and persisted (`orca.sidebar.width`), clamped 220–420px, default 236px.
Every row must survive the 220px floor: names `truncate`, badges and action icons `shrink-0`,
metadata lines truncate rather than wrap.

## 6. Accessibility constraints

- The tree uses semantic disclosure: project rows expose `aria-expanded`; the worktree group is
  labelled by its project row via `aria-label`.
- Active worktree is announced with `aria-current="true"`, not color alone.
- Status is never encoded by color alone — the dirty/clean state also carries text.
- All icon-only controls carry `aria-label` (enforced by `IconButton`).
- Focus is visible on every control: `focus-visible:ring-1 focus-visible:ring-ring`.
- Nested action buttons must not be DOM descendants of the row button (no nested interactives).

## 7. Accepted debt

- **Inactive projects show no worktree count.** The client only holds the active workspace's
  worktrees, so a count for collapsed projects would require a backend fan-out (`cmd_worktree_list`
  per project). Deferred; the collapsed chevron communicates "expandable" without promising a number.
- Worktree rows re-derive display names from branch strings on each render. Cheap at realistic
  worktree counts; revisit only if a project exceeds a few hundred worktrees.
