import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { Globe, Pin, TerminalSquare, X } from "lucide-react";
import { memo, useMemo, type KeyboardEvent, type MouseEvent, type PointerEvent } from "react";

import { resolveActivityIndicator, type ActivitySummary } from "../../lib/activity";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../../lib/agentIcon";
import { cn } from "../../lib/cn";
import type { WorkspaceTab } from "../../lib/types";
import { StatusDot } from "../ui/StatusDot";

type SortableTabProps = {
  tab: WorkspaceTab;
  groupId: string;
  index: number;
  active: boolean;
  unread: boolean;
  activity?: ActivitySummary;
  isRenaming: boolean;
  renameValue: string;
  onRenameValueChange: (value: string) => void;
  onCommitRename: (tabId: string) => void;
  onCancelRename: () => void;
  onActivate: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onContextMenu: (event: MouseEvent, tab: WorkspaceTab) => void;
};

export const SortableTab = memo(function SortableTab({
  tab,
  groupId,
  index,
  active,
  unread,
  activity,
  isRenaming,
  renameValue,
  onRenameValueChange,
  onCommitRename,
  onCancelRename,
  onActivate,
  onClose,
  onContextMenu,
}: SortableTabProps) {
  const sortableData = useMemo(
    () => ({ type: "tab", tabId: tab.id, groupId, index }),
    [tab.id, groupId, index],
  );
  const sortable = useSortable({
    id: `tab:${tab.id}`,
    disabled: isRenaming,
    data: sortableData,
  });
  const resolvedActivity = tab.kind === "browser" ? null : resolveActivityIndicator(activity);
  const activityIndicator = unread && (resolvedActivity === null || resolvedActivity === "done") ? "unread" : resolvedActivity;
  const isPinned = Boolean(tab.pinned);
  const agentType = activity?.agentType;
  const agentLogo = resolveAgentLogo(agentType);
  const isMonochrome = isMonochromeAgentLogo(agentType);

  const style = {
    transform: CSS.Transform.toString(sortable.transform),
    transition: sortable.transition,
  };

  const stopPointer = (event: PointerEvent) => event.stopPropagation();

  return (
    <div
      ref={sortable.setNodeRef}
      {...sortable.attributes}
      {...sortable.listeners}
      role="tab"
      aria-selected={active}
      tabIndex={active ? 0 : -1}
      data-tab-dnd-id={tab.id}
      data-tab-group-id={groupId}
      data-tab-index={index}
      data-dnd-type="tab"
      draggable={false}
      onContextMenu={(event) => onContextMenu(event, tab)}
      onClick={() => onActivate(tab.id)}
      onKeyDown={(event: KeyboardEvent) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onActivate(tab.id);
        }
      }}
      style={style}
      className={cn(
        "no-drag group relative flex min-w-tab max-w-tab cursor-pointer touch-none items-center gap-1.5 border-r border-border px-2 text-[12px] transition-colors outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
        active ? "bg-accent text-foreground" : "bg-card text-muted-foreground hover:bg-accent/45 hover:text-foreground",
        sortable.isDragging && "z-40 opacity-30",
      )}
    >
      {tab.kind === "browser" ? (
        <Globe className="size-3 shrink-0 text-primary" />
      ) : (
        <>
          {agentLogo ? (
            <img
              src={agentLogo}
              alt=""
              aria-hidden="true"
              data-testid="tab-agent-icon"
              data-agent-type={agentType}
              className={cn(
                "size-3 shrink-0",
                isMonochrome && "agent-tab-logo--monochrome opacity-80 group-hover:opacity-100",
                isMonochrome && active && "opacity-100",
              )}
            />
          ) : (
            <TerminalSquare data-testid="tab-terminal-icon" className="size-3 shrink-0" />
          )}
          {activityIndicator ? (
            <span
              data-testid={activityIndicator === "unread" ? "tab-unread-dot" : `tab-${activityIndicator}-indicator`}
              aria-label={`Agent ${activityIndicator}`}
              className="inline-flex size-3 shrink-0 items-center justify-center"
            >
              <StatusDot state={activityIndicator} />
            </span>
          ) : null}
        </>
      )}

      {isRenaming ? (
        <input
          type="text"
          value={renameValue}
          autoFocus
          data-tab-rename-input="true"
          onClick={(event) => event.stopPropagation()}
          onPointerDown={stopPointer}
          onChange={(event) => onRenameValueChange(event.target.value)}
          onBlur={() => onCommitRename(tab.id)}
          onKeyDown={(event) => {
            event.stopPropagation();
            if (event.key === "Enter") {
              event.preventDefault();
              event.currentTarget.blur();
            } else if (event.key === "Escape") {
              event.preventDefault();
              onCancelRename();
              event.currentTarget.blur();
            }
          }}
          className="min-w-0 flex-1 rounded border border-ring bg-background px-1 py-0.5 text-xs text-foreground outline-none"
        />
      ) : (
        <span className="min-w-0 flex-1 truncate text-left">{tab.label}</span>
      )}

      {isPinned ? <Pin className="size-2.5 shrink-0 text-muted-foreground/80" aria-label="Pinned tab" /> : null}

      {!isPinned ? (
        <span
          role="button"
          tabIndex={0}
          aria-label={`Close ${tab.label}`}
          onPointerDown={stopPointer}
          onClick={(event) => {
            event.stopPropagation();
            onClose(tab.id);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              event.stopPropagation();
              onClose(tab.id);
            }
          }}
          className="no-drag rounded p-0.5 text-muted-foreground opacity-0 hover:bg-muted hover:text-foreground group-hover:opacity-100 focus:opacity-100"
        >
          <X className="size-3" />
        </span>
      ) : null}

      {active ? (
        <span data-testid="tab-active-indicator" className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-foreground" />
      ) : null}
    </div>
  );
});
