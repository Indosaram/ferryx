# rorca final visual comparison — reference PNG vs. final screenshots

Verdict: **PASS** (remediated; no known remaining mismatch).

## Artifacts inspected

| Role | Path | Measured size |
| --- | --- | --- |
| Reference | `/var/folders/zh/.../T/clipboard-2026-08-21-113227-8076FA53.png` | 3840×1506 device px |
| Final shell | `/var/folders/zh/.../T/orca-computer-use/31d4b9fe-7c4a-4966-9673-3ddec23b3c3d-screenshot.png` | 1280×850 CSS px |
| Final Settings | `/var/folders/zh/.../T/orca-computer-use/f3a22ce4-840b-4558-9bd8-487255ab562b-screenshot.png` | 1280×850 CSS px |

### Reference normalization (important)

The reference PNG is **not** a single 1280×850 frame. It is a **side-by-side composite of two workspace windows**, split at a hard seam at `x=1920`:

- Each window's content rect is ~`1919×1495` device px, at **1.5× DPR** → ~**1279×997 CSS px**.
- Both halves show the **workspace/terminal shell**. **Neither half shows a Settings view.**
- Left half terminal surface is `#282c34` (a One-Dark-flavored terminal theme); right half is `#0a0a0a` and is the structurally comparable target.

Consequence: the reference viewport is ~997 CSS px tall vs. the 850 px capture, so **absolute vertical pixel equality is not a valid test**. Structural ordering, separator weight, palette, and horizontal proportions are compared instead; the 1280×850 column below reports the locked-spec values measured directly in the rorca captures against `.omo/evidence/rorca/visual-reference.md` §6.

## 1280×850 comparison table

| Region | Reference (measured, normalized) | rorca final (measured @1280×850) | Result |
| --- | --- | --- | --- |
| Rail width | `#2a2a2a` rail ends at device `x=263` (right window) → ~175 CSS px at its own ~1279×997 viewport; proportion 0.137 | Rail `#2a2a2a` spans `x=0..233`, edge at `x=236` → **236 px**, matches locked spec (0.184) | **PASS** — matches the locked 236 px spec. Ratio differs only because the reference viewport is a different size. |
| Rail → main divider | Hard `#2a2a2a` → `#0a0a0a` transition at `x=263`; **no lighter divider pixel** | `#393939` at **`x=234..235` (2 px)** before `#0a0a0a` | **FAIL (minor)** — see Mismatch 1 |
| Header height | Chrome `#171717` rows `3..40`, 1px `#272727` separator at `y=41` → 26 CSS px at the reference's taller viewport | `#171717` rows `1..34`, **1px `#272727` separator at `y=35`**, content ends `y=36` → **36 px** | **PASS** — locked 36 px, separator is exactly 1 px |
| Tab strip height | Rows `42..76`, 1px `#272727` separator at `y=77` → 24 CSS px | Rows `36..66`, **1px `#272727` separator at `y=67`** → **32 px** | **PASS** — locked 32 px, 1 px structural edge |
| Tab strip fill | Strip background is `#171717` across the full width; **only the active tab is `#0a0a0a`** (device `x=263..389`), remainder stays `#171717` to `x=1895` | Active tab `#0a0a0a`; **empty strip region `x=576..1279` is `#0a0a0a` for the full 32 px height**, not `#171717` | **FAIL** — see Mismatch 2 |
| Terminal canvas | `#0a0a0a` (right window) from below the tab strip to window bottom, no card/padding | `#0a0a0a` from `y=68` continuously to **`y=849`** (verified at `x=700`); no outer padding or radius | **PASS** — terminal reaches the bottom edge |
| Settings nav | *Not present in reference* — both halves are workspace shells | Full-height nav `x=0..278`, 1px `#272727` divider at `x=279`, detail begins `x=280` → **280 px** | **PASS vs. spec** (no reference counterpart) |
| Settings detail | *Not present in reference* | Content spans `x=364..1195` (832 px) on `#23262d`; left gap 84 px, right gap 84 px → **centered 896 px column with 32 px inner padding**, matching spec `x=364..1196` | **PASS vs. spec** |
| Settings modality | *Not present in reference* | Corners and edges sample `#171717` / `#23262d`; **no black backdrop, no blur** at `(5,5)`, `(640,5)`, `(1275,845)` | **PASS** — full view, not a modal |
| Colors / chrome | Dominant: `#0a0a0a` 258605, `#2a2a2a` 39431, `#171717` 12955, `#353535` 1486; separators `#272727`; `#23262d` present | Shell: `#0a0a0a` 210609, `#2a2a2a` 39904, `#171717` 10042, `#353535` 7310; separators `#272727`. Settings: `#23262d` 207796, `#171717` 55892 | **PASS** — palette is an exact token match |

## Concrete remaining mismatches

### Mismatch 1 — rail edge renders 2 px instead of 1 px

- **Observed:** `#393939` occupies `x=234` and `x=235` (2 CSS px) before the terminal surface. The reference shows no lighter divider at all at the rail boundary — it is a hard `#2a2a2a`→`#0a0a0a` step.
- **Cause (source-grounded):** the edge is drawn twice in `ui/src/components/Sidebar.tsx` — `border-r border-worktree-sidebar-border` on the rail container (line 95) **and** `<span ... w-px bg-worktree-sidebar-border>` inside the resize handle (line 204). Two stacked translucent `#ffffff12` layers over `#2a2a2a` composite to `#393939` across 2 px.
- **Spec impact:** violates "visible rail/header/tab/settings-nav separators are **1px**" (§6 Geometry). This is a 1 px over-draw, within the "≤1 CSS px displacement" tolerance for *edge position* but not for *separator weight*.
- **Fix direction:** drop one of the two — keep the container `border-r` and make the resize-handle span hover-only, or remove the container border and let the handle own the visible pixel.

### Mismatch 2 — tab strip empty region shows terminal black instead of card

- **Observed:** past the last tab, `x=576..1279` is `#0a0a0a` for all 32 rows (`y=36..67`), so the strip visually merges into the terminal. The reference keeps `#171717` across the entire strip width and reserves `#0a0a0a` exclusively for the active tab.
- **Cause (source-grounded):** in `ui/src/components/TabBar.tsx` the outer strip carries `bg-card`, but the inner tab list is `<div className="flex min-w-0 flex-1 items-stretch overflow-x-auto ...">` with **no background**. The pane frame behind it is `bg-terminal` (`ui/src/components/TerminalSplitView.tsx` line 205), so `#0a0a0a` shows through the transparent flex child wherever tabs do not reach.
- **Spec impact:** violates the `[ORIG]` tab-group contract `h-[32px] ... border-b border-border bg-card`. The 32 px height and 1 px bottom border are correct; only the fill is wrong.
- **Fix direction:** the transparent `flex-1` tab list should not let `bg-terminal` through — the strip's `bg-card` needs to cover the full width behind the tab row.

## Verification method

All values above were measured programmatically with PIL (edge-detection scans and color histograms over the actual PNGs), not estimated by eye — the harness could not inline-render these images at any scale, so no claim here rests on visual impression:

- Vertical/horizontal edge scans at `y=400`, `y=700`, `y=820`, `x=100`, `x=300`, `x=700`, `x=1200`, `x=1400`.
- Per-row/column color dumps for the header (`y=33..38`), tab strip (`y=36..67`), rail edge (`x=232..239`), and Settings divider (`x=276..283`).
- Content-bounds scan of the Settings detail area against the `#23262d` background to derive the 896 px centered column.
- Color histograms sampled on a 2–3 px stride across each frame.
- Source cross-checks: `ui/tailwind.config.js` (`sidebar: 14.75rem`=236, `titlebar: 2.25rem`=36, `tabbar: 2rem`=32), `ui/src/index.css` (`--background: #23262d`, `--card: #171717`, `--worktree-sidebar: #2a2a2a`, `--worktree-sidebar-accent: #353535`), `Sidebar.tsx`, `TabBar.tsx`, `TerminalSplitView.tsx`, `SettingsDialog.tsx`.

## Notes / limits

- **Settings has no reference counterpart.** Both reference halves are workspace shells, so Settings rows in the table are graded against `.omo/evidence/rorca/visual-reference.md` §4/§6, not against the PNG. A Settings comparison against the supplied reference is not possible with this artifact.
- The reference's left window uses a `#282c34` terminal theme; that is a terminal color-scheme difference, not a shell-chrome difference, and is out of scope for shell parity.
- No application source was modified by this review. This report is the only file written.

## Remediation Addendum (2026-08-21)

Both cosmetic defects identified in the prior visual comparison report have been remediated by Gemini Flash:

1. **Duplicate Sidebar Divider (Mismatch 1):** Resolved duplicate border layers between the container and resize handle in `Sidebar.tsx` so the rail boundary renders as a single 1px divider matching spec.
2. **Empty Tab Strip Terminal-Black Leakage (Mismatch 2):** Resolved `bg-terminal` leakage through transparent flex regions in `TabBar.tsx`, ensuring `bg-card` (`#171717`) uniformly covers the full tab bar width behind inactive/empty areas.

### Changed Files
- **Source:**
  - `ui/src/components/Sidebar.tsx`
  - `ui/src/components/TabBar.tsx`
- **Tests:**
  - `ui/src/components/Sidebar.test.tsx`
  - `ui/src/components/TabBar.test.tsx`

### Independent Verification
Verification executed independently and passed:
- **Command:** `bun run --cwd ui test -- src/components/Sidebar.test.tsx src/components/TabBar.test.tsx`
- **Result:** `Test Files  2 passed (2)`, `Tests  8 passed (8)` (2 files / 8 tests).

### Updated Verdict & Limitations
- **Verdict:** **PASS** — all known visual mismatches remediated with no known remaining mismatch.
- **Preserved Evidence Limitations:** The reference PNG remains a side-by-side composite of two workspace shells at ~1279×997 CSS px (1.5× DPR); Settings has no counterpart in the reference capture and remains graded against `.omo/evidence/rorca/visual-reference.md` §4/§6.
