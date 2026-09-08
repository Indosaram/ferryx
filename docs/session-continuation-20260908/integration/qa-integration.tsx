import React, { useState, useCallback, useMemo, useEffect, useLayoutEffect } from "react";
import ReactDOM from "react-dom/client";
import { Sidebar } from "@/components/Sidebar";
import { TerminalSplitView } from "@/components/TerminalSplitView";
import { AddProjectDialog } from "@/components/ProjectDialogs";
import { workspaceReducer, type WorkspaceState, type WorkspaceAction } from "@/state/workspaceStore";
import { emptySidebarWorkspaceIds } from "@/state/sidebarWorkspaceState";
import { createLayoutState } from "@/state/layout";
import type { RegisteredProject, Worktree, TerminalSession, LayoutState, TerminalTab } from "@/lib/types";
import "@/index.css";

// --- Mock Tauri Runtime ---
(window as any).isTauri = true;

const hosts = [
  { id: "windows", label: "Windows QA", hostname: "windows.example", source: "manual", authMethod: "agent" },
  { id: "linux", label: "Linux QA", hostname: "linux.example", source: "manual", authMethod: "agent" },
];

let callbackId = 0;
const ipcCalls: Array<{ cmd: string; args?: any }> = [];

Object.defineProperty(window, "__TAURI_INTERNALS__", {
  value: {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
    },
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (cmd: string, args?: any) => {
      ipcCalls.push({ cmd, args });

      // SSH Host and Directory mocks
      if (cmd === "cmd_ssh_list_hosts") return hosts;
      if (cmd === "cmd_ssh_list_directories") {
        const home = args?.request?.hostId === "windows" ? "C:\\Users\\developer" : "/home/developer";
        const separator = home.startsWith("C:") ? "\\" : "/";
        const rawPath = args?.request?.path ?? home;
        const normalized = rawPath.replace(/^~/, home).replace(/\\/g, "/").replace(/\/$/, "");
        const homeSlash = home.replace(/\\/g, "/");

        const names = normalized === homeSlash
          ? ["code", "Documents", "denied", "empty", "loading"]
          : normalized.endsWith("/code")
          ? ["ferryx", "frontend", "project-demo"]
          : [];

        const canonical = separator === "\\" ? normalized.replace(/\//g, "\\") : normalized;
        return {
          path: canonical,
          homePath: home,
          parentPath: home,
          truncated: false,
          entries: names.map((name) => ({
            name,
            path: `${canonical}${separator}${name}`,
            hidden: false,
          })),
        };
      }
      if (cmd === "cmd_project_register_remote") {
        return {
          hostId: args?.request?.hostId,
          workspaceId: "ssh:registered",
          repoRoot: args?.request?.repoPath,
          gitRoot: null,
        };
      }

      // Native Terminal mocks
      if (cmd === "cmd_native_terminal_attach") return undefined;
      if (cmd === "cmd_native_terminal_set_bounds") {
        return {
          presented: true,
          renderDeferred: false,
          cursorCol: 0,
          cursorRow: 0,
          cellWidthPx: 9,
          cellHeightPx: 18,
          presentationToken: "qa-tok",
          lastFrameToken: "qa-tok",
        };
      }
      if (cmd === "cmd_native_terminal_mouse") return { mouseTrackingEnabled: false };
      if (cmd === "cmd_native_terminal_send_input" || cmd === "cmd_native_terminal_set_focus") {
        return {
          presented: true,
          renderDeferred: false,
          cursorCol: 0,
          cursorRow: 0,
          cellWidthPx: 9,
          cellHeightPx: 18,
        };
      }
      if (cmd.startsWith("plugin:event|")) return callbackId;

      return null;
    },
  },
  configurable: true,
});

const initialProjects: RegisteredProject[] = [
  {
    workspaceId: "orca-local",
    repoRoot: "/repos/orca-local",
    gitRoot: "/repos/orca-local",
    gitRemote: "https://github.com/project/orca-local.git",
  },
  {
    workspaceId: "orca-parked",
    repoRoot: "/repos/orca-parked",
    gitRoot: "/repos/orca-parked",
    gitRemote: "https://github.com/project/orca-parked.git",
  },
];

const initialWorktrees: Record<string, Worktree[]> = {
  "orca-local": [
    {
      path: "/repos/orca-local",
      head: "111111",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
    {
      path: "/repos/orca-local/wt-feature",
      head: "222222",
      branch: "refs/heads/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
  ],
};

function createInitialWorkspace(): WorkspaceState {
  const initialTabs: TerminalTab[] = [
    { id: "term-1", kind: "terminal", label: "main", sessionId: "session-1" },
  ];
  const layout = createLayoutState(initialTabs);
  const sessions: Record<string, TerminalSession> = {
    "session-1": {
      id: "session-1",
      backendSessionId: "backend-1",
      cwd: "/repos/orca-local",
      worktreePath: "/repos/orca-local",
      cols: 80,
      rows: 24,
      running: true,
    },
  };
  return {
    workspaceId: "orca-local",
    worktrees: initialWorktrees["orca-local"],
    activeWorktreePath: "/repos/orca-local",
    sessions,
    layout,
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

function IntegrationApp() {
  const [projects, setProjects] = useState<RegisteredProject[]>(initialProjects);
  const [activeProjectId, setActiveProjectId] = useState<string>("orca-local");
  const [workspaceState, setWorkspaceState] = useState<WorkspaceState>(createInitialWorkspace);
  const [addProjectOpen, setAddProjectOpen] = useState(false);

  const dispatch = useCallback((action: WorkspaceAction) => {
    setWorkspaceState((cur) => workspaceReducer(cur, action));
  }, []);

  const emptyWorkspaceIds = useMemo(
    () => emptySidebarWorkspaceIds(projects, activeProjectId, workspaceState, []),
    [projects, activeProjectId, workspaceState],
  );

  useLayoutEffect(() => {
    window.dispatchEvent(new CustomEvent("qa:updated"));
  });

  useEffect(() => {
    const waitForUpdate = (fn: () => void): Promise<boolean> => {
      return new Promise((resolve) => {
        window.addEventListener("qa:updated", () => resolve(true), { once: true });
        fn();
      });
    };

    (window as any).__QA__ = {
      closeTab: (tabId: string) => waitForUpdate(() => dispatch({ type: "CLOSE_TAB", tabId })),
      reopenTab: () => {
        return waitForUpdate(() => {
          const session: TerminalSession = {
            id: "session-2",
            backendSessionId: "backend-2",
            cwd: "/repos/orca-local",
            worktreePath: "/repos/orca-local",
            cols: 80,
            rows: 24,
            running: true,
          };
          dispatch({
            type: "ADD_TAB_WITH_SESSION",
            tab: { id: "term-2", kind: "terminal", label: "main", sessionId: "session-2" },
            session,
            targetWorktreePath: "/repos/orca-local",
          });
        });
      },
      openAddProject: () => waitForUpdate(() => setAddProjectOpen(true)),
      closeAddProject: () => waitForUpdate(() => setAddProjectOpen(false)),
      getState: () => {
        const localChevron = document.querySelector('button[aria-label*="orca-local"][aria-expanded]');
        const worktreeList = document.querySelector('[aria-label="orca-local worktrees"]');
        const leaf = document.querySelector('[data-testid="pane-leaf"]');
        const terminalPane = document.querySelector('[data-testid="native-terminal-pane"]');
        const toolbar = document.querySelector('[data-testid="pane-toolbar"]');
        const handleBacking = document.querySelector('[data-testid="terminal-pane-handle-backing"]');
        const bottomBacking = document.querySelector('[data-testid="terminal-pane-bottom-backing"]');

        const leafRect = leaf?.getBoundingClientRect();
        const termRect = terminalPane?.getBoundingClientRect();

        return {
          leafFound: leaf !== null,
          termFound: terminalPane !== null,
          activeProjectId,
          tabCount: workspaceState.layout.tabs.length,
          emptyWorkspaceIds,
          orcaLocalExpanded: localChevron?.getAttribute("aria-expanded") === "true",
          hasWorktreeList: worktreeList !== null,
          terminalHeight: termRect?.height ?? 0,
          leafHeight: leafRect?.height ?? 0,
          hasHandleBacking: handleBacking !== null,
          hasBottomBacking: bottomBacking !== null,
          toolbarHidden: toolbar?.classList.contains("opacity-0") ?? true,
        };
      },
    };
    (window as any).__QA_READY__ = true;
    window.dispatchEvent(new CustomEvent("qa:ready"));
  }, [activeProjectId, dispatch, emptyWorkspaceIds, projects, workspaceState]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground font-sans">
      <Sidebar
        open={true}
        projects={projects}
        activeProjectId={activeProjectId}
        worktrees={workspaceState.worktrees}
        inactiveProjectWorktrees={{}}
        emptyWorkspaceIds={emptyWorkspaceIds}
        agents={[]}
        activePath={workspaceState.activeWorktreePath ?? "/repos/orca-local"}
        onSelectProject={(p) => setActiveProjectId(p.workspaceId)}
        onSelectWorktree={() => undefined}
        onCreateWorktree={() => undefined}
        onAddProject={() => setAddProjectOpen(true)}
      />

      <main className="flex min-w-0 flex-1 flex-col overflow-hidden h-full">
        <header className="flex h-9 shrink-0 items-center justify-between border-b border-border/40 px-3 text-xs bg-muted/20">
          <span className="font-semibold text-muted-foreground">ORCA-LITE INTEGRATED SURFACE QA</span>
          <span className="font-mono text-[11px] text-muted-foreground">
            Tabs: {workspaceState.layout.tabs.length} | Empty: {JSON.stringify(emptyWorkspaceIds)}
          </span>
        </header>

        <div className="flex flex-1 flex-col min-h-0 min-w-0 relative h-full">
          <TerminalSplitView
            layout={workspaceState.layout}
            sessions={workspaceState.sessions}
            activeTabId={workspaceState.layout.tabs[0]?.id ?? null}
            activeLeafId={workspaceState.layout.activeLeafId}
            onSelectTab={() => undefined}
            onCloseTab={(tabId) => dispatch({ type: "CLOSE_TAB", tabId })}
            onSplit={() => undefined}
            onUnsplit={() => undefined}
            onFocusLeaf={() => undefined}
          />
        </div>
      </main>

      {addProjectOpen && (
        <AddProjectDialog
          initialHostId="windows"
          projects={projects}
          onClose={() => setAddProjectOpen(false)}
          onRegistered={(project) => {
            setProjects((cur) => [...cur, project]);
            setAddProjectOpen(false);
          }}
        />
      )}
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(<IntegrationApp />);
}
