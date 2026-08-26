# Remote Mobile Terminal E2E — 2026-08-26

## Outcome

Remote terminals now negotiate a measured viewport grid **before** the grid WebSocket sends its
first frame. A remote device can also request a different terminal tab in the same worktree. The
request remains a Desktop focus request: it never grants an attachment to a background terminal.

The remote/mobile browser title follows the active terminal tab as `<tab title> - Ferryx`. It uses
only the already-published safe tab label, falls back to the active terminal title when no tab list
is present, and returns to `Ferryx` when no terminal is focused, the device disconnects, or the
browser is unpaired.

The mobile chrome is intentionally dense: a 28px Ferryx header, compact Desktop-context selector,
and a 28px terminal row. That row keeps direct tab selection and adds Previous/Next terminal
controls with a `current / total` indicator, so every published terminal in the selected worktree
is reachable without horizontally hunting through the tab strip. Navigation keeps the current
terminal visible until Ferryx Desktop confirms the selected tab.

## Automated evidence

- `cargo test --manifest-path src-tauri/Cargo.toml --lib remote` — **48 passed**.
  - `test_grid_render_initial_frame_uses_requested_viewport_dimensions` proves a `47x18` grid
    query resizes the PTY and produces a first `grid` frame at `47x18`, rather than its former
    desktop `93x27` geometry.
  - `test_remote_select_workspace_with_tab_selector_and_primary_worktree` proves a Control device
    may request a published same-context tab, an unpublished tab is rejected with `400`, and a
    view-only device remains `403`.
  - Existing active-desktop selection, terminal control, and active-session revocation tests pass.
- `bun --cwd ui test src/App.test.tsx src/lib/tauri.test.ts src/remote/RemoteUI.test.tsx src/remote/RemoteTerminal.contract.test.tsx`
  — **103 passed**.
  - The initial socket URL includes measured `cols` and `rows`; later `ResizeObserver` changes
    send a resize only when geometry changes.
  - The mobile selector renders safe same-worktree terminal tabs and posts a tab-scoped selection
    request.
  - The Desktop listener changes the requested tab only after selecting its worktree, including
    selection of the primary worktree (`worktreeSlug: null`).
- `bun --cwd ui test src/remote/RemoteUI.test.tsx` — **17 passed**.
  - The title is set from the active safe tab/terminal on authenticated load, updates after an
    unsolicited Desktop focus change, and resets to `Ferryx` for no-focus, disconnect, and
    unpaired states.
- `bun --cwd ui test src/remote/RemoteUI.test.tsx` — **19 passed** after terminal traversal UI.
  - Previous/Next controls issue a safe tab-scoped request and move the mirror only after the
    matching Desktop active-selection event and confirmed state refresh.
  - The regression walks `1 / 3 → 2 / 3 → 3 / 3 → 2 / 3`, proving the complete sequential
    path and disabled endpoints without timer-based waiting.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` and
  `cargo check --manifest-path src-tauri/Cargo.toml` — passed.
- `cd ui && bun run build` — passed.
- Browser visual QA: the production remote client was rendered at `390x844` and an iPhone 13
  viewport. The unpaired pairing surface remains contained, readable, and horizontally
  overflow-free in portrait.

## Security boundary verified

- Remote workspace state still exposes exactly the Desktop-active terminal session for attach.
- A tab request is accepted only when its ID is in the Desktop-published tab list for the same
  workspace/worktree context; arbitrary or unpublished IDs return `400`.
- Desktop independently verifies that the requested tab belongs to the target worktree before it
  activates it.
- Tab metadata contains only ID and label. Browser tabs and other-worktree terminal tabs are not
  published; path-like labels are normalized to `Terminal` at both the Desktop publish path and
  IPC boundary.
- Existing view-only `403` and focus-change socket revocation behavior are unchanged.

## Required device E2E

Desktop UI and a paired physical mobile device must be used for this portion; they were not driven
by automation.

1. In Ferryx Desktop, enable Remote access and create a **Control** pairing PIN. Pair the mobile
   browser, then leave the Desktop app running and focused.
2. Open a worktree with two terminal tabs (not browser tabs). Give each an unmistakable prompt,
   for example `printf 'TAB-ONE\n'` and `printf 'TAB-TWO\n'` in its respective tab.
3. On the phone in portrait, select the worktree and wait for **Desktop context confirmed**. The
   first rendered terminal grid must already fit the device width: no initial desktop-width
   clipping, compression, or need to rotate/resize before it becomes usable.
4. In the active terminal, run `printf 'MOBILE-GRID-OK\n'`. Confirm the exact text appears on the
   Desktop terminal and remote grid. Rotate to landscape and back; after each resize, confirm the
   grid reflows to the new width and new output remains readable without horizontal clipping.
5. Use the compact Previous/Next terminal controls to move between tabs (the `1 / N` indicator
   tracks the current tab); direct selection from the adjacent tab strip remains available. Confirm
   Ferryx Desktop focuses that exact tab, remote reports confirmation, and the remote terminal
   changes to its `TAB-TWO` output. Navigate back and confirm the reciprocal switch.
6. While on a secondary worktree, select the primary worktree from the remote context selector and
   then one of its terminal tabs. Confirm Desktop selects the primary worktree/tab rather than
   leaving the prior worktree active.
7. As a negative control, change Desktop focus to a different terminal while the phone remains
   connected. Confirm the old remote attachment closes/refreshes and the phone cannot continue
   operating the no-longer-active session.

Record any failed step with the device orientation, selected worktree/tab labels, and whether
Desktop context confirmation appeared.
