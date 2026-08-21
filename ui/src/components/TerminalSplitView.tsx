import { Columns2, Rows2, X } from "lucide-react";
import React, { useRef, useState } from "react";

import type { ActivitySummary } from "../lib/activity";
import type { LayoutState, TabPaneLayout, TerminalSession, WorkspaceTab } from "../lib/types";
import type { PaneDirection, PaneNode } from "../state/paneTree";
import { BrowserPane } from "./BrowserPane";
import { TabBar } from "./TabBar";
import { TerminalPane } from "./TerminalPane";
import { IconButton } from "./ui/IconButton";

type TerminalSplitViewProps = {
  layout: LayoutState;
  sessions: Record<string, TerminalSession>;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onCloseOtherTabs?: (tabId: string) => void;
  onCloseTabsToRight?: (tabId: string) => void;
  onCloseTabsToLeft?: (tabId: string) => void;
  onReorderTab?: (tabId: string, targetIndex: number) => void;
  onRenameTab?: (tabId: string, label: string) => void;
  onToggleTabPin?: (tabId: string, pinned: boolean) => void;
  onAddTab?: () => void;
  onAddBrowserTab?: (url?: string) => void;
  onNavigateBrowserTab?: (tabId: string, url: string) => void;
  onReloadBrowserTab?: (tabId: string) => void;
  onSplitPane?: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane?: (tabId: string, leafId: string) => void;
  onSetRatio?: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes?: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane?: (tabId: string, leafId: string) => void;
  onTitleChange?: (tabId: string, title: string, sessionId?: string) => void;
  onBell?: (sessionId: string, tabId: string) => void;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
  leadingSpacer?: number;
};

export function TerminalSplitView({
  layout,
  sessions,
  onActivateTab = () => undefined,
  onCloseTab = () => undefined,
  onCloseOtherTabs,
  onCloseTabsToRight,
  onCloseTabsToLeft,
  onReorderTab,
  onRenameTab,
  onToggleTabPin,
  onAddTab = () => undefined,
  onAddBrowserTab = () => undefined,
  onNavigateBrowserTab = () => undefined,
  onReloadBrowserTab = () => undefined,
  onSplitPane = () => undefined,
  onClosePane = () => undefined,
  onSetRatio = () => undefined,
  onSwapPanes = () => undefined,
  onFocusPane = () => undefined,
  onTitleChange = () => undefined,
  onBell,
  unreadTabIds,
  activityByTabId,
  leadingSpacer = 0,
}: TerminalSplitViewProps) {
  const [draggedLeafId, setDraggedLeafId] = useState<{ tabId: string; leafId: string } | null>(null);

  const activeTab = layout.tabs.find((tab) => tab.id === layout.activeTabId) ?? layout.tabs[0] ?? null;
  const activeTabLayout = activeTab ? getTabPaneLayout(layout, activeTab) : null;
  const isBrowserTab = activeTab?.kind === "browser";

  const splitTab = (tabId: string, direction: PaneDirection) => {
    const tab = layout.tabs.find((candidate) => candidate.id === tabId);
    if (!tab || tab.kind === "browser") return;
    const tabLayout = getTabPaneLayout(layout, tab);
    const targetLeafId = tabLayout.activeLeafId ?? firstLeafId(tabLayout.root);
    if (targetLeafId) onSplitPane(tab.id, targetLeafId, direction);
  };

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden bg-terminal" data-testid="terminal-layout">
      <TabBar
        tabs={layout.tabs}
        activeTabId={activeTab?.id ?? ""}
        unreadTabIds={unreadTabIds}
        activityByTabId={activityByTabId}
        onActivate={onActivateTab}
        onClose={onCloseTab}
        onCloseOthers={onCloseOtherTabs}
        onCloseToRight={onCloseTabsToRight}
        onCloseToLeft={onCloseTabsToLeft}
        onReorderTabs={onReorderTab}
        onRenameTab={onRenameTab}
        onTogglePin={onToggleTabPin}
        onSplitRight={(tabId) => splitTab(tabId, "horizontal")}
        onSplitDown={(tabId) => splitTab(tabId, "vertical")}
        onAdd={onAddTab}
        onAddBrowser={onAddBrowserTab}
        leadingSpacer={leadingSpacer}
      />

      <div className="relative min-h-0 flex-1 overflow-hidden bg-terminal">
        {!activeTab ? null : isBrowserTab ? (
          <BrowserPane
            tab={activeTab}
            visible={true}
            onNavigate={(url) => onNavigateBrowserTab(activeTab.id, url)}
            onReload={() => onReloadBrowserTab(activeTab.id)}
          />
        ) : activeTabLayout ? (
          <PaneRenderer
            node={activeTabLayout.root}
            tab={activeTab}
            tabLayout={activeTabLayout}
            sessions={sessions}
            path=""
            draggedLeafId={draggedLeafId}
            setDraggedLeafId={setDraggedLeafId}
            onSplitPane={onSplitPane}
            onClosePane={onClosePane}
            onSetRatio={onSetRatio}
            onSwapPanes={onSwapPanes}
            onFocusPane={onFocusPane}
            onTitleChange={onTitleChange}
            onBell={onBell}
          />
        ) : null}
      </div>
    </div>
  );
}

function getTabPaneLayout(layout: LayoutState, tab: WorkspaceTab): TabPaneLayout {
  const isBrowserTab = tab.kind === "browser";
  return layout.layoutsByTabId?.[tab.id] ?? {
    root: { type: "leaf", leafId: "leaf-default" },
    activeLeafId: "leaf-default",
    expandedLeafId: null,
    sessionIdsByLeafId: { "leaf-default": isBrowserTab ? "" : tab.sessionId },
  };
}

function firstLeafId(node: PaneNode): string {
  let current = node;
  while (current.type === "split") current = current.first;
  return current.leafId;
}

type PaneRendererProps = {
  node: PaneNode;
  tab: WorkspaceTab;
  tabLayout: TabPaneLayout;
  sessions: Record<string, TerminalSession>;
  path: string;
  draggedLeafId: { tabId: string; leafId: string } | null;
  setDraggedLeafId: (value: { tabId: string; leafId: string } | null) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSetRatio: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
  onTitleChange: (tabId: string, title: string, sessionId?: string) => void;
  onBell?: (sessionId: string, tabId: string) => void;
};

function PaneRenderer(props: PaneRendererProps) {
  const { node, tab, tabLayout, sessions, path } = props;

  if (node.type === "leaf") {
    const defaultSessionId = tab.kind === "browser" ? "" : tab.sessionId;
    const sessionId = tabLayout.sessionIdsByLeafId[node.leafId] ?? defaultSessionId;
    const session = sessions[sessionId] ?? {
      id: sessionId,
      cwd: "",
      workspaceId: "",
      worktree: null,
      backendSessionId: null,
      lifecycle: "working" as const,
    };
    const isOnlyLeaf = tabLayout.root.type === "leaf";

    return (
      <PaneLeafView
        leafId={node.leafId}
        tab={tab}
        session={session}
        isOnlyLeaf={isOnlyLeaf}
        isActive={tabLayout.activeLeafId === node.leafId}
        draggedLeafId={props.draggedLeafId}
        setDraggedLeafId={props.setDraggedLeafId}
        onSplitPane={props.onSplitPane}
        onClosePane={props.onClosePane}
        onSwapPanes={props.onSwapPanes}
        onFocusPane={props.onFocusPane}
        onTitleChange={props.onTitleChange}
        onBell={props.onBell}
      />
    );
  }

  const isHorizontal = node.direction === "horizontal";
  const ratio = node.ratio ?? 0.5;

  return (
    <div
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden ${isHorizontal ? "flex-row" : "flex-col"}`}
      data-testid="pane-split"
      data-direction={node.direction}
    >
      <div
        className="relative min-h-0 min-w-0 overflow-hidden"
        style={{ flexBasis: `${ratio * 100}%`, flexGrow: 0, flexShrink: 0 }}
      >
        <PaneRenderer {...props} node={node.first} path={path ? `${path}.first` : "first"} />
      </div>

      <PaneResizeDivider
        direction={node.direction}
        ratio={ratio}
        onRatioChange={(newRatio) => props.onSetRatio(tab.id, path, newRatio)}
      />

      <div
        className="relative min-h-0 min-w-0 flex-1 overflow-hidden"
        style={{ flexBasis: `${(1 - ratio) * 100}%`, flexGrow: 1, flexShrink: 1 }}
      >
        <PaneRenderer {...props} node={node.second} path={path ? `${path}.second` : "second"} />
      </div>
    </div>
  );
}

type PaneLeafViewProps = {
  leafId: string;
  tab: WorkspaceTab;
  session: TerminalSession;
  isOnlyLeaf: boolean;
  isActive: boolean;
  draggedLeafId: { tabId: string; leafId: string } | null;
  setDraggedLeafId: (value: { tabId: string; leafId: string } | null) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
  onTitleChange: (tabId: string, title: string, sessionId?: string) => void;
  onBell?: (sessionId: string, tabId: string) => void;
};

function PaneLeafView({
  leafId,
  tab,
  session,
  isOnlyLeaf,
  isActive,
  draggedLeafId,
  setDraggedLeafId,
  onSplitPane,
  onClosePane,
  onSwapPanes,
  onFocusPane,
  onTitleChange,
  onBell,
}: PaneLeafViewProps) {
  const [isDragOver, setIsDragOver] = useState(false);
  const [isHoveredTop, setIsHoveredTop] = useState(false);

  const handleDragStart = (event: React.DragEvent) => {
    if (event.target instanceof Element && event.target.closest("button")) {
      event.preventDefault();
      return;
    }
    event.dataTransfer.setData("text/plain", `${tab.id}:${leafId}`);
    event.dataTransfer.effectAllowed = "move";
    setDraggedLeafId({ tabId: tab.id, leafId });
  };

  const handleDragEnd = () => {
    setDraggedLeafId(null);
    setIsDragOver(false);
  };

  const handleDragOver = (event: React.DragEvent) => {
    if (draggedLeafId && draggedLeafId.tabId === tab.id && draggedLeafId.leafId !== leafId) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      setIsDragOver(true);
    }
  };

  const handleDragLeave = (event: React.DragEvent) => {
    const related = event.relatedTarget;
    if (related instanceof Node && event.currentTarget.contains(related)) return;
    setIsDragOver(false);
  };

  const handleDrop = (event: React.DragEvent) => {
    event.preventDefault();
    setIsDragOver(false);
    if (draggedLeafId && draggedLeafId.tabId === tab.id && draggedLeafId.leafId !== leafId) {
      onSwapPanes(tab.id, draggedLeafId.leafId, leafId);
    }
    setDraggedLeafId(null);
  };

  return (
    <div
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden bg-terminal transition-all ${
        isDragOver ? "ring-2 ring-primary/80 ring-inset" : ""
      }`}
      data-testid="pane-leaf"
      data-leaf-id={leafId}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onClick={() => onFocusPane(tab.id, leafId)}
    >
      <div
        className="absolute inset-x-0 top-0 z-20 h-6"
        data-testid="pane-toolbar-hotspot"
        onMouseEnter={() => setIsHoveredTop(true)}
        onMouseLeave={(event) => {
          const related = event.relatedTarget;
          if (!(related instanceof Element) || !related.closest('[data-testid="pane-toolbar"]')) {
            setIsHoveredTop(false);
          }
        }}
      />

      <div
        draggable
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        className={`pointer-events-none absolute inset-x-0 top-0 z-30 flex h-6 cursor-grab items-center justify-between border-b border-border/30 bg-background/85 px-2 text-[11px] text-muted-foreground backdrop-blur-md transition-opacity duration-150 active:cursor-grabbing select-none ${
          isHoveredTop ? "pointer-events-auto opacity-100" : "opacity-0"
        }`}
        data-testid="pane-toolbar"
        onMouseEnter={() => setIsHoveredTop(true)}
        onMouseLeave={() => setIsHoveredTop(false)}
      >
        <div className="pointer-events-none flex min-w-0 items-center gap-1.5">
          <span className="truncate text-[10px] font-medium text-muted-foreground/70">{tab.label}</span>
        </div>

        <div className="flex items-center gap-0.5">
          <IconButton
            label="Split pane right"
            size="sm"
            className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
            onClick={(event) => {
              event.stopPropagation();
              onSplitPane(tab.id, leafId, "horizontal");
            }}
          >
            <Columns2 className="size-3" />
          </IconButton>
          <IconButton
            label="Split pane down"
            size="sm"
            className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
            onClick={(event) => {
              event.stopPropagation();
              onSplitPane(tab.id, leafId, "vertical");
            }}
          >
            <Rows2 className="size-3" />
          </IconButton>
          {!isOnlyLeaf ? (
            <IconButton
              label="Close split view"
              size="sm"
              className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
              onClick={(event) => {
                event.stopPropagation();
                onClosePane(tab.id, leafId);
              }}
            >
              <X className="size-3" />
            </IconButton>
          ) : null}
        </div>
      </div>

      <div className="h-full w-full min-h-0 flex-1 overflow-hidden">
        <TerminalPane
          key={`${leafId}:${session.id}`}
          session={session}
          active={isActive}
          onTitleChange={(title) => onTitleChange(tab.id, title, session.id)}
          onBell={() => onBell?.(session.id, tab.id)}
        />
      </div>
    </div>
  );
}

const MIN_PANE_SIZE_PX = 80;

type PaneResizeDividerProps = {
  direction: PaneDirection;
  ratio: number;
  onRatioChange: (ratio: number) => void;
};

function PaneResizeDivider({ direction, ratio, onRatioChange }: PaneResizeDividerProps) {
  const dividerRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const isHorizontal = direction === "horizontal";

  const handlePointerDown = (event: React.PointerEvent) => {
    event.preventDefault();
    const parent = dividerRef.current?.parentElement;
    if (!parent) return;

    draggingRef.current = true;
    document.body.style.cursor = isHorizontal ? "col-resize" : "row-resize";
    const parentRect = parent.getBoundingClientRect();

    const handlePointerMove = (pointerEvent: PointerEvent) => {
      if (!draggingRef.current) return;
      const totalSize = isHorizontal ? parentRect.width : parentRect.height;
      if (totalSize <= 0) return;
      if (totalSize <= MIN_PANE_SIZE_PX * 2) {
        onRatioChange(0.5);
        return;
      }
      const position = isHorizontal ? pointerEvent.clientX - parentRect.left : pointerEvent.clientY - parentRect.top;
      const minimumRatio = MIN_PANE_SIZE_PX / totalSize;
      const clampedRatio = Math.max(minimumRatio, Math.min(1 - minimumRatio, position / totalSize));
      onRatioChange(Number(clampedRatio.toFixed(4)));
    };

    const handlePointerUp = () => {
      draggingRef.current = false;
      document.body.style.cursor = "";
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerUp);
  };

  return (
    <div
      ref={dividerRef}
      role="separator"
      aria-label="Resize terminal panes"
      aria-orientation={isHorizontal ? "vertical" : "horizontal"}
      aria-valuenow={Math.round(ratio * 100)}
      data-divider-hit-target="true"
      onPointerDown={handlePointerDown}
      className={`no-drag relative z-20 flex shrink-0 touch-none items-center justify-center ${
        isHorizontal ? "w-1.5 cursor-col-resize hover:bg-primary/20" : "h-1.5 cursor-row-resize hover:bg-primary/20"
      }`}
    >
      <span
        data-testid="split-divider-line"
        className={isHorizontal ? "h-full w-px bg-border/80" : "h-px w-full bg-border/80"}
      />
    </div>
  );
}
