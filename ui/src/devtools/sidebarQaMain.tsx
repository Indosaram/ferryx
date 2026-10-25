import ReactDOM from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ActivitySummary } from "../lib/activity";
import type { SshHost } from "../lib/sshHosts";
import {
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "../lib/storageKeys";
import type { RegisteredProject } from "../lib/tauri";
import type { ActiveAgent, DirtyState, Worktree } from "../lib/types";
import { sidebarParityFixture } from "../components/sidebar/sidebarParityFixture";
import "../index.css";
import "../settings-runtime.css";

// Fixture-only entry: no real daemon, SSH target, account, or terminal is contacted.
// Query params: width (px, default 236), theme (light|dark, default dark). The Sidebar has no
// filter input, so a `filter` param is accepted and ignored.
// `fixture=parity` renders the shared sidebar parity fixture the remote QA harness also renders.
const params = new URLSearchParams(location.search);
const parity = params.get("fixture") === "parity";
const parsedWidth = Number(params.get("width"));
const width = Number.isFinite(parsedWidth) && parsedWidth > 0 ? Math.round(parsedWidth) : 236;
document.documentElement.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
Object.defineProperty(globalThis, "isTauri", { value: true, configurable: true });

// Local project headers render `workspaceId` verbatim as the label, so local IDs are the display names.
const LOCAL_ID = "ferryx";
const REMOTE_ID = "ssh:omarchy:relay";
// A lone SSH project renders as one standalone root button; a second SSH member sharing the
// git common dir turns the group into a real worktree list with one row per member.
const REMOTE_LATENCY_ID = "ssh:omarchy:relay-latency";
const SITE_ID = "site";
const FERRYX_ROOT = "/fixture/code/ferryx";
const RELAY_ROOT = "/home/fixture/code/relay";
const RELAY_LATENCY_ROOT = `${RELAY_ROOT}/.orca-worktrees/relay/latency`;
const RELAY_COMMON_DIR = `${RELAY_ROOT}/.git`;
const SITE_ROOT = "/fixture/code/site";

// The Sidebar reads width, collapsed projects, and row order from storage on mount.
localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(width));
localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(parity ? sidebarParityFixture.collapsedGroupIds : [SITE_ID]));
if (parity) localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify(sidebarParityFixture.worktreeOrder));
else localStorage.removeItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY);

const omarchy: SshHost = {
  id: "omarchy",
  label: "omarchy",
  hostname: "omarchy.fixture",
  source: "config",
  authMethod: "agent",
};

mockIPC((command) => {
  switch (command) {
    case "cmd_ssh_list_hosts": return [omarchy];
    case "cmd_native_sidebar_context_menu": return null;
    case "plugin:event|listen": return 1;
    case "plugin:event|unlisten": return null;
    default: return null;
  }
}, { shouldMockEvents: true });

function worktree(path: string, branch: string, head: string, extra: Partial<Worktree> = {}): Worktree {
  return { path, head, branch, bare: false, detached: false, locked: null, prunable: null, ...extra };
}

function activity(partial: Partial<ActivitySummary>): ActivitySummary {
  return {
    workingCount: 0,
    waitingCount: 0,
    doneCount: 0,
    runningCount: 0,
    hasWorking: false,
    hasWaiting: false,
    hasDone: false,
    hasUnread: false,
    ...partial,
  };
}

const projects: RegisteredProject[] = [
  { workspaceId: LOCAL_ID, repoRoot: FERRYX_ROOT, gitRoot: FERRYX_ROOT, gitBranch: "main", gitHead: "a1b2c3d", target: { kind: "local" } },
  { workspaceId: REMOTE_ID, repoRoot: RELAY_ROOT, gitRoot: RELAY_ROOT, gitCommonDir: RELAY_COMMON_DIR, gitBranch: "main", gitHead: "e4f5a6b", hostLabel: "omarchy", target: { kind: "ssh", hostId: omarchy.id } },
  { workspaceId: REMOTE_LATENCY_ID, repoRoot: RELAY_LATENCY_ROOT, gitRoot: RELAY_LATENCY_ROOT, gitCommonDir: RELAY_COMMON_DIR, gitBranch: "orca/relay/latency", gitHead: "f5a6b7c", hostLabel: "omarchy", target: { kind: "ssh", hostId: omarchy.id } },
  { workspaceId: SITE_ID, repoRoot: SITE_ROOT, gitRoot: SITE_ROOT, gitBranch: "main", gitHead: "c7d8e9f", target: { kind: "local" } },
];

const ferryxWorktrees: Worktree[] = [
  worktree(FERRYX_ROOT, "main", "a1b2c3d"),
  worktree(`${FERRYX_ROOT}/.orca-worktrees/ferryx/remote-settings`, "orca/ferryx/remote-settings", "b2c3d4e"),
  worktree(`${FERRYX_ROOT}/.orca-worktrees/ferryx/sidebar-polish`, "orca/ferryx/sidebar-polish", "c3d4e5f"),
  worktree(`${FERRYX_ROOT}/.orca-worktrees/ferryx/daemon-lifetime`, "orca/ferryx/daemon-lifetime", "d4e5f6a"),
];
// Each SSH member owns exactly its own root row; listing both under REMOTE_ID would duplicate the
// latency row once the Sidebar synthesizes the second member's root.
const relayWorktrees: Worktree[] = [
  worktree(RELAY_ROOT, "main", "e4f5a6b", { workspaceId: REMOTE_ID, hostLabel: "omarchy" }),
  worktree(RELAY_LATENCY_ROOT, "orca/relay/latency", "f5a6b7c", { workspaceId: REMOTE_LATENCY_ID, hostLabel: "omarchy" }),
];
const siteWorktrees: Worktree[] = [
  worktree(SITE_ROOT, "main", "c7d8e9f"),
  worktree(`${SITE_ROOT}/.orca-worktrees/site/launch-page`, "orca/site/launch-page", "d8e9f0a"),
];

// Selected row: ferryx/remote-settings. Dirty row: ferryx/sidebar-polish.
const activePath = ferryxWorktrees[1].path;
const statuses: Record<string, DirtyState | undefined> = {
  [ferryxWorktrees[2].path]: {
    isDirty: true,
    files: [
      { statusCode: " M", path: "ui/src/components/Sidebar.tsx" },
      { statusCode: "??", path: "ui/sidebar-qa.html" },
    ],
  },
};
const activityByWorktreePath: Record<string, ActivitySummary | undefined> = {
  [ferryxWorktrees[1].path]: activity({ workingCount: 1, runningCount: 1, hasWorking: true, agentType: "claude" }),
  [ferryxWorktrees[2].path]: activity({ waitingCount: 1, runningCount: 1, hasWaiting: true, agentType: "codex" }),
  [ferryxWorktrees[3].path]: activity({ doneCount: 1, hasDone: true, agentType: "claude" }),
  [relayWorktrees[1].path]: activity({ workingCount: 1, runningCount: 1, hasWorking: true, agentType: "codex" }),
};
const unreadWorktreePaths: Record<string, boolean> = {
  [ferryxWorktrees[0].path]: true,
  [relayWorktrees[0].path]: true,
};
const agents: ActiveAgent[] = [
  {
    id: "fixture-agent-1",
    name: "claude",
    task: "Redesign remote settings",
    state: "working",
    worktree: null,
    worktreePath: ferryxWorktrees[1].path,
    sessionId: "fixture-session-1",
  },
];

const markFixture = (key: string) => (value?: { path?: string; workspaceId?: string } | void) => {
  document.documentElement.dataset[key] = value && typeof value === "object"
    ? (value.path ?? value.workspaceId ?? "true")
    : "true";
};

const { Sidebar } = await import("../components/Sidebar");
// Imported after `isTauri` is defined: the host store picks its desktop mode at module load.
const { remoteHostStore } = await import("../state/remoteHostStore");
const { updateListingFailures } = await import("../state/worktreeListingFailures");
if (parity) {
  remoteHostStore.setState((state) => ({ ...state, hosts: sidebarParityFixture.pairedHosts, nativeStatus: "ready", machineFeaturesEnabled: true }));
  updateListingFailures(() => sidebarParityFixture.listingFailures);
}
const attention = {
  rows: [1, 2, 3].map((n) => ({
    id: `qa-row-${n}`, revision: 1, workspaceId: LOCAL_ID, sessionId: `qa-sess-${n}`, state: "needs-you" as const,
    who: "omo", location: `ferryx / row ${n}`, text: "Needs input", at: 0,
  })),
  onOpen: () => markFixture("fixtureAttentionOpen")(),
  onDismiss: () => markFixture("fixtureAttentionDismiss")(),
  onDismissAll: () => markFixture("fixtureAttentionDismissAll")(),
  inboxOpen: params.get("inbox") === "1",
  onInboxOpenChange: () => markFixture("fixtureInboxToggle")(),
};
const root = document.getElementById("root");
if (root && parity) {
  const fixture = sidebarParityFixture;
  ReactDOM.createRoot(root).render(
    <div data-testid="sidebar-qa-root" className="flex overflow-hidden bg-background" style={{ width: `${width}px`, height: "100vh" }}>
      <Sidebar
        open
        projects={fixture.projects}
        activeProjectId={fixture.activeProjectId}
        worktrees={fixture.worktrees}
        inactiveProjectWorktrees={fixture.inactiveProjectWorktrees}
        agents={[...sidebarParityFixture.desktopAgents]}
        activePath={fixture.active.path}
        unreadWorktreePaths={sidebarParityFixture.desktopUnread}
        activityByWorktreePath={sidebarParityFixture.desktopActivity}
        onSelectProject={markFixture("fixtureSelectedProject")}
        onSelectWorktree={markFixture("fixtureSelectedWorktree")}
        onCreateWorktree={markFixture("fixtureCreateWorktree")}
        onOpenSettings={() => markFixture("fixtureOpenSettings")()}
        attention={{ ...attention, rows: [...sidebarParityFixture.desktopAttention] }}
      />
    </div>,
  );
  document.documentElement.dataset.qaReady = "true";
} else if (root) {
  ReactDOM.createRoot(root).render(
    <div data-testid="sidebar-qa-root" className="flex overflow-hidden bg-background" style={{ width: `${width}px`, height: "100vh" }}>
      <Sidebar
        open
        projects={projects}
        activeProjectId={LOCAL_ID}
        worktrees={ferryxWorktrees}
        inactiveProjectWorktrees={{
          [REMOTE_ID]: [relayWorktrees[0]],
          [REMOTE_LATENCY_ID]: [relayWorktrees[1]],
          [SITE_ID]: siteWorktrees,
        }}
        agents={agents}
        activePath={activePath}
        statuses={statuses}
        unreadWorktreePaths={unreadWorktreePaths}
        activityByWorktreePath={activityByWorktreePath}
        onSelectProject={markFixture("fixtureSelectedProject")}
        onSelectWorktree={markFixture("fixtureSelectedWorktree")}
        onCreateWorktree={markFixture("fixtureCreateWorktree")}
        onOpenSettings={() => markFixture("fixtureOpenSettings")()}
        attention={attention}
      />
    </div>,
  );
}
