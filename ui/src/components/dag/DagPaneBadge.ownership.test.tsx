import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { parseDagRunSnapshot } from "../../lib/dagTypes";
import dagRunSampleJson from "../../state/__fixtures__/dagRunSample.json";
import { dagRunOwnership } from "../../state/dagRunOwnership";
import { dagStore } from "../../state/dagStore";
import { DagPaneBadge } from "./DagPaneBadge";

const snapshot = parseDagRunSnapshot(dagRunSampleJson)!;

describe("DAG badge exact pane ownership", () => {
  beforeEach(() => {
    dagStore.reset();
    dagRunOwnership.reset();
  });
  afterEach(cleanup);

  it.each([undefined, null, "", "   ", "foreign-session"])(
    "hides unowned runs with root %s even on a working agent pane",
    (rootSessionId) => {
      dagStore.applySnapshot("/repo", {
        ...snapshot, runId: "run", status: "running", rootSessionId,
      });
      render(<DagPaneBadge projectPath="/repo" paneId="pane"
        providerSessionId="current-session" agentPresent agentWorking />);
      expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
      expect(dagRunOwnership.ownerOf("run")).toBeUndefined();
    },
  );

  it("hides an unresolved owner's run on an idle agent pane", () => {
    dagStore.applySnapshot("/repo", {
      ...snapshot, runId: "run", status: "running", rootSessionId: "foreign-session",
    });
    render(<DagPaneBadge projectPath="/repo" paneId="pane" agentPresent />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });

  it("ignores a stale claim after the pane changes agent sessions", () => {
    dagStore.applySnapshot("/repo", {
      ...snapshot, runId: "run", status: "running", rootSessionId: "owner",
    });
    dagRunOwnership.claim("run", "pane");
    const view = render(<DagPaneBadge projectPath="/repo" paneId="pane"
      providerSessionId="owner" agentPresent agentWorking />);
    fireEvent.click(screen.getByTestId("dag-pane-badge-button"));
    view.rerender(<DagPaneBadge projectPath="/repo" paneId="pane"
      providerSessionId="new-session" agentPresent agentWorking />);
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows only the owner among sibling panes and stays visible while waiting", () => {
    dagStore.applySnapshot("/repo", {
      ...snapshot, runId: "run", status: "running", rootSessionId: "owner",
    });
    const view = render(<>
      <section data-testid="sibling"><DagPaneBadge projectPath="/repo"
        paneId="other" providerSessionId="other" agentPresent agentWorking /></section>
      <section data-testid="owner"><DagPaneBadge projectPath="/repo"
        paneId="owner" providerSessionId="owner" /></section>
    </>);
    expect(screen.getAllByTestId("dag-pane-badge")).toHaveLength(1);
    expect(screen.getByTestId("owner")).toContainElement(screen.getByTestId("dag-pane-badge"));
    view.rerender(<DagPaneBadge projectPath="/repo" paneId="owner"
      providerSessionId="owner" agentPresent agentWorking={false} />);
    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
    act(() => dagStore.removeRun("/repo", "run"));
    expect(screen.queryByTestId("dag-pane-badge")).not.toBeInTheDocument();
  });
});
