import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import type { DagNodeSnapshot } from "../../lib/dagTypes";
import { DagNodeInspector } from "./DagNodeInspector";
import * as tauriBridge from "../../lib/tauri";

vi.mock("../../lib/tauri", () => ({
  dagReadNodeArtifact: vi.fn(),
  isTauri: vi.fn(() => true),
}));

const mockNode = (overrides?: Partial<DagNodeSnapshot>): DagNodeSnapshot => ({
  id: "step_1",
  label: "Build Binary",
  state: "completed",
  dependsOn: [],
  attempt: 1,
  route: { kind: "category", category: "quick" },
  startedAt: "2026-09-17T10:00:00.000Z",
  completedAt: "2026-09-17T10:01:30.000Z",
  taskId: "st_01a04db8",
  prompt: "TASK: Compile native binary. DELIVERABLE: bin/out. SCOPE: src.",
  runStats: {
    runtimeMs: 90000,
    turns: 4,
    toolCalls: 3,
    inputTokens: 15000,
    outputTokens: 600,
    totalTokens: 15600,
    generationMs: 400,
    tokensPerSecond: 750,
    costUsd: 0.0032,
    cacheReadTokens: 10000,
    cacheWriteTokens: 1200,
  },
  resultArtifact: {
    relativePath: "dag/results/dag_123/step_1.txt",
    sha256: "sha256_mock_hash",
    bytes: 2048,
  },
  error: null,
  ...overrides,
});

describe("DagNodeInspector", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(tauriBridge.dagReadNodeArtifact).mockResolvedValue("Default artifact content");
  });

  afterEach(() => {
    cleanup();
  });

  it("returns null when node is null", () => {
    const { container } = render(
      <DagNodeInspector node={null} onClose={vi.fn()} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders header, meta bar, and loads deliverable artifact on mount", async () => {
    vi.mocked(tauriBridge.dagReadNodeArtifact).mockResolvedValueOnce(
      "Generated deliverable contents",
    );

    render(
      <DagNodeInspector
        node={mockNode()}
        projectPath="/test/project"
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("Build Binary")).toBeInTheDocument();
    expect(screen.getByText("st_01a04db8")).toBeInTheDocument();
    expect(screen.getByText("1m 30s")).toBeInTheDocument();
    expect(screen.getByText("15.6k")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByText("Generated deliverable contents")).toBeInTheDocument();
    });
  });

  it("switches to prompt tab and displays prompt content", () => {
    render(
      <DagNodeInspector
        node={mockNode()}
        projectPath="/test/project"
        onClose={vi.fn()}
      />,
    );

    const promptTab = screen.getByRole("tab", { name: /prompt/i });
    fireEvent.click(promptTab);

    expect(screen.getByText("TASK: Compile native binary. DELIVERABLE: bin/out. SCOPE: src.")).toBeInTheDocument();
  });

  it("switches to stats tab and renders token and throughput details", () => {
    render(
      <DagNodeInspector
        node={mockNode()}
        projectPath="/test/project"
        onClose={vi.fn()}
      />,
    );

    const statsTab = screen.getByRole("tab", { name: /stats/i });
    fireEvent.click(statsTab);

    expect(screen.getByText("Token Consumption")).toBeInTheDocument();
    expect(screen.getByText("15,000")).toBeInTheDocument();
    expect(screen.getByText("15,600")).toBeInTheDocument();
    expect(screen.getByText("750 tok/s")).toBeInTheDocument();
    expect(screen.getByText("$0.0032 USD")).toBeInTheDocument();
  });

  it("renders error tab automatically on failed node", () => {
    const failedNode = mockNode({
      state: "failed",
      error: {
        code: "task_error",
        message: "Child turn exited abnormally",
        at: "2026-09-17T10:01:00.000Z",
      },
    });

    render(
      <DagNodeInspector
        node={failedNode}
        projectPath="/test/project"
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("task_error")).toBeInTheDocument();
    expect(screen.getByText("Child turn exited abnormally")).toBeInTheDocument();
  });

  it("supports close and navigation between nodes", () => {
    const node1 = mockNode({ id: "step_1", label: "Step 1" });
    const node2 = mockNode({ id: "step_2", label: "Step 2" });
    const onSelectNode = vi.fn();
    const onClose = vi.fn();

    render(
      <DagNodeInspector
        node={node1}
        allNodes={[node1, node2]}
        onSelectNode={onSelectNode}
        onClose={onClose}
      />,
    );

    const nextBtn = screen.getByRole("button", { name: /next node/i });
    fireEvent.click(nextBtn);
    expect(onSelectNode).toHaveBeenCalledWith("step_2");

    const closeBtn = screen.getByRole("button", { name: /close inspector/i });
    fireEvent.click(closeBtn);
    expect(onClose).toHaveBeenCalled();
  });
});
