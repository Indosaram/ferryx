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
import React, { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { BrowserDuplicateControl } from "./BrowserDuplicateControl";
import type { ActivitySummary } from "../lib/activity";
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
  onAddBrowser?: (url?: string, profileId?: string) => void;
  onDuplicateBrowser?: (tabId: string, profileId?: string) => void;
  onAddMarkdown?: () => void;
  onNewDag?: () => void;
  onAddMobileEmulator?: () => void;
  onOpenSettings?: () => void;
  agents?: Array<{ name: string; command: string; args: string }>;
  onLaunchAgent?: (agent: { name: string; command: string; args: string }) => void;
  defaultAgentId?: string | null;
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
  onDuplicateBrowser,
  onAddMarkdown,
  onNewDag,
  onAddMobileEmulator,
  onOpenSettings,
  agents,
  onLaunchAgent,
  defaultAgentId,
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

  const handleContextMenu = useCallback((event: React.MouseEvent, tab: WorkspaceTab) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({ tabId: tab.id, x: event.clientX, y: event.clientY });
  }, []);

  const handleStartRename = (tab: WorkspaceTab) => {
    if (!onRenameTab) return;
    renameCancelledRef.current = false;
    setRenamingTabId(tab.id);
    setRenameValue(tab.label);
    setContextMenu(null);
  };

  const handleCommitRename = useCallback((tabId: string) => {
    const cancelled = renameCancelledRef.current;
    renameCancelledRef.current = false;
    if (!cancelled) {
      const label = renameValue.trim();
      if (label) onRenameTab?.(tabId, label);
    }
    setRenamingTabId(null);
  }, [onRenameTab, renameValue]);

  const handleCancelRename = useCallback(() => {
    renameCancelledRef.current = true;
  }, []);

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
  const sortableItems = useMemo(() => tabs.map((tab) => `tab:${tab.id}`), [tabs]);

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
      <SortableContext items={sortableItems} strategy={horizontalListSortingStrategy}>
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
                onCancelRename={handleCancelRename}
                onActivate={onActivate}
                onClose={onClose}
                onContextMenu={handleContextMenu}
              />
            );
          })}
        </div>
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
              onNewBrowser={(url, profileId) => {
                setIsNewTabOpen(false);
                onAddBrowser?.(url, profileId);
              }}
              onNewMarkdown={
                onAddMarkdown
                  ? () => {
                      setIsNewTabOpen(false);
                      onAddMarkdown();
                    }
                  : undefined
              }
              onNewDag={
                onNewDag
                  ? () => {
                      setIsNewTabOpen(false);
                      onNewDag();
                    }
                  : undefined
              }
              onNewMobileEmulator={
                onAddMobileEmulator
                  ? () => {
                      setIsNewTabOpen(false);
                      onAddMobileEmulator();
                    }
                  : undefined
              }
              onOpenSettings={
                onOpenSettings
                  ? () => {
                      setIsNewTabOpen(false);
                      onOpenSettings();
                    }
                  : undefined
              }
              agents={agents}
              defaultAgentId={defaultAgentId}
              onLaunchAgent={
                onLaunchAgent
                  ? (agent) => {
                      setIsNewTabOpen(false);
                      onLaunchAgent(agent);
                    }
                  : undefined
              }
            />
          </div>
        ) : null}
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
          onDuplicateBrowser={
            contextTab.kind === "browser" && onDuplicateBrowser
              ? (profileId) => {
                  onDuplicateBrowser(contextTab.id, profileId);
                  setContextMenu(null);
                }
              : undefined
          }
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
  onDuplicateBrowser,
  onCloseTab,
  onCloseOthers,
  onCloseToRight,
  onCloseToLeft,
}: {
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
  onDuplicateBrowser?: (profileId: string) => void;
  onCloseTab: () => void;
  onCloseOthers: () => void;
  onCloseToRight: () => void;
  onCloseToLeft: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const handleClick = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("mousedown", handleClick);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("mousedown", handleClick);
    };
  }, [onClose]);

  const hasTabsToRight = state.index >= 0 && state.index < tabs.length - 1;
  const hasTabsToLeft = state.index > 0;
  const hasOtherTabs = tabs.length > 1;

  const left = Math.min(state.x, window.innerWidth - 200);
  const top = Math.min(state.y, window.innerHeight - 260);

  return (
    <div
      ref={menuRef}
      role="menu"
      className="fixed z-50 min-w-44 rounded-md border border-border bg-popover p-1 text-xs text-popover-foreground shadow-md"
      style={{ left: `${left}px`, top: `${top}px` }}
    >
      {state.tab.kind === "browser" && onDuplicateBrowser ? (
        <BrowserDuplicateControl tab={state.tab} onDuplicate={onDuplicateBrowser} />
      ) : null}
      {hasSplitRightHandler ? (
        <button
          type="button"
          role="menuitem"
          disabled={!canSplitTerminal}
          onClick={onSplitRight}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
        >
          <Columns2 className="size-3.5" />
          Split terminal right
        </button>
      ) : null}
      {hasSplitDownHandler ? (
        <button
          type="button"
          role="menuitem"
          disabled={!canSplitTerminal}
          onClick={onSplitDown}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
        >
          <Rows2 className="size-3.5" />
          Split terminal down
        </button>
      ) : null}

      {hasMoveTabToSplitHandler ? (
        <>
          <div className="my-1 border-t border-border" />
          <button
            type="button"
            role="menuitem"
            onClick={() => onMoveTabToSplit("right")}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            <ArrowRight className="size-3.5" />
            Move Tab to Split Right
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => onMoveTabToSplit("bottom")}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            <ArrowDown className="size-3.5" />
            Move Tab to Split Down
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => onMoveTabToSplit("left")}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            <ArrowLeft className="size-3.5" />
            Move Tab to Split Left
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => onMoveTabToSplit("top")}
            className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
          >
            <ArrowUp className="size-3.5" />
            Move Tab to Split Up
          </button>
        </>
      ) : null}

      <div className="my-1 border-t border-border" />

      {hasPinHandler ? (
        <button
          type="button"
          role="menuitem"
          onClick={onTogglePin}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
        >
          {state.tab.pinned ? <PinOff className="size-3.5" /> : <Pin className="size-3.5" />}
          {state.tab.pinned ? "Unpin tab" : "Pin tab"}
        </button>
      ) : null}

      {hasRenameHandler ? (
        <button
          type="button"
          role="menuitem"
          onClick={onRename}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground"
        >
          <Pencil className="size-3.5" />
          Rename tab
        </button>
      ) : null}

      <div className="my-1 border-t border-border" />

      <button
        type="button"
        role="menuitem"
        disabled={state.tab.pinned}
        onClick={onCloseTab}
        className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs text-destructive transition-colors hover:bg-destructive/10 disabled:opacity-40"
      >
        <X className="size-3.5" />
        Close tab
      </button>

      {hasCloseOthersHandler ? (
        <button
          type="button"
          role="menuitem"
          disabled={!hasOtherTabs}
          onClick={onCloseOthers}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
        >
          <ListX className="size-3.5" />
          Close other tabs
        </button>
      ) : null}

      {hasCloseToRightHandler ? (
        <button
          type="button"
          role="menuitem"
          disabled={!hasTabsToRight}
          onClick={onCloseToRight}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
        >
          <PanelRightClose className="size-3.5" />
          Close tabs to right
        </button>
      ) : null}

      {hasCloseToLeftHandler ? (
        <button
          type="button"
          role="menuitem"
          disabled={!hasTabsToLeft}
          onClick={onCloseToLeft}
          className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-left text-xs transition-colors hover:bg-accent hover:text-accent-foreground disabled:opacity-40"
        >
          <PanelLeftClose className="size-3.5" />
          Close tabs to left
        </button>
      ) : null}
    </div>
  );
}
