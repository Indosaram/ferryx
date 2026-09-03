# Ferryx Daemon Update Strategy

## Recommendation

Use a **draining daemon generation** model:

1. The old daemon keeps ownership of every PTY session it already runs.
2. After an app update, the GUI starts a new daemon generation from the new binary.
3. New terminal sessions are created only in the new generation.
4. Existing tabs remain connected to the old generation until they close or the user explicitly chooses to restore them in the new generation.
5. When the old generation owns no sessions, it exits and removes its generation-specific socket.

This updates the backend immediately for new work without killing live shells, pretending that disk metadata can recreate a live PTY, or waiting forever for every old session to close.

## Why not force-restart the daemon?

A live PTY consists of process state, the PTY master file descriptor, terminal modes, environment, job-control state, and the child process tree. The current disk persistence records restoration metadata and terminal history; it does not serialize that live operating-system state. Killing or `exec`-replacing the daemon without an explicit file-descriptor handoff therefore destroys live session ownership.

Agent CLIs can often resume from their own provider session IDs, and plain shells can be recreated in the same CWD, but neither is identical to preserving the running PTY.

## Why not defer one daemon upgrade indefinitely?

The current model waits for `active_sessions.is_empty()`. Long-lived terminal sessions make that condition effectively unreachable. Meanwhile every independently created client can retry the same upgrade request, producing the observed log storm. It also leaves newly opened terminals on the old backend.

## Why not transfer live PTYs into the new daemon?

On Unix, a purpose-built handoff could pass PTY master descriptors with `SCM_RIGHTS`; Windows would need a different mechanism. Correctly transferring stream sequence state, writer ownership, lifecycle watchers, child-reaping responsibility, and in-flight IPC is substantially more complex and riskier than allowing the old daemon to drain. It can be considered later, but it should not be the first fix.

## Required behavior

### Daemon identity

- Give each daemon generation an immutable identifier derived from the binary version/build identity.
- Use generation-specific endpoints, rather than one socket that a new process must steal from the old process.
- Persist `daemonGeneration` beside each backend session binding.

### Routing

- The GUI maintains connections to every generation still owning visible sessions.
- Existing sessions route to their recorded generation.
- Spawn, workspace registration, remote gateway ownership, and other new mutable operations route to the current generation.
- A generation with no owned sessions shuts down automatically.

### Restore and failure handling

- If an old generation crashes, mark its sessions disconnected.
- Agent panes resume with their captured `agentType` and provider session ID.
- Plain shells can be recreated in their saved CWD only through an explicit recovery action or a clearly documented automatic policy.
- Restored terminal history is evidence of prior output, not proof that the old process is alive.

### Upgrade request behavior

- Stop sending `UpgradeBinary` once generation routing is active; the new GUI starts the new generation directly.
- Until generation routing ships, make upgrade suppression process-global and persist one `upgrade_pending` flag in the daemon.
- A deferred request should log once, not once per client or stream.
- The old daemon should execute a pending upgrade only when its final session closes.

## Rollout order

1. **Hotfix:** process-global request deduplication, one deferred log, and daemon-side `upgrade_pending` execution when the final session closes.
2. **Generation identity:** add generation IDs to handshake responses, persisted session bindings, and client routing.
3. **Side-by-side startup:** launch the new generation after app replacement while keeping old-generation connections alive.
4. **Drain lifecycle:** route new sessions to the current generation and automatically retire empty old generations.
5. **Recovery QA:** verify update with live shell, running agent, split panes, app restart, old-daemon crash, and final old-session closure.

## Acceptance criteria

- Installing and launching a new Ferryx build does not terminate any live PTY.
- A new terminal opened after the update runs on the new daemon binary.
- Existing terminals continue receiving input/output through their old daemon.
- Closing the last old-generation terminal retires the old daemon automatically.
- No repeated `UpgradeBinary` or `UpgradeDeferred` logs occur.
- A crashed old generation exposes deterministic reconnect/recreate behavior instead of reporting a false live-session restoration.
