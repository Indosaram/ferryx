import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import dagRunSampleJson from "../../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot, type DagRunSnapshot } from "../../lib/dagTypes";
import { dagRunOwnership } from "../../state/dagRunOwnership";
import { dagStore } from "../../state/dagStore";
import { DagPaneBadge } from "./DagPaneBadge";

const baseSnapshot: DagRunSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

describe("DagPaneBadge", () => {
  beforeEach(() => {
    dagStore.reset();
    dagRunOwnership.reset();
  });

  afterEach(() => {
    cleanup();
  });

  it("idle hidden: renders null when dagStore has no runs", () => {
    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("idle hidden: renders null when projectPath is undefined or empty", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-active-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    const { unmount } = render(<DagPaneBadge projectPath={undefined} agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    unmount();

    render(<DagPaneBadge projectPath="" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("live matching-project icon with pulse/glow: renders floating bottom-right icon with active animation", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-active-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);

    const badge = screen.getByTestId("dag-pane-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveClass("no-drag", "absolute", "bottom-0", "right-5", "z-30");

    const button = screen.getByTestId("dag-pane-badge-button");
    expect(button).toHaveAttribute("aria-label", "dag run in progress");
    expect(button).toHaveClass("animate-pulse");
    expect(button).toHaveStyle({ filter: "drop-shadow(0 0 4px currentColor)" });
  });

  it("nonmatching project hidden: does not render when runs belong to another project", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-other-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/project-a", runningRun);

    render(<DagPaneBadge projectPath="/repo/project-b" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("nested cwd matching respects directory boundaries and does not prefix-match sibling directories", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-root-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/a", runningRun);

    // Exact match
    const { unmount: unmount1 } = render(<DagPaneBadge projectPath="/repo/a" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount1();

    // Trailing slash match
    const { unmount: unmount2 } = render(<DagPaneBadge projectPath="/repo/a/" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount2();

    // Nested cwd match
    const { unmount: unmount3 } = render(<DagPaneBadge projectPath="/repo/a/nested/subdir" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount3();

    // Sibling directory with shared prefix must NOT match (/repo/a vs /repo/another)
    const { unmount: unmount4 } = render(<DagPaneBadge projectPath="/repo/another" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    unmount4();

    // Sibling directory /repo/a-pkg must NOT match
    render(<DagPaneBadge projectPath="/repo/a-pkg" agentWorking agentPresent paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("click opens a large centered modal with live graph and closes on backdrop click", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-popover-1",
      name: "Pipeline Alpha",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);

    expect(screen.queryByTestId("dag-pane-modal")).not.toBeInTheDocument();

    const button = screen.getByTestId("dag-pane-badge-button");
    fireEvent.click(button);

    const modal = screen.getByTestId("dag-pane-modal");
    expect(modal).toBeInTheDocument();
    expect(modal).toHaveAttribute("role", "dialog");
    expect(modal).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("dialog")).toBe(modal);
    // The modal fills the window instead of anchoring to the pane so the graph is readable.
    expect(modal).toHaveClass("h-[min(820px,86vh)]", "w-[min(1280px,92vw)]");
    expect(modal.closest("[data-testid='dag-pane-badge']")).toBeNull();

    expect(screen.getByTestId("dag-graph-view")).toBeInTheDocument();
    expect(screen.getByTestId("dag-pane-modal-title")).toHaveTextContent("Pipeline Alpha");

    const backdrop = screen.getByTestId("dag-pane-modal-backdrop");
    fireEvent.click(backdrop);
    expect(screen.queryByTestId("dag-pane-modal")).not.toBeInTheDocument();
  });

  it("keeps the badge through a working -> waiting flap because the pane owns the run", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-latched",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    const { rerender } = render(
      <DagPaneBadge projectPath="/repo/my-project" paneId="pane-a" agentPresent agentWorking />,
    );
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();

    rerender(
      <DagPaneBadge
        projectPath="/repo/my-project"
        paneId="pane-a"
        agentPresent
        agentWorking={false}
      />,
    );
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
  });

  it("never lets a sibling pane of the same project take over an owned run", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-owned",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(
      <DagPaneBadge projectPath="/repo/my-project" paneId="pane-a" agentPresent agentWorking />,
    );
    expect(screen.getAllByTestId("dag-pane-badge")).toHaveLength(1);

    render(
      <DagPaneBadge projectPath="/repo/my-project" paneId="pane-b" agentPresent agentWorking />,
    );
    expect(screen.getAllByTestId("dag-pane-badge")).toHaveLength(1);
  });

  it("releases ownership once the run stops running so a later run can be reclaimed", () => {
    dagStore.applySnapshot("/repo/my-project", {
      ...baseSnapshot,
      runId: "run-cycle",
      status: "running",
    });

    render(
      <DagPaneBadge projectPath="/repo/my-project" paneId="pane-a" agentPresent agentWorking />,
    );
    expect(dagRunOwnership.ownerOf("run-cycle")).toBe("pane-a");

    act(() => {
      dagStore.applySnapshot("/repo/my-project", {
        ...baseSnapshot,
        runId: "run-cycle",
        status: "completed",
      });
    });
    expect(dagRunOwnership.ownerOf("run-cycle")).toBeUndefined();
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("pane without an agent stays clean even when its project has a running dag", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-sibling-shell",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking={false} agentPresent={false} paneId="pane-a" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("badge renders a graph glyph so the running run is recognizable", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-glyph",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);

    const button = screen.getByTestId("dag-pane-badge-button");
    const glyph = button.querySelector("svg");
    expect(glyph).not.toBeNull();
    expect(glyph?.querySelectorAll("circle")).toHaveLength(3);
    expect(button).toHaveClass("text-indigo-300");
  });

  it("non-running runs hidden: completed, failed, cancelled, and paused runs render null immediately without grace period", () => {
    const nonRunningStatuses = ["completed", "failed", "cancelled", "paused"] as const;

    for (const status of nonRunningStatuses) {
      dagStore.reset();
      const nonRunningRun: DagRunSnapshot = {
        ...baseSnapshot,
        runId: `run-${status}`,
        status,
        updatedAt: new Date().toISOString(),
      };
      dagStore.applySnapshot("/repo/my-project", nonRunningRun);

      const { unmount } = render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);
      expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
      unmount();
    }
  });

  it("disappears immediately when run transitions away from running, closing an open modal", () => {
    const activeRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-live-1",
      name: "Pipeline In Progress",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", activeRun);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);

    // Open popover
    fireEvent.click(screen.getByTestId("dag-pane-badge-button"));
    expect(screen.getByTestId("dag-pane-modal")).toBeInTheDocument();

    // Run completes
    act(() => {
      const completedRun: DagRunSnapshot = {
        ...activeRun,
        status: "completed",
      };
      dagStore.applySnapshot("/repo/my-project", completedRun);
    });

    // Entire badge and popover must disappear immediately
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("dag-pane-modal")).not.toBeInTheDocument();
  });

  it("renders the run name once in the modal, not once per nested header", () => {
    dagStore.applySnapshot("/repo/my-project", {
      ...baseSnapshot,
      runId: "run-single-title",
      name: "Unique Run Title",
      status: "running",
    });

    render(
      <DagPaneBadge projectPath="/repo/my-project" paneId="pane-a" agentPresent agentWorking />,
    );
    fireEvent.click(screen.getByTestId("dag-pane-badge-button"));

    expect(screen.getAllByText("Unique Run Title")).toHaveLength(1);
  });

  it("shows only this pane's newest run instead of listing every project run", () => {
    const run1: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-1",
      name: "Run First",
      status: "running",
      updatedAt: "2026-08-29T12:00:00.000Z",
    };
    const run2: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-2",
      name: "Run Second",
      status: "running",
      updatedAt: "2026-08-29T12:01:00.000Z",
    };
    dagStore.applySnapshot("/repo/my-project", run1);
    dagStore.applySnapshot("/repo/my-project", run2);

    render(<DagPaneBadge projectPath="/repo/my-project" agentWorking agentPresent paneId="pane-a" />);

    fireEvent.click(screen.getByTestId("dag-pane-badge-button"));

    expect(screen.getByTestId("dag-pane-run-run-2")).toBeInTheDocument();
    expect(screen.queryByTestId("dag-pane-run-run-1")).not.toBeInTheDocument();

    expect(screen.getByTestId("dag-graph-view")).toHaveAttribute("data-run-id", "run-2");
    expect(screen.getByTestId("dag-pane-modal-title")).toHaveTextContent("Run Second");
  });


});
