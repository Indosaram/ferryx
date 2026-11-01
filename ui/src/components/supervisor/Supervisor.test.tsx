import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  adoptArchivedGoals,
  desktopUnifiedClient,
  moveSupervisorHere,
  normalizeSnapshot,
  setSupervisorSnapshotForTest,
  SUPERVISOR_STOP_REASON_LABELS,
  SUPERVISOR_STOP_REASONS,
  useSupervisor,
  type SupervisorGoalView,
  type SupervisorSnapshot,
} from "../../lib/supervisor";
import { SupervisorPaneBarView } from "./SupervisorPaneBar";
import { adoptableLanes, desktopConnection, SupervisorPanelView } from "./SupervisorPanel";

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

describe("SupervisorPanelView unified supervisor", () => {
  const unifiedGoal = {
    goalId: "g-7", supervisorId: "s-1", projectId: "p-1", laneKey: "sv-7", title: "Write result.txt", runState: "waiting_event",
    stateDetail: "WAITING_ON: 1 pending dependencies", revision: 5,
    pendingWaits: [{ waitId: "w1", kind: "external_event", childGoalId: null, eventKey: "ci:green" }],
    nextTrigger: null, updatedAt: "", mode: "full", turnLimit: 20, turnsUsed: 2, grant: ["edit"],
  };
  const wire = {
    hub: { available: true, running: true },
    lanes: [lane],
    watched: [],
    actions: [],
    supervisor: {
      identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
      projects: [{ projectId: "p-1", name: "qa-supervisor" }],
      goals: [unifiedGoal],
      health: { state: "ready", generation: 1, pid: 9, capabilities: [], serverVersion: null, fault: null, detail: null, queuedDecisions: 0 },
    },
  };

  function client() {
    return {
      request: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
      conversation: vi.fn(async () => ({ kind: "ok" as const, value: { messages: [], next: 0 } })),
      memory: vi.fn(async () => ({ kind: "error" as const, code: "MEMORY_RECEIPTS_UNAVAILABLE", message: "", status: 503 })),
      remember: vi.fn(async () => ({ kind: "ok" as const, value: {} })),
      refresh: vi.fn(),
    };
  }

  it("shows the live goal, its wait and the lanes together; the lane pause still works", async () => {
    const fake = client();
    render(<SupervisorPanelView snapshot={normalizeSnapshot(wire)} now={now} client={fake} />);
    await act(async () => {
      await fake.conversation.mock.results[0]?.value;
      await fake.memory.mock.results[0]?.value;
    });
    expect(screen.getByTestId("supervisor-unified").dataset.connection).toBe("live");
    expect(screen.getByTestId("supervisor-goal-wait").textContent).toContain("ci:green");
    expect(screen.getByTestId("supervisor-goal").dataset.revision).toBe("5");
    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Pause Ledger" }));
    });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_set_paused", { laneKey: "ledger", paused: true });
  });

  it("keeps the last goals when the hub becomes unavailable and never sends", () => {
    const fake = client();
    const last = normalizeSnapshot(wire).supervisor!;
    const off: SupervisorSnapshot = { hub: { available: false, running: false, error: "daemon unreachable" }, lanes: [], watched: [], actions: [] };
    render(<SupervisorPanelView snapshot={off} now={now} lastUnified={last} client={fake} />);
    expect(screen.getByTestId("supervisor-panel").dataset.supervisorState).toBe("off");
    expect(screen.getByTestId("supervisor-connection").dataset.state).toBe("disconnected");
    expect(screen.getByTestId("supervisor-goal-id").textContent).toBe("g-7");
    expect((screen.getByTestId("supervisor-pause") as HTMLButtonElement).disabled).toBe(true);
    expect(fake.conversation).not.toHaveBeenCalled();
    expect(fake.request).not.toHaveBeenCalled();
    // No local fallback: the desktop never asks its daemon to start a supervisor.
    expect(ipc.invoke).not.toHaveBeenCalled();
  });

  it("offers adoption only for legacy lanes with a live goal contract", () => {
    const snapshot = normalizeSnapshot({ ...wire, lanes: [
      { ...lane, key: "ledger", goalContract: goal({ mode: "full" }) },
      { ...lane, key: "notes", title: "Notes", goalContract: goal({ mode: "off" }) },
      { ...lane, key: "sv-7", title: "Supervisor lane", goalContract: goal() },
      { ...lane, key: "bare", title: "Bare" },
    ] });
    expect(adoptableLanes(snapshot, snapshot.supervisor ?? null)).toEqual([{ key: "ledger", title: "Ledger" }]);
    expect(desktopConnection(snapshot)).toBe("live");
    expect(desktopConnection({ ...snapshot, supervisor: null })).toBe("unavailable");
    expect(desktopConnection({ ...snapshot, hub: { ...snapshot.hub, role: "standby", holderLabel: "Studio" } })).toBe("standby");
  });
});

describe("SupervisorPanelView standby", () => {
  const standby = (over: Partial<SupervisorSnapshot["hub"]> = {}): SupervisorSnapshot => ({
    hub: { available: true, running: false, role: "standby", holderMachineId: "studio", holderLabel: "Studio", ...over },
    lanes: [],
    watched: [],
    actions: [],
  });

  it("says where the supervisor runs and moves it here instead of starting one", async () => {
    const moveHere = vi.fn(async () => ({ kind: "ok" as const, moved: true }));
    render(<SupervisorPanelView snapshot={standby()} now={now} moveHere={moveHere} />);
    expect(screen.getByTestId("supervisor-panel").dataset.supervisorState).toBe("standby");
    expect(screen.getByTestId("supervisor-standby-text").textContent).toBe("Supervisor runs on Studio. This computer is on standby.");
    expect(screen.queryByText(/ferryx hub start/)).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move supervisor here" }));
    });
    expect(moveHere).toHaveBeenCalledWith(false);
  });

  it("offers no move where this computer has no hub", () => {
    render(<SupervisorPanelView snapshot={standby({ available: false })} now={now} moveHere={vi.fn()} />);
    expect(screen.getByTestId("supervisor-standby-text")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Move supervisor here" })).toBeNull();
  });

  it("asks again before a forced move, showing what will be lost", async () => {
    const checkpointAt = new Date(2026, 9, 31, 9, 30).getTime();
    const moveHere = vi.fn(async (force: boolean) =>
      force ? { kind: "ok" as const, moved: true } : { kind: "force-required" as const, holderLabel: "Studio", checkpointAt, message: "" });
    render(<SupervisorPanelView snapshot={standby()} now={now} moveHere={moveHere} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move supervisor here" }));
    });
    expect(screen.getByTestId("supervisor-move-force-warning").textContent).toContain("Changes since 09:30 on Studio will be lost.");
    expect(moveHere).toHaveBeenCalledTimes(1);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Move anyway" }));
    });
    expect(moveHere).toHaveBeenLastCalledWith(true);
  });

  it("lists archived goals with Adopt and the pairing and host-mapping notices", async () => {
    const adoptArchived = vi.fn(async () => ({ kind: "ok" as const, value: {} }));
    const snapshot = standby({
      holderPaired: false,
      unmappedHosts: ["host-9"],
      adoptable: [{ goalId: "old-1", title: "Fix the flaky test", supervisorId: "s-old", fromMachine: "Laptop" }],
    });
    render(<SupervisorPanelView snapshot={snapshot} now={now} adoptArchived={adoptArchived} />);
    expect(screen.getByTestId("supervisor-adoptable-goal").textContent).toContain("Adoptable from Laptop");
    expect(screen.getByTestId("supervisor-standby-unpaired").textContent).toContain("not paired to Studio");
    expect(screen.getByTestId("supervisor-standby-unmapped").textContent).toContain("host-9");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Adopt Fix the flaky test" }));
    });
    expect(adoptArchived).toHaveBeenCalledWith(["old-1"]);
  });

  it("moves and adopts through the typed IPC commands and keeps the force payload", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_move_here") {
        throw { code: "SUPERVISOR_FORCE_REQUIRED", message: "holder unreachable", details: { holderLabel: "Studio", checkpointAt: 1_790_000_000, status: 409 } };
      }
      if (command === "cmd_supervisor_adopt_archived") return { adopted: 1 };
      return undefined;
    });
    await expect(moveSupervisorHere(false)).resolves.toEqual({ kind: "force-required", holderLabel: "Studio", checkpointAt: 1_790_000_000_000, message: "holder unreachable" });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_move_here", { force: false });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({ kind: "ok", value: { adopted: 1 } });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_adopt_archived", { goalIds: ["old-1"] });
  });

  it("keeps a structured or plain refusal of the adopt command an error result, never a throw", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_adopt_archived") {
        throw { code: "SUPERVISOR_GOAL_NOT_ADOPTABLE", message: "goal old-1 is already adopted", details: { status: 409 } };
      }
      return undefined;
    });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({
      kind: "error", code: "SUPERVISOR_GOAL_NOT_ADOPTABLE", message: "goal old-1 is already adopted", status: 409,
    });
    // A daemon without the command yet rejects with a bare string.
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_adopt_archived") throw "command cmd_supervisor_adopt_archived not found";
      return undefined;
    });
    await expect(adoptArchivedGoals(["old-1"])).resolves.toEqual({
      kind: "error", code: "INTERNAL_ERROR", message: "command cmd_supervisor_adopt_archived not found", status: null,
    });
  });

  it("shows a structured adopt refusal as a toast and re-enables Adopt", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_adopt_archived") {
        throw { code: "SUPERVISOR_GOAL_NOT_ADOPTABLE", message: "goal old-1 is already adopted", details: { status: 409 } };
      }
      return undefined;
    });
    const snapshot = standby({ adoptable: [{ goalId: "old-1", title: "Fix the flaky test", supervisorId: "s-old", fromMachine: "Laptop" }] });
    render(<SupervisorPanelView snapshot={snapshot} now={now} />);
    const adopt = screen.getByRole("button", { name: "Adopt Fix the flaky test" }) as HTMLButtonElement;
    const toastShown = new Promise<void>((resolve) => toasts.error.mockImplementationOnce(() => resolve()));
    await act(async () => {
      fireEvent.click(adopt);
      await toastShown;
    });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_adopt_archived", { goalIds: ["old-1"] });
    expect(toasts.error).toHaveBeenCalledWith("Could not adopt the goal", { description: "goal old-1 is already adopted" });
    expect(screen.getByTestId("supervisor-adoptable-goal")).toBeTruthy();
    expect(adopt.disabled).toBe(false);
  });

  it("keeps a SUPERVISOR_STANDBY refusal of the move command a plain error by its code", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_move_here") {
        throw { code: "SUPERVISOR_STANDBY", message: "SUPERVISOR_FORCE_REQUIRED is not this", details: { holderMachineId: "studio", holderLabel: "Studio", status: 409 } };
      }
      return undefined;
    });
    await expect(moveSupervisorHere(true)).resolves.toEqual({ kind: "error", code: "SUPERVISOR_STANDBY", message: "SUPERVISOR_FORCE_REQUIRED is not this", status: 409 });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_move_here", { force: true });
  });

  it("shows no lease warnings on a standby whose optional fields are missing", () => {
    render(<SupervisorPanelView snapshot={standby()} now={now} moveHere={vi.fn()} />);
    expect(screen.getByTestId("supervisor-standby-text").textContent).toBe("Supervisor runs on Studio. This computer is on standby.");
    expect(screen.queryByTestId("supervisor-lease-notice")).toBeNull();
  });

  it("warns on a standby about dropped observations and why it will not take over", () => {
    render(<SupervisorPanelView snapshot={standby({ observeDropped: 1, promotionBlockReason: "the hub is stopped on this machine, so it does not take over" })} now={now} moveHere={vi.fn()} />);
    const notices = screen.getAllByTestId("supervisor-lease-notice");
    expect(notices.map((notice) => notice.dataset.kind)).toEqual(["observe-dropped", "promotion-blocked"]);
    expect(notices[0]?.textContent).toBe("This computer dropped 1 pane update because no paired supervisor took it.");
  });

  it("warns on the holder about hosts it cannot reach and stays a live panel", () => {
    const holder: SupervisorSnapshot = {
      hub: { available: true, running: true, role: "holder", holderMachineId: "studio", holderLabel: "Studio", unreachableHosts: ["Laptop", "Desk"] },
      lanes: [lane],
      watched: [],
      actions: [],
    };
    render(<SupervisorPanelView snapshot={holder} now={now} moveHere={vi.fn()} />);
    expect(screen.getByTestId("supervisor-panel").dataset.supervisorState).not.toBe("standby");
    expect(screen.queryByTestId("supervisor-standby")).toBeNull();
    const notice = screen.getByTestId("supervisor-lease-notice");
    expect(notice.dataset.kind).toBe("unreachable-hosts");
    expect(notice.textContent).toBe("The supervisor cannot reach Laptop, Desk. Sessions there wait until it is reachable again.");
  });
});

describe("desktopUnifiedClient", () => {
  it("sends envelopes and reads through the typed IPC commands and keeps refusal codes", async () => {
    ipc.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_supervisor_request") throw { code: "REVISION_CONFLICT", message: "goal g changed", details: { code: "REVISION_CONFLICT", status: 409 } };
      if (command === "cmd_supervisor_read") return { projectId: "p", messages: [], next: 0 };
      return undefined;
    });
    const envelope = { requestId: "r", goalId: "g", expectedRevision: 1, action: "pause", payload: {} };
    await expect(desktopUnifiedClient.request(envelope)).resolves.toEqual({ kind: "error", code: "REVISION_CONFLICT", message: "goal g changed", status: 409 });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_request", { envelope });
    await expect(desktopUnifiedClient.conversation("p", 2)).resolves.toMatchObject({ kind: "ok" });
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "conversation", projectId: "p", after: 2 } });
    await desktopUnifiedClient.memory();
    expect(ipc.invoke).toHaveBeenCalledWith("cmd_supervisor_read", { request: { view: "memory" } });
  });
});
