import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { TerminalSplitView } from "./src/components/TerminalSplitView";
import type { LayoutState, TerminalSession, TerminalTab } from "./src/lib/types";
import type { TerminalActivity } from "./src/lib/activity";
import type { DagRunSnapshot } from "./src/lib/dagTypes";
import { dagStore } from "./src/state/dagStore";
import "./src/index.css";
import "./src/settings-runtime.css";

// --- Mock Tauri Bridge ---
(window as any).isTauri = true;

interface TauriCall {
  command: string;
  args?: any;
  timestamp: number;
}
const ipcCalls: TauriCall[] = [];
const ipcListeners: Array<(call: TauriCall) => void> = [];
let callbackId = 0;
let failNextBounds = false;

Object.defineProperty(window, "__TAURI_INTERNALS__", {
  value: {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (command: string, args?: any) => {
      const call = { command, args, timestamp: Date.now() };
      ipcCalls.push(call);
      ipcListeners.forEach((listener) => {
        try { listener(call); } catch {}
      });

      if (command.startsWith("plugin:event|")) {
        return callbackId;
      }
      if (command === "cmd_native_terminal_set_bounds") {
        if (failNextBounds) {
          throw { code: "INTERNAL_ERROR", message: "Native surface unavailable" };
        }
        return {
          presented: true,
          renderDeferred: false,
          cursorCol: 0,
          cursorRow: 0,
          cellWidthPx: 9,
          cellHeightPx: 18,
          presentationToken: "qa-token-1",
          lastFrameToken: "qa-token-1",
        };
      }
      if (command === "cmd_native_terminal_attach") {
        return undefined;
      }
      if (command === "cmd_native_terminal_mouse") {
        return { mouseTrackingEnabled: false };
      }
      if (command === "cmd_native_terminal_send_input" || command === "cmd_native_terminal_set_focus") {
        return {
          presented: true,
          renderDeferred: false,
          cursorCol: 0,
          cursorRow: 0,
          cellWidthPx: 9,
          cellHeightPx: 18,
        };
      }
      return null;
    },
  },
});

const initialSessions: Record<string, TerminalSession> = {
  "session-qa-1": {
    id: "session-qa-1",
    cwd: "/repo/main",
    worktreePath: "/repo/main",
    workspaceId: "ws-qa",
    worktree: { wsId: "ws-qa", slug: "main" },
    backendSessionId: "backend-session-qa-1",
    providerSession: { id: "provider-dag-owner", key: "session_id" },
    lifecycle: "working",
  },
};

const initialTabs: TerminalTab[] = [
  { id: "tab-qa-1", label: "main", sessionId: "session-qa-1" },
];

const initialLayout: LayoutState = {
  tabs: initialTabs,
  activeTabId: "tab-qa-1",
  tabGroups: {
    "group-qa": { id: "group-qa", tabIds: ["tab-qa-1"], activeTabId: "tab-qa-1" },
  },
  tabGroupLayout: { type: "group", groupId: "group-qa" },
  focusedGroupId: "group-qa",
  layoutsByTabId: {
    "tab-qa-1": {
      root: { type: "leaf", leafId: "leaf-qa-1" },
      activeLeafId: "leaf-qa-1",
      expandedLeafId: null,
      sessionIdsByLeafId: { "leaf-qa-1": "session-qa-1" },
    },
  },
};

function QaTerminalHarness() {
  const [layout, setLayout] = useState<LayoutState>(initialLayout);
  const [sessions] = useState<Record<string, TerminalSession>>(initialSessions);
  const [activities, setActivities] = useState<Record<string, TerminalActivity>>({});
  const [containerPadding, setContainerPadding] = useState(0);

  const toggleAttention = () => {
    setActivities((prev) => {
      const current = prev["session-qa-1"];
      if (current && !current.seen && current.state === "waiting") {
        return {};
      }
      return {
        "session-qa-1": {
          state: "waiting",
          seen: false,
          title: "Process completed with exit code 0",
        },
      };
    });
  };

  const toggleDagBadge = () => {
    const active = dagStore.activeRunIds("/repo/main");
    if (active.length > 0) {
      dagStore.reset();
    } else {
      const sampleSnapshot: DagRunSnapshot = {
        version: 1,
        runId: "run-qa-1",
        repoPath: "/repo/main",
        status: "running",
        tasks: [],
        startedAt: Date.now(),
        finishedAt: null,
        rootSessionId: "provider-dag-owner",
      };
      dagStore.applySnapshot("/repo/main", sampleSnapshot);
    }
  };

  const triggerBoundsFailure = () => {
    failNextBounds = true;
    setContainerPadding((p) => (p === 0 ? 1 : 0));
  };

  const recoverBounds = () => {
    failNextBounds = false;
    setContainerPadding((p) => (p === 0 ? 1 : 0));
  };

  const handleSplitPane = (tabId: string, _leafId: string, direction: "horizontal" | "vertical") => {
    const newLeafId = `leaf-split-${Date.now()}`;
    const newSessionId = `session-split-${Date.now()}`;
    sessions[newSessionId] = {
      id: newSessionId,
      cwd: "/repo/main",
      worktreePath: "/repo/main",
      workspaceId: "ws-qa",
      worktree: { wsId: "ws-qa", slug: "main" },
      backendSessionId: `backend-${newSessionId}`,
      lifecycle: "working",
    };
    setLayout((prev) => {
      const tabLayout = prev.layoutsByTabId[tabId];
      if (!tabLayout) return prev;
      return {
        ...prev,
        layoutsByTabId: {
          ...prev.layoutsByTabId,
          [tabId]: {
            ...tabLayout,
            root: {
              type: "split",
              direction,
              ratio: 0.5,
              first: tabLayout.root,
              second: { type: "leaf", leafId: newLeafId },
            },
            sessionIdsByLeafId: {
              ...tabLayout.sessionIdsByLeafId,
              [newLeafId]: newSessionId,
            },
          },
        },
      };
    });
  };

  React.useEffect(() => {
    (window as any).__QA_READY__ = true;
    window.dispatchEvent(new CustomEvent("qa:ready"));
  }, []);

  // Expose QA state and controls on window for automation
  (window as any).__QA__ = {
    ipcCalls,
    getIpcCalls: () => [...ipcCalls],
    clearIpcCalls: () => { ipcCalls.length = 0; },
    onIpc: (listener: (call: TauriCall) => void) => {
      ipcListeners.push(listener);
      return () => {
        const idx = ipcListeners.indexOf(listener);
        if (idx >= 0) ipcListeners.splice(idx, 1);
      };
    },
    waitForIpc: (command: string, timeoutMs = 3000) => {
      const existing = ipcCalls.find((c) => c.command === command);
      if (existing) return Promise.resolve(existing);
      return new Promise<TauriCall>((resolve, reject) => {
        const timer = setTimeout(() => {
          unsub();
          reject(new Error(`Timeout waiting for IPC ${command}`));
        }, timeoutMs);
        const unsub = (window as any).__QA__.onIpc((call: TauriCall) => {
          if (call.command === command) {
            clearTimeout(timer);
            unsub();
            resolve(call);
          }
        });
      });
    },
    toggleAttention,
    toggleDagBadge,
    triggerBoundsFailure,
    recoverBounds,
  };

  return (
    <div className="flex h-screen w-screen flex-col bg-background text-foreground select-none">
      {/* QA Header & Controls */}
      <header className="flex h-10 items-center justify-between border-b border-border/40 bg-muted/40 px-3 text-xs">
        <div className="flex items-center gap-2 font-mono">
          <span className="font-semibold text-primary">Terminal QA</span>
          <span className="text-muted-foreground">| Port 5212</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            id="qa-btn-attention"
            onClick={toggleAttention}
            className="rounded bg-secondary px-2 py-1 text-xs hover:bg-secondary/80"
          >
            Toggle Attention
          </button>
          <button
            id="qa-btn-dag"
            onClick={toggleDagBadge}
            className="rounded bg-secondary px-2 py-1 text-xs hover:bg-secondary/80"
          >
            Toggle DAG Badge
          </button>
          <button
            id="qa-btn-bounds-err"
            onClick={triggerBoundsFailure}
            className="rounded bg-destructive/20 px-2 py-1 text-xs text-destructive hover:bg-destructive/30"
          >
            Fail Bounds
          </button>
          <button
            id="qa-btn-bounds-recover"
            onClick={recoverBounds}
            className="rounded bg-secondary px-2 py-1 text-xs hover:bg-secondary/80"
          >
            Clear Bounds Error
          </button>
        </div>
      </header>

      {/* Main Terminal Viewport Container */}
      <main
        id="qa-terminal-container"
        style={{ paddingRight: containerPadding }}
        className="relative flex flex-1 flex-col min-h-0 min-w-0 bg-background"
      >
        <TerminalSplitView
          layout={layout}
          sessions={sessions}
          activityBySessionId={activities}
          onActivateTab={(tabId) => setLayout((prev) => ({ ...prev, activeTabId: tabId }))}
          onSplitPane={handleSplitPane}
          onClosePane={(_tabId, _leafId) => undefined}
        />
      </main>
    </div>
  );
}

const rootElement = document.getElementById("root");
if (rootElement) {
  createRoot(rootElement).render(<QaTerminalHarness />);
}
