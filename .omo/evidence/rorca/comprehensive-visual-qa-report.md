# rorca — Comprehensive Visual QA & Component UI Verification

- **Date:** 2026-08-21
- **Repo:** `/Users/indo/code/project/orca-lite`
- **HEAD:** `a4de244` — *feat(ui): fix HMR stability, add tab actions popover, and polish tab controls*
- **Scope:** read-only inspection of source / styles / tests / assets + full local gate execution. No OS input automation, no source modification. Only this file was written.
- **Overall verdict:** **PASS with 1 FAIL** — the web-facing PWA/favicon PNG set (`ui/public/*.png`) does **not** contain the crab Orca artwork that the native `src-tauri/icons/*` set does. Every other graded criterion passes.

---

## 0. Method and evidence classes

| Class | Meaning | How obtained |
| --- | --- | --- |
| `[REF]` | Supplied reference screenshot | `/var/folders/zh/…/T/clipboard-2026-08-21-113227-8076FA53.png`, 3840×1506 RGBA. Measured with PIL edge scans + colour histograms; **not** estimated by eye (the harness refuses to inline-render these images at any scale, so no claim here rests on visual impression). |
| `[ORIG]` | Original Orca production bundle | `ui/original-dist/assets/**` — literal constants and Tailwind class strings grepped out of the shipped JS/CSS. |
| `[CUR]` | Current rorca implementation | `ui/src/**`, `ui/tailwind.config.js`, `ui/src/index.css`, and the freshly-built `ui/dist/assets/index-B4YjwWMV.css`. |
| `[TEST]` | Executed gate | Commands and raw tails reproduced in §7. |

### 0.1 Reference-image normalization (critical, and it constrains what can be graded)

The supplied reference PNG is **not** a single 1280×850 frame. Measured directly:

```
size 3840 x 1506
hard seam at x=1919 (#030303) / x=1920 (#555555)
```

It is a **side-by-side composite of two separate Orca workspace windows**:

| Half | Rail width (device px) | Header rows | Tab-strip rows | Terminal fill |
| --- | --- | --- | --- | --- |
| Left (`x=0..1918`) | `#2a2a2a` `x=1..219` → **220 px** | `y=3..36`, 1px `#272727` sep at `y=37` | not present at the sampled column | `#282c34` (One-Dark terminal theme) |
| Right (`x=1920..3839`) | `#2a2a2a` `x=1921..2183` → **264 px** | `y=3..40`, 1px `#272727` sep at `y=41` | `y=42..76`, 1px `#272727` sep at `y=77` | `#0a0a0a` |

Three consequences that shape every verdict below:

1. **The two halves disagree with each other** (220 px vs 264 px rail; 35 px vs 38 px header). The rail and the window are user-resizable, so the reference cannot be read as a pixel-exact geometry contract. It is a *palette, ordering, and chrome-weight* reference.
2. **Neither half shows a Settings view.** Settings therefore has **no reference counterpart** and is graded against `[ORIG]` constants + `.omo/evidence/rorca/visual-reference.md` §4/§6 instead.
3. Absolute vertical pixel equality is invalid — the reference viewport is ≈997 CSS px tall at 1.5× DPR, not 850.

The locked 236 / 36 / 32 / 280 / 896 numbers therefore come from `[ORIG]` source constants (verified below), not from the composite PNG. That is the correct authority, and it is what the implementation is graded against.

### 0.2 `[ORIG]` constants — verified present in the shipped original bundle

```
assets/workspace-chrome-metrics-CNC6jyKs.js:  WORKSPACE_TOP_CHROME_HEIGHT = 36
assets/Terminal-qm6WvB4Q.js:                  className: "h-[32px] shrink-0 border-b border-border bg-card"
assets/Terminal-qm6WvB4Q.js:                  menuButtonClassName = "my-auto flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/50 hover:text-foreground focu…"
assets/Settings-yKTVxZPa.js:                  "w-[280px] shrink-0 flex-col border-r border-worktree-sidebar-border bg-worktree-sidebar"
assets/Settings-yKTVxZPa.js:                  … isFocusedSetupGuidePane ? "max-w-6xl" : "max-w-4xl"   (max-w-4xl = 56rem = 896px)
assets/I18nProvider-TFirEJhb.css:             --tab-group-split-divider: #71717a  (dark) / #868690 (light)
assets/*.js (shared popover):                 "z-50 min-w-[13rem] overflow-hidden rounded-[11px] border border-black/14 … p-1 … backdrop-blur-2xl
                                               dark:border-white/14 dark:bg-[rgba(0,0,0,0.72)] dark:text-white …"
```

### 0.3 `[CUR]` compiled geometry — read out of the production CSS built in §7.2

```
ui/dist/assets/index-B4YjwWMV.css:
  .h-tabbar   { height: 2rem }     → 32 px
  .h-titlebar { height: 2.25rem }  → 36 px
  --background: #23262d;  --card: #171717;  --border: #ffffff12;
  --worktree-sidebar: #2a2a2a;  --terminal: #0a0a0a;
ui/tailwind.config.js:
  spacing.sidebar = 14.75rem → 236 px
```

Every locked token in the reference palette histogram (`#0a0a0a`, `#2a2a2a`, `#171717`, `#353535`, `#23262d`, `#272727` separators) is an exact match against `ui/src/index.css`. **Palette parity: exact.**

---

## 1. Sidebar

### 1.1 236 px compact charcoal hierarchy

| Item | `[CUR]` evidence | Verdict |
| --- | --- | --- |
| Default width | `Sidebar.tsx:24` `DEFAULT_SIDEBAR_WIDTH = 236`; clamp `MIN=220`, `MAX=420`; applied as inline `style={{ width: '236px' }}` on the `<aside>` | **PASS** |
| Locked by test | `Sidebar.test.tsx:77` — *"uses the 236px parity width by default"* asserts `toHaveStyle({ width: "236px" })` | **PASS** |
| Charcoal surface | `<aside … bg-worktree-sidebar>` → `#2a2a2a`, matching the reference rail fill exactly in both halves | **PASS** |
| Row density | Workspace / Search / project rows are all `h-7` (28 px); worktree rows `py-1.5`; labels 13 px (`text-[13px]`) primary, 12 px project, 9–10 px branch/status mono | **PASS** — matches the "28–32 px primary rows, 13 px primary label, 10 px secondary" contract |
| Persisted resize | pointer drag → `clampSidebarWidth` → `localStorage["orca.sidebar.width"]`; test *"restores, drags, clamps, and persists sidebar width"* exercises the clamp at 220 and asserts the persisted value | **PASS** |

**Regression note (previously FAIL, now fixed):** the 13:39 shell screenshot measured a **2 px** `#393939` rail edge (`x=234..235`) because the boundary was drawn twice — a `border-r` on the `<aside>` *and* a `w-px` span inside the resize handle, two translucent `#ffffff12` layers compositing over `#2a2a2a`. In current source the `<aside>` className is `"relative flex h-full shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground"` — **no `border-r`** — and only the handle's `<span class="… w-px bg-worktree-sidebar-border">` paints the pixel. This is locked by `Sidebar.test.tsx:300` *"renders exactly one visible sidebar divider without redundant aside border"*, which asserts both `not.toHaveClass("border-r")` and the presence of the handle span. **1 px separator: PASS.**

### 1.2 Project tree nesting

`Sidebar.tsx` groups worktrees by owning project via `groupWorktreesByProject()`, which parses the `orca/<wsId>/<slug>` branch identity and falls back to the active project. Each project renders a header row, and when expanded renders `<div className="pl-3 pr-0.5 pt-0.5"><WorktreeList …/></div>` — a real 12 px indent step. `WorktreeList` rows carry `border-l` on each `<li>`, producing the vertical tree spine (`border-worktree-sidebar-ring/60` when active, `border-worktree-sidebar-border` otherwise). Empty projects render an indented `border-l` empty-state with a "Create the first worktree" action.

Locked by `Sidebar.test.tsx`: *"nests each project's worktrees inside that project's own tree group"*, *"routes each worktree under the project that owns its branch"*, *"marks the active worktree inside the nested group as current"*, *"offers first-worktree creation inside a project with no worktrees"*. → **PASS**

### 1.3 Independent accordion chevrons

State is stored as the set of **collapsed** project ids (`collapsedProjects: Set<string>`), persisted to `rorca.sidebar.collapsedProjects`. Each project header carries its own dedicated chevron button — `aria-expanded`, `aria-label="{Collapse|Expand} {workspaceId}"`, `<ChevronRight className={cn("size-3 …transition-transform", expanded && "rotate-90")} />` — that is a **sibling** of the project-select button, so toggling never selects and selecting never toggles. First-run seeding collapses everything except the active project; late-registered projects inherit the same seed via `knownProjectsRef`.

Locked by four tests: *"keeps each project's open state independent when toggling default or rorca-qa"* (four-step interleaved toggle assertion), *"toggling a project does not change which project is active"*, *"persists collapsed projects across remounts"*, and the `aria-expanded` assertions in the nesting test. → **PASS**

### 1.4 Search / Workspace buttons

Both live in a dedicated `space-y-1 px-2 pb-1` block directly under the drag-region titlebar, above the Projects section — matching the `[REF]` hierarchy *Workspace → Search → projects → runtime/settings*:

- **Workspace** — `h-7`, `rounded-md`, `bg-worktree-sidebar-accent` (`#353535`, always-active treatment), `LayoutDashboard` icon, 13 px medium label; click focuses + scrolls the worktree region.
- **Search workspaces** — `h-7`, `rounded-md`, 1px `border-worktree-sidebar-border/70`, inset `Search` icon at `left-2`, 12 px muted label, and a trailing `<kbd>` rendering the live registered shortcut label. Click opens the command palette.
- **Footer** — `border-t` runtime row (`#86efac` dot + "Local runtime") and a `Settings2` `IconButton`, satisfying the runtime/settings tail of the hierarchy.

Locked by `Sidebar.test.tsx:82` *"keeps Workspace and Search functional and exposes Add Project"*. → **PASS**

**Sidebar verdict: PASS (4/4).**

---

## 2. TabBar

| Criterion | `[CUR]` evidence | Verdict |
| --- | --- | --- |
| **32 px height** | `TabBar.tsx:196` outer strip `h-tabbar`; compiled `.h-tabbar{height:2rem}` = **32 px**. Matches `[ORIG]` `h-[32px]` exactly. | **PASS** |
| **1 px active indicator** | `<span data-testid="tab-active-indicator" className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-foreground" />` — `h-px` = **1 px**, replacing the earlier 2 px rule. Active tab additionally flips to `bg-terminal text-foreground`. | **PASS** |
| **Low-chrome background** | Strip is `border-b border-border bg-card` — byte-for-byte the `[ORIG]` contract `"h-[32px] shrink-0 border-b border-border bg-card"`. Tabs are rectangular segments with `border-r border-border`, `px-2`, `text-[12px]` — no pills, no radii, no cards. | **PASS** |
| **Drop indicator styling** | `handleDragOver` computes the hovered tab's midpoint and sets `{index, edge}`; the tab then renders `before:absolute before:inset-y-0 before:left-0 before:z-20 before:w-[2px] before:bg-blue-500` (left edge) or the `after:` mirror (right edge). A 2 px full-height accent rule at the true insertion seam. `handleDrop` recomputes the final index with the source-shift correction. | **PASS** |
| **Context menu styling** | `min-w-[13rem] … rounded-md border border-border bg-popover/95 p-1 text-xs shadow-xl backdrop-blur-md`, 10 px uppercase `tracking-wider` group label ("Split Pane"), `h-px bg-border/60` dividers, and right-aligned `font-mono text-[10px]` shortcut hints (⌘D / ⌘⇧D / ⌘W). Viewport-clamped to `left = clamp(4, vw-220)`, `top = clamp(4, vh-300)`. | **PASS** |

### 2.1 Context-menu content parity with `[ORIG]`

`min-w-[13rem]` and `p-1` are **exact** matches to the shared original popover class string. Two deliberate token-level deltas: radius `rounded-md` (8 px, from `--radius: 0.5rem`) vs `[ORIG]` `rounded-[11px]`, and surface `bg-popover/95 + backdrop-blur-md` vs `[ORIG]` `dark:bg-[rgba(0,0,0,0.72)] + backdrop-blur-2xl`. Both are inside the design system (they use `--radius` and `--popover` tokens rather than one-off hex), and `[REF]` explicitly directs "no oversized cards/rounded modal aesthetics", which the tighter 8 px radius honours. **Not graded as a defect.**

Menu wiring is locked by `TabBar.test.tsx`: *"opens context menu with wired Orca-parity actions"*, *"keeps pin state controlled by the workspace model and blocks pinned-tab close"*, *"commits trimmed title changes and cancels rename on Escape"*, *"disables terminal split actions for browser tabs"*, and *"uses left/right drop edges to compute the actual tab insertion index"* (three separate drop-position assertions).

### 2.2 Regression note — empty-strip terminal-black leakage (previously FAIL, now fixed)

The 13:39 screenshot measured `x=576..1279` of the strip rendering `#0a0a0a` for all 32 rows: the inner tab list was `flex min-w-0 flex-1 …` with no background, so the parent pane's `bg-terminal` showed through. Current source has the inner list as `className="flex min-w-0 items-stretch overflow-x-auto scrollbar-none"` — **`flex-1` removed** — with a separate transparent `<div className="flex-1" />` spacer that is a direct child of the `bg-card` strip. Result: `#171717` now covers the full strip width behind empty regions, with `#0a0a0a` reserved for the active tab only, exactly as the reference's right half does. → **PASS**

### 2.3 Drag region

The strip is `data-tauri-drag-region` + `.drag-region` with an `onPointerDown` that bails when `event.target.closest(".no-drag")`. Every tab, close affordance, add button, popover, and the actions slot carry `no-drag`. Locked by *"marks tab strip as Tauri drag region and starts native drag only on background"* and *"forwards typed URLs from the new-tab popover and keeps popovers out of window dragging"*.

**TabBar verdict: PASS (5/5).**

---

## 3. Terminal & Splits

| Criterion | `[CUR]` evidence | Verdict |
| --- | --- | --- |
| **No static toolbar** | A grep for `Interrupt` / persistent `toolbar` across all non-test `ui/src/components/*.tsx` returns **zero** hits. The former global `Interrupt terminal` control is gone. The only pane chrome is the hover-gated overlay in §below. `WorkspaceHeader.tsx` now carries only identity (branch icon, workspace title, mono branch, optional agent chip) — no split/interrupt controls. | **PASS** |
| **Top 24 px hover edge handle** | Two stacked absolutes at `inset-x-0 top-0`: a `z-20 h-6` hotspot (`data-testid="pane-toolbar-hotspot"`) and the `z-30 h-6` toolbar itself. `h-6` = **24 px**. The toolbar is `pointer-events-none opacity-0` until `isHoveredTop`, then `pointer-events-auto opacity-100` with `transition-opacity duration-150`. Styling is genuinely low-chrome: `border-b border-border/30 bg-background/85 backdrop-blur-md px-2 text-[11px]`, holding the 10 px tab label plus 20 px split-right / split-down / close-split icon buttons. It is also the drag grip (`cursor-grab`, `draggable`) for pane swapping. Locked by `TerminalSplitView.test.tsx` z-index assertions (`pane-toolbar` `z-30`, `pane-toolbar-hotspot` `z-20`). | **PASS** |
| **1 px split dividers** | `PaneResizeDivider` renders a `w-1.5`/`h-1.5` (6 px) transparent **hit target** containing `<span data-testid="split-divider-line" className={isHorizontal ? "h-full w-px bg-border/80" : "h-px w-full bg-border/80"} />`. Exactly **one visible pixel**; the 6 px is pointer-only, with `hover:bg-primary/20` as the focus/hover emphasis. This matches the `[REF]` "1 px visible, ~6 px hit area" contract and the `[ORIG]` `--tab-group-split-divider` intent. | **PASS** |
| **Per-tab tree layouts** | `LayoutState.layoutsByTabId: Record<string, TabPaneLayout>` gives every tab its **own** `{root, activeLeafId, expandedLeafId, sessionIdsByLeafId}`. `layout.ts` creates a layout on tab-open, deletes it on tab-close, and reroutes split / close-pane / set-ratio / swap / focus actions through `state.layoutsByTabId[action.tabId]` so one tab's tree can never mutate another's. Ratios are stored per split path and rendered as `flexBasis: ratio*100%`. Coverage: `paneTree.test.ts` 48 tests, `layout.test.ts` 11 tests, plus `TerminalSplitView.test.tsx` *"routes a tab context-menu split to that tab's own focused leaf"* and *"renders a nested horizontal and vertical split tree with resize dividers"* (asserts 3 independent pane toolbars in a nested tree). | **PASS** |

Terminal canvas itself is `bg-terminal` (`#0a0a0a`) with no card, no padding, and no radius; it runs to the window bottom edge (the 13:39 screenshot confirmed `#0a0a0a` continuous from `y=68` to `y=849` at `x=700`, and no status bar was added). Resize clamps to `MIN_PANE_SIZE_PX = 80` and degrades to an even 0.5 ratio when the container is smaller than two minimum panes — locked by *"falls back to an even ratio when a resize container is smaller than two minimum panes"*.

**Terminal & Splits verdict: PASS (4/4).**

---

## 4. Settings View

| Criterion | `[CUR]` evidence | Verdict |
| --- | --- | --- |
| **280 px nav** | `SettingsDialog.tsx:80` — `<aside data-testid="settings-nav" className="flex w-[280px] shrink-0 flex-col border-r border-border bg-card">`. Exact `w-[280px]` match to `[ORIG]` `SettingsSidebar`. Locked by `SettingsDialog.test.tsx:79` `toHaveClass("w-[280px]")`. | **PASS** |
| **896 px centered detail column** | `SettingsDialog.tsx:106` — `<div className="mx-auto w-full max-w-[896px] px-8 pb-16 pt-10">`. `896px` is the literal resolution of `[ORIG]` `max-w-4xl` (56rem); `px-8` (32 px) and `pt-10` (40 px) are exact `[ORIG]` matches. `mx-auto` centers it in the `flex-1` main column. | **PASS** |
| **4 sections** | All four required nav entries render as buttons: **General**, **Terminal**, **Keyboard Shortcuts**, **Workspace** — each with its own `<section aria-labelledby>` body. Two additional sections ship beyond the requirement (**Notifications**, **Remote Access**), which is additive, not a parity break. Locked by `SettingsDialog.test.tsx:75` *"is a full-view original-Orca-style nav/detail surface"*, which asserts the presence of all four by accessible name, plus dedicated per-section tests for Terminal (Ghostty precedence + local override persistence), Shortcuts (registry region), and Notifications. | **PASS** |
| **Full view, not a modal** | Root is `<div role="dialog" aria-label="Settings" aria-modal={false} className="fixed inset-0 z-50 flex overflow-hidden bg-background text-foreground">` — **no backdrop, no `bg-black/60`, no `backdrop-blur`, no `rounded-xl`, no `max-w-xl`**. Verified against the 13:39 Settings capture: nav `#171717` spans `x=0..278`, a 1 px `#272727` divider sits at `x=279`, detail begins `x=280` on `#23262d`; corner samples at `(5,5)`, `(640,5)`, `(1275,845)` return chrome/background tokens, never a dimmed overlay. Content measured at `x=364..1195` → left gap 84 px / right gap 84 px → a centered 896 px column with 32 px inner padding, matching spec `x=364..1196`. | **PASS** |

**Known cosmetic delta (not graded as failure):** `[ORIG]` `SettingsSidebar` is `bg-worktree-sidebar` (`#2a2a2a`) + `border-worktree-sidebar-border`; `[CUR]` uses `bg-card` (`#171717`) + `border-border`. Both sides are semantic design tokens from the same palette — there is no hardcoded hex and no ad-hoc value — and the reference PNG contains **no Settings view to arbitrate against**. Recorded here as a deliberate, in-system divergence for a future product call, not a defect.

Section chrome is systematic: 24 px/600 page titles, `text-sm leading-6` descriptions, 11 px uppercase `tracking-[0.08em]` group labels, `border-y border-border` grouped rows, `min-h-10` shortcut rows with `font-mono text-[10px]` `<kbd>` chips, and a shared `inputClass` (`h-8 rounded-md border-input bg-background text-xs focus:border-ring`) for every field.

**Settings verdict: PASS (4/4).**

---

## 5. Identity & Assets

### 5.1 rorca app identity — PASS

| Surface | Value | Source |
| --- | --- | --- |
| Tauri product name | `"rorca"` | `src-tauri/tauri.conf.json:3` |
| Main window title | `"rorca"` | `src-tauri/tauri.conf.json:16` |
| Browser document title | `<title>rorca</title>` | `ui/index.html:11` |
| Settings footer | `rorca · local desktop` | `SettingsDialog.tsx` |
| Bundle identifier | `com.orca.lite` | `src-tauri/tauri.conf.json:5` — legacy id, intentionally unchanged to avoid breaking installed-app identity/preferences continuity. Not a visual criterion. |

Locked by two independent gates:
- `src-tauri/tests/rorca_native_contract.rs::tauri_metadata_uses_rorca_identity_and_generated_icons` — asserts `productName == "rorca"`, `windows[0].title == "rorca"`, that `bundle.icon` is exactly `["icons/32x32.png","icons/128x128.png","icons/128x128@2x.png","icons/icon.icns","icons/icon.ico"]`, and that **every one of those files exists on disk**. All five verified present.
- `ui/src/index-html.test.ts::uses the rorca title and crab-Orca favicon`.

### 5.2 Crab-containing Orca icon — master SVG: PASS

`ui/src/assets/rorca-icon.svg` is a 1024×1024 `role="img" aria-label="rorca crab icon"` master with a full design-system-consistent construction: `#151921` rounded-`228` plate, `#2a3441` inset stroke, a `shell` linear gradient (`#ff806b → #d9465f`), a `wave` gradient (`#75e6ff → #2e9bd5`), a `feDropShadow` filter, three symmetric leg pairs (`#ed5a65` / `#d9465f` / `#c63555`), the orca-body silhouette in `url(#shell)`, an eye group, and a mouth/highlight pair. Locked by `ui/src/assets/rorca-icon.test.ts` (asserts the aria-label and both gradient ids). Referenced as the primary favicon from `ui/index.html:5`, and it survives the production build — `ui/dist/assets/rorca-icon-DWxrWmKi.svg`, 2.60 kB.

### 5.3 Native icon set — PASS (verified by pixel analysis, not by assumption)

Colour histograms over `src-tauri/icons/icon.png` (512×512) and `128x128.png` return the exact crab palette — `#c63555`, `#d9475f`, `#e55863`, `#ed5a65`, `#fb8a78` — i.e. the SVG's leg/shell ramp rasterized. A luminance render of `src-tauri/icons/icon.png` resolves the crab unambiguously: a wide domed shell, two outrigger pincers at the upper flanks, the eye/wave detail across the shell face, and two legs descending below.

```
                     .:--======--:.
                 .-=+**************+=-.
               :+*********************+=:
       .-=-. :+**************************=. .-=-.     ← pincers
       -++++=************=---=++++++++++**+=++++-
        -+++***+******++-.#-  -++++++++++++++++-      ← eye
         :=**++****++++*- :.- :++++++++++++++=:
          -*+++*++++*****+-:-=*++++++++++++++:
          =*++++++*##********##%%%#++++++++++-        ← wave/mouth band
          -*+++++**##**+=++**#*++*++++++++==+-
          :*+++++++**#####****+=++++++++====+.
           =++++++++++++++++++++++++++======-
           .+++++++++++++++++++++++=========
          .==++++++++++++++++++++============.
         :====--=++++++++++++++=========::====:
         -===:  :==++++++++++++++======:  :===-
          ... .-=----:.::-----:::..----=-. ...
             .==-=:                  :=-==.          ← legs
             .---:                    :---.
```

All 5 configured bundle icons plus the full Windows Square*/StoreLogo set and the `android/` + `ios/` trees are present and were regenerated together (all `Aug 21 17:32`).

### 5.4 Web PNG icon set — **FAIL**

`ui/public/*.png` — which feed `ui/index.html`'s `favicon-16x16`, `favicon-32x32`, `apple-touch-icon`, and the PWA `icon-192` / `icon-512` — do **not** contain the crab artwork. Measured:

```
ui/public/icon-512.png         crab-red pixel share: 0.0%   dominant: #030305 #040305 #010101 #020202 #000000
ui/public/icon-192.png         crab-red pixel share: 0.0%
ui/public/apple-touch-icon.png crab-red pixel share: 0.0%
ui/public/favicon-32x32.png    crab-red pixel share: 0.0%
```

They are not empty — `icon-512.png` has 3583 distinct colours and 16.25 % of pixels above a luminance sum of 60, with pure white highlights — but that content is a **monochrome whale/orca silhouette on black**, a different and older asset:

```
--- ui/public/icon-512.png
            ..................            ...
         .:::.........                      .:.
       :......             =-                   .
       :....        -=. .:#@*+:. .=:            .
       :..         -%%#%%%@@*+*#*%#+:           .
       :.      *#*#@%%%%%%%@*+**#**+*++*+       .
       .      -%%%%%%%%%####*++***++++***:      .
       .     =%%%%%#+-:..      ..::-++++++-     .
       .   .*%###*=.                .-+++++=.   .
       .  .#%##*-                      :=++**.  .
```

Zero crab-red pixels; no pincers; no legs. The mtimes corroborate the drift: `ui/public/*` are `Aug 21 15:03`, while `src-tauri/icons/*` were regenerated at `Aug 21 17:32`. The stale set propagated straight into the shipped bundle — `ui/dist/icon-512.png` measures the same near-black histogram (`#030305 #040305 #010101 #020202 #000000`) after the §7.2 build.

**Impact:** the desktop app is unaffected (Tauri consumes `src-tauri/icons/*`, and the in-app/browser favicon is the correct SVG). The defect is confined to raster fallbacks — browser tab icon where SVG favicons are unsupported, iOS home-screen `apple-touch-icon`, and any PWA/manifest install surface.

**Fix direction:** re-rasterize `favicon-16x16`, `favicon-32x32`, `favicon.ico`, `apple-touch-icon`, `icon-192`, `icon-512` from `ui/src/assets/rorca-icon.svg` with the same generator that produced the 17:32 `src-tauri/icons/*` set, then rebuild. A guard test in the shape of `rorca-icon.test.ts` — asserting a non-zero crab-red pixel share in `ui/public/icon-512.png` — would keep the two sets from drifting again.

**Identity & Assets verdict: PASS on identity, PASS on native icons, FAIL on web PNG icon set.**

---

## 6. Design-system conformance audit

| Check | Result |
| --- | --- |
| Every colour references a token / CSS variable | **PASS** — all component colours resolve through Tailwind tokens backed by `--*-rgb` / `--*-alpha` triples in `index.css`. The single literal in shell chrome is the tab drop indicator's `bg-blue-500`, a Tailwind-scale value used deliberately as a transient drag affordance (matching `[ORIG]`'s own use of `bg-blue-500` in `EditorPanel`/`rename-file`); `text-blue-400` marks browser tabs likewise. No raw hex appears in any component file. |
| Every spacing uses the system scale | **PASS** — 4 px grid throughout (`gap-0.5/1/1.5/2`, `px-2/3/8`, `pt-10`, `pb-16`); named tokens `spacing.sidebar/titlebar/tabbar` carry the three locked shell dimensions; `w-[280px]` / `max-w-[896px]` are the two intentional literal parity constants transcribed from `[ORIG]`. |
| Composition follows existing patterns | **PASS** — shared primitives `IconButton`, `SectionHeader`, `StatusDot`, and `cn()` are reused across Sidebar, TabBar, WorktreeList, WorkspaceHeader, and SplitView. |
| Consistency between old and new components | **PASS** — one type scale (13/12/11/10/9 px), one radius family (`--radius: 0.5rem` → lg/md/sm), one 1 px `#ffffff12` separator weight everywhere. |
| Zero hardcoded magic numbers for visual properties | **PASS**, with the two documented parity literals above and `MIN_PANE_SIZE_PX = 80` / sidebar clamp bounds, which are behavioural constraints rather than visual styling. |

---

## 7. Test execution results

All four gates were executed on this machine (darwin arm64, M4 Max) at HEAD `a4de244`. Raw tails below.

### 7.1 Frontend Vitest — **PASS**

```
$ cd ui && bun run test        # vitest run --maxWorkers=1
 RUN  v3.2.7 /Users/indo/code/project/orca-lite/ui
 ✓ src/components/Sidebar.test.tsx            (16 tests)  286ms
 ✓ src/components/SettingsDialog.test.tsx      (8 tests)  216ms
 ✓ src/components/TabBar.test.tsx             (10 tests)  169ms
 ✓ src/state/workspaceRuntime.test.tsx         (3 tests)  167ms
 ✓ src/App.test.tsx                            (9 tests)  119ms
 ✓ src/components/TerminalSplitView.test.tsx   (8 tests)   98ms
 ✓ src/components/WorktreeDeleteDialog.test.tsx (4 tests)  87ms
 ✓ src/components/Sidebar.activity.test.tsx    (2 tests)   64ms
 ✓ src/components/WorktreeList.test.tsx        (6 tests)   73ms
 ✓ src/components/ProjectDialogs.test.tsx      (2 tests)   66ms
 ✓ src/components/CommandPalette.test.tsx      (2 tests)   66ms
 ✓ src/state/projectWorkspaceScope.test.tsx    (2 tests)   61ms
 ✓ src/lib/browserTauri.test.ts                (1 test)    53ms
 ✓ src/components/WorkspaceHeader.test.tsx     (4 tests)   54ms
 ✓ src/components/NewTabPopover.test.tsx       (4 tests)   29ms
 ✓ src/lib/shortcuts.test.tsx                 (23 tests)   30ms
 ✓ src/components/BrowserToolbar.test.tsx      (3 tests)   24ms
 ✓ src/state/workspaceStore.test.tsx          (10 tests)   18ms
 ✓ src/components/BrowserPane.test.tsx         (2 tests)   14ms
 ✓ src/state/workspaceActivity.test.tsx        (3 tests)   11ms
 ✓ src/lib/terminalSettings.test.tsx           (3 tests)    7ms
 ✓ src/state/workspaceStore.browserLifecycle.test.tsx (1 test) 8ms
 ✓ src/state/paneTree.test.ts                 (48 tests)    5ms
 ✓ src/lib/tauri.test.ts                       (9 tests)    4ms
 ✓ src/state/layout.test.ts                   (11 tests)    4ms
 ✓ src/lib/agentTitle.test.ts                  (7 tests)    3ms
 ✓ src/lib/terminalRenderer.test.ts            (3 tests)    2ms
 ✓ src/lib/activity.test.ts                    (3 tests)    1ms
 ✓ src/lib/terminalEvents.test.ts              (4 tests)    2ms
 ✓ src/lib/terminalEvents.bus.test.ts          (2 tests)    1ms
 ✓ src/lib/terminalOutput.test.ts              (2 tests)    1ms
 ✓ src/lib/ipcErrors.test.ts                   (2 tests)    1ms
 ✓ src/lib/sessionPersistence.test.ts          (1 test)     1ms
 ✓ src/lib/terminalTransport/terminalTransport.test.ts (2 tests) 1ms
 ✓ src/assets/rorca-icon.test.ts               (1 test)     1ms
 ✓ src/index-html.test.ts                      (1 test)     1ms
 ✓ src/test/viteHmrConfig.test.ts              (1 test)    60ms

 Test Files  37 passed (37)
      Tests  223 passed (223)
   Duration  15.73s
```

**37 files / 223 tests, 0 failed, 0 skipped, single run.** Directly UI-relevant coverage: Sidebar 16 + Sidebar.activity 2, TabBar 10, TerminalSplitView 8, SettingsDialog 8, WorktreeList 6, WorkspaceHeader 4, NewTabPopover 4, paneTree 48, layout 11, icon/html identity 2.

### 7.2 Build (`tsc && vite build`) — **PASS**

```
$ cd ui && bun run build
vite v6.4.3 building for production...
✓ 1684 modules transformed.
dist/index.html                              0.93 kB │ gzip:  0.48 kB
dist/assets/rorca-icon-DWxrWmKi.svg          2.60 kB │ gzip:  1.13 kB
dist/assets/geist-variable-CrgPqtmy.woff2   69.44 kB
dist/assets/xterm-9CEnUXvW.css               5.75 kB │ gzip:  1.98 kB
dist/assets/index-B4YjwWMV.css              39.33 kB │ gzip:  7.47 kB
dist/assets/addon-fit-YJmn1quW.js            1.60 kB │ gzip:  0.71 kB
dist/assets/browser-BUtqMOtk.js             25.78 kB │ gzip: 10.13 kB
dist/assets/addon-webgl-BrQ0bpT6.js        111.96 kB │ gzip: 30.52 kB
dist/assets/xterm-BqvuqXEL.js              332.63 kB │ gzip: 84.18 kB
dist/assets/index-0xVaJ9pl.js              333.28 kB │ gzip: 99.13 kB
✓ built in 1.72s
```

TypeScript strict pass clean (zero `tsc` diagnostics), 1684 modules transformed, crab SVG emitted. The compiled CSS from this build is the source of the §0.3 geometry/token measurements.

### 7.3 Cargo Test — **PASS**

```
$ cd src-tauri && cargo test
   Running unittests src/lib.rs         → 118 passed; 0 failed
   Running unittests src/main.rs        →   0 passed; 0 failed
   Running tests/backend_hardening.rs   →   8 passed; 0 failed
   Running tests/e2e_agent_workflow.rs  →   1 passed; 0 failed
   Running tests/ipc_hardening_contract.rs → 6 passed; 0 failed
   Running tests/rorca_native_contract.rs  → 9 passed; 0 failed
   Running tests/session_persistence_integration.rs → 1 passed; 0 failed
   Running tests/worktree_safety.rs     →   7 passed; 0 failed
   Doc-tests orca_lite_lib              →   0 passed; 0 failed
```

**150 tests, 0 failed, 0 ignored.** The identity-relevant `rorca_native_contract.rs` suite passed in full:

```
test tauri_metadata_uses_rorca_identity_and_generated_icons ... ok
test main_window_has_explicit_titlebar_drag_permission ... ok
test ghostty_parser_combines_font_families_and_reads_macos_option_as_alt ... ok
test ghostty_parser_handles_quotes_and_macos_option_keywords ... ok
test terminal_preferences_use_safe_absent_and_malformed_defaults ... ok
test loads_real_ghostty_config_from_system ... ok
test project_registration_rejects_non_git_roots_and_unregistered_branch_queries ... ok
test project_registration_is_idempotent_for_the_same_workspace_and_root ... ok
test project_registration_returns_canonical_root_and_lists_local_branches ... ok
```

### 7.4 Cargo Clippy — **PASS**

```
$ cd src-tauri && cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.30s
```

Zero warnings, zero errors under `-D warnings` across all targets (lib, bins, tests, benches, examples).

### 7.5 Gate summary

| Gate | Command | Result |
| --- | --- | --- |
| Frontend Vitest | `cd ui && bun run test` | **PASS** — 37 files / 223 tests |
| Build | `cd ui && bun run build` | **PASS** — tsc clean, 1684 modules |
| Cargo Test | `cd src-tauri && cargo test` | **PASS** — 150 tests / 8 binaries |
| Cargo Clippy | `cd src-tauri && cargo clippy --all-targets -- -D warnings` | **PASS** — 0 warnings |

---

## 8. Verdict matrix

| # | Criterion | Verdict | Basis |
| --- | --- | --- | --- |
| 1.1 | Sidebar — 236 px compact charcoal hierarchy | **PASS** | `DEFAULT_SIDEBAR_WIDTH = 236`; `bg-worktree-sidebar` `#2a2a2a`; `h-7` rows / 13 px labels; asserted by test |
| 1.2 | Sidebar — project tree nesting | **PASS** | `groupWorktreesByProject` + `pl-3` indent + `border-l` spine; 4 tests |
| 1.3 | Sidebar — independent accordion chevrons | **PASS** | collapsed-set model, per-project chevron button, persisted; 4 tests incl. interleaved-toggle |
| 1.4 | Sidebar — search / workspace buttons | **PASS** | dedicated 28 px rows in Workspace→Search→projects→runtime order; test-locked |
| 1.5 | Sidebar — single 1 px divider | **PASS** | prior 2 px double-draw remediated; regression test asserts `not.toHaveClass("border-r")` |
| 2.1 | TabBar — 32 px height | **PASS** | `.h-tabbar{height:2rem}` in compiled CSS; exact `[ORIG]` `h-[32px]` |
| 2.2 | TabBar — 1 px active indicator | **PASS** | `h-px bg-foreground` at `inset-x-0 bottom-0` |
| 2.3 | TabBar — low-chrome background | **PASS** | `border-b border-border bg-card` — byte-exact `[ORIG]` contract |
| 2.4 | TabBar — drop indicator styling | **PASS** | midpoint-computed `before:/after: w-[2px] bg-blue-500`; 3 insertion-index assertions |
| 2.5 | TabBar — context menu styling | **PASS** | `min-w-[13rem]` / `p-1` exact `[ORIG]`; tokenized surface; viewport-clamped; 4 tests |
| 2.6 | TabBar — full-width `bg-card` fill | **PASS** | prior `bg-terminal` leakage remediated by removing `flex-1` from the tab list |
| 3.1 | Terminal — no static toolbar | **PASS** | zero `Interrupt`/persistent-toolbar hits; header carries identity only |
| 3.2 | Terminal — top 24 px hover edge handle | **PASS** | `h-6` hotspot `z-20` + `h-6` toolbar `z-30`, opacity-gated; z-index test-locked |
| 3.3 | Splits — 1 px dividers | **PASS** | `w-px`/`h-px` `bg-border/80` line inside a 6 px transparent hit target |
| 3.4 | Splits — per-tab tree layouts | **PASS** | `layoutsByTabId` isolation; 48 + 11 + 8 tests |
| 4.1 | Settings — 280 px nav | **PASS** | `w-[280px]`, exact `[ORIG]`; test-locked |
| 4.2 | Settings — 896 px centered detail column | **PASS** | `mx-auto max-w-[896px] px-8 pt-10`; measured 84 px gutters at 1280 |
| 4.3 | Settings — 4 sections (General/Terminal/Shortcuts/Workspace) | **PASS** | all four present (+2 additive); test-locked by accessible name |
| 4.4 | Settings — full view, not a modal | **PASS** | `aria-modal={false}`, no backdrop/blur/radius; corner samples confirm |
| 5.1 | Identity — rorca app identity | **PASS** | `productName`/window title/`<title>` all `rorca`; native contract test asserts + verifies icon files exist |
| 5.2 | Assets — crab Orca master SVG | **PASS** | `rorca-icon.svg` labeled crab, both gradients, ships to dist |
| 5.3 | Assets — native icon set contains the crab | **PASS** | crab palette + luminance render confirm pincers/legs/shell; all 5 bundle icons on disk |
| 5.4 | Assets — web PNG/PWA icon set contains the crab | **FAIL** | 0.0 % crab-red across `ui/public/*.png`; stale whale silhouette, propagated into `ui/dist/icon-512.png` |
| 6 | Design-system conformance | **PASS** | tokenized colour, 4 px grid, shared primitives, single type/radius/separator scale |
| 7.1 | Frontend Vitest | **PASS** | 37 files / 223 tests |
| 7.2 | Build | **PASS** | tsc clean, 1684 modules |
| 7.3 | Cargo Test | **PASS** | 150 tests / 8 binaries |
| 7.4 | Cargo Clippy | **PASS** | 0 warnings under `-D warnings` |

**Score: 27 PASS / 1 FAIL.**

---

## 9. Limits of this report

State these plainly rather than papering over them:

1. **No live GUI capture was taken.** Scope forbids OS input automation, so no screenshot of the *current* HEAD exists. Every geometry claim about current code is grounded in source constants and the **compiled** `ui/dist/assets/index-B4YjwWMV.css` produced by the §7.2 build, not in a rendered frame.
2. **The newest available app screenshots (13:39) are stale.** They predate the icon regeneration (15:03) and the HMR / tab-actions / sidebar-collapse commits (17:22–18:01). They are cited **only** to document the two now-remediated defects (2 px rail edge, tab-strip black leakage) and the Settings measurements, both of which remain valid for the code paths they exercised. A fresh GUI capture at 1280×850 is the one outstanding verification step.
3. **The reference PNG cannot arbitrate exact geometry.** It is a two-window composite whose halves disagree (220 vs 264 px rail, 35 vs 38 px header), so it is authoritative for palette, ordering, and chrome weight only. Exact dimensions are graded against `[ORIG]` bundle constants, which were re-verified from source in §0.2.
4. **Settings has no reference counterpart at all.** Both halves of the composite are workspace shells. Settings is graded purely against `[ORIG]` + `visual-reference.md` §4/§6.
5. **The reference's left half uses a `#282c34` One-Dark terminal theme.** That is a terminal colour-scheme choice, not shell chrome, and is out of scope for shell parity.
6. **Read-only honoured.** No application source, test, config, or asset was modified. `.omo/evidence/rorca/comprehensive-visual-qa-report.md` is the only file written.

---

## 10. Recommended follow-up (single item)

Regenerate `ui/public/{favicon-16x16,favicon-32x32,favicon.ico,apple-touch-icon,icon-192,icon-512}.png` from `ui/src/assets/rorca-icon.svg` using the same generator that produced the 17:32 `src-tauri/icons/*` set, rebuild, and add a pixel-share guard test alongside `rorca-icon.test.ts` so the web and native icon sets cannot drift apart again. Everything else in the redesign is verified and green.
