import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const overIds = vi.hoisted(() => new Set<string>());

vi.mock("@dnd-kit/core", () => ({
  useDroppable: ({ id }: { id: string }) => ({
    setNodeRef: vi.fn(),
    isOver: overIds.has(id),
  }),
}));

import { SplitEdgeDropZone } from "./SplitEdgeDropZone";

afterEach(() => {
  cleanup();
  overIds.clear();
});

describe("SplitEdgeDropZone", () => {
  it("renders identical tab-style split feedback for group-edge and pane-edge targets", () => {
    overIds.add("group-edge:group-a:left");
    overIds.add("pane-edge:tab-a:leaf-a:left");

    render(
      <>
        <SplitEdgeDropZone
          id="group-edge:group-a:left"
          data={{ type: "group-edge", groupId: "group-a", edge: "left" }}
          edge="left"
          testId="group-edge"
          groupId="group-a"
        />
        <SplitEdgeDropZone
          id="pane-edge:tab-a:leaf-a:left"
          data={{ type: "pane-edge", tabId: "tab-a", leafId: "leaf-a", edge: "left" }}
          edge="left"
          testId="pane-edge"
          tabId="tab-a"
          leafId="leaf-a"
        />
      </>,
    );

    const groupEdge = screen.getByTestId("group-edge");
    const paneEdge = screen.getByTestId("pane-edge");
    expect(groupEdge.className).toBe(paneEdge.className);
    expect(groupEdge).toHaveAttribute("data-split-edge-drop-zone", "true");
    expect(paneEdge).toHaveAttribute("data-split-edge-drop-zone", "true");
    expect(groupEdge).toHaveClass("inset-y-0", "left-0", "w-[20%]");
    expect(paneEdge).toHaveClass("inset-y-0", "left-0", "w-[20%]");

    const feedback = screen.getAllByTestId("split-edge-drop-feedback");
    expect(feedback).toHaveLength(2);
    expect(feedback.map((node) => node.textContent)).toEqual(["New split", "New split"]);
    expect(feedback[0].className).toBe(feedback[1].className);
  });

  it.each([
    {
      edge: "left" as const,
      expectedHitClasses: ["inset-y-0", "left-0", "w-[20%]"],
      expectedPreviewClasses: ["inset-y-0", "left-0"],
      halfDimensionPattern: /w-(1\/2|\[50%\])/,
    },
    {
      edge: "right" as const,
      expectedHitClasses: ["inset-y-0", "right-0", "w-[20%]"],
      expectedPreviewClasses: ["inset-y-0", "right-0"],
      halfDimensionPattern: /w-(1\/2|\[50%\])/,
    },
    {
      edge: "top" as const,
      expectedHitClasses: ["left-[20%]", "right-[20%]", "top-0", "h-[20%]"],
      expectedPreviewClasses: ["top-0"],
      halfDimensionPattern: /h-(1\/2|\[50%\])/,
    },
    {
      edge: "bottom" as const,
      expectedHitClasses: ["bottom-0", "left-[20%]", "right-[20%]", "h-[20%]"],
      expectedPreviewClasses: ["bottom-0"],
      halfDimensionPattern: /h-(1\/2|\[50%\])/,
    },
  ])(
    "retains 20% droppable hit strip but renders distinct 50% half-parent visual preview for $edge edge",
    ({ edge, expectedHitClasses, expectedPreviewClasses, halfDimensionPattern }) => {
      const zoneId = `pane-edge:tab-1:leaf-1:${edge}`;
      overIds.add(zoneId);

      render(
        <SplitEdgeDropZone
          id={zoneId}
          data={{ type: "pane-edge", tabId: "tab-1", leafId: "leaf-1", edge }}
          edge={edge}
          testId={`split-zone-${edge}`}
          tabId="tab-1"
          leafId="leaf-1"
        />,
      );

      const hitStrip = screen.getByTestId(`split-zone-${edge}`);
      expect(hitStrip).toHaveAttribute("data-split-edge-drop-zone", "true");
      expect(hitStrip).toHaveAttribute("data-drop-edge", edge);
      for (const cls of expectedHitClasses) {
        expect(hitStrip).toHaveClass(cls);
      }

      // A distinct visual preview element must cover exactly half (50%) of the parent
      const preview = screen.getByTestId("split-edge-preview");
      expect(preview).not.toBe(hitStrip);
      expect(preview).toHaveAttribute("data-split-edge-preview", "true");
      expect(preview).toHaveAttribute("data-preview-edge", edge);

      for (const cls of expectedPreviewClasses) {
        expect(preview).toHaveClass(cls);
      }
      expect(preview.className).toMatch(halfDimensionPattern);

      if (edge === "top" || edge === "bottom") {
        expect(preview.className).toMatch(/inset-x-0|left-0.*right-0/);
      }
    },
  );

  it("does not render visual split preview when isOver is false", () => {
    render(
      <SplitEdgeDropZone
        id="pane-edge:tab-1:leaf-1:left"
        data={{ type: "pane-edge", tabId: "tab-1", leafId: "leaf-1", edge: "left" }}
        edge="left"
        testId="split-zone-left"
        tabId="tab-1"
        leafId="leaf-1"
      />,
    );

    expect(screen.getByTestId("split-zone-left")).toBeInTheDocument();
    expect(screen.queryByTestId("split-edge-preview")).toBeNull();
  });
});
