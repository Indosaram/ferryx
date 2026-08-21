# rorca desktop UI parity — execution plan

## Objective

Replace the current Orca Lite desktop surface with **rorca**: a compact, charcoal, original-Orca-like workspace UI, crab-containing Orca-style identity, working terminal/session keyboard controls, real project/worktree flows, Ghostty-backed terminal defaults, and a settings surface that is functional rather than decorative.

Reference image: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-21-113227-8076FA53.png`

Visual/code references:

- `ui/original-dist/`
- `ui/original-dist/assets/Settings-yKTVxZPa.js`
- `ui/original-dist/assets/plugin-manifest-Bs-50M_g.js`
- `ORCA_LITE_PARITY_AUDIT.md`
- `ORCA_LITE_PARITY_FIX_EXECUTION.md`

## Explicit decisions

### Identity

- Product/window/browser title: `rorca`.
- Create a new crab-containing Orca-style icon with the image generator; use a dark graphite round-square, compact coral crab silhouette, and restrained cyan/blue Orca-style eye/wave accent.
- Generate native/macOS derivatives from the accepted master asset and wire the Tauri bundle icon list. Do not modify the original Orca application.

### Terminal controls and shortcuts

- The app-level shortcut layer must listen in capture phase and must not dispatch app commands while a terminal pane is receiving ordinary text input.
- For modifier shortcut chords, `preventDefault()` only after the chord matches a registered action. Plain typing and Ctrl-C must continue to reach xterm.
- Register and surface: new tab, close tab, next/previous tab, horizontal split, vertical split, unsplit, command palette.
- Each terminal pane owns its own split/menu affordance. Remove the global `Interrupt terminal` button; the terminal retains raw Ctrl-C delivery and any contextual menu action uses the existing signal IPC only when a foreground process is present.

### Project/worktree UX

- Add Project is a first-class flow: choose a local Git repository, create/register a workspace identity, then show it in the project rail.
- Add Worktree begins from the selected project and uses a **branch dropdown/combobox** populated from real local branches; no arbitrary branch text input.
- Workspace focuses the active project/worktree rail. Search opens the command palette. Tab actions perform real tab/session transitions.

### Settings and Ghostty

- Settings follows the original Orca information architecture: a compact left settings nav and a spacious right detail panel, with real sections only.
- In-scope sections: General/Appearance, Terminal, Keyboard Shortcuts, Workspace.
- Native Ghostty parsing is authoritative for imported defaults. Repeated `font-family` keys are last-wins (`Noto Sans KR` in the currently observed config); explicit rorca local setting overrides import; malformed/absent config falls back to safe existing defaults.
- Map `font-family` to xterm `fontFamily`; map `macos-option-as-alt=true` to the terminal input behavior/preferences surfaced in settings. Store the imported source/status visibly; never fake a Ghostty setting.

### Visual and native window behavior

- Match the reference viewport at 1280×850: 236px compact charcoal rail, low-chrome tabs, subtle #23262d workspace background, 1px muted separators, no oversized cards/rounded modal aesthetics.
- The titlebar uses a Tauri drag region only over noninteractive background; every button/input/resize/split control is `no-drag`.
- Sidebar resize remains interactive and persists its width.

## Failing-first proof plan

| Requirement | RED proof before implementation | GREEN proof | Real-surface proof |
|---|---|---|---|
| rorca identity/icon | new native metadata test asserts legacy identity/no rorca asset | config/resource test proves rorca metadata and generated assets | launch app, title/icon screenshot |
| shortcut routing | new `shortcuts`/`App` tests fire exact `KeyboardEvent`s while terminal/nonterminal targets vary; legacy behavior fails | registered chords invoke actions; plain typing stays unhandled | focus canvas, issue chords, screenshot tab/pane states |
| split controls | existing/global header control expectation fails when pane-local contract is asserted | pane-local control tests pass, global interrupt absent | click per-pane control, visual proof |
| Project/branch flow | component/store tests expect repo selection + dropdown data and fail | tests prove selected repo/branch actions dispatch typed state | Add Project + Add Worktree dropdown action log/screenshot |
| settings/Ghostty | parser tests for repeated values/malformed input and component tests for displayed effective values fail | parser/settings/xterm option tests green | Settings screenshot, Ghostty values visible and applied |
| titlebar drag | component styling test shows interactive controls inside drag region fail | controls carry no-drag, background carries drag-region | drag titlebar then click control, capture moved window screenshot |
| visual shell | baseline reference comparison documents old hierarchy mismatch | screenshot inspection shows compact rail/canvas/settings composition | visual screenshot/compare artifact at 1280×850 |

## Mass-ULW topology

Write ownership is intentionally restricted: UI code overlaps across the app shell, terminal, settings, and workspace state. Parallel UI writers would cause conflicts.

```text
wave 1:
  native-contract  (src-tauri/**, icons/**, native config only)
  visual-reference (read-only original-Orca/reference measurement report)
wave 2:
  frontend-parity  (ui/** only; consumes native contract and visual report)
wave 3:
  local-gates      (read-only cargo/bun/test/build)
wave 4:
  final-verifier   (read-only evidence/citation/visual/real-surface verdict)
```

| Node | Category | Reason | Scope |
|---|---|---|---|
| `native-contract` | `unspecified-high` | typed Rust/Tauri and Ghostty boundary work | `src-tauri/**`, native icons |
| `visual-reference` | `visual-engineering` | extract layout/token/settings structure from original renderer and target image | read-only |
| `frontend-parity` | `visual-engineering` | UI controls, state wiring, CSS, keyboard handling and tests overlap | `ui/**` exclusively |
| `local-gates` | `unspecified-high` | mechanical regression gates | read-only |
| `final-verifier` | `unspecified-high` | cross-cutting evidence and real-surface acceptance | read-only |

The `frontend-parity` node starts only after the native contract and visual reference results are available. If an implementation node cannot attach because of a provider/capacity error, do not retry blindly; record the terminal error and choose a fresh, explicitly documented fallback.

## Implementation sequence

1. Capture baseline screenshot/tree and existing keyboard UI failure. Store under `.omo/evidence/rorca/baseline/`.
2. Generate and inspect the icon master; create platform assets only after visual approval.
3. Write failing native tests:
   - Ghostty config duplicate-font precedence, absent/malformed fallback.
   - typed Tauri IPC result for effective terminal preferences.
   - rorca identity/bundle metadata where testable.
4. Implement native Ghostty reader, preference DTO/command, rorca title/bundle/icon integration. Run Rust RED→GREEN and clippy.
5. Write frontend failing tests in narrow increments:
   - keyboard chord routing, terminal-target guard, pane-local split controls;
   - project add/branch dropdown/workspace/search actions;
   - settings sections/effective Ghostty application;
   - titlebar no-drag boundaries and removal of interrupt control.
6. Rebuild frontend in this order:
   - app shell/tokens/compact rail;
   - terminal tab/pane header and capture shortcut routing;
   - Project + worktree branch selector flow;
   - settings navigation/detail surface and Ghostty bridge;
   - sidebar/titlebar drag behavior and icon/title polish.
7. Execute gates:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
   - `bun run --cwd ui test`
   - `bun run --cwd ui build`
8. Real GUI QA at 1280×850 with `cargo tauri dev` and `orca computer`:
   - title/icon/shell screenshot;
   - terminal shortcuts/pane controls;
   - Project/worktree branch dropdown;
   - settings Ghostty values;
   - titlebar drag plus clickable control;
   - compare final screenshot against reference.
9. Cleanly stop `cargo tauri dev`, Vite and temporary QA resources; write cleanup receipt and append final evidence.

## Risks and mitigations

- Synthetic typing may not reach xterm canvas: treat as an automation channel issue until a real mouse/keyboard or alternate OS-level input path proves otherwise; do not mark shortcut functionality as verified without a captured state transition.
- Ghostty config is user-controlled: parse a small supported subset, preserve unknown keys, and never execute config.
- Add Project needs a trusted filesystem boundary: reuse/extend the native workspace registry and validate/canonicalize paths; never accept a raw unverified path into Git operations.
- Pixel-perfect is scoped to the supplied 1280×850 visual hierarchy and original-Orca-like density/tokens, not unsupported browser/editor/agent orchestration surfaces.
- No commits are authorized in this session. Keep all changes uncommitted and report the final diff.

## Acceptance record

Append RED/GREEN outputs, screenshot paths, action logs, diagnostics, gate outputs, and cleanup receipt here after each completed criterion.
