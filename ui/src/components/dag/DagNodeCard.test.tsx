import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { DagNodeSnapshot, DagNodeState } from "../../lib/dagTypes";
import { DagNodeCard } from "./DagNodeCard";

function node(state: DagNodeState): DagNodeSnapshot {
  return {
    id: `node-${state}`,
    label: `step ${state}`,
    state,
    dependsOn: [],
    attempt: 1,
    route: { kind: "category", category: "quick" },
    startedAt: null,
    completedAt: null,
    error: null,
    taskId: null,
  } as DagNodeSnapshot;
}

describe("DagNodeCard", () => {
  afterEach(() => {
    cleanup();
  });

  it("renders a running node calmly, without a blinking whole-card animation", () => {
    render(<DagNodeCard node={node("running")} isCriticalPath={false} blockedCount={0} />);

    const card = screen.getByTestId("dag-node-node-running");
    expect(card.className).not.toContain("animate-pulse");
    expect(card.className).not.toContain("animate-");
  });

  it("names every state in words so the graph is readable at a glance", () => {
    const expectations: Array<[DagNodeState, string]> = [
      ["running", "running"],
      ["completed", "done"],
      ["failed", "failed"],
      ["pending", "waiting"],
      ["scheduled", "queued"],
    ];

    for (const [state, label] of expectations) {
      const { unmount } = render(
        <DagNodeCard node={node(state)} isCriticalPath={false} blockedCount={0} />,
      );
      expect(screen.getByTestId("dag-node-state-label")).toHaveTextContent(label);
      unmount();
    }
  });

  it("keeps the bottleneck badge readable instead of amber-on-amber", () => {
    render(<DagNodeCard node={node("running")} isCriticalPath={false} blockedCount={3} />);

    const badge = screen.getByTestId("dag-bottleneck-badge");
    expect(badge).toHaveTextContent("blocks 3");
    expect(badge.className).toContain("text-amber-600");
  });
});
