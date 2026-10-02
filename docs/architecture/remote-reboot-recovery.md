# Remote SSH Reboot Recovery Architecture

**Status:** Implemented; deployment is separate.
**Scope:** Remote SSH terminal and agent session recovery across host reboots.
**Target Path:** `docs/architecture/remote-reboot-recovery.md`

---

## 1. Overview & Problem Definition

When a remote host reboots (scheduled kernel updates, hardware recycling, power interruptions), all processes executing on that host terminate abruptly. In local desktop Ferryx environments, a rolling handover preserves PTY master file descriptors inside a long-running daemon process. However, across a remote machine reboot:
1. All remote helper daemons, PTY masters, OS child processes, and memory state cease to exist.
2. The local Ferryx desktop client detects the severed SSH tunnel and marks the remote session as `disconnected` or `expired`.
3. Historical behavior either left the pane permanently dead or speculatively spawned a bare interactive shell (`/bin/bash` or `/bin/zsh`), discarding agent context, working directory, and conversation transcripts.

Remote SSH Reboot Recovery provides an authoritative, deterministic lifecycle allowing remote sessions—specifically long-running AI agent sessions such as OMO—to recover after physical host reboots without altering frontend pane layout, without mutating logical daemon session identities, and without claiming impossible memory hibernation.

---

## 2. Logical ID, Backend Session ID, and Incarnation

A foundational architectural requirement in Ferryx is the strict separation between visual, daemon-logical, and remote execution identities:

```
+-------------------------------------------------------------------------+
| Frontend Layout / UI (Desktop Client)                                   |
|   Leaf ID: "leaf-3"                                                     |
|   Logical Session ID (Pane ID): "pane-ssh-7a91" (DURABLE & INVARIANT)   |
+-------------------------------------------------------------------------+
                                    |
                                    v (1:1 stable mapping)
+-------------------------------------------------------------------------+
| Local Daemon Session Layer                                              |
|   Backend Session ID: "backend-ssh-7a91" (STABLE LOGICAL DAEMON ID)     |
|   - Persisted across local restarts, handovers, and remote reboots      |
|   - NO backend rebind or ID replacement occurs during recovery          |
+-------------------------------------------------------------------------+
                                    |
                                    v (points to transient remote execution)
+-------------------------------------------------------------------------+
| Remote Incarnations (Transient Host Processes)                          |
|   Incarnation 0 (Pre-Reboot):                                           |
|     - TargetRef: target-alpha-1                                         |
|     - Remote Helper PID: 1042                                           |
|     - Child PTY PID: 1089 (omo agent)                                   |
|     - Boot ID: "linux-boot-7a91bc02..."                                 |
|                                                                         |
|   [HOST PHYSICAL REBOOT OCCURS - INCARNATION 0 DESTROYED]               |
|                                                                         |
|   Incarnation 1 (Post-Reboot Recovery):                                 |
|     - TargetRef: target-alpha-2 (NEW REMOTE TARGET REF)                 |
|     - Remote Helper PID: 2014                                           |
|     - Resumed PTY PID: 2055 (`omo --session <providerSessionId>`)       |
|     - Boot ID: "linux-boot-f904de11..." (STABLE KERNEL PER-BOOT ID)     |
+-------------------------------------------------------------------------+
```

### Invariants:
1. **Durable Frontend Identity (`session.id` / `paneId`):**
   The visual tab, binary split position, terminal pane instance, and unread badge state remain anchored to `session.id`. The pane is never destroyed, swapped, or converted into an unmanaged standby shell.
2. **Stable Daemon Identifier (`backendSessionId`):**
   `backendSessionId` is the daemon's logical session identifier. It remains invariant across connection drops and remote reboots. Recovery does **not** rebind or mint a new `backendSessionId`; the daemon re-attaches the existing logical session to the newly spawned remote helper target.
3. **Transient Remote Incarnation (`TargetRef` / remote PID):**
   Only the remote target reference and remote PTY process identity change across a reboot.

---

## 3. Exact Provider Resume Contract

Ferryx does not engage in heuristic process guessing or speculative command-line synthesis. Recovery is governed by strict metadata recorded at session launch and updated on authoritative agent lifecycle reports.

### Authoritative Record Schema (`RecoveryRecord`):
```json
{
  "logicalSessionId": "original-client-request-id",
  "host": "omarchy",
  "exactPreviousTarget": {
    "hostId": "omarchy",
    "ownerId": "helper-owner",
    "epoch": "7826902970500486205",
    "backendSessionId": "remote-pty-1"
  },
  "bootId": "linux-boot-7a91bc02...",
  "projectId": "ssh:omarchy:omo-native-rs",
  "projectRoot": "/home/indo/projects/omo-native-rs",
  "worktree": null,
  "cwd": "/home/indo/projects/omo-native-rs",
  "cols": 120,
  "rows": 34,
  "agent": "omo",
  "providerSession": {
    "key": "session_id",
    "id": "01a0f117-dfe2-7de2-8c71-3343ae3ab192"
  },
  "disabled": false,
  "updatedAt": 1790946591
}
```

### Resume Resolution Rules:
1. **Agent Whitelisting & Validation:**
   - OMO requires an existing journal whose session header identifies the exact saved ID.
   - Claude and Codex additionally require an authenticated `transcriptPath` with a matching provider journal header.
   - Other providers are refused; stored programs, arguments, environment variables, and shell scripts are never replayed.
   - If `agent` is missing or unrecognized, recovery rejects with `REMOTE_RECOVERY_UNSUPPORTED`.
2. **Provider Session Identity:**
   - The provider session ID is extracted from `providerSession.id` or `providerSession.sessionId`.
   - It is strictly validated: non-empty, maximum 128 ASCII alphanumeric characters plus safe delimiters (`-`, `_`, `.`, `:`). Any illegal character immediately rejects with `REMOTE_RECOVERY_INVALID`.
3. **Exact Command Reconstruction:**
   - Commands are allowlisted: `omo --session <id>`, `claude --resume <id>`, or `codex resume <id>`.
   - A fixed Windows `cmd.exe /d /s /c call` wrapper supports installed `.cmd`/`.bat` CLI launchers; unsafe wrapper arguments are rejected.
4. **No Speculative Shell Fallback:**
   - If recovery fails, or if an agent reference is missing, Ferryx **never** spawns a default shell (`/bin/sh`, `/bin/zsh`, `cmd.exe`) under the guise of recovery.
   - The failure error is surfaced directly in the pane overlay (`role="alert"`), and "Open new shell" remains an explicit, separate user affordance.

---

## 4. Boot Proof & Physical Reality (No Process-Memory Recovery)

### Stable Kernel Per-Boot Identity
To prove that a remote helper restart was caused by a real operating system reboot rather than an intentional daemon restart or transient network drop, Ferryx captures a stable kernel per-boot identity:
- **Linux:** `/proc/sys/kernel/random/boot_id` (a persistent UUID generated at kernel boot) or `/proc/stat` `btime` (system boot time in seconds since epoch).
- **macOS:** `sysctl kern.boottime` (retrieving the kernel `timeval` boot timestamp).
- **Windows:** Stable kernel per-boot identity (such as the kernel boot GUID / system boot environment UUID), rejecting wall-clock-minus-uptime arithmetic to remain completely immune to clock drift, NTP adjustments, or sleep/wake skew.

### Technical Truth: No Process-Memory Hibernation
- **What is NOT recovered:** Process virtual memory, CPU registers, RAM contents, running threads, open network sockets, uncommitted pipe buffers, and background child process trees are **permanently terminated** by the operating system reboot. Ferryx makes no claim of OS process memory snapshotting.
- **Why recovery works:** Recovery relies entirely on the persistence contracts of the underlying agents (e.g. OMO agent journals and checkpointed session transcripts stored on the remote filesystem). When Ferryx re-launches `omo --session <providerSessionId>`, the agent CLI discovers its existing transcript and continues the conversation graph seamlessly.

### Explicit restart intent
Restoration and background reconnect never launch a replacement agent. The existing
retry command authorizes a recovery attempt only after an ownership-expiry failure.
The helper must then prove a changed kernel boot identity. A same-boot helper
restart, transport interruption, authentication failure, or naturally missing target
does not authorize recovery.

Explicit stops and observed successful child exits disable the receipt. An orderly
shutdown and an unobserved exit immediately before shutdown cannot always be
distinguished. The UI therefore requires the user to choose recovery rather than
silently resurrecting a conversation.

---

## 5. Pre-Feature Sessions & Unavailable Record Modeling

### Pre-Feature Receipt Absence Handling
1. **Undisclosed Receipt Reality:**
   - The desktop UI cannot know whether the remote helper has an on-disk `RecoveryRecord` prior to invoking the remote recovery RPC.
   - An expired SSH agent pane can offer recovery only with its existing `backendSessionId`; an agent reference alone cannot reconstruct the daemon binding.
   - When clicked, the helper inspects its local recovery store. A missing receipt is explicitly refused; it cannot be synthesized from UI metadata.
   - The UI displays this explicit refusal in the error alert without claiming impossible magic restoration.
2. **Missing Backend Modeling (`unavailable_record_safety`):**
   - Without a `backendSessionId`, recovery fails closed even if provider metadata exists.
   - In this state, the pane presents *"The remote process has exited or is no longer available on the host"*, offers only "Open new shell", and makes zero false recovery attempts.

---

## 6. Remote Helper Deployment Requirement

Reboot recovery requires the version-qualified Ferryx Remote Helper (`ferryx-remote-helper`) installed on the target machine:
1. **Helper Responsibilities:**
   - Host lock management (`host.lock`) via `flock` to guarantee single-helper ownership per user/host.
   - Atomic recording of `RecoveryRecord`, `PreLaunchMarker`, and `CompletionReceipt`.
   - Kernel boot ID resolution.
   - Capability `ptyRecoveryV1` and RPC `pty.recover` with `{logicalSessionId, previousTarget}`.
   - Durable user-private storage outside the reboot-volatile runtime directory, shared across qualified helper versions. Host and logical IDs use SHA-256 filenames, not lossy sanitization.
2. **Helper Outage or Version Mismatch:**
   - If the remote helper executable is missing or outdated, the desktop daemon surfaces a structured error (`HELPER_NOT_INSTALLED` or `HELPER_VERSION_MISMATCH`).
   - The UI displays the error description without entering an infinite retry loop.

---

## 7. Two-Phase Markers & Crash Isolation

To prevent recovery loops, race conditions, or partial writes during sudden power loss:

```
Step 1: Acquire host lock (`host.lock`)
Step 2: Check for existing `PreLaunchMarker`
        - If present from a previous crashed attempt: quarantine attempt to avoid loop.
Step 3: Publish logical-session and provider pre-launch markers before spawning.
Step 4: Launch recovered agent process (`omo --session <id>`) with assigned PTY.
Step 5: Publish the updated record and completion receipt with new PID and target.
Step 6: Remove both pre-launch markers.
```

Writes use private temporary files with file synchronization, atomic rename and
directory synchronization on Unix, or `MoveFileExW` with replacement/write-through
on Windows. Publication and marker-removal failures are propagated.
Lost responses reuse a completion receipt only for the same live execution;
ambiguous markers refuse another launch. A second logical controller for the same
provider conversation is refused rather than sharing its input stream.

The local daemon checkpoints the new descriptor before adopting it or reporting
Connected. The daemon backend ID remains unchanged; remote read/agent cursors
reset, a replay boundary is emitted, and old-generation input is fenced out.

---

## 8. UI Mock Boundary vs. Backend Helper Boundary

To maintain test rigor without testing theater, the validation surface is cleanly split across two distinct verification domains:

| Subsystem / Layer | Verification Mechanism | Mock Boundary / Scope |
|---|---|---|
| **Frontend UI (`TerminalPane` and reconnect helper)** | Isolated headless browser harness (`scripts/qa/ssh-reboot-recovery*`) | Real `TerminalPane` and production `reconnectSshSession`, with injected `toIpcError` and deferred retry promises. Native canvas, DAG badge and Tauri IPC are boundary mocks; unexpected process-mutating calls throw. Checks recovery, pending, failure and unavailable states at 548x340 and 1024x640. The complete desktop application is not mounted. |
| **Backend Helper (`remote-helper`, `boot_identity.rs`, `recovery.rs`)** | Rust tests and isolated helper entrypoint scenarios | Actual filesystem locks, writes, PTYs and child processes; injected boot identities model reboot loss without rebooting a production machine. Platform boot identity and artifact capability checks are separate evidence. |
