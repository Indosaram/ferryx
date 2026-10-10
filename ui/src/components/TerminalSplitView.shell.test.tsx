import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, SystemPermissionsStatus, TerminalSession } from "../lib/types";
import { resetNotificationSettings } from "../lib/notificationSettings";
import { TerminalSplitView } from "./TerminalSplitView";

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

const nativeMenu = vi.hoisted(() => ({
  lastCall: null as null | {
    command: string;
    items: Array<Record<string, unknown>>;
    position: { x: number; y: number };
    onAction: (id: string) => void;
  },
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: vi.fn(
    async (
      command: string,
      items: Array<Record<string, unknown>>,
      position: { x: number; y: number },
      onAction: (id: string) => void,
    ) => {
      nativeMenu.lastCall = { command, items, position, onAction };
      return () => undefined;
    },
  ),
}));

const terminalPaneProps = vi.hoisted(() => ({ bySessionId: new Map<string, any>() }));

vi.mock("./TerminalPane", () => ({
  TerminalPane: (props: any) => {
    terminalPaneProps.bySessionId.set(props.session?.id ?? "", props);
    return (
      <div
        data-testid="terminal-pane"
        data-session-id={props.session?.id}
      />
    );
  },
}));

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
}));

vi.mock("../lib/tauri", async () => {
  const actual = await vi.importActual<typeof import("../lib/tauri")>("../lib/tauri");
  return {
    ...actual,
    getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  };
});

/**
 * Reports `platform` as the HOST platform and resolves once TabBar has actually asked for
 * it, so tests await the exact IPC request instead of a fixed delay.
 */
function stubHostPlatform(platform: string) {
  return new Promise<void>((resolve) => {
    mockTauri.getSystemPermissionsStatus.mockImplementation(() => {
      resolve();
      return Promise.resolve({ platform } as SystemPermissionsStatus);
    });
  });
}

function findMenuEntryRecursively(
  entries: Array<Record<string, unknown>>,
  predicate: (entry: Record<string, unknown>) => boolean,
): Record<string, unknown> | null {
  for (const entry of entries) {
    if (predicate(entry)) return entry;
    if (entry.kind === "submenu" && Array.isArray(entry.items)) {
      const child = findMenuEntryRecursively(
        entry.items as Array<Record<string, unknown>>,
        predicate,
      );
      if (child) return child;
    }
  }
  return null;
}

describe("TerminalSplitView Windows shell selection forwarding", () => {
  const originalPlatform = navigator.platform;
  const originalUserAgent = navigator.userAgent;

  beforeEach(() => {
    resetNotificationSettings();
    terminalPaneProps.bySessionId.clear();
    // The browser OS is deliberately the opposite of the host: the shell profile menu must
    // follow the HOST platform the backend reports, never navigator.
    Object.defineProperty(navigator, "platform", { value: "MacIntel", configurable: true });
    Object.defineProperty(navigator, "userAgent", {
      value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
      configurable: true,
    });
  });

  afterEach(() => {
    cleanup();
    resetNotificationSettings();
    nativeMenu.lastCall = null;
    Object.defineProperty(navigator, "platform", { value: originalPlatform, configurable: true });
    Object.defineProperty(navigator, "userAgent", { value: originalUserAgent, configurable: true });
  });

  function singleTabLayout(): LayoutState {
    const tabId = "tab-1";
    const sessionId = "session-1";
    const leafId = "leaf-1";
    return {
      tabs: [{ id: tabId, label: "main", sessionId }],
      activeTabId: tabId,
      layoutsByTabId: {
        [tabId]: {
          root: { type: "leaf", leafId },
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: sessionId },
        },
      },
      tabGroups: {
        "group-default": { id: "group-default", tabIds: [tabId], activeTabId: tabId },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
    };
  }

  const sessions: Record<string, TerminalSession> = {
    "session-1": {
      id: "session-1",
      cwd: "/repo",
      worktreePath: "/repo",
      workspaceId: "ws-1",
      worktree: null,
      backendSessionId: "backend-1",
      lifecycle: "working",
    },
  };

  it("forwards cmd shell selection from TabBar through TabGroupView to onAddTab on Windows", async () => {
    const onAddTab = vi.fn();
    const ready = stubHostPlatform("windows");
    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={sessions}
        onAddTab={onAddTab}
      />,
    );
    await act(async () => {
      await ready;
    });

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(nativeMenu.lastCall).not.toBeNull();

    const cmdEntry = findMenuEntryRecursively(
      nativeMenu.lastCall!.items,
      (entry) =>
        entry.id === "new-terminal:cmd" ||
        (typeof entry.label === "string" && entry.label.includes("Command Prompt")),
    );
    expect(cmdEntry).not.toBeNull();

    nativeMenu.lastCall!.onAction((cmdEntry as { id: string }).id);
    expect(onAddTab).toHaveBeenCalledWith("cmd");
  });

  it("forwards pwsh, powershell, and wsl shells from TabBar through TabGroupView to onAddTab on Windows", async () => {
    const expectedShells = [
      { id: "new-terminal:pwsh", shell: "pwsh", label: "PowerShell" },
      { id: "new-terminal:powershell", shell: "powershell", label: "Windows PowerShell" },
      { id: "new-terminal:wsl", shell: "wsl", label: "WSL" },
    ];

    for (const target of expectedShells) {
      const onAddTab = vi.fn();
      const ready = stubHostPlatform("windows");
      const { unmount } = render(
        <TerminalSplitView
          layout={singleTabLayout()}
          sessions={sessions}
          onAddTab={onAddTab}
        />,
      );
      await act(async () => {
        await ready;
      });

      fireEvent.click(screen.getByRole("button", { name: "New tab" }));
      expect(nativeMenu.lastCall).not.toBeNull();

      const entry = findMenuEntryRecursively(
        nativeMenu.lastCall!.items,
        (item) => item.id === target.id || item.label === target.label,
      );
      expect(entry).not.toBeNull();

      nativeMenu.lastCall!.onAction((entry as { id: string }).id);
      expect(onAddTab).toHaveBeenCalledWith(target.shell);

      unmount();
    }
  });

  it("forwards generic default New Terminal action without shell to onAddTab", async () => {
    const onAddTab = vi.fn();
    const ready = stubHostPlatform("windows");
    render(
      <TerminalSplitView
        layout={singleTabLayout()}
        sessions={sessions}
        onAddTab={onAddTab}
      />,
    );
    await act(async () => {
      await ready;
    });

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(nativeMenu.lastCall).not.toBeNull();

    const defaultTerminal = findMenuEntryRecursively(
      nativeMenu.lastCall!.items,
      (entry) => entry.id === "new-terminal",
    );
    expect(defaultTerminal).not.toBeNull();

    nativeMenu.lastCall!.onAction("new-terminal");
    expect(onAddTab).toHaveBeenCalledWith();
  });

  it("forwards shell selection to onAddTab when all tabs are closed (empty layout fallback)", async () => {
    const onAddTab = vi.fn();
    const emptyLayout: LayoutState = { tabs: [], activeTabId: null, layoutsByTabId: {} };

    const ready = stubHostPlatform("windows");
    render(
      <TerminalSplitView
        layout={emptyLayout}
        sessions={{}}
        onAddTab={onAddTab}
      />,
    );
    await act(async () => {
      await ready;
    });

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    expect(nativeMenu.lastCall).not.toBeNull();

    const cmdEntry = findMenuEntryRecursively(
      nativeMenu.lastCall!.items,
      (entry) =>
        entry.id === "new-terminal:cmd" ||
        (typeof entry.label === "string" && entry.label.includes("Command Prompt")),
    );
    expect(cmdEntry).not.toBeNull();

    nativeMenu.lastCall!.onAction((cmdEntry as { id: string }).id);
    expect(onAddTab).toHaveBeenCalledWith("cmd");
  });

  describe("ui-attach-transport-2 reconnect routing", () => {
    it("wires onReconnect to local shell handler instead of onReconnectAgentSession for non-agent shells", async () => {
      const onReconnectAgentSession = vi.fn();
      const onReconnectLocalSession = vi.fn();
      const onOpenNewShell = vi.fn();

      const localNonAgentSession: TerminalSession = {
        id: "session-local-plain",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-1",
        worktree: null,
        backendSessionId: "backend-local-1",
        lifecycle: "exited",
      };

      const layout: LayoutState = {
        tabs: [{ id: "tab-local", label: "Local", sessionId: "session-local-plain" }],
        activeTabId: "tab-local",
        layoutsByTabId: {
          "tab-local": {
            root: { type: "leaf", leafId: "leaf-local" },
            activeLeafId: "leaf-local",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-local": "session-local-plain" },
          },
        },
      };

      render(
        <TerminalSplitView
          layout={layout}
          sessions={{ "session-local-plain": localNonAgentSession }}
          onReconnectAgentSession={onReconnectAgentSession}
          onReconnectLocalSession={onReconnectLocalSession}
          onOpenNewShell={onOpenNewShell}
        />,
      );

      await act(async () => {
        await terminalPaneProps.bySessionId.get("session-local-plain").onReconnect("session-local-plain");
      });

      expect(onReconnectLocalSession).toHaveBeenCalledWith("session-local-plain");
      expect(onReconnectAgentSession).not.toHaveBeenCalled();
    });

    it("wires onReconnect to onReconnectAgentSession for agent sessions", async () => {
      const onReconnectAgentSession = vi.fn();
      const onReconnectLocalSession = vi.fn();

      const agentSession: TerminalSession = {
        id: "session-agent",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-1",
        worktree: null,
        backendSessionId: "backend-agent-1",
        lifecycle: "exited",
        agentType: "claude",
        providerSession: { key: "session_id", id: "agent-123" },
      };

      const layout: LayoutState = {
        tabs: [{ id: "tab-agent", label: "Agent", sessionId: "session-agent" }],
        activeTabId: "tab-agent",
        layoutsByTabId: {
          "tab-agent": {
            root: { type: "leaf", leafId: "leaf-agent" },
            activeLeafId: "leaf-agent",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-agent": "session-agent" },
          },
        },
      };

      render(
        <TerminalSplitView
          layout={layout}
          sessions={{ "session-agent": agentSession }}
          onReconnectAgentSession={onReconnectAgentSession}
          onReconnectLocalSession={onReconnectLocalSession}
        />,
      );

      await act(async () => {
        await terminalPaneProps.bySessionId.get("session-agent").onReconnect("session-agent");
      });

      expect(onReconnectAgentSession).toHaveBeenCalledWith("session-agent");
      expect(onReconnectLocalSession).not.toHaveBeenCalled();
    });

    it("falls back to onOpenNewShell for local non-agent shell when onReconnectLocalSession is omitted", async () => {
      const onReconnectAgentSession = vi.fn();
      const onOpenNewShell = vi.fn();

      const localNonAgentSession: TerminalSession = {
        id: "session-local-fallback",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-1",
        worktree: null,
        backendSessionId: "backend-local-2",
        lifecycle: "exited",
      };

      const layout: LayoutState = {
        tabs: [{ id: "tab-fb", label: "Fallback", sessionId: "session-local-fallback" }],
        activeTabId: "tab-fb",
        layoutsByTabId: {
          "tab-fb": {
            root: { type: "leaf", leafId: "leaf-fb" },
            activeLeafId: "leaf-fb",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-fb": "session-local-fallback" },
          },
        },
      };

      render(
        <TerminalSplitView
          layout={layout}
          sessions={{ "session-local-fallback": localNonAgentSession }}
          onReconnectAgentSession={onReconnectAgentSession}
          onOpenNewShell={onOpenNewShell}
        />,
      );

      await act(async () => {
        await terminalPaneProps.bySessionId.get("session-local-fallback").onReconnect("session-local-fallback");
      });

      expect(onOpenNewShell).toHaveBeenCalledWith("session-local-fallback");
      expect(onReconnectAgentSession).not.toHaveBeenCalled();
    });

    it("wires onRefreshSessionIdentity through TerminalSplitView down to TerminalPane", () => {
      const onRefreshSessionIdentity = vi.fn();
      const session: TerminalSession = {
        id: "session-wire-test",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-1",
        worktree: null,
        backendSessionId: "backend-wire-1",
        lifecycle: "running",
      };
      const layout: LayoutState = {
        tabs: [{ id: "tab-wire", label: "Wire", sessionId: "session-wire-test" }],
        activeTabId: "tab-wire",
        layoutsByTabId: {
          "tab-wire": {
            root: { type: "leaf", leafId: "leaf-wire" },
            activeLeafId: "leaf-wire",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-wire": "session-wire-test" },
          },
        },
      };

      render(
        <TerminalSplitView
          layout={layout}
          sessions={{ "session-wire-test": session }}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
        />,
      );

      expect(terminalPaneProps.bySessionId.get("session-wire-test").onRefreshSessionIdentity).toBe(onRefreshSessionIdentity);
    });
  });
});
