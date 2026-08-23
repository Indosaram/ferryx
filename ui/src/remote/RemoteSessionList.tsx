import {
  Check,
  ChevronDown,
  GitBranch,
  LoaderCircle,
  Monitor,
  Terminal as TerminalIcon,
  X,
} from "lucide-react";
import React, { useMemo, useState, type ReactNode } from "react";

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
};

export type RemoteContextOption = {
  workspaceId: string;
  worktreeSlug: string | null;
  worktreeLabel: string | null;
};

export type RemoteWorkspaceModel = {
  context: RemoteContext;
  options: RemoteContextOption[];
};

type UnknownRecord = Record<string, unknown>;

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
    workspaceId: safeContextText(item.workspaceId),
    worktreeLabel: safeContextText(item.worktreeLabel),
  };
}

function contextOption(value: unknown, fallbackWorkspaceId: string | null): RemoteContextOption | null {
  const item = record(value);
  if (!item) return null;
  const workspaceId = safeContextText(item.workspaceId) ?? fallbackWorkspaceId;
  if (!workspaceId) return null;
  const worktreeSlug = safeContextText(item.worktreeSlug ?? item.slug);
  const worktreeLabel = safeContextText(item.worktreeLabel ?? item.label ?? item.branch);
  return { workspaceId, worktreeSlug, worktreeLabel: worktreeLabel ?? worktreeSlug };
}

function appendOption(options: RemoteContextOption[], option: RemoteContextOption | null) {
  if (!option) return;
  const key = `${option.workspaceId}\u0000${option.worktreeSlug ?? ""}\u0000${option.worktreeLabel ?? ""}`;
  if (!options.some((candidate) =>
    `${candidate.workspaceId}\u0000${candidate.worktreeSlug ?? ""}\u0000${candidate.worktreeLabel ?? ""}` === key
  )) {
    options.push(option);
  }
}

function optionFromGitWorktree(value: unknown, workspaceId: string): RemoteContextOption | null {
  const item = record(value);
  if (!item) return null;

  const explicitSlug = safeContextText(item.worktreeSlug ?? item.slug);
  const explicitLabel = safeContextText(item.worktreeLabel ?? item.label);
  if (explicitSlug || explicitLabel) {
    return {
      workspaceId,
      worktreeSlug: explicitSlug,
      worktreeLabel: explicitLabel ?? explicitSlug,
    };
  }

  const branch = safeContextText(item.branch)?.replace(/^refs\/heads\//, "") ?? null;
  if (!branch) return { workspaceId, worktreeSlug: null, worktreeLabel: null };
  const prefix = `orca/${workspaceId}/`;
  const slug = branch.startsWith(prefix) ? branch.slice(prefix.length) : null;
  return {
    workspaceId,
    worktreeSlug: safeContextText(slug),
    worktreeLabel: safeContextText(slug ?? branch),
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
    },
    options,
  };
}

function contextName(context: Pick<RemoteContext, "workspaceId" | "worktreeLabel" | "worktreeSlug">) {
  const workspace = context.workspaceId ?? "No workspace selected";
  const worktree = context.worktreeLabel ?? context.worktreeSlug;
  return worktree ? `${workspace} / ${worktree}` : workspace;
}

function optionName(option: RemoteContextOption) {
  return contextName(option);
}

function isCurrentOption(option: RemoteContextOption, context: RemoteContext) {
  if (option.workspaceId !== context.workspaceId) return false;
  const optionWorktree = option.worktreeSlug ?? option.worktreeLabel;
  const activeWorktree = context.worktreeSlug ?? context.worktreeLabel;
  return optionWorktree ? optionWorktree === activeWorktree : activeWorktree === null;
}

type RemoteWorkspaceMirrorProps = {
  model: RemoteWorkspaceModel;
  pending: RemoteContextOption | null;
  statusMessage: string | null;
  onSelect: (option: RemoteContextOption) => void;
  children?: ReactNode;
};

export const RemoteWorkspaceMirror: React.FC<RemoteWorkspaceMirrorProps> = ({
  model,
  pending,
  statusMessage,
  onSelect,
  children,
}) => {
  const [selectorOpen, setSelectorOpen] = useState(false);
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
      <div className="border-b border-border bg-card p-2">
        <button
          type="button"
          aria-label="Change workspace context"
          aria-expanded={selectorOpen}
          onClick={() => setSelectorOpen((open) => !open)}
          className="flex h-11 w-full items-center gap-3 rounded-md border border-border bg-background px-3 text-left transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-secondary text-muted-foreground">
            <Monitor className="size-4" aria-hidden="true" />
          </span>
          <span className="min-w-0 flex-1" aria-label="Current desktop context">
            <span className="block text-xs font-medium text-foreground">Desktop context</span>
            <span className="block truncate font-mono text-xs text-muted-foreground">
              {contextName(model.context)}
            </span>
          </span>
          <ChevronDown
            aria-hidden="true"
            className={`size-4 shrink-0 text-muted-foreground transition-transform ${selectorOpen ? "rotate-180" : ""}`}
          />
        </button>
        {statusMessage ? (
          <div role="status" aria-live="polite" className="mt-2 flex items-center gap-2 px-1 text-xs text-muted-foreground">
            {pending ? (
              <LoaderCircle className="size-3.5 animate-spin text-status-working motion-reduce:animate-none" aria-hidden="true" />
            ) : (
              <Check className="size-3.5 text-status-success" aria-hidden="true" />
            )}
            <span>{statusMessage}</span>
          </div>
        ) : null}
      </div>

      {selectorOpen ? (
        <div className="absolute inset-x-2 top-2 z-20 rounded-lg border border-border bg-popover text-popover-foreground shadow-xl" role="dialog" aria-label="Workspace context">
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <div>
              <h2 className="text-sm font-semibold">Choose desktop context</h2>
              <p className="text-xs text-muted-foreground">Ferryx Desktop confirms the active terminal.</p>
            </div>
            <button
              type="button"
              aria-label="Close workspace context"
              onClick={() => setSelectorOpen(false)}
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
              groupedOptions.map(([workspaceId, options]) => (
                <section key={workspaceId} className="mb-2 last:mb-0" aria-label={workspaceId}>
                  <h3 className="px-2 py-1 text-xs font-semibold text-muted-foreground">{workspaceId}</h3>
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
                          onClick={() => onSelect(option)}
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
                            <span className="block truncate text-sm font-medium">{workspaceId}</span>
                            <span className="block truncate font-mono text-xs text-muted-foreground">
                              {worktree ?? "Primary worktree"}
                            </span>
                          </span>
                          {active ? <Check className="size-4 shrink-0 text-status-success" aria-label="Active" /> : null}
                        </button>
                      );
                    })}
                  </div>
                </section>
              ))
            )}
          </div>
        </div>
      ) : null}

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
                Focus a terminal in Ferryx Desktop to mirror it here. Browser tabs and background terminals stay private.
              </p>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
