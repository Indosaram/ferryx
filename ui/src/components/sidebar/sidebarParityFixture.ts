// One desktop sidebar state that the desktop and the remote harnesses both render, so their
// captures can be compared row for row. Data only: each harness turns it into its own inputs.
import type { ActivitySummary } from "../../lib/activity";
import type { HostEndpoint } from "../../state/remoteHostStore";
import type { RegisteredProject } from "../../lib/tauri";
import type { Worktree } from "../../lib/types";
import type { WorktreeOrder } from "../../lib/sidebarPersistence";
import type { SidebarHost } from "./sidebarLabels";

const FERRYX = "/fixture/code/ferryx";
const FERRYX_SSH = "/home/fixture/ferryx";
const RELAY = "/home/fixture/relay";
const SITE = "/fixture/code/site";
const API_ID = `daemon:${"d".repeat(64)}`;
const DOCS_ID = `daemon:${"e".repeat(64)}`;

const row = (path: string, branch: string, extra: Partial<Worktree> = {}): Worktree => ({
  path, head: "a1b2c3d", branch, bare: false, detached: false, locked: null, prunable: null, ...extra,
});

export type SidebarParityFixture = {
  readonly activeProjectId: string;
  /** The row both harnesses mark active. */
  readonly active: { readonly workspaceId: string; readonly worktreeSlug: string; readonly path: string };
  readonly projects: RegisteredProject[];
  /** The active project's worktrees. */
  readonly worktrees: Worktree[];
  readonly inactiveProjectWorktrees: Record<string, Worktree[]>;
  readonly sshHosts: readonly SidebarHost[];
  readonly pairedHosts: Readonly<Record<string, HostEndpoint>>;
  readonly listingFailures: Readonly<Record<string, string>>;
  readonly collapsedGroupIds: readonly string[];
  readonly worktreeOrder: WorktreeOrder;
  /** One running agent as the desktop sees it (by worktree path). */
  readonly desktopActivity: Readonly<Record<string, ActivitySummary>>;
  /** The hub supervisor lane typing into one worktree right now (relative to the render clock). */
  readonly supervisorLane: { readonly sessionId: string; readonly cwd: string };
  /** The same agent as the remote session inventory reports it. */
  readonly remoteActivity: readonly { readonly workspaceId: string; readonly worktreeSlug: string | null; readonly activityState: "working" | "waiting" | "done" }[];
};

/**
 * A local + SSH member of one repo with a custom row order, a standalone SSH project whose
 * listing fails, a collapsed local project with a waiting agent, an offline and an online paired
 * project, an unseen finished agent, and the hub supervisor typing into the active row.
 */
export const sidebarParityFixture: SidebarParityFixture = {
  activeProjectId: "ferryx",
  active: { workspaceId: "ferryx", worktreeSlug: "remote-settings", path: `${FERRYX}/.orca-worktrees/ferryx/remote-settings` },
  projects: [
    { workspaceId: "ferryx", repoRoot: FERRYX, gitRoot: FERRYX, gitRemote: "git@github.com:ferryx/ferryx.git", gitBranch: "main", gitHead: "a1b2c3d", target: { kind: "local" } },
    { workspaceId: "ssh:omarchy:ferryx", repoRoot: FERRYX_SSH, gitRoot: FERRYX_SSH, gitRemote: "https://github.com/ferryx/ferryx.git", gitBranch: "main", gitHead: "b2c3d4e", hostLabel: "omarchy", target: { kind: "ssh", hostId: "omarchy" } },
    { workspaceId: "ssh:omarchy:relay", repoRoot: RELAY, gitRoot: RELAY, gitRemote: "git@github.com:ferryx/relay.git", gitBranch: "main", gitHead: "c3d4e5f", hostLabel: "omarchy", target: { kind: "ssh", hostId: "omarchy" } },
    { workspaceId: "site", repoRoot: SITE, gitRoot: SITE, gitRemote: "git@github.com:ferryx/site.git", gitBranch: "main", gitHead: "d4e5f6a", target: { kind: "local" } },
    { workspaceId: API_ID, repoRoot: "/Users/fixture/code/api", gitRoot: "/Users/fixture/code/api", remoteWorkspaceId: "api", target: { kind: "pairedDaemon", hostId: "studio" } },
    { workspaceId: DOCS_ID, repoRoot: "/Users/fixture/code/docs", gitRoot: "/Users/fixture/code/docs", remoteWorkspaceId: "docs", target: { kind: "pairedDaemon", hostId: "laptop" } },
  ],
  worktrees: [
    row(FERRYX, "main"),
    row(`${FERRYX}/.orca-worktrees/ferryx/remote-settings`, "orca/ferryx/remote-settings"),
    row(`${FERRYX}/.orca-worktrees/ferryx/sidebar-polish`, "orca/ferryx/sidebar-polish"),
  ],
  inactiveProjectWorktrees: {
    "ssh:omarchy:ferryx": [row(FERRYX_SSH, "main", { workspaceId: "ssh:omarchy:ferryx", hostLabel: "omarchy" })],
    "ssh:omarchy:relay": [row(RELAY, "main", { workspaceId: "ssh:omarchy:relay", hostLabel: "omarchy" })],
    site: [row(SITE, "main"), row(`${SITE}/.orca-worktrees/site/launch-page`, "orca/site/launch-page")],
  },
  sshHosts: [{ id: "omarchy", label: "omarchy" }],
  pairedHosts: {
    studio: {
      hostId: "studio", name: "Studio Mac", displayName: "Studio Mac", address: "", transport: "relay",
      online: false, authStatus: "paired", grantScope: "machine", generation: "1", machineId: "machine-studio",
    },
    laptop: {
      hostId: "laptop", name: "Laptop", displayName: "Laptop", address: "", transport: "relay",
      online: true, authStatus: "paired", grantScope: "machine", generation: "1", machineId: "machine-laptop",
    },
  },
  listingFailures: { "ssh:omarchy:relay": "Connection reset by peer" },
  collapsedGroupIds: ["site"],
  // Custom order: the dragged sidebar-polish row and the SSH root sit above the local root.
  worktreeOrder: {
    ferryx: [
      `${FERRYX}/.orca-worktrees/ferryx/sidebar-polish`,
      FERRYX_SSH,
      FERRYX,
      `${FERRYX}/.orca-worktrees/ferryx/remote-settings`,
    ],
  },
  desktopActivity: {
    [`${FERRYX}/.orca-worktrees/ferryx/sidebar-polish`]: {
      workingCount: 1, waitingCount: 0, doneCount: 0, runningCount: 1,
      hasWorking: true, hasWaiting: false, hasDone: false, hasUnread: false,
    },
    // A finished agent the user has not looked at yet.
    [FERRYX]: {
      workingCount: 0, waitingCount: 0, doneCount: 1, runningCount: 0,
      hasWorking: false, hasWaiting: false, hasDone: true, hasUnread: false,
    },
    // An agent waiting for input inside a collapsed project: the project slot and footer show it.
    [SITE]: {
      workingCount: 0, waitingCount: 1, doneCount: 0, runningCount: 0,
      hasWorking: false, hasWaiting: true, hasDone: false, hasUnread: false,
    },
  },
  supervisorLane: { sessionId: "hub-lane-1", cwd: `${FERRYX}/.orca-worktrees/ferryx/remote-settings` },
  remoteActivity: [
    { workspaceId: "ferryx", worktreeSlug: "sidebar-polish", activityState: "working" },
    { workspaceId: "ferryx", worktreeSlug: null, activityState: "done" },
    { workspaceId: "site", worktreeSlug: null, activityState: "waiting" },
  ],
};
