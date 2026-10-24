// Fixture builders for the phone home: supervisor snapshots, machine views, account options and
// inventories. Shared by the component tests and the mobile QA harness (no network).
import type { SupervisorAction, SupervisorLane, SupervisorSnapshot } from "../../lib/supervisorModel";
import type { RemoteSessionInventory, RemoteSessionInventoryEntry } from "../remoteSessionInventory";
import type { AccountWorktreeOption } from "../useAccountWorktrees";
import type { SupervisorMachineView } from "./phoneHomeModel";

export function fixtureLane(overrides: Partial<SupervisorLane> & Pick<SupervisorLane, "key">): SupervisorLane {
  return {
    title: "Ship the remote inbox",
    status: "running",
    sessionId: `session-${overrides.key}`,
    cwd: "/Users/me/code/ferryx",
    goal: "Land the inbox and keep CI green",
    paused: false,
    lastActivityAt: "2026-10-24T09:00:00Z",
    lastReason: null,
    ...overrides,
  };
}

export function fixtureAction(overrides: Partial<SupervisorAction> & Pick<SupervisorAction, "atMs">): SupervisorAction {
  return {
    deviceId: "hub",
    op: "input",
    sessionId: null,
    requestId: `req-${overrides.atMs}`,
    code: "OK",
    text: "continue",
    ...overrides,
  };
}

export function fixtureSnapshot(overrides: Partial<SupervisorSnapshot> = {}): SupervisorSnapshot {
  return { hub: { available: true, running: true }, lanes: [], watched: [], actions: [], ...overrides };
}

export function fixtureMachine(
  overrides: Partial<SupervisorMachineView> & Pick<SupervisorMachineView, "machineId">,
): SupervisorMachineView {
  return { label: overrides.machineId, state: "ok", snapshot: fixtureSnapshot(), error: null, ...overrides };
}

export function fixtureOption(
  overrides: Partial<AccountWorktreeOption> & Pick<AccountWorktreeOption, "machineId" | "sessionId">,
): AccountWorktreeOption {
  return {
    machineDisplayName: overrides.machineId,
    machinePlatform: "darwin",
    machineOnline: true,
    workspaceId: "ferryx",
    workspaceLabel: "ferryx",
    worktreeSlug: "main",
    worktreeLabel: "main",
    agentType: "claude",
    inventoryActivityState: null,
    ...overrides,
  };
}

export function fixtureInventory(
  machineId: string,
  entries: ReadonlyArray<Partial<RemoteSessionInventoryEntry> & Pick<RemoteSessionInventoryEntry, "sessionId">>,
): RemoteSessionInventory {
  return {
    machineId,
    revision: "1",
    completeness: "complete",
    entries: entries.map((entry) => ({
      machineId,
      workspaceId: "ferryx",
      worktreeSlug: "main",
      worktreeLabel: "main",
      daemonEpoch: null,
      title: null,
      running: true,
      agentType: "claude",
      providerSessionId: null,
      activityState: null,
      ...entry,
    })),
    unavailableWorkspaceIds: [],
    fetchedAt: 0,
  };
}
