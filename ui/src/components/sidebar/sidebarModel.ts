import type { ActivitySummary } from "../../lib/activity";
import { workspaceName } from "../../lib/branchFilter";
import { projectRootWorktree } from "../../lib/projectIdentity";
import { groupProjects, type ProjectGroup } from "../../lib/projectGrouping";
import type { WorktreeOrder } from "../../lib/sidebarPersistence";
import type { RegisteredProject } from "../../lib/tauri";
import { worktreeIdentity, type AgentState, type Worktree } from "../../lib/types";
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
  readonly worktreeOrder: WorktreeOrder;
  readonly collapsedGroupIds: ReadonlySet<string>;
  readonly advancedFeatures: boolean;
  readonly listingFailures: Readonly<Record<string, string>>;
};

function memberKind(project: RegisteredProject): SidebarMemberKind {
  if (project.target?.kind === "ssh") return "ssh";
  return project.target?.kind === "pairedDaemon" ? "paired" : "local";
}

function worktreeNode(groupId: string, row: Worktree, input: SidebarModelInput, label = worktreeLabel(row)): SidebarWorktreeNode {
  const workspaceId = row.workspaceId ?? groupId;
  return {
    key: worktreeSortableId(groupId, row.path, row.workspaceId),
    workspaceId,
    path: row.path,
    worktreeSlug: worktreeIdentity(row)?.slug ?? null,
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
        ...pairedSlotStatus(target.hostId, input.remoteState, input.reachability(target.hostId)),
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

/** The sidebar as the desktop renders it: groups, order, labels, rows and static states. */
export function buildSidebarModel(input: SidebarModelInput): SidebarModel {
  const groups = groupProjects(input.projects);
  const rowsByProject = applyWorktreeOrder(groupWorktreesByProject(input), input.worktreeOrder);
  const projects = groups.map((group) => projectNode(group, rowsByProject.get(group.primaryProject.workspaceId) ?? [], input));
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

/** Projects and rows left by the filter box, with forceExpanded set where a row matched. */
export function filterSidebarModel(model: SidebarModel, query: string) {
  return filterSidebarEntries(
    model.projects.map((node) => ({ group: node, rows: node.worktrees })),
    query,
    { project: (node) => node.filterLabels, row: (row) => row.label },
  );
}

export type SidebarLiveSignals = {
  readonly activityByWorktreePath?: Readonly<Record<string, ActivitySummary | undefined>>;
  readonly unreadWorktreePaths?: Readonly<Record<string, boolean>>;
  readonly agentStateByPath?: Readonly<Record<string, AgentState | undefined>>;
  readonly activeWorkspaceId?: string;
  readonly activePath?: string;
  readonly isSupervisorTyping?: (worktreePath: string) => boolean;
};

export type SidebarRowLive = { active: boolean; slot: SidebarSlotState; supervisorTyping: boolean };

export type SidebarLive = {
  rows: Record<string, SidebarRowLive>;
  projects: Record<string, ProjectSlot>;
  footer: string;
};

const isPairedNode = (node: { readonly workspaceId?: string | undefined }) => Boolean(node.workspaceId?.startsWith("daemon:"));

/**
 * One worktree row's live state. Paired rows ignore local signals: path-only local terminal data
 * must never light up a machine row with the same absolute path.
 */
export function worktreeRowLive(
  node: { readonly path: string; readonly workspaceId?: string | undefined },
  active: boolean,
  signals: SidebarLiveSignals,
): SidebarRowLive {
  const paired = isPairedNode(node);
  const summary = paired ? undefined : signals.activityByWorktreePath?.[node.path];
  const unread = !paired && !active && Boolean(summary?.hasUnread || signals.unreadWorktreePaths?.[node.path]);
  const slot = worktreeRowSlot({ activitySummary: summary, unread, agentState: paired ? undefined : signals.agentStateByPath?.[node.path] });
  const typing = signals.isSupervisorTyping?.(node.path) ?? false;
  return { active, slot, supervisorTyping: supervisorSlotWins(slot, isRemoteWorktreeRow(node), typing) };
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
      if (isPairedNode(node) || seen.has(footerKey)) continue;
      seen.add(footerKey);
      footerActivities.push(signals.activityByWorktreePath?.[node.path]);
    }
    const activity = summarizeProjectActivity(visible, signals.activityByWorktreePath, signals.unreadWorktreePaths);
    const { status } = project;
    const statusSlot = status ? { status: { kind: status.kind, label: status.label }, title: status.title, connected: status.kind === "online" } : null;
    projects[project.groupId] = projectSlot({ status: statusSlot, activity, attentionState: projectAttentionState(activity) });
  }
  return { rows, projects, footer: sidebarFooterSummary(footerActivities) };
}
