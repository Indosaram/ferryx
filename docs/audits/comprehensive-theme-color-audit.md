# Comprehensive Theme Color Audit

Date: 2026-08-22

## Scope

Audited Charcoal, Dark, Light, and every configured accent color across application surfaces, text, actions, destructive controls, status indicators, remote UI, and the Settings QR panel.

## Corrections

### Appearance runtime

- `ui/src/main.tsx` now imports `settings-runtime.css` and installs `installSettingsRuntimeBridge()`.
- Appearance choices now update CSS theme, accent, density, and `color-scheme` at runtime instead of only persisting an isolated Settings-panel value.

### Base theme tokens

- Charcoal, Dark, and Light retain aligned `background`, `sidebar`, and `worktree-sidebar` base surfaces.
- Light receives contrast-safe semantic status colors:
  - working `#2563eb`
  - warning `#a16207`
  - success `#15803d`
  - idle `#5b6472`
- Light destructive controls use `#b91c1c` with `#fafafa` foreground.
- Dark-capable themes use `#0a0a0a` on the default bright destructive token.

### Accent tokens

- Blue, Emerald, Purple, Amber, and Rose accent actions now set both `primary-foreground` and `sidebar-primary-foreground` to `#0a0a0a`.
- This preserves readable button labels across every accent selection, including Light mode.

### Theme-adaptive UI surfaces

- Remote app and session list use semantic background, card, border, foreground, muted, and primary tokens rather than fixed neutral/blue hex classes.
- Settings Remote/QR panel uses `bg-card` and `border-border` rather than a fixed dark panel.
- Remote/status badges use semantic status tokens.
- Worktree destructive actions, mobile key hover state, default status dots, and browser-tab globe icon use semantic foreground/status/primary tokens.

## Intentionally retained fixed colors

- Terminal ANSI palettes remain terminal-rendering data, not application UI theme values.
- QR code foreground/background stays white/dark for scanner reliability.
- Modal dimming overlays retain translucent black because their behavior is independent of the selected app surface theme.

## Verification

- `npx vitest run src/appearanceThemeContract.test.ts src/lib/settingsRuntime.test.ts` — **11 tests passed**.
- Earlier in this change set, `npm run build` completed successfully after the appearance runtime and primary color corrections.
- A later shared-worktree change introduced two `getInitialProject` exports in `ui/src/lib/tauri.ts`. It blocks the current Settings/Remote test modules at transform time and blocks a fresh build, but it is outside this theme audit's changed files.
