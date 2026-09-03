import {
  Check,
  ChevronLeft,
  ChevronRight,
  GitBranch,
  LoaderCircle,
  Terminal as TerminalIcon,
  X,
} from "lucide-react";
import React, { useMemo, type ReactNode } from "react";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";

export type RemoteTerminalTabInfo = {
  id: string;
  label: string;
  activityState?: "working" | "waiting" | "done";
  agentType?: string;
  worktreeSlug?: string;
  worktreeLabel?: string;
  sessionId?: string;
};

export type RemoteTerminalItem = {
  sessionId: string;
  running?: boolean;
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
};

export type RemoteContextOption = {
  workspaceId: string;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
  tabId?: string | null;
  sessionId?: string | null;
  attention?: "working" | "waiting" | "done";
};

export type RemoteWorkspaceModel = {
  context: RemoteContext;
  options: RemoteContextOption[];
};

type UnknownRecord = Record<string, unknown>;

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

function terminal(value: unknown): RemoteTerminalItem | null {
  const item = record(value);
  const sessionId = safeContextText(item?.sessionId);
  if (!item || !sessionId || item.running === false) return null;
  return {
    sessionId,
    running: item.running !== false,
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
  if (state === "working") return 2;
  if (state === "done") return 1;
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
  return {
    id,
    label,
    ...(activityState ? { activityState } : {}),
    ...(agentType ? { agentType } : {}),
    ...(worktreeSlug ? { worktreeSlug } : {}),
    ...(worktreeLabel ? { worktreeLabel } : {}),
    ...(sessionId ? { sessionId } : {}),
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

  return {
    context: {
      workspaceId: activeWorkspaceId,
      worktreeSlug: activeWorktreeSlug,
      worktreeLabel: activeWorktreeLabel,
      activeTerminal,
      activeTabId,
      terminalTabs,
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
  const name = contextName(option);
  return option.attention ? `${name} (${option.attention})` : name;
}

function isCurrentOption(option: RemoteContextOption, context: RemoteContext) {
  if (option.workspaceId !== context.workspaceId) return false;
  const optionWorktree = option.worktreeSlug ?? option.worktreeLabel;
  const activeWorktree = context.worktreeSlug ?? context.worktreeLabel;
  return optionWorktree ? optionWorktree === activeWorktree : activeWorktree === null;
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
  onSelect: (option: RemoteContextOption) => void;
  children?: ReactNode;
};

export const RemoteWorkspaceMirror: React.FC<RemoteWorkspaceMirrorProps> = ({
  model,
  pending,
  selectorOpen,
  onSelectorOpenChange,
  onSelect,
  children,
}) => {
  const groupedOptions = useMemo(() => {
    const groups = new Map<string, RemoteContextOption[]>();
    for (const option of model.options) {
      const group = groups.get(option.workspaceId) ?? [];
      group.push(option);
      groups.set(option.workspaceId, group);
    }
    return [...groups.entries()];
  }, [model.options]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col bg-background">
      {selectorOpen ? (
        <div className="absolute inset-x-2 top-1.5 z-20 rounded-lg border border-border bg-popover text-popover-foreground shadow-xl" role="dialog" aria-label="Workspace context">
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <div>
              <h2 className="text-sm font-semibold">Choose worktree</h2>
            </div>
            <button
              type="button"
              aria-label="Close workspace context"
              onClick={() => onSelectorOpenChange(false)}
              className="flex size-9 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <X className="size-4" aria-hidden="true" />
            </button>
          </div>
          <div className="max-h-96 overflow-y-auto p-2 scrollbar-sleek">
            {groupedOptions.length === 0 ? (
              <p className="px-3 py-6 text-center text-xs text-muted-foreground">
                No selectable desktop contexts are available.
              </p>
            ) : (
              groupedOptions.map(([workspaceId, options]) => {
                const projectAttention = options.reduce<"working" | "waiting" | "done" | undefined>((acc, opt) => {
                  if (!opt.attention) return acc;
                  if (!acc || attentionRank(opt.attention) > attentionRank(acc)) return opt.attention;
                  return acc;
                }, undefined);

                return (
                  <section key={workspaceId} className="mb-2 last:mb-0" aria-label={workspaceId}>
                    <div className="flex items-center justify-between px-2 py-1">
                      <h3 className="text-xs font-semibold text-muted-foreground">{workspaceId}</h3>
                      {projectAttention === "working" ? (
                        <LoaderCircle
                          aria-hidden="true"
                          className="size-3 animate-spin text-status-working motion-reduce:animate-none"
                        />
                      ) : projectAttention === "waiting" ? (
                        <span
                          aria-hidden="true"
                          className="size-1.5 rounded-full bg-status-warning ring-2 ring-status-warning/20"
                        />
                      ) : projectAttention === "done" ? (
                        <span
                          aria-hidden="true"
                          className="size-1.5 rounded-full bg-status-success"
                        />
                      ) : null}
                    </div>
                    <div className="space-y-1">
                      {options.map((option) => {
                        const active = isCurrentOption(option, model.context);
                        const loading = pending
                          ? pending.workspaceId === option.workspaceId &&
                            pending.worktreeSlug === option.worktreeSlug &&
                            pending.worktreeLabel === option.worktreeLabel
                          : false;
                        const worktree = option.worktreeLabel ?? option.worktreeSlug;
                        return (
                          <button
                            key={`${option.workspaceId}:${option.worktreeSlug ?? option.worktreeLabel ?? "workspace"}`}
                            type="button"
                            aria-current={active ? "true" : undefined}
                            aria-label={optionName(option)}
                            disabled={pending !== null}
                            onClick={() => {
                              onSelectorOpenChange(false);
                              onSelect(option);
                              focusTerminalInput();
                            }}
                            className="flex min-h-11 w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60"
                          >
                            <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-secondary text-muted-foreground">
                              {loading ? (
                                <LoaderCircle className="size-4 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                              ) : (
                                <GitBranch className="size-4" aria-hidden="true" />
                              )}
                            </span>
                            <span className="min-w-0 flex-1">
                              <span className="block truncate font-mono text-sm font-medium">
                                {worktree ?? "Primary worktree"}
                              </span>
                            </span>
                            {option.attention === "working" ? (
                              <LoaderCircle
                                aria-hidden="true"
                                data-testid="worktree-working-indicator"
                                className="size-3.5 shrink-0 animate-spin text-status-working motion-reduce:animate-none"
                              />
                            ) : option.attention === "waiting" ? (
                              <span
                                aria-hidden="true"
                                data-testid="worktree-waiting-indicator"
                                className="size-2 shrink-0 rounded-full bg-status-warning ring-2 ring-status-warning/20"
                              />
                            ) : option.attention === "done" ? (
                              <span
                                aria-hidden="true"
                                data-testid="worktree-done-indicator"
                                className="size-2 shrink-0 rounded-full bg-status-success"
                              />
                            ) : null}
                            {active ? <Check className="size-4 shrink-0 text-status-success" aria-label="Active" /> : null}
                          </button>
                        );
                      })}
                    </div>
                  </section>
                );
              })
            )}
          </div>
        </div>
      ) : null}

      {model.context.terminalTabs && model.context.terminalTabs.length > 0 ? (() => {
        const tabs = model.context.terminalTabs;
        const activeIdx = tabs.findIndex((tab) => tab.id === model.context.activeTabId);
        const currentIndex = activeIdx >= 0 ? activeIdx : 0;
        const currentOrdinal = currentIndex + 1;

        return (
          <div className="flex items-center border-b border-border bg-card px-1.5 py-0.5">
            <div className="flex items-center gap-0.5 shrink-0 pr-1 border-r border-border">
              <button
                type="button"
                aria-label="Previous terminal tab"
                disabled={pending !== null || currentIndex <= 0}
                onClick={() => {
                  const prevTab = tabs[currentIndex - 1];
                  if (prevTab && model.context.workspaceId) {
                    onSelect({
                      workspaceId: model.context.workspaceId,
                      worktreeSlug: prevTab.worktreeSlug ?? model.context.worktreeSlug,
                      worktreeLabel: prevTab.worktreeLabel ?? model.context.worktreeLabel,
                      tabId: prevTab.id,
                      sessionId: prevTab.sessionId,
                    });
                    focusTerminalInput();
                  }
                }}
                className="relative flex size-7 touch-manipulation items-center justify-center rounded text-muted-foreground transition-colors before:absolute before:-inset-1 before:content-[''] hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
              >
                <ChevronLeft className="size-3.5" aria-hidden="true" />
              </button>
              <span
                className="px-1 text-[11px] font-mono font-medium text-muted-foreground select-none"
                aria-label={`Terminal position: Tab ${currentOrdinal} of ${tabs.length}`}
              >
                {currentOrdinal} / {tabs.length}
              </span>
              <button
                type="button"
                aria-label="Next terminal tab"
                disabled={pending !== null || currentIndex >= tabs.length - 1}
                onClick={() => {
                  const nextTab = tabs[currentIndex + 1];
                  if (nextTab && model.context.workspaceId) {
                    onSelect({
                      workspaceId: model.context.workspaceId,
                      worktreeSlug: nextTab.worktreeSlug ?? model.context.worktreeSlug,
                      worktreeLabel: nextTab.worktreeLabel ?? model.context.worktreeLabel,
                      tabId: nextTab.id,
                      sessionId: nextTab.sessionId,
                    });
                    focusTerminalInput();
                  }
                }}
                className="relative flex size-7 touch-manipulation items-center justify-center rounded text-muted-foreground transition-colors before:absolute before:-inset-1 before:content-[''] hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40"
              >
                <ChevronRight className="size-3.5" aria-hidden="true" />
              </button>
            </div>
            <div
              role="tablist"
              aria-label="Terminal tabs"
              className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto px-1.5 scrollbar-sleek"
            >
              {tabs.map((tab) => {
                const isActive = tab.id === model.context.activeTabId;
                // Panes from every worktree are listed, so the worktree disambiguates same-named tabs.
                const foreignWorktree =
                  tab.worktreeLabel && tab.worktreeLabel !== model.context.worktreeLabel
                    ? tab.worktreeLabel
                    : null;
                const tabDescription = foreignWorktree ? `${tab.label} - ${foreignWorktree}` : tab.label;
                const tabAriaLabel = tab.activityState
                  ? `${tabDescription} (${tab.activityState})`
                  : tabDescription;
                const logo = resolveAgentLogo(tab.agentType);
                const isMonochrome = isMonochromeAgentLogo(tab.agentType);

                return (
                  <button
                    key={tab.id}
                    role="tab"
                    aria-selected={isActive}
                    aria-label={tabAriaLabel}
                    disabled={pending !== null}
                    onClick={() => {
                      if (isActive || !model.context.workspaceId) {
                        if (isActive) {
                          focusTerminalInput();
                        }
                        return;
                      }
                      onSelect({
                        workspaceId: model.context.workspaceId,
                        worktreeSlug: tab.worktreeSlug ?? model.context.worktreeSlug,
                        worktreeLabel: tab.worktreeLabel ?? model.context.worktreeLabel,
                        tabId: tab.id,
                        sessionId: tab.sessionId,
                      });
                      focusTerminalInput();
                    }}
                    className={`flex h-7 min-w-0 max-w-40 items-center gap-1.5 rounded px-2 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 ${
                      isActive
                        ? "bg-accent text-accent-foreground font-semibold"
                        : "text-muted-foreground hover:bg-accent/50 hover:text-foreground"
                    }`}
                  >
                    {logo ? (
                      <img
                        src={logo}
                        alt=""
                        aria-hidden="true"
                        data-testid="tab-agent-icon"
                        data-agent-type={tab.agentType}
                        className={`size-3 shrink-0 ${isMonochrome ? "agent-tab-logo--monochrome opacity-80" : ""}`}
                      />
                    ) : (
                      <TerminalIcon data-testid="tab-terminal-icon" className="size-3 shrink-0 opacity-70" aria-hidden="true" />
                    )}
                    <span className="truncate">{tab.label}</span>
                    {foreignWorktree ? (
                      <span className="shrink-0 truncate text-[10px] font-normal opacity-60">{foreignWorktree}</span>
                    ) : null}
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
              })}
            </div>
          </div>
        );
      })() : null}

      <div className="flex min-h-0 flex-1 flex-col">
        {model.context.activeTerminal ? (
          children
        ) : (
          <div className="flex flex-1 items-center justify-center p-6">
            <div className="max-w-sm text-center">
              <span className="mx-auto flex size-12 items-center justify-center rounded-lg border border-border bg-card text-muted-foreground">
                <TerminalIcon className="size-5" aria-hidden="true" />
              </span>
              <h2 className="mt-4 text-sm font-semibold text-foreground">No focused terminal</h2>
              <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                {model.context.terminalTabs && model.context.terminalTabs.length > 0
                  ? "Pick a terminal from the list above to mirror it here. Browser tabs stay private."
                  : "Open a terminal in Ferryx Desktop to mirror it here. Browser tabs stay private."}
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
