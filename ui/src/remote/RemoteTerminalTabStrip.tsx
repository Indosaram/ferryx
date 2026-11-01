// The terminals of the attached worktree as a tab strip above the mirrored terminal, like the
// desktop's tab bar. The stepper and ordinal describe the strip's own order; on touch every
// control grows to a 44px target.
import type { KeyboardEvent } from "react";
import { Check, ChevronLeft, ChevronRight, LoaderCircle, Plus, Terminal as TerminalIcon } from "lucide-react";

import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";
import { cn } from "../lib/cn";
import { selectionOption, type RemoteInventoryActivityState, type RemoteInventoryModel } from "./remoteInventoryView";
import type { RemoteContextOption, RemoteTerminalTabInfo } from "./RemoteSessionList";

/** One strip tab: a pane the desktop has open, or a running session of this worktree. */
export type RemoteStripTab = {
  readonly key: string;
  readonly label: string;
  /** Accessible name: the label, the foreign worktree when it differs, then the activity. */
  readonly ariaLabel: string;
  readonly sessionId: string | null;
  readonly agentType: string | null;
  readonly activity: RemoteInventoryActivityState | null;
  readonly active: boolean;
  readonly option: RemoteContextOption | null;
  readonly pane: RemoteTerminalTabInfo | null;
};

type StripContext = {
  readonly workspaceId: string | null;
  readonly worktreeSlug: string | null;
  readonly worktreeLabel: string | null;
  readonly activeMachineId: string | null;
};

function ariaLabel(label: string, foreignWorktree: string | null, activity: RemoteInventoryActivityState | null) {
  const description = foreignWorktree ? `${label} - ${foreignWorktree}` : label;
  return activity ? `${description} (${activity})` : description;
}

function paneTab(tab: RemoteTerminalTabInfo, active: boolean, contextLabel: string | null): RemoteStripTab {
  const foreign = tab.worktreeLabel && tab.worktreeLabel !== contextLabel ? tab.worktreeLabel : null;
  const activity = tab.activityState ?? null;
  return {
    key: `pane:${tab.id}`,
    label: tab.label,
    ariaLabel: ariaLabel(tab.label, foreign, activity),
    sessionId: tab.sessionId ?? null,
    agentType: tab.agentType ?? null,
    activity,
    active,
    option: null,
    pane: tab,
  };
}

/**
 * Every desktop pane in rendered inventory order, plus the running sessions of the attached
 * worktree that no pane shows yet; sessions of other worktrees live behind their sidebar row.
 */
export function stripTabs(inventory: RemoteInventoryModel, activeTabId: string | null, context: StripContext): RemoteStripTab[] {
  const tabs: RemoteStripTab[] = [];
  for (const project of inventory.projects) {
    const machineMatches = context.activeMachineId === null || project.machineId === null || project.machineId === context.activeMachineId;
    for (const worktree of project.worktrees) {
      const current = machineMatches && worktree.workspaceId === context.workspaceId && (worktree.slug ?? null) === (context.worktreeSlug ?? null);
      for (const entry of worktree.entries) {
        if (entry.kind === "pane") tabs.push(paneTab(entry.tab, entry.tab.id === activeTabId, context.worktreeLabel));
        else if (entry.pane) tabs.push(paneTab(entry.pane, entry.pane.id === activeTabId, context.worktreeLabel));
        else if (current) {
          tabs.push({
            key: entry.key,
            label: entry.label,
            ariaLabel: ariaLabel(entry.label, null, entry.activity),
            sessionId: entry.sessionId,
            agentType: entry.option.agentType ?? null,
            activity: entry.activity,
            active: entry.active,
            option: selectionOption(entry.selection),
            pane: null,
          });
        }
      }
    }
  }
  for (const tab of inventory.orphanPanes) tabs.push(paneTab(tab, tab.id === activeTabId, context.worktreeLabel));
  return tabs;
}

function ActivityGlyph({ activity }: { readonly activity: RemoteInventoryActivityState | null }) {
  if (activity === "working") {
    return <LoaderCircle aria-hidden="true" data-testid="tab-working-indicator" className="size-2.5 shrink-0 animate-spin text-status-working motion-reduce:animate-none" />;
  }
  if (activity === "waiting") {
    return (
      <span aria-hidden="true" data-testid="tab-waiting-indicator" className="flex shrink-0 items-center gap-1 text-status-warning">
        <span className="size-2 rounded-full bg-current" />
        <span className="hidden text-xs [@media(pointer:coarse)]:inline">Needs you</span>
      </span>
    );
  }
  if (activity === "done") {
    return (
      <span aria-hidden="true" data-testid="tab-done-indicator" className="flex shrink-0 items-center gap-1 text-status-success">
        <Check className="size-3" />
        <span className="hidden text-xs [@media(pointer:coarse)]:inline">Done</span>
      </span>
    );
  }
  return null;
}

const STEP_BUTTON =
  "flex size-6 shrink-0 touch-manipulation items-center justify-center rounded text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40 [@media(pointer:coarse)]:size-11";

/** Arrow keys, Home and End move focus between tabs; the browser's focus follows the roving tabindex. */
function moveTabFocus(event: KeyboardEvent<HTMLDivElement>) {
  const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]:not(:disabled)'));
  if (tabs.length === 0) return;
  const current = tabs.findIndex((tab) => tab === document.activeElement);
  let next: number;
  switch (event.key) {
    case "ArrowRight":
      next = current < 0 ? 0 : (current + 1) % tabs.length;
      break;
    case "ArrowLeft":
      next = current < 0 ? tabs.length - 1 : (current - 1 + tabs.length) % tabs.length;
      break;
    case "Home":
      next = 0;
      break;
    case "End":
      next = tabs.length - 1;
      break;
    default:
      return;
  }
  event.preventDefault();
  tabs[next]?.focus();
}

export type RemoteTerminalTabStripProps = {
  readonly tabs: readonly RemoteStripTab[];
  /** Pane count the desktop published; the ordinal's denominator. */
  readonly paneCount: number;
  readonly locked: boolean;
  readonly busyKey: string | null;
  readonly onSelect: (tab: RemoteStripTab) => void;
  readonly onCreateTerminal?: () => void;
  readonly createDisabled: boolean;
};

export function RemoteTerminalTabStrip({ tabs, paneCount, locked, busyKey, onSelect, onCreateTerminal, createDisabled }: RemoteTerminalTabStripProps) {
  const panes = tabs.filter((tab) => tab.pane !== null);
  const index = panes.findIndex((tab) => tab.active);
  const ordinal = index + 1;
  const focusableKey = tabs.find((tab) => tab.active)?.key ?? tabs[0]?.key ?? null;
  return (
    <div data-testid="remote-terminal-tab-strip" className="flex h-8 shrink-0 items-center gap-0.5 border-b border-border bg-card px-1 [@media(pointer:coarse)]:h-12">
      <button
        type="button"
        aria-label="Previous terminal tab"
        disabled={locked || index <= 0}
        onClick={() => {
          const previous = panes[index - 1];
          if (previous) onSelect(previous);
        }}
        className={STEP_BUTTON}
      >
        <ChevronLeft className="size-3.5" aria-hidden="true" />
      </button>
      {index >= 0 ? (
        <span
          className="shrink-0 select-none px-1 font-mono text-xs font-medium text-muted-foreground"
          aria-label={`Terminal position: Tab ${ordinal} of ${paneCount}`}
        >
          {`${ordinal} / ${paneCount}`}
        </span>
      ) : null}
      <button
        type="button"
        aria-label="Next terminal tab"
        disabled={locked || index < 0 || index >= panes.length - 1}
        onClick={() => {
          const next = panes[index + 1];
          if (next) onSelect(next);
        }}
        className={STEP_BUTTON}
      >
        <ChevronRight className="size-3.5" aria-hidden="true" />
      </button>
      <div role="tablist" aria-label="Terminal tabs" onKeyDown={moveTabFocus} className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto overflow-y-hidden scrollbar-sleek">
        {tabs.map((tab) => {
          const logo = resolveAgentLogo(tab.agentType ?? undefined);
          const busy = busyKey === tab.key;
          return (
            <button
              key={tab.key}
              type="button"
              role="tab"
              data-session-id={tab.sessionId ?? undefined}
              aria-selected={tab.active}
              tabIndex={tab.key === focusableKey ? 0 : -1}
              aria-label={tab.ariaLabel}
              aria-busy={busy || undefined}
              disabled={locked}
              onClick={() => onSelect(tab)}
              className={cn(
                "flex h-6 max-w-48 shrink-0 items-center gap-1.5 rounded-md px-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-60 [@media(pointer:coarse)]:h-11 [@media(pointer:coarse)]:px-3",
                tab.active ? "bg-foreground/10 text-foreground" : "text-muted-foreground hover:bg-foreground/5 hover:text-foreground",
              )}
            >
              {busy ? (
                <LoaderCircle aria-hidden="true" className="size-3 shrink-0 animate-spin motion-reduce:animate-none" />
              ) : logo ? (
                <img
                  src={logo}
                  alt=""
                  data-testid="tab-agent-icon"
                  data-agent-type={tab.agentType ?? undefined}
                  className={cn("size-3 shrink-0", isMonochromeAgentLogo(tab.agentType ?? undefined) && "agent-tab-logo--monochrome opacity-80")}
                />
              ) : (
                <TerminalIcon data-testid="tab-terminal-icon" aria-hidden="true" className="size-3 shrink-0 opacity-70" />
              )}
              <span className="min-w-0 truncate text-xs leading-tight [@media(pointer:coarse)]:text-sm">{tab.label}</span>
              <ActivityGlyph activity={tab.activity} />
            </button>
          );
        })}
      </div>
      {onCreateTerminal ? (
        <button
          type="button"
          data-testid="remote-new-terminal"
          aria-label="New terminal tab"
          disabled={createDisabled}
          onClick={onCreateTerminal}
          className={STEP_BUTTON}
        >
          <Plus className="size-3.5" aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
