import { FolderTree, GitBranch, LogOut, PanelLeft, Plus, Server, Terminal as TerminalIcon, X } from "lucide-react";
import React, { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { SidebarViewId } from "../components/sidebar/SidebarViewTabs";
import { IconButton } from "../components/ui/IconButton";
import { buildRemoteAttentionRows, type AttentionRow } from "../features/ferryx/attention/attentionModel";
import { buildRemoteInventory, declaresOwnProperty, type RemoteInventoryModel } from "./remoteInventoryView";
import { RemoteSidebar, type PublishedAttentionTarget, type RemoteSidebarTarget } from "./RemoteSidebar";
import { RemoteTerminalTabStrip, stripTabs, type RemoteStripTab } from "./RemoteTerminalTabStrip";
import type { RemoteSidebarSection } from "./useRemoteSidebarModel";
import { useOverlayHistory } from "./home/usePhoneNavigation";
import { useDragDismiss } from "./useDragDismiss";
import { cn } from "../lib/cn";

export type RemoteTerminalTabInfo = {
  id: string;
  label: string;
  activityState?: "working" | "waiting" | "done";
  agentType?: string;
  worktreeSlug?: string | null;
  worktreeLabel?: string;
  sessionId?: string;
  /** Declared only when the pane knows its workspace; an absent field falls back to the context. */
  workspaceId?: string | null;
  /** Epoch of the session this pane mirrors, when the machine declares one. */
  daemonEpoch?: string | number | null;
};

export type RemoteTerminalItem = {
  sessionId: string;
  running?: boolean;
  daemonEpoch?: string | number | null;
  title?: string | null;
  workspaceId?: string | null;
  worktreeLabel?: string | null;
};

export type RemoteContext = {
  workspaceId: string | null;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  activeTerminal: RemoteTerminalItem | null;
  activeTabId?: string | null;
  terminalTabs?: RemoteTerminalTabInfo[];
  /** Epoch the context points at, when the desktop declares one without a terminal item. */
  daemonEpoch?: string | number | null;
};

export type RemoteContextOption = {
  unavailable?: boolean;
  workspaceId: string;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  tabId?: string | null;
  sessionId?: string | null;
  sessionLabel?: string;
  attention?: "working" | "waiting" | "done";
  /** Declared agent metadata for a session row; `running` alone is not an activity state. */
  activityState?: "working" | "waiting" | "done";
  running?: boolean;
  agentType?: string | null;
  daemonEpoch?: string | number | null;
  /** Human project name for the group header; the workspace id stays the row identity. */
  projectLabel?: string | null;
  workspaceLabel?: string | null;
  /**
   * Account inventory only: the hidden machine that owns this option, used for dispatch and
   * row identity. It is never rendered; the picker shows worktrees exactly like the desktop.
   */
  machineId?: string;
  /**
   * Open only what the machine already runs: never start a terminal for this choice. Set on a
   * desktop-published Inbox row, which names an existing pane, so a stale row cannot spawn a shell.
   */
  attachOnly?: boolean;
};

export type RemoteWorkspaceModel = {
  context: RemoteContext;
  options: RemoteContextOption[];
  /** The host's waiting or unseen-done sessions across every project (`sessionAttention`). */
  sessionAttention?: RemoteSessionAttention[];
};

export type RemoteSessionAttention = {
  sessionId: string;
  workspaceId: string;
  worktreeSlug: string | null;
  state: "waiting" | "done";
  agentType: string | null;
};

type UnknownRecord = Record<string, unknown>;

const REMOTE_DESKTOP_SIDEBAR_QUERY = "(min-width: 768px)";

/**
 * The remote shell follows the desktop layout: a persistent sidebar where there is room for
 * one, and the same inventory behind a drawer on phones. jsdom has no media-query engine, so
 * an absent or inert matchMedia leaves the drawer path in place.
 */
export function useRemoteDesktopSidebar(): boolean {
  const [desktop, setDesktop] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
    const query = window.matchMedia(REMOTE_DESKTOP_SIDEBAR_QUERY);
    return Boolean(query) && query.matches === true;
  });
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(REMOTE_DESKTOP_SIDEBAR_QUERY);
    if (!query || typeof query.addEventListener !== "function") return;
    setDesktop(query.matches === true);
    const onChange = (event: MediaQueryListEvent) => setDesktop(event.matches === true);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return desktop;
}

function focusTerminalInput(): void {
  if (typeof document === "undefined") return;
  const sink = document.querySelector<HTMLTextAreaElement>(
    'textarea[data-testid="remote-terminal-input-sink"]'
  );
  sink?.focus({ preventScroll: true });
}

function record(value: unknown): UnknownRecord | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as UnknownRecord)
    : null;
}

function records(value: unknown): UnknownRecord[] {
  return Array.isArray(value) ? value.map(record).filter((item): item is UnknownRecord => item !== null) : [];
}

function safeContextText(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  if (!text) return null;
  if (/^(?:~[/\\]|[/\\]|[a-zA-Z]:[/\\]|file:)/.test(text)) return null;
  if (/(?:^|\s)(?:~[/\\]|[/\\](?:Users|Volumes|home|private|tmp|var|opt|etc)\b|[a-zA-Z]:[/\\])/.test(text)) {
    return null;
  }
  return text;
}

function safeEpoch(value: unknown): string | number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value !== "string") return null;
  const text = value.trim();
  return text.length > 0 ? text : null;
}

function terminal(value: unknown): RemoteTerminalItem | null {
  const item = record(value);
  const sessionId = safeContextText(item?.sessionId);
  if (!item || !sessionId || item.running === false) return null;
  return {
    sessionId,
    running: item.running !== false,
    daemonEpoch: safeEpoch(item.daemonEpoch ?? item.daemon_epoch),
    title: safeContextText(item.title ?? item.label),
    workspaceId: safeContextText(item.workspaceId),
    worktreeLabel: safeContextText(item.worktreeLabel),
  };
}

function parseActivityState(value: unknown): "working" | "waiting" | "done" | undefined {
  if (value === "working" || value === "waiting" || value === "done") {
    return value;
  }
  return undefined;
}

function attentionRank(state?: "working" | "waiting" | "done"): number {
  if (state === "waiting") return 3;
  if (state === "done") return 2;
  if (state === "working") return 1;
  return 0;
}

function tabItem(value: unknown): RemoteTerminalTabInfo | null {
  const item = record(value);
  const id = safeContextText(item?.id ?? item?.tabId);
  const rawLabel = item?.label ?? item?.tabLabel ?? item?.title;
  const label = safeContextText(rawLabel) ?? "Terminal";
  if (!id) return null;
  const activityState = parseActivityState(item?.activityState ?? item?.activity_state ?? item?.state);
  const agentType = safeContextText(item?.agentType ?? item?.agent_type) ?? undefined;
  const worktreeSlug = safeContextText(item?.worktreeSlug ?? item?.worktree_slug) ?? undefined;
  const worktreeLabel = safeContextText(item?.worktreeLabel ?? item?.worktree_label) ?? undefined;
  const sessionId = safeContextText(item?.sessionId ?? item?.session_id) ?? undefined;
  const daemonEpoch = safeEpoch(item?.daemonEpoch ?? item?.daemon_epoch);
  return {
    id,
    label,
    ...(activityState ? { activityState } : {}),
    ...(agentType ? { agentType } : {}),
    ...(worktreeSlug ? { worktreeSlug } : {}),
    ...(worktreeLabel ? { worktreeLabel } : {}),
    ...(sessionId ? { sessionId } : {}),
    ...(daemonEpoch !== null ? { daemonEpoch } : {}),
  };
}

function tabItems(value: unknown): RemoteTerminalTabInfo[] {
  return Array.isArray(value)
    ? value.map(tabItem).filter((item): item is RemoteTerminalTabInfo => item !== null)
    : [];
}

function contextOption(value: unknown, fallbackWorkspaceId: string | null): RemoteContextOption | null {
  const item = record(value);
  if (!item) return null;
  const workspaceId = safeContextText(item.workspaceId) ?? fallbackWorkspaceId;
  if (!workspaceId) return null;
  const worktreeSlug = safeContextText(item.worktreeSlug ?? item.slug);
  const worktreeLabel = safeContextText(item.worktreeLabel ?? item.label ?? item.branch);
  const tabId = safeContextText(item.tabId ?? item.tab_id);
  const sessionId = safeContextText(item.sessionId ?? item.session_id);
  const attention = parseActivityState(item.attention ?? item.activityState ?? item.activity_state);
  return {
    workspaceId,
    worktreeSlug,
    worktreeLabel: worktreeLabel ?? worktreeSlug,
    ...(tabId ? { tabId } : {}),
    ...(sessionId ? { sessionId } : {}),
    ...(attention ? { attention } : {}),
  };
}

function appendOption(options: RemoteContextOption[], option: RemoteContextOption | null) {
  if (!option) return;
  const key = `${option.workspaceId}\u0000${option.worktreeSlug ?? ""}\u0000${option.worktreeLabel ?? ""}`;
  const existing = options.find((candidate) =>
    `${candidate.workspaceId}\u0000${candidate.worktreeSlug ?? ""}\u0000${candidate.worktreeLabel ?? ""}` === key
  );
  if (!existing) {
    options.push(option);
  } else if (option.attention && (!existing.attention || attentionRank(option.attention) > attentionRank(existing.attention))) {
    existing.attention = option.attention;
  }
}

function optionFromGitWorktree(value: unknown, workspaceId: string): RemoteContextOption | null {
  const item = record(value);
  if (!item) return null;

  const explicitSlug = safeContextText(item.worktreeSlug ?? item.slug);
  const explicitLabel = safeContextText(item.worktreeLabel ?? item.label);
  const attention = parseActivityState(item.attention ?? item.activityState ?? item.activity_state);
  if (explicitSlug || explicitLabel) {
    return {
      workspaceId,
      worktreeSlug: explicitSlug,
      worktreeLabel: explicitLabel ?? explicitSlug,
      ...(attention ? { attention } : {}),
    };
  }

  const branch = safeContextText(item.branch)?.replace(/^refs\/heads\//, "") ?? null;
  if (!branch) return { workspaceId, worktreeSlug: null, worktreeLabel: null, ...(attention ? { attention } : {}) };
  // Managed branches are `<prefix>/<workspace>/<slug>` with the Ferryx or legacy Orca prefix.
  const managed = ["ferryx", "orca"].map((root) => `${root}/${workspaceId}/`).find((prefix) => branch.startsWith(prefix));
  const slug = managed ? branch.slice(managed.length) : null;
  return {
    workspaceId,
    worktreeSlug: safeContextText(slug),
    worktreeLabel: safeContextText(slug ?? branch),
    ...(attention ? { attention } : {}),
  };
}

/**
 * Accepts the current typed contract plus older state shapes. Explicit active
 * terminal fields always win. A legacy sessions array is used only when it has
 * exactly one entry; ambiguous/malformed arrays never become a session switcher.
 */
export function normalizeRemoteWorkspaceState(value: unknown): RemoteWorkspaceModel {
  const state = record(value) ?? {};
  const declaredContext =
    record(state.activeContext) ??
    record(state.activeSelection) ??
    record(state.selection) ??
    {};

  const workspaceId = safeContextText(
    declaredContext.workspaceId ?? state.activeWorkspaceId ?? state.workspaceId,
  );
  const worktreeSlug = safeContextText(
    declaredContext.worktreeSlug ?? declaredContext.slug ?? state.activeWorktreeSlug,
  );
  const worktreeLabel = safeContextText(
    declaredContext.worktreeLabel ?? declaredContext.label ?? state.activeWorktreeLabel,
  );

  const sessionRows = records(state.sessions);
  const hasExplicitTerminalDeclaration =
    "activeTerminal" in declaredContext ||
    "terminal" in declaredContext ||
    "activeTerminal" in state ||
    "focusedTerminal" in state;
  const explicitTerminal =
    terminal(declaredContext.activeTerminal) ??
    terminal(declaredContext.terminal) ??
    terminal(state.activeTerminal) ??
    terminal(state.focusedTerminal);
  const declaredSessionId = safeContextText(
    declaredContext.sessionId ?? state.activeSessionId ?? state.focusedSessionId,
  );
  const declaredSession = declaredSessionId
    ? terminal(sessionRows.find((item) => item.sessionId === declaredSessionId)) ?? {
        sessionId: declaredSessionId,
        running: true,
      }
    : null;
  const legacySingleSession =
    !hasExplicitTerminalDeclaration && sessionRows.length === 1 ? terminal(sessionRows[0]) : null;
  const activeTerminal = explicitTerminal ?? declaredSession ?? legacySingleSession;
  const activeTabId = safeContextText(
    declaredContext.tabId ?? declaredContext.activeTabId ?? state.activeTabId ?? state.tabId,
  );
  const declaredEpoch =
    activeTerminal?.daemonEpoch ?? safeEpoch(declaredContext.daemonEpoch ?? state.daemonEpoch);
  const terminalTabs = tabItems(
    declaredContext.terminalTabs ?? declaredContext.tabs ?? state.terminalTabs ?? state.tabs,
  );
  const activeWorkspaceId = workspaceId ?? safeContextText(activeTerminal?.workspaceId);
  const activeWorktreeSlug = worktreeSlug;
  const activeWorktreeLabel =
    worktreeLabel ?? safeContextText(activeTerminal?.worktreeLabel) ?? activeWorktreeSlug;

  const options: RemoteContextOption[] = [];
  for (const item of records(state.contexts ?? state.contextOptions ?? state.selections)) {
    appendOption(options, contextOption(item, null));
  }

  const projectRows = records(state.projects ?? state.workspaces);
  for (const project of projectRows) {
    const projectId = safeContextText(project.workspaceId ?? project.id);
    if (!projectId) continue;
    const projectWorktrees = records(project.worktrees ?? project.contexts);
    if (projectWorktrees.length === 0) {
      appendOption(options, { workspaceId: projectId, worktreeSlug: null, worktreeLabel: null });
      continue;
    }
    for (const item of projectWorktrees) appendOption(options, contextOption(item, projectId));
  }

  if (activeWorkspaceId) {
    for (const item of records(state.worktrees)) {
      appendOption(options, optionFromGitWorktree(item, activeWorkspaceId));
    }
    appendOption(options, {
      workspaceId: activeWorkspaceId,
      worktreeSlug: activeWorktreeSlug,
      worktreeLabel: activeWorktreeLabel,
    });
  }

  for (const project of projectRows) {
    const projectId = safeContextText(project.workspaceId ?? project.id);
    if (!projectId?.startsWith("ssh:")) continue;
    const sessions = sessionRows.map(terminal)
      .filter((session) => session?.workspaceId === projectId);
    sessions.forEach((session, index) => {
      if (!session) return;
      options.push({
        workspaceId: projectId,
        worktreeSlug: null,
        worktreeLabel: session.worktreeLabel ?? null,
        sessionId: session.sessionId,
        sessionLabel: session.title ?? `Terminal ${index + 1}`,
      });
    });
  }

  const sessionAttention: RemoteSessionAttention[] = [];
  for (const item of records(state.sessionAttention)) {
    const sessionId = safeContextText(item.backendSessionId);
    const entryWorkspaceId = safeContextText(item.workspaceId);
    const entryState = parseActivityState(item.state);
    if (!sessionId || !entryWorkspaceId || (entryState !== "waiting" && entryState !== "done")) continue;
    sessionAttention.push({
      sessionId,
      workspaceId: entryWorkspaceId,
      worktreeSlug: safeContextText(item.worktreeSlug),
      state: entryState,
      agentType: safeContextText(item.agentType),
    });
  }

  return {
    context: {
      workspaceId: activeWorkspaceId,
      worktreeSlug: activeWorktreeSlug,
      worktreeLabel: activeWorktreeLabel,
      activeTerminal,
      activeTabId,
      terminalTabs,
      daemonEpoch: declaredEpoch,
    },
    options,
    ...(sessionAttention.length > 0 ? { sessionAttention } : {}),
  };
}

export function contextName(context: Pick<RemoteContext, "workspaceId" | "worktreeLabel" | "worktreeSlug">) {
  const workspace = context.workspaceId ?? "No workspace selected";
  const worktree = context.worktreeLabel ?? context.worktreeSlug;
  // An SSH-host project id is an opaque hash; its root label is the project folder name.
  if (worktree && context.workspaceId?.startsWith("ssh:")) return worktree;
  return worktree ? `${workspace} / ${worktree}` : workspace;
}

export function getRemoteDocumentTitle(model: RemoteWorkspaceModel): string {
  if (!model.context.activeTerminal && (!model.context.terminalTabs || model.context.terminalTabs.length === 0)) {
    return "Ferryx";
  }
  const activeTab = model.context.terminalTabs?.find(
    (tab) => tab.id === model.context.activeTabId,
  );
  const title =
    activeTab?.label ??
    model.context.activeTerminal?.title ??
    model.context.terminalTabs?.[0]?.label ??
    (model.context.activeTerminal ? "Terminal" : null);

  return title ? `${title} - Ferryx` : "Ferryx";
}


/** The remote supervisor view the sidebar's Supervisor tab shows. */
export type RemoteSidebarSupervisor = { readonly typing: boolean; readonly content: ReactNode };

type RemoteWorkspaceMirrorProps = {
  model: RemoteWorkspaceModel;
  pending: RemoteContextOption | null;
  selectorOpen: boolean;
  onSelectorOpenChange: (open: boolean) => void;
  onOpenHosts?: () => void;
  /** Account mode: replaces the host (machine) action with a neutral account sign-out. Not shown
   * inline on the phone home, where the Account sheet owns sign-out. */
  onSignOut?: () => void;
  onSelect: (option: RemoteContextOption) => void;
  onCreateTerminal?: () => void;
  onCreateWorktree?: () => void;
  /** Phone: the no-terminal state offers a way back to the chat view. */
  onBackToChat?: () => void;
  creationError?: string | null;
  /** Machine that owns model.context; account options from other machines are never current. */
  activeMachineId?: string | null;
  /** Each machine's sidebar as its desktop publishes it, one section per machine. */
  sidebarSections?: readonly RemoteSidebarSection[];
  /** Account mode: the Supervisor tab's content; absent hides the tab. */
  supervisor?: RemoteSidebarSupervisor;
  onSidebarViewChange?: (view: SidebarViewId) => void;
  /** Inventory loading/offline/error rows, rendered above the tree. */
  pickerStatus?: ReactNode;
  /**
   * Phone home only: render the sidebar as a full-width page body instead of a drawer, without
   * the view tabs, terminal tab strip or close control. `children` is ignored.
   */
  inline?: boolean;
  children?: ReactNode;
};

/** The sidebar inventory exactly as RemoteWorkspaceMirror builds it, so callers outside
 * the component (swipe navigation) reason over the same rendered sequence. */
export function mirrorInventory(
  model: RemoteWorkspaceModel,
  activeMachineId: string | null,
): RemoteInventoryModel {
  return buildRemoteInventory({
    options: model.options,
    panes: model.context.terminalTabs ?? [],
    context: {
      workspaceId: model.context.workspaceId,
      worktreeSlug: model.context.worktreeSlug,
      worktreeLabel: model.context.worktreeLabel,
      activeTabId: model.context.activeTabId ?? null,
      activeSessionId: model.context.activeTerminal?.sessionId ?? null,
      daemonEpoch: model.context.activeTerminal?.daemonEpoch ?? model.context.daemonEpoch ?? null,
    },
    activeMachineId,
  });
}

/** Tab rows in the exact order the tablist renders them: grouped entries first, then
 * orphan panes. Ordinals, arrows, and swipes must all describe this sequence. */
export function renderedPaneOrder(inventory: RemoteInventoryModel): RemoteTerminalTabInfo[] {
  const order: RemoteTerminalTabInfo[] = [];
  for (const project of inventory.projects) {
    for (const worktree of project.worktrees) {
      for (const entry of worktree.entries) {
        if (entry.kind === "pane") order.push(entry.tab);
        else if (entry.kind === "session" && entry.pane) order.push(entry.pane);
      }
    }
  }
  order.push(...inventory.orphanPanes);
  return order;
}

/** Pure pane-to-option mapping behind the picker rows: a pane's own declared workspace and
 * slug win over the active context, undeclared fields inherit it, and daemonEpoch travels
 * only when the pane declares one. Extracted verbatim from the row click so the compact
 * session sheet can reuse it. */
export function paneSelectionOption(
  model: RemoteWorkspaceModel,
  tab: RemoteTerminalTabInfo,
): RemoteContextOption | null {
  const workspaceId = declaresOwnProperty(tab, "workspaceId")
    ? tab.workspaceId ?? null
    : model.context.workspaceId;
  if (!workspaceId) return null;
  const declaredSlug = declaresOwnProperty(tab, "worktreeSlug");
  const worktreeSlug = declaredSlug ? tab.worktreeSlug ?? null : model.context.worktreeSlug;
  const worktreeLabel = declaredSlug
    ? tab.worktreeLabel ?? worktreeSlug
    : tab.worktreeLabel ?? model.context.worktreeLabel;
  return {
    workspaceId,
    worktreeSlug,
    worktreeLabel,
    tabId: tab.id,
    sessionId: tab.sessionId,
    ...(tab.daemonEpoch !== null && tab.daemonEpoch !== undefined
      ? { daemonEpoch: tab.daemonEpoch }
      : {}),
  };
}

/** The option a sidebar row selects: the inventory's own option for that worktree when one
 * exists (it carries the account machine), otherwise the row's identity. */
export function sidebarRowOption(model: RemoteWorkspaceModel, target: RemoteSidebarTarget): RemoteContextOption {
  const { node } = target;
  const accountMachines = model.options.some((option) => option.machineId !== undefined);
  const known = model.options.find((option) =>
    !option.sessionId &&
    option.workspaceId === node.workspaceId &&
    option.worktreeSlug === node.worktreeSlug &&
    (!accountMachines || option.machineId === target.machineId));
  if (known) return known;
  return {
    workspaceId: node.workspaceId,
    worktreeSlug: node.worktreeSlug,
    worktreeLabel: node.worktreeSlug ?? node.label,
    ...(accountMachines ? { machineId: target.machineId } : {}),
  };
}

/**
 * The option a desktop-published Inbox row opens, read-only and never creating a terminal: that
 * machine's session by the pane's backend session id, otherwise the pane's worktree, otherwise the
 * workspace (its root worktree first). The row's own `sessionId` is the desktop's local pane id and
 * is never matched against the inventory.
 */
export function publishedAttentionOption(model: RemoteWorkspaceModel, target: PublishedAttentionTarget): RemoteContextOption {
  const { machineId, row } = target;
  const accountMachines = model.options.some((option) => option.machineId !== undefined);
  const onMachine = (option: RemoteContextOption) =>
    !option.unavailable && option.workspaceId === row.workspaceId && (!accountMachines || option.machineId === machineId);
  const attachOnly = (option: RemoteContextOption): RemoteContextOption => ({ ...option, attachOnly: true });
  const session = row.backendSessionId
    ? model.options.find((option) => onMachine(option) && option.sessionId === row.backendSessionId)
    : undefined;
  if (session) return attachOnly(session);
  const worktrees = model.options.filter((option) => onMachine(option) && !option.sessionId);
  const known = (row.worktreeSlug !== undefined ? worktrees.find((option) => option.worktreeSlug === row.worktreeSlug) : undefined) ??
    worktrees.find((option) => option.worktreeSlug === null) ?? worktrees[0];
  if (known) return attachOnly(known);
  return attachOnly({ workspaceId: row.workspaceId, worktreeSlug: null, worktreeLabel: null, ...(accountMachines ? { machineId } : {}) });
}

const PHONE_ACTION =
  "flex h-11 shrink-0 items-center gap-1.5 rounded-lg bg-chat-surface-raised px-3 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50";

export const RemoteWorkspaceMirror: React.FC<RemoteWorkspaceMirrorProps> = ({
  model,
  pending,
  selectorOpen,
  onSelectorOpenChange,
  onOpenHosts,
  onSignOut,
  onSelect,
  onCreateTerminal,
  onCreateWorktree,
  onBackToChat,
  creationError,
  activeMachineId = null,
  sidebarSections = [],
  supervisor,
  onSidebarViewChange,
  pickerStatus,
  inline = false,
  children,
}) => {
  const desktopSidebar = useRemoteDesktopSidebar();
  const paneTabs = model.context.terminalTabs ?? [];
  const inventory = useMemo(() => mirrorInventory(model, activeMachineId), [activeMachineId, model]);
  const tabs = useMemo(
    () => stripTabs(inventory, model.context.activeTabId ?? null, {
      workspaceId: model.context.workspaceId,
      worktreeSlug: model.context.worktreeSlug,
      worktreeLabel: model.context.worktreeLabel,
      activeMachineId,
    }),
    [activeMachineId, inventory, model.context],
  );

  const selectOption = (option: RemoteContextOption) => {
    onSelect(option);
    focusTerminalInput();
  };
  const selectTab = (tab: RemoteStripTab) => {
    const option = tab.pane ? paneSelectionOption(model, tab.pane) : tab.option;
    if (option) selectOption(option);
  };

  const [view, setView] = useState<SidebarViewId>("worktrees");
  useEffect(() => {
    if (!selectorOpen && !desktopSidebar) setView("worktrees");
  }, [desktopSidebar, selectorOpen]);
  const changeView = (next: SidebarViewId) => {
    setView(next);
    onSidebarViewChange?.(next);
  };

  const attentionRows = model.context.workspaceId
    ? buildRemoteAttentionRows(model.context.workspaceId, model.context.workspaceId, paneTabs, model.context.activeTabId)
    : [];
  const openAttentionRow = (row: AttentionRow) => {
    const tab = paneTabs.find((candidate) => candidate.id === row.id);
    const option = tab ? paneSelectionOption(model, tab) : null;
    if (!option) return;
    selectOption(option);
    onSelectorOpenChange(false);
  };

  const openPublishedAttention = (target: PublishedAttentionTarget) => {
    if (pending !== null) return;
    onSelectorOpenChange(false);
    selectOption(publishedAttentionOption(model, target));
  };

  // The phone drawer is worktrees only: the phone home owns the one Inbox and the one Supervisor.
  const phoneDrawer = !inline && !desktopSidebar;
  const drawerRef = useRef<HTMLDivElement | null>(null);
  const drawerBackdropRef = useRef<HTMLButtonElement | null>(null);
  const drawerOpen = phoneDrawer && selectorOpen;
  useOverlayHistory(drawerOpen, () => onSelectorOpenChange(false));
  // Swipe left to dismiss. The drawer unmounts on close with no exit animation, so the hook slides
  // it out from where the finger left it and closes once it is off screen.
  const { handlers: drawerDragHandlers } = useDragDismiss({
    axis: "x-left",
    panelRef: drawerRef,
    backdropRef: drawerBackdropRef,
    active: drawerOpen,
    deferDismiss: true,
    onDismiss: () => onSelectorOpenChange(false),
  });
  useEffect(() => {
    if (!drawerOpen) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    drawerRef.current?.querySelector<HTMLElement>("button, input, [tabindex]:not([tabindex='-1'])")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onSelectorOpenChange(false);
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      if (previous?.isConnected) previous.focus();
    };
  }, [drawerOpen, onSelectorOpenChange]);
  const trapTab = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Tab") return;
    const focusables = Array.from(drawerRef.current?.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex='-1'])") ?? []);
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const sidebarEmpty = sidebarSections.length === 0;
  const busyTab = pending?.sessionId ? tabs.find((tab) => tab.sessionId === pending.sessionId)?.key ?? null : null;
  const createDisabled = pending !== null || !model.context.workspaceId;

  const sidebarBody = (
    <>
      {inline || desktopSidebar ? null : (
        <div className="flex h-12 shrink-0 items-center justify-end px-1">
          <IconButton label="Close worktree list" size="touch" onClick={() => onSelectorOpenChange(false)}>
            <X className="size-4" aria-hidden="true" />
          </IconButton>
        </div>
      )}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <RemoteSidebar
          sections={sidebarSections}
          density={desktopSidebar ? "compact" : "touch"}
          pending={pending ? { machineId: pending.machineId ?? null, workspaceId: pending.workspaceId, worktreeSlug: pending.worktreeSlug } : null}
          onSelect={(target) => {
            onSelectorOpenChange(false);
            selectOption(sidebarRowOption(model, target));
          }}
          view={inline || phoneDrawer ? "worktrees" : view}
          {...(inline || phoneDrawer ? {} : { onViewChange: changeView, inbox: { rows: attentionRows, onOpen: openAttentionRow }, onOpenPublishedAttention: openPublishedAttention })}
          {...(supervisor && !inline && !phoneDrawer ? { supervisor } : {})}
          status={pickerStatus}
        />
        {sidebarEmpty && !pickerStatus ? (
          desktopSidebar ? (
            <p className="px-2 py-6 text-center text-[11px] text-muted-foreground">No selectable desktop worktrees are available.</p>
          ) : (
            <div data-testid="remote-worktrees-empty" className="flex flex-col items-center gap-3 px-4 py-8 text-center">
              <p className="text-sm text-chat-foreground-secondary">No worktrees to open yet. Open Ferryx on your computer to list its projects.</p>
              {onCreateTerminal ? (
                <button type="button" disabled={createDisabled} onClick={onCreateTerminal} className={PHONE_ACTION}>
                  <TerminalIcon className="size-4" aria-hidden="true" />
                  New terminal
                </button>
              ) : null}
            </div>
          )
        ) : null}
      </div>
      {desktopSidebar ? null : (
        <div className="flex shrink-0 items-center justify-end gap-2 border-t border-chat-border px-3 py-2">
          {onOpenHosts && !onSignOut ? (
            <button type="button" onClick={onOpenHosts} className={cn(PHONE_ACTION, "mr-auto")}>
              <Server className="size-4" aria-hidden="true" />
              Computers
            </button>
          ) : null}
          {onCreateWorktree ? (
            <button type="button" disabled={createDisabled} onClick={onCreateWorktree} className={PHONE_ACTION}>
              <GitBranch className="size-4" aria-hidden="true" />
              New worktree
            </button>
          ) : null}
          {inline && onCreateTerminal ? (
            <button type="button" disabled={createDisabled} onClick={onCreateTerminal} className={PHONE_ACTION}>
              <Plus className="size-4" aria-hidden="true" />
              New terminal
            </button>
          ) : null}
        </div>
      )}
      {desktopSidebar ? (
      <div className="flex h-8 shrink-0 items-center justify-between gap-1 border-t border-worktree-sidebar-border px-1.5">
        {onSignOut ? (
          <IconButton label="Sign out" onClick={onSignOut}>
            <LogOut className="size-3.5" aria-hidden="true" />
          </IconButton>
        ) : onOpenHosts ? (
          <IconButton label="Machines" onClick={onOpenHosts}>
            <Server className="size-3.5" aria-hidden="true" />
          </IconButton>
        ) : (
          <span />
        )}
        <div className="flex items-center gap-1">
          {onCreateWorktree ? (
            <IconButton label="New worktree" disabled={createDisabled} onClick={onCreateWorktree}>
              <GitBranch className="size-3.5" aria-hidden="true" />
            </IconButton>
          ) : null}
        </div>
      </div>
      ) : null}
    </>
  );

  if (inline) {
    return (
      <div
        data-testid="remote-worktrees-inline"
        aria-label="Workspace context"
        role="region"
        className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen text-chat-foreground"
      >
        {sidebarBody}
        {creationError ? <p role="alert" className="shrink-0 px-3 py-2 text-sm text-chat-danger">{creationError}</p> : null}
      </div>
    );
  }

  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 overflow-hidden bg-background">
      {desktopSidebar ? (
        <nav
          data-testid="remote-desktop-sidebar"
          aria-label="Workspace context"
          className="flex min-h-0 w-[236px] shrink-0 flex-col border-r border-worktree-sidebar-border bg-worktree-sidebar text-worktree-sidebar-foreground"
        >
          {sidebarBody}
        </nav>
      ) : selectorOpen ? (
        <>
          <button
            ref={drawerBackdropRef}
            type="button"
            aria-label="Dismiss worktree list"
            onClick={() => onSelectorOpenChange(false)}
            className="absolute inset-0 z-30 bg-chat-screen/70"
          />
          <div
            ref={drawerRef}
            data-testid="remote-sidebar-drawer"
            role="dialog"
            aria-modal="true"
            aria-label="Workspace context"
            onKeyDown={trapTab}
            {...drawerDragHandlers}
            // The shared sidebar body stops pointerdown from bubbling (desktop reorder), so a swipe
            // that starts on a row is only seen in the capture phase.
            onPointerDown={undefined}
            onPointerDownCapture={drawerDragHandlers.onPointerDown}
            style={{ touchAction: "pan-y" }}
            className="absolute inset-y-0 left-0 z-40 flex min-h-0 w-[88%] max-w-[340px] animate-drawer-in flex-col border-r border-chat-border bg-chat-screen text-chat-foreground motion-reduce:animate-none"
          >
            {sidebarBody}
          </div>
        </>
      ) : null}

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <RemoteTerminalTabStrip
          tabs={tabs}
          paneCount={paneTabs.length}
          locked={pending !== null}
          busyKey={busyTab}
          onSelect={selectTab}
          {...(onCreateTerminal ? { onCreateTerminal } : {})}
          createDisabled={createDisabled}
        />
        {creationError ? <p role="alert" className="shrink-0 px-3 py-2 text-xs text-destructive">{creationError}</p> : null}

        <div className="flex min-h-0 flex-1 flex-col">
          {children ? (
            children
          ) : (
            <div className="flex flex-1 items-center justify-center p-6">
              {desktopSidebar ? (
                <div className="max-w-sm text-center">
                  <span className="mx-auto flex size-12 items-center justify-center rounded-lg border border-border bg-input text-muted-foreground">
                    <PanelLeft className="size-5" aria-hidden="true" />
                  </span>
                  <h2 className="mt-4 text-sm font-semibold text-foreground">No focused terminal</h2>
                  <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                    {paneTabs.length > 0
                      ? "Pick a terminal from the tabs to mirror it here. Browser tabs stay private."
                      : "Open a terminal in Ferryx Desktop to mirror it here. Browser tabs stay private."}
                  </p>
                </div>
              ) : (
                <div data-testid="remote-terminal-empty" className="flex max-w-sm flex-col items-center gap-3 text-center">
                  <h2 className="text-base font-semibold text-chat-foreground">No focused terminal</h2>
                  <p className="text-sm leading-relaxed text-chat-foreground-secondary">
                    {paneTabs.length > 0 ? "Pick a terminal from the tabs above." : "There's no terminal open in this worktree yet."}
                  </p>
                  <div className="flex flex-wrap justify-center gap-2">
                    <button type="button" onClick={() => onSelectorOpenChange(true)} className={PHONE_ACTION}>
                      <FolderTree className="size-4" aria-hidden="true" />
                      Open worktrees
                    </button>
                    {onCreateTerminal ? (
                      <button type="button" disabled={createDisabled} onClick={onCreateTerminal} className={PHONE_ACTION}>
                        <Plus className="size-4" aria-hidden="true" />
                        New terminal
                      </button>
                    ) : null}
                    {onBackToChat ? (
                      <button type="button" onClick={onBackToChat} className={PHONE_ACTION}>
                        Back to chat
                      </button>
                    ) : null}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
