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
  }
}

export function formatRouteText(route: DagNodeRoute): string {
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

  for (let i = 0; i < run.waves.length; i++) {
    const wave = run.waves[i];
    const hasRunning = wave.nodeIds.some((id) => nodeMap.get(id)?.state === "running");
    if (hasRunning) return i;
  }

  for (let i = 0; i < run.waves.length; i++) {
    const wave = run.waves[i];
    const hasActive = wave.nodeIds.some((id) => {
      const state = nodeMap.get(id)?.state;
      return state === "pending" || state === "scheduled" || state === "blocked" || state === "paused";
    });
    if (hasActive) return i;
  }

  return run.waves.length - 1;
}

export function formatHeaderSummary(name: string, activeWave: number, totalWaves: number, counts: DagRunCounts): string {
  return `${name} \u2014 wave ${activeWave}/${totalWaves} \u2014 ${counts.completed}/${counts.total} done, ${counts.running} running`;
}
