# SSH reconnect after daemon restart

## Result

Disconnected SSH project panes now expose **Reconnect SSH**, independently of
detected agent type or provider-session availability. The action uses the existing
shell replacement path and preserves the frontend pane and workspace identities.
It opens a new SSH shell, not a resumed agent run.

## Cause and changes

- `ui/src/components/TerminalPane.tsx` previously selected the agent disconnect
  overlay when agent metadata remained. Its reconnect button depended on
  `getAgentReconnectAffordance`, which rejects missing provider references.
- `ui/src/lib/shellReplacement.ts` also rejected any session with `agentType`.
  SSH workspaces now bypass that local-agent restriction. The spawn request keeps
  the SSH workspace ID, worktree, and CWD, with `startup: null`. The backend
  `cmd_terminal_spawn` routes SSH workspace IDs to `TerminalStartup::RemoteSsh`.
- `ui/src/state/workspaceStore.ts` clears stale agent references and activity when
  an SSH pane receives its new backend. Existing local agent resume behavior is
  retained.
- The pane disables repeated activation while the request is pending and displays
  structured connection errors inline, leaving reconnection available after failure.

## Evidence

Failing-first regression:

```sh
bun run --cwd ui test src/components/TerminalPane.sshReconnect.test.tsx
```

Before the fix: 4 failures, 1 pass. Failures included
`Terminal session cannot be replaced with a new shell` and a missing accessible
`Reconnect SSH` button.

Related checks:

```sh
bun run --cwd ui test \
  src/components/TerminalPane.sshReconnect.test.tsx \
  src/components/TerminalPane.test.tsx \
  src/lib/agentReconnect.test.ts \
  src/lib/agentReconnect.integration.test.ts \
  src/state/workspaceStore.test.tsx
```

85 tests passed across 5 files. The new fixture was subsequently corrected to use
the actual `LayoutState.layoutsByTabId` contract; its 5 tests passed again.

```sh
bun run --cwd ui build
git diff --check
```

Both completed successfully. LSP diagnostics were unavailable because the LSP
daemon could not be reached; the build's TypeScript check passed.

A temporary, isolated Vite server rendered the actual `TerminalPane` in
`Bun.WebView`, with native terminal and DAG rendering stubbed. Browser interactions
verified pending/disabled state, rejection displayed as an alert, successful retry,
and removal of the disconnect overlay. DOM geometry checks at 1000 x 650 and
390 x 844 found the button within the viewport; all measured mobile labels and
controls had no horizontal overflow. The button measured 220 x 32 pixels.
Screenshots were captured at `/tmp/ferryx-ssh-reconnect-desktop.png` and
`/tmp/ferryx-ssh-reconnect-mobile.png`, but this session's model could not inspect
images, so these checks are not a visual-design verdict.
The temporary server and browser were closed.

## Desktop verification still required

No live daemon was restarted and no existing SSH connection was manipulated.
The browser check does not prove native terminal attachment or a real SSH login.

Using the debug application launched with `bun tauri dev`, open an already
disconnected SSH project pane and click **Reconnect SSH**. Confirm that a remote
shell prompt appears in the same pane and that `hostname` and `pwd` identify the
expected host and project. There is no need to restart the daemon again for this
check.

## Shared workspace

These frontend changes are uncommitted. Concurrent backend changes appeared in
`daemon/remote_ssh_tests.rs`, `daemon/server.rs`, `ipc/terminal.rs`, `ipc/tests.rs`,
and `ssh/worktree.rs`; they were not authored or changed by this session and were
not included in its verification claims.
