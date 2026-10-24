// One desktop sidebar state that the desktop and the remote harnesses both render, so their
// captures can be compared row for row. Data only: each harness turns it into its own inputs.
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
};

/**
 * A local + SSH member of one repo with a custom row order, a standalone SSH project whose
 * listing fails, a collapsed local project, and an offline paired project.
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
};
