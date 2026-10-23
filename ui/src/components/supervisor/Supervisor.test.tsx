import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { setSupervisorSnapshotForTest, useSupervisor, type SupervisorSnapshot } from "../../lib/supervisor";
import { SupervisorPaneBarView } from "./SupervisorPaneBar";
import { SupervisorPanelView } from "./SupervisorPanel";

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/repo/main", goal: "Get tests green", paused: false, lastActivityAt: "" };
const now = Date.UTC(2026, 9, 23, 12, 0, 0);

afterEach(cleanup);

describe("useSupervisor", () => {
  it("settles after a snapshot instead of re-rendering itself", () => {
    let renders = 0;
    function Probe() {
      renders += 1;
      useSupervisor();
      return null;
    }
    render(<Probe />);
    act(() => {
      setSupervisorSnapshotForTest({
        hub: { available: true, running: true },
        lanes: [lane],
        watched: [],
        actions: [{ deviceId: "owner", op: "send", sessionId: "s1", requestId: "r", code: "OK", atMs: Date.now(), text: "x" }],
      });
    });
    expect(renders).toBeLessThanOrEqual(4);
  });
});

describe("SupervisorPaneBarView", () => {
  it("shows the exact sent text with Take over while acting", () => {
    render(<SupervisorPaneBarView state={{ kind: "acting", text: "cargo test -p ferryx-hub", atMs: now - 2000, lane }} now={now} />);
    expect(screen.getByTestId("supervisor-pane-bar").dataset.supervision).toBe("acting");
    expect(screen.getByText("cargo test -p ferryx-hub")).toBeTruthy();
    expect(screen.getByText("2s ago")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Take over" })).toBeTruthy();
  });
  it("offers Resume when paused", () => {
    render(<SupervisorPaneBarView state={{ kind: "paused", lane: { ...lane, paused: true } }} now={now} />);
    expect(screen.getByRole("button", { name: /Resume/ })).toBeTruthy();
  });
});

describe("SupervisorPanelView", () => {
  it("is honest when no hub is open", () => {
    const off: SupervisorSnapshot = { hub: { available: false, running: false }, lanes: [], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={off} now={now} />);
    expect(screen.getByTestId("supervisor-panel").dataset.supervisorState).toBe("off");
    expect(screen.getByText("The supervisor is not running")).toBeTruthy();
  });
  it("lists lanes with goal and pause, and the recent input", () => {
    const snap: SupervisorSnapshot = {
      hub: { available: true, running: true },
      lanes: [lane],
      watched: [],
      actions: [{ deviceId: "owner", op: "send", sessionId: "s1", requestId: "r", code: "OK", atMs: now - 1000, text: "run tests" }],
    };
    render(<SupervisorPanelView snapshot={snap} now={now} />);
    expect(screen.getByTestId("supervisor-panel").dataset.supervisorState).toBe("typing");
    expect(screen.getByText("Get tests green")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Pause Ledger" })).toBeTruthy();
    expect(screen.getByTestId("supervisor-action").textContent).toContain("Ledger · “run tests”");
  });
  it("shows a lane read error on a running hub instead of claiming there are no lanes", () => {
    const snap: SupervisorSnapshot = { hub: { available: true, running: true, error: "database is locked" }, lanes: [], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={snap} now={now} />);
    expect(screen.getByTestId("supervisor-panel-error").textContent).toContain("database is locked");
    expect(screen.queryByText(/No lanes yet/)).toBeNull();
  });
});
