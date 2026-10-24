import { describe, expect, it } from "vitest";

import type { ActivitySummary } from "../../lib/activity";
import type { PairedHostReachabilityView } from "../../lib/pairedHostReachability";
import type { RegisteredProject } from "../../lib/tauri";
import type { Worktree } from "../../lib/types";
import type { RemoteHostState } from "../../state/remoteHostStore";
import { buildSidebarModel, filterSidebarModel, sidebarLiveState, type SidebarModelInput } from "./sidebarModel";
import { initialCollapsedProjects } from "./sidebarRows";

const REMOTE = "git@github.com:indo/ferryx.git";
const local: RegisteredProject = { workspaceId: "ferryx", repoRoot: "/code/ferryx", gitRoot: "/code/ferryx", gitRemote: REMOTE };
const ssh: RegisteredProject = {
  workspaceId: "ssh:ferryx", repoRoot: "/srv/ferryx", gitRoot: "/srv/ferryx",
  gitRemote: "https://github.com/indo/ferryx", target: { kind: "ssh", hostId: "box" },
};
const paired: RegisteredProject = {
  workspaceId: "daemon:site", repoRoot: "/work/site", gitRoot: "/work/site", gitRemote: "git@github.com:indo/site.git",
  target: { kind: "pairedDaemon", hostId: "studio" }, remoteWorkspaceId: "site",
};

const row = (path: string, branch: string | null, extra: Partial<Worktree> = {}): Worktree => ({
  path, head: "abc", branch, bare: false, detached: false, locked: null, prunable: null, ...extra,
});

const offlineHosts: RemoteHostState = {
  hosts: { studio: { hostId: "studio", name: "Studio", address: "", transport: "relay", online: false, authStatus: "paired", grantScope: "machine", machineId: "m-studio" } },
  activeHostId: null, discovering: false, nativeStatus: "ready", machineFeaturesEnabled: true,
};
const unsupported: PairedHostReachabilityView = { supported: false, entry: null };

function fixture(overrides: Partial<SidebarModelInput> = {}): SidebarModelInput {
  return {
    source: "desktop",
    generatedAt: "2026-10-24T00:00:00.000Z",
    projects: [local, ssh, paired],
    activeProjectId: "ferryx",
    worktrees: [row("/code/ferryx", "main"), row("/code/ferryx/.orca-worktrees/relay", "orca/ferryx/relay-latency")],
    inactiveProjectWorktrees: { "daemon:site": [row("/work/site", "main")] },
    hosts: [{ id: "box", label: "Build box" }],
    remoteState: offlineHosts,
    reachability: () => unsupported,
    worktreeOrder: {},
    collapsedGroupIds: new Set(),
    advancedFeatures: true,
    listingFailures: {},
    ...overrides,
  };
}

const activity = (state: "working" | "waiting"): ActivitySummary => ({
  workingCount: state === "working" ? 1 : 0, waitingCount: state === "waiting" ? 1 : 0, doneCount: 0,
  runningCount: state === "working" ? 1 : 0, hasWorking: state === "working", hasWaiting: state === "waiting",
  hasDone: false, hasUnread: false,
});

describe("buildSidebarModel", () => {
  it("merges an SSH checkout of the same repository into the local project's group", () => {
    const model = buildSidebarModel(fixture());

    const ferryx = model.projects.find((node) => node.groupId === "ferryx");
    expect(model.projects.map((node) => node.groupId)).toEqual(["ferryx", "daemon:site"]);
    expect(ferryx?.members).toEqual([
      { workspaceId: "ferryx", kind: "local", machineId: null },
      { workspaceId: "ssh:ferryx", kind: "ssh", machineId: null },
    ]);
    expect(ferryx?.worktrees.map((node) => [node.label, node.hostLabel])).toEqual([
      ["main", null], ["relay-latency", null], ["ferryx", "Build box"],
    ]);
  });

  it("labels a lone paired project with its machine and reports the machine state", () => {
    const site = buildSidebarModel(fixture()).projects.find((node) => node.groupId === "daemon:site");

    expect(site).toMatchObject({
      label: "site (Studio)",
      hostLabel: "Studio",
      members: [{ workspaceId: "daemon:site", kind: "paired", machineId: "m-studio" }],
      status: { kind: "offline", label: "Offline" },
    });
    expect(site?.worktrees).toEqual([expect.objectContaining({ workspaceId: "daemon:site", hostLabel: "Studio", label: "main" })]);
  });

  it("keeps the desktop project order and the saved worktree order", () => {
    const flipped = buildSidebarModel(fixture({
      projects: [paired, local, ssh],
      worktreeOrder: { ferryx: ["/code/ferryx/.orca-worktrees/relay"] },
    }));

    expect(flipped.projects.map((node) => node.groupId)).toEqual(["daemon:site", "ferryx"]);
    expect(flipped.projects[1]?.worktrees.map((node) => node.label)).toEqual(["relay-latency", "main", "ferryx"]);
  });

  it("reports a listing failure as the project status and the empty hint for a project without rows", () => {
    const model = buildSidebarModel(fixture({
      projects: [paired], activeProjectId: undefined, inactiveProjectWorktrees: {}, listingFailures: { "daemon:site": "reset by peer" },
    }));

    expect(model.projects[0]?.status).toEqual({ kind: "failure", label: "Unavailable" });
    expect(buildSidebarModel(fixture({ projects: [] }))).toMatchObject({ projects: [], emptyState: "noProjects" });
  });

  it("publishes only collapsed ids of groups that exist, in render order", () => {
    const collapsed = initialCollapsedProjects({
      stored: null, projects: [local, ssh, paired], activeProjectId: "ferryx", emptyGroupIds: new Set(),
    });

    const model = buildSidebarModel(fixture({ collapsedGroupIds: new Set([...collapsed, "gone"]) }));

    expect(model.collapsedGroupIds).toEqual(["daemon:site"]);
  });

  it("survives the gateway JSON round trip unchanged", () => {
    const model = buildSidebarModel(fixture());

    expect(JSON.parse(JSON.stringify(model))).toEqual(model);
  });
});

describe("initialCollapsedProjects", () => {
  it("collapses every group but the active one on first run, and empty groups always", () => {
    const seeded = initialCollapsedProjects({
      stored: null, projects: [local, ssh, paired], activeProjectId: "ssh:ferryx", emptyGroupIds: new Set(["x"]),
    });

    expect([...seeded].sort()).toEqual(["daemon:site", "x"]);
  });

  it("treats a persisted set as authoritative", () => {
    const restored = initialCollapsedProjects({
      stored: ["ferryx"], projects: [local, ssh, paired], activeProjectId: "ferryx", emptyGroupIds: new Set(),
    });

    expect([...restored]).toEqual(["ferryx"]);
  });
});

describe("filterSidebarModel", () => {
  it("keeps only matching rows and forces their project open", () => {
    const visible = filterSidebarModel(buildSidebarModel(fixture()), "RELAY");

    expect(visible.map((entry) => [entry.group.groupId, entry.forceExpanded, entry.rows.map((r) => r.label)]))
      .toEqual([["ferryx", true, ["relay-latency"]]]);
  });

  it("keeps every row of a project whose host label matches", () => {
    const visible = filterSidebarModel(buildSidebarModel(fixture()), "studio");

    expect(visible.map((entry) => [entry.group.groupId, entry.forceExpanded, entry.rows.length])).toEqual([["daemon:site", false, 1]]);
  });
});

describe("sidebarLiveState", () => {
  it("counts running and needs-input rows once each and ignores paired rows in the footer", () => {
    const model = buildSidebarModel(fixture());

    const live = sidebarLiveState(model, {
      activityByWorktreePath: {
        "/code/ferryx": activity("working"),
        "/code/ferryx/.orca-worktrees/relay": activity("waiting"),
        "/work/site": activity("working"),
      },
      activeWorkspaceId: "ferryx",
      activePath: "/code/ferryx",
    });

    expect(live.footer).toBe("1 running · 1 needs input");
    expect(live.projects.ferryx).toMatchObject({ kind: "attention", state: "waiting" });
    expect(live.projects["daemon:site"]).toMatchObject({ kind: "paired", label: "Offline", connected: false });
  });

  it("marks the active row and lets needs input outrank supervisor typing", () => {
    const model = buildSidebarModel(fixture());
    const [root, relay] = model.projects[0]?.worktrees ?? [];

    const live = sidebarLiveState(model, {
      activityByWorktreePath: { "/code/ferryx/.orca-worktrees/relay": activity("waiting") },
      activeWorkspaceId: "ferryx",
      activePath: "/code/ferryx",
      isSupervisorTyping: () => true,
    });

    expect(root && live.rows[root.key]).toEqual({ active: true, slot: null, supervisorTyping: true });
    expect(relay && live.rows[relay.key]).toEqual({ active: false, slot: "waiting", supervisorTyping: false });
  });
});
