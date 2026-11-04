// The supervisor lease on the remote panel: which machine is the one account supervisor host,
// and the standby cards and lease warnings shown above the shared PanelSurface.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SupervisorHubState, SupervisorSnapshot } from "../../lib/supervisorModel";
import { normalizeUnified, type SupervisorMoveResult } from "../../lib/unifiedSupervisor";
import type { SupervisorMachineView } from "../home/phoneHomeModel";
import { accountHost, newHostMemory, panelClaimants, pendingHint, type PhoneSupervisor } from "../home/usePhoneSupervisor";
import type { AdoptArchivedResult } from "../remoteSupervisor";
import { supervisorLeaseCards } from "./leaseCards";
import { pickSetupHost } from "./panelHost";

afterEach(cleanup);

function snapshot(overrides: Partial<SupervisorSnapshot> = {}): SupervisorSnapshot {
  return { hub: { available: true, running: true }, lanes: [], watched: [], actions: [], ...overrides };
}

function machine(overrides: Partial<SupervisorMachineView> & Pick<SupervisorMachineView, "machineId">): SupervisorMachineView {
  return { label: overrides.machineId, state: "ok", snapshot: snapshot(), error: null, ...overrides };
}

const unifiedWire = {
  identity: { supervisorId: "s-1", ownerPrincipal: "desktop:qa-owner" },
  projects: [{ projectId: "p-1", name: "qa-supervisor" }],
  goals: [{ goalId: "g-1", supervisorId: "s-1", projectId: "p-1", laneKey: "sv-1", title: "Write result.txt", runState: "running",
    stateDetail: null, revision: 2, pendingWaits: [], nextTrigger: null, updatedAt: "", mode: "full", turnLimit: 20, turnsUsed: 1, grant: ["edit"] }],
  health: { state: "ready", generation: 1, pid: 3, capabilities: [], serverVersion: null, fault: null, detail: null, queuedDecisions: 0 },
};
const live = normalizeUnified(unifiedWire)!;
const archived = normalizeUnified({ ...unifiedWire, identity: { supervisorId: "s-old", ownerPrincipal: "desktop:qa-owner" } })!;

const standbyHub = (over: Partial<SupervisorHubState> = {}) =>
  snapshot({ hub: { available: true, running: false, role: "standby", holderMachineId: "studio", holderLabel: "Studio", ...over } });

function renderLease(
  machines: SupervisorMachineView[],
  over: Partial<Pick<PhoneSupervisor, "moveHere" | "adoptArchived" | "host">> = {},
) {
  const supervisor = {
    machines,
    host: null,
    moveHere: vi.fn(async (): Promise<SupervisorMoveResult> => ({ kind: "ok", moved: true })),
    adoptArchived: vi.fn(async (): Promise<AdoptArchivedResult> => ({ kind: "ok", adopted: 1 })),
    ...over,
  };
  const node = supervisorLeaseCards(supervisor, true);
  render(<>{node}</>);
  return { supervisor, node };
}

describe("the account supervisor host", () => {
  it("picks the lease holder as the one account supervisor, not a standby's own hub", () => {
    const memory = newHostMemory();
    const machines = [
      machine({ machineId: "laptop", label: "Laptop", snapshot: snapshot({ hub: { available: true, running: false, role: "standby", holderMachineId: "studio", holderLabel: "Studio" }, supervisor: archived }) }),
      machine({ machineId: "studio", label: "Studio", snapshot: snapshot({ hub: { available: true, running: true, role: "holder" }, supervisor: live }) }),
    ];
    const host = accountHost(machines, memory, () => null);
    expect(host).toMatchObject({ machineId: "studio", label: "Studio", connection: "live" });
    expect(host!.view.identity?.supervisorId).toBe("s-1");
    // The panel pins the holder alone: the standby's archived supervisor never claims it.
    expect(panelClaimants(machines, host).map((claimant) => claimant.machineId)).toEqual(["studio"]);
    // The holder drops off: the last known holder stays, disconnected; the standby never replaces it.
    const gone = [machines[0]!, machine({ machineId: "studio", label: "Studio", state: "offline", snapshot: null })];
    expect(accountHost(gone, memory, () => null)).toMatchObject({ machineId: "studio", connection: "disconnected" });
  });

  it("lets every older daemon naming a supervisor claim the panel, so two of them pin nothing", () => {
    const machines = [
      machine({ machineId: "studio", snapshot: snapshot({ supervisor: live }) }),
      machine({ machineId: "laptop", snapshot: snapshot({ supervisor: live }) }),
    ];
    const host = accountHost(machines, newHostMemory(), () => null);
    expect(panelClaimants(machines, host).map((claimant) => claimant.machineId).sort()).toEqual(["laptop", "studio"]);
    expect(panelClaimants(machines, null)).toEqual([]);
  });

  it("follows the holder a SUPERVISOR_STANDBY refusal named", () => {
    const memory = newHostMemory();
    const machines = [
      machine({ machineId: "laptop", snapshot: snapshot({ hub: { available: true, running: true, role: "holder" }, supervisor: live }) }),
      machine({ machineId: "studio", label: "Studio", snapshot: snapshot({ hub: { available: true, running: true }, supervisor: live }) }),
    ];
    expect(accountHost(machines, memory, () => null)?.machineId).toBe("laptop");
    const stale = [machine({ machineId: "laptop", snapshot: snapshot({ hub: { available: true, running: true }, supervisor: live }) }), machines[1]!];
    expect(accountHost(stale, newHostMemory(), () => null)?.machineId).toBe("laptop");
    expect(accountHost(stale, memory, () => null, "studio")?.machineId).toBe("studio");
  });

  it("never takes a standby's archived supervisor as the account view, even when a hint or the last holder names it", () => {
    const machines = [machine({ machineId: "laptop", snapshot: snapshot({ hub: { available: true, running: false, role: "standby" }, supervisor: archived }) })];
    const hinted = newHostMemory();
    expect(accountHost(machines, hinted, () => null, "laptop")).toBeNull();
    expect(hinted.lastSeen.has("laptop")).toBe(false);
    const remembered = newHostMemory();
    remembered.lastHolder = "laptop";
    expect(accountHost(machines, remembered, () => null)).toBeNull();
  });

  it("drops the holder hint once any snapshot reports a holder", () => {
    const standbyOnly = [machine({ machineId: "laptop", snapshot: snapshot({ hub: { available: true, running: false, role: "standby" } }) })];
    expect(pendingHint(standbyOnly, "studio")).toBe("studio");
    expect(pendingHint([], "studio")).toBe("studio");
    const holder = [...standbyOnly, machine({ machineId: "desk", snapshot: snapshot({ hub: { available: true, running: true, role: "holder" } }) })];
    expect(pendingHint(holder, "studio")).toBeNull();
    const provisional = [machine({ machineId: "desk", snapshot: snapshot({ hub: { available: true, running: true, role: "provisional" } }) })];
    expect(pendingHint(provisional, "studio")).toBeNull();
  });

  it("never turns a standby on: setup goes to a stopped computer that is not on standby", () => {
    const stoppedStandby = machine({ machineId: "laptop", snapshot: standbyHub() });
    expect(pickSetupHost(null, [stoppedStandby])).toEqual({ kind: "none" });
    const stopped = machine({ machineId: "desk", label: "Desk", snapshot: snapshot({ hub: { available: true, running: false } }) });
    expect(pickSetupHost(null, [stoppedStandby, stopped])).toEqual({ kind: "host", machineId: "desk", label: "Desk" });
  });
});

describe("supervisor lease cards", () => {
  it("shows nothing when no computer is on standby or has a lease warning", () => {
    const { node } = renderLease([machine({ machineId: "studio", snapshot: snapshot({ hub: { available: true, running: true, role: "holder" } }) })]);
    expect(node).toBeNull();
  });

  it("shows standby with Move supervisor here for that computer, never Turn on", async () => {
    const { supervisor } = renderLease([machine({ machineId: "laptop", label: "Laptop", snapshot: standbyHub() })]);
    expect(screen.getByTestId("supervisor-standby-text")).toHaveTextContent("Supervisor runs on Studio. Laptop is on standby.");
    expect(screen.queryByRole("button", { name: "Turn on" })).toBeNull();
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-move-here")));
    expect(supervisor.moveHere).toHaveBeenCalledWith("laptop", false);
    expect(supervisor.moveHere).toHaveBeenCalledTimes(1);
  });

  it("offers no move where the standby machine has no hub", () => {
    renderLease([machine({ machineId: "laptop", snapshot: standbyHub({ available: false }) })]);
    expect(screen.getByTestId("supervisor-standby-text")).toHaveTextContent("Supervisor runs on Studio");
    expect(screen.queryByTestId("supervisor-move-here")).toBeNull();
  });

  it("needs a second confirm for a forced move and shows the data-loss window", async () => {
    const checkpointAt = new Date(2026, 9, 31, 14, 5).getTime();
    const moveHere = vi.fn(async (_machineId: string, force: boolean): Promise<SupervisorMoveResult> =>
      force ? { kind: "ok", moved: true } : { kind: "force-required", holderLabel: "Studio", checkpointAt, message: "holder unreachable" });
    renderLease([machine({ machineId: "laptop", snapshot: standbyHub() })], { moveHere });
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-move-here")));
    expect(moveHere).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("supervisor-move-force-warning")).toHaveTextContent("Changes since 14:05 on Studio will be lost.");
    expect(screen.queryByTestId("supervisor-move-here")).toBeNull();
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-move-force-confirm")));
    expect(moveHere).toHaveBeenLastCalledWith("laptop", true);
    expect(screen.queryByTestId("supervisor-move-force")).toBeNull();
  });

  it("cancelling a forced move sends nothing", async () => {
    const moveHere = vi.fn(async (): Promise<SupervisorMoveResult> => ({ kind: "force-required", holderLabel: null, checkpointAt: null, message: "" }));
    renderLease([machine({ machineId: "laptop", snapshot: standbyHub() })], { moveHere });
    await act(async () => fireEvent.click(screen.getByTestId("supervisor-move-here")));
    expect(screen.getByTestId("supervisor-move-force-warning")).toHaveTextContent("Changes on Studio since its last checkpoint will be lost.");
    fireEvent.click(screen.getByTestId("supervisor-move-force-cancel"));
    expect(moveHere).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("supervisor-move-here")).toBeEnabled();
  });

  it("lists archived goals as adoptable and adopts one on request", async () => {
    const snap = standbyHub({ adoptable: [{ goalId: "old-1", title: "Fix the flaky test", supervisorId: "s-old", fromMachine: "Laptop" }] });
    const { supervisor } = renderLease([machine({ machineId: "laptop", label: "Laptop", snapshot: snap })]);
    const goal = screen.getByTestId("supervisor-adoptable-goal");
    expect(goal).toHaveTextContent("Fix the flaky test");
    expect(goal).toHaveTextContent("Adoptable from Laptop");
    await act(async () => fireEvent.click(within(goal).getByRole("button", { name: "Adopt Fix the flaky test" })));
    expect(supervisor.adoptArchived).toHaveBeenCalledWith("laptop", ["old-1"]);
  });

  it("says when the standby is not paired to the holder and when goals name an unmapped host", () => {
    renderLease([machine({ machineId: "laptop", label: "Laptop", snapshot: standbyHub({ holderPaired: false, unmappedHosts: ["host-9"] }) })]);
    expect(screen.getByTestId("supervisor-standby-unpaired")).toHaveTextContent("Laptop is not paired to Studio");
    expect(screen.getByTestId("supervisor-standby-unmapped")).toHaveTextContent("host-9");
  });

  it("warns about dropped observations and a blocked takeover on a standby", () => {
    const snap = standbyHub({ observeDropped: 2, promotionBlockReason: "this machine has no checkpoint; the lease records cp-9" });
    renderLease([machine({ machineId: "laptop", label: "Laptop", snapshot: snap })]);
    const notices = screen.getAllByTestId("supervisor-lease-notice");
    expect(notices.map((notice) => notice.dataset.kind)).toEqual(["observe-dropped", "promotion-blocked"]);
    expect(notices[0]).toHaveTextContent("Laptop dropped 2 pane updates");
    expect(notices[1]).toHaveTextContent("this machine has no checkpoint; the lease records cp-9");
  });

  it("warns on the holder about hosts its fence cannot reach and offers no move there", () => {
    const snap = snapshot({ hub: { available: true, running: true, role: "holder", unreachableHosts: ["Laptop"], observeDropped: 0 } });
    renderLease([machine({ machineId: "studio", label: "Studio", snapshot: snap })]);
    const notice = screen.getByTestId("supervisor-lease-notice");
    expect(notice.dataset.kind).toBe("unreachable-hosts");
    expect(notice).toHaveTextContent("The supervisor cannot reach Laptop.");
    expect(screen.queryByTestId("supervisor-standby")).toBeNull();
    expect(screen.queryByTestId("supervisor-move-here")).toBeNull();
  });
});
