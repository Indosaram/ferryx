// Fixture builders for the phone shell: supervisor lanes and account options. Shared by the
// component tests and the mobile QA harness (no network).
import type { SupervisorLane } from "../../lib/supervisorModel";
import type { AccountWorktreeOption } from "../useAccountWorktrees";

export function fixtureLane(overrides: Partial<SupervisorLane> & Pick<SupervisorLane, "key">): SupervisorLane {
  return {
    title: "Ship the remote inbox",
    // The hub's own vocabulary: `insert_lane` starts at `queued` and `LaneStatus` has no
    // `running`. A fixture default outside that set would let the model pass here while the
    // real phone showed "Status unknown" for every freshly queued lane.
    status: "working",
    sessionId: `session-${overrides.key}`,
    cwd: "/Users/me/code/ferryx",
    goal: "Land the inbox and keep CI green",
    paused: false,
    lastActivityAt: "2026-10-24T09:00:00Z",
    lastReason: null,
    ...overrides,
  };
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
