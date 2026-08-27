# Settings Panel Redesign: Analysis and Spec

## 1. Problem Statement

User feedback indicates that the settings dialog feels fragmented and unpolished across its various tabs. Visual presentation, control behavior, typography, and layout rules diverge depending on which section the user selects.

Two prominent examples illustrate the issue:

1. **General Tab as Dead Filler**: The General tab contains static promotional cards and an unnecessary table of contents that mirrors the left-hand navigation list. It provides almost no useful configuration controls, wasting the default settings surface. See before screenshot: `.omo/plans/evidence/settings-panel/before/general.png`.
2. **Notification Tab Divergence**: The Notifications tab bypasses the shared section header used by the rest of the application. It hand-rolls its own header bar and introduces bespoke iOS-style slider pills with custom CSS animations, completely breaking consistency with the native checkboxes used in other panels. See before screenshot: `.omo/plans/evidence/settings-panel/before/notifications.png`.

Similar visual fractures appear across all nine settings tabs:
- General (`.omo/plans/evidence/settings-panel/before/general.png`)
- Appearance (`.omo/plans/evidence/settings-panel/before/appearance.png`)
- Terminal (`.omo/plans/evidence/settings-panel/before/terminal.png`)
- Keyboard Shortcuts (`.omo/plans/evidence/settings-panel/before/keyboard-shortcuts.png`)
- Workspace (`.omo/plans/evidence/settings-panel/before/workspace.png`)
- Agents (`.omo/plans/evidence/settings-panel/before/agents.png`)
- Browser (`.omo/plans/evidence/settings-panel/before/browser.png`)
- Notifications (`.omo/plans/evidence/settings-panel/before/notifications.png`)
- Remote Access (`.omo/plans/evidence/settings-panel/before/remote-access.png`)

This document defines the normative design spec and migration blueprint to unify all nine tabs under a shared component hierarchy.

---

## 2. Inconsistency Catalog

Below is the summary catalog of the 13 UI/UX inconsistency patterns identified across the settings dialog. For full per-section analysis, see the [Settings Sections Audit Ledger](../../.omo/plans/ledger/settings-sections-audit.md).

| ID | Inconsistency Pattern | Affected Section(s) | File & Line Reference | Divergence Summary |
|---|---|---|---|---|
| 1 | **Hand-rolled Heading vs Shared `SettingsHeading`** | Notifications vs All 8 others | `ui/src/components/SettingsDialog.tsx:1162-1178` | Notifications bypasses `SettingsHeading`, using a hand-rolled `text-sm` bar instead of the standard `text-[15px]` heading. |
| 2 | **Visible `<h2>` vs Hidden `sr-only` vs Missing Accessible Heading** | Terminal, General, Notifications | `ui/src/components/SettingsDialog.tsx:266, 528, 1161` | General lacks an `<h2>`; Notifications uses `role="region"` without `aria-labelledby`; Terminal renders visible `<h2>`; others use `<h2 className="sr-only">`. |
| 3 | **Custom iOS Switch Pill vs Native Checkbox** | Notifications vs General, Terminal, Browser, Agents | `ui/src/components/SettingsDialog.tsx:1215-1223, 1234-1242, 1253-1261` | Notifications creates bespoke CSS switch toggles, while General, Terminal, Browser, and Agents use native `size-4 accent-foreground` checkboxes. |
| 4 | **`SettingRow` API Inconsistency (`title` vs `label`)** | Notifications vs General, Appearance, Workspace, Browser, Remote Access, Agents | `ui/src/components/SettingsDialog.tsx:1208, 1227, 1246, 1264, 1284, 1297` vs `1988-2009` | Notifications passes `title="..."` while all other sections pass `label="..."`. `SettingRow` was patched with a fallback to mask this. |
| 5 | **Duplicate Private `SettingRow` Component Definition** | Browser (`BrowserSettingsPanel`) vs SettingsDialog | `ui/src/components/BrowserSettingsPanel.tsx:26-41` vs `ui/src/components/SettingsDialog.tsx:1988-2009` | `BrowserSettingsPanel.tsx` defines a private `SettingRow` with different padding and max-width rules instead of importing a shared primitive. |
| 6 | **Bypassing `SettingRow` for Ad-Hoc Form Layouts** | Terminal, Keyboard Shortcuts | `ui/src/components/SettingsDialog.tsx:540-588, 669-684` | Terminal avoids `SettingRow` completely, constructing bare stacked labels and a custom Option-as-Alt row. Keyboard Shortcuts builds custom shortcut rows. |
| 7 | **Button Height and Corner Radius Drift** | Workspace, Browser, Notifications, Remote Access | `ui/src/components/SettingsDialog.tsx:764, 772, 981, 1173, 1282, 1301, 1509, 1559, 1651` | Standard buttons use `h-7 rounded-md`, but item actions drift to `rounded` or unconstrained padding (`px-2.5 py-1 text-xs`, `px-3 py-1.5`). |
| 8 | **Row Grouping Pattern Divergence** | All 9 tabs (General, Appearance, Terminal, Keyboard Shortcuts, Workspace, Agents, Browser, Notifications, Remote Access) | `ui/src/components/SettingsDialog.tsx:273, 455, 540, 663, 747, 1180, 1207, 1499, 1801, 1870` | Tabs arbitrarily mix standalone cards, `border-y border-border` row groups, `divide-y rounded-lg bg-card` lists, and uncontained stacks. |
| 9 | **Button Font Size Discrepancies** | Notifications, Remote Access, Agents vs General, Appearance, Terminal, Workspace, Browser | `ui/src/components/SettingsDialog.tsx:1173, 1282, 1301, 1509, 1559, 1572, 1812` vs `405, 448, 533, 728, 911, 1083` | Standard buttons use `text-[11px]`, but Notifications, Remote Access, and Agents use `text-xs` (12px) across several controls. |
| 10 | **Accent Color Token Mismatch on Interactive Controls** | Notifications vs General, Terminal, Browser, Agents | `ui/src/components/SettingsDialog.tsx:1287` vs `301, 586, 957, 1906` | Native checkboxes use `accent-foreground`, while the Notifications volume slider uses `accent-primary`. |
| 11 | **Border Alpha & Opacity Fragmentation** | Notifications, Agents, SettingsDialog (`SettingRow`) | `ui/src/components/SettingsDialog.tsx:1162, 1180, 1870, 1914, 1996` | Sections introduce arbitrary border opacities (`border-border/70`, `border-border/50`, `border-border/40`) instead of crisp `border-border`. |
| 12 | **Hardcoded Hex and Named CSS Colors** | Remote Access, Notifications | `ui/src/components/SettingsDialog.tsx:1438, 1219` | Remote Access hardcodes hex values in QR code canvas generation; Notifications hardcodes named colors `after:bg-white` and `after:border-white`. |
| 13 | **Select Input Focus Ring Class Omission** | Notifications vs Appearance, Browser | `ui/src/components/SettingsDialog.tsx:1269` vs `463, 924` | Appearance and Browser selects include `transition-colors focus:border-ring`, whereas Notifications drops the focus ring entirely. |

---

## 3. Root Causes

1. **Lack of Shared Form & Row Primitives**: `SettingRow` was declared locally in `SettingsDialog.tsx` instead of a public `ui/` primitive, causing duplicate implementations in `BrowserSettingsPanel.tsx` and custom layouts in `TerminalSettings`.
2. **Missing Standard Control Wrapper Primitives**: Without a shared toggle primitive, `NotificationSettings` hand-rolled custom switch pills while other tabs used native checkboxes.
3. **Independent Feature Implementations**: Distinct contributors built sections like Notifications, Remote Access, and Agents in isolation without design linting, causing drift in font sizes, radii, and border tokens.
4. **Undocumented Token Fallbacks**: Classes like `border-input`, `bg-primary`, and arbitrary alpha modifiers (`border-border/70`) were added ad-hoc without formal definition in `DESIGN.md`.

---

## 4. Unified Section Spec (Normative)

All settings sections must conform to these exact specifications:

### S1. Headings & Accessibility
Every section root element must render:
- The shared `SettingsHeading` component (`icon`, `title`, `description`).
- An accessible `<h2 id="settings-<section>-heading" className="sr-only">Title</h2>`.
- The container `<section aria-labelledby="settings-<section>-heading">` linking the two.

### S2. Boolean Controls
Native checkboxes styled with `className="size-4 accent-foreground"` are the sole standard boolean control. The custom iOS switch pills in `NotificationSettings` are completely removed.

### S3. SettingRow API Normalization
The `SettingRow` component requires a `label: string` prop across all call sites. A backwards-compatible `title?: string` alias may exist during transition, but all nine panels must pass `label`.

### S4. Shared Component Promotion
`SettingsHeading` and `SettingRow` are promoted to `ui/src/components/ui/SettingsPrimitives.tsx`. The private duplicate `SettingRow` in `BrowserSettingsPanel.tsx` is deleted and replaced with this shared import.

### S5. Button Hierarchy & Dimensions
- **Secondary Action Standard**: `h-7 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground` (padding may extend to `px-2.5` for buttons with icons).
- **Primary Action Standard**: `h-7 rounded-md bg-primary px-2 text-[11px] font-medium text-primary-foreground hover:bg-primary/90 shadow-sm`.

### S6. Border Alpha Discipline
All section grouping containers, card wrappers, and separators must use full-alpha `border-border`. The only permitted alpha modifier is `SettingRow`'s internal hairline divider `border-border/40` as documented in `DESIGN.md`.

### S7. Layout & Surface Rules
- **Primary User Preferences**: Configurable settings rows must live inside `border-y border-border` row groups built with `SettingRow`.
- **Management & Operational Content**: Self-contained management blocks (Software Update card, CLI Launcher card, Paired Devices lists, Active Web Tabs) must render inside `rounded-lg border border-border bg-card` cards.

### S8. Inputs, Selects & Sliders
- All `<select>` and `<input>` elements must include `outline-none transition-colors focus:border-ring`.
- The sound select dropdown in `NotificationSettings` must restore its `focus:border-ring` style.
- The volume slider in `NotificationSettings` must use `accent-foreground` instead of `accent-primary`.

---

## 5. General Tab Final Composition

All static filler copy in `GeneralSettings` is removed:
- The static introductory card ("Ferryx desktop") is removed.
- The table of contents list ("Settings sections") duplicating the sidebar navigation is removed.

### New Composition
The functionalized General tab contains three clear sections:

1. **Behavior Row Group (`border-y border-border`)**:
   - **Confirm before closing a tab**: Existing setting, persisted in `ferryx.settings.general` via `useGeneralSettings`.
   - **Show sidebar on startup**: New toggle, persists `ferryx.sidebar.open` via helpers in `ui/src/lib/generalSettings.ts`.
2. **Ferryx CLI Launcher Card (`rounded-lg border border-border bg-card`)**:
   - Relocated from `BrowserSettings` to `GeneralSettings`. Manages the command-line helper symlink in `~/.local/bin/ferryx`.
3. **Software Update Card (`rounded-lg border border-border bg-card`)**:
   - Kept in `GeneralSettings`. Displays application version checks, download progress, and restart actions.

### Explicitly Excluded from General (Ownership Kept Intact)
- **External link routing (`openLinksInBuiltInBrowser`)**: Stays in the Browser section.
- **Master notification toggle (`enabled`)**: Stays in the Notifications section.
- **Default AI assistant (`defaultAgentId`)**: Stays in the Agents section.
- **Default new-tab kind**: Future work. No backing state or storage key exists yet.

---

## 6. Accepted Debt

The following deliberate exceptions are documented and accepted:

1. **Hardcoded Hex in QR Code Generator (#12)**: The QR generator in `RemoteAccessSettings` uses `{ dark: "#ffffff", light: "#171717" }` for canvas rendering. This is functional and kept as-is since canvas drawing requires exact hex strings rather than CSS custom variables.
2. **Terminal Grid Layout**: The `TerminalSettings` form grid uses stacked label blocks instead of `SettingRow` due to its multi-column layout for font sizing and cursor styles (per S7 note).
3. **Keyboard Shortcut Keycaps**: Keycap pills in `ShortcutSettings` retain muted background tones (`bg-muted/70`, `bg-muted/40`) as documented in `DESIGN.md` under the keycap styling recipe.

---

## 7. Verification Plan

Implementation will follow a strict test-driven process:

1. **RED-First Test Authoring**:
   - Add failing unit and integration tests asserting the presence of accessible headings (`aria-labelledby` + `sr-only` pairs) across all nine sections.
   - Assert `SettingRow` accepts `label` everywhere and renders consistent DOM structures.
   - Assert `GeneralSettings` renders the CLI card and sidebar startup toggle while omitting filler copy.
   - Assert `NotificationSettings` renders native `size-4 accent-foreground` checkboxes and no switch pills.
2. **Vitest Scope**:
   - Execute `bun test` or `bun x vitest run` covering `ui/src/components/SettingsDialog.test.tsx` and related settings test suites.
3. **TypeScript Typecheck Gate**:
   - Run `bun x tsc --noEmit` from `ui/` to ensure clean type conformance across all shared primitives and callers.
4. **Visual Regression Verification**:
   - Run `node ui/qa/captureSettingsShots.mjs` to capture after-screenshots for all nine tabs.
   - Inspect captured artifacts against baseline evidence to confirm layout alignment, typography consistency, and token compliance.
