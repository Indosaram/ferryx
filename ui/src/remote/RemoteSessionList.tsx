import {
  ChevronLeft,
  ChevronRight,
  GitBranch,
  Inbox,
  LoaderCircle,
  LogOut,
  PanelLeft,
  Plus,
  Server,
  Terminal as TerminalIcon,
  X,
} from "lucide-react";
import React, { useEffect, useMemo, useState, type ReactNode } from "react";
import { IconButton } from "../components/ui/IconButton";
import { AttentionInbox } from "../features/ferryx/attention/AttentionInbox";
import { buildRemoteAttentionRows, type AttentionRow } from "../features/ferryx/attention/attentionModel";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";
import { cn } from "../lib/cn";
import {
  RemoteInventoryProjectRow,
  RemoteInventorySessionRow,
  RemoteInventoryWorktreeRow,
} from "./remoteInventoryRows";
import {
  buildRemoteInventory,
  declaresOwnProperty,
  selectionOption,
  type RemoteInventoryEntry,
  type RemoteInventoryModel,
  type RemoteInventorySelectionTarget,
  type RemoteInventoryWorktree,
} from "./remoteInventoryView";

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
};

export type RemoteWorkspaceModel = {
  context: RemoteContext;
  options: RemoteContextOption[];
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
  const prefix = `orca/${workspaceId}/`;
  const slug = branch.startsWith(prefix) ? branch.slice(prefix.length) : null;
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
  };
}

export function contextName(context: Pick<RemoteContext, "workspaceId" | "worktreeLabel" | "worktreeSlug">) {
  const workspace = context.workspaceId ?? "No workspace selected";
  const worktree = context.worktreeLabel ?? context.worktreeSlug;
  return worktree ? `${workspace} / ${worktree}` : workspace;
}

function optionName(option: RemoteContextOption) {
  const name = `${contextName(option)}${option.sessionLabel ? ` / ${option.sessionLabel}` : ""}`;
  return option.attention ? `${name} (${option.attention})` : name;
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

type RemoteWorkspaceMirrorProps = {
  model: RemoteWorkspaceModel;
  pending: RemoteContextOption | null;
  selectorOpen: boolean;
  onSelectorOpenChange: (open: boolean) => void;
  onOpenHosts?: () => void;
  /** Account mode: replaces the host (machine) action with a neutral account sign-out. */
  onSignOut?: () => void;
  onSelect: (option: RemoteContextOption) => void;
  onCreateTerminal?: () => void;
  onCreateWorktree?: () => void;
  creationError?: string | null;
  /** Machine that owns model.context; account options from other machines are never current. */
  activeMachineId?: string | null;
  /** Inventory loading/offline/error rows, rendered only inside the opened picker. */
  pickerStatus?: ReactNode;
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
  creationError,
  activeMachineId = null,
  pickerStatus,
  children,
}) => {
  const desktopSidebar = useRemoteDesktopSidebar();
  const paneTabs = model.context.terminalTabs ?? [];

  const inventory = useMemo(
    () => mirrorInventory(model, activeMachineId),
    [activeMachineId, model, paneTabs],
  );
  // The position arrows and ordinal describe the rows the tablist actually renders as
  // selected, in render order — never the desktop tab-strip order, and never a first-row
  // fallback when no rendered row holds the selection.
  const renderedPanes = useMemo(() => renderedPaneOrder(inventory), [inventory]);
  const currentPaneIndex = renderedPanes.findIndex((tab) => tab.id === model.context.activeTabId);
  const currentPaneOrdinal = currentPaneIndex >= 0 ? currentPaneIndex + 1 : 0;

  const selectOption = (option: RemoteContextOption) => {
    onSelect(option);
    focusTerminalInput();
  };

  const selectPane = (tab: RemoteTerminalTabInfo) => {
    const workspaceId = declaresOwnProperty(tab, "workspaceId")
      ? tab.workspaceId ?? null
      : model.context.workspaceId;
    if (!workspaceId) return;
    const declaredSlug = declaresOwnProperty(tab, "worktreeSlug");
    const worktreeSlug = declaredSlug ? tab.worktreeSlug ?? null : model.context.worktreeSlug;
    const worktreeLabel = declaredSlug
      ? tab.worktreeLabel ?? worktreeSlug
      : tab.worktreeLabel ?? model.context.worktreeLabel;
    selectOption({
      workspaceId,
      worktreeSlug,
      worktreeLabel,
      tabId: tab.id,
      sessionId: tab.sessionId,
      ...(tab.daemonEpoch !== null && tab.daemonEpoch !== undefined
        ? { daemonEpoch: tab.daemonEpoch }
        : {}),
    });
  };

  const selectSelection = (selection: RemoteInventorySelectionTarget) => {
    selectOption(selectionOption(selection));
  };

  const isPendingOption = (option: RemoteContextOption) =>
    pending
      ? (pending.machineId ?? null) === (option.machineId ?? null) &&
        pending.workspaceId === option.workspaceId &&
        pending.worktreeSlug === option.worktreeSlug &&
        pending.worktreeLabel === option.worktreeLabel &&
        pending.sessionId === option.sessionId
      : false;

  const attentionRows = model.context.workspaceId
    ? buildRemoteAttentionRows(model.context.workspaceId, model.context.workspaceId, paneTabs, model.context.activeTabId)
    : [];
  const openAttentionRow = (row: AttentionRow) => {
    const tab = paneTabs.find((candidate) => candidate.id === row.id);
    if (!tab) return;
    selectPane(tab);
    onSelectorOpenChange(false);
  };
  // The worktree list is the sheet's default; the inbox is a sub view the header icon opens.
  const [inboxOpen, setInboxOpen] = useState(false);
  useEffect(() => {
    if (!selectorOpen) setInboxOpen(false);
  }, [selectorOpen]);

  const renderPaneRow = (tab: RemoteTerminalTabInfo, siblingWorktreeLabel: string | null | undefined) => {
    const logo = resolveAgentLogo(tab.agentType);
    const monochrome = isMonochromeAgentLogo(tab.agentType);
    const paneActive = tab.id === model.context.activeTabId;
    // The worktree disambiguates same-named panes, exactly as the old tab strip did.
    const foreignWorktree =
      tab.worktreeLabel && tab.worktreeLabel !== siblingWorktreeLabel ? tab.worktreeLabel : null;
    const tabDescription = foreignWorktree ? `${tab.label} - ${foreignWorktree}` : tab.label;
    const tabAriaLabel = tab.activityState
      ? `${tabDescription} (${tab.activityState})`
      : tabDescription;
    return (
      <button
        key={tab.id}
        type="button"
        role="tab"
        data-session-id={tab.sessionId ?? undefined}
        aria-selected={paneActive}
        aria-label={tabAriaLabel}
        disabled={pending !== null}
        onClick={() => selectPane(tab)}
        className={cn(
          "flex min-h-[24px] w-full items-center gap-1.5 rounded-md py-0.5 pl-6 pr-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60",
          paneActive ? "bg-white/[0.06] text-foreground" : "text-worktree-sidebar-foreground/85 hover:bg-white/[0.04]",
        )}
      >
        {logo ? (
          <img
            src={logo}
            alt=""
            data-testid="tab-agent-icon"
            data-agent-type={tab.agentType}
            className={cn("size-3 shrink-0", monochrome && "agent-tab-logo--monochrome opacity-80")}
          />
        ) : (
          <TerminalIcon data-testid="tab-terminal-icon" className="size-3 shrink-0 opacity-70" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 truncate text-[11px] leading-tight">{tab.label}</span>
        {tab.activityState === "working" ? (
          <LoaderCircle
            aria-hidden="true"
            data-testid="tab-working-indicator"
            className="size-2.5 shrink-0 animate-spin text-status-working motion-reduce:animate-none"
          />
        ) : tab.activityState === "waiting" ? (
          <span
            aria-hidden="true"
            data-testid="tab-waiting-indicator"
            className="size-1.5 shrink-0 rounded-full bg-status-warning ring-2 ring-status-warning/20"
          />
        ) : tab.activityState === "done" ? (
          <span
            aria-hidden="true"
            data-testid="tab-done-indicator"
            className="size-1.5 shrink-0 rounded-full bg-status-success"
          />
        ) : null}
      </button>
    );
  };

  const renderEntry = (entry: RemoteInventoryEntry) => {
    if (entry.kind === "pane") {
      return renderPaneRow(entry.tab, model.context.worktreeLabel);
    }
    if (entry.pane) {
      return renderPaneRow(entry.pane, model.context.worktreeLabel);
    }
    return (
      <RemoteInventorySessionRow
        key={entry.key}
        testId="remote-session-row"
        label={entry.label}
        sessionId={entry.sessionId}
        activity={entry.activity}
        agentType={entry.option.agentType ?? null}
        active={entry.active}
        disabled={pending !== null}
        busy={isPendingOption(entry.option)}
        onSelect={() => {
          onSelectorOpenChange(false);
          selectSelection(entry.selection);
        }}
      />
    );
  };

  const renderWorktree = (worktree: RemoteInventoryWorktree) => {
    const selectable = worktree.option ?? selectionOption(worktree.selection);
    // The project header already names the project, so the row shows the worktree alone; the root
    // worktree never inherits the active context's worktree name.
    const visibleLabel = worktree.isRootWorktree
      ? "root"
      : worktree.label ?? worktree.slug ?? "Worktree";
    return (
      <div key={worktree.key} data-testid="remote-worktree-group">
        <RemoteInventoryWorktreeRow
          label={optionName(selectable)}
          visibleLabel={visibleLabel}
          isRootWorktree={worktree.isRootWorktree}
          active={worktree.active}
          disabled={pending !== null}
          busy={isPendingOption(selectable)}
          activity={worktree.activity}
          hasSessions={worktree.entries.length > 0}
          onSelect={() => {
            onSelectorOpenChange(false);
            selectSelection(worktree.selection);
          }}
        />
        {worktree.entries.length > 0 ? (
          <div className="mt-px" data-testid="remote-session-rows" data-worktree={visibleLabel}>
            {worktree.entries.map((entry) => renderEntry(entry))}
          </div>
        ) : null}
      </div>
    );
  };

  const inventoryEmpty = inventory.projects.length === 0 && inventory.orphanPanes.length === 0;

  const sidebarBody = (
    <>
      <div className="flex h-8 items-center justify-between border-b border-worktree-sidebar-border px-1.5">
        <button
          type="button"
          aria-label={attentionRows.length > 0 ? `Inbox (${attentionRows.length})` : "Inbox"}
          aria-pressed={inboxOpen}
          onClick={() => setInboxOpen((open) => !open)}
          className={cn(
            "relative flex size-6 items-center justify-center rounded-md transition-colors hover:bg-worktree-sidebar-accent hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            inboxOpen ? "bg-white/[0.08] text-worktree-sidebar-foreground" : "text-muted-foreground",
          )}
        >
          <Inbox className="size-3.5" aria-hidden="true" />
          {attentionRows.length > 0 ? (
            <span
              data-testid="remote-attention-count"
              className="absolute -right-0.5 -top-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-status-warning px-1 text-[9px] font-semibold leading-none text-black"
            >
              {attentionRows.length > 9 ? "9+" : attentionRows.length}
            </span>
          ) : null}
        </button>
        {desktopSidebar ? null : (
          <button
            type="button"
            aria-label="Close worktree list"
            onClick={() => onSelectorOpenChange(false)}
            className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-worktree-sidebar-accent hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <X className="size-3.5" aria-hidden="true" />
          </button>
        )}
      </div>
      {inboxOpen ? (
        <div data-testid="remote-attention-inbox" className="flex max-h-96 min-h-0 flex-col">
          <AttentionInbox rows={attentionRows} onOpen={openAttentionRow} compact />
        </div>
      ) : (
        <>
          <div className="flex h-7 shrink-0 items-center gap-0.5 border-b border-worktree-sidebar-border px-1">
            <button
              type="button"
              aria-label="Previous terminal tab"
              disabled={pending !== null || currentPaneIndex <= 0}
              onClick={() => {
                const prevTab = renderedPanes[currentPaneIndex - 1];
                if (prevTab) selectPane(prevTab);
              }}
              className="relative flex size-6 touch-manipulation items-center justify-center rounded text-muted-foreground transition-colors hover:bg-worktree-sidebar-accent hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
            >
              <ChevronLeft className="size-3.5" aria-hidden="true" />
            </button>
            <span
              className="px-1 text-[11px] font-mono font-medium text-muted-foreground select-none"
              aria-label={
                currentPaneIndex >= 0
                  ? `Terminal position: Tab ${currentPaneOrdinal} of ${paneTabs.length}`
                  : `Terminal position: unknown of ${paneTabs.length}`
              }
            >
              {currentPaneIndex >= 0 ? `${currentPaneOrdinal} / ${paneTabs.length}` : `? / ${paneTabs.length}`}
            </span>
            <button
              type="button"
              aria-label="Next terminal tab"
              disabled={
                pending !== null ||
                currentPaneIndex < 0 ||
                currentPaneIndex >= renderedPanes.length - 1
              }
              onClick={() => {
                const nextTab = renderedPanes[currentPaneIndex + 1];
                if (nextTab) selectPane(nextTab);
              }}
              className="relative flex size-6 touch-manipulation items-center justify-center rounded text-muted-foreground transition-colors hover:bg-worktree-sidebar-accent hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
            >
              <ChevronRight className="size-3.5" aria-hidden="true" />
            </button>
          </div>
          <div
            role="tablist"
            aria-label="Terminal tabs"
            className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto p-1.5 scrollbar-sleek"
          >
            {pickerStatus}
            {inventoryEmpty ? (
              pickerStatus ? null : (
                <p className="px-2 py-6 text-center text-[11px] text-muted-foreground">
                  No selectable desktop worktrees are available.
                </p>
              )
            ) : (
              inventory.projects.map((project) => (
                <section
                  key={project.key}
                  data-project-key={project.key}
                  className="mb-0.5 last:mb-0"
                  aria-label={project.workspaceId}
                >
                  <RemoteInventoryProjectRow label={project.label} activity={project.activity} />
                  <div data-testid="remote-project-worktrees">
                    {project.worktrees.map((worktree) => renderWorktree(worktree))}
                  </div>
                </section>
              ))
            )}
            {inventory.orphanPanes.length > 0 ? (
              <section className="mb-0.5 last:mb-0" aria-label="Other panes">
                <div className="flex h-7 items-center gap-1.5 rounded-md px-2 text-[11px] font-medium text-worktree-sidebar-foreground/65">
                  <h3 className="min-w-0 flex-1 truncate">Other panes</h3>
                </div>
                <div>{inventory.orphanPanes.map((tab) => renderPaneRow(tab, model.context.worktreeLabel))}</div>
              </section>
            ) : null}
          </div>
        </>
      )}
      {onCreateTerminal ? (
        <div className="border-t border-worktree-sidebar-border p-1">
          <button
            type="button"
            data-testid="remote-new-terminal"
            disabled={pending !== null || !model.context.workspaceId}
            onClick={onCreateTerminal}
            className="flex h-7 w-full items-center gap-1.5 rounded-md px-2 text-left text-[11px] font-medium text-worktree-sidebar-foreground transition-colors hover:bg-worktree-sidebar-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
          >
            <Plus className="size-3 shrink-0" aria-hidden="true" />
            <span className="min-w-0 flex-1 truncate">New terminal</span>
          </button>
        </div>
      ) : null}
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
            <IconButton
              label="New worktree"
              disabled={pending !== null || !model.context.workspaceId}
              onClick={onCreateWorktree}
            >
              <GitBranch className="size-3.5" aria-hidden="true" />
            </IconButton>
          ) : null}
          {onCreateTerminal ? (
            <IconButton
              label="New terminal tab"
              disabled={pending !== null || !model.context.workspaceId}
              onClick={onCreateTerminal}
            >
              <Plus className="size-3.5" aria-hidden="true" />
            </IconButton>
          ) : null}
        </div>
      </div>
    </>
  );

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
            type="button"
            aria-label="Dismiss worktree list"
            onClick={() => onSelectorOpenChange(false)}
            className="absolute inset-0 z-30 bg-black/50"
          />
          <div
            data-testid="remote-sidebar-drawer"
            role="dialog"
            aria-label="Workspace context"
            className="absolute inset-y-0 left-0 z-40 flex min-h-0 w-[85%] max-w-[280px] flex-col border-r border-worktree-sidebar-border bg-worktree-sidebar text-worktree-sidebar-foreground shadow-xl"
          >
            {sidebarBody}
          </div>
        </>
      ) : null}

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {creationError ? <p role="alert" className="shrink-0 px-3 py-2 text-xs text-destructive">{creationError}</p> : null}

        <div className="flex min-h-0 flex-1 flex-col">
          {children ? (
            children
          ) : (
            <div className="flex flex-1 items-center justify-center p-6">
              <div className="max-w-sm text-center">
                <span className="mx-auto flex size-12 items-center justify-center rounded-lg border border-border bg-input text-muted-foreground">
                  {desktopSidebar ? (
                    <PanelLeft className="size-5" aria-hidden="true" />
                  ) : (
                    <TerminalIcon className="size-5" aria-hidden="true" />
                  )}
                </span>
                <h2 className="mt-4 text-sm font-semibold text-foreground">No focused terminal</h2>
                <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                  {model.context.terminalTabs && model.context.terminalTabs.length > 0
                    ? "Pick a terminal from the list to mirror it here. Browser tabs stay private."
                    : "Open a terminal in Ferryx Desktop to mirror it here. Browser tabs stay private."}
                </p>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
