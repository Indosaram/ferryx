/**
 * Presentation layer of the remote session inventory: it turns the account's
 * RemoteContextOption[] plus the mirrored desktop panes into the desktop's project ->
 * worktree -> session tree. It owns no data fetching and no second inventory model, only the
 * row identity, placement and rollup rules the picker renders:
 *
 * - group identity is machine qualified, so identical workspaces and session ids on two
 *   machines stay distinct rows with distinct selection payloads;
 * - a pane's current state overrides the option's last-known state for that session;
 * - worktree and project indicators are rollups computed by the desktop activity helpers.
 */
import {
  combineActivitySummaries,
  resolveActivityIndicator,
  summarizeActivities,
  type TerminalActivity,
} from "../lib/activity";
import type { RemoteContextOption, RemoteTerminalTabInfo } from "./RemoteSessionList";

export type RemoteInventoryActivityState = "working" | "waiting" | "done";

const KEY_SEPARATOR = "\u0000";
const ROOT_WORKTREE_SENTINEL = "\u0000root";

export type RemoteInventorySelectionTarget = {
  machineId: string | null;
  daemonEpoch: string | number | null;
  workspaceId: string;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  sessionId: string | null;
  tabId: string | null;
};

export type RemoteInventoryContext = {
  workspaceId: string | null;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  activeTabId?: string | null;
  activeSessionId?: string | null;
  activeMachineId?: string | null;
  daemonEpoch?: string | number | null;
};

export type RemoteInventoryPaneEntry = {
  kind: "pane";
  key: string;
  sessionId: string | null;
  tabId: string;
  label: string;
  activity: RemoteInventoryActivityState | null;
  worktreeLabel: string | null;
  tab: RemoteTerminalTabInfo;
  active: boolean;
  selection: RemoteInventorySelectionTarget;
};

export type RemoteInventorySessionEntry = {
  kind: "session";
  key: string;
  sessionId: string | null;
  label: string;
  activity: RemoteInventoryActivityState | null;
  running: boolean;
  option: RemoteContextOption;
  pane: RemoteTerminalTabInfo | null;
  active: boolean;
  selection: RemoteInventorySelectionTarget;
};

export type RemoteInventoryEntry = RemoteInventorySessionEntry | RemoteInventoryPaneEntry;

export type RemoteInventoryWorktree = {
  key: string;
  machineId: string | null;
  workspaceId: string;
  slug: string | null;
  label: string | null;
  isRootWorktree: boolean;
  option: RemoteContextOption | null;
  selection: RemoteInventorySelectionTarget;
  activity: RemoteInventoryActivityState | null;
  active: boolean;
  entries: RemoteInventoryEntry[];
};

export type RemoteInventoryProject = {
  key: string;
  machineId: string | null;
  workspaceId: string;
  label: string;
  activity: RemoteInventoryActivityState | null;
  worktrees: RemoteInventoryWorktree[];
};

export type RemoteInventoryModel = {
  projects: RemoteInventoryProject[];
  orphanPanes: RemoteTerminalTabInfo[];
};

/**
 * Only declared agent metadata is a state. `running` is not activity: a session the daemon
 * lists as running has no known agent state, so the honest value stays null.
 */
export function resolveInventoryActivity(
  ...declared: unknown[]
): RemoteInventoryActivityState | null {
  for (const value of declared) {
    if (value === "working" || value === "waiting" || value === "done") return value;
  }
  return null;
}

export function inventoryMachineKey(machineId: string | null | undefined): string {
  return machineId ?? "";
}

export function inventoryEpochKey(epoch: string | number | null | undefined): string {
  return epoch === null || epoch === undefined ? "" : String(epoch);
}

/** An epoch the two sides disagree on is a different generation; an undeclared epoch is unknown. */
export function epochsCompatible(
  left: string | number | null | undefined,
  right: string | number | null | undefined,
): boolean {
  const a = inventoryEpochKey(left);
  const b = inventoryEpochKey(right);
  return a === "" || b === "" || a === b;
}

export function inventoryProjectKey(source: {
  machineId?: string | null;
  workspaceId: string;
}): string {
  return `${inventoryMachineKey(source.machineId)}${KEY_SEPARATOR}${source.workspaceId}`;
}

export function inventoryWorktreeKey(source: {
  machineId?: string | null;
  workspaceId: string;
  worktreeSlug?: string | null;
}): string {
  return `${inventoryProjectKey(source)}${KEY_SEPARATOR}${
    source.worktreeSlug ?? ROOT_WORKTREE_SENTINEL
  }`;
}

export function inventorySessionKey(source: {
  machineId?: string | null;
  workspaceId: string;
  worktreeSlug?: string | null;
  sessionId?: string | null;
  daemonEpoch?: string | number | null;
}): string {
  return `${inventoryWorktreeKey(source)}${KEY_SEPARATOR}${source.sessionId ?? ""}${
    KEY_SEPARATOR
  }${inventoryEpochKey(source.daemonEpoch)}`;
}

export function isRootWorktreeSlug(slug: string | null | undefined): boolean {
  return slug === null || slug === undefined || slug === "";
}

export function inventorySelectionTarget(source: {
  machineId?: string | null;
  daemonEpoch?: string | number | null;
  workspaceId: string;
  worktreeSlug?: string | null;
  worktreeLabel?: string | null;
  sessionId?: string | null;
  tabId?: string | null;
}): RemoteInventorySelectionTarget {
  return {
    machineId: source.machineId ?? null,
    daemonEpoch: source.daemonEpoch ?? null,
    workspaceId: source.workspaceId,
    worktreeSlug: source.worktreeSlug ?? null,
    worktreeLabel: source.worktreeLabel ?? null,
    sessionId: source.sessionId ?? null,
    tabId: source.tabId ?? null,
  };
}

/**
 * Two targets address the same session only when machine, workspace and worktree agree; a
 * differing epoch is a different generation of that session when both sides declare one.
 */
export function isSameInventorySelection(
  left: RemoteInventorySelectionTarget,
  right: RemoteInventorySelectionTarget,
): boolean {
  if (
    inventoryMachineKey(left.machineId) !== inventoryMachineKey(right.machineId) ||
    left.workspaceId !== right.workspaceId ||
    (left.worktreeSlug ?? null) !== (right.worktreeSlug ?? null) ||
    (left.sessionId ?? null) !== (right.sessionId ?? null)
  ) {
    return false;
  }
  const leftEpoch = inventoryEpochKey(left.daemonEpoch);
  const rightEpoch = inventoryEpochKey(right.daemonEpoch);
  return leftEpoch === "" || rightEpoch === "" || leftEpoch === rightEpoch;
}

export function selectionOption(
  selection: RemoteInventorySelectionTarget,
): RemoteContextOption {
  return {
    workspaceId: selection.workspaceId,
    worktreeSlug: selection.worktreeSlug,
    worktreeLabel: selection.worktreeLabel,
    ...(selection.machineId ? { machineId: selection.machineId } : {}),
    ...(selection.daemonEpoch !== null && selection.daemonEpoch !== undefined
      ? { daemonEpoch: selection.daemonEpoch }
      : {}),
    ...(selection.sessionId ? { sessionId: selection.sessionId } : {}),
    ...(selection.tabId ? { tabId: selection.tabId } : {}),
  };
}

export function optionActivity(option: RemoteContextOption): RemoteInventoryActivityState | null {
  return resolveInventoryActivity(
    option.attention,
    (option as { activityState?: unknown }).activityState,
  );
}

export function paneActivity(
  tab: RemoteTerminalTabInfo,
): RemoteInventoryActivityState | null {
  return resolveInventoryActivity(tab.activityState);
}

function activityOf(entry: RemoteInventoryEntry): RemoteInventoryActivityState | null {
  return entry.activity;
}

function toTerminalActivity(
  state: RemoteInventoryActivityState | null,
  label: string,
): TerminalActivity | null {
  if (state === null) return null;
  return { state, title: label, isAgent: true };
}

/** Worktree and project indicators are rollups; the desktop helpers own the precedence. */
export function rollupActivity(
  states: ReadonlyArray<RemoteInventoryActivityState | null | undefined>,
): RemoteInventoryActivityState | null {
  const activities = states
    .map((state, index) => toTerminalActivity(state ?? null, `entry-${index}`))
    .filter((activity): activity is TerminalActivity => activity !== null);
  if (activities.length === 0) return null;
  const indicator = resolveActivityIndicator(combineActivitySummaries([summarizeActivities(activities)]));
  if (indicator === null) return null;
  return indicator === "unread" ? "done" : indicator;
}

function optionLabel(option: RemoteContextOption): string | null {
  const label = option.sessionLabel ?? option.worktreeLabel ?? option.worktreeSlug;
  return label && label.trim().length > 0 ? label : null;
}

function sessionEntryLabel(option: RemoteContextOption): string {
  const declared = option.sessionLabel?.trim();
  if (declared) return declared;
  const sid = option.sessionId ?? "";
  if (sid) return `Session ${sid.slice(0, 8)}`;
  return optionLabel(option) ?? "Session";
}

function paneLabel(tab: RemoteTerminalTabInfo): string {
  const label = tab.label?.trim();
  return label && label.length > 0 ? label : "Terminal";
}

function projectLabelOf(option: RemoteContextOption): string | null {
  const label = option.projectLabel ?? option.workspaceLabel;
  return label && label.trim().length > 0 ? label.trim() : null;
}

type WorktreeDraft = RemoteInventoryWorktree;

type ProjectDraft = {
  key: string;
  machineId: string | null;
  workspaceId: string;
  label: string;
  worktrees: Map<string, WorktreeDraft>;
};

function draftProject(
  projects: Map<string, ProjectDraft>,
  source: { machineId?: string | null; workspaceId: string; label?: string | null },
): ProjectDraft {
  const key = inventoryProjectKey(source);
  let project = projects.get(key);
  if (!project) {
    project = {
      key,
      machineId: source.machineId ?? null,
      workspaceId: source.workspaceId,
      label: source.label?.trim() || source.workspaceId,
      worktrees: new Map<string, WorktreeDraft>(),
    };
    projects.set(key, project);
  } else if (source.label?.trim() && project.label === project.workspaceId) {
    project.label = source.label.trim();
  }
  return project;
}

function draftWorktree(
  project: ProjectDraft,
  source: {
    machineId?: string | null;
    workspaceId: string;
    worktreeSlug?: string | null;
    worktreeLabel?: string | null;
  },
  option: RemoteContextOption | null,
): WorktreeDraft {
  const key = inventoryWorktreeKey(source);
  const declared = option && !option.sessionId ? option : null;
  let worktree = project.worktrees.get(key);
  if (!worktree) {
    const slug = source.worktreeSlug ?? null;
    worktree = {
      key,
      machineId: source.machineId ?? null,
      workspaceId: source.workspaceId,
      slug,
      label: source.worktreeLabel ?? null,
      isRootWorktree: isRootWorktreeSlug(slug),
      option: declared,
      selection: inventorySelectionTarget({
        machineId: source.machineId ?? null,
        daemonEpoch: declared?.daemonEpoch ?? null,
        workspaceId: source.workspaceId,
        worktreeSlug: slug,
        worktreeLabel: source.worktreeLabel ?? null,
      }),
      activity: null,
      active: false,
      entries: [],
    };
    project.worktrees.set(key, worktree);
    return worktree;
  }
  if (!worktree.option && declared) worktree.option = declared;
  if (!worktree.label && source.worktreeLabel) worktree.label = source.worktreeLabel;
  return worktree;
}

function findWorktreeByLabel(
  project: ProjectDraft,
  label: string | null | undefined,
): WorktreeDraft | undefined {
  if (!label) return undefined;
  return [...project.worktrees.values()].find((candidate) => candidate.label === label);
}

export function declaresOwnProperty<T extends object>(source: T, key: keyof T): boolean {
  return Object.prototype.hasOwnProperty.call(source, key);
}

function paneSlug(tab: RemoteTerminalTabInfo, contextSlug: string | null): string | null {
  return declaresOwnProperty(tab, "worktreeSlug") ? tab.worktreeSlug ?? null : contextSlug;
}

function paneWorkspaceId(
  tab: RemoteTerminalTabInfo,
  contextWorkspaceId: string | null,
): string | null {
  return declaresOwnProperty(tab, "workspaceId") ? tab.workspaceId ?? null : contextWorkspaceId;
}

export function buildRemoteInventory(input: {
  options: readonly RemoteContextOption[];
  panes?: readonly RemoteTerminalTabInfo[];
  context?: RemoteInventoryContext;
  activeMachineId?: string | null;
}): RemoteInventoryModel {
  const projects = new Map<string, ProjectDraft>();
  const context = input.context ?? null;
  const activeMachineId = input.activeMachineId ?? context?.activeMachineId ?? null;
  const activeSessionId = context?.activeSessionId ?? null;
  const activeTabId = context?.activeTabId ?? null;
  const contextWorkspaceId = context?.workspaceId ?? null;
  const contextSlug = context?.worktreeSlug ?? null;
  const contextEpoch = context?.daemonEpoch ?? null;

  const machineMatches = (machineId: string | null) =>
    activeMachineId === null || machineId === null || machineId === activeMachineId;
  const epochMatches = (epoch: string | number | null | undefined) =>
    epochsCompatible(epoch, contextEpoch);

  for (const option of input.options) {
    if (option.sessionId) continue;
    const project = draftProject(projects, {
      machineId: option.machineId ?? null,
      workspaceId: option.workspaceId,
      label: projectLabelOf(option),
    });
    draftWorktree(project, option, option);
  }

  for (const option of input.options) {
    if (!option.sessionId) continue;
    const project = draftProject(projects, {
      machineId: option.machineId ?? null,
      workspaceId: option.workspaceId,
      label: projectLabelOf(option),
    });
    const worktree = draftWorktree(project, option, option);
    const key = inventorySessionKey(option);
    if (worktree.entries.some((entry) => entry.key === key)) continue;
    worktree.entries.push({
      kind: "session",
      key,
      sessionId: option.sessionId,
      label: sessionEntryLabel(option),
      activity: optionActivity(option),
      running: option.running !== false,
      option,
      pane: null,
      active:
        activeSessionId !== null &&
        option.sessionId === activeSessionId &&
        machineMatches(option.machineId ?? null) &&
        (contextWorkspaceId === null || option.workspaceId === contextWorkspaceId) &&
        ((contextSlug ?? null) === null || (option.worktreeSlug ?? null) === contextSlug) &&
        epochMatches(option.daemonEpoch),
      selection: inventorySelectionTarget({
        machineId: option.machineId ?? null,
        daemonEpoch: option.daemonEpoch ?? null,
        workspaceId: option.workspaceId,
        worktreeSlug: option.worktreeSlug ?? null,
        worktreeLabel: option.worktreeLabel ?? null,
        sessionId: option.sessionId,
      }),
    });
  }

  const orphanPanes: RemoteTerminalTabInfo[] = [];
  const paneIsActive = (tab: RemoteTerminalTabInfo) =>
    activeTabId !== null && activeTabId === tab.id && epochMatches(tab.daemonEpoch);
  for (const tab of input.panes ?? []) {
    const slug = paneSlug(tab, contextSlug);
    const workspaceId = paneWorkspaceId(tab, contextWorkspaceId);
    const project =
      workspaceId === null
        ? undefined
        : (activeMachineId === null
            ? projects.get(inventoryProjectKey({ machineId: null, workspaceId }))
            : projects.get(inventoryProjectKey({ machineId: activeMachineId, workspaceId }))
          ) ?? projects.get(inventoryProjectKey({ machineId: null, workspaceId }));
    const worktree = project
      ? project.worktrees.get(
          inventoryWorktreeKey({
            machineId: project.machineId,
            workspaceId: project.workspaceId,
            worktreeSlug: slug,
          }),
        ) ?? findWorktreeByLabel(project, tab.worktreeLabel)
      : undefined;
    if (!project || !worktree) {
      orphanPanes.push(tab);
      continue;
    }
    const paneSessionId = tab.sessionId ?? null;
    const existing =
      paneSessionId === null
        ? undefined
        : worktree.entries.find(
            (entry): entry is RemoteInventorySessionEntry =>
              entry.kind === "session" &&
              entry.sessionId === paneSessionId &&
              epochsCompatible(entry.option.daemonEpoch, tab.daemonEpoch),
          );
    if (existing) {
      existing.pane = tab;
      // A pane only speaks for the session when it declares a state; an undeclared pane must not
      // erase the activity the option already reported.
      const declared = paneActivity(tab);
      if (declared !== null) existing.activity = declared;
      existing.active = paneIsActive(tab);
      continue;
    }
    worktree.entries.push({
      kind: "pane",
      key: `${worktree.key}${KEY_SEPARATOR}pane${KEY_SEPARATOR}${tab.id}`,
      sessionId: paneSessionId,
      tabId: tab.id,
      label: paneLabel(tab),
      activity: paneActivity(tab),
      worktreeLabel: tab.worktreeLabel ?? null,
      tab,
      active: paneIsActive(tab),
      selection: inventorySelectionTarget({
        machineId: worktree.machineId,
        daemonEpoch: tab.daemonEpoch ?? null,
        workspaceId: worktree.workspaceId,
        worktreeSlug: worktree.slug,
        worktreeLabel: worktree.label,
        sessionId: paneSessionId,
        tabId: tab.id,
      }),
    });
  }

  const result: RemoteInventoryProject[] = [];
  const allWorktrees = [...projects.values()].flatMap((project) => [...project.worktrees.values()]);
  // An active pane or session row is the precise selection; the worktree rows must not claim it
  // for a different worktree, which is what a null-slug pane against a named context would do.
  const activeEntryWorktreeKey =
    allWorktrees.find((worktree) => worktree.entries.some((entry) => entry.active))?.key ?? null;
  for (const project of projects.values()) {
    const projectMatches =
      context !== null &&
      context.workspaceId === project.workspaceId &&
      machineMatches(project.machineId);
    const worktrees: RemoteInventoryWorktree[] = [];
    for (const worktree of project.worktrees.values()) {
      const worktreeMatches =
        projectMatches &&
        ((contextSlug ?? null) === null
          ? worktree.slug === null
          : worktree.slug === contextSlug);
      worktree.active =
        activeEntryWorktreeKey !== null
          ? worktree.key === activeEntryWorktreeKey
          : worktreeMatches && worktree.entries.length > 0;
      worktree.activity = rollupActivity([
        worktree.option ? optionActivity(worktree.option) : null,
        ...worktree.entries.map(activityOf),
      ]);
      worktrees.push(worktree);
    }
    result.push({
      key: project.key,
      machineId: project.machineId,
      workspaceId: project.workspaceId,
      label: project.label,
      activity: rollupActivity(worktrees.map((worktree) => worktree.activity)),
      worktrees,
    });
  }

  return { projects: result, orphanPanes };
}