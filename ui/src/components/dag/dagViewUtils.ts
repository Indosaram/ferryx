import type { DagNodeRoute, DagNodeSnapshot, DagNodeState, DagRunCounts, DagRunSnapshot } from "../../lib/dagTypes";

export const CARD_WIDTH = 220;
export const CARD_HEIGHT = 56;
export const GAP_X = 72;
export const GAP_Y = 14;
export const PAD_X = 32;
export const PAD_Y = 44;
export const WAVE_LABEL_HEIGHT = 18;

export function getNodeStateGlyph(state: DagNodeState): string {
  switch (state) {
    case "completed": return "✓";
    case "failed": return "✗";
    case "running": return "▶";
    case "pending": return "◌";
    case "scheduled": return "◔";
    case "blocked": return "⊟";
    case "skipped":
    case "cancelled": return "⊘";
    case "paused": return "⏸";
    case "unknown": return "?";
  }
}

export function formatRouteText(route: DagNodeRoute): string {
  if (route.kind === "unknown") {
    return "unknown";
  }
  if (route.kind === "category") {
    return `category:${route.category}`;
  }
  return `agent:${route.agent}`;
}

export function calculateNodePosition(colIndex: number, rowIndex: number): { x: number; y: number } {
  return {
    x: PAD_X + colIndex * (CARD_WIDTH + GAP_X),
    y: PAD_Y + rowIndex * (CARD_HEIGHT + GAP_Y),
  };
}

export function deriveActiveWaveIndex(run: DagRunSnapshot): number {
  if (!run.waves || run.waves.length === 0) return 0;
  const nodeMap = new Map<string, DagNodeSnapshot>(run.nodes.map((n) => [n.id, n]));
  const sortedWaves = [...run.waves].sort((a, b) => a.index - b.index);

  for (let i = 0; i < sortedWaves.length; i++) {
    const wave = sortedWaves[i];
    const hasRunning = wave.nodeIds.some((id) => nodeMap.get(id)?.state === "running");
    if (hasRunning) return i;
  }

  for (let i = 0; i < sortedWaves.length; i++) {
    const wave = sortedWaves[i];
    const hasActive = wave.nodeIds.some((id) => {
      const state = nodeMap.get(id)?.state;
      return state === "pending" || state === "scheduled" || state === "blocked" || state === "paused";
    });
    if (hasActive) return i;
  }

  return sortedWaves.length - 1;
}

export function formatHeaderSummary(name: string, counts: DagRunCounts): string {
  return `${name} \u2014 ${counts.completed}/${counts.total} done, ${counts.running} running`;
}

export type EdgePathOptions = {
  readonly sourceIndex?: number;
  readonly totalSources?: number;
  readonly targetIndex?: number;
  readonly totalTargets?: number;
};

/**
 * Calculates a smooth bezier curve between two nodes in the DAG layout.
 *
 * For adjacent columns (colSpan <= 1), draws a direct S-curve.
 * For multi-hop edges (colSpan > 1), routes an upward arc above intermediate cards
 * to avoid occluding intermediate nodes or overlapping straight edges.
 * Also distributes source and target port anchor Y positions to prevent arrow
 * marker collisions when multiple edges attach to the same node.
 */
export function calculateEdgePath(
  fromPos: { readonly x: number; readonly y: number },
  toPos: { readonly x: number; readonly y: number },
  options?: EdgePathOptions,
): string {
  const totalSources = options?.totalSources ?? 1;
  const sourceIndex = options?.sourceIndex ?? 0;
  const totalTargets = options?.totalTargets ?? 1;
  const targetIndex = options?.targetIndex ?? 0;

  const sourcePortOffset = totalSources > 1
    ? (sourceIndex + 1) * (28 / (totalSources + 1)) - 14
    : 0;
  const targetPortOffset = totalTargets > 1
    ? (targetIndex + 1) * (28 / (totalTargets + 1)) - 14
    : 0;

  const x1 = fromPos.x + CARD_WIDTH;
  const y1 = fromPos.y + CARD_HEIGHT / 2 + sourcePortOffset;
  const x2 = toPos.x;
  const y2 = toPos.y + CARD_HEIGHT / 2 + targetPortOffset;

  const colStep = CARD_WIDTH + GAP_X;
  const colSpan = Math.round(Math.abs(toPos.x - fromPos.x) / colStep);

  if (colSpan <= 1) {
    const dx = Math.max(20, Math.abs(x2 - x1) * 0.5);
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  }

  const dx = Math.max(24, Math.abs(x2 - x1) * 0.35);
  // Cap the upward arc offset to ensure high colSpan curves stay within the canvas (y >= 10)
  // while still clearing the top edge of intermediate cards (card top is at PAD_Y = 44).
  const arcOffset = Math.min(52, 26 + (colSpan - 1) * 10);
  const cy1 = y1 - arcOffset;
  const cy2 = y2 - arcOffset;

  return `M ${x1} ${y1} C ${x1 + dx} ${cy1}, ${x2 - dx} ${cy2}, ${x2} ${y2}`;
}

