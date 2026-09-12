import {
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Bell, TerminalSquare, X } from "lucide-react";

import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";
import { notificationCenterStore, type NotificationCenterStore } from "../../lib/notificationCenter/notificationCenterStore";
import type { NotificationEntry } from "../../lib/notificationCenter/types";
import { StatusDot, type StatusDotState } from "../ui/StatusDot";
import { useNotificationCenter } from "./useNotificationCenter";

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
}: NotificationCenterPopoverProps): ReactNode {
  const { entries, unreadCount, markAllRead, dismissEntry, clearAll } = useNotificationCenter(store);
  const modalRef = useRef<HTMLDivElement>(null);
  const previousActiveElementRef = useRef<HTMLElement | null>(null);

  const [popoverStyle, setPopoverStyle] = useState<CSSProperties>({
    bottom: 42,
    left: 8,
  });

  useLayoutEffect(() => {
    if (!anchorRef?.current) return;
    const rect = anchorRef.current.getBoundingClientRect();
    const bottom = Math.max(8, window.innerHeight - rect.top + 6);
    const left = Math.max(8, rect.left);
    setPopoverStyle({ bottom, left });
  }, [anchorRef]);

  useEffect(() => {
    if (!open) return;

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
      clearTimeout(focusTimer);
      document.removeEventListener("keydown", onKeyDown, true);
      previousActiveElementRef.current?.focus();
    };
  }, [open, onClose]);

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
            <button
              type="button"
              onClick={() => markAllRead()}
              disabled={unreadCount === 0}
              className="rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40 transition-colors"
            >
              Mark all read
            </button>
            <button
              type="button"
              onClick={() => clearAll()}
              disabled={entries.length === 0}
              className="rounded px-2 py-0.5 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-40 transition-colors"
            >
              Clear all
            </button>
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

        {/* Content */}
        {entries.length === 0 ? (
          <div
            data-testid="notification-empty-state"
            className="flex flex-col items-center justify-center p-8 text-center text-xs text-muted-foreground"
          >
            No new notifications
          </div>
        ) : (
          <div className="flex flex-col divide-y divide-border/40 overflow-y-auto max-h-[380px] scrollbar-sleek">
            {entries.map((entry) => {
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

                  {/* Right: Unread indicator / bell indicator + Clear button */}
                  <div className="flex shrink-0 items-center gap-1.5 self-center ml-1">
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
                    <button
                      type="button"
                      aria-label="Clear notification"
                      data-testid="notification-clear-button"
                      onClick={(e) => {
                        e.stopPropagation();
                        dismissEntry(entry.id);
                      }}
                      className="rounded p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-accent hover:text-foreground focus:opacity-100 group-hover:opacity-100"
                    >
                      <X className="size-3.5" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>,
    document.body,
  );
}
