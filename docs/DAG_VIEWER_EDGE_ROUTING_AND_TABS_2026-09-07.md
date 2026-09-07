# Ferryx DAG Viewer Edge Routing, Multi-Run Tabs & Wave Cleanup

**Date:** 2026-09-07
**Scope:** UI DAG Graph View (`DagGraphView.tsx`, `DagEdgeLayer.tsx`, `dagViewUtils.ts`), Modal Container (`DagPaneBadge.tsx`), and test suites

## 1. Background & Problem Statement

Users noticed a visual discrepancy between the active task list in terminal widgets and the DAG graph rendered in Ferryx:
1. **Hidden Multi-Hop Edges:** Tasks with dependencies skipping intermediate stages (e.g. `verify-cutover` depending on `u52`, `u53`, and `u54`) had their connection paths drawn straight through intermediate cards on the same horizontal line (`y = 72`), completely hidden behind intermediate node cards. As a result, fan-in/convergence graphs appeared as simple 1:1 serial pipelines.
2. **Missing Multi-Run Visibility:** When multiple DAG workflows were running simultaneously in the same project/session (e.g. `Durable cron ownership...` and `Remote cancellation deadline...`), the DAG modal only rendered a single run with no mechanism to view or switch to sibling runs.
3. **Misleading Wave Partitioning Labels:** Column headers (`WAVE 1`, `WAVE 2`) and modal summary texts (`wave 3/4`) caused confusion by implying pagination or fragmenting unified workflow graphs.

## 2. Changes & Implementation Details

### 2.1 Curved Arc Edge Routing & Port Distribution
- **Files:** `ui/src/components/dag/dagViewUtils.ts`, `ui/src/components/dag/DagEdgeLayer.tsx`
- **Path Calculation:** Added `calculateEdgePath` in `dagViewUtils.ts`.
  - Single-hop edges (adjacent columns, `colSpan <= 1`) retain a direct, smooth cubic bezier S-curve.
  - Multi-hop edges (`colSpan > 1`) route an upward arc (`arcOffset = Math.min(52, 26 + (colSpan - 1) * 10)`) above intermediate cards (`cy < 44px`), avoiding occlusion by intervening cards while capping maximum height so high span curves stay safely within the visible canvas boundary.
  - Distinct connection ports: distributed source and target anchor Y positions (`sourcePortOffset`, `targetPortOffset`) across cards when multiple edges attach to the same node, ensuring every arrow marker is cleanly visible.

### 2.2 Modal Multi-Run Tabs
- **Files:** `ui/src/components/dag/DagPaneBadge.tsx`
- **Run Selection:** Filtered and prioritized active runs into `paneRuns`.
- **Tab Bar:** Added an accessible tab list (`role="tablist"`) in the modal header when `paneRuns.length > 1`.
- **Isolation:** Kept single-run rendering clean and isolated; switching tabs selectively mounts only the active run's graph container.

### 2.3 Wave Label & Header Simplification
- **Files:** `ui/src/components/dag/DagGraphView.tsx`
- **Header:** Removed `wave X/Y` text from the header, presenting a clear completion summary (`X/Y done, Z running`).
- **Columns:** Hidden visual `wave N` column labels while preserving accessible test IDs for tests and screen readers (`sr-only`).

## 3. Verification & Test Evidence

### Frontend Vitest Suites
All 8 DAG-related test suites passed cleanly (78 tests total):
- `src/components/dag/DagGraphView.test.tsx` (14 passed)
- `src/components/dag/DagPaneBadge.test.tsx` (25 passed, including new multi-run tab switching test)
- `src/components/dag/DagNodeCard.test.tsx` (5 passed)
- `src/components/dag/dagViewUtils.test.ts` (12 passed, including new edge routing and port distribution tests)
- `src/lib/dagTypes.test.ts` (8 passed)
- `src/lib/dagWatchRoots.test.ts` (3 passed)
- `src/state/dagStore.test.ts` (6 passed)
- `src/state/dagRunOwnership.test.ts` (5 passed)

### Typecheck
- `bun x tsc --noEmit -p ui/tsconfig.json`: Exit code 0, zero type errors.
