import { FolderPlus, GitBranch, Plus, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { worktreeErrorMessage } from "./lib/ipcErrors";
import { createWorktree, toIpcError } from "./lib/tauri";
import type { Worktree } from "./lib/types";
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
    activeWorktreeId: state.activeWorktreeId,
    syncWorktrees,
    ensureTabForWorktree,
  });
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [newSlug, setNewSlug] = useState("");
  const [newBaseRef, setNewBaseRef] = useState("HEAD");
  const [createError, setCreateError] = useState<string | null>(null);

  const activeWorktree = useMemo(
    () => state.worktrees.find((worktree) => worktree.worktreeId === state.activeWorktreeId) ?? null,
    [state.activeWorktreeId, state.worktrees],
  );
  const activeAgent = agents.find((agent) => agent.worktreeId === activeWorktree?.worktreeId);

  const handleSelectWorktree = useCallback(
    (worktree: Worktree) => {
      void ensureTabForWorktree(worktree).catch(reportRuntimeError);
    },
    [ensureTabForWorktree, reportRuntimeError],
  );

  const handleCreateWorktreeSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const slug = newSlug.trim();
    if (!slug || !activeWorktree) return;

    try {
      setCreateError(null);
      await createWorktree({ wsId: activeWorktree.wsId, slug, baseRef: newBaseRef || null });
      setIsCreateOpen(false);
      setNewSlug("");
      await refreshWorktrees();
    } catch (error) {
      setCreateError(worktreeErrorMessage(toIpcError(error)));
    }
  };

  const handleAddTerminalTab = () => {
    if (!activeWorktree) return;
    void openTab(activeWorktree).catch(reportRuntimeError);
  };

  const handleCloseTab = (tabId: string) => {
    void closeTab(tabId).catch(reportRuntimeError);
  };

  const handleSplit = () => {
    if (state.layout.split === "none") {
      void enableSplit("horizontal").catch(reportRuntimeError);
    } else if (state.layout.split === "horizontal") {
      rotateSplit();
    } else {
      disableSplit();
    }
  };

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
              onActivate={activatePrimary}
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