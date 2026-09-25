import {
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Bell, TerminalSquare, Trash2, X } from "lucide-react";

import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";
import { notificationCenterStore, type NotificationCenterStore } from "../../lib/notificationCenter/notificationCenterStore";
import {
  filterEntries,
  groupEntries,
  type AttentionFilter,
} from "../../lib/notificationCenter/attentionView";
import type { NotificationEntry } from "../../lib/notificationCenter/types";
import { StatusDot, type StatusDotState } from "../ui/StatusDot";
import { AttentionInbox } from "../../features/ferryx/control/AttentionInbox";
import { targetKey, type Agent } from "../../features/ferryx/control/client";
import {
  buildDesktopInventory,
  isUnreadAgent,
  type DesktopWorkspace,
} from "../../features/ferryx/control/desktopInventory";
import { AttentionAskPanel } from "./AttentionAskPanel";
import { useNotificationCenter } from "./useNotificationCenter";

export const POPOVER_WIDTH = 380;

const PLACEMENT_GAP = 6;
const VIEWPORT_MARGIN = 8;
const MIN_POPOVER_HEIGHT = 120;

export interface PopoverPlacement {
  top?: number;
  bottom?: number;
  left: number;
  maxHeight: number;
}

/**
 * The popover must open toward the side with room. A trigger in the sidebar's top row leaves
 * almost nothing above it, so anchoring upward (the original bottom-strip behaviour) would place
 * the whole dialog off-screen and read as "the button does nothing".
 */
export function resolvePopoverPlacement(
  anchor: { top: number; bottom: number; left: number },
  viewport: { width: number; height: number },
  popoverWidth: number,
): PopoverPlacement {
  const spaceAbove = anchor.top - PLACEMENT_GAP - VIEWPORT_MARGIN;
  const spaceBelow = viewport.height - anchor.bottom - PLACEMENT_GAP - VIEWPORT_MARGIN;
  const placeBelow = spaceBelow >= spaceAbove;
  const maxLeft = Math.max(VIEWPORT_MARGIN, viewport.width - popoverWidth - VIEWPORT_MARGIN);
  const left = Math.min(Math.max(VIEWPORT_MARGIN, anchor.left), maxLeft);
  const maxHeight = Math.max(MIN_POPOVER_HEIGHT, placeBelow ? spaceBelow : spaceAbove);

  return placeBelow
    ? { top: anchor.bottom + PLACEMENT_GAP, left, maxHeight }
    : { bottom: viewport.height - anchor.top + PLACEMENT_GAP, left, maxHeight };
}

export function formatNotificationLocation(workspaceLabel?: string, worktreeLabel?: string): string {
  const ws = workspaceLabel?.trim();
  const wt = worktreeLabel?.trim();
  if (ws && wt) {
    return ws === wt ? ws : `${ws} / ${wt}`;
  }
  return ws || wt || "";
}

export function formatRelativeTime(timestamp: number, now = Date.now()): string {
  const diff = Math.max(0, now - timestamp);
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function checkSessionNavigable(
  isSessionNavigable: IsSessionNavigable | undefined,
  workspaceId: string,
  sessionId: string,
): boolean {
  if (!isSessionNavigable) return true;
  if (isSessionNavigable.length >= 2) {
    return (isSessionNavigable as (ws: string, sid: string) => boolean)(workspaceId, sessionId);
  }
  return (isSessionNavigable as (sid: string) => boolean)(sessionId);
}

export type IsSessionNavigable =
  | ((sessionId: string) => boolean)
  | ((workspaceId: string, sessionId: string) => boolean);

export interface NotificationCenterPopoverProps {
  open?: boolean;
  anchorRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
  onNavigateToSession?: (target: { workspaceId: string; sessionId: string; revision: number }) => void;
  isSessionNavigable?: IsSessionNavigable;
  store?: NotificationCenterStore;
  attentionInventory?: {
    workspaces: DesktopWorkspace[];
    unavailableHosts?: readonly string[];
    onSelectAgent: (agent: Agent) => void;
  };
}

function reasonToStatusDotState(reason: NotificationEntry["reason"]): StatusDotState {
  if (reason === "waiting") return "waiting";
  if (reason === "done") return "done";
  return "unread";
}

export function NotificationCenterPopover({
  open = true,
  anchorRef,
  onClose,
  onNavigateToSession,
  isSessionNavigable,
  store = notificationCenterStore,
  attentionInventory,
}: NotificationCenterPopoverProps): ReactNode {
  const { entries, unreadCount, markAllRead, dismissEntry } = useNotificationCenter(store);
  const [tab, setTab] = useState<"attention" | "ask" | "agents">("attention");
  const [filter, setFilter] = useState<AttentionFilter>("all");
  const visibleEntries = filterEntries(entries, filter);
  const groups = groupEntries(visibleEntries);
  const modalRef = useRef<HTMLDivElement>(null);
  const previousActiveElementRef = useRef<HTMLElement | null>(null);

  const [popoverStyle, setPopoverStyle] = useState<CSSProperties>({
    bottom: 42,
    left: 8,
    maxHeight: 420,
  });

  const updatePosition = useCallback(() => {
    if (!anchorRef?.current) return;
    const rect = anchorRef.current.getBoundingClientRect();
    const popoverWidth =
      modalRef.current?.getBoundingClientRect().width ||
      modalRef.current?.offsetWidth ||
      POPOVER_WIDTH;
    setPopoverStyle(
      resolvePopoverPlacement(
        { top: rect.top, bottom: rect.bottom, left: rect.left },
        { width: window.innerWidth, height: window.innerHeight },
        popoverWidth,
      ),
    );
  }, [anchorRef]);

  useLayoutEffect(() => {
    updatePosition();
  }, [updatePosition]);

  useEffect(() => {
    if (!open) return;

    const handleResize = () => {
      updatePosition();
    };
    window.addEventListener("resize", handleResize);

    previousActiveElementRef.current = document.activeElement as HTMLElement | null;
    const focusTimer = setTimeout(() => {
      if (modalRef.current) {
        const focusable = modalRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length > 0) {
          focusable[0].focus();
        } else {
          modalRef.current.focus();
        }
      }
    }, 0);

    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key === "Tab" && modalRef.current) {
        const focusable = modalRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        );
        if (focusable.length > 0) {
          const first = focusable[0];
          const last = focusable[focusable.length - 1];
          if (event.shiftKey && document.activeElement === first) {
            event.preventDefault();
            last.focus();
          } else if (!event.shiftKey && document.activeElement === last) {
            event.preventDefault();
            first.focus();
          }
        }
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("resize", handleResize);
      clearTimeout(focusTimer);
      document.removeEventListener("keydown", onKeyDown, true);
      previousActiveElementRef.current?.focus();
    };
  }, [open, onClose, updatePosition]);

  if (!open) return null;

  return createPortal(
    <>
      <div
        data-testid="notification-center-backdrop"
        className="fixed inset-0 z-[100] bg-black/40"
        onClick={onClose}
        onKeyDown={(e) => e.stopPropagation()}
        aria-hidden="true"
      />
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-label="Notifications"
        tabIndex={-1}
        data-testid="notification-center-popover"
        className="fixed z-[101] flex w-[380px] max-w-[calc(100vw-1rem)] flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-2xl outline-none"
        style={popoverStyle}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-3.5 py-2.5 bg-muted/20">
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-xs font-semibold text-foreground">Notifications</span>
            {unreadCount > 0 ? (
              <span className="rounded-full bg-primary/15 px-1.5 py-0.5 text-[10px] font-medium text-primary leading-none">
                {unreadCount} unread
              </span>
            ) : null}
          </div>
          <div className="flex items-center gap-1 shrink-0">
            {tab === "attention" ? (
              <button
                type="button"
                onClick={() => markAllRead()}
                disabled={unreadCount === 0}
                className="rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40 transition-colors"
              >
                Mark all read
              </button>
            ) : null}
            <button
              type="button"
              aria-label="Close notifications"
              onClick={onClose}
              className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors ml-0.5"
            >
              <X className="size-3.5" />
            </button>
          </div>
        </div>

        {/* Tabs */}
        <div className="flex items-center gap-1 border-b border-border px-3 py-1.5">
          <button
            type="button"
            data-testid="notification-tab-attention"
            aria-pressed={tab === "attention"}
            onClick={() => setTab("attention")}
            className={
              tab === "attention"
                ? "rounded px-2 py-0.5 text-[11px] font-medium bg-primary/15 text-primary"
                : "rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
            }
          >
            Needs you
          </button>
          <button
            type="button"
            data-testid="notification-tab-ask"
            aria-pressed={tab === "ask"}
            onClick={() => setTab("ask")}
            className={
              tab === "ask"
                ? "rounded px-2 py-0.5 text-[11px] font-medium bg-primary/15 text-primary"
                : "rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
            }
          >
            Ask
          </button>
          {attentionInventory ? (
            <button
              type="button"
              data-testid="notification-tab-agents"
              aria-pressed={tab === "agents"}
              onClick={() => setTab("agents")}
              className={
                tab === "agents"
                  ? "rounded px-2 py-0.5 text-[11px] font-medium bg-primary/15 text-primary"
                  : "rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
              }
            >
              All agents
            </button>
          ) : null}
        </div>

        {tab === "attention" ? (
          <div className="flex items-center gap-1 px-3 py-1.5" role="group" aria-label="Filter">
            {([
              ["all", "All"],
              ["unread", "Unread"],
              ["needs-you", "Needs you"],
            ] as [AttentionFilter, string][]).map(([id, label]) => (
              <button
                key={id}
                type="button"
                data-testid={`notification-filter-${id}`}
                aria-pressed={filter === id}
                onClick={() => setFilter(id)}
                className={
                  filter === id
                    ? "rounded px-2 py-0.5 text-[11px] bg-accent text-foreground"
                    : "rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground"
                }
              >
                {label}
              </button>
            ))}
          </div>
        ) : null}

        {/* Content */}
        {tab === "ask" ? (
          <AttentionAskPanel className="flex min-h-0 flex-1 flex-col overflow-y-auto scrollbar-sleek" />
        ) : tab === "agents" && attentionInventory ? (
          <div className="min-h-0 flex-1 overflow-y-auto scrollbar-sleek">
            <AttentionInbox
              snapshot={buildDesktopInventory(
                attentionInventory.workspaces,
                attentionInventory.unavailableHosts ?? [],
              )}
              onSelect={(target) => {
                const live = buildDesktopInventory(
                  attentionInventory.workspaces,
                  attentionInventory.unavailableHosts ?? [],
                );
                const agent = live.items.find((item) => targetKey(item.target) === targetKey(target));
                if (agent) attentionInventory.onSelectAgent(agent);
              }}
              isUnread={(agent) => isUnreadAgent(agent, attentionInventory.workspaces)}
            />
          </div>
        ) : entries.length === 0 ? (
          <div
            data-testid="notification-empty-state"
            className="flex flex-col items-center justify-center p-8 text-center text-xs text-muted-foreground"
          >
            No new notifications
          </div>
        ) : groups.length === 0 ? (
          <div
            data-testid="notification-filter-empty-state"
            className="flex flex-col items-center justify-center p-8 text-center text-xs text-muted-foreground"
          >
            Nothing matches this filter
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto scrollbar-sleek">
            {groups.map((group) => (
              <section key={group.section} data-testid={`notification-section-${group.section}`}>
                <h3 className="px-3 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
                  {group.title} <span className="font-normal">{group.items.length}</span>
                </h3>
                <div className="flex flex-col divide-y divide-border/40">
                  {group.items.map((entry) => {
              const isNavigable = checkSessionNavigable(isSessionNavigable, entry.workspaceId, entry.sessionId);
              const isUnread = "unread" in entry.read;
              const location = formatNotificationLocation(entry.labels.workspaceLabel, entry.labels.worktreeLabel);
              const relativeTime = formatRelativeTime(entry.lastOccurredAt);
              const agentLogo = resolveAgentLogo(entry.labels.agentLabel);
              const isMonochrome = isMonochromeAgentLogo(entry.labels.agentLabel);
              const statusDotState = reasonToStatusDotState(entry.reason);

              const handleRowClick = () => {
                if (!isNavigable) return;
                onNavigateToSession?.({
                  workspaceId: entry.workspaceId,
                  sessionId: entry.sessionId,
                  revision: entry.revision,
                });
                onClose();
              };

              const handleRowKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  handleRowClick();
                }
              };

              return (
                <div
                  key={entry.id}
                  data-testid={`notification-row-${entry.id}`}
                  role="button"
                  tabIndex={isNavigable ? 0 : -1}
                  aria-disabled={!isNavigable}
                  onClick={handleRowClick}
                  onKeyDown={handleRowKeyDown}
                  className={cn(
                    "group relative flex items-start gap-2.5 p-3 transition-colors outline-none",
                    isNavigable
                      ? "cursor-pointer hover:bg-accent/40 focus-visible:bg-accent/40"
                      : "cursor-default opacity-60",
                    isUnread && "bg-muted/15",
                  )}
                >
                  {/* Left: agent logo / terminal icon + StatusDot */}
                  <div className="relative mt-0.5 flex shrink-0 items-center justify-center">
                    {agentLogo ? (
                      <img
                        src={agentLogo}
                        alt={entry.labels.agentLabel ?? "Agent"}
                        data-testid="notification-agent-icon"
                        className={cn(
                          "size-4 shrink-0 rounded-sm object-contain",
                          isMonochrome && "agent-tab-logo--monochrome opacity-80",
                        )}
                      />
                    ) : (
                      <TerminalSquare
                        className="size-4 shrink-0 text-muted-foreground"
                        aria-hidden="true"
                        data-testid="notification-terminal-icon"
                      />
                    )}
                    <span
                      data-testid="notification-status-dot"
                      className="absolute -bottom-1 -right-1 inline-flex size-2.5 items-center justify-center"
                    >
                      <StatusDot state={statusDotState} />
                    </span>
                  </div>

                  {/* Middle: Title, Location, Session ended */}
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <div className="flex items-center justify-between gap-1">
                      <span className="truncate text-xs font-medium text-foreground">
                        {entry.labels.terminalTitle || entry.labels.agentLabel || "Terminal"}
                      </span>
                      <span className="shrink-0 text-[10px] text-muted-foreground">
                        {relativeTime}
                      </span>
                    </div>

                    {location ? (
                      <span className="truncate text-[11px] text-muted-foreground">
                        {location}
                      </span>
                    ) : null}

                    {!isNavigable ? (
                      <span data-testid="session-ended-label" className="text-[11px] italic text-muted-foreground">
                        Session ended
                      </span>
                    ) : null}
                  </div>

                  {/* Right: dismiss / unread indicator / bell indicator */}
                  <div className="flex shrink-0 items-center gap-1.5 self-center ml-1">
                    <button
                      type="button"
                      data-testid={`notification-dismiss-${entry.id}`}
                      aria-label="Dismiss notification"
                      onClick={(event) => {
                        event.stopPropagation();
                        dismissEntry(entry.id);
                      }}
                      className="rounded p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground group-hover:opacity-100 focus-visible:opacity-100"
                    >
                      <Trash2 className="size-3" />
                    </button>
                    {isUnread ? (
                      <span
                        data-testid="unread-indicator"
                        aria-label="Unread"
                        className="size-2 shrink-0 rounded-full bg-primary"
                      />
                    ) : null}
                    {entry.reason === "bell" ? (
                      <Bell
                        data-testid="bell-indicator"
                        className="size-3 shrink-0 text-muted-foreground"
                        aria-label="Bell alert"
                      />
                    ) : null}
                  </div>
                </div>
              );
                  })}
                </div>
              </section>
            ))}
          </div>
        )}
      </div>
    </>,
    document.body,
  );
}
