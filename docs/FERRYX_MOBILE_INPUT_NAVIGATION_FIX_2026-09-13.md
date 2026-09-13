# Mobile remote input and navigation fixes

Date: 2026-09-13

## Outcome

Implemented the five reported mobile remote regressions:

- Hangul composition no longer forwards provisional input or duplicates the final composition insertion.
- Worktree lists constrain horizontal overflow while retaining vertical scrolling.
- Terminal touch scrolling owns the gesture surface and flushes remaining whole scroll rows on release.
- Mobile keyboard focus requires a completed tap rather than touch-down or connection lifecycle events. Native mouse defaults cannot steal focus back to the terminal grid. The textarea remains stationary, uses at least 16px text, and survives cursor and viewport changes.
- The tab bar offers New terminal tab, including when the selected worktree has no terminal. It sends an authenticated creation request and waits for publication of a new session.

## Implementation

- `ui/src/remote/RemoteTerminal.tsx`: composition, focus, and gesture ownership.
- `ui/src/remote/RemoteTerminal.mobile.test.tsx`: eight deterministic regression tests.
- `ui/src/remote/RemoteApp.tsx`: visual viewport height and creation request/confirmation/error handling.
- `ui/src/remote/RemoteSessionList.tsx`, `MobileHostDrawer.tsx`: bounded list layout and new-tab control.
- `ui/src/App.tsx`, `ui/src/lib/tauri.ts`: remote creation event reaches the existing desktop `openTab` operation rather than reusing an existing tab.
- `src-tauri/src/remote/protocol.rs`, `server.rs`, `tests.rs`: `createTerminal` on the existing workspace-selection endpoint, preserving authentication and worktree validation. Creation cannot also name an existing tab or session.

The request is `POST /api/v1/workspace/select` with the selected `workspaceId`, optional `worktreeSlug`, and `createTerminal: true`. This requires the updated desktop frontend and gateway, not only refreshed mobile assets.

## Verification

- Terminal RED: all eight added cases failed before the implementation.
- Terminal GREEN: 66 tests across lifecycle, contract, gestures, and grid protocol.
- Combined remote frontend: `bun run --cwd ui test src/remote` passed 13 files and 157 tests, including the final long-workspace regression.
- Frontend typecheck and production web assets: `bun run --cwd ui build` succeeded.
- Rust targeted HTTP integration: `test_remote_select_workspace_with_tab_selector_and_primary_worktree` passed, including creation authentication, invalid worktree, conflicting existing-tab selector, and emitted creation event checks.
- Browser integration: 60 checks passed at actual CSS widths 360, 390, 768, and 1280. Tests use real React components, real headless Chrome, trusted taps/clicks, and fixture-controlled HTTP/WebSocket boundaries.
- At 360px: document width 360, list client/scroll width 342, horizontal scroll 0, and vertical scrollTop 150. Header/navigation controls remained inside the viewport.
- Changed remote component files had no error diagnostics. Fresh `RemoteUI.test.tsx` diagnostics timed out; its tests and the frontend typecheck passed. The terminal retains an existing deprecated `keyCode` hint.
- The UI mechanical detector returned `[]`; `git diff --check` passed.

Browser harness, 24 screenshots, request traces, and detailed results:

- `docs/evidence/mobile-remote-20260913/README.md`
- `docs/evidence/mobile-remote-20260913/qa.mjs`
- `docs/evidence/mobile-remote-20260913/post-fix/results.json`

## Limits and unrelated failures

This is not an end-to-end claim about a real phone keyboard or a running desktop PTY. Scripted composition sequences cannot prove every native iOS/Android IME ordering. Browser scrolling checks establish wire messages, not actual PTY scrollback. Viewport resizing establishes DOM focus retention, not native keyboard visibility.

Screenshot capture succeeded, but image inspection was unavailable: both parent and child image reads reported that their models cannot receive images. Layout containment is therefore DOM-measured, not a pixel-review verdict. The new-tab button uses the incumbent 28px navigation control size, below the 44px mobile target recommendation.

The broader App/IPC test run exposed four unrelated existing failures: startup update checking, two focused-terminal payload expectations missing `attentionInventory`, and a notification probe expectation missing `sound`. Those paths were not changed to make the tests pass. An initial Rust build interruption from concurrent worktree-rescan edits cleared before the successful targeted HTTP test.

No release artifact was built, no desktop application or daemon was restarted, and no repository commit was created. Foreign worktree changes were preserved.

## Required real-device acceptance

Use the updated debug desktop/gateway (desktop launch command: `bun tauri dev`) and reload the remote client.

1. Enter several Korean sentences in the terminal, including repeated syllables, spaces, backspace, and Enter. Check for separated jamo, lost text, and duplicates.
2. Scroll the terminal up and back to the bottom. The gesture must scroll terminal history and must not summon the keyboard.
3. Tap the prompt once. The keyboard must remain open during typing, output updates, and viewport resizing.
4. Open the worktree picker with long names. Vertical scrolling must work without sideways movement.
5. Press New terminal tab in both a populated worktree and a selected worktree with no terminal. Confirm a real new desktop terminal in the intended directory, then type in it from mobile.

These physical-device and live-desktop checks remain pending user verification.
