# Ferryx DAG Viewer Bug Audit, Fix & Verification Report

**Date:** 2026-09-05
**Scope:** Backend Watcher / IPC pipeline, React components, and Zustand state synchronization

## Overview
A comprehensive audit of the Ferryx DAG viewer subsystem identified 16 functional, layout, and performance defects across the Rust backend and TypeScript/React UI. All identified defects were resolved and validated using failing-first tests, Vitest test suites, and Cargo integration tests.

---

## 1. Resolved Issues & Root Cause Analysis

### 1.1 Multi-Project Ownership Retain Wipeout
- **Files:** `ui/src/state/dagRunOwnership.ts`, `ui/src/components/dag/DagPaneBadge.tsx`
- **Root Cause:** `dagRunOwnership.retain(activeRunIds)` was global. Calling `retain` from Project A with only Project A's active run IDs caused `owners.delete()` on all runs belonging to Project B, prematurely stripping live DAG ownership from panes in other projects.
- **Fix:** Extended `retain` with optional `allKnownRunIdsInScope`. In `DagPaneBadge`, retain is scoped to `runs.map(c => c.runId)`. Unrelated projects' runs are preserved.

### 1.2 Missing `sessions` Prop in `TerminalSplitView`
- **Files:** `ui/src/components/TerminalSplitView.tsx`
- **Root Cause:** `PaneLeafViewProps` omitted `sessions`, causing `TerminalPane` to receive `undefined` for `sessions`. `DagPaneBadge` was unable to determine `exactlyMatchedByAnotherPane`, leading to arbitrary agent panes claiming runs intended for sibling panes.
- **Fix:** Added `sessions: Readonly<Record<string, TerminalSession>>` to `PaneLeafViewProps` and forwarded `sessions` down to `TerminalPane`.

### 1.3 macOS `/private/` Path Normalization
- **Files:** `ui/src/components/dag/DagPaneBadge.tsx`
- **Root Cause:** Backend canonicalization resolved symlinks (e.g., `/tmp/...` -> `/private/tmp/...`), while frontend session paths remained non-canonical, breaking boundary matching.
- **Fix:** Stripped `/private/` prefix in `cleanPath` before boundary and equality checks.

### 1.4 Ownership Matching Priority Inversion
- **Files:** `ui/src/components/dag/DagPaneBadge.tsx`
- **Root Cause:** Pane-claim check (`owned`) was evaluated before exact `rootSessionId` match, allowing arbitrary claims to supersede authoritative session binding.
- **Fix:** Reordered matching logic to evaluate exact `rootSessionId === providerSessionId` prior to looking up `ownersByRunId`.

### 1.5 Active Wave Sort Inconsistency
- **Files:** `ui/src/components/dag/dagViewUtils.ts`
- **Root Cause:** `deriveActiveWaveIndex` iterated `run.waves` in raw checkpoint order, while `DagGraphView` sorted waves by `index`, causing wave column highlights and headers to desync.
- **Fix:** Sorted waves by `index` inside `deriveActiveWaveIndex` prior to finding running or scheduled waves.

### 1.6 SVG Marker ID DOM Collisions
- **Files:** `ui/src/components/dag/DagEdgeLayer.tsx`
- **Root Cause:** Hardcoded static marker IDs (`dag-arrow`, `dag-arrow-critical`) collided when multiple DAG graphs mounted simultaneously.
- **Fix:** Replaced static IDs with scoped identifiers generated via `React.useId()`.

### 1.7 Node Card Text Overflow & Style Clashes
- **Files:** `ui/src/components/dag/DagNodeCard.tsx`
- **Root Cause:** 56px card height with `p-2.5` and `mt-2` overflowed text content. `ring-1` on critical path collided with CSS `shadow` on running nodes.
- **Fix:** Added `overflow-hidden`, reduced padding to `p-2` and margin to `mt-1`. Switched critical path outline to `outline outline-1 -outline-offset-1 outline-primary/70`.

### 1.8 Unhandled `unknown` State & Route
- **Files:** `ui/src/lib/dagTypes.ts`, `ui/src/components/dag/DagNodeCard.tsx`, `ui/src/components/dag/dagViewUtils.ts`
- **Root Cause:** Rust backend emits `Unknown` on unrecognized enum variants, but frontend `deriveDagRunCounts` threw on unexpected variants and `STATE_APPEARANCE` lacked `unknown`.
- **Fix:** Added `unknown` to `DagNodeState`, `VALID_NODE_STATES`, and `DagNodeRoute`. Updated `deriveDagRunCounts` to count totals without throwing, and provided fallback appearances in `DagNodeCard` and glyph/route helpers.

### 1.9 Watcher Storm & Generic Root Scanning Prevention
- **Files:** `src-tauri/src/dag/watcher.rs`
- **Root Cause:** When `.omo/senpi-task/dag` did not yet exist, the watcher watched the root directory recursively, generating high notification churn on `target/`, `node_modules/`, `.git/`, etc.
- **Fix:** Added noise filtering in the watcher event handler for build/cache/git directories, and constrained scanning in `scan_and_emit` to verified DAG runs directories.

### 1.10 Offload Blocking Disk I/O on Async Runtime
- **Files:** `src-tauri/src/ipc/dag.rs`
- **Root Cause:** `load_current_snapshots` performed synchronous file reads on the async Tokio worker thread inside `tauri::async_runtime::spawn`.
- **Fix:** Wrapped `load_current_snapshots` invocation in `run_blocking`.

### 1.11 App Watcher Ref Retry Recovery
- **Files:** `ui/src/App.tsx`
- **Root Cause:** If `watchDagProject` rejected, the path remained in `dagWatchedPathsRef.current`, preventing subsequent watch retries.
- **Fix:** Removed the path from `dagWatchedPathsRef.current` in the `.catch` handler.

### 1.12 Deterministic Run Date Sorting & Timestamp Handling
- **Files:** `ui/src/components/dag/DagPaneBadge.tsx`, `ui/src/state/dagStore.ts`
- **Root Cause:** `runUpdatedAt` in `DagPaneBadge.tsx` returned `Number.NaN` on missing or invalid timestamps, resulting in undefined `sort` comparison order. `selectRunSummaries` in `dagStore.ts` used `localeCompare` on empty strings, misordering newly started runs without an `updatedAt` timestamp.
- **Fix:** Safely parsed timestamps in both modules falling back cleanly to 0, ensuring strictly deterministic numeric sorting.

### 1.13 Unmapped Nodes & Empty Waves Fallback
- **Files:** `ui/src/components/dag/DagGraphView.tsx`, `ui/src/components/dag/DagGraphView.test.tsx`
- **Root Cause:** In runs where `waves` array was empty or nodes existed outside partitioned wave columns, `DagGraphView` failed to position or render those nodes, hiding active tasks from the DAG view.
- **Fix:** Added fallback wave calculation in `DagGraphView.tsx` to detect unassigned nodes and group them into a visible fallback wave column, ensuring all tasks in a run remain accessible and visible.

### 1.14 Partial JSON Write Resilience in Backend Watcher
- **Files:** `src-tauri/src/dag/watcher.rs`
- **Root Cause:** When checkpoint files were written incrementally by tasks or agent processes, the file watcher could attempt to parse partial JSON during an active flush, silently dropping the update when parsing failed once.
- **Fix:** Added retry loop with bounded 50ms exponential backoff in `scan_and_emit` to allow incomplete disk flushes to settle before rejecting.

### 1.15 Direct Journal Snapshot Propagation with Session IDs
- **Files:** `src-tauri/src/dag/journal.rs`, `src-tauri/src/ipc/dag.rs`
- **Root Cause:** `JournalDagRunSnapshot` previously lacked `root_session_id` and `parent_session_id` fields, forcing the IPC watcher callback to repeatedly read the filesystem (`load_current_snapshots`) to re-hydrate session IDs.
- **Fix:** Extended `DagRunSnapshot` in `journal.rs` to parse and serialize `rootSessionId` and `parentSessionId` directly from checkpoints, allowing watcher events to emit directly to Tauri without secondary disk I/O.

### 1.16 Modal Focus Trap & Terminal Keyboard Event Isolation
- **Files:** `ui/src/components/dag/DagPaneBadge.tsx`, `ui/src/components/dag/DagPaneBadge.test.tsx`
- **Root Cause:** Opening the DAG graph modal left web/terminal focus ambiguous; keyboard shortcuts and typing could leak through to the background PTY session, risking unintended command input or process termination.
- **Fix:** Implemented an accessible focus trap in `DagPaneBadge.tsx`, auto-focusing the modal close button upon opening, trapping `Tab` / `Shift+Tab` cycling, capturing `Escape` to close cleanly, and calling `event.stopPropagation()` on all modal keydown events to isolate the terminal.

---

## 2. Verification & Test Evidence

### Frontend Vitest Suites
All 9 test suites passed cleanly (88 tests total):
- `src/state/dagRunOwnership.test.ts` (5 passed)
- `src/components/dag/dagViewUtils.test.ts` (9 passed)
- `src/lib/dagTypes.test.ts` (8 passed)
- `src/components/dag/DagNodeCard.test.tsx` (5 passed)
- `src/components/dag/DagPaneBadge.test.tsx` (23 passed)
- `src/components/dag/DagGraphView.test.tsx` (14 passed)
- `src/state/dagStore.test.ts` (6 passed)
- `src/components/TerminalPane.test.tsx` (17 passed)
- `src/components/TerminalSplitView.paneHandleReach.test.tsx` (1 passed)

### Typecheck
- `bun run --cwd ui tsc --noEmit`: Clean exit 0 with zero errors or warnings.

### Backend Cargo Suites
- `cargo test --manifest-path src-tauri/Cargo.toml --lib dag`: 11 passed, 0 failed, 1 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::dag`: 4 passed, 0 failed.
