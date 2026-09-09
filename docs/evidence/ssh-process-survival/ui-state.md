# Frontend state and persistence for SSH recovery

Status: verified with 32 frontend unit and integration tests.

## Model and state additions

- `TerminalSession` fields:
  - `remoteConnectionState?: "connected" | "reconnecting" | "disconnected" | "expired" | "missing" | "legacyLost"`
  - `remoteGeneration?: number | null`
  - `remoteFailure?: RemoteFailure | null`
  - `remoteReplayGap?: RemoteReplayGap | null`
- Action `SESSION_REMOTE_STATUS` updates remote connection state, generation, and failures without touching local pane layout or erasing agent metadata.
- `SESSION_LIFECYCLE` with `lifecycle: "exited"` for remote workspaces transitions `remoteConnectionState` to `"reconnecting"` instead of destroying the session.
- `replaceExitedShellSession` explicitly rejects replacing SSH sessions with fresh shells.
- `ensureSessionBackends` ignores remote SSH sessions so background reconciliation never spawns replacement shells.
- `startSshRecovery` in `ui/src/lib/sshRecovery.ts` listens to `terminal_remote_status`, reconciles initial state via `getTerminalRemoteStatus`, and tracks daemon epoch changes.

## Verification

- `ui/src/lib/sshRecovery.test.ts`: 11 passed.
- `ui/src/lib/shellReplacement.test.ts`: 2 passed.
- `ui/src/state/workspaceStore.sshReattach.test.ts`: 1 passed.
- `ui/src/state/workspaceRestore.test.tsx`: 18 passed.
