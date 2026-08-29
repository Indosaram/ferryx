import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import dagRunSampleJson from "../../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot, type DagRunSnapshot } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { createDagPaneContent } from "../../lib/types";
import { serializeWorkspaceState, deserializeWorkspaceState } from "../../lib/sessionPersistence";
import { createLayoutState } from "../../state/layout";
import { createLeafNode } from "../../state/paneTree";
import { TerminalSplitView } from "../TerminalSplitView";
import { DagGraphView } from "./DagGraphView";
import { calculateNodePosition } from "./dagViewUtils";

const sampleSnapshot: DagRunSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

describe("DagGraphView", () => {
  beforeEach(() => {
    dagStore.reset();
  });

  afterEach(cleanup);

  it("renders empty state text when no dag runs exist", () => {
    render(<DagGraphView />);
    expect(screen.getByText("No dag runs yet")).toBeInTheDocument();
  });

  it("renders one column per wave (4) for the fixture", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);
    const waveColumns = screen.getAllByTestId("dag-wave-column");
    expect(waveColumns).toHaveLength(4);
  });

  it("positions node cards on the same geometry the edge layer draws to", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);

    const waves = [...sampleSnapshot.waves].sort((a, b) => a.index - b.index);
    waves.forEach((wave, colIndex) => {
      wave.nodeIds.forEach((nodeId, rowIndex) => {
        const expected = calculateNodePosition(colIndex, rowIndex);
        const card = screen.getByTestId(`dag-node-${nodeId}`);
        expect(card.style.left).toBe(`${expected.x}px`);
        expect(card.style.top).toBe(`${expected.y}px`);
      });
    });
  });

  it("draws edges with a theme-aware stroke instead of hardcoded white", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);

    const paths = document.querySelectorAll("path[data-testid^='dag-edge-']");
    expect(paths.length).toBeGreaterThan(0);
    for (const path of paths) {
      expect(path.getAttribute("stroke")).toContain("--foreground-rgb");
    }
  });

  it("renders a card containing the first node label from the fixture", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);
    expect(screen.getByText("SGR attribute extraction")).toBeInTheDocument();
  });

  it("marks critical path nodes", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);
    const criticalNode = screen.getByTestId("dag-node-extract");
    expect(criticalNode).toHaveAttribute("data-critical-path", "true");
  });

  it("shows badge text blocks 3 for bottleneck node", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);
    expect(screen.getByText("blocks 3")).toBeInTheDocument();
  });

  it("header shows derived counts and wave information", () => {
    render(<DagGraphView snapshot={sampleSnapshot} />);
    const header = screen.getByTestId("dag-header");
    expect(header).toHaveTextContent("Ferryx native terminal rendering fixes");
    expect(header).toHaveTextContent("wave 4/4");
    expect(header).toHaveTextContent("1/6 done, 0 running");
  });

  it("reads active run from dagStore when snapshot prop is omitted", () => {
    dagStore.applySnapshot("/repo/project", sampleSnapshot);
    render(<DagGraphView projectPath="/repo/project" />);
    expect(screen.getByText("SGR attribute extraction")).toBeInTheDocument();
    expect(screen.getByText("blocks 3")).toBeInTheDocument();
  });

  it("renders state glyphs and routes accurately across node states", () => {
    const customSnapshot: DagRunSnapshot = {
      ...sampleSnapshot,
      nodes: [
        {
          id: "node-running",
          label: "Agent Task",
          state: "running",
          dependsOn: [],
          attempt: 1,
          route: { kind: "agent", agent: "codex" },
          startedAt: "2026-08-28T16:00:00Z",
          completedAt: null,
          error: null,
          taskId: "task-1",
        },
        {
          id: "node-paused",
          label: "Paused Task",
          state: "paused",
          dependsOn: ["node-running"],
          attempt: 3,
          route: { kind: "category", category: "review" },
          startedAt: null,
          completedAt: null,
          error: null,
          taskId: null,
        },
      ],
      waves: [
        { index: 0, nodeIds: ["node-running"] },
        { index: 1, nodeIds: ["node-paused"] },
      ],
      counts: {
        total: 2,
        completed: 0,
        failed: 0,
        cancelled: 0,
        skipped: 0,
        running: 1,
      },
    };

    render(<DagGraphView snapshot={customSnapshot} />);
    expect(screen.getByText("agent:codex")).toBeInTheDocument();
    expect(screen.getByText("category:review")).toBeInTheDocument();
    expect(screen.getByText("x3")).toBeInTheDocument();
    expect(screen.queryByText("x1")).not.toBeInTheDocument();
  });
});

describe("Dag pane tree integration", () => {
  afterEach(cleanup);

  it("survives layout serialization round-trip for a dag pane leaf", () => {
    const layout = createLayoutState([
      { id: "tab-1", kind: "terminal", label: "Main", sessionId: "sess-1" },
    ]);

    const dagContent = createDagPaneContent({ runId: "run-sample-1" });
    const tabLayout = layout.layoutsByTabId["tab-1"];
    const dagLeafNode = createLeafNode("leaf-dag", "dag");

    const layoutWithDag = {
      ...layout,
      layoutsByTabId: {
        "tab-1": {
          ...tabLayout,
          root: {
            type: "split" as const,
            direction: "horizontal" as const,
            first: tabLayout.root,
            second: dagLeafNode,
            ratio: 0.5,
          },
          sessionIdsByLeafId: {
            ...tabLayout.sessionIdsByLeafId,
            "leaf-dag": "",
          },
          contentsByLeafId: {
            ...tabLayout.contentsByLeafId,
            "leaf-dag": dagContent,
          },
        },
      },
    };

    const workspaceState: any = {
      worktrees: [{ path: "/repo", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
      activeWorktreePath: "/repo",
      sessions: {
        "sess-1": {
          id: "sess-1",
          cwd: "/repo",
          worktreePath: "/repo",
          workspaceId: "ws-1",
          worktree: null,
          backendSessionId: "backend-1",
          lifecycle: "working",
        },
      },
      layout: layoutWithDag,
    };

    const serialized = serializeWorkspaceState("ws-1", "/repo", workspaceState);
    const restored = deserializeWorkspaceState("ws-1", serialized, ["backend-1"]);

    expect(restored).not.toBeNull();
    const restoredTabLayout = restored!.layout.layoutsByTabId["tab-1"];
    expect(restoredTabLayout.contentsByLeafId?.["leaf-dag"]).toMatchObject({
      kind: "dag",
      dag: { runId: "run-sample-1" },
    });
  });

  it("mounts DagGraphView in TerminalSplitView when leaf content kind is dag", () => {
    dagStore.reset();
    dagStore.applySnapshot("/repo", sampleSnapshot);

    const layout = createLayoutState([
      { id: "tab-1", kind: "terminal", label: "Main", sessionId: "sess-1" },
    ]);
    const tabLayout = layout.layoutsByTabId["tab-1"];
    const dagContent = createDagPaneContent({ runId: sampleSnapshot.runId });

    const layoutWithDag = {
      ...layout,
      layoutsByTabId: {
        "tab-1": {
          ...tabLayout,
          root: { type: "leaf" as const, leafId: "leaf-dag" },
          contentsByLeafId: {
            "leaf-dag": dagContent,
          },
        },
      },
    };

    render(
      <TerminalSplitView
        layout={layoutWithDag}
        sessions={{
          "sess-1": {
            id: "sess-1",
            cwd: "/repo",
            worktreePath: "/repo",
            workspaceId: "ws-1",
            worktree: null,
            backendSessionId: "backend-1",
            lifecycle: "working",
          },
        }}
      />,
    );

    expect(screen.getByTestId("dag-graph-view")).toBeInTheDocument();
  });
});
