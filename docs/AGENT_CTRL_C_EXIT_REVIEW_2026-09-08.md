# Agent Ctrl+C exit review

Date: 2026-09-08

Scope: Current Ferryx working tree. Read-only review of production code; no
implementation changes. Existing unrelated uncommitted changes were preserved.

## Conclusion

There are concrete paths where an intentional agent exit appears disconnected,
where reconnect is disabled after an actual PTY exit, and where Ctrl+C delivery
does not match the user's physical key presses. This review does not establish
loss of an agent's saved conversation or failure of any particular agent's
internal shutdown handler.

The reviewed UI calls the condition "Session disconnected", not "session lost".
Distinguish the agent conversation, the agent process, and the daemon-owned PTY:
exiting an agent launched inside a shell normally leaves the PTY alive, whereas
exiting an agent launched directly by the resume path ends that PTY.

## Findings

### F1 - P1: A terminal exit retains the backend ID and disables reconnect

References:

- `ui/src/state/workspaceStore.ts:316-321`
- `ui/src/state/workspaceStore.ts:1969-1994`
- `ui/src/lib/agentResumeAffordance.ts:155-164`
- `ui/src/components/TerminalPane.tsx:92`
- `ui/src/components/TerminalPane.tsx:203-205`

The lifecycle subscriber dispatches `SESSION_LIFECYCLE`. Its reducer updates
`lifecycle` to `exited` but retains `backendSessionId`. The overlay treats
`lifecycle === "exited"` as disconnected, while the reconnect affordance requires
both `lifecycle === "exited"` and `backendSessionId === null`.

Consequently, a live exit event produces a disconnected agent pane without an
enabled reconnect action, even with a valid provider session reference. An
especially direct trigger is Ctrl+C exit after agent resume: the daemon launches
the resume command as the PTY child, not inside a surviving interactive shell
(`src-tauri/src/terminal/shell.rs:397-400,410-432`).

Execution evidence: Vite SSR loaded the real `workspaceReducer`,
`createLayoutState`, and `getAgentReconnectAffordance`. A running Claude fixture
with backend `review-pty` and a valid provider reference received
`SESSION_LIFECYCLE(exited)`. The result was:

```json
{
  "lifecycle": "exited",
  "backendSessionId": "review-pty",
  "reconnectLifecycle": "idle",
  "affordanceStatus": "none",
  "canReconnect": false,
  "reason": "Session is already active"
}
```

Recommendation: Make the exited-session binding invariant consistent across
the lifecycle reducer and reconnect/conflict checks. Add a test that starts
from a live binding and feeds an exit event, rather than constructing an
already-exited fixture with a null backend.

### F2 - P1: An input receipt failure can replay an already-delivered Ctrl+C

References:

- `src-tauri/src/ipc/native_terminal.rs:879-907`
- `ui/src/components/NativeTerminalPane.tsx:838-860`

The native input command first writes bytes to the daemon, then obtains a window
and dispatches a main-thread receipt query. Those later operations can fail
after the PTY has already received the input. The frontend retries every input
error after reattaching, without distinguishing pre-write failure from
post-write receipt failure.

If the same surface owner remains current and recovery succeeds, one physical
Ctrl+C can therefore be delivered twice. For an agent that treats the first
Ctrl+C as cancel and a subsequent Ctrl+C as exit, this can cross from cancellation
into termination. The same issue applies to other non-idempotent terminal input.

This is a code-proven conditional path, not a measured occurrence or a claim
that every Ctrl+C is duplicated. The surface-owner guard prevents some stale
retries but does not establish that the first write failed.

Recommendation: Separate input acceptance from optional receipt/render failure,
and retry only failures known to precede input delivery.

### F3 - P2: Non-working agent states swallow Ctrl+C and synthesize extra interrupts

References:

- `ui/src/components/NativeTerminalPane.tsx:496-501`
- `ui/src/components/NativeTerminalPane.tsx:883-962`
- `ui/src/components/NativeTerminalPane.tsx:2194-2203`
- `ui/src/state/workspaceStore.ts:2058-2067`
- `ui/src/components/NativeTerminalPane.test.tsx:5429-5608`

For any detected agent whose activity is not exactly `working`, the first
Ctrl+C is changed to Ctrl+U. This includes `waiting`, which represents a
backend `blocked` state, as well as missing activity when persisted agent
metadata is present. Repeated presses more than 700 ms apart continue sending
only Ctrl+U, so they never deliver the requested interrupt.

A second press within 700 ms instead sends Ctrl+C immediately and schedules
another Ctrl+C 120 ms later. The delayed callback does not verify that the same
agent process is still in the foreground. If the first interrupt already
returns control to the shell, the delayed interrupt can reach that shell or
its next foreground command. The callback's existing surface-owner guard is
not an agent-process identity check.

The existing tests explicitly lock in `u, u` for spaced presses and `u, c, c`
for a double press. They verify this policy, not successful cancellation or
graceful agent shutdown.

Recommendation: Keep actual Ctrl+C delivery separate from an editor-clear
gesture. At minimum, do not infer editable idle state from every non-working
activity state or assume all agents share the same double-interrupt protocol.

### F4 - P2: Normal PTY exit and unexpected disconnection lose their distinction

References:

- `src-tauri/src/terminal/pty.rs:243-245,284-296`
- `src-tauri/src/daemon/server.rs:2529-2543`
- `src-tauri/src/ipc/terminal.rs:555-563`
- `ui/src/state/workspaceStore.ts:316-321`
- `ui/src/components/TerminalPane.tsx:92-99,189-190`

The PTY watcher obtains an exit code, but the daemon output stream emits
`Exit { exit_code: None }` on channel closure. The desktop event has no reason,
and the workspace action retains only the lifecycle. Finally, all exited
agent panes use "Session disconnected", regardless of intentional exit,
successful completion, or failure.

A correctly terminating directly-launched/resumed agent can therefore look
like a lost connection. This display is not evidence that its saved provider
conversation was deleted.

Recommendation: Preserve termination information through the daemon protocol
and UI state, and distinguish intentional/normal exit from transport loss.
An exit code alone should not be treated as proof of user intent.

### F5 - P2: Agent-owned status has no end-of-process release within a surviving shell

References:

- `src-tauri/resources/agent-extensions/ferryx-agent-state.ts:87-124`
- `src-tauri/src/native_terminal/surface_host.rs:497-499`
- `src-tauri/src/native_terminal/surface_host.rs:1556-1566`
- `ui/src/state/workspaceStore.ts:2170-2202`

The bundled OMO extension reports session start, agent start, settled, and blocked
changes, but has no session shutdown handler. Once a report arrives, the native
PTY session sets `agent_reports_own_state = true`; subsequent terminal output
skips screen-state detection. That ownership is not released when the reporting
agent process exits inside a shell.

If Ctrl+C ends that process before a final settled/unblocked report arrives,
the shell can remain usable while the last working/waiting state remains.
The shell title path deliberately preserves in-flight screen-sourced state.
Even when the last report was idle, a later non-reporting agent in the same
PTY cannot regain screen-based state detection through this path.

This is a missing lifecycle path, not proof that a particular OMO runtime
always omits its final settled event.

Recommendation: Scope authoritative status ownership to an agent incarnation
and release it on confirmed process/session end. A final idle report alone is
insufficient to restore detection for the next process.

## Checks and limitations

- Existing tests ran once:

```sh
cd ui
node node_modules/vitest/vitest.mjs run \
  src/components/NativeTerminalPane.test.tsx \
  src/lib/agentResumeAffordance.test.ts \
  src/state/workspaceStore.test.tsx \
  --maxWorkers=1
```

- Result: 3 files passed, 212 tests passed, exit code 0.
- The production reducer/affordance execution reproduced F1 without changing
  source or test files. Its temporary in-process Vite SSR configuration emitted
  a dependency-scanner alias warning for `@/lib/cn`; the requested modules loaded
  and the reproduction completed. The Vite server was closed afterward.
- F2, F4, and F5 are supported by code-path inspection, not injected live IPC
  faults or real-agent shutdown tests.
- Remote Ctrl+C follows a different path: `RemoteTerminal.tsx:598-602` sends
  `signal: interrupt`; `terminal/session.rs:210-214,240-244` writes byte `0x03`
  on both Unix and non-Unix targets. The desktop Ctrl+U/double-send policy
  should not be generalized to that remote path.
- No desktop input was injected, no user agent was interrupted, and no daemon
  was restarted or terminated. No live Windows/Linux shutdown validation or
  backend build was performed.
- LSP symbol inspection was unavailable because its daemon did not become
  reachable. Two parallel review tracks could not start because the delegated
  model's OAuth refresh failed; findings were reviewed directly instead.
- Existing tests use timing-based waits and auto-advancing fake timers in some
  cases. Their passing result is not an end-to-end graceful-shutdown guarantee.

## Proposed order

Fix F1 and F2 first, then settle the intended Ctrl+C policy in F3. Preserve
termination reason for F4 and add an explicit agent-incarnation end transition
for F5. Implementation requires approval following this review.
