import { FolderPlus, GitBranch, Plus, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { worktreeErrorMessage } from "./lib/ipcErrors";
import { useShortcuts } from "./lib/shortcuts";
import { createWorktree, DEFAULT_WORKSPACE_ID, toIpcError } from "./lib/tauri";
import { worktreeIdentity, type Worktree } from "./lib/types";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { useWorkspaceStore } from "./state/workspaceStore";

export function App() {
  const {
    state,
    agents,
    openTab,
    ensureTabForWorktree,
    closeTab,
    enableSplit,
    rotateSplit,
    disableSplit,
    activatePrimary,
    syncWorktrees,
  } = useWorkspaceStore();
  const { runtimeError, refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({
    activeWorktreePath: state.activeWorktreePath,
    syncWorktrees,
    ensureTabForWorktree,
  });
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [newSlug, setNewSlug] = useState("");
  const [newBaseRef, setNewBaseRef] = useState("HEAD");
  const [createError, setCreateError] = useState<string | null>(null);

  const activeWorktree = useMemo(
    () => state.worktrees.find((worktree) => worktree.path === state.activeWorktreePath) ?? null,
    [state.activeWorktreePath, state.worktrees],
  );
  const activeAgent = agents.find((agent) => agent.worktreePath === activeWorktree?.path);

  const handleSelectWorktree = useCallback(
    (worktree: Worktree) => {
      void ensureTabForWorktree(worktree).catch(reportRuntimeError);
    },
    [ensureTabForWorktree, reportRuntimeError],
  );

  const handleSelectTerminalTab = useCallback(
    (tabId: string) => {
      const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
      const session = tab ? state.sessions[tab.sessionId] : undefined;
      const worktree = session ? state.worktrees.find((candidate) => candidate.path === session.cwd) : undefined;
      if (!worktree || worktree.path === state.activeWorktreePath) {
        activatePrimary(tabId);
        return;
      }
      void ensureTabForWorktree(worktree)
        .then(() => activatePrimary(tabId))
        .catch(reportRuntimeError);
    },
    [activatePrimary, ensureTabForWorktree, reportRuntimeError, state.activeWorktreePath, state.layout.tabs, state.sessions, state.worktrees],
  );

  const handleCreateWorktreeSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const slug = newSlug.trim();
    if (!slug || !activeWorktree) return;

    try {
      setCreateError(null);
      await createWorktree({
        workspaceId: DEFAULT_WORKSPACE_ID,
        worktree: { wsId: worktreeIdentity(activeWorktree)?.wsId ?? DEFAULT_WORKSPACE_ID, slug },
        baseRef: newBaseRef || null,
      });
      setIsCreateOpen(false);
      setNewSlug("");
      await refreshWorktrees();
    } catch (error) {
      setCreateError(worktreeErrorMessage(toIpcError(error)));
    }
  };

  const handleAddTerminalTab = useCallback(() => {
    if (!activeWorktree) return;
    void openTab(activeWorktree).catch(reportRuntimeError);
  }, [activeWorktree, openTab, reportRuntimeError]);

  const handleCloseTab = useCallback(
    (tabId: string) => {
      void closeTab(tabId).catch(reportRuntimeError);
    },
    [closeTab, reportRuntimeError],
  );

  const handleCycleTab = useCallback(
    (offset: number) => {
      const tabs = state.layout.tabs;
      if (tabs.length < 2) return;
      const currentIndex = Math.max(0, tabs.findIndex((tab) => tab.id === state.layout.primaryTabId));
      const nextIndex = (currentIndex + offset + tabs.length) % tabs.length;
      handleSelectTerminalTab(tabs[nextIndex].id);
    },
    [handleSelectTerminalTab, state.layout.primaryTabId, state.layout.tabs],
  );

  const handleSplit = () => {
    if (state.layout.split === "none") {
      void enableSplit("horizontal").catch(reportRuntimeError);
    } else if (state.layout.split === "horizontal") {
      rotateSplit();
    } else {
      disableSplit();
    }
  };

  const shortcutHandlers = useMemo(
    () => ({
      "tab.newTerminal": handleAddTerminalTab,
      "tab.close": () => {
        if (state.layout.primaryTabId) handleCloseTab(state.layout.primaryTabId);
      },
      "tab.next": () => handleCycleTab(1),
      "tab.previous": () => handleCycleTab(-1),
      "terminal.splitRight": () => void enableSplit("horizontal").catch(reportRuntimeError),
      "terminal.splitDown": () => void enableSplit("vertical").catch(reportRuntimeError),
      "terminal.unsplit": disableSplit,
      "commandPalette.open": () => setIsCommandPaletteOpen(true),
    }),
    [
      disableSplit,
      enableSplit,
      handleAddTerminalTab,
      handleCloseTab,
      handleCycleTab,
      reportRuntimeError,
      state.layout.primaryTabId,
    ],
  );
  useShortcuts(shortcutHandlers);

  return (
    <div className="flex h-screen w-screen select-none overflow-hidden bg-background font-sans text-foreground">
      <Sidebar
        worktrees={state.worktrees}
        agents={agents}
        activePath={activeWorktree?.path || ""}
        onSelectWorktree={handleSelectWorktree}
        onCreateWorktree={() => {
          setCreateError(null);
          setIsCreateOpen(true);
        }}
        onOpenCommandPalette={() => setIsCommandPaletteOpen(true)}
      />

      <main className="flex h-full flex-1 flex-col overflow-hidden bg-card">
        {activeWorktree ? (
          <>
            <WorkspaceHeader
              worktree={activeWorktree}
              agent={activeAgent}
              onSplit={handleSplit}
              splitState={state.layout.split}
            />
            <TabBar
              tabs={state.layout.tabs}
              activeTabId={state.layout.primaryTabId ?? ""}
              onActivate={handleSelectTerminalTab}
              onClose={handleCloseTab}
              onAdd={handleAddTerminalTab}
            />
            <TerminalSplitView layout={state.layout} sessions={state.sessions} />
          </>
        ) : (
          <div className="flex h-full flex-1 items-center justify-center text-xs text-muted-foreground">
            {runtimeError ? `Workspace unavailable (${runtimeError.code})` : "No workspace available"}
          </div>
        )}
      </main>

      <CommandPalette
        open={isCommandPaletteOpen}
        worktrees={state.worktrees}
        tabs={state.layout.tabs}
        onSelectWorktree={handleSelectWorktree}
        onSelectTab={handleSelectTerminalTab}
        onClose={() => setIsCommandPaletteOpen(false)}
      />

      {runtimeError && activeWorktree ? (
        <div className="pointer-events-none fixed bottom-3 right-3 z-40 max-w-error rounded-md border border-destructive/30 bg-card/95 px-3 py-2 text-[11px] text-destructive shadow-lg">
          {runtimeError.code}: {runtimeError.message}
        </div>
      ) : null}

      {isCreateOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
          <form
            onSubmit={handleCreateWorktreeSubmit}
            className="w-96 animate-enter space-y-4 rounded-xl border border-border bg-card p-5 shadow-2xl"
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 text-sm font-semibold">
                <FolderPlus className="size-4 text-primary" />
                <span>Create New Worktree</span>
              </div>
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="text-muted-foreground hover:text-foreground"
              >
                <X className="size-4" />
              </button>
            </div>

            <div className="space-y-3 text-xs">
              <div>
                <label className="mb-1 block text-muted-foreground">Branch / Worktree Slug</label>
                <input
                  type="text"
                  autoFocus
                  placeholder="e.g. feature-auth"
                  value={newSlug}
                  onChange={(event) => setNewSlug(event.target.value)}
                  className="w-full rounded-md border border-input bg-background px-3 py-1.5 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
                />
              </div>

              <div>
                <label className="mb-1 block text-muted-foreground">Base Ref / Branch</label>
                <div className="flex items-center gap-2 rounded-md border border-input bg-background px-3 py-1.5 text-xs text-foreground">
                  <GitBranch className="size-3.5 text-muted-foreground" />
                  <input
                    type="text"
                    value={newBaseRef}
                    onChange={(event) => setNewBaseRef(event.target.value)}
                    className="flex-1 bg-transparent focus:outline-none"
                  />
                </div>
              </div>
              {createError ? <p className="rounded-md bg-destructive/10 px-2 py-1.5 text-destructive">{createError}</p> : null}
            </div>

            <div className="flex justify-end gap-2 pt-2">
              <button
                type="button"
                onClick={() => setIsCreateOpen(false)}
                className="rounded-md px-3 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="flex items-center gap-1.5 rounded-md bg-primary px-3.5 py-1.5 text-xs font-medium text-primary-foreground transition-colors hover:bg-primary/90"
              >
                <Plus className="size-3.5" />
                <span>Create Worktree</span>
              </button>
            </div>
          </form>
        </div>
      ) : null}
    </div>
  );
}

export default App;
