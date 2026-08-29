import React from "react";
import type { DagEdge } from "../../lib/dagTypes";
import { CARD_HEIGHT, CARD_WIDTH } from "./dagViewUtils";

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
  const criticalEdgesSet = React.useMemo(() => {
    const set = new Set<string>();
    for (let i = 0; i < criticalPath.length - 1; i++) {
      set.add(`${criticalPath[i]}->${criticalPath[i + 1]}`);
    }
    return set;
  }, [criticalPath]);

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
          id="dag-arrow"
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
          id="dag-arrow-critical"
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

        const x1 = fromPos.x + CARD_WIDTH;
        const y1 = fromPos.y + CARD_HEIGHT / 2;
        const x2 = toPos.x;
        const y2 = toPos.y + CARD_HEIGHT / 2;
        const dx = Math.max(20, Math.abs(x2 - x1) * 0.5);

        const d = `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
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
            markerEnd={isCritical ? "url(#dag-arrow-critical)" : "url(#dag-arrow)"}
            data-testid={`dag-edge-${edge.from}-${edge.to}`}
            data-critical={isCritical ? "true" : "false"}
          />
        );
      })}
    </svg>
  );
}
