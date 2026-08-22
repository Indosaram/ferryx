import { SortableContext, horizontalListSortingStrategy } from "@dnd-kit/sortable";
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Columns2,
  ListX,
  PanelLeftClose,
  PanelRightClose,
  Pencil,
  Pin,
  PinOff,
  Plus,
  Rows2,
  X,
} from "lucide-react";
import React, { type ReactNode, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import type { WorkspaceTab } from "../lib/types";
import { NewTabPopover } from "./NewTabPopover";
import { SortableTab } from "./tab-dnd/SortableTab";
import type { TabDropEdge } from "./tab-dnd/tabDragTypes";
import { IconButton } from "./ui/IconButton";

type TabBarProps = {
  groupId?: string;
  tabs: WorkspaceTab[];
  activeTabId: string;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onCloseOthers?: (id: string) => void;
  onCloseToRight?: (id: string) => void;
  onCloseToLeft?: (id: string) => void;
  onRenameTab?: (id: string, newLabel: string) => void;
  onTogglePin?: (id: string, pinned: boolean) => void;
  /** Terminal-pane split. Intentionally separate from whole-tab group split. */
  onSplitRight?: (tabId: string) => void;
  onSplitDown?: (tabId: string) => void;
  /** Whole-tab split, valid for terminal and browser tabs. */
  onMoveTabToSplit?: (tabId: string, edge: TabDropEdge) => void;
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

export function TabBar({
  groupId = "group-default",
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onCloseOthers,
  onCloseToRight,
  onCloseToLeft,
  onRenameTab,
  onTogglePin,
  onSplitRight,
  onSplitDown,
  onMoveTabToSplit,
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
  const renameCancelledRef = useRef(false);

  useEffect(() => {
    if (renamingTabId && !tabs.some((tab) => tab.id === renamingTabId)) setRenamingTabId(null);
    if (contextMenu && !tabs.some((tab) => tab.id === contextMenu.tabId)) setContextMenu(null);
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

  const startWindowDrag = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if (
      event.target instanceof Element &&
      (event.target.closest(".no-drag") ||
        event.target.closest('[role="tab"]') ||
        event.target.closest("button") ||
        event.target.closest("input"))
    ) {
      return;
    }
    event.preventDefault();
    void getCurrentWindow().startDragging();
  };

  const contextTab = contextMenu ? tabs.find((tab) => tab.id === contextMenu.tabId) ?? null : null;
  const contextIndex = contextTab ? tabs.findIndex((tab) => tab.id === contextTab.id) : -1;

  return (
    <div
      data-testid="tab-strip"
      data-tab-group-id={groupId}
      onPointerDown={startWindowDrag}
      className="relative flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1 select-none"
    >
      {leadingSpacer > 0 ? (
        <div
          data-testid="tab-strip-leading-spacer"
          style={{ width: `${leadingSpacer}px` }}
          className="shrink-0 border-r border-border"
        />
      ) : null}
      <SortableContext items={tabs.map((tab) => `tab:${tab.id}`)} strategy={horizontalListSortingStrategy}>
        <div className="flex min-w-0 items-stretch overflow-x-auto scrollbar-none" role="tablist">
          {tabs.map((tab, index) => {
            const active = tab.id === activeTabId;
            return (
              <SortableTab
                key={tab.id}
                tab={tab}
                groupId={groupId}
                index={index}
                active={active}
                unread={Boolean(unreadTabIds?.[tab.id] && !active)}
                activity={activityByTabId?.[tab.id]}
                isRenaming={renamingTabId === tab.id}
                renameValue={renameValue}
                onRenameValueChange={setRenameValue}
                onCommitRename={handleCommitRename}
                onCancelRename={() => {
                  renameCancelledRef.current = true;
                }}
                onActivate={onActivate}
                onClose={onClose}
                onContextMenu={handleContextMenu}
              />
            );
          })}
          {showAdd ? (
            <div className="no-drag relative flex shrink-0 items-center">
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
      </SortableContext>

      <div className="flex-1" />
      {actions ? <div className="no-drag ml-0.5 flex shrink-0 items-center gap-0.5">{actions}</div> : null}

      {contextMenu && contextTab && contextIndex >= 0 ? (
        <TabContextMenuPopup
          state={{ tab: contextTab, x: contextMenu.x, y: contextMenu.y, index: contextIndex }}
          tabs={tabs}
          canSplitTerminal={contextTab.kind !== "browser"}
          hasSplitRightHandler={Boolean(onSplitRight)}
          hasSplitDownHandler={Boolean(onSplitDown)}
          hasMoveTabToSplitHandler={Boolean(onMoveTabToSplit)}
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
          onMoveTabToSplit={(edge) => {
            onMoveTabToSplit?.(contextTab.id, edge);
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
  canSplitTerminal: boolean;
  hasSplitRightHandler: boolean;
  hasSplitDownHandler: boolean;
  hasMoveTabToSplitHandler: boolean;
  hasRenameHandler: boolean;
  hasPinHandler: boolean;
  hasCloseOthersHandler: boolean;
  hasCloseToRightHandler: boolean;
  hasCloseToLeftHandler: boolean;
  onClose: () => void;
  onSplitRight: () => void;
  onSplitDown: () => void;
  onMoveTabToSplit: (edge: TabDropEdge) => void;
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
  canSplitTerminal,
  hasSplitRightHandler,
  hasSplitDownHandler,
  hasMoveTabToSplitHandler,
  hasRenameHandler,
  hasPinHandler,
  hasCloseOthersHandler,
  hasCloseToRightHandler,
  hasCloseToLeftHandler,
  onClose,
  onSplitRight,
  onSplitDown,
  onMoveTabToSplit,
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
  const viewportWidth = typeof window === "undefined" ? 1024 : window.innerWidth;
  const viewportHeight = typeof window === "undefined" ? 768 : window.innerHeight;
  const left = Math.max(4, Math.min(Math.max(4, viewportWidth - 220), state.x));
  const top = Math.max(4, Math.min(Math.max(4, viewportHeight - 420), state.y));
  const menuButtonClass =
    "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-foreground hover:bg-accent hover:text-accent-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent";

  const moveItems: Array<{ edge: TabDropEdge; label: string; icon: ReactNode }> = [
    { edge: "left", label: "Move Tab to Split Left", icon: <ArrowLeft className="size-3.5 text-muted-foreground" /> },
    { edge: "right", label: "Move Tab to Split Right", icon: <ArrowRight className="size-3.5 text-muted-foreground" /> },
    { edge: "top", label: "Move Tab to Split Up", icon: <ArrowUp className="size-3.5 text-muted-foreground" /> },
    { edge: "bottom", label: "Move Tab to Split Down", icon: <ArrowDown className="size-3.5 text-muted-foreground" /> },
  ];

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
      <div className="px-1.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60">Move Tab to Split</div>
      {moveItems.map((item) => (
        <button
          key={item.edge}
          type="button"
          role="menuitem"
          disabled={!hasMoveTabToSplitHandler}
          onClick={() => onMoveTabToSplit(item.edge)}
          className={menuButtonClass}
        >
          {item.icon}
          <span>{item.label}</span>
        </button>
      ))}

      <div className="my-1 h-px bg-border/60" />
      <div className="px-1.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60">Split Terminal</div>
      <button type="button" role="menuitem" disabled={!canSplitTerminal || !hasSplitRightHandler} onClick={onSplitRight} className={cn(menuButtonClass, "justify-between")}>
        <div className="flex items-center gap-2">
          <Columns2 className="size-3.5 text-muted-foreground" />
          <span>Split terminal right</span>
        </div>
        <span className="font-mono text-[10px] text-muted-foreground">⌘D</span>
      </button>
      <button type="button" role="menuitem" disabled={!canSplitTerminal || !hasSplitDownHandler} onClick={onSplitDown} className={cn(menuButtonClass, "justify-between")}>
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
      <button type="button" role="menuitem" disabled={Boolean(state.tab.pinned)} onClick={onCloseTab} className={cn(menuButtonClass, "justify-between")}>
        <div className="flex items-center gap-2">
          <X className="size-3.5 text-muted-foreground" />
          <span>Close</span>
        </div>
        <span className="font-mono text-[10px] text-muted-foreground">⌘W</span>
      </button>
      <button type="button" role="menuitem" disabled={!hasCloseOthersHandler || !hasClosableOther} onClick={onCloseOthers} className={menuButtonClass}>
        <ListX className="size-3.5 text-muted-foreground" />
        <span>Close Others</span>
      </button>
      <button type="button" role="menuitem" disabled={!hasCloseToRightHandler || !hasClosableToRight} onClick={onCloseToRight} className={menuButtonClass}>
        <PanelRightClose className="size-3.5 text-muted-foreground" />
        <span>Close Tabs To The Right</span>
      </button>
      <button type="button" role="menuitem" disabled={!hasCloseToLeftHandler || !hasClosableToLeft} onClick={onCloseToLeft} className={menuButtonClass}>
        <PanelLeftClose className="size-3.5 text-muted-foreground" />
        <span>Close Tabs To The Left</span>
      </button>
    </div>
  );
}
