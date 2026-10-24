import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  setSupervisorSnapshotForTest,
  SUPERVISOR_STOP_REASON_LABELS,
  SUPERVISOR_STOP_REASONS,
  useSupervisor,
  type SupervisorGoalView,
  type SupervisorSnapshot,
} from "../../lib/supervisor";
import { SupervisorPaneBarView } from "./SupervisorPaneBar";
import { SupervisorPanelView } from "./SupervisorPanel";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: ipc.invoke }));
const toasts = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock("sonner", () => ({ toast: toasts }));

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/repo/main", goal: "Get tests green", paused: false, lastActivityAt: "" };
const now = Date.UTC(2026, 9, 23, 12, 0, 0);

afterEach(cleanup);
beforeEach(() => {
  ipc.invoke.mockReset();
  ipc.invoke.mockResolvedValue(undefined);
  toasts.error.mockReset();
});

function goal(overrides: Partial<SupervisorGoalView> = {}): SupervisorGoalView {
  return {
    mode: "supervised",
    turnsUsed: 3,
    turnLimit: 12,
    revision: 1,
    requirements: [
      { id: "r1", text: "cargo test passes", verifier: "cargo test", status: "met", evidenceRefs: ["e1"] },
      { id: "r2", text: "clippy is clean", verifier: "cargo clippy", status: "unmet", evidenceRefs: [] },
      { id: "r3", text: "docs updated", verifier: "review", status: "pending", evidenceRefs: [] },
      { id: "r4", text: "bench numbers", verifier: "owner", status: "waived", evidenceRefs: [] },
    ],
    stopReason: null,
    proposal: null,
    pendingRevisions: [],
    ...overrides,
  };
}

function goalCalls() {
  return ipc.invoke.mock.calls.filter(([command]) => command === "cmd_supervisor_goal");
}

function deferred() {
  let reject!: (error: unknown) => void;
  let resolve!: (value: unknown) => void;
  const promise = new Promise((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

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
  it("renders a contract-less lane exactly as before", () => {
    const plain = render(<SupervisorPaneBarView state={{ kind: "left-to-you", lane }} now={now} />).container.innerHTML;
    cleanup();
    const nulled = render(<SupervisorPaneBarView state={{ kind: "left-to-you", lane: { ...lane, goalContract: null } }} now={now} />).container.innerHTML;
    expect(nulled).toBe(plain);
    expect(screen.getByText("The supervisor would not answer this one; it is in your Inbox")).toBeTruthy();
    expect(screen.queryByTestId("supervisor-stop-reason")).toBeNull();
  });
  it("shows the specific stop reason instead of the generic hand-off text", () => {
    for (const reason of SUPERVISOR_STOP_REASONS) {
      render(<SupervisorPaneBarView state={{ kind: "left-to-you", lane: { ...lane, goalContract: goal({ stopReason: reason }) } }} now={now} />);
      expect(screen.getByTestId("supervisor-stop-reason").textContent).toBe(SUPERVISOR_STOP_REASON_LABELS[reason]);
      expect(screen.queryByText(/would not answer this one/)).toBeNull();
      cleanup();
    }
    expect(SUPERVISOR_STOP_REASON_LABELS.delivery_uncertain).toBe("Instruction may not have arrived");
  });
  it("approves an awaiting next step through cmd_supervisor_goal once", async () => {
    const pending = deferred();
    ipc.invoke.mockImplementation((command: string) => (command === "cmd_supervisor_goal" ? pending.promise : Promise.resolve(undefined)));
    const awaiting = { ...lane, goalContract: goal({ proposal: { text: "run cargo clippy and fix warnings", state: "awaiting_approval", gapIds: ["r2"] } }) };
    render(<SupervisorPaneBarView state={{ kind: "awaiting-approval", lane: awaiting }} now={now} />);
    expect(screen.getByTestId("supervisor-pane-bar").dataset.supervision).toBe("awaiting-approval");
    expect(screen.getByText("Next step ready")).toBeTruthy();
    const approve = screen.getByRole("button", { name: /Approve/ });
    const discard = screen.getByRole("button", { name: /Discard/ });
    fireEvent.click(approve);
    expect(goalCalls()).toEqual([["cmd_supervisor_goal", { request: { action: "approve", laneKey: "ledger" } }]]);
    expect((approve as HTMLButtonElement).disabled).toBe(true);
    expect((discard as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(approve);
    expect(goalCalls()).toHaveLength(1);
    await act(async () => {
      pending.resolve(null);
      await pending.promise;
    });
  });
  it("reports an approve failure and re-enables the buttons", async () => {
    const pending = deferred();
    ipc.invoke.mockImplementation((command: string) => (command === "cmd_supervisor_goal" ? pending.promise : Promise.resolve(undefined)));
    const awaiting = { ...lane, goalContract: goal({ proposal: { text: "next", state: "awaiting_approval", gapIds: [] } }) };
    render(<SupervisorPaneBarView state={{ kind: "awaiting-approval", lane: awaiting }} now={now} />);
    const approve = screen.getByRole("button", { name: /Approve/ }) as HTMLButtonElement;
    // Resolves when the failure toast fires; the buttons re-enable in the same synchronous step.
    const toastShown = new Promise<void>((resolve) => toasts.error.mockImplementationOnce(() => resolve()));
    fireEvent.click(approve);
    expect(approve.disabled).toBe(true);
    await act(async () => {
      pending.reject(new Error("REQUEST_CONFLICT"));
      await toastShown;
    });
    expect(toasts.error).toHaveBeenCalledWith("Could not approve the next step", { description: "REQUEST_CONFLICT" });
    expect(screen.getByTestId("supervisor-pane-bar-error")).toBeTruthy();
    expect(approve.disabled).toBe(false);
    expect((screen.getByRole("button", { name: /Discard/ }) as HTMLButtonElement).disabled).toBe(false);
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
  it("shows no contract view for a lane without a goal contract", () => {
    const snap: SupervisorSnapshot = { hub: { available: true, running: true }, lanes: [{ ...lane, goalContract: null }], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={snap} now={now} />);
    expect(screen.getByText("Get tests green")).toBeTruthy();
    expect(screen.queryByTestId("supervisor-lane-goal")).toBeNull();
  });
  it("shows requirements, turns, mode, the proposal and owner revisions for a contract lane", async () => {
    const contract = goal({
      proposal: { text: "run cargo clippy", state: "awaiting_approval", gapIds: ["r2"] },
      pendingRevisions: [{ id: 7, change: { add: "bench numbers" }, instructionQuote: "also add bench numbers" }],
    });
    const snap: SupervisorSnapshot = { hub: { available: true, running: true }, lanes: [{ ...lane, goalContract: contract }], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={snap} now={now} />);
    const chips = screen.getAllByTestId("supervisor-requirement-status").map((chip) => chip.dataset.status);
    expect(chips).toEqual(["met", "unmet", "pending", "waived"]);
    expect(screen.getByTestId("supervisor-goal-turns").textContent).toBe("3 / 12 turns");
    expect(screen.getByRole("tab", { name: "Supervised" }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByTestId("supervisor-goal-proposal").textContent).toContain("run cargo clippy");
    expect(screen.getByRole("button", { name: "Approve" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Discard" })).toBeTruthy();
    expect(screen.getByTestId("supervisor-goal-revision").textContent).toContain("also add bench numbers");
    expect(screen.getByRole("button", { name: "Reject" })).toBeTruthy();

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Accept" }));
    });
    expect(goalCalls()).toEqual([["cmd_supervisor_goal", { request: { action: "acceptRevision", id: 7 } }]]);
  });
  it("changes the lane mode through cmd_supervisor_goal", () => {
    const snap: SupervisorSnapshot = { hub: { available: true, running: true }, lanes: [{ ...lane, goalContract: goal() }], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={snap} now={now} />);
    act(() => {
      fireEvent.click(screen.getByRole("tab", { name: "Full" }));
    });
    expect(goalCalls()).toEqual([["cmd_supervisor_goal", { request: { action: "mode", laneKey: "ledger", mode: "full" } }]]);
  });
});
