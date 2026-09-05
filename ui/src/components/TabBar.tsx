import { useDroppable } from "@dnd-kit/core";
import { SortableContext, horizontalListSortingStrategy } from "@dnd-kit/sortable";
import { Plus } from "lucide-react";
import React, { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { ActivitySummary } from "../lib/activity";
import {
  newBrowserTabUrl,
  resolveSupportedBrowserProfileId,
  supportedBrowserProfiles,
  useBrowserSettings,
} from "../lib/browserSettings";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { formatBindingLabel, isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import type { WorkspaceTab } from "../lib/types";
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
  onAdd: (shell?: string) => void;
  onAddBrowser?: (url?: string, profileId?: string) => void;
  onDuplicateBrowser?: (tabId: string, profileId?: string) => void;
  onAddMarkdown?: () => void;
  onAddMobileEmulator?: () => void;
  onOpenSettings?: () => void;
  agents?: Array<{ name: string; command: string; args: string; enabled?: boolean; available?: boolean }>;
  onLaunchAgent?: (agent: { name: string; command: string; args: string }) => void;
  defaultAgentId?: string | null;
  actions?: ReactNode;
  showAdd?: boolean;
  leadingSpacer?: number;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
};

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
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameCancelledRef = useRef(false);
  const addButtonRef = useRef<HTMLDivElement>(null);
  const menuUnlistenRef = useRef<(() => void) | null>(null);
  const isMac = isMacShortcutPlatform();
  const { settings: browserSettings } = useBrowserSettings();

  useEffect(() => {
    if (renamingTabId && !tabs.some((tab) => tab.id === renamingTabId)) setRenamingTabId(null);
  }, [renamingTabId, tabs]);

  useEffect(() => {
    return () => {
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
    };
  }, []);

  const openMenu = (
    command: Parameters<typeof openNativePopupMenu>[0],
    items: NativeMenuEntry[],
    position: { x: number; y: number },
    actions: Record<string, () => void>,
  ) => {
    menuUnlistenRef.current?.();
    void openNativePopupMenu(command, items, position, (id) => {
      actions[id]?.();
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
    })
      .then((unlisten) => {
        menuUnlistenRef.current = unlisten;
      })
      .catch(() => undefined);
  };

  const handleNewTabClick = () => {
    const rect = addButtonRef.current?.getBoundingClientRect();
    const position = rect
      ? { x: rect.left, y: rect.bottom + 4 }
      : { x: 0, y: 0 };
    const items: NativeMenuEntry[] = [
      {
        kind: "item",
        id: "new-terminal",
        label: "New Terminal",
        shortcut: shortcutLabel("tab.newTerminal", isMac),
      },
      {
        kind: "item",
        id: "new-browser",
        label: "New Browser Tab",
        shortcut: shortcutLabel("tab.newBrowser", isMac),
      },
    ];
    if (onAddMarkdown) {
      items.push({
        kind: "item",
        id: "new-markdown",
        label: "New Markdown",
        shortcut: formatBindingLabel({ key: "m", mod: true, shift: true }, isMac),
      });
    }
    if (onAddMobileEmulator) {
      items.push({
        kind: "item",
        id: "new-mobile-emulator",
        label: "New Mobile Emulator",
        shortcut: formatBindingLabel({ key: "e", mod: true, alt: true, shift: true }, isMac),
      });
    }
    const launchable = (agents ?? []).filter(
      (agent) => agent.enabled !== false && agent.available !== false,
    );
    const defaultIndex = launchable.findIndex(
      (agent) => defaultAgentId && defaultAgentId !== "none" && agent.name === defaultAgentId,
    );
    if (defaultIndex > 0) {
      const [selected] = launchable.splice(defaultIndex, 1);
      launchable.unshift(selected);
    }
    if (launchable.length > 0 && onLaunchAgent) {
      items.push({ kind: "separator" });
      for (const agent of launchable) {
        const base = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
        const isDefault = Boolean(
          defaultAgentId && defaultAgentId !== "none" && agent.name === defaultAgentId,
        );
        items.push({
          kind: "item",
          id: `agent:${agent.name}`,
          label: isDefault ? `${base} (Default)` : base,
          icon: `agent:${agent.name}`,
        });
      }
    }
    if (onOpenSettings) {
      items.push({ kind: "separator" });
      items.push({ kind: "item", id: "agent-settings", label: "Agent settings" });
    }
    const actions: Record<string, () => void> = {
      "new-terminal": () => onAdd(),
      "new-browser": () => onAddBrowser?.(newBrowserTabUrl(browserSettings)),
      "agent-settings": () => onOpenSettings?.(),
    };
    if (onAddMarkdown) actions["new-markdown"] = () => onAddMarkdown();
    if (onAddMobileEmulator) actions["new-mobile-emulator"] = () => onAddMobileEmulator();
    if (onLaunchAgent) {
      for (const agent of launchable) {
        actions[`agent:${agent.name}`] = () => onLaunchAgent(agent);
      }
    }
    openMenu("cmd_native_new_tab_menu", items, position, actions);
  };

  const handleStartRename = useCallback((tab: WorkspaceTab) => {
    if (!onRenameTab) return;
    renameCancelledRef.current = false;
    setRenamingTabId(tab.id);
    setRenameValue(tab.label);
  }, [onRenameTab]);

  const handleContextMenu = useCallback((event: React.MouseEvent, tab: WorkspaceTab) => {
    event.preventDefault();
    event.stopPropagation();
    const index = tabs.findIndex((candidate) => candidate.id === tab.id);
    const hasTabsToRight = index >= 0 && index < tabs.length - 1;
    const hasTabsToLeft = index > 0;
    const hasOtherTabs = tabs.length > 1;
    const canSplitTerminal = tab.kind !== "browser";
    const items: NativeMenuEntry[] = [];
    const actions: Record<string, () => void> = {};
    if (tab.kind === "browser" && onDuplicateBrowser) {
      const profiles = supportedBrowserProfiles(browserSettings);
      const currentProfileId = resolveSupportedBrowserProfileId(tab.profileId, browserSettings);
      items.push({
        kind: "submenu",
        label: "Duplicate browser tab",
        items: profiles.map((profile) => ({
          kind: "item" as const,
          id: `duplicate:${profile.id}`,
          label: profile.id === currentProfileId ? `${profile.name} (current)` : profile.name,
        })),
      });
      for (const profile of profiles) {
        actions[`duplicate:${profile.id}`] = () => onDuplicateBrowser(tab.id, profile.id);
      }
    }
    if (onSplitRight) {
      items.push({
        kind: "item",
        id: "split-right",
        label: "Split terminal right",
        enabled: canSplitTerminal,
      });
      actions["split-right"] = () => onSplitRight(tab.id);
    }
    if (onSplitDown) {
      items.push({
        kind: "item",
        id: "split-down",
        label: "Split terminal down",
        enabled: canSplitTerminal,
      });
      actions["split-down"] = () => onSplitDown(tab.id);
    }
    if (onMoveTabToSplit) {
      items.push({ kind: "separator" });
      const edges: Array<[TabDropEdge, string]> = [
        ["right", "Move Tab to Split Right"],
        ["bottom", "Move Tab to Split Down"],
        ["left", "Move Tab to Split Left"],
        ["top", "Move Tab to Split Up"],
      ];
      for (const [edge, label] of edges) {
        items.push({ kind: "item", id: `move:${edge}`, label });
        actions[`move:${edge}`] = () => onMoveTabToSplit(tab.id, edge);
      }
    }
    items.push({ kind: "separator" });
    if (onTogglePin) {
      items.push({ kind: "item", id: "pin", label: tab.pinned ? "Unpin tab" : "Pin tab" });
      actions["pin"] = () => onTogglePin(tab.id, !tab.pinned);
    }
    if (onRenameTab) {
      items.push({ kind: "item", id: "rename", label: "Rename tab" });
      actions["rename"] = () => handleStartRename(tab);
    }
    items.push({ kind: "separator" });
    items.push({ kind: "item", id: "close", label: "Close tab", enabled: !tab.pinned });
    actions["close"] = () => {
      if (!tab.pinned) onClose(tab.id);
    };
    if (onCloseOthers) {
      items.push({ kind: "item", id: "close-others", label: "Close other tabs", enabled: hasOtherTabs });
      actions["close-others"] = () => onCloseOthers(tab.id);
    }
    if (onCloseToRight) {
      items.push({ kind: "item", id: "close-right", label: "Close tabs to right", enabled: hasTabsToRight });
      actions["close-right"] = () => onCloseToRight(tab.id);
    }
    if (onCloseToLeft) {
      items.push({ kind: "item", id: "close-left", label: "Close tabs to left", enabled: hasTabsToLeft });
      actions["close-left"] = () => onCloseToLeft(tab.id);
    }
    openMenu("cmd_native_tab_context_menu", items, { x: event.clientX, y: event.clientY }, actions);
  }, [browserSettings, tabs, handleStartRename, onClose, onCloseOthers, onCloseToLeft, onCloseToRight, onDuplicateBrowser, onMoveTabToSplit, onSplitDown, onSplitRight, onTogglePin]);

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

  const sortableItems = useMemo(() => tabs.map((tab) => `tab:${tab.id}`), [tabs]);

  // The whole strip (including its blank area) is a drop target so a pane dragged onto the
  // tab row detaches into a new appended tab. Indexed tab targets outrank this via dropPriority.
  const strip = useDroppable({ id: `tab-strip:${groupId}`, data: { type: "tab-strip", groupId, tabCount: tabs.length } });

  return (
    <div
      ref={strip.setNodeRef}
      data-testid="tab-strip"
      data-tab-group-id={groupId}
      data-dnd-type="tab-strip"
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
          <div ref={addButtonRef} className="no-drag relative flex shrink-0 items-center">
            <IconButton
              label="New tab"
              size="sm"
              className="no-drag my-auto ml-1"
              onClick={handleNewTabClick}
            >
              <Plus className="size-3.5" />
            </IconButton>
          </div>
        ) : null}
      </SortableContext>

      <div className="flex-1" />
      {actions ? <div className="no-drag ml-0.5 flex shrink-0 items-center gap-0.5">{actions}</div> : null}
    </div>
  );
}
