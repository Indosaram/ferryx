# A20 - desktop shell ownership and capability gating

Status: partial implementation verified by UI tests; AC04 NOT met. Required UI build blocked by concurrent out-of-lane TypeScript errors. No live paired terminal or actual macOS menu behavior is claimed.

## Implemented

- App no longer submits stored paired project paths to local registration (nor SSH registration). Existing daemon-owned references retain their host-qualified workspace identity and metadata.
- App subscribes to the existing machineFeaturesEnabled gate. Disabled paired projects retain the desktop shell and display an unavailable-support alert; new-terminal and split entry points reject terminal creation while disabled.
- Paired references with the gate enabled proceed through existing worktree refresh/restore, not local path registration. This enabled path still depends on A16 native proxy integration and is not established by these tests.
- The mirror branch and active-host global guards were already absent on entry; no removal was necessary. No shortcut matcher, mobile entry point, Rust menu, session registry, worktree mutation, or agent-boundary code changed.

## Evidence

- A20-RED.log captures the real pre-fix failure: paired bootstrap called local registerProject twice with /srv/repo. The initial local-shell fixture also incorrectly expected tab creation through spawnTerminalDetailed; inspection showed open-tab uses spawnTerminal and splitting uses spawnTerminalDetailed. Those assertions were corrected, not production behavior.
- The same RED log records a deliberate assertion inversion after the fix: requiring local registration fails. The assertion was restored.
- A20-GREEN.log retains development results and the final single focused invocation: Node v22.22.3, 6 test files / 99 tests passed (A20 paired shell, App SSH, project metadata, shortcuts, mobile zero-config security, mobile routing).
- Tests mount real App/workspace state with mocked IPC and terminal rendering. They check disabled paired registration/creation, catalog preservation, persistent main/palette/local tab identity across host inventory selection, and local new-tab/split menu callbacks plus keyboard new-tab. They use awaited React act scopes, not sleeps or polling.
- LSP diagnostics initially reported no diagnostics for both changed source files. After the final test-only additions, the LSP request timed out; the build's TypeScript pass reported only the external errors below.
- Required `bun run --cwd ui build` exited 2: ui/src/lib/pairedWorktreeActions.ts:44 TS18048 (result.outcome.error possibly undefined), :47 TS2345 (possibly undefined worktree). This is concurrently edited A21 code, untouched by A20. No suppression or repair attempted.
- No Rust changes, so no cargo check required. No desktop automation, daemon/PTY access, deployment, release build, or commit performed. Commands used an isolated HOME under this worktree, removed at completion.

## Unproven / required human desktop QA

1. After A16 integration and a successful UI build, enable the advertised native proxy gate and open an actual paired-daemon project. Verify native terminal output, tab creation, splits in both directions, pane focus, tab navigation, close-pane/close-tab, pinned tabs, and persistence/restart lifecycle, comparing Local and SSH.
2. Exercise actual macOS File menu New Terminal Tab/Close Tab, Window behavior, and native shortcut forwarding (new tab, split, close, tab/worktree selection, palette, sidebar, Settings). These tests invoke menu callbacks, not AppKit menu items.
3. Change host inventory selection in Settings while the palette/layout are open; then create/split/close a local-project terminal without clearing inventory selection.
4. Test paired feature disablement/re-enablement with populated saved split layouts; verify layouts are retained and no unintended backend operation occurs. Test offline, revoked and expired paired ownership through the completed A16/A21 surface.
5. Verify physical Cmd+V/C with Korean 2-set input and native terminal focus. Shortcut matching was not changed by A20.

Assumption: paired projects are existing daemon-registered references, not local filesystem roots. The shell must not re-register them from a raw remote path. Browser/jsdom tests do not establish actual native PTY or macOS lifecycle behavior.
