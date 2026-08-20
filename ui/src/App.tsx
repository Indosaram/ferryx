import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { TerminalPane } from "./components/TerminalPane";
import type { ActiveAgent, TerminalTab, Worktree } from "./lib/types";
import { Terminal as TerminalIcon } from "lucide-react";

export function App() {
  const [worktrees, setWorktrees] = useState<Worktree[]>([]);
  const [activeWorktree, setActiveWorktree] = useState<Worktree | null>(null);
  const [agents, setAgents] = useState<ActiveAgent[]>([]);
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string>("");

  const fetchWorktrees = async () => {
    try {
      const list = await invoke<Worktree[]>("cmd_worktree_list", {});
      setWorktrees(list);

      if (list.length > 0) {
        if (!activeWorktree || !list.some((w) => w.path === activeWorktree.path)) {
          const first = list[0];
          setActiveWorktree(first);
          ensureTabForWorktree(first);
        }
      }
    } catch (err) {
      console.error("Failed to list worktrees:", err);
    }
  };

  useEffect(() => {
    fetchWorktrees();
    setAgents([
      {
        id: "agent_claude_1",
        name: "Claude Code",
        task: "Implementing Rust Tauri migration",
        state: "working",
        worktreePath: worktrees[0]?.path || ".",
      },
      {
        id: "agent_codex_1",
        name: "Codex",
        task: "Reviewing PTY session synchronization",
        state: "waiting",
        worktreePath: worktrees[0]?.path || ".",
      },
    ]);
  }, []);

  const ensureTabForWorktree = (wt: Worktree) => {
    const existing = tabs.find((t) => t.cwd === wt.path);
    if (existing) {
      setActiveTabId(existing.id);
      return;
    }

    const dirName = wt.path.split("/").pop() || wt.path;
    const newTab: TerminalTab = {
      id: `tab_${Date.now()}_${Math.random().toString(36).substring(2, 6)}`,
      label: dirName,
      cwd: wt.path,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newTab.id);
  };

  const handleSelectWorktree = (wt: Worktree) => {
    setActiveWorktree(wt);
    ensureTabForWorktree(wt);
  };

  const handleCreateWorktree = async () => {
    const slug = prompt("Enter new worktree slug (e.g. feature-auth):");
    if (!slug || !slug.trim()) return;

    try {
      const wsId = `ws_${Date.now().toString(36)}`;
      const basePath = worktrees[0]?.path || ".";
      const targetPath = `${basePath}/../wt_${slug.trim()}`;
      await invoke("cmd_worktree_create", {
        request: {
          wsId,
          slug: slug.trim(),
          path: targetPath,
        },
      });
      await fetchWorktrees();
    } catch (err: any) {
      alert(`Failed to create worktree: ${err}`);
    }
  };

  const handleAddTerminalTab = () => {
    if (!activeWorktree) return;
    const dirName = activeWorktree.path.split("/").pop() || activeWorktree.path;
    const count = tabs.filter((t) => t.cwd === activeWorktree.path).length + 1;
    const newTab: TerminalTab = {
      id: `tab_${Date.now()}_${Math.random().toString(36).substring(2, 6)}`,
      label: `${dirName} (${count})`,
      cwd: activeWorktree.path,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newTab.id);
  };

  const handleCloseTab = (tabId: string) => {
    const nextTabs = tabs.filter((t) => t.id !== tabId);
    setTabs(nextTabs);
    if (activeTabId === tabId && nextTabs.length > 0) {
      const lastTab = nextTabs[nextTabs.length - 1];
      setActiveTabId(lastTab.id);
      const matchingWt = worktrees.find((w) => w.path === lastTab.cwd);
      if (matchingWt) setActiveWorktree(matchingWt);
    } else if (nextTabs.length === 0) {
      setActiveTabId("");
    }
  };

  const activeAgent = agents.find((a) => a.worktreePath === activeWorktree?.path);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background font-sans text-foreground">
      <Sidebar
        worktrees={worktrees}
        agents={agents}
        activePath={activeWorktree?.path || ""}
        onSelectWorktree={handleSelectWorktree}
        onCreateWorktree={handleCreateWorktree}
      />

      <main className="flex h-full flex-1 flex-col overflow-hidden bg-card">
        {activeWorktree ? (
          <>
            <WorkspaceHeader worktree={activeWorktree} agent={activeAgent} />
            <TabBar
              tabs={tabs}
              activeTabId={activeTabId}
              onActivate={setActiveTabId}
              onClose={handleCloseTab}
              onAdd={handleAddTerminalTab}
            />
            <div className="relative flex-1 overflow-hidden">
              {tabs.map((tab) => (
                <div
                  key={tab.id}
                  className={`absolute inset-0 h-full w-full ${
                    tab.id === activeTabId ? "block" : "hidden"
                  }`}
                >
                  <TerminalPane cwd={tab.cwd} active={tab.id === activeTabId} />
                </div>
              ))}
            </div>
          </>
        ) : (
          <div className="flex h-full w-full flex-col items-center justify-center gap-2 text-muted-foreground">
            <TerminalIcon className="size-8 text-muted-foreground/50" />
            <p className="text-sm">Select or create a worktree from the sidebar to start.</p>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;
