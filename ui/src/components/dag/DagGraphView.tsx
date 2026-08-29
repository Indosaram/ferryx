import React, { useSyncExternalStore } from "react";
import type { DagNodeSnapshot, DagRunSnapshot } from "../../lib/dagTypes";
import { deriveDagRunCounts } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { DagEdgeLayer } from "./DagEdgeLayer";
import { DagNodeCard } from "./DagNodeCard";
import {
  CARD_HEIGHT,
  CARD_WIDTH,
  calculateNodePosition,
  deriveActiveWaveIndex,
  GAP_X,
  GAP_Y,
  PAD_X,
  PAD_Y,
} from "./dagViewUtils";

export type DagGraphViewProps = {
  readonly runId?: string | null;
  readonly projectPath?: string;
  readonly snapshot?: DagRunSnapshot;
};

export function DagGraphView({
  runId,
  projectPath,
  snapshot: propSnapshot,
}: DagGraphViewProps): JSX.Element {
  const storeState = useSyncExternalStore(dagStore.subscribe, () => dagStore.getState());

  const activeRun: DagRunSnapshot | null = React.useMemo(() => {
    if (propSnapshot) return propSnapshot;

    if (runId) {
      for (const projectRuns of Object.values(storeState.runsByProject)) {
        if (projectRuns[runId]) return projectRuns[runId];
      }
    }

    if (projectPath) {
      const summaries = dagStore.runSummaries(projectPath);
      if (summaries.length > 0) return summaries[0];
    }

    for (const projectRuns of Object.values(storeState.runsByProject)) {
      const runs = Object.values(projectRuns);
      if (runs.length > 0) return runs[0];
    }

    return null;
  }, [propSnapshot, runId, projectPath, storeState]);

  if (!activeRun) {
    return (
      <div
        className="flex h-full w-full items-center justify-center bg-background text-sm text-muted-foreground select-none"
        data-testid="dag-graph-view"
      >
        <span>No dag runs yet</span>
      </div>
    );
  }

  const nodes = activeRun.nodes;
  const waves = [...activeRun.waves].sort((a, b) => a.index - b.index);
  const counts = activeRun.counts || deriveDagRunCounts(nodes);
  const activeWaveIdx = deriveActiveWaveIndex(activeRun);
  const totalWaves = waves.length;
  const activeWaveNumber = totalWaves > 0 ? activeWaveIdx + 1 : 0;

  const nodeMap = new Map<string, DagNodeSnapshot>(nodes.map((n) => [n.id, n]));
  const bottleneckMap = new Map<string, number>(
    (activeRun.bottlenecks || []).map((b) => [b.nodeId, b.blockedCount]),
  );
  const criticalPathSet = new Set<string>(activeRun.criticalPath || []);

  const nodePositions = new Map<string, { x: number; y: number }>();
  let maxCol = 0;
  let maxRow = 0;

  waves.forEach((wave, colIndex) => {
    maxCol = Math.max(maxCol, colIndex);
    wave.nodeIds.forEach((nodeId, rowIndex) => {
      maxRow = Math.max(maxRow, rowIndex);
      nodePositions.set(nodeId, calculateNodePosition(colIndex, rowIndex));
    });
  });

  const contentWidth = PAD_X * 2 + (maxCol + 1) * CARD_WIDTH + maxCol * GAP_X;
  const contentHeight = PAD_Y * 2 + (maxRow + 1) * CARD_HEIGHT + maxRow * GAP_Y;

  return (
    <div
      className="relative flex h-full w-full flex-col overflow-hidden bg-background text-foreground select-none"
      data-testid="dag-graph-view"
      data-run-id={activeRun.runId}
    >
      {/* Header */}
      <div
        className="flex h-9 shrink-0 items-center justify-between border-b border-border/40 bg-card/60 px-3 text-xs text-muted-foreground backdrop-blur-sm"
        data-testid="dag-header"
      >
        <div className="flex items-center gap-2 min-w-0 font-medium">
          <span className="truncate text-foreground">{activeRun.name}</span>
          <span className="text-muted-foreground/60">&mdash;</span>
          <span className="shrink-0 text-muted-foreground font-mono">
            wave {activeWaveNumber}/{totalWaves}
          </span>
          <span className="text-muted-foreground/60">&mdash;</span>
          <span className="shrink-0 text-muted-foreground">
            {counts.completed}/{counts.total} done, {counts.running} running
          </span>
        </div>
      </div>

      {/* Graph Area */}
      <div className="relative flex-1 overflow-auto p-8 scrollbar-sleek">
        <div
          className="relative"
          style={{ width: contentWidth, height: contentHeight, minWidth: "100%", minHeight: "100%" }}
        >
          {/* SVG Bezier Edges Layer Under HTML Cards */}
          <DagEdgeLayer
            edges={activeRun.edges}
            nodePositions={nodePositions}
            criticalPath={activeRun.criticalPath || []}
            width={contentWidth}
            height={contentHeight}
          />

          {/* HTML Node Columns */}
          <div className="absolute inset-0 flex" style={{ gap: GAP_X, padding: `${PAD_Y}px ${PAD_X}px` }}>
            {waves.map((wave) => (
              <div
                key={wave.index}
                data-testid="dag-wave-column"
                data-wave-index={wave.index}
                className="flex flex-col"
                style={{ width: CARD_WIDTH, gap: GAP_Y }}
              >
                {wave.nodeIds.map((nodeId) => {
                  const node = nodeMap.get(nodeId);
                  if (!node) return null;
                  const isCritical = criticalPathSet.has(node.id);
                  const blockedCount = bottleneckMap.get(node.id) ?? 0;
                  return (
                    <DagNodeCard
                      key={node.id}
                      node={node}
                      isCriticalPath={isCritical}
                      blockedCount={blockedCount}
                      style={{ height: CARD_HEIGHT, width: CARD_WIDTH }}
                    />
                  );
                })}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
