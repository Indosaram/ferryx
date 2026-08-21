import { Columns2, GripVertical, Rows2, X } from "lucide-react";
import React, { useRef, useState } from "react";

import type { LayoutState, TabPaneLayout, TerminalSession, WorkspaceTab } from "../lib/types";
import type { PaneDirection, PaneNode } from "../state/paneTree";
import { TabBar } from "./TabBar";
import { TerminalPane } from "./TerminalPane";
import { BrowserPane } from "./BrowserPane";
import { IconButton } from "./ui/IconButton";

type TerminalSplitViewProps = {
  layout: LayoutState;
  sessions: Record<string, TerminalSession>;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onAddTab?: () => void;
  onAddBrowserTab?: () => void;
  onNavigateBrowserTab?: (tabId: string, url: string) => void;
  onReloadBrowserTab?: (tabId: string) => void;
  onSplitPane?: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane?: (tabId: string, leafId: string) => void;
  onSetRatio?: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes?: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane?: (tabId: string, leafId: string) => void;
  onTitleChange?: (tabId: string, title: string) => void;
  onBell?: (sessionId: string, tabId: string) => void;
  unreadTabIds?: Record<string, boolean>;
  leadingSpacer?: number;
};

export function TerminalSplitView({
  layout,
  sessions,
  onActivateTab = () => undefined,
  onCloseTab = () => undefined,
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
  leadingSpacer = 0,
}: TerminalSplitViewProps) {
  const [draggedLeafId, setDraggedLeafId] = useState<{ tabId: string; leafId: string } | null>(null);

  const activeTab = layout.tabs.find((t) => t.id === layout.activeTabId) ?? layout.tabs[0] ?? null;
  if (!activeTab) {
    return <div className="relative flex-1 overflow-hidden bg-terminal" data-testid="terminal-layout" />;
  }

  const isBrowserTab = activeTab.kind === "browser";

  const activeTabLayout: TabPaneLayout = layout.layoutsByTabId?.[activeTab.id] ?? {
    root: { type: "leaf", leafId: "leaf-default" },
    activeLeafId: "leaf-default",
    expandedLeafId: null,
    sessionIdsByLeafId: { "leaf-default": isBrowserTab ? "" : activeTab.sessionId },
  };

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden bg-terminal" data-testid="terminal-layout">
      <TabBar
        tabs={layout.tabs}
        activeTabId={activeTab.id}
        unreadTabIds={unreadTabIds}
        onActivate={onActivateTab}
        onClose={onCloseTab}
        onAdd={onAddTab}
        onAddBrowser={onAddBrowserTab}
        leadingSpacer={leadingSpacer}
      />

      <div className="relative min-h-0 flex-1 overflow-hidden bg-terminal">
        {isBrowserTab ? (
          <BrowserPane
            tab={activeTab}
            visible={true}
            onNavigate={(url) => onNavigateBrowserTab(activeTab.id, url)}
            onReload={() => onReloadBrowserTab(activeTab.id)}
          />
        ) : (
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
        )}
      </div>
    </div>
  );
}

type PaneRendererProps = {
  node: PaneNode;
  tab: WorkspaceTab;
  tabLayout: TabPaneLayout;
  sessions: Record<string, TerminalSession>;
  path: string;
  draggedLeafId: { tabId: string; leafId: string } | null;
  setDraggedLeafId: (val: { tabId: string; leafId: string } | null) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSetRatio: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
  onTitleChange: (tabId: string, title: string) => void;
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
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden ${
        isHorizontal ? "flex-row" : "flex-col"
      }`}
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
  setDraggedLeafId: (val: { tabId: string; leafId: string } | null) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
  onTitleChange: (tabId: string, title: string) => void;
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

  const handleDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData("text/plain", `${tab.id}:${leafId}`);
    e.dataTransfer.effectAllowed = "move";
    setDraggedLeafId({ tabId: tab.id, leafId });
  };

  const handleDragEnd = () => {
    setDraggedLeafId(null);
    setIsDragOver(false);
  };

  const handleDragOver = (e: React.DragEvent) => {
    if (draggedLeafId && draggedLeafId.tabId === tab.id && draggedLeafId.leafId !== leafId) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      setIsDragOver(true);
    }
  };

  const handleDragLeave = () => {
    setIsDragOver(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragOver(false);
    if (draggedLeafId && draggedLeafId.tabId === tab.id && draggedLeafId.leafId !== leafId) {
      onSwapPanes(tab.id, draggedLeafId.leafId, leafId);
    }
    setDraggedLeafId(null);
  };

  return (
    <div
      className={`group/pane relative flex h-full w-full min-h-0 min-w-0 overflow-hidden bg-terminal transition-all ${
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
        className="pointer-events-none absolute top-1.5 left-2 z-20 flex h-7 items-center gap-1.5 rounded-md border border-border/80 bg-card/90 px-2 text-[11px] text-muted-foreground shadow-sm backdrop-blur-sm opacity-0 transition-opacity duration-150 group-hover/pane:pointer-events-auto group-hover/pane:opacity-100 select-none"
        data-testid="pane-toolbar"
      >
        <div
          draggable
          onDragStart={handleDragStart}
          onDragEnd={handleDragEnd}
          className="flex cursor-grab items-center p-0.5 text-muted-foreground hover:text-foreground active:cursor-grabbing"
          title="Drag to reorder pane"
          data-testid="pane-drag-handle"
        >
          <GripVertical className="size-3.5" />
        </div>
        <span className="truncate font-mono text-[11px] text-foreground/80">
          {tab.label}
        </span>

        <div className="ml-1 flex items-center gap-0.5 border-l border-border/60 pl-1">
          <IconButton
            label="Split pane right"
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              onSplitPane(tab.id, leafId, "horizontal");
            }}
          >
            <Columns2 className="size-3.5" />
          </IconButton>
          <IconButton
            label="Split pane down"
            size="sm"
            onClick={(e) => {
              e.stopPropagation();
              onSplitPane(tab.id, leafId, "vertical");
            }}
          >
            <Rows2 className="size-3.5" />
          </IconButton>
          {!isOnlyLeaf && (
            <IconButton
              label="Close split view"
              size="sm"
              onClick={(e) => {
                e.stopPropagation();
                onClosePane(tab.id, leafId);
              }}
            >
              <X className="size-3.5" />
            </IconButton>
          )}
        </div>
      </div>

      <div className="h-full w-full min-h-0 flex-1 overflow-hidden">
        <TerminalPane
          key={`${leafId}:${session.id}`}
          session={session}
          active={isActive}
          onTitleChange={(title) => onTitleChange(tab.id, title)}
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

  const handlePointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    draggingRef.current = true;
    document.body.style.cursor = isHorizontal ? "col-resize" : "row-resize";

    const parent = dividerRef.current?.parentElement;
    if (!parent) return;
    const parentRect = parent.getBoundingClientRect();

    const handlePointerMove = (ev: PointerEvent) => {
      if (!draggingRef.current) return;
      const totalSize = isHorizontal ? parentRect.width : parentRect.height;
      if (totalSize <= 0) return;
      const pos = isHorizontal ? ev.clientX - parentRect.left : ev.clientY - parentRect.top;
      const clampedRatio = Math.max(
        MIN_PANE_SIZE_PX / totalSize,
        Math.min(1 - MIN_PANE_SIZE_PX / totalSize, pos / totalSize)
      );
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
        isHorizontal
          ? "w-1.5 cursor-col-resize hover:bg-primary/20"
          : "h-1.5 cursor-row-resize hover:bg-primary/20"
      }`}
    >
      <span
        data-testid="split-divider-line"
        className={isHorizontal ? "h-full w-px bg-border/80" : "h-px w-full bg-border/80"}
      />
    </div>
  );
}
