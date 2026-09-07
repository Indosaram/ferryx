import React from "react";
import type { DagEdge } from "../../lib/dagTypes";
import { calculateEdgePath } from "./dagViewUtils";

export type DagEdgeLayerProps = {
  readonly edges: readonly DagEdge[];
  readonly nodePositions: ReadonlyMap<string, { x: number; y: number }>;
  readonly criticalPath: readonly string[];
  readonly width: number;
  readonly height: number;
};

export function DagEdgeLayer({
  edges,
  nodePositions,
  criticalPath,
  width,
  height,
}: DagEdgeLayerProps): JSX.Element {
  const arrowMarkerId = React.useId();
  const arrowCriticalMarkerId = React.useId();

  const criticalEdgesSet = React.useMemo(() => {
    const set = new Set<string>();
    for (let i = 0; i < criticalPath.length - 1; i++) {
      set.add(`${criticalPath[i]}->${criticalPath[i + 1]}`);
    }
    return set;
  }, [criticalPath]);

  // Group edges by source and target to calculate distinct anchor port offsets.
  // This prevents multiple incoming/outgoing arrows from overlapping exactly on top of each other.
  const outgoingMap = React.useMemo(() => {
    const map = new Map<string, DagEdge[]>();
    for (const edge of edges) {
      const list = map.get(edge.from) ?? [];
      list.push(edge);
      map.set(edge.from, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) => {
        const ax = nodePositions.get(a.to)?.x ?? 0;
        const bx = nodePositions.get(b.to)?.x ?? 0;
        return bx - ax;
      });
    }
    return map;
  }, [edges, nodePositions]);

  const incomingMap = React.useMemo(() => {
    const map = new Map<string, DagEdge[]>();
    for (const edge of edges) {
      const list = map.get(edge.to) ?? [];
      list.push(edge);
      map.set(edge.to, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) => {
        const ax = nodePositions.get(a.from)?.x ?? 0;
        const bx = nodePositions.get(b.from)?.x ?? 0;
        return ax - bx;
      });
    }
    return map;
  }, [edges, nodePositions]);

  return (
    <svg
      className="absolute inset-0 pointer-events-none"
      width={Math.max(width, 100)}
      height={Math.max(height, 100)}
      style={{ minWidth: "100%", minHeight: "100%" }}
      data-testid="dag-edge-layer"
    >
      <defs>
        <marker
          id={arrowMarkerId}
          viewBox="0 0 6 6"
          refX="5"
          refY="3"
          markerWidth="4"
          markerHeight="4"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 6 3 L 0 6 z" fill="rgb(var(--foreground-rgb) / 0.45)" />
        </marker>
        <marker
          id={arrowCriticalMarkerId}
          viewBox="0 0 6 6"
          refX="5"
          refY="3"
          markerWidth="4"
          markerHeight="4"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 6 3 L 0 6 z" fill="rgb(var(--foreground-rgb) / 0.9)" />
        </marker>
      </defs>

      {edges.map((edge) => {
        const fromPos = nodePositions.get(edge.from);
        const toPos = nodePositions.get(edge.to);
        if (!fromPos || !toPos) return null;

        const outList = outgoingMap.get(edge.from) ?? [];
        const sourceIndex = outList.indexOf(edge);
        const totalSources = outList.length;

        const inList = incomingMap.get(edge.to) ?? [];
        const targetIndex = inList.indexOf(edge);
        const totalTargets = inList.length;

        const d = calculateEdgePath(fromPos, toPos, {
          sourceIndex: sourceIndex >= 0 ? sourceIndex : 0,
          totalSources,
          targetIndex: targetIndex >= 0 ? targetIndex : 0,
          totalTargets,
        });
        const isCritical = criticalEdgesSet.has(`${edge.from}->${edge.to}`);

        return (
          <path
            key={`${edge.from}->${edge.to}`}
            d={d}
            fill="none"
            stroke={
              isCritical
                ? "rgb(var(--foreground-rgb) / 0.8)"
                : "rgb(var(--foreground-rgb) / 0.35)"
            }
            strokeWidth={isCritical ? 2 : 1.5}
            markerEnd={isCritical ? `url(#${arrowCriticalMarkerId})` : `url(#${arrowMarkerId})`}
            data-testid={`dag-edge-${edge.from}-${edge.to}`}
            data-critical={isCritical ? "true" : "false"}
          />
        );
      })}
    </svg>
  );
}
