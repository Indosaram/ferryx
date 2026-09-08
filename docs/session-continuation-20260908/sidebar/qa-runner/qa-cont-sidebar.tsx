import React, { useState, useCallback, useMemo, useEffect, useLayoutEffect } from "react";
import ReactDOM from "react-dom/client";
import { Sidebar } from "./src/components/Sidebar";
import { workspaceReducer, type WorkspaceState, type WorkspaceAction } from "./src/state/workspaceStore";
import { emptySidebarWorkspaceIds } from "./src/state/sidebarWorkspaceState";
import { createLayoutState } from "./src/state/layout";
import type { RegisteredProject, Worktree, TerminalSession } from "./src/lib/types";
import "./src/index.css";

// Install mock Tauri runtime so Sidebar and UI components do not throw
if (typeof window !== "undefined" && !(window as any).__TAURI_INTERNALS__) {
  let callbackId = 0;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: () => ++callbackId,
      unregisterCallback: () => undefined,
      invoke: async (command: string, args?: any) => {
        if (command === "cmd_ssh_list_hosts") {
          return [];
        }
        return null;
      },
    },
    configurable: true,
  });
}

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
      branch: "refs/heads/orca/orca-local/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
  ],
  "orca-parked": [
    {
      path: "/repos/orca-parked",
      head: "333333",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
  ],
};

function createInitialWorkspace(
  workspaceId: string,
  initialTabs: Array<{ id: string; kind: "terminal" | "browser"; label: string; url?: string }>,
): WorkspaceState {
  const layout = createLayoutState(
    initialTabs.map((t) =>
      t.kind === "browser"
        ? {
            id: t.id,
            kind: "browser" as const,
            label: t.label,
            url: t.url ?? "about:blank",
            browserId: t.id,
          }
        : {
            id: t.id,
            kind: "terminal" as const,
            label: t.label,
            sessionId: t.id,
          },
    ),
  );

  const sessions: Record<string, TerminalSession> = {};
  for (const t of initialTabs) {
    if (t.kind === "terminal") {
      sessions[t.id] = {
        id: t.id,
        backendSessionId: t.id,
        cwd: `/repos/${workspaceId}`,
        worktreePath: `/repos/${workspaceId}`,
        cols: 80,
        rows: 24,
        running: true,
      };
    }
  }

  return {
    workspaceId,
    worktrees: initialWorktrees[workspaceId] ?? [],
    activeWorktreePath: `/repos/${workspaceId}`,
    sessions,
    layout,
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

function QaApp() {
  const [projects] = useState<RegisteredProject[]>(initialProjects);
  const [activeProjectId, setActiveProjectId] = useState<string>("orca-local");

  // Live active workspace state, managed strictly by actual workspaceReducer
  const [liveState, setLiveState] = useState<WorkspaceState>(() =>
    createInitialWorkspace("orca-local", [
      { id: "term-1", kind: "terminal", label: "Terminal 1" },
      { id: "browser-1", kind: "browser", label: "Browser 1", url: "https://example.com" },
    ]),
  );

  // Parked workspace snapshots, e.g. "orca-parked"
  const [parkedSnapshots, setParkedSnapshots] = useState<Array<[string, WorkspaceState]>>(() => [
    [
      "orca-parked",
      createInitialWorkspace("orca-parked", [
        { id: "parked-tab-1", kind: "browser", label: "Parked Browser" },
      ]),
    ],
  ]);

  const dispatchLive = useCallback((action: WorkspaceAction) => {
    setLiveState((current) => workspaceReducer(current, action));
  }, []);

  const emptyWorkspaceIds = useMemo(
    () => emptySidebarWorkspaceIds(projects, activeProjectId, liveState, parkedSnapshots),
    [projects, activeProjectId, liveState, parkedSnapshots],
  );

  const inactiveProjectWorktrees = useMemo(() => {
    const result: Record<string, Worktree[]> = {};
    for (const [id, ws] of parkedSnapshots) {
      result[id] = ws.worktrees;
    }
    return result;
  }, [parkedSnapshots]);

  // Signal commit after each layout effect
  useLayoutEffect(() => {
    window.dispatchEvent(new CustomEvent("qa:updated"));
  });

  // Expose QA harness interface
  useEffect(() => {
    const waitForUpdate = (fn: () => void): Promise<boolean> => {
      return new Promise((resolve) => {
        window.addEventListener("qa:updated", () => resolve(true), { once: true });
        fn();
      });
    };

    (window as any).__QA__ = {
      closeTab: (tabId: string) => {
        return waitForUpdate(() => dispatchLive({ type: "CLOSE_TAB", tabId }));
      },
      addTerminalTab: ({ id, label, path }: { id: string; label: string; path?: string }) => {
        return waitForUpdate(() => {
          const session: TerminalSession = {
            id,
            backendSessionId: id,
            cwd: path ?? "/repos/orca-local",
            worktreePath: path ?? "/repos/orca-local",
            cols: 80,
            rows: 24,
            running: true,
          };
          dispatchLive({
            type: "ADD_TAB_WITH_SESSION",
            tab: { id, kind: "terminal", label, sessionId: id },
            session,
            targetWorktreePath: path ?? "/repos/orca-local",
          });
        });
      },
      addBrowserTab: ({ id, label, url }: { id: string; label: string; url?: string }) => {
        return waitForUpdate(() => {
          dispatchLive({
            type: "ADD_TAB_WITH_SESSION",
            tab: { id, kind: "browser", label, url: url ?? "about:blank", browserId: id },
          });
        });
      },
      closeParkedTab: (workspaceId: string) => {
        return waitForUpdate(() => {
          setParkedSnapshots((current) =>
            current.map(([id, ws]) => {
              if (id === workspaceId) {
                const firstTabId = ws.layout.tabs[0]?.id;
                if (!firstTabId) return [id, ws];
                return [id, workspaceReducer(ws, { type: "CLOSE_TAB", tabId: firstTabId })];
              }
              return [id, ws];
            }),
          );
        });
      },
      addParkedTab: (workspaceId: string) => {
        return waitForUpdate(() => {
          setParkedSnapshots((current) =>
            current.map(([id, ws]) => {
              if (id === workspaceId) {
                const newTabId = `parked-tab-${Date.now()}`;
                return [
                  id,
                  workspaceReducer(ws, {
                    type: "ADD_TAB_WITH_SESSION",
                    tab: { id: newTabId, kind: "browser", label: "Restored Parked Tab", url: "about:blank", browserId: newTabId },
                  }),
                ];
              }
              return [id, ws];
            }),
          );
        });
      },
      clickChevron: (workspaceId: string) => {
        const btn = document.querySelector(`button[aria-label*="${workspaceId}"][aria-expanded]`) as HTMLButtonElement | null;
        if (btn) btn.click();
        return new Promise<boolean>((resolve) => {
          requestAnimationFrame(() => resolve(Boolean(btn)));
        });
      },
      clickTitle: (workspaceId: string) => {
        const btn = document.querySelector(`button[aria-label="${workspaceId}"]`) as HTMLButtonElement | null;
        if (btn) btn.click();
        return new Promise<boolean>((resolve) => {
          requestAnimationFrame(() => resolve(Boolean(btn)));
        });
      },
      getState: () => {
        const localChevron = document.querySelector(`button[aria-label*="orca-local"][aria-expanded]`);
        const parkedChevron = document.querySelector(`button[aria-label*="orca-parked"][aria-expanded]`);
        const localList = document.querySelector(`[aria-label="orca-local worktrees"]`);
        const parkedList = document.querySelector(`[aria-label="orca-parked worktrees"]`);

        return {
          activeProjectId,
          liveTabs: liveState.layout.tabs.map((t) => ({ id: t.id, kind: t.kind, label: t.label })),
          liveTabCount: liveState.layout.tabs.length,
          emptyWorkspaceIds,
          orcaLocalExpanded: localChevron?.getAttribute("aria-expanded") === "true",
          orcaLocalChevronLabel: localChevron?.getAttribute("aria-label") ?? null,
          orcaLocalHasWorktreeList: localList !== null,
          orcaParkedExpanded: parkedChevron?.getAttribute("aria-expanded") === "true",
          orcaParkedChevronLabel: parkedChevron?.getAttribute("aria-label") ?? null,
          orcaParkedHasWorktreeList: parkedList !== null,
        };
      },
    };

    (window as any).__QA_READY__ = true;
    window.dispatchEvent(new CustomEvent("qa:ready"));
  }, [activeProjectId, dispatchLive, emptyWorkspaceIds, liveState, parkedSnapshots]);

  return (
    <div className="flex h-screen w-screen bg-[#181818] text-[#cccccc]">
      <Sidebar
        open={true}
        projects={projects}
        activeProjectId={activeProjectId}
        worktrees={liveState.worktrees}
        inactiveProjectWorktrees={inactiveProjectWorktrees}
        emptyWorkspaceIds={emptyWorkspaceIds}
        agents={[]}
        activePath={liveState.activeWorktreePath ?? "/repos/orca-local"}
        onSelectProject={(p) => setActiveProjectId(p.workspaceId)}
        onSelectWorktree={() => undefined}
        onCreateWorktree={() => undefined}
      />
      <div className="flex flex-1 flex-col p-4">
        <h1 className="text-sm font-semibold text-[#888888] mb-2 uppercase tracking-wide">
          QA Harness: Real Sidebar + workspaceReducer Live/Parked Verification
        </h1>
        <div className="bg-[#242424] rounded border border-[#333333] p-3 text-xs font-mono space-y-1">
          <div>Active Project: {activeProjectId}</div>
          <div>Live Tabs ({liveState.layout.tabs.length}): {liveState.layout.tabs.map((t) => `${t.id} (${t.kind})`).join(", ") || "none"}</div>
          <div>Empty Workspace IDs: {JSON.stringify(emptyWorkspaceIds)}</div>
        </div>
      </div>
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(<QaApp />);
}
