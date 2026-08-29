import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { parseDagRunSnapshot, type DagRunSnapshot } from "../../lib/dagTypes";
import { dagStore } from "../../state/dagStore";
import fixture from "../../state/__fixtures__/dagRunSample.json";
import { DagActivityBadge } from "./DagActivityBadge";

const COMPLETED_GRACE_MS = 90_000;
const PROJECT = "/tmp/dag-badge-test";
const parsed = parseDagRunSnapshot(fixture) as DagRunSnapshot;

function runningRun(overrides: Partial<DagRunSnapshot> = {}): DagRunSnapshot {
  return {
    ...parsed,
    status: "running",
    updatedAt: new Date().toISOString(),
    counts: { ...parsed.counts, running: 2 },
    ...overrides,
  };
}

function applyRunning(): DagRunSnapshot {
  const run = runningRun();
  dagStore.applySnapshot(PROJECT, run);
  return run;
}

afterEach(() => {
  cleanup();
  dagStore.removeRun(PROJECT, parsed.runId);
});

describe("DagActivityBadge", () => {
  it("renders nothing when no dag runs are live", () => {
    render(<DagActivityBadge />);
    expect(screen.queryByTestId("dag-activity-badge")).not.toBeInTheDocument();
  });

  it("shows the glowing graph badge while a run is running", () => {
    applyRunning();
    render(<DagActivityBadge />);
    const badge = screen.getByTestId("dag-activity-badge");
    expect(badge.querySelector("svg.animate-pulse")).not.toBeNull();
    expect(badge.querySelector("svg")).not.toBeNull();
  });

  it("opens the popover with the running run and its graph on click", () => {
    const run = applyRunning();
    render(<DagActivityBadge />);
    fireEvent.click(screen.getByRole("button"));
    expect(screen.getByTestId("dag-activity-popover")).toBeInTheDocument();
    expect(screen.getByTestId(`dag-activity-run-${run.runId}`)).toBeInTheDocument();
    expect(screen.getAllByText(run.name).length).toBeGreaterThan(0);
    expect(screen.getByTestId("dag-graph-view")).toBeInTheDocument();
  });

  it("hides the badge after the run completes outside the grace window", () => {
    const stale = runningRun({
      status: "completed",
      updatedAt: new Date(Date.now() - 10 * COMPLETED_GRACE_MS).toISOString(),
    });
    dagStore.applySnapshot(PROJECT, stale);
    render(<DagActivityBadge />);
    expect(screen.queryByTestId("dag-activity-badge")).not.toBeInTheDocument();
  });
});
