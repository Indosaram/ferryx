# rorca completion audit checklist

This is the acceptance ledger for `.omo/plans/rorca-ui-parity.md`. Entries are
evidence-based: a pending or unverified item is not accepted as complete.

| Requirement | Required artifact / proof | Current state |
| --- | --- | --- |
| Durable plan and outcome | `.omo/plans/rorca-ui-parity.md` and a completed outcome markdown in the repo | Plan exists; outcome documented in `.omo/evidence/rorca/final-outcome.md` |
| Crab-Orca app icon | Source SVG, generated Tauri icon resources, failing-first asset test, inspected output | `ui/src/assets/rorca-icon.svg`; generated `src-tauri/icons/*`; RED `ENOENT`, then targeted Vitest GREEN; generated 128px PNG visually inspected |
| rorca identity | Failing-first title/product/resource test, final `tauri.conf`/HTML inspection, running app title | Browser HTML RED (`omo bridge` / Vite icon) then GREEN: `src/index-html.test.ts` and `src/assets/rorca-icon.test.ts` passed 2/2; native product/window metadata verified |
| Compact original-Orca shell | Reference token/layout report, final 1280x850 screenshot, comparison artifact | `.omo/evidence/rorca/final-visual-comparison.md` PASS; 236px rail, 36px header, 32px tab strip, 896px settings panel verified |
| Terminal shortcut routing | RED/GREEN UI/store tests for all chords and PTY ownership; real action log/screenshots | 24 focused shortcut tests passing in `ui/src/lib/shortcuts.test.tsx`; pending user desktop check for live keyboard chords (Cmd/Ctrl numeric, Cmd-Option-D) |
| Pane-local controls / no global Interrupt | Source/test assertion plus desktop accessibility tree and screenshot | Pane-local controls verified with 13 focused split tests in `TerminalSplitView.test.tsx`; global Interrupt removed; pending user desktop check for nested 3-pane behavior |
| Tabs/workspace/search controls | RED/GREEN dispatch tests and desktop action log/screenshots | Automated store/component tests passing (96 UI tests green); pending user desktop verification |
| Add Project / worktree branch dropdown | Native IPC + UI RED/GREEN tests; real dialog/branch selection action log | Native & frontend tests verified (`native-worker-receipt.md`, `ProjectDialogs.test.tsx`); branch dropdown and workspace flows green |
| Ghostty import/settings application | Parser/component RED/GREEN tests; settings screenshot showing imported values | Native parser RED→GREEN receipt: `.omo/evidence/rorca/native-worker-receipt.md` (last-wins font, option-as-alt, safe defaults); settings UI tested in `SettingsDialog.test.tsx` |
| Titlebar drag and control exclusions | Drag classification test and real titlebar drag / sidebar resize evidence | Titlebar capability contract and no-drag exclusions verified in source/tests; pending user desktop window movement check |
| Regression gates | `cargo test`, `cargo clippy -- -D warnings`, `bun test`, `bun build`, LSP results | **ALL GATES PASS**: `bun run --cwd ui test` (22 files, 96 tests green), `bun run --cwd ui build` success, `cargo test` success, `cargo clippy -D warnings` clean |
| Visual proof | Baseline/final images and comparison/inspection under `.omo/evidence/rorca/` | PASS in `.omo/evidence/rorca/final-visual-comparison.md` (remediated 1px rail divider and full tab bar card background) |
| Cleanup | Dev process/PTY teardown receipt with no leaked app terminal process | Verified clean |

## User-Run Desktop Manual Checks (Pending)

Refer to `.omo/evidence/rorca/final-outcome.md` for the manual acceptance checklist:
- [ ] Nested three-pane behavior
- [ ] Cmd/Ctrl numeric selections (`Cmd/Ctrl+1..9`)
- [ ] Cmd-Option-D split shortcut
- [ ] Titlebar window movement / dragging

## Existing baseline evidence

- Reference: `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-21-113227-8076FA53.png`
- Baseline shell: `.omo/evidence/rorca/baseline/`
- Split shortcut action screenshot:
  `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/orca-computer-use/0110f852-0ff2-463c-87fa-ec8df42d202a-screenshot.png`
- Settings baseline screenshot:
  `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/orca-computer-use/a9f87ac4-497b-4a88-b0c0-487e92e6f899-screenshot.png`

## Completion rule

The objective is complete only when every table row has direct positive evidence.
Passing test commands are insufficient for the desktop interaction and visual rows:
each must also have fresh real-surface screenshots/action logs.
