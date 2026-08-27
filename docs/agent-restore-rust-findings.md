# Ferryx PTY Root PID Exposure Findings

## 1. `src-tauri/src/terminal/pty.rs` — Child PID Capture & Storage
When a PTY is spawned in `PtyManager::spawn_with_id_and_worktree`:
- At lines 154–157, `pair.slave.spawn_command(cmd)` spawns the child process and returns a handle `child: Box<dyn Child + Send + Sync>`:
  ```rust
  let child = pair
      .slave
      .spawn_command(cmd)
      .map_err(|e| PtyError::SpawnError(format!("Failed to spawn command: {e}")))?;
  ```
- At line 165, `child` is passed into `PtySessionConfig` and provided to `PtySession::new`:
  ```rust
  let session = Arc::new(PtySession::new(PtySessionConfig {
      id: session_id.clone(),
      master: pair.master,
      child,
      writer,
      reader,
      cols,
      rows,
      tx,
      worktree_path,
  }));
  ```
- **Finding:** The numeric PID is not captured into a standalone variable or field in `pty.rs` at spawn time, but the child process handle `child` (which implements `portable_pty::Child`) is captured and stored inside the `PtySession`.

---

## 2. `src-tauri/src/terminal/session.rs` — Session Struct PID Holding
- In `PtySessionConfig` (`src-tauri/src/terminal/session.rs:32`):
  ```rust
  pub child: Box<dyn Child + Send + Sync>,
  ```
- In `PtySession` (`src-tauri/src/terminal/session.rs:50`):
  ```rust
  child: Arc<Mutex<Option<Box<dyn Child + Send + Sync>>>>,
  ```
- At `src-tauri/src/terminal/session.rs:147-152`, `PtySession` exposes a public method to retrieve the root PID dynamically:
  ```rust
  pub fn pid(&self) -> Option<u32> {
      self.child
          .lock()
          .as_ref()
          .and_then(|child| child.process_id())
  }
  ```
- **Finding:** `PtySession` does not store a standalone `pid: Option<u32>` field, but it retains the `child` handle and already implements `pub fn pid(&self) -> Option<u32>` to query `child.process_id()`.

---

## 3. `src-tauri/src/daemon/protocol.rs` — Daemon Protocol Messages & PID Fields
`DAEMON_PROTOCOL_VERSION` is `2` (`src-tauri/src/daemon/protocol.rs:10`):
- `DaemonSessionDetails` (`src-tauri/src/daemon/protocol.rs:38-49`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct DaemonSessionDetails {
      pub session_id: String,
      pub workspace_id: Option<String>,
      pub worktree: Option<WorktreeIdentity>,
      pub cwd: Option<String>,
      pub cols: u16,
      pub rows: u16,
      pub running: bool,
      pub start_sequence: Option<u64>,
      pub end_sequence: Option<u64>,
  }
  ```
  *(Does not contain a `pid` field)*
- `DaemonResponse::SpawnOk` (`src-tauri/src/daemon/protocol.rs:159-162`):
  ```rust
      #[serde(rename_all = "camelCase")]
      SpawnOk {
          session_id: String,
      },
  ```
  *(Does not contain a `pid` field)*
- `DaemonResponse::ListSessionsOk` (`src-tauri/src/daemon/protocol.rs:167-170`):
  ```rust
      #[serde(rename_all = "camelCase")]
      ListSessionsOk {
          epoch: u64,
          sessions: Vec<String>,
      },
  ```
  *(Does not contain a `pid` field)*
- Note: The only response containing a `pid` is `DaemonResponse::HandshakeOk` (`src-tauri/src/daemon/protocol.rs:146-150`):
  ```rust
      #[serde(rename_all = "camelCase")]
      HandshakeOk {
          version: u32,
          pid: u32,
          epoch: u64,
      },
  ```
  This is the daemon server process PID (`std::process::id()`), not the PTY root child PID.
- **Finding:** Neither `DaemonSessionDetails`, `SpawnOk`, nor `ListSessionsOk` carry a PTY child PID field.

---

## 4. `src-tauri/src/ipc/` — Tauri Commands Returning PID to Frontend
A full search across `src-tauri/src/ipc/` reveals that no command returns a PID to the UI layer:
- `cmd_terminal_spawn` (`src-tauri/src/ipc/terminal.rs:555-676`) returns `SpawnTerminalResponse` (`src-tauri/src/ipc/terminal.rs:320-323`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct SpawnTerminalResponse {
      pub session_id: String,
  }
  ```
- `cmd_terminal_attach` (`src-tauri/src/ipc/terminal.rs:679-700`) returns `AttachTerminalResponse` (`src-tauri/src/ipc/terminal.rs:373-384`).
- `cmd_terminal_list` (`src-tauri/src/ipc/terminal.rs:905-918`) returns `Vec<TerminalSessionSummary>` (`src-tauri/src/ipc/terminal.rs:326-330`):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct TerminalSessionSummary {
      pub session_id: String,
      pub worktree_path: Option<PathBuf>,
  }
  ```
- `cmd_terminal_get_cwd` (`src-tauri/src/ipc/terminal.rs:703-717`) returns `TerminalCwdResponse` (`src-tauri/src/ipc/terminal.rs:333-336`).
- Internal functions `process_cwd(pid: u32)` (`src-tauri/src/ipc/terminal.rs:830`) and `macos_proc::get_proc_cwd(pid: u32)` (`src-tauri/src/ipc/terminal.rs:802`) use PIDs internally for OS process table inspection, but no IPC command returns a PID.
- **Finding:** No Tauri command in `src-tauri/src/ipc/` returns a PTY child PID to the frontend.

---

## 5. `ui/src/lib/` — TypeScript Types Carrying PTY PID
- A search for the token `"pid"` across all files in `ui/src/lib/` returned no results.
- `TerminalSessionSummary` (`ui/src/lib/types.ts:47-51`):
  ```typescript
  export type TerminalSessionSummary = {
    sessionId: string;
    worktreePath: string | null;
    daemonEpoch?: string | null;
  };
  ```
- `TerminalSession` (`ui/src/lib/types.ts:55-69`):
  ```typescript
  export type TerminalSession = {
    id: string;
    cwd: string;
    worktreePath?: string;
    workspaceId: string;
    worktree: WorktreeIdentity | null;
    backendSessionId: string | null;
    lifecycle: TerminalLifecycle;
    ownerId?: string | null;
    daemonEpoch?: string | null;
    lastOutputSequence?: string | null;
  };
  ```
- `AttachTerminalResponse` (`ui/src/lib/types.ts:346-353`):
  ```typescript
  export type AttachTerminalResponse = {
    sessionId: string;
    daemonEpoch?: string | null;
    historyStartSequence?: string | null;
    historyEndSequence?: string | null;
    history: string;
    gap?: TerminalReplayGap | null;
  };
  ```
- **Finding:** No TypeScript type in `ui/src/lib/` carries a PTY PID today.

---

## 6. CONCLUSION
**Is the PID available to the UI TODAY?**
**No.**

### Minimal Additive Change to Expose It:
1. **Struct / Protocol Message:**
   - Add an optional field `pid: Option<u32>` to `DaemonSessionDetails` in `src-tauri/src/daemon/protocol.rs`:
     ```rust
     #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
     #[serde(rename_all = "camelCase")]
     pub struct DaemonSessionDetails {
         pub session_id: String,
         pub workspace_id: Option<String>,
         pub worktree: Option<WorktreeIdentity>,
         pub cwd: Option<String>,
         pub cols: u16,
         pub rows: u16,
         pub running: bool,
         pub start_sequence: Option<u64>,
         pub end_sequence: Option<u64>,
         #[serde(default, skip_serializing_if = "Option::is_none")]
         pub pid: Option<u32>,
     }
     ```
   - In `src-tauri/src/daemon/server.rs` (`describe_session_response` at line 1018), populate `pid: pty_session.pid()`.
   - Optionally add `pub pid: Option<u32>` to `SpawnTerminalResponse` and `TerminalSessionSummary` in `src-tauri/src/ipc/terminal.rs`.
   - In `ui/src/lib/types.ts`, add `pid?: number | null` to `TerminalSessionSummary` and/or `TerminalSession`.

2. **Protocol Versioning & Backward Compatibility:**
   - **Does NOT require bumping `DAEMON_PROTOCOL_VERSION`**: Adding an optional field (`Option<u32>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`) to `DaemonSessionDetails` is fully backward-compatible with existing JSON serialization in serde (which ignores unknown fields by default since `DaemonSessionDetails` does not use `deny_unknown_fields`).
