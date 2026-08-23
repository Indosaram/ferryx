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

export function edgeHitTargetClass(edge: TabDropEdge): string {
  switch (edge) {
    case "left":
      return "inset-y-0 left-0 w-[20%]";
    case "right":
      return "inset-y-0 right-0 w-[20%]";
    case "top":
      return "left-[20%] right-[20%] top-0 h-[20%]";
    case "bottom":
      return "bottom-0 left-[20%] right-[20%] h-[20%]";
  }
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
