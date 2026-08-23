import type { TabDropEdge } from "./tabDragTypes";
import { SplitEdgeDropZone } from "./SplitEdgeDropZone";

const EDGES: TabDropEdge[] = ["left", "right", "top", "bottom"];

type PaneEdgeDropZonesProps = {
  tabId: string;
  leafId: string;
};

export function PaneEdgeDropZones({ tabId, leafId }: PaneEdgeDropZonesProps) {
  return (
    <>
      {EDGES.map((edge) => (
        <SplitEdgeDropZone
          key={edge}
          id={`pane-edge:${tabId}:${leafId}:${edge}`}
          data={{ type: "pane-edge", tabId, leafId, edge }}
          edge={edge}
          testId="pane-edge-drop-zone"
          tabId={tabId}
          leafId={leafId}
        />
      ))}
    </>
  );
}
