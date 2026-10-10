# Local Pane Liveness & Reliability Contract Specification (Task 2)

**Status:** Published & Frozen  
**Worktree:** `C=/Volumes/T9-Mac/project/ferryx-wt/local-pane-liveness-completion-foundation`  
**Reference Design:** `S=/Volumes/T9-Mac/project/ferryx/.omo/evidence/local-split-reliability/design.md` & `D-to-I-handoff.md`  

---

## 1. Concrete File Ownership

| Lane | Worker ID | Owned File Paths |
|------|-----------|------------------|
| **Contracts** (This Task) | Task 2 | `src-tauri/src/daemon/protocol.rs`<br>`src-tauri/src/ipc/error.rs`<br>`ui/src/lib/types.ts`<br>`ui/src/lib/localSplitContract.ts`<br>`.omo/evidence/local-pane-liveness-completion-replan/task-2/contracts.md` |
| **Backend Runtime** | `st_01a10259` | `src-tauri/src/daemon/**` (except `protocol.rs`)<br>`src-tauri/src/terminal/**`<br>`src-tauri/src/ipc/terminal.rs`<br>`src-tauri/src/ipc/debug.rs`<br>`src-tauri/src/ipc/qa_barrier.rs` (runtime coordination)<br>Backend tests and main registration |
| **Native Terminal** | `st_01a10258` | `src-tauri/src/ipc/native_terminal.rs`<br>`src-tauri/src/ipc/native_terminal_disabled.rs`<br>`src-tauri/src/native_terminal/**` |
| **Frontend Runtime** | `st_01a1025c` | `ui/src/state/**`<br>`ui/src/components/**`<br>`ui/src/lib/tauri.ts` (callsites)<br>`ui/src/lib/nativeTerminalLifecycle.ts`<br>`ui/src/lib/terminalEvents.ts`<br>UI test suites |
| **Scripts & Release** | `st_01a10254` | `scripts/**` |

---

## 2. OLD -> NEW Field Mappings

### 2.1 `DaemonSessionDetails` (`src-tauri/src/daemon/protocol.rs`)
- **Existing fields reused unchanged:** `session_id`, `pid`, `pgid`, `cols`, `rows`, `state`, `worktree`, `cwd`, `reader_paused`, `kernel_stopped`, `registry_suspended`, `suspension_source`.
- **NEW field:**
  - `pub incarnation: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`):
    - Stable lifetime PTY incarnation string generated once per PTY creation (`uuid::Uuid::new_v4()`).
    - Survives daemon handovers unchanged.
    - If `None`, identifies recoverable-unconfirmed legacy session.
- **NEW helpers:**
  - `is_recoverable_legacy(&self) -> bool`
  - `matches_incarnation(&self, expected: &str) -> bool`

### 2.2 `PtySessionSnapshot` & `PtySessionExport` (`src-tauri/src/terminal/session.rs`)
- **Existing fields reused unchanged:** `session_id`, `pid`, `pgid`, `cols`, `rows`, `worktree_path`, `state`, `hub_snapshot`, `master_raw_fd`, `master_fd`.
- **NEW field:**
  - `pub incarnation: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`):
    - Preserved across `snapshot()`, `from_parts()`, and handover `adopt_from_transfer()`.

### 2.3 `SpawnTerminalRequest` (`src-tauri/src/ipc/terminal.rs`, `ui/src/lib/tauri.ts`)
- **Existing fields reused unchanged:** `workspace_id`, `worktree`, `cwd`, `cols`, `rows`, `client_request_id`, `shell`, `startup`, `inherit_from_session_id`.
- **NEW fields:**
  - `pub create_only: Option<bool>` (`#[serde(default)]`): when true, spawn creates PTY on daemon and returns identity without attaching native surface.
  - `pub prepared_local_split: Option<PreparedLocalSplit>` (`#[serde(default)]`): pre-validated split identity and environment.
  - `pub remaining_ms: Option<u64>` (`#[serde(default)]`): clipped stage budget for create stage.

### 2.4 `AttachTerminalResponse` (`src-tauri/src/ipc/terminal.rs`, `ui/src/lib/types.ts`)
- **Existing fields reused unchanged:** `session_id`, `daemon_epoch`, `history_start_sequence`, `history_end_sequence`, `history`, `gap`.
- **NEW fields:**
  - `pub incarnation: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`): stable PTY incarnation.
  - `pub attach_tuple: Option<PaneAttachTuple>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`): authoritative 7-tuple.

### 2.5 `DaemonRequest` (`src-tauri/src/daemon/protocol.rs`)
- **`DaemonRequest::Spawn`:**
  - **NEW field:** `pub local_split: Option<LocalSplitEnvelope>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
- **NEW request variants:**
  - `SpawnOperationStatus { client_request_id: String, origin_epoch: u64, expires_at_unix_ms: u64 }`
  - `CancelSpawnOperation { client_request_id: String, origin_epoch: u64, expires_at_unix_ms: u64 }`

### 2.6 `DaemonResponse` (`src-tauri/src/daemon/protocol.rs`)
- **`DaemonResponse::HandshakeOk`:**
  - **NEW fields:**
    - `pub capabilities: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`)
    - `pub admission_time_unix_ms: Option<u64>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`)
- **NEW response variant:**
  - `SpawnOperationOk { operation: SplitOperationResult }`

### 2.7 `IpcErrorCode` (`src-tauri/src/ipc/error.rs`, `ui/src/lib/types.ts`)
- **NEW error codes:**
  - `UNSUPPORTED_CAPABILITY`: Daemon lacks `localSplitLifecycleV1`.
  - `SPAWN_REQUEST_CONFLICT`: Duplicate request ID with conflicting parameters.
  - `SPAWN_REQUEST_EXPIRED`: Request deadline expired.
  - `SPAWN_EPOCH_CHANGED`: Origin daemon epoch does not match active epoch.
  - `SPAWN_ATTEMPT_TIMEOUT`: Monotonic attempt budget exceeded.
  - `SPAWN_CANCELLED`: Operation was cancelled before or during creation.

### 2.8 `ArmSpec` (`src-tauri/src/ipc/qa_barrier.rs`)
- **Existing fields reused:** `name`, `run_id`, `operation_id`, `deadline_ms`.
- **NEW fields:**
  - `target_role: Option<String>`: restricted strictly to `"predecessor"` or `"successor"` (never a session ID or wildcard).
  - `target_backend_session_id: Option<String>`: exact bound target session ID.
  - `client_request_id: Option<String>`
  - `source_backend_session_id: Option<String>`
  - `workspace_id: Option<String>`
  - `worktree_path: Option<String>`

---

## 3. Authoritative Contract Types

### 3.1 The Authoritative 7-Field Attach Tuple (`PaneAttachTuple`)
Connecting visual UI state to PTY backend:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneAttachTuple {
    pub backend_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incarnation: Option<String>,
    pub daemon_epoch: String,
    pub frontend_session_id: String,
    pub pane_identity: String,
    pub binding_key: String,
    pub attempt_generation: u64,
}
```
Matching TypeScript interface in `ui/src/lib/types.ts`:
```ts
export interface PaneAttachTuple {
  readonly backendSessionId: string;
  readonly incarnation?: string | null;
  readonly daemonEpoch: string;
  readonly frontendSessionId: string;
  readonly paneIdentity: string;
  readonly bindingKey: string;
  readonly attemptGeneration: number;
}
```

### 3.2 Presentation Receipt (`PanePresentationReceipt`)
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanePresentationReceipt {
    pub attach_tuple: PaneAttachTuple,
    pub presented: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_time_unix_ms: Option<u64>,
}
```

### 3.3 Reliable Split Operation Lifecycle DTOs
```rust
pub const LOCAL_SPLIT_LIFECYCLE_CAPABILITY: &str = "localSplitLifecycleV1";
pub const LOCAL_SPLIT_VALIDITY_MS: u64 = 600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitIdentity {
    pub request_id: String,
    pub origin_epoch: String,
    pub expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedLocalSplit {
    pub identity: SplitIdentity,
    pub workspace_id: String,
    pub worktree: Option<WorktreeIdentity>,
    pub cwd: String,
    pub shell: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSplitEnvelope {
    pub origin_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub remaining_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SplitOperationResult<Epoch = u64> {
    Absent { can_create: bool },
    Pending { cancel_requested: bool },
    Created {
        session_id: String,
        daemon_epoch: Epoch,
        session: DaemonSessionDetails,
        ownership: SplitOwnership,
    },
    Cancelled,
    Exited,
    Failed { error: crate::ipc::IpcError, no_child: SplitNoChild },
    Unknown { reason: SplitUnknownReason },
}
```

### 3.4 Monotonic 15s Attempt Budget and Stage Caps
```rust
pub const ATTEMPT_TOTAL_BUDGET_MS: u64 = 15_000;
pub const STAGE_CREATE_OR_STATUS_MAX_MS: u64 = 9_000;
pub const STAGE_ATTACH_OR_LISTENER_MAX_MS: u64 = 4_000;
pub const STAGE_PRESENTATION_MAX_MS: u64 = 2_000;
pub const STAGE_CWD_PROBE_MAX_MS: u64 = 500;
pub const CANCEL_ACK_MAX_MS: u64 = 3_000;
pub const DAEMON_CANCEL_CLEANUP_MAX_MS: u64 = 2_500;
pub const WARM_NATIVE_READY_TARGET_MS: u64 = 2_000;

#[inline]
pub fn clip_stage_budget(remaining_total_ms: u64, stage_cap_ms: u64) -> u64 {
    remaining_total_ms.min(stage_cap_ms)
}
```

### 3.5 Checked QA Live-Arm Binding Contract (`QaBarrierChannel`)
```rust
pub fn bind_target_session(
    &self,
    name: &str,
    operation_id: &str,
    session_id: &str,
) -> Result<QaBoundAck, String>;
```
- Invariant 1: `targetRole` must be `"predecessor"` or `"successor"` (or omitted). Session ID or wildcard (`"*"`) in `targetRole` is rejected.
- Invariant 2: Identical duplicate bind (same `operation_id` and same `session_id`) succeeds idempotently with existing ACK.
- Invariant 3: Conflicting bind (different `session_id` or different `operation_id`) is strictly rejected with an error.
- Invariant 4: Written to `<name>.bound-ack.json` atomically AFTER internal state is updated.
- Invariant 5: When feature `local-split-qa` is disabled, the module is completely absent from the binary.

---

## 4. Consumer Implementation Directives

### 4.1 For Backend Consumer (`st_01a10259`)
1. In `src-tauri/src/daemon/server.rs` and `session_service.rs`:
   - Store and track `SplitOperationResult` by `(client_request_id, origin_epoch)` in an in-memory map.
   - Handle `DaemonRequest::SpawnOperationStatus` and `DaemonRequest::CancelSpawnOperation`.
   - On `Spawn` with `local_split: Some(env)`, enforce `origin_epoch == self.epoch` (otherwise fail with `SPAWN_EPOCH_CHANGED`).
2. Handshake:
   - Already emits `capabilities: vec![LOCAL_SPLIT_LIFECYCLE_CAPABILITY.to_string()]`.

### 4.2 For Native Terminal Consumer (`st_01a10258`)
1. In `src-tauri/src/ipc/native_terminal.rs` and `src-tauri/src/native_terminal/**`:
   - Consume `PaneAttachTuple` upon attach.
   - Return/emit `PanePresentationReceipt { attach_tuple, presented: true, presentation_time_unix_ms }` strictly after surface is ready/visible.
   - If incoming attach tuple has mismatched `backend_session_id`, `incarnation`, `daemon_epoch`, `frontend_session_id`, `pane_identity`, `binding_key`, or `attempt_generation`, reject presentation without detaching or destroying the session.

### 4.3 For Frontend Consumer (`st_01a1025c`)
1. In `ui/src/lib/localSplitContract.ts` and `ui/src/state/**`:
   - Import types and helpers from `ui/src/lib/localSplitContract.ts`.
   - Before attempting reliable split, verify `hasLocalSplitCapability(handshakeCapabilities)`. If false, abort or surface `UNSUPPORTED_CAPABILITY`.
   - Use `createAttemptBudget(startTimeMs, ATTEMPT_TOTAL_BUDGET_MS)` to track remaining time across create (9s), attach (4s), and presentation (2s).
   - On split creation with `createOnly: true`, save `(backendSessionId, incarnation, daemonEpoch)` into workspace store BEFORE attaching native surface.
   - Validate `PaneAttachTuple` on attach responses and presentation receipts via `matchesAttachTuple`.

### 4.4 For Scripts Consumer (`st_01a10254`)
1. Reference `LOCAL_SPLIT_LIFECYCLE_CAPABILITY` in integration and release validation scripts.
