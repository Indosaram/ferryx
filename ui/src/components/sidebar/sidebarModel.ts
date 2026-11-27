import type { ActivitySummary } from "../../lib/activity";
import { workspaceName } from "../../lib/branchFilter";
import { projectRootWorktree } from "../../lib/projectIdentity";
import { groupProjects, type ProjectGroup } from "../../lib/projectGrouping";
import type { LaneLifecycleState, LanePullRequestView, SidebarLaneLifecycle, WorktreeOrder } from "../../lib/sidebarPersistence";
import { laneLifecycleKey, worktreeLifecycleIdentity } from "../../lib/sidebarPersistence";
import type { RegisteredProject } from "../../lib/tauri";
import { worktreeIdentity, type AgentOutcome, type AgentState, type Worktree } from "../../lib/types";
import {
  filterSidebarEntries,
  groupHeaderShowsHost,
  isRemoteWorktreeRow,
  isStandaloneRemoteGroup,
  projectDisplayLabel,
  projectFilterLabels,
  projectHostLabel,
  SIDEBAR_EMPTY_TEXT,
  worktreeLabel,
  worktreeRowHost,
} from "./sidebarLabels";
import { deduplicateSidebarProjects } from "./sidebarDedup";
import { applyWorktreeOrder, groupWorktreesByProject, worktreeSortableId, type SidebarRowsInput } from "./sidebarRows";
import {
  pairedSlotStatus,
  projectAttentionState,
  projectSlot,
  projectStatus,
  sidebarFooterSummary,
  summarizeProjectActivity,
  supervisorSlotWins,
  worktreeRowSlot,
  type ProjectSlot,
  type SidebarStatus,
} from "./sidebarSlots";
import type { SidebarSessionSignals } from "./sidebarSessionSignals";
import type { SidebarSlotState } from "./SidebarStateSlot";

export type { SidebarStatus } from "./sidebarSlots";
export { sidebarFooterSummary } from "./sidebarSlots";

export type SidebarMemberKind = "local" | "ssh" | "paired";

export type SidebarWorktreeNode = {
  key: string;
  workspaceId: string;
  /** Filesystem path on the owning machine; live signals are keyed by it. */
  path: string;
  worktreeSlug: string | null;
  /**
   * The server-owned lifecycle identity of this row (`w:<wsId>/<slug>` or `p:<ws>/<path>`).
   * It is the join key between a rendered row and `lane_lifecycles`, so a pin, snooze, settle or
   * archive survives an app restart instead of living in this window.
   */
  lifecycleKey: string;
  /** The state the server reports, already resolved (`expired snooze` reads `active`). */
  lifecycle: LaneLifecycleState | null;
  lifecycleRevision?: number;
  /** Absent on older published snapshots; no PR reserves no visual space. */
  pullRequest?: LanePullRequestView | null;
  /** The per-worktree pin set folded from `SET_TAB_PINNED`; empty when nothing is pinned. */
  pinnedTabs: string[];
  label: string;
  hostLabel: string | null;
  stale: boolean;
  listingFailure: string | null;
};

export type SidebarProjectNode = {
  groupId: string;
  label: string;
  hostLabel: string | null;
  members: { workspaceId: string; kind: SidebarMemberKind; machineId: string | null }[];
  primaryWorkspaceId: string;
  status: SidebarStatus | null;
  /** Every label the filter box matches for this project (member folders and host names). */
  filterLabels: string[];
  worktrees: SidebarWorktreeNode[];
  worktreesEmptyHint: string | null;
};

export type SidebarModel = {
  schemaVersion: 1;
  revision: number;
  generatedAt: string;
  source: "desktop" | "inventory";
  projects: SidebarProjectNode[];
  collapsedGroupIds: string[];
  advancedFeatures: boolean;
  emptyState: "noProjects" | null;
  /** Set only on a published snapshot: the desktop's live-only signals at publish time. */
  signals?: SidebarSessionSignals;
};

export type SidebarModelInput = SidebarRowsInput & {
  readonly source: SidebarModel["source"];
  readonly generatedAt: string;
  /** The daemon assigns the published revision; builders pass 0 unless they own one. */
  readonly revision?: number;
  /**
   * The pre-server order, still read from `localStorage` before the one-time import lands.
   * Ignored for any workspace the server model already orders.
   */
  readonly worktreeOrder: WorktreeOrder;
  /**
   * The server-owned order (`workspaceId -> lifecycle keys`). This is the authority: a workspace
   * present here is ordered by it and `worktreeOrder` is never consulted for that workspace, so
   * the two cannot fight. An absent map (a build with no hub) falls back to `worktreeOrder`.
   */
  readonly laneOrder?: Readonly<Record<string, readonly string[]>>;
  /** Each row's server state and folded pin set, keyed by lifecycle key. */
  readonly laneLifecycles?: Readonly<Record<string, SidebarLaneLifecycle>>;
  readonly collapsedGroupIds: ReadonlySet<string>;
  readonly advancedFeatures: boolean;
  readonly listingFailures: Readonly<Record<string, string>>;
};

/**
 * A worktree row's lifecycle key, from the same function the model uses, so the drag handler and
 * the builder cannot disagree about which row a key names.
 */
export function worktreeLifecycleKey(row: Worktree, workspaceId: string): string {
  return worktreeLifecycleIdentity(row, workspaceId).worktreeKey;
}

function memberKind(project: RegisteredProject): SidebarMemberKind {
  if (project.target?.kind === "ssh") return "ssh";
  return project.target?.kind === "pairedDaemon" ? "paired" : "local";
}

function worktreeNode(groupId: string, row: Worktree, input: SidebarModelInput, label = worktreeLabel(row)): SidebarWorktreeNode {
  const workspaceId = row.workspaceId ?? groupId;
  const identity = worktreeIdentity(row);
  // A managed worktree keys on the branch's own workspace id, so the key does not depend on which
  // project group happens to render the row; anything else keys on the row's workspace and path.
  const lifecycleKey = worktreeLifecycleIdentity(row, groupId).worktreeKey;
  const lifecycle = input.laneLifecycles?.[lifecycleKey];
  return {
    key: worktreeSortableId(groupId, row.path, row.workspaceId),
    workspaceId,
    path: row.path,
    worktreeSlug: identity?.slug ?? null,
    lifecycleKey,
    lifecycle: lifecycle?.state ?? null,
    ...(lifecycle?.pullRequest ? { pullRequest: lifecycle.pullRequest } : {}),
    pinnedTabs: [...(lifecycle?.pinnedTabs ?? [])],
    label,
    hostLabel: worktreeRowHost(row) ?? null,
    stale: Boolean(row.stale || row.freshness?.stale),
    listingFailure: input.listingFailures[workspaceId] ?? null,
  };
}

function statusNode(slot: ReturnType<typeof projectStatus>): SidebarStatus | null {
  if (!slot) return null;
  return slot.title === undefined ? slot.status : { ...slot.status, title: slot.title };
}

function projectNode(group: ProjectGroup, rows: readonly Worktree[], input: SidebarModelInput): SidebarProjectNode {
  const project = group.primaryProject;
  const hostLabel = projectHostLabel(project, input);
  const target = project.target;
  const paired = target?.kind === "pairedDaemon"
    ? {
        staleSummary: rows.find((row) => row.stale || row.freshness?.stale)?.hostSummary,
        ...pairedSlotStatus(target.hostId, input.remoteState, input.reachability(target.hostId), input.route?.(target.hostId)),
      }
    : null;
  const standalone = isStandaloneRemoteGroup(group);
  const root = projectRootWorktree(project, hostLabel ?? undefined);
  // A lone SSH project is one root row named after the folder, never a managed slug.
  const worktrees = standalone
    ? [worktreeNode(group.groupId, root, input, workspaceName(root))]
    : rows.map((row) => worktreeNode(group.groupId, row, input));
  return {
    groupId: group.groupId,
    label: projectDisplayLabel(project, hostLabel, groupHeaderShowsHost(group)),
    hostLabel,
    members: group.memberProjects.map((member) => ({
      workspaceId: member.workspaceId,
      kind: memberKind(member),
      machineId: member.target?.kind === "pairedDaemon" ? (input.remoteState.hosts[member.target.hostId]?.machineId ?? null) : null,
    })),
    primaryWorkspaceId: project.workspaceId,
    status: statusNode(projectStatus({ listingFailure: input.listingFailures[project.workspaceId], paired })),
    filterLabels: projectFilterLabels(group, input),
    worktrees,
    worktreesEmptyHint: worktrees.length === 0 ? SIDEBAR_EMPTY_TEXT.noWorktrees : null,
  };
}

/**
 * One project's rows in the server's section order.
 *
 * The server owns the order, so a workspace it orders is ordered ONLY by it: `worktreeOrder`
 * (the pre-import `localStorage` copy) is not consulted again for that workspace, which is what
 * stops the two from fighting. Rows the server has no lifecycle for follow in the stable local
 * order `applyWorktreeOrder` already produced, so a fresh checkout appears at the end of the
 * section instead of in an arbitrary place.
 */
function applyLaneOrder(
  rows: readonly Worktree[],
  workspaceId: string,
  laneOrder: Readonly<Record<string, readonly string[]>> | undefined,
): Worktree[] {
  const keys = laneOrder?.[workspaceId];
  if (!keys || keys.length === 0) return [...rows];
  const rank = new Map(keys.map((key, index) => [key, index] as const));
  const rowKey = (row: Worktree) => {
    const identity = worktreeIdentity(row);
    return laneLifecycleKey({
      workspaceId: identity?.wsId ?? (row.workspaceId ?? workspaceId),
      path: row.path,
      worktreeSlug: identity?.slug ?? null,
    });
  };
  return [...rows].sort(
    (left, right) =>
      (rank.get(rowKey(left)) ?? Number.MAX_SAFE_INTEGER) - (rank.get(rowKey(right)) ?? Number.MAX_SAFE_INTEGER),
  );
}

/** The sidebar as the desktop renders it: groups, order, labels, rows and static states. */
export function buildSidebarModel(input: SidebarModelInput): SidebarModel {
  const dedupedProjects = deduplicateSidebarProjects(input.projects, input.hosts, input.remoteState);
  const groups = groupProjects(dedupedProjects);
  const rowsByProject = applyWorktreeOrder(groupWorktreesByProject({ ...input, projects: dedupedProjects }), input.worktreeOrder);
  const projects = groups.map((group) => {
    const workspaceId = group.primaryProject.workspaceId;
    const rows = rowsByProject.get(workspaceId) ?? [];
    return projectNode(group, applyLaneOrder(rows, workspaceId, input.laneOrder), input);
  });
  return {
    schemaVersion: 1,
    revision: input.revision ?? 0,
    generatedAt: input.generatedAt,
    source: input.source,
    projects,
    collapsedGroupIds: projects.map((node) => node.groupId).filter((id) => input.collapsedGroupIds.has(id)),
    advancedFeatures: input.advancedFeatures,
    emptyState: input.projects.length === 0 ? "noProjects" : null,
  };
}

/** Active rows; a search can still find settled rows to reopen them. */
export function filterSidebarModel(model: SidebarModel, query: string) {
  return filterSidebarEntries(
    model.projects.map((node) => ({ group: node, rows: node.worktrees.filter((row) =>
      row.lifecycle !== "settled" || query.trim() !== "",
    ) })),
    query,
    { project: (node) => node.filterLabels, row: (row) => row.label },
  );
}

export function worktreeActivityKey(workspaceId: string, path: string): string {
  return `${workspaceId}\0${path}`;
}

export type SidebarLiveSignals = {
  readonly activityByWorktreePath?: Readonly<Record<string, ActivitySummary | undefined>>;
  /** Workspace-scoped activity keyed by `worktreeActivityKey(workspaceId, path)`. Prevents cross-machine path collisions and enables paired (`daemon:`) rows. */
  readonly activityByWorktreeKey?: Readonly<Record<string, ActivitySummary | undefined>>;
  readonly unreadWorktreePaths?: Readonly<Record<string, boolean>>;
  readonly unreadWorktreeKeys?: Readonly<Record<string, boolean>>;
  readonly agentStateByPath?: Readonly<Record<string, AgentState | undefined>>;
  readonly activeWorkspaceId?: string;
  readonly activePath?: string;
  readonly isSupervisorTyping?: (worktreePath: string) => boolean;
};

/**
 * `outcome` is set only while the row shows an unseen completion whose program said how it ended;
 * focusing the pane marks the completion seen, which drops it.
 */
export type SidebarRowLive = { active: boolean; slot: SidebarSlotState; supervisorTyping: boolean; outcome?: AgentOutcome };

export type SidebarLive = {
  rows: Record<string, SidebarRowLive>;
  projects: Record<string, ProjectSlot>;
  footer: string;
};

const isPairedNode = (node: { readonly workspaceId?: string | undefined }) => Boolean(node.workspaceId?.startsWith("daemon:"));

export function lookupWorktreeActivity(
  node: { readonly path: string; readonly workspaceId?: string | undefined },
  signals: Pick<SidebarLiveSignals, "activityByWorktreePath" | "activityByWorktreeKey">,
): ActivitySummary | undefined {
  if (node.workspaceId) {
    const scoped = signals.activityByWorktreeKey?.[worktreeActivityKey(node.workspaceId, node.path)];
    if (scoped !== undefined) return scoped;
  }
  return isPairedNode(node) ? undefined : signals.activityByWorktreePath?.[node.path];
}

export function lookupWorktreeUnread(
  node: { readonly path: string; readonly workspaceId?: string | undefined },
  signals: Pick<SidebarLiveSignals, "unreadWorktreePaths" | "unreadWorktreeKeys">,
): boolean {
  if (node.workspaceId) {
    const scoped = signals.unreadWorktreeKeys?.[worktreeActivityKey(node.workspaceId, node.path)];
    if (scoped !== undefined) return Boolean(scoped);
  }
  return !isPairedNode(node) && Boolean(signals.unreadWorktreePaths?.[node.path]);
}

/**
 * One worktree row's live state. Paired rows ignore path-only local signals (preventing a local
 * terminal at the same absolute path from lighting up a paired machine row), while still reading
 * their own workspace-scoped activity keyed by `worktreeActivityKey(workspaceId, path)`.
 */
export function worktreeRowLive(
  node: { readonly path: string; readonly workspaceId?: string | undefined },
  active: boolean,
  signals: SidebarLiveSignals,
): SidebarRowLive {
  const paired = isPairedNode(node);
  const summary = lookupWorktreeActivity(node, signals);
  const unread = !active && Boolean(summary?.hasUnread || lookupWorktreeUnread(node, signals));
  const slot = worktreeRowSlot({ activitySummary: summary, unread, agentState: paired ? undefined : signals.agentStateByPath?.[node.path] });
  const typing = signals.isSupervisorTyping?.(node.path) ?? false;
  const outcome = (slot === "done" || slot === "unread") ? summary?.outcome : undefined;
  return { active, slot, supervisorTyping: supervisorSlotWins(slot, isRemoteWorktreeRow(node), typing), ...(outcome ? { outcome } : {}) };
}

/**
 * The sidebar's live state, the only place either app derives it: per-row state, per-project slot
 * and the footer. Project slots and the footer count only what the filter box leaves visible.
 */
export function sidebarLiveState(model: SidebarModel, signals: SidebarLiveSignals, filterQuery: string): SidebarLive {
  const rows: Record<string, SidebarRowLive> = {};
  const projects: Record<string, ProjectSlot> = {};
  for (const project of model.projects) {
    const activeGroup = project.members.some((member) => member.workspaceId === signals.activeWorkspaceId);
    for (const node of project.worktrees) {
      const active = activeGroup && node.path === signals.activePath && node.workspaceId === signals.activeWorkspaceId;
      rows[node.key] = worktreeRowLive(node, active, signals);
    }
  }
  const seen = new Set<string>();
  const footerActivities: (ActivitySummary | undefined)[] = [];
  for (const { group: project, rows: visible } of filterSidebarModel(model, filterQuery)) {
    for (const node of visible) {
      const footerKey = `${node.workspaceId}:${node.path}`;
      if (seen.has(footerKey)) continue;
      seen.add(footerKey);
      const summary = lookupWorktreeActivity(node, signals);
      if (summary !== undefined) footerActivities.push(summary);
    }
    const activity = summarizeProjectActivity(
      visible,
      signals.activityByWorktreePath,
      signals.unreadWorktreePaths,
      signals.activityByWorktreeKey,
      signals.unreadWorktreeKeys,
    );
    const { status } = project;
    const statusSlot = status ? { status: { kind: status.kind, label: status.label }, title: status.title, connected: status.kind === "online" } : null;
    projects[project.groupId] = projectSlot({ status: statusSlot, activity, attentionState: projectAttentionState(activity) });
  }
  return { rows, projects, footer: sidebarFooterSummary(footerActivities) };
}
