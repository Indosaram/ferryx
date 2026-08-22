import { useDroppable } from "@dnd-kit/core";
import type { ReactNode } from "react";

import { cn } from "../../lib/cn";
import type { TabDropEdge } from "./tabDragTypes";

type TabGroupDropSurfaceProps = {
  groupId: string;
  children: ReactNode;
};

export function TabGroupDropSurface({ groupId, children }: TabGroupDropSurfaceProps) {
  const body = useDroppable({
    id: `group-body:${groupId}`,
    data: { type: "group-body", groupId },
  });

  return (
    <div
      ref={body.setNodeRef}
      data-tab-group-body-id={groupId}
      data-dnd-type="group-body"
      className="relative min-h-0 flex-1 overflow-hidden bg-terminal"
    >
      {children}
      <TabGroupEdgeDropZone groupId={groupId} edge="left" />
      <TabGroupEdgeDropZone groupId={groupId} edge="right" />
      <TabGroupEdgeDropZone groupId={groupId} edge="top" />
      <TabGroupEdgeDropZone groupId={groupId} edge="bottom" />
    </div>
  );
}

function TabGroupEdgeDropZone({ groupId, edge }: { groupId: string; edge: TabDropEdge }) {
  const { setNodeRef, isOver } = useDroppable({
    id: `group-edge:${groupId}:${edge}`,
    data: { type: "group-edge", groupId, edge },
  });

  return (
    <div
      ref={setNodeRef}
      aria-hidden="true"
      data-testid="tab-group-edge-drop-zone"
      data-drop-edge={edge}
      data-drop-group-id={groupId}
      data-dnd-type="group-edge"
      className={cn(
        "pointer-events-none absolute z-40 transition-colors",
        edge === "left" && "inset-y-0 left-0 w-[20%]",
        edge === "right" && "inset-y-0 right-0 w-[20%]",
        edge === "top" && "left-[20%] right-[20%] top-0 h-[20%]",
        edge === "bottom" && "bottom-0 left-[20%] right-[20%] h-[20%]",
        isOver && "border-2 border-primary bg-primary/20 backdrop-blur-[1px]",
      )}
    >
      {isOver ? (
        <span className="absolute bottom-2 left-2 rounded-sm bg-background/80 px-1.5 py-0.5 text-[10px] font-medium text-foreground shadow-sm">
          New split
        </span>
      ) : null}
    </div>
  );
}
