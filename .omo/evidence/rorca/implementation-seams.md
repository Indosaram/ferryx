# rorca implementation seams

Read-only DAG `dag_1fa93a04-6ee9-4789-8ae9-fc932d9b0fcd` completed on
2026-08-21. These are verified handoff targets, not implementation claims.

## Native scope (`src-tauri/**`)

| Requirement | Smallest verified seam |
| --- | --- |
| rorca title/product/icon | `src-tauri/tauri.conf.json` product/window/bundle fields; generated assets under `src-tauri/icons/` |
| Ghostty preferences | New typed parser/DTO beside `src-tauri/src/ipc/terminal.rs`; register command in `src-tauri/src/lib.rs`; parser tests must cover last `font-family` wins, option-as-alt, absent/malformed safe defaults |
| Project registration | `src-tauri/src/worktree/registry.rs` with existing canonical-root validation patterns; typed IPC and camelCase response in `src-tauri/src/ipc/` |
| Branch dropdown data | `src-tauri/src/worktree/git.rs` branch list, manager wrapper, worktree IPC command, then `invoke_handler![]` registration |
| Safety invariants | Preserve `run_blocking`, `WriterLeaseGuard`, `WorkspaceRegistry::resolve_terminal_target`, structured camelCase errors, `WORKTREE_CHANGED_EVENT`, and safe-vs-destructive branch deletion |

Relevant existing tests: `src-tauri/tests/backend_hardening.rs`,
`src-tauri/tests/ipc_hardening_contract.rs`,
`src-tauri/tests/worktree_safety.rs`, `src-tauri/tests/e2e_agent_workflow.rs`,
and `src-tauri/src/ipc/tests.rs`.

## Frontend scope (`ui/**`)

| Requirement | Verified seam |
| --- | --- |
| rorca document identity | `ui/index.html`; master `ui/src/assets/rorca-icon.svg` |
| Compact shell | `ui/src/App.tsx`, `components/Sidebar.tsx`, `WorkspaceHeader.tsx`, `TabBar.tsx`, `index.css` |
| Pane-local controls | `components/TerminalPane.tsx`, `TerminalSplitView.tsx`, pure reducers in `state/layout.ts`; remove header split and global Interrupt in `WorkspaceHeader.tsx`/`App.tsx` |
| Keyboard routing | `lib/shortcuts.ts` must use capture-phase policy and preserve xterm input behavior in `TerminalPane.tsx` |
| Tabs/workspace/search | `components/TabBar.tsx`, `CommandPalette.tsx`, `state/workspaceStore.ts` |
| Project/worktree selector | `components/WorktreeList.tsx`, creation flow in `App.tsx`, typed calls in `lib/tauri.ts` |
| Settings/Ghostty | `components/SettingsDialog.tsx`, `lib/terminalSettings.ts`, `lib/terminalRenderer.ts` |
| Window dragging | `Sidebar.tsx`, `WorkspaceHeader.tsx`, `index.css`; add real `data-tauri-drag-region` only to non-control background |

Relevant existing tests: `components/{Sidebar,WorkspaceHeader,TerminalSplitView,
CommandPalette,SettingsDialog,WorktreeList}.test.tsx`,
`lib/{shortcuts,terminalSettings,terminalRenderer}.test.*`, and
`state/{layout,workspaceStore}.test.tsx`.

## Reference inputs

- `ui/original-dist/assets/Settings-yKTVxZPa.js`
- `ui/original-dist/assets/{App-D7xQsIRS,QuickOpen-DsnfKLBp,
  terminal-shortcut-policy-BOkUsz_T,request-active-terminal-pane-split-So9AiZw3}.js`
- `/var/folders/zh/7cc25lt91b1_dj577306nwdh0000gn/T/clipboard-2026-08-21-113227-8076FA53.png`

The exclusive frontend Web task is staged in `.omo/evidence/rorca/web-wave2.json`;
it must not start until native IPC and the visual-reference report are available.
