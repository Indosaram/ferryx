// One desktop state that exercises every sidebar shape both apps must draw the same way: a repo
// with a local and an SSH member, an offline paired project, a project whose listing failed, a
// collapsed group and a custom worktree order. Shared by the desktop/remote parity tests.
import type { ActivitySummary } from "../../lib/activity";
import type { WorktreeOrder } from "../../lib/sidebarPersistence";
import type { SshHost } from "../../lib/sshHosts";
import type { RegisteredProject, Worktree } from "../../lib/types";
import type { RemoteHostState } from "../../state/remoteHostStore";
import { worktreeSortableId } from "./sidebarRows";

const REMOTE = "git@github.com:acme/ferryx.git";
const PAIRED_ID = `daemon:${"a".repeat(64)}`;

const row = (path: string, branch: string | null, extra: Partial<Worktree> = {}): Worktree => ({
  path, head: "abc123", branch, bare: false, detached: false, locked: null, prunable: null, ...extra,
});

const projects: RegisteredProject[] = [
  { workspaceId: "ferryx", repoRoot: "/repos/ferryx", gitRoot: "/repos/ferryx", gitCommonDir: "/repos/ferryx/.git", gitRemote: REMOTE, gitBranch: "main" },
  { workspaceId: "ssh:ferryx", repoRoot: "/home/me/ferryx", gitRoot: "/home/me/ferryx", gitRemote: REMOTE, gitBranch: "main", target: { kind: "ssh", hostId: "build" } },
  { workspaceId: PAIRED_ID, repoRoot: "/srv/app", gitRoot: "/srv/app", remoteWorkspaceId: "app", target: { kind: "pairedDaemon", hostId: "gamma" } },
  { workspaceId: "docs", repoRoot: "/repos/docs", gitRoot: "/repos/docs" },
  { workspaceId: "site", repoRoot: "/repos/site", gitRoot: "/repos/site" },
];

const sshHosts: SshHost[] = [
  { id: "build", label: "Build box", hostname: "build.local", source: "manual", authMethod: "agent" },
];

const remoteHostState: RemoteHostState = {
  hosts: {
    gamma: { hostId: "gamma", name: "Gamma", address: "", transport: "relay", online: false, authStatus: "paired", grantScope: "machine", generation: "7" },
  },
  activeHostId: null,
  discovering: false,
  nativeStatus: "ready",
  machineFeaturesEnabled: true,
};

/** feat-b, then the root, then feat-a: not the listing order. */
const worktreeOrder: WorktreeOrder = {
  ferryx: [
    worktreeSortableId("ferryx", "/repos/ferryx-wt/feat-b"),
    worktreeSortableId("ferryx", "/repos/ferryx"),
    worktreeSortableId("ferryx", "/repos/ferryx-wt/feat-a"),
  ],
};

const working: ActivitySummary = {
  workingCount: 1, waitingCount: 0, doneCount: 0, runningCount: 1,
  hasWorking: true, hasWaiting: false, hasDone: false, hasUnread: false,
};

export const sidebarParityFixture = {
  projects,
  activeProjectId: "ferryx",
  activePath: "/repos/ferryx-wt/feat-a",
  /** The same row as activeProjectId + activePath, named the way the remote inventory names it. */
  remoteSelection: { workspaceId: "ferryx", worktreeSlug: "feat-a" },
  worktrees: [
    row("/repos/ferryx", "refs/heads/main"),
    row("/repos/ferryx-wt/feat-a", "refs/heads/orca/ferryx/feat-a"),
    row("/repos/ferryx-wt/feat-b", "refs/heads/orca/ferryx/feat-b"),
  ],
  inactiveProjectWorktrees: {
    "ssh:ferryx": [row("/home/me/ferryx", "main")],
    [PAIRED_ID]: [row("/srv/app", "refs/heads/main", { workspaceId: PAIRED_ID })],
    site: [row("/repos/site", "refs/heads/main")],
  } satisfies Record<string, Worktree[]>,
  sshHosts,
  remoteHostState,
  listingFailures: { docs: "Connection reset by peer" } satisfies Record<string, string>,
  worktreeOrder,
  collapsedGroupIds: ["site"],
  /** One running agent, as the desktop sees it (by path) and the remote inventory reports it. */
  desktopActivity: { "/repos/ferryx-wt/feat-b": working } satisfies Record<string, ActivitySummary>,
  remoteActivity: [{ workspaceId: "ferryx", worktreeSlug: "feat-b", activityState: "working" as const }],
};
