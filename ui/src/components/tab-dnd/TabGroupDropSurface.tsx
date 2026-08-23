import { useDroppable } from "@dnd-kit/core";
import type { ReactNode } from "react";

import { SplitEdgeDropZone } from "./SplitEdgeDropZone";
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
  return (
    <SplitEdgeDropZone
      id={`group-edge:${groupId}:${edge}`}
      data={{ type: "group-edge", groupId, edge }}
      edge={edge}
      testId="tab-group-edge-drop-zone"
      groupId={groupId}
    />
  );
}
