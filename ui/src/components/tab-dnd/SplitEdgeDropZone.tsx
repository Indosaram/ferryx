import { useDroppable } from "@dnd-kit/core";

import { cn } from "../../lib/cn";
import type { GroupEdgeDropData, PaneEdgeDropData, TabDropEdge } from "./tabDragTypes";

type SplitEdgeDropData = GroupEdgeDropData | PaneEdgeDropData;

type SplitEdgeDropZoneProps = {
  id: string;
  data: SplitEdgeDropData;
  edge: TabDropEdge;
  testId: string;
  groupId?: string;
  tabId?: string;
  leafId?: string;
};

/**
 * Shared split-edge target used by both tab drags and pane-handle drags.
 * Drag source only changes the command resolved after drop; target geometry and
 * visual feedback intentionally follow the same tab-drag contract.
 */
export function SplitEdgeDropZone({
  id,
  data,
  edge,
  testId,
  groupId,
  tabId,
  leafId,
}: SplitEdgeDropZoneProps) {
  const { setNodeRef, isOver } = useDroppable({ id, data });

  return (
    <>
      <div
        ref={setNodeRef}
        aria-hidden="true"
        data-testid={testId}
        data-split-edge-drop-zone="true"
        data-dnd-type={data.type}
        data-drop-edge={edge}
        data-drop-group-id={groupId}
        data-tab-id={tabId}
        data-leaf-id={leafId}
        className={cn(
          "pointer-events-none absolute z-40 transition-colors",
          edgeHitTargetClass(edge),
        )}
      />
      {isOver ? (
        <div
          aria-hidden="true"
          data-testid="split-edge-preview"
          data-split-edge-preview="true"
          data-preview-edge={edge}
          className={cn(
            "pointer-events-none absolute z-40 border-2 border-primary bg-primary/20 backdrop-blur-[1px] transition-colors",
            edgePreviewClass(edge),
          )}
        >
          <span
            data-testid="split-edge-drop-feedback"
            className="absolute bottom-2 left-2 rounded-sm bg-background/80 px-1.5 py-0.5 text-[10px] font-medium text-foreground shadow-sm"
          >
            New split
          </span>
        </div>
      ) : null}
    </>
  );
}

/**
 * Every edge zone measures the whole surface, so a pointer anywhere inside it collides with
 * all four. `resolveSplitEdgeForPoint` then picks the single winning edge, which is what puts
 * the boundaries exactly on the two center lines instead of leaving a dead middle.
 */
export function edgeHitTargetClass(_edge: TabDropEdge): string {
  return "inset-0";
}

/**
 * Maps a pointer position inside `rect` to the edge whose half it falls in, splitting along
 * the two diagonals so the vertical center line separates left/right and the horizontal one
 * separates top/bottom. Ties resolve to the horizontal split, matching the toolbar default.
 */
export function resolveSplitEdgeForPoint(
  point: { x: number; y: number },
  rect: { left: number; top: number; width: number; height: number },
): TabDropEdge {
  if (rect.width <= 0 || rect.height <= 0) return "right";

  const normalizedX = (point.x - rect.left) / rect.width - 0.5;
  const normalizedY = (point.y - rect.top) / rect.height - 0.5;

  if (Math.abs(normalizedY) > Math.abs(normalizedX)) {
    return normalizedY < 0 ? "top" : "bottom";
  }
  return normalizedX < 0 ? "left" : "right";
}

function edgePreviewClass(edge: TabDropEdge): string {
  switch (edge) {
    case "left":
      return "inset-y-0 left-0 w-1/2";
    case "right":
      return "inset-y-0 right-0 w-1/2";
    case "top":
      return "inset-x-0 top-0 h-1/2";
    case "bottom":
      return "inset-x-0 bottom-0 h-1/2";
  }
}
