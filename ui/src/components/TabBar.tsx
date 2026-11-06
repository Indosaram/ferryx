import { useDroppable } from "@dnd-kit/core";
import { SortableContext, horizontalListSortingStrategy } from "@dnd-kit/sortable";
import { Plus } from "lucide-react";
import React, { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type { ActivitySummary } from "../lib/activity";
import { useAdvancedFeatures } from "../lib/advancedFeatures";
import {
  newBrowserTabUrl,
  resolveSupportedBrowserProfileId,
  supportedBrowserProfiles,
  useBrowserSettings,
} from "../lib/browserSettings";
import { copyTextToClipboard } from "../lib/clipboard";
import { disambiguateFileTabLabels } from "../lib/fileTabLabels";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { chatSupportedAgents, loadLastChatAgentId, orderChatAgents } from "../lib/paneChatPreferences";
import {
  requestSessionLifecycleAction,
  useSleepingSessionIds,
} from "../lib/sessionLifecycle";
import { formatBindingLabel, isMacShortcutPlatform, shortcutLabel } from "../lib/shortcuts";
import { useShortcutOverrides } from "../lib/shortcutOverrides";
import { getSystemPermissionsStatus } from "../lib/tauri";
import { isTerminalTab, type FileTab, type WorkspaceTab } from "../lib/types";
import type { TerminalTabDisplay } from "../state/tabDisplay";
import { SortableTab } from "./tab-dnd/SortableTab";
import type { TabDropEdge } from "./tab-dnd/tabDragTypes";
import { IconButton } from "./ui/IconButton";
import { fileManagerActionLabel } from "./WorktreeList";

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
  onResetAgentState?: (tabId: string) => void;
  /**
   * Multi-window: moves a terminal tab to another window (`null` opens a new one). Only passed
   * when the multi-window shell is available, so the menu items stay hidden otherwise.
   */
  onMoveTabToWindow?: (tabId: string, targetLabel: string | null) => void;
  /** Labels of the other open windows, read each time the tab menu opens. */
  listOtherWindows?: () => Promise<string[]>;
  /** Session that the tab menu's Restart acts on. Defaults to the tab's own session. */
  sessionIdForLifecycle?: (tab: WorkspaceTab) => string | null;
  filePathsForTab?: (tab: FileTab) => { absolute: string; relative: string | null };
  onReloadFileTab?: (tabId: string) => void;
  onOpenFileExternally?: (tabId: string) => void;
  onRevealFileTab?: (tabId: string) => void;
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
  /** Local-only: opens a new tab running `agent` with its pane in chat view. Omit to hide New Chat. */
  onNewChat?: (agent: { name: string; command: string; args: string }) => void;
  /** Local-only: splits the tab's focused pane with `agent` in chat view. Omit to hide Split Chat. */
  onSplitChat?: (tabId: string, direction: "horizontal" | "vertical", agent: { name: string; command: string; args: string }) => void;
  defaultAgentId?: string | null;
  actions?: ReactNode;
  showAdd?: boolean;
  leadingSpacer?: number;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
  /** Tabs the supervisor typed into moments ago; they carry the eye. */
  supervisorActingTabIds?: Record<string, boolean>;
  /** Resolved live text/tooltip per terminal tab; falls back to `tab.label` when absent. */
  tabDisplayById?: Record<string, TerminalTabDisplay | undefined>;
};

const WINDOWS_SHELL_OPTIONS = [
  { id: "new-terminal:pwsh", shell: "pwsh", label: "PowerShell" },
  { id: "new-terminal:powershell", shell: "powershell", label: "Windows PowerShell" },
  { id: "new-terminal:cmd", shell: "cmd", label: "Command Prompt" },
  { id: "new-terminal:wsl", shell: "wsl", label: "WSL" },
] as const;

/** Menu label for a desktop window label (`main` or `ws-<id>`). */
export function windowMenuLabel(label: string): string {
  return label === "main" ? "Main Window" : `Window ${label.replace(/^ws-/, "").slice(0, 6)}`;
}

/**
 * True when `label` is a label this app generates for `path` (the basename or a
 * trailing path suffix from disambiguation), as opposed to a user rename.
 */
function isGeneratedFileTabLabel(label: string, path: string): boolean {
  if (label === path) return true;
  const segments = path.split(/[/\\]/).filter((segment) => segment.length > 0);
  for (let count = 1; count <= segments.length; count++) {
    if (segments.slice(segments.length - count).join("/") === label) return true;
  }
  return false;
}

/**
 * Display labels for file tabs, derived from the file tabs currently in this strip so a
 * closed duplicate drops its sibling's disambiguation. User-renamed labels are kept.
 */
export function computeFileTabLabels(tabs: readonly WorkspaceTab[]): Record<string, string> {
  const result: Record<string, string> = {};
  const generated: FileTab[] = [];
  for (const tab of tabs) {
    if (tab.kind !== "file") continue;
    if (tab.customLabel !== undefined) result[tab.id] = tab.customLabel;
    else if (isGeneratedFileTabLabel(tab.label, tab.path)) generated.push(tab);
    else result[tab.id] = tab.label;
  }
  const labels = disambiguateFileTabLabels(generated.map((tab) => tab.path));
  generated.forEach((tab, index) => {
    result[tab.id] = labels[index]!;
  });
  return result;
}

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
  onResetAgentState,
  onMoveTabToWindow,
  listOtherWindows,
  sessionIdForLifecycle,
  filePathsForTab,
  onReloadFileTab,
  onOpenFileExternally,
  onRevealFileTab,
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
  onNewChat,
  onSplitChat,
  defaultAgentId,
  actions,
  showAdd = true,
  leadingSpacer = 0,
  unreadTabIds,
  activityByTabId,
  supervisorActingTabIds,
  tabDisplayById,
}: TabBarProps) {
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameCancelledRef = useRef(false);
  const fileTabLabelById = useMemo(() => computeFileTabLabels(tabs), [tabs]);
  const addButtonRef = useRef<HTMLDivElement>(null);
  const menuUnlistenRef = useRef<(() => void) | null>(null);
  const [hostPlatform, setHostPlatform] = useState<string | null>(null);
  const isMac = isMacShortcutPlatform();
  const { overrides: shortcutOverrides } = useShortcutOverrides();
  const newTerminalChord = shortcutLabel("tab.newTerminal", isMac, shortcutOverrides);
  // The shell profile menu follows the HOST platform reported by the backend, never
  // the browser OS: the remote web client can drive a host of a different platform.
  const isWindows = hostPlatform === "windows";
  const { settings: browserSettings } = useBrowserSettings();
  const advancedFeatures = useAdvancedFeatures();
  // Hidden entry point only: the tab.newBrowser shortcut stays bound in App.
  const offerBrowser = Boolean(onAddBrowser) && advancedFeatures;
  const sleepingSessionIds = useSleepingSessionIds();

  useEffect(() => {
    if (renamingTabId && !tabs.some((tab) => tab.id === renamingTabId)) setRenamingTabId(null);
  }, [renamingTabId, tabs]);

  useEffect(() => {
    return () => {
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    // The IPC binding may be unavailable or partial (missing export, non-function, sync throw),
    // and tab-bar rendering must never break over it. Deferring the call into the promise chain
    // routes any such failure into the catch below. The platform only picks which shell presets
    // the menu offers, so an unknown platform falls back to the non-Windows list.
    void Promise.resolve()
      .then(() => getSystemPermissionsStatus())
      .then((status) => {
        if (!cancelled) setHostPlatform(status?.platform ?? null);
      })
      .catch(() => {
        if (!cancelled) setHostPlatform(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const chatAgents = useCallback(
    () =>
      orderChatAgents(
        chatSupportedAgents((agents ?? []).filter((agent) => agent.enabled !== false && agent.available !== false)),
        loadLastChatAgentId(),
        defaultAgentId,
      ),
    [agents, defaultAgentId],
  );

  const openMenu = (
    command: Parameters<typeof openNativePopupMenu>[0],
    items: NativeMenuEntry[],
    position: { x: number; y: number },
    actions: Record<string, () => void>,
  ) => {
    menuUnlistenRef.current?.();
    const controller = new AbortController();
    menuUnlistenRef.current = () => controller.abort();
    void openNativePopupMenu(command, items, position, (id) => {
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
      actions[id]?.();
    }, controller.signal)
      .catch((error: unknown) => console.warn("Could not open native tab menu", error));
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
        shortcut: shortcutLabel("tab.newTerminal", isMac, shortcutOverrides),
      },
    ];
    const newChatAgents = onNewChat ? chatAgents() : [];
    if (newChatAgents.length > 0) {
      items.push({
        kind: "submenu",
        label: "New Chat",
        items: newChatAgents.map((agent, index) => ({
          kind: "item" as const,
          id: `new-chat:${agent.name}`,
          label: agent.name.charAt(0).toUpperCase() + agent.name.slice(1),
          icon: `agent:${agent.name}`,
          ...(index === 0 ? { shortcut: shortcutLabel("tab.newChat", isMac, shortcutOverrides) } : {}),
        })),
      });
    }
    if (isWindows) {
      items.push({
        kind: "submenu",
        label: "New Terminal Profile",
        items: WINDOWS_SHELL_OPTIONS.map((option) => ({
          kind: "item",
          id: option.id,
          label: option.label,
        })),
      });
    }
    if (advancedFeatures) {
      items.push({
        kind: "item",
        id: "new-browser",
        label: "New Browser Tab",
        shortcut: shortcutLabel("tab.newBrowser", isMac, shortcutOverrides),
      });
    }
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
    if (isWindows) {
      for (const option of WINDOWS_SHELL_OPTIONS) {
        actions[option.id] = () => onAdd(option.shell);
      }
    }
    if (onAddMarkdown) actions["new-markdown"] = () => onAddMarkdown();
    if (onAddMobileEmulator) actions["new-mobile-emulator"] = () => onAddMobileEmulator();
    for (const agent of newChatAgents) {
      actions[`new-chat:${agent.name}`] = () => onNewChat?.(agent);
    }
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
    setRenameValue(
      (isTerminalTab(tab) && tab.customLabel) || tabDisplayById?.[tab.id]?.text || fileTabLabelById[tab.id] || tab.label,
    );
  }, [onRenameTab, tabDisplayById, fileTabLabelById]);

  const handleContextMenu = useCallback((event: React.MouseEvent, tab: WorkspaceTab) => {
    event.preventDefault();
    event.stopPropagation();
    const position = { x: event.clientX, y: event.clientY };
    const index = tabs.findIndex((candidate) => candidate.id === tab.id);
    const hasTabsToRight = index >= 0 && index < tabs.length - 1;
    const hasTabsToLeft = index > 0;
    const hasOtherTabs = tabs.length > 1;
    const canSplitTerminal = isTerminalTab(tab);
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
    const splitChatAgents = onSplitChat && canSplitTerminal ? chatAgents() : [];
    if (onSplitChat && splitChatAgents.length > 0) {
      const splits: Array<["horizontal" | "vertical", string, string]> = [
        ["horizontal", "right", "Split chat right"],
        ["vertical", "down", "Split chat down"],
      ];
      for (const [direction, edge, label] of splits) {
        items.push({
          kind: "submenu",
          label,
          items: splitChatAgents.map((agent) => ({
            kind: "item" as const,
            id: `split-chat-${edge}:${agent.name}`,
            label: agent.name.charAt(0).toUpperCase() + agent.name.slice(1),
            icon: `agent:${agent.name}`,
          })),
        });
        for (const agent of splitChatAgents) {
          actions[`split-chat-${edge}:${agent.name}`] = () => onSplitChat(tab.id, direction, agent);
        }
      }
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
    if (tab.kind === "file") {
      const filePaths = filePathsForTab ? filePathsForTab(tab) : null;
      const fileItems: NativeMenuEntry[] = [];
      if (filePaths) {
        fileItems.push({ kind: "item", id: "copy-path", label: "Copy Path" });
        actions["copy-path"] = () => {
          void copyTextToClipboard(filePaths.absolute);
        };
        fileItems.push({
          kind: "item",
          id: "copy-relative-path",
          label: "Copy Relative Path",
          enabled: filePaths.relative !== null,
        });
        if (filePaths.relative !== null) {
          actions["copy-relative-path"] = () => {
            void copyTextToClipboard(filePaths.relative!);
          };
        }
      }
      if (onRevealFileTab) {
        fileItems.push({
          kind: "item",
          id: "reveal",
          label: fileManagerActionLabel(),
        });
        actions["reveal"] = () => onRevealFileTab(tab.id);
      }
      if (onOpenFileExternally) {
        fileItems.push({
          kind: "item",
          id: "open-externally",
          label: "Open Externally",
        });
        actions["open-externally"] = () => onOpenFileExternally(tab.id);
      }
      if (onReloadFileTab) {
        fileItems.push({
          kind: "item",
          id: "reload",
          label: "Reload",
        });
        actions["reload"] = () => onReloadFileTab(tab.id);
      }
      if (fileItems.length > 0) {
        items.push({ kind: "separator" });
        items.push(...fileItems);
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
    if (canSplitTerminal && onResetAgentState) {
      items.push({ kind: "item", id: "reset-agent-state", label: "Reset Agent State", icon: "refresh" });
      actions["reset-agent-state"] = () => onResetAgentState(tab.id);
    }
    if (isTerminalTab(tab)) {
      const lifecycleSessionId = sessionIdForLifecycle?.(tab) ?? tab.sessionId;
      items.push({ kind: "separator" });
      items.push({ kind: "item", id: "restart-session", label: "Restart Session" });
      actions["restart-session"] = () => {
        onActivate(tab.id);
        requestSessionLifecycleAction("restart", lifecycleSessionId);
      };
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
    if (!(onMoveTabToWindow && isTerminalTab(tab))) {
      openMenu("cmd_native_tab_context_menu", items, position, actions);
      return;
    }
    // The move entries sit above the close group; the window list is read when the menu opens so
    // a window opened or closed since the last menu is reflected.
    const closeIndex = items.findIndex((entry) => entry.kind === "item" && entry.id === "close");
    const insertAt = closeIndex < 0 ? items.length : closeIndex;
    const openWithWindows = (otherWindows: string[]) => {
      const moveItems: NativeMenuEntry[] = [{ kind: "item", id: "move-window:new", label: "Move to New Window" }];
      actions["move-window:new"] = () => onMoveTabToWindow(tab.id, null);
      if (otherWindows.length > 0) {
        moveItems.push({
          kind: "submenu",
          label: "Move to Window",
          items: otherWindows.map((label) => ({
            kind: "item" as const,
            id: `move-window:${label}`,
            label: windowMenuLabel(label),
          })),
        });
        for (const label of otherWindows) actions[`move-window:${label}`] = () => onMoveTabToWindow(tab.id, label);
      }
      moveItems.push({ kind: "separator" });
      const withMove = [...items.slice(0, insertAt), ...moveItems, ...items.slice(insertAt)];
      openMenu("cmd_native_tab_context_menu", withMove, position, actions);
    };
    if (!listOtherWindows) {
      openWithWindows([]);
      return;
    }
    void listOtherWindows().catch(() => [] as string[]).then(openWithWindows);
  }, [browserSettings, tabs, handleStartRename, onActivate, onClose, onCloseOthers, onCloseToLeft, onCloseToRight, onDuplicateBrowser, onMoveTabToSplit, onMoveTabToWindow, listOtherWindows, onResetAgentState, onSplitDown, onSplitRight, onSplitChat, chatAgents, onTogglePin, sessionIdForLifecycle, filePathsForTab, onReloadFileTab, onOpenFileExternally, onRevealFileTab]);

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
            const sleeping = isTerminalTab(tab) && sleepingSessionIds.has(tab.sessionId);
            const fileTitle = tab.kind === "file" && filePathsForTab ? filePathsForTab(tab).absolute : undefined;
            return (
              <SortableTab
                key={tab.id}
                tab={tab}
                groupId={groupId}
                index={index}
                active={active}
                unread={Boolean(unreadTabIds?.[tab.id] && !active)}
                sleeping={sleeping}
                activity={activityByTabId?.[tab.id]}
                supervisorActing={Boolean(supervisorActingTabIds?.[tab.id])}
                displayText={tabDisplayById?.[tab.id]?.text ?? fileTabLabelById[tab.id]}
                displayTooltip={tabDisplayById?.[tab.id]?.tooltip}
                fileTitle={fileTitle}
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
              title={newTerminalChord ? `New tab (${newTerminalChord})` : "New tab"}
              data-shortcut={offerBrowser ? "tab.newTerminal tab.newBrowser" : "tab.newTerminal"}
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
