import {
  Columns2,
  Globe,
  ListX,
  PanelLeftClose,
  PanelRightClose,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Rows2,
  TerminalSquare,
  X,
} from "lucide-react";
import React, { type ReactNode, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { resolveActivityIndicator, type ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import type { WorkspaceTab } from "../lib/types";
import { NewTabPopover } from "./NewTabPopover";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";

type TabBarProps = {
  tabs: WorkspaceTab[];
  activeTabId: string;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onCloseOthers?: (id: string) => void;
  onCloseToRight?: (id: string) => void;
  onCloseToLeft?: (id: string) => void;
  onReorderTabs?: (tabId: string, targetIndex: number) => void;
  onRenameTab?: (id: string, newLabel: string) => void;
  onTogglePin?: (id: string, pinned: boolean) => void;
  onSplitRight?: (tabId: string) => void;
  onSplitDown?: (tabId: string) => void;
  onAdd: () => void;
  onAddBrowser?: (url?: string) => void;
  actions?: ReactNode;
  showAdd?: boolean;
  leadingSpacer?: number;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
};

type ContextMenuState = {
  tabId: string;
  x: number;
  y: number;
} | null;

type DropTarget = { index: number; edge: "left" | "right" } | null;

export function TabBar({
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onCloseOthers,
  onCloseToRight,
  onCloseToLeft,
  onReorderTabs,
  onRenameTab,
  onTogglePin,
  onSplitRight,
  onSplitDown,
  onAdd,
  onAddBrowser,
  actions,
  showAdd = true,
  leadingSpacer = 0,
  unreadTabIds,
  activityByTabId,
}: TabBarProps) {
  const [isNewTabOpen, setIsNewTabOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>(null);
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [draggedTabId, setDraggedTabId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<DropTarget>(null);
  const renameCancelledRef = useRef(false);
  const draggedTabIdRef = useRef<string | null>(null);
  const dropTargetRef = useRef<DropTarget>(null);

  useEffect(() => {
    if (renamingTabId && !tabs.some((tab) => tab.id === renamingTabId)) {
      setRenamingTabId(null);
    }
    if (contextMenu && !tabs.some((tab) => tab.id === contextMenu.tabId)) {
      setContextMenu(null);
    }
  }, [contextMenu, renamingTabId, tabs]);

  const handleContextMenu = (event: React.MouseEvent, tab: WorkspaceTab) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({ tabId: tab.id, x: event.clientX, y: event.clientY });
  };

  const handleStartRename = (tab: WorkspaceTab) => {
    if (!onRenameTab) return;
    renameCancelledRef.current = false;
    setRenamingTabId(tab.id);
    setRenameValue(tab.label);
    setContextMenu(null);
  };

  const handleCommitRename = (tabId: string) => {
    const cancelled = renameCancelledRef.current;
    renameCancelledRef.current = false;
    if (!cancelled) {
      const label = renameValue.trim();
      if (label) onRenameTab?.(tabId, label);
    }
    setRenamingTabId(null);
  };

  const clearDragState = () => {
    draggedTabIdRef.current = null;
    dropTargetRef.current = null;
    setDraggedTabId(null);
    setDropTarget(null);
  };

  const handleDragStart = (event: React.DragEvent, tabId: string) => {
    draggedTabIdRef.current = tabId;
    dropTargetRef.current = null;
    setDropTarget(null);
    setDraggedTabId(tabId);
    event.dataTransfer.setData("text/plain", tabId);
    event.dataTransfer.effectAllowed = "move";
  };

  const handleDragOver = (event: React.DragEvent, index: number) => {
    if (!draggedTabIdRef.current || !onReorderTabs) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";

    const rect = event.currentTarget.getBoundingClientRect();
    const midX = rect.width > 0 ? rect.left + rect.width / 2 : 0;
    const clientX = typeof event.clientX === "number" && !Number.isNaN(event.clientX) ? event.clientX : (event.nativeEvent as any)?.clientX ?? 0;
    const isRightEdge = rect.width > 0 ? clientX >= midX : true;
    const nextTarget: Exclude<DropTarget, null> = { index, edge: isRightEdge ? "right" : "left" };
    const currentTarget = dropTargetRef.current;
    if (currentTarget?.index !== nextTarget.index || currentTarget.edge !== nextTarget.edge) {
      dropTargetRef.current = nextTarget;
      setDropTarget(nextTarget);
    }
  };

  const handleDragLeave = (event: React.DragEvent, index: number) => {
    const related = event.relatedTarget;
    if (related instanceof Node && event.currentTarget.contains(related)) return;
    if (dropTargetRef.current?.index === index) {
      dropTargetRef.current = null;
      setDropTarget(null);
    }
  };

  const handleDrop = (event: React.DragEvent, targetIndex: number) => {
    event.preventDefault();
    const sourceTabId = draggedTabIdRef.current || event.dataTransfer.getData("text/plain");
    const sourceIndex = tabs.findIndex((tab) => tab.id === sourceTabId);
    if (sourceIndex >= 0 && onReorderTabs) {
      const rect = event.currentTarget.getBoundingClientRect();
      const currentTarget = dropTargetRef.current;
      const insertAfterTarget =
        currentTarget?.index === targetIndex
          ? currentTarget.edge === "right"
          : (typeof event.clientX === "number" && !Number.isNaN(event.clientX) ? event.clientX : (event.nativeEvent as any)?.clientX ?? 0) >= rect.left + rect.width / 2;
      const insertionIndex = targetIndex + (insertAfterTarget ? 1 : 0);
      const finalIndex = Math.max(
        0,
        Math.min(tabs.length - 1, insertionIndex - (sourceIndex < insertionIndex ? 1 : 0)),
      );
      if (finalIndex !== sourceIndex) onReorderTabs(sourceTabId, finalIndex);
    }
    clearDragState();
  };

  const startWindowDrag = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest(".no-drag")) return;
    void getCurrentWindow().startDragging();
  };

  const contextTab = contextMenu ? tabs.find((tab) => tab.id === contextMenu.tabId) ?? null : null;
  const contextIndex = contextTab ? tabs.findIndex((tab) => tab.id === contextTab.id) : -1;

  return (
    <div
      data-testid="tab-strip"
      data-tauri-drag-region
      onPointerDown={startWindowDrag}
      className="drag-region relative flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1 select-none"
    >
      {leadingSpacer > 0 ? (
        <div
          data-testid="tab-strip-leading-spacer"
          style={{ width: `${leadingSpacer}px` }}
          className="shrink-0 border-r border-border"
        />
      ) : null}
      <div className="flex min-w-0 items-stretch overflow-x-auto scrollbar-none" role="tablist">
        {tabs.map((tab, index) => {
          const active = tab.id === activeTabId;
          const isUnread = Boolean(unreadTabIds?.[tab.id] && !active);
          const resolvedActivity = tab.kind === "browser" ? null : resolveActivityIndicator(activityByTabId?.[tab.id]);
          const activityIndicator =
            isUnread && (resolvedActivity === null || resolvedActivity === "done") ? "unread" : resolvedActivity;
          const isPinned = Boolean(tab.pinned);
          const isRenaming = renamingTabId === tab.id;
          const isDropLeft = dropTarget?.index === index && dropTarget.edge === "left";
          const isDropRight = dropTarget?.index === index && dropTarget.edge === "right";

          return (
            <div
              key={tab.id}
              role="tab"
              aria-selected={active}
              tabIndex={active ? 0 : -1}
              draggable={!isRenaming && Boolean(onReorderTabs)}
              onDragStart={(event) => handleDragStart(event, tab.id)}
              onDragOver={(event) => handleDragOver(event, index)}
              onDragLeave={(event) => handleDragLeave(event, index)}
              onDrop={(event) => handleDrop(event, index)}
              onDragEnd={clearDragState}
              onContextMenu={(event) => handleContextMenu(event, tab)}
              onClick={() => onActivate(tab.id)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  onActivate(tab.id);
                }
              }}
              className={cn(
                "no-drag group relative flex min-w-tab max-w-tab cursor-pointer items-center gap-1.5 border-r border-border px-2 text-[12px] transition-colors outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
                active ? "bg-terminal text-foreground" : "bg-card text-muted-foreground hover:bg-accent/45 hover:text-foreground",
                draggedTabId === tab.id && "opacity-40",
                isDropLeft && "before:absolute before:inset-y-0 before:left-0 before:z-20 before:w-[2px] before:bg-blue-500 before:content-['']",
                isDropRight && "after:absolute after:inset-y-0 after:right-0 after:z-20 after:w-[2px] after:bg-blue-500 after:content-['']",
              )}
            >
              {tab.kind === "browser" ? (
                <Globe className="size-3 shrink-0 text-blue-400" />
              ) : activityIndicator ? (
                <span
                  data-testid={activityIndicator === "unread" ? "tab-unread-dot" : `tab-${activityIndicator}-indicator`}
                  aria-label={`Agent ${activityIndicator}`}
                  className="inline-flex size-3 shrink-0 items-center justify-center"
                >
                  <StatusDot state={activityIndicator} />
                </span>
              ) : (
                <TerminalSquare className="size-3 shrink-0" />
              )}

              {isRenaming ? (
                <input
                  type="text"
                  value={renameValue}
                  autoFocus
                  data-tab-rename-input="true"
                  onClick={(event) => event.stopPropagation()}
                  onPointerDown={(event) => event.stopPropagation()}
                  onChange={(event) => setRenameValue(event.target.value)}
                  onBlur={() => handleCommitRename(tab.id)}
                  onKeyDown={(event) => {
                    event.stopPropagation();
                    if (event.key === "Enter") {
                      event.preventDefault();
                      event.currentTarget.blur();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      renameCancelledRef.current = true;
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
        })}
        {showAdd ? (
          <div className="no-drag relative flex items-center shrink-0">
            <IconButton
              label="New tab"
              size="sm"
              className="no-drag my-auto ml-1"
              onClick={() => setIsNewTabOpen((previous) => !previous)}
            >
              <Plus className="size-3.5" />
            </IconButton>
            <NewTabPopover
              open={isNewTabOpen}
              onClose={() => setIsNewTabOpen(false)}
              onNewTerminal={() => {
                setIsNewTabOpen(false);
                onAdd();
              }}
              onNewBrowser={(url) => {
                setIsNewTabOpen(false);
                onAddBrowser?.(url);
              }}
            />
          </div>
        ) : null}
      </div>

      <div className="flex-1" />

      {actions ? <div className="no-drag ml-0.5 flex shrink-0 items-center gap-0.5">{actions}</div> : null}

      {contextMenu && contextTab && contextIndex >= 0 ? (
        <TabContextMenuPopup
          state={{ tab: contextTab, x: contextMenu.x, y: contextMenu.y, index: contextIndex }}
          tabs={tabs}
          canSplit={contextTab.kind !== "browser"}
          hasSplitRightHandler={Boolean(onSplitRight)}
          hasSplitDownHandler={Boolean(onSplitDown)}
          hasRenameHandler={Boolean(onRenameTab)}
          hasPinHandler={Boolean(onTogglePin)}
          hasCloseOthersHandler={Boolean(onCloseOthers)}
          hasCloseToRightHandler={Boolean(onCloseToRight)}
          hasCloseToLeftHandler={Boolean(onCloseToLeft)}
          onClose={() => setContextMenu(null)}
          onSplitRight={() => {
            onSplitRight?.(contextTab.id);
            setContextMenu(null);
          }}
          onSplitDown={() => {
            onSplitDown?.(contextTab.id);
            setContextMenu(null);
          }}
          onTogglePin={() => {
            onTogglePin?.(contextTab.id, !contextTab.pinned);
            setContextMenu(null);
          }}
          onRename={() => handleStartRename(contextTab)}
          onCloseTab={() => {
            if (!contextTab.pinned) onClose(contextTab.id);
            setContextMenu(null);
          }}
          onCloseOthers={() => {
            onCloseOthers?.(contextTab.id);
            setContextMenu(null);
          }}
          onCloseToRight={() => {
            onCloseToRight?.(contextTab.id);
            setContextMenu(null);
          }}
          onCloseToLeft={() => {
            onCloseToLeft?.(contextTab.id);
            setContextMenu(null);
          }}
        />
      ) : null}
    </div>
  );
}

type TabContextMenuPopupProps = {
  state: { tab: WorkspaceTab; x: number; y: number; index: number };
  tabs: WorkspaceTab[];
  canSplit: boolean;
  hasSplitRightHandler: boolean;
  hasSplitDownHandler: boolean;
  hasRenameHandler: boolean;
  hasPinHandler: boolean;
  hasCloseOthersHandler: boolean;
  hasCloseToRightHandler: boolean;
  hasCloseToLeftHandler: boolean;
  onClose: () => void;
  onSplitRight: () => void;
  onSplitDown: () => void;
  onTogglePin: () => void;
  onRename: () => void;
  onCloseTab: () => void;
  onCloseOthers: () => void;
  onCloseToRight: () => void;
  onCloseToLeft: () => void;
};

function TabContextMenuPopup({
  state,
  tabs,
  canSplit,
  hasSplitRightHandler,
  hasSplitDownHandler,
  hasRenameHandler,
  hasPinHandler,
  hasCloseOthersHandler,
  hasCloseToRightHandler,
  hasCloseToLeftHandler,
  onClose,
  onSplitRight,
  onSplitDown,
  onTogglePin,
  onRename,
  onCloseTab,
  onCloseOthers,
  onCloseToRight,
  onCloseToLeft,
}: TabContextMenuPopupProps) {
  const popupRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleOutside = (event: MouseEvent) => {
      if (popupRef.current && !popupRef.current.contains(event.target as Node)) onClose();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", handleOutside, true);
    window.addEventListener("contextmenu", handleOutside, true);
    window.addEventListener("keydown", handleKeyDown, true);
    return () => {
      window.removeEventListener("mousedown", handleOutside, true);
      window.removeEventListener("contextmenu", handleOutside, true);
      window.removeEventListener("keydown", handleKeyDown, true);
    };
  }, [onClose]);

  const hasClosableOther = tabs.some((tab) => tab.id !== state.tab.id && !tab.pinned);
  const hasClosableToRight = tabs.slice(state.index + 1).some((tab) => !tab.pinned);
  const hasClosableToLeft = tabs.slice(0, state.index).some((tab) => !tab.pinned);
  const splitRightDisabled = !canSplit || !hasSplitRightHandler;
  const splitDownDisabled = !canSplit || !hasSplitDownHandler;
  const viewportWidth = typeof window === "undefined" ? 1024 : window.innerWidth;
  const viewportHeight = typeof window === "undefined" ? 768 : window.innerHeight;
  const left = Math.max(4, Math.min(Math.max(4, viewportWidth - 220), state.x));
  const top = Math.max(4, Math.min(Math.max(4, viewportHeight - 300), state.y));

  const menuButtonClass =
    "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-foreground hover:bg-accent hover:text-accent-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent";

  return (
    <div
      ref={popupRef}
      role="menu"
      aria-label="Tab context menu"
      className="no-drag fixed z-50 min-w-[13rem] select-none rounded-md border border-border bg-popover/95 p-1 text-xs text-popover-foreground shadow-xl backdrop-blur-md animate-in fade-in-50 zoom-in-95"
      style={{ left: `${left}px`, top: `${top}px` }}
      onClick={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="px-1.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60">Split Pane</div>
      <button type="button" role="menuitem" disabled={splitRightDisabled} onClick={onSplitRight} className={cn(menuButtonClass, "justify-between")}>
        <div className="flex items-center gap-2">
          <Columns2 className="size-3.5 text-muted-foreground" />
          <span>Split terminal right</span>
        </div>
        <span className="font-mono text-[10px] text-muted-foreground">⌘D</span>
      </button>
      <button type="button" role="menuitem" disabled={splitDownDisabled} onClick={onSplitDown} className={cn(menuButtonClass, "justify-between")}>
        <div className="flex items-center gap-2">
          <Rows2 className="size-3.5 text-muted-foreground" />
          <span>Split terminal down</span>
        </div>
        <span className="font-mono text-[10px] text-muted-foreground">⌘⇧D</span>
      </button>

      <div className="my-1 h-px bg-border/60" />

      <button type="button" role="menuitem" disabled={!hasPinHandler} onClick={onTogglePin} className={menuButtonClass}>
        {state.tab.pinned ? <PinOff className="size-3.5 text-muted-foreground" /> : <Pin className="size-3.5 text-muted-foreground" />}
        <span>{state.tab.pinned ? "Unpin Tab" : "Pin Tab"}</span>
      </button>
      <button type="button" role="menuitem" disabled={!hasRenameHandler} onClick={onRename} className={menuButtonClass}>
        <Pencil className="size-3.5 text-muted-foreground" />
        <span>Change Title</span>
      </button>

      <div className="my-1 h-px bg-border/60" />

      <button
        type="button"
        role="menuitem"
        disabled={Boolean(state.tab.pinned)}
        onClick={onCloseTab}
        className={cn(menuButtonClass, "justify-between")}
      >
        <div className="flex items-center gap-2">
          <X className="size-3.5 text-muted-foreground" />
          <span>Close</span>
        </div>
        <span className="font-mono text-[10px] text-muted-foreground">⌘W</span>
      </button>
      <button
        type="button"
        role="menuitem"
        disabled={!hasCloseOthersHandler || !hasClosableOther}
        onClick={onCloseOthers}
        className={menuButtonClass}
      >
        <ListX className="size-3.5 text-muted-foreground" />
        <span>Close Others</span>
      </button>
      <button
        type="button"
        role="menuitem"
        disabled={!hasCloseToRightHandler || !hasClosableToRight}
        onClick={onCloseToRight}
        className={menuButtonClass}
      >
        <PanelRightClose className="size-3.5 text-muted-foreground" />
        <span>Close Tabs To The Right</span>
      </button>
      <button
        type="button"
        role="menuitem"
        disabled={!hasCloseToLeftHandler || !hasClosableToLeft}
        onClick={onCloseToLeft}
        className={menuButtonClass}
      >
        <PanelLeftClose className="size-3.5 text-muted-foreground" />
        <span>Close Tabs To The Left</span>
      </button>
    </div>
  );
}
