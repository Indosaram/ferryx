# Final Forward Landing Audit Receipt: Remote Parity

**Generated**: 2026-10-01T00:48:25.534Z  
**Source of Truth**: `.omo/evidence/remote-parity-20260930/windows-approved-recovery/remote/`  
**Destination Root**: `ui/src/remote/`  
**Overall Status**: SUCCESS - ALL 13 FILES MATCH APPROVED RECOVERY SOURCE

---

## 1. Landed Files and Integrity Verification

| # | File | Path | Size (Bytes) | SHA-256 Hash | Integrity |
|---|------|------|--------------|--------------|-----------|
| 1 | `remoteInventoryRows.tsx` | `ui/src/remote/remoteInventoryRows.tsx` | 5780 | `ee788f4968a11a22aa912b4444bbde5026d4f2e913b5116524022797c87df9f9` | VERIFIED (100% match) |
| 2 | `remoteInventoryView.ts` | `ui/src/remote/remoteInventoryView.ts` | 18993 | `a119dafc7dd3d8d68849a5f5fc2f1c421c14f7cb39834f577f3437538b8c4a56` | VERIFIED (100% match) |
| 3 | `remoteInventoryView.test.ts` | `ui/src/remote/remoteInventoryView.test.ts` | 19479 | `29b9720a21227b3065d018b6fc3c55cd5f734c19e949160b0a55662d90c4c5ea` | VERIFIED (100% match) |
| 4 | `RemoteSessionList.inventory.test.tsx` | `ui/src/remote/RemoteSessionList.inventory.test.tsx` | 13369 | `17dd529235f68ee09ffd67ccbae0999602c53ef2ffa1b3a1b53425547a0e974a` | VERIFIED (100% match) |
| 5 | `RemoteApp.inventorySelection.test.tsx` | `ui/src/remote/RemoteApp.inventorySelection.test.tsx` | 19587 | `ac1b436f69af9720ee3156387664de13a8b8d527d282fd39a1b723795e1b1a81` | VERIFIED (100% match) |
| 6 | `RemoteTerminal.agentState.test.tsx` | `ui/src/remote/RemoteTerminal.agentState.test.tsx` | 4990 | `259cfcda18405336dd0aa88985fbe513859c1e5e03ec4e869c0a7f9ea1a6e625` | VERIFIED (100% match) |
| 7 | `RemoteTerminal.tsx` | `ui/src/remote/RemoteTerminal.tsx` | 56326 | `b0740aeb66200db42badf6035c1d0d010b6ea7758751e7b706327e207e143d89` | VERIFIED (100% match) |
| 8 | `RemoteSessionList.tsx` | `ui/src/remote/RemoteSessionList.tsx` | 34933 | `4ac73887b6e02e0cb4a24afb459b081188f7758db42598ad5aaeca0b89fab392` | VERIFIED (100% match) |
| 9 | `RemoteUI.test.tsx` | `ui/src/remote/RemoteUI.test.tsx` | 92753 | `76a3d12a5607adf8d643081c39e3147219f61f95a70152b170d90b5ff44f9932` | VERIFIED (100% match) |
| 10 | `remoteSessionInventory.ts` | `ui/src/remote/remoteSessionInventory.ts` | 24309 | `d34a605efa0f42a98ad15e1c8982056fcb00c592e3441d81cf5c13f6eaed0a56` | VERIFIED (100% match) |
| 11 | `remoteSessionInventory.test.ts` | `ui/src/remote/remoteSessionInventory.test.ts` | 27004 | `5ca6fa39997a8306fe380a4d768e127d095938633738396e00aebe56e1ce0897` | VERIFIED (100% match) |
| 12 | `useAccountWorktrees.ts` | `ui/src/remote/useAccountWorktrees.ts` | 22500 | `5d2c322224c168fbcb28d9a4d3eae9b307c96ad6bad82817d89556b6ecdd7c67` | VERIFIED (100% match) |
| 13 | `RemoteApp.tsx` | `ui/src/remote/RemoteApp.tsx` | 109032 | `895b178eeef5ef93ecce94eaf17dbd6dc8354a3fcbfd305ffd701887b34b50c4` | VERIFIED (100% match) |

---

## 2. Classification & Provenance Details

### A. Six Session-Owned New Files (Full Content Replaced)
1. `remoteInventoryRows.tsx`: ee788f4968a11a22aa912b4444bbde5026d4f2e913b5116524022797c87df9f9
2. `remoteInventoryView.ts`: a119dafc7dd3d8d68849a5f5fc2f1c421c14f7cb39834f577f3437538b8c4a56
3. `remoteInventoryView.test.ts`: 29b9720a21227b3065d018b6fc3c55cd5f734c19e949160b0a55662d90c4c5ea (Canonical full test suite restored, replacing prior 5KB stub)
4. `RemoteSessionList.inventory.test.tsx`: 17dd529235f68ee09ffd67ccbae0999602c53ef2ffa1b3a1b53425547a0e974a (Canonical full test suite restored, replacing prior 5KB stub)
5. `RemoteApp.inventorySelection.test.tsx`: ac1b436f69af9720ee3156387664de13a8b8d527d282fd39a1b723795e1b1a81 (Canonical full test suite restored, replacing prior 1KB stub)
6. `RemoteTerminal.agentState.test.tsx`: 259cfcda18405336dd0aa88985fbe513859c1e5e03ec4e869c0a7f9ea1a6e625

### B. Three Existing Non-Collision Files (Approved Recovery Parity)
1. `RemoteTerminal.tsx`: b0740aeb66200db42badf6035c1d0d010b6ea7758751e7b706327e207e143d89 (`onAgentStateFrame` socket handling and lifecycle dependency)
2. `RemoteSessionList.tsx`: 4ac73887b6e02e0cb4a24afb459b081188f7758db42598ad5aaeca0b89fab392 (Documentation annotations, pickerStatus, activeMachineId, onSignOut)
3. `RemoteUI.test.tsx`: 76a3d12a5607adf8d643081c39e3147219f61f95a70152b170d90b5ff44f9932 (Per-machine/workspace groupKeys validation with region accessible assertions)

### C. Four Collision & Inventory Files (Approved Recovery Parity)
1. `remoteSessionInventory.ts`: d34a605efa0f42a98ad15e1c8982056fcb00c592e3441d81cf5c13f6eaed0a56
2. `remoteSessionInventory.test.ts`: 5ca6fa39997a8306fe380a4d768e127d095938633738396e00aebe56e1ce0897
3. `useAccountWorktrees.ts`: 5d2c322224c168fbcb28d9a4d3eae9b307c96ad6bad82817d89556b6ecdd7c67 (Preserves `initialized`, `errorCode`, clean `activeGenerationRef`, project basenames and session inventories)
4. `RemoteApp.tsx`: 895b178eeef5ef93ecce94eaf17dbd6dc8354a3fcbfd305ffd701887b34b50c4 (Full callback memoization, `machineTabsFromInventory`, inventory selection wireup, sign-out and saved worktree reload restore)

---

## 3. Policy & Constraint Adherence
- **Zero Candidate Mutations**: `.omo/evidence/.../windows-approved-recovery/` and original `.omo/evidence/.../candidate/` were strictly read-only and unaltered.
- **Zero Local Builds/Tests**: No Mac cargo/bun builds or tests executed.
- **Zero Git Commits**: Tree left intact with working tree modifications only.
- **Destination Confirmed**: Every write strictly targeted `/Volumes/T9-Mac/project/ferryx/ui/src/remote/` without exception.

---

## 4. Foreign Work Preservation & Collision Audit

### A. Pre-Edit Snapshots and Baseline Comparison
Before this landing operation, the repository state was audited against the baseline snapshots recorded in `.omo/evidence/remote-parity-20260930/audit/shared-write-audit.json` and `.omo/evidence/remote-parity-20260930/baseline/`:

| File | Baseline Hash | Pre-Edit Working Tree Hash | Approved Recovery Hash (Landed) | Status / Foreign Delta Analysis |
|------|---------------|----------------------------|---------------------------------|---------------------------------|
| `useAccountWorktrees.ts` | `198286bcddad510f9d0df8a05b0701b9cece8807de6dca393dbb351fa8d8a0dd` (17,372 B) | `782783250739766eec0fb9abb0f17a360fd0bdff0fc46a5a14b18501db60342e` (20,690 B) | `5d2c322224c168fbcb28d9a4d3eae9b307c96ad6bad82817d89556b6ecdd7c67` (22,500 B) | **Verified Additive Forward Landing**. All foreign changes preserved. |
| `RemoteApp.tsx` | `fffc1b4e4dc7a504d4d07e25accc0624f34cdcb342447cc5ef3b2d1d392af01c` (98,520 B) | `1d7ff5030ab9ff915766db459ef9817c4ab5ca4cd79ef0b4140dd1427de24ae6` (104,358 B) | `895b178eeef5ef93ecce94eaf17dbd6dc8354a3fcbfd305ffd701887b34b50c4` (109,032 B) | **Verified Forward Parity Landing**. Only prior incomplete/stale local hunks replaced. |
| `remoteSessionInventory.ts` | *None (new module)* | `62bec4dc61637a74259d45c1333ccad961be35e8b6d69488f5e8ffc7aeae80e4` (21,147 B) | `d34a605efa0f42a98ad15e1c8982056fcb00c592e3441d81cf5c13f6eaed0a56` (24,309 B) | **Session-owned file**. Upgraded from partial local draft to canonical verified candidate. |
| `remoteSessionInventory.test.ts` | *None (new module)* | `e08e83cb842c9a37d68296d8ec2064d639d734705eb2312e762f51d252ec1263` (20,196 B) | `5ca6fa39997a8306fe380a4d768e127d095938633738396e00aebe56e1ce0897` (27,004 B) | **Session-owned file**. Upgraded from partial local draft to canonical verified candidate. |

---

### B. Detailed Hunk and Removal Audit

#### 1. `useAccountWorktrees.ts`
- **Pre-edit state vs Baseline**:
  The pre-edit working tree file had added initial inventory hooks (`applyInventoryEvent`, `noteSessionsPayload`, etc.) but was in an incomplete transitional state: `probeMachine` still executed a local `for (const s of sessions)` loop mutating `machineOpts` inline, and returned flat snapshot options instead of rebuilding live session options from `sessionInventories[machine.machineId]`.
- **Foreign work preserved from Commit 9d70fee8**:
  - `initialized` boolean state and return field on `UseAccountWorktreesResult`.
  - `errorCode` extraction from `AccountSessionError` or generic error objects.
  - `activeGenerationRef.current += 1` on unmount cleanup.
- **Lines Removed / Replaced**:
  The only lines removed were the intermediate 24-line `for (const s of sessions)` loop in `probeMachine` (which populated snapshot `machineOpts`) in favor of storing the payload into `noteSessionsPayload` and computing live `accountOptions` from the reactive inventory store (`sessionInventories`). No peer or foreign functionality was dropped.

#### 2. `RemoteApp.tsx`
- **Pre-edit state (`1d7ff5030...`) vs Baseline (`fffc1b4e...`)**:
  Pre-edit working tree had landed an earlier partial draft of remote session inventory imports and `machineSessionTabs`. However, it lacked:
  - `machineTabsFromInventory` using `RemoteSessionInventoryEntry` with epoch tracking.
  - Re-usable `terminalCreateWebSocket` callback memoization via `useMemo` (the previous draft instantiated identical closures inline across JSX).
  - `onAgentStateFrame` socket handler (`handleMachineAgentStateFrame`) and `ws.onmessage` hook integration.
  - Ephemeral session preservation across account option acquisition (`target.sessionId` / `target.daemonEpoch` in `setInitialAccountTarget`).
  - Coalesced `scheduleInventoryRefresh` via timer ref.
- **Foreign work preserved from Commit 9d70fee8**:
  - `accountSelectionGenerationRef` race guard and cancellation.
  - `accountSessionTokenRef` check in `selectAccountOption`.
  - `handleSignOut` invoking `logoutAccountSession`.
  - Automatic worktree reload restore `useEffect` with target checking.
  - `getAccountLastSelectedTarget` and `setAccountLastSelectedTarget` persistence.
- **Lines Removed / Replaced**:
  - Replaced the preliminary `machineSessionTabs` helper with `machineTabsFromInventory` (which accepts canonical `RemoteSessionInventoryEntry` and preserves epoch).
  - Consolidated duplicate inline `createWebSocket` definitions in `<RemoteTerminal />` into the memoized `terminalCreateWebSocket`.
  - No foreign features or peer bugfixes were removed.

#### 3. `remoteSessionInventory.ts` and `remoteSessionInventory.test.ts`
- Solely owned by the remote inventory feature.
- Upgraded from early partial drafts to the full Windows-approved implementation including identity invariants (`epochMatches`, strict running booleans, target machine id validation, and non-authoritative downgrade handling).
