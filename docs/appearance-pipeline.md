# Ferryx appearance pipeline (source of truth)

Documented from the delivered appearance modules and stylesheets. Paths below are
relative to the repository root. The pipeline describes DOM application, not a
second theme engine or a native terminal colour-token API.

## Ownership and state

`ui/src/lib/appearanceSettings.ts` owns the appearance record, its normalization,
load/save/reset functions, React subscriptions and `applyAppearanceSettings()`.
`ui/src/lib/storageKeys.ts` defines its single localStorage key:
`ferryx.settings.appearance`. Theme documents, selection and standalone CSS are
fields in that record, not separate storage keys.

| Field | Default | Accepted settings values |
| --- | --- | --- |
| `theme` | `charcoal` | `charcoal`, `dark`, `light`, `system` |
| `accentColor` | `default` | `default`, `blue`, `emerald`, `purple`, `amber`, `rose` |
| `density` | `compact` | `compact`, `comfortable` |
| `customThemes` | `[]` | Validated v1 documents with unique ids |
| `activeCustomThemeId` | `null` | An id present in the validated library, otherwise `null` |
| `customCss` | Empty string | Stored text; scoped and validated before DOM injection |
| `renderMath` | `true` | Boolean |

`loadAppearanceSettings()` returns settings only.
`loadAppearanceSettingsState()` returns `{ settings, invalidThemes }`, retaining
rejected raw documents and field-level errors. Duplicate ids become diagnostic
rows. Malformed JSON and malformed stored records also retain diagnostics. Loading
does not rewrite storage; subsequent saves preserve rejection rows. Reset removes
the record and returns defaults.

Saves dispatch `ferryx:appearance-settings`. The React hook also listens for
storage events for this key (or a cleared store).
`ui/src/lib/settingsRuntimeBridge.ts` applies the initial record and appearance
change events. System-theme changes trigger reapplication when the preference is
`system`. The bridge sends the resulting light/dark side to the desktop terminal,
with unchanged-side requests deduplicated; it does not send user token maps.

## Application order (lowest to highest layer)

```
built-in palette and selected accent
  -> user theme colour tokens (and derived RGB companions)
  -> active theme css
  -> standalone user customCss
```

These are application layers, not a replacement for the normal CSS cascade:
scoped styles still obey selector specificity. Standalone CSS wins over theme CSS
for equal-specificity rules targeting the same property because it is appended
last. The CSS layers style descendants of chat scopes; they cannot declare token
custom properties or target the page root.

`applyAppearanceSettings()` defaults to `document.documentElement`. It revalidates
the active document and scopes both CSS sources before changing the DOM. It then:

1. Disconnects the prior scope observer, restores prior inline token values,
   removes injected styles and runtime-added chat markers, and clears the custom
   theme attribute.
2. Sets `data-theme`, `data-theme-preference`, `data-accent`, `data-density` and
   inline `color-scheme`. A valid active document supplies its `dark` or `light`
   base; otherwise the saved built-in preference resolves normally. `charcoal`
   remains `charcoal` on the root and uses a dark colour scheme. `system` resolves
   to `light` or `dark` from the native OS theme event or media query.
3. Applies document colours with `root.style.setProperty`, recording the old
   values and priorities for restoration. Each colour also gets a derived
   `-rgb` property containing a space-separated, rounded/clamped sRGB triplet;
   alpha is not part of that triplet.
4. Appends scoped theme CSS, then scoped standalone CSS, as separate style
   elements after the palette and inline token overrides are installed.

Built-in palette declarations live in `ui/src/index.css` and
`ui/src/settings-runtime.css`; the applier leaves them in CSS rather than copying
them into an independent token store. Inline document tokens live on the root.
Descendant-specific declarations can override inherited values: for example,
`ui/src/index.css` defines separate `.remote-app-root` neutral tokens. A root
override is not a promise that every descendant's locally declared neutral token
will change.

The selected accent remains active with a user theme, supplying tokens the
document does not override. Default chat primary values are `#346bf1` with
`52 107 241` in dark/charcoal and `#1d4ed8` with `29 78 216` in light; both use
`#ffffff` as primary foreground. Accent rules change chat brand and user-bubble
tokens (primary, primary foreground, link, code and user bubble), not the
remaining neutral chat surfaces.

## Theme documents and colour validation

`ui/src/lib/themeDocument.ts` defines `ThemeDocumentV1`, `parseThemeDocument()` and
`THEME_TOKEN_ALLOWLIST`. The allowlist's palette source is the colour declarations
in the `:root` and `:root[data-theme="light"]` blocks of `ui/src/index.css`.
The runtime allowlist is explicit, not dynamically discovered from CSS.

A document has `version`, `id`, `name`, `base`, `tokens` and optional `css`.
Validation returns either the complete document or `{ path, message }` errors;
invalid fields are not partially applied.

| Boundary | Rejection rule |
| --- | --- |
| Document | Must be a non-array object; unknown top-level fields are rejected |
| `version` | Must be numeric `1` |
| `id` | Must match `^[a-z0-9][a-z0-9-]{1,63}$` (2-64 characters) |
| `name` | Must be a nonblank string, at most 64 characters |
| `base` | Must be `dark` or `light` |
| `tokens` | Must be a non-array object with at most 200 entries |
| Token names | Only allowlisted colours; no radius, timing, numeric or authored `*-rgb` tokens |
| Token values | Hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`), supported `rgb()`/`rgba()`, `hsl()`/`hsla()` or `oklch()` syntax |
| Other values | Named colours, bare numbers, references such as `var()` and unsupported colour syntax are rejected |
| `css` | If present, must be text of at most 32 KiB in UTF-8 |

Colour channels are parsed without a browser CSS parser. Contrast validation runs
after structural/value validation and requires at least 4.5:1 for:

- `--chat-foreground` on `--chat-surface`.
- `--chat-foreground` on `--chat-screen` (there is no `--chat-background` token).
- `--chat-primary-foreground` on `--chat-primary`.

Missing members use the validator's base-specific default chat colours. Alpha is
composited before measurement. A failure names both tokens and reports the ratio.
Primary fill against the chat surface is not one of these checks. This document
validation does not certify the contrast of arbitrary scoped CSS or recompute
contrast against the selected accent's fallback colours.

The document parser checks CSS type and size, not its rule grammar. Import in
`ui/src/components/settings/AppearanceSection.tsx` calls the CSS scoper after the
document parser; the DOM applier repeats both boundaries. A theme with refused
CSS contributes neither tokens nor theme CSS and falls back to the saved built-in
palette. Refused standalone CSS contributes no standalone rules; it does not
invalidate an otherwise valid theme. Application returns source-qualified errors.

## Scoped CSS and recovery

`ui/src/lib/customThemeCss.ts` uses an explicit tokenizer and allowlists rather
than a browser stylesheet parser that could silently discard invalid input.
Each selector is prefixed with:

```
html[data-custom-theme="<scopeId>"] [data-chat-scope]
```

A descendant separator follows that prefix. The scope id is the validated active
document id, or `custom-css` when no document passes document validation. If that
document's CSS is then refused, independently valid standalone CSS retains the
already chosen id while the palette falls back to the built-in preference.
Selectors must fit the plain-selector grammar: no `:root`, `html`, `body`, nesting,
escaped identifiers or functional pseudos. Accepted simple pseudos are explicitly
listed in the module. Every selector in a comma-separated list is prefixed.

Only allowlisted presentational properties are accepted: colours, backgrounds,
borders, outlines, shadows, fonts/text, spacing, opacity and `display`. `position`,
`z-index`, authored custom properties and `!important` are refused. Function names
are allowlisted; resource URLs must use `data:`. External URLs, CSS escapes and
unsupported resource functions are refused. Each sheet is capped at 32 KiB UTF-8.

At-rules are refused except non-nested `prefers-reduced-motion` media wrappers
(with no value, `reduce` or `no-preference`). This excludes imports, font faces,
namespaces, pages, supports and layers. Malformed rules also fail validation.
Any rejected rule refuses the entire sheet; errors identify `rule <n>` and no
partial CSS is returned.

The applier marks real `[data-testid="mobile-chat-workspace"]` containers with
`data-chat-scope` and observes chat mounted later. Preview owners may opt in with
that attribute directly. Prefixed selectors target descendants, not the scope
container itself. Settings and recovery controls stay outside the marked real
chat workspace.

The theme library's **Remove custom CSS** control clears standalone `customCss`;
it does not erase an active document's `css`. Selecting a built-in or deleting the
active document removes that document's layer while retaining standalone CSS.
The editor reports the theme and standalone sources separately. Theme export
includes document `css`; standalone text has its own **Export custom CSS** action.

## Executable precedence reference

The existing todo-6 checks are in `ui/src/lib/customThemeCss.test.ts`; no duplicate
prose-pinning test is needed. The test named
`preserves built-in -> theme CSS -> standalone CSS precedence and exact reset`
uses these values:

| Stage | Fixture declaration | Computed chat button colour |
| --- | --- | --- |
| Built-in fixture | `#chat { color: rgb(0, 0, 255) }` | Captured as `builtin` for restoration |
| Theme CSS | `#chat { color: red }` | Asserted `rgb(255, 0, 0)` |
| Standalone CSS | `#chat { color: green }` | Asserted `rgb(0, 128, 0)` |
| Reset | Defaults reapplied | Asserted equal to captured `builtin`; injected styles absent |

The adjacent token-restoration test asserts a light-base theme writes
`--background: #fafafa`, derives `--background-rgb: 250 250 250`, and derives
`--chat-code-rgb: 0 128 0` from `hsl(120 100% 25%)`. Removing the theme restores
the pre-existing inline `--background: #123456`, clears those newly introduced RGB
and code overrides, and returns `data-theme` to `charcoal`.

These are references to the delivered test assertions, not a new test-run receipt.
This documentation-only change was checked by source reading and cited-path
existence checks; it did not execute tests or a live appearance change.

## T3 Code parity extensions

In addition to the legacy `ThemeDocumentV1` / chat-scoped CSS pipeline documented above,
Ferryx ships full T3 Code theme customization parity across desktop, remote/mobile, and CLI:

- **OKLCH palette engine & curated themes** (`ui/src/lib/themePalettes.ts`):
  - `createVividThemeColors(appearance, canvas, accent)` derives all 61 T3 Code color roles in OKLCH with WCAG contrast guarantees and maps them onto Ferryx tokens (`themeColorsToFerryxTokens`) plus native terminal colors (`themeColorsToTerminalPalette`).
  - Ships all 5 curated dual-mode T3 Code presets (`t3-chat`, `grove`, `ocean`, `ember`, `iris`) with radial preview circles (`ThemePreviewCircles.tsx`) and independent light/dark theme halves (`themeHalves: { light?, dark? }`).
- **T3 Code & VS Code theme import + OpenVSX search** (`ui/src/lib/vscodeThemeImport.ts`, `ui/src/lib/openVsxThemes.ts`, `ThemeImportDialog.tsx`, `ThemeSearchSection.tsx`):
  - `parseAnyThemeImport()` auto-detects Ferryx v1, T3 Code full v1, T3 Code shorthand (`{ name, appearance, canvas, accent, colors?, variants?, css? }`), and VS Code `.json`/`.jsonc` themes.
  - `serializeToT3ThemeFile()` exports standard T3 Code v1 JSON.
  - OpenVSX search extracts themes directly from `.vsix` archives without external ZIP dependencies.
- **App-wide safe Custom CSS & stable selectors** (`ui/src/lib/themeCss.ts`, `ThemeCssEditor.tsx`):
  - Themes authored with `cssScope: "app"` (visual editor, T3 Code import, environment themes) apply CSS app-wide after built-in styles via `applyAppThemeCss()`, stripping remote fetches (`@import`, `image-set()`, `src()`, non-`data:`/`#id` `url()`, fetch escapes) while supporting `.dark`, `:root:not(.dark)`, `@keyframes`, `!important`, `var(--theme-background-image)`, and stable `data-slot` / `data-*` hooks (`THEME_CSS_HOOKS`).
  - Border/input alpha companions (`--border-alpha`, `--input-alpha`, `--sidebar-border-alpha`, `--worktree-sidebar-border-alpha`) are derived from custom token alpha and restored on reset; `.remote-app-root` receives a mirrored custom-token stylesheet so custom themes apply equally on remote/mobile.
- **Visual Theme Editor & live element inspector** (`ThemeEditorPanel.tsx`, `ThemeColorPicker.tsx`, `themeInspector.ts`):
  - Quick OKLCH Canvas/Accent editor, dual light/dark variant authoring, 6-group advanced token editor, live DOM element picker ("Pick from app"), and live preview with exact rollback on Cancel.
- **Command Palette & Remote parity** (`CommandPalette.tsx`, `RemoteAppearanceSheet.tsx`):
  - Interactive Command Palette theme picker with live hover preview across built-in, curated, environment, and saved custom themes.
  - Mobile/remote sheet supports curated themes, environment themes, and T3/VS Code JSON/JSONC import with 44px touch targets.
- **Environment themes, wallpapers, native terminal palette & CLI** (`src-tauri/src/theme/mod.rs`, `src-tauri/src/ipc/theme.rs`, `src-tauri/src/remote/theme_api.rs`, `src-tauri/src/cli/theme.rs`):
  - Publishes/reads JSON themes from `<data_dir>/themes/` (`FERRYX_THEMES_DIR`) with atomic writes and sidecar background images (`<id>.{jpg,jpeg,png,webp}`, <= 16 MiB) exposed via Tauri IPC (`cmd_theme_*`) and authenticated remote HTTP endpoints (`GET /api/v1/themes`, `GET /api/v1/themes/{themeId}/background`).
  - Native terminal renderer accepts custom terminal palette overrides via `cmd_terminal_set_theme_palette`.
  - `ferryx-cli theme <show|set|clear|guide>` (also catalogued in `ferryx-cli api describe theme`) lets users and coding agents inspect, publish, switch, and clear environment themes.

## Explicit non-goals

Server-side remote CSS fetching (`@import`, external `url()`, `image-set()`) remains
blocked at runtime to prevent screen-content exfiltration.
