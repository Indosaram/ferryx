import { cleanup, fireEvent, render, screen, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import dagRunSampleJson from "../../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot, type DagRunSnapshot } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import { DagPaneBadge } from "./DagPaneBadge";

const baseSnapshot: DagRunSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

describe("DagPaneBadge", () => {
  beforeEach(() => {
    dagStore.reset();
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("idle hidden: renders null when dagStore has no runs", () => {
    render(<DagPaneBadge projectPath="/repo/my-project" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("idle hidden: renders null when projectPath is undefined or empty", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-active-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    const { unmount } = render(<DagPaneBadge projectPath={undefined} />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    unmount();

    render(<DagPaneBadge projectPath="" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("live matching-project icon with pulse/glow: renders floating bottom-right icon with active animation", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-active-1",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" />);

    const badge = screen.getByTestId("dag-pane-badge");
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveClass("no-drag", "absolute", "bottom-2.5", "right-2.5", "z-30");

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

    render(<DagPaneBadge projectPath="/repo/project-b" />);
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
    const { unmount: unmount1 } = render(<DagPaneBadge projectPath="/repo/a" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount1();

    // Trailing slash match
    const { unmount: unmount2 } = render(<DagPaneBadge projectPath="/repo/a/" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount2();

    // Nested cwd match
    const { unmount: unmount3 } = render(<DagPaneBadge projectPath="/repo/a/nested/subdir" />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    unmount3();

    // Sibling directory with shared prefix must NOT match (/repo/a vs /repo/another)
    const { unmount: unmount4 } = render(<DagPaneBadge projectPath="/repo/another" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    unmount4();

    // Sibling directory /repo/a-pkg must NOT match
    render(<DagPaneBadge projectPath="/repo/a-pkg" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("click opens graph popover upward with live graph and closes on backdrop click", () => {
    const runningRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-popover-1",
      name: "Pipeline Alpha",
      status: "running",
    };
    dagStore.applySnapshot("/repo/my-project", runningRun);

    render(<DagPaneBadge projectPath="/repo/my-project" />);

    // Initially popover is closed
    expect(screen.queryByTestId("dag-pane-popover")).not.toBeInTheDocument();

    // Click badge button to open popover
    const button = screen.getByTestId("dag-pane-badge-button");
    fireEvent.click(button);

    const popover = screen.getByTestId("dag-pane-popover");
    expect(popover).toBeInTheDocument();
    // Popover is anchored bottom-8 right-0 to open upward
    expect(popover).toHaveClass("absolute", "bottom-8", "right-0");

    // Popover contains DAG graph view and run name
    expect(screen.getByTestId("dag-graph-view")).toBeInTheDocument();
    expect(screen.getByTestId("dag-pane-run-run-popover-1")).toHaveTextContent("Pipeline Alpha");

    // Click backdrop to close popover
    const backdrop = screen.getByTestId("dag-pane-popover-backdrop");
    fireEvent.click(backdrop);
    expect(screen.queryByTestId("dag-pane-popover")).not.toBeInTheDocument();
  });

  it("stale terminal run hidden: completed/failed run older than grace period is hidden", () => {
    const now = Date.now();
    const staleTime = new Date(now - 120_000).toISOString(); // 120s ago (>90s grace)

    const completedRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-completed-stale",
      status: "completed",
      completedAt: staleTime,
      updatedAt: staleTime,
    };
    dagStore.applySnapshot("/repo/my-project", completedRun);

    render(<DagPaneBadge projectPath="/repo/my-project" />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("recent completed run is visible without pulse and hides after grace period expires", () => {
    vi.useFakeTimers();
    const baseTime = 1_700_000_000_000;
    vi.setSystemTime(baseTime);

    const recentTime = new Date(baseTime - 30_000).toISOString(); // 30s ago (<90s grace)
    const recentRun: DagRunSnapshot = {
      ...baseSnapshot,
      runId: "run-recent-completed",
      status: "completed",
      completedAt: recentTime,
      updatedAt: recentTime,
    };
    dagStore.applySnapshot("/repo/my-project", recentRun);

    render(<DagPaneBadge projectPath="/repo/my-project" />);

    // Visible within grace period, not pulsing
    const button = screen.getByTestId("dag-pane-badge-button");
    expect(button).toBeInTheDocument();
    expect(button).toHaveAttribute("aria-label", "recent dag runs");
    expect(button).not.toHaveClass("animate-pulse");
    expect(button).toHaveClass("opacity-60");

    // Advance time past the 90s grace period (e.g. 70s later, total 100s since run completed)
    act(() => {
      vi.advanceTimersByTime(70_000);
    });

    // Stale run is now hidden
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("switches graph preview when clicking between multiple popover runs", () => {
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

    render(<DagPaneBadge projectPath="/repo/my-project" />);

    fireEvent.click(screen.getByTestId("dag-pane-badge-button"));

    expect(screen.getByTestId("dag-pane-run-run-1")).toBeInTheDocument();
    expect(screen.getByTestId("dag-pane-run-run-2")).toBeInTheDocument();

    // Default active is latest run (run-2)
    const graphView = screen.getByTestId("dag-graph-view");
    expect(graphView).toHaveAttribute("data-run-id", "run-2");

    // Click run-1
    fireEvent.click(screen.getByTestId("dag-pane-run-run-1"));
    expect(screen.getByTestId("dag-graph-view")).toHaveAttribute("data-run-id", "run-1");
  });
});
