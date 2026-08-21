# rorca original-Orca visual reference

Decision-ready frontend specification for the `rorca` parity pass at the locked **1280×850** reference viewport.

## Evidence and confidence

- **[REF] Reference-recorded** — `.omo/plans/rorca-ui-parity.md` records the supplied screenshot target as 1280×850 with a **236px compact charcoal rail**, **#23262d workspace**, **1px muted separators**, and low-chrome tabs. The delegated workspace rejects direct reads of the supplied `/var/folders/.../clipboard-2026-08-21-113227-8076FA53.png`; this report does not bypass that sandbox. Final GUI QA must still compare the implementation to that PNG.
- **[ORIG] Original Orca source** — `ui/original-dist/**`, especially the exact classes/constants named below.
- **[CUR] Current rorca UI** — `ui/src/**` and `ui/tailwind.config.js`; current values are cited where they identify a concrete parity delta.

The geometry below uses [REF] for screenshot-derived values already captured in the authoritative plan, then fills otherwise-unspecified dimensions from [ORIG].

## 1. Locked 1280×850 geometry

### Workspace shell

| Region | Exact target bounds | Frontend decision | Evidence |
| --- | --- | --- | --- |
| Viewport | `x=0 y=0 w=1280 h=850` | Screenshot QA is performed at this CSS-pixel viewport. | [REF] `.omo/plans/rorca-ui-parity.md` → “Visual and native window behavior”. |
| Project/worktree rail | `x=0 y=0 w=236 h=850` | Default persisted width for the reference capture is **236px**. Keep interactive resizing, but initialize/reset the parity screenshot to 236px. | [REF] plan: “236px compact charcoal rail”. [CUR] `Sidebar.tsx` currently uses `DEFAULT_SIDEBAR_WIDTH = 264`, so 264 is not the target. |
| Rail/main separator | right edge of rail, **1px visible** | Preserve a wider pointer hit target if desired, but render only one muted pixel. | [REF] plan: “1px muted separators”. [CUR] `Sidebar.tsx` currently uses a 6px-wide (`w-1.5`) hover surface; that may remain the hit area, not the visible divider. |
| Main workspace | `x=236 y=0 w=1044 h=850` | No outer card, modal shell, or large-radius frame. | [REF] plan: no oversized cards/rounded modal aesthetics. |
| Workspace/title chrome | `x=236 y=0 w=1044 h=36` | **36px** total height. Drag only the noninteractive background; every control is no-drag. | [ORIG] `workspace-chrome-metrics-CNC6jyKs.js` → `WORKSPACE_TOP_CHROME_HEIGHT = 36`. [CUR] Tailwind `titlebar = 2.5rem` / 40px is 4px too tall. |
| Terminal tab strip | `x=236 y=36 w=1044 h=32` | **32px** low-chrome strip; structural bottom border is 1px. | [ORIG] `Terminal-qm6WvB4Q.js` → tab-group strip `h-[32px] ... border-b border-border bg-card`. [CUR] Tailwind `tabbar = 2.25rem` / 36px is 4px too tall. |
| Terminal canvas | `x=236 y=68 w=1044 h=782` | Terminal fills to the bottom in rorca; do not add an unused status bar just to mimic an unrelated original-Orca surface. | Derived from 850 − 36 − 32. [CUR] rorca has no status bar. [ORIG] `STATUS_BAR_RESERVE_HEIGHT = 24` exists in `workspace-chrome-metrics-CNC6jyKs.js`, but is not an in-scope rorca requirement. |
| Split divider | pane boundary, **1px visible** | Visible divider stays 1px; retain at least a ~6px transparent drag hit area. Focus/hover may use `#71717a`. | [REF] 1px muted separator. [ORIG] dark token `--tab-group-split-divider: #71717a` in `I18nProvider-TFirEJhb.css`. [CUR] `TerminalSplitView.tsx` currently paints the whole `w-1.5`/`h-1.5` surface. |

If a later product decision explicitly restores an original-style status bar, [ORIG] reserves **24px**, yielding terminal `y=68..825` (`h=758`) and status `y=826..849` (`h=24`). That is **not** the parity target for this frontend pass.

### Settings shell

Settings replaces the workspace content as a **full view**, not a centered modal.

| Region | Exact target bounds at 1280×850 | Decision | Evidence |
| --- | --- | --- | --- |
| Settings nav | `x=0 y=0 w=280 h=850` | Dedicated settings sidebar, charcoal rail styling, 1px right divider. | [ORIG] `Settings-yKTVxZPa.js` → `SettingsSidebar`: `w-[280px] ... border-r ... bg-worktree-sidebar`. |
| Settings detail viewport | `x=280 y=0 w=1000 h=850` | Scroll detail only; do not backdrop or float it over the workspace. | [ORIG] `Settings()` → `settings-view-shell flex ... overflow-hidden bg-background`, followed by sidebar + detail flex column. |
| Centered detail column | approximately `x=332..1228`, max `w=896` | `max-w-4xl`, centered. | [ORIG] `Settings()` → `mx-auto ... px-8 pt-10 ... max-w-4xl`. Tailwind `4xl = 56rem = 896px`. |
| Detail outer padding | `32px` left/right; `40px` top | First heading baseline begins from this compact, spacious grid, not a dialog header. | [ORIG] same `px-8 pt-10`. |
| Effective inner content span | approximately `x=364..1196` | 32px internal horizontal padding inside the centered 896px column. | Derived from the original `max-w-4xl` + `px-8` layout. |

[CUR] `SettingsDialog.tsx` is currently a `max-w-xl`, `rounded-xl`, centered modal over `bg-black/60 backdrop-blur-sm`; all four traits are intentional parity deltas.

## 2. Visual tokens

Use the existing original-Orca dark palette rather than introducing a new gray scale. The current rorca CSS already contains most of these exact values.

| Token / role | Exact value | Usage | Evidence |
| --- | --- | --- | --- |
| `--background` | `#0a0a0a` | Terminal/deepest surface and settings detail base. | [ORIG] `.dark` token block in `I18nProvider-TFirEJhb.css`; [CUR] `index.css`. |
| `--workspace-reference` | `#23262d` | Main workspace surround/gutters where terminal/chrome do not cover the surface. | [REF] plan explicitly records `#23262d workspace background`. |
| `--card` / chrome | `#171717` | Header/tab chrome and compact bounded control surfaces. | [ORIG] dark token block; [CUR] `index.css`. |
| `--worktree-sidebar` | `#2a2a2a` | 236px normal rail and 280px Settings nav. | [ORIG] dark token block; [CUR] `index.css`. |
| `--worktree-sidebar-accent` | `#353535` | Active/hover rail row. | [ORIG] dark token block; [CUR] `index.css`. |
| `--muted` | `#262626` | Quiet controls and grouped settings surfaces. | [ORIG] dark token block; [CUR] `index.css`. |
| `--accent` | `#404040` | Strong hover/active control state, sparingly. | [ORIG] dark token block; [CUR] `index.css`. |
| `--foreground` | `#fafafa` | Primary text/icon. | [ORIG] dark token block; [CUR] `index.css`. |
| `--muted-foreground` | `#a1a1a1` | Secondary metadata, labels, descriptions. | [ORIG] dark token block; [CUR] `index.css`. |
| `--border` | `#ffffff12` | All structural 1px separators. | [ORIG] dark token block; [CUR] `index.css`. |
| `--input` | `#ffffff26` | Input outline only. | [ORIG] dark token block; [CUR] `index.css`. |
| `--ring` | `#737373` | Keyboard focus and resize focus. | [ORIG] dark token block; [CUR] `index.css`. |
| split focus | `#71717a` | Focus/hover emphasis on a split divider, not its idle fill. | [ORIG] `--tab-group-split-divider`. |
| success | `#86efac` | Runtime/clean status. | [ORIG] dark token block; [CUR] `index.css`. |
| destructive | `#ff6568` | Delete/error only. | [ORIG] dark token block; [CUR] `index.css`. |

### Type

- **UI family:** Geist first, then system sans. [CUR] `index.css` embeds `Geist`; this is already compatible with the dense original-Orca style.
- **Primary nav/row text:** **13px**, medium only for active/primary labels. [ORIG] `SettingsSidebar` nav items use `text-[13px]`; current rail/worktree primary labels are also 13px.
- **Tab/project chrome text:** **12px**. [CUR] `TabBar.tsx` uses `text-xs`; `WorktreeList.tsx` project header uses 12px.
- **Micro labels/group headings:** **11px**; uppercase labels may use original `tracking-[0.18em]`. [ORIG] Settings group labels are 11px uppercase.
- **Branch/path/status metadata:** **10px**, mono for branch/path values. [CUR] `WorkspaceHeader.tsx` and `WorktreeList.tsx` already use 10px mono branch labels.
- **Settings page title:** **24px / 600**, tight line height; description **14px / 24px**. [ORIG] `SettingsSection` uses `text-2xl font-semibold leading-tight` and `text-sm leading-6`.
- **Terminal fallback:** 13px with `lineHeight: 1.2`. Effective `fontFamily` must come from rorca local override → native Ghostty import → safe fallback, in that precedence. [CUR] `terminalSettings.ts` default font size is 13; `TerminalPane.tsx` uses `lineHeight: 1.2`. [REF] plan defines Ghostty precedence/application.

### Spacing, radius, borders

- Use a **4px base grid**: 4, 8, 12, 16, 24, 32, 40. This matches original/current utility usage (`gap-1`, `gap-2`, `px-3`, `p-4`, `px-8`, `pt-10`).
- Base radius is **10px** (`0.625rem`), with **8px** medium and **6px** small. [ORIG] `I18nProvider-TFirEJhb.css` → `--radius: .625rem`; [CUR] Tailwind computes `md = radius − 2px`, `sm = radius − 4px`.
- Structural shell edges are **square**. Use 6–8px radii for compact rows/buttons and 10px only for genuinely bounded groups. Do **not** place the rail, main workspace, terminal, tab strip, or entire Settings view inside rounded cards. [REF] “no oversized cards/rounded modal aesthetics”.
- All shell/header/tab/nav/group separators are **1px** and `#ffffff12` at idle. Do not render a 6px solid resize bar.
- Standard chrome icon buttons are **28×28px** with 6px radius. [ORIG] `Terminal-qm6WvB4Q.js` → `menuButtonClassName` is `h-7 w-7 rounded-md`.

## 3. Workspace composition decisions

### Rail — 236px default

1. Keep the hierarchy **Workspace → Search → project/worktrees → runtime/settings**.
2. Make the rail dense: 8–12px horizontal padding, 28–32px primary rows, 13px primary labels, 10px secondary branch/status text.
3. Avoid the current tall “card per worktree” treatment. An active row may use `#353535` plus a subtle 1px/ring state; inactive rows should normally have no visible card border.
4. Sidebar resizing remains functional/persistent, but the baseline/reference capture must be reset to **236px**.

Evidence: [REF] compact 236px rail; [ORIG] worktree-sidebar palette; [CUR] `WorktreeList.tsx` currently adds multi-line `py-2` cards and `Sidebar.tsx` defaults to 264px.

### Workspace header — 36px

- One 36px strip, 1px bottom separator, compact worktree/project identity at the left.
- Keep the branch as quiet 10px secondary metadata only if it fits without increasing height.
- The noninteractive background is the Tauri drag surface; every icon button, field, tab, split handle, menu and resize handle is no-drag.
- Remove the current **global `Interrupt terminal`** control. Split/terminal actions belong to the pane/tab-group chrome.

Evidence: [ORIG] `WORKSPACE_TOP_CHROME_HEIGHT = 36`; [REF] titlebar drag/no-drag decision and pane-local controls; [CUR] `WorkspaceHeader.tsx` currently exposes global Split + Interrupt controls.

### Tab/pane strip — 32px

- Each terminal pane owns its own **32px** tab-group strip when split.
- Tabs are low-chrome rectangular segments, not pills/cards. Use 12px labels, 8px horizontal padding, 1px structural edges, and contrast/fill for the active tab. Keep any active underline to **≤1px**; avoid the current prominent 2px white rule.
- Pane action buttons are 28×28px and appear in the active/focused pane. Put split/unsplit/menu affordances here.
- Close affordance may remain hover-revealed; no independent tall toolbar may be introduced above or below this strip.

Evidence: [ORIG] `Terminal-qm6WvB4Q.js` tab group uses `h-[32px]`, `pr-1.5`, focused pane actions, and 28×28 menu buttons; [REF] pane-local split/menu decision; [CUR] `TabBar.tsx` is currently 36px with a 2px active rule.

### Terminal canvas

- Terminal surface remains `#0a0a0a`; do not wrap it in a card or add padding that changes xterm geometry.
- Keep current neutral terminal theme direction: foreground `#d4d4d4`, cursor `#e5e5e5`, selection `#52525299` unless native/effective theme work explicitly supplies a real value.
- At split boundaries, render one visible pixel and use an invisible larger hit target for resizing.
- Ordinary typing and Ctrl-C remain terminal-owned; visual chrome must not imply a global interrupt action.

Evidence: [CUR] `TerminalPane.tsx` terminal theme; [REF] terminal shortcut/interrupt decisions; [ORIG] split-divider token.

## 4. Settings information architecture

Use the original-Orca **full-page sidebar + detail** pattern, reduced to the real rorca feature set. Do not reproduce original sections that rorca cannot actually support.

### Left nav

Top to bottom:

1. **Back to app** — 13px compact row.
2. **Search settings** — optional but, if present, use the original 13px compact search input.
3. Four real navigation rows:
   - **General** — contains the in-scope **Appearance** subsection rather than manufacturing a fifth top-level page.
   - **Terminal**
   - **Keyboard Shortcuts**
   - **Workspace**

Nav row contract: 13px, 12px horizontal padding, 6px vertical padding, 8–10px radius; active state `#353535` + subtle ring; inactive text is approximately 60% foreground.

Evidence: [ORIG] `SettingsSidebar` is 280px and uses `rounded-lg px-3 py-1.5 text-[13px]`, active `bg-worktree-sidebar-accent ... ring-1`; original nav metadata has `General`, `Terminal`, `Appearance`, and `Shortcuts`; [REF] explicitly scopes `General/Appearance, Terminal, Keyboard Shortcuts, Workspace`.

### General / Appearance

Only surface settings backed by real state. Put **Appearance** first inside General. Appropriate rows are the actual theme/rail/window presentation controls available to rorca; do not copy original Orca tray/mobile/editor options just for visual density.

Row style: subsection label/header, then restrained divide-y rows (`border-y` / `divide-border/40`) rather than separate rounded cards for every setting.

Evidence: [ORIG] `AppearancePane` and `SettingsSection`; [REF] functional-not-decorative settings requirement.

### Terminal

Required visible information/controls:

- **Font Family** — show the effective family actually applied to xterm.
- **Font Size** — fallback remains 13px unless imported/overridden.
- **Scrollback** — preserve the existing real control.
- **macOS Option as Alt** — surface the native effective preference where supported.
- **Ghostty source/status** — visibly identify imported config source/status and precedence; never display a fabricated imported value.
- **Import from Ghostty** action only when backed by the native bridge.

Precedence shown in the UI must be: **rorca local override → parsed Ghostty value → safe fallback**. Repeated Ghostty `font-family` keys are last-wins, as specified by the native contract plan.

Evidence: [ORIG] `TerminalAppearanceSection` has “Terminal Typography”, “Font Family”, and “Import from Ghostty”; `GhosttyImportModal` exposes config(s), import status, unsupported keys and no-config state. [REF] defines the native Ghostty precedence, `font-family → xterm fontFamily`, `macos-option-as-alt`, and source/status requirement. [CUR] `terminalSettings.ts` currently only models `fontSize` + `scrollback`, identifying the gap.

### Keyboard Shortcuts

List only registered rorca actions and their actual chords:

- new terminal tab
- close tab
- previous tab
- next tab
- split right
- split down
- unsplit
- command palette

Use compact grouped rows with 13px action title and a 10–11px mono/kbd chord at the right. If rebinding is not actually implemented, keep this page read-only rather than drawing fake record/edit controls.

Evidence: [REF] required shortcut list; [ORIG] `ShortcutsPane` detail title is “Keyboard Shortcuts” and uses a row/list surface; [CUR] `shortcuts.ts`/`SettingsDialog.tsx` already expose registered shortcut metadata.

### Workspace

Keep this page about real local workspace/project state only: project registration/add flow, selected project/worktree defaults, and persisted rail/workspace preferences that the frontend can actually mutate. The **Add Project** and **Add Worktree** flows stay first-class app actions; branch selection must be a real local-branch combobox, not decorative text input.

Evidence: [REF] Project/worktree UX and in-scope Workspace settings decision; [ORIG] Settings sidebar has a real `Projects` section derived from repos rather than static fake rows; [CUR] current create-worktree UI still uses arbitrary branch text input.

## 5. Screenshot target regions

Capture and compare these regions independently at 1280×850 so a terminal repaint does not obscure shell regressions:

| ID | Crop | What must match |
| --- | --- | --- |
| `R1-rail` | `(0,0)–(236,850)` | Width, charcoal surface, compact row density, active row, 1px right edge, bottom settings/runtime chrome. |
| `R2-header` | `(236,0)–(1280,36)` | Exact 36px height, drag-safe empty background, compact identity, no global Interrupt. |
| `R3-tabs` | `(236,36)–(1280,68)` | Exact 32px strip, low-chrome tabs, 28px pane actions, 1px separator. |
| `R4-terminal` | `(236,68)–(1280,850)` | Terminal reaches bottom; no accidental outer padding/card; 1px split divider if split. |
| `R5-settings-nav` | `(0,0)–(280,850)` while Settings is open | Full-height 280px nav, Back/Search, four compact real sections, 1px divider. |
| `R6-settings-detail` | `(280,0)–(1280,850)` while Settings is open | No modal/backdrop; centered max-896px detail column, 32px horizontal + 40px top spacing, restrained row groups. |

The frontend worker should also retain one full 1280×850 shell screenshot and one full Settings screenshot for the final comparison artifact.

## 6. Acceptance checks

### Geometry

- At CSS viewport **1280×850**, normal shell computes to: rail **236px**, header **36px**, tab strip **32px**, terminal top **68px**, terminal bottom **850px**.
- Structural edge displacement from those reference coordinates is **≤1 CSS px**.
- Settings computes to nav **280px** and detail **1000px**; normal detail content uses `max-width: 896px`, `padding-inline: 32px`, `padding-top: 40px`.
- Visible rail, header, tab, settings-nav and split separators are **1px**, even when pointer hit targets are larger.

### Color/type/chrome

- Computed idle colors match the token table: rail `#2a2a2a`, rail active `#353535`, chrome `#171717`, terminal `#0a0a0a`, workspace-reference surface `#23262d`, foreground `#fafafa`, muted foreground `#a1a1a1`, border `#ffffff12`.
- Shell structural surfaces have **no large-radius wrapper**. Compact controls stay within the 6/8/10px radius scale.
- UI uses Geist/system sans; rail/nav primary labels are 13px, tabs 12px, micro/metadata 10–11px, Settings title 24px.
- Pane/chrome icon buttons are **28×28px** where present.

### Functional visual state

- There is **no global “Interrupt terminal”** button in the workspace header.
- Split/menu actions are available from the relevant terminal pane/tab-group chrome and remain no-drag.
- Settings is a **full view**, not a modal overlay; no black backdrop or blur is visible behind it.
- Settings nav exposes only **General, Terminal, Keyboard Shortcuts, Workspace** (with Appearance inside General) unless a newly implemented real section is explicitly added to scope.
- Terminal settings visibly distinguish local override, Ghostty-imported value/status, and fallback; the displayed font family equals the value applied to xterm.
- Keyboard Shortcuts shows the registered chord for every required action; no decorative rebind UI is shown unless rebinding works.
- Workspace/project UI never presents arbitrary branch text as the intended branch-selection control; the real branch combobox is the accepted surface.

### Final visual proof

1. Capture normal shell and Settings at exactly **1280×850**.
2. Verify `R1`–`R6` independently, then inspect the full frames for overall density.
3. Fail the visual gate for any >1px locked-edge shift, 264/40/36 legacy geometry, modal Settings shell, thick structural divider, oversized worktree cards, global Interrupt, or nonfunctional/fake settings rows.
4. Compare the final full shell against the supplied reference PNG; the PNG comparison remains mandatory because the delegation harness could not directly read that external path.

## 7. Concrete source map

- `.omo/plans/rorca-ui-parity.md` — authoritative recorded reference dimensions/colors, titlebar behavior, pane-local controls, settings scope, Ghostty semantics.
- `ui/original-dist/assets/workspace-chrome-metrics-CNC6jyKs.js` — 36px top chrome, 24px optional original status reserve.
- `ui/original-dist/assets/Terminal-qm6WvB4Q.js` — 32px tab-group strip, focused pane actions, 28×28 menu chrome.
- `ui/original-dist/assets/I18nProvider-TFirEJhb.css` — exact dark palette, 10px base radius, sidebar colors, split-divider token.
- `ui/original-dist/assets/Settings-yKTVxZPa.js` — 280px Settings sidebar, nav/search classes, `max-w-4xl px-8 pt-10` detail layout, `SettingsSection`, `TerminalAppearanceSection`, Ghostty import surface, `ShortcutsPane`.
- `ui/original-dist/assets/useSettingsNavigationMetadata-CkcKmDGe.js` — original General/Terminal/Appearance/Shortcuts titles and descriptions.
- `ui/src/components/Sidebar.tsx` + `ui/tailwind.config.js` — current 264px/40px/36px geometry that must move to the locked target.
- `ui/src/components/WorkspaceHeader.tsx` — current global Split/Interrupt mismatch.
- `ui/src/components/TabBar.tsx` — current 36px/2px-indicator mismatch.
- `ui/src/components/TerminalSplitView.tsx` + `TerminalPane.tsx` — current splitter hit surface and terminal palette/renderer behavior.
- `ui/src/components/SettingsDialog.tsx` + `ui/src/lib/terminalSettings.ts` — current modal settings structure and limited terminal settings model that must be replaced/extended by the frontend parity worker.
