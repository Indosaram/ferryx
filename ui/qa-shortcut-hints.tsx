import { useState } from "react";
import { createRoot } from "react-dom/client";
import { ShortcutHints } from "./src/components/ShortcutHints";
import { Sidebar } from "./src/components/Sidebar";
import { TerminalSplitView } from "./src/components/TerminalSplitView";
import { BrowserToolbar } from "./src/components/BrowserToolbar";
import { SHORTCUTS, useShortcuts } from "./src/lib/shortcuts";
import type { LayoutState, TerminalSession, Worktree } from "./src/lib/types";
import "./src/index.css";
import "./src/settings-runtime.css";

let callbackId = 0;
Object.defineProperty(window, "__TAURI_INTERNALS__", {
  value: {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    transformCallback: () => ++callbackId,
    unregisterCallback: () => undefined,
    invoke: async (command: string) => command.startsWith("plugin:event|") ? callbackId : null,
  },
});
const params = new URLSearchParams(location.search);
document.documentElement.dataset.theme = params.get("theme") ?? "dark";
const isMac = params.get("platform") !== "linux";
Object.defineProperty(navigator, "platform", { value: isMac ? "MacIntel" : "Linux x86_64" });
Object.defineProperty(navigator, "userAgent", { value: isMac ? "Macintosh" : "Linux" });

const worktrees: Worktree[] = [
  { path: "/fixture", branch: "main", workspaceId: "demo", isMain: true },
  { path: "/fixture/.orca-worktrees/wt-review", branch: "orca/demo/review", workspaceId: "demo", isMain: false },
];
const tabs = [{ id: "tab-a", label: "main", sessionId: "session-a" }, { id: "tab-b", label: "review", sessionId: "session-b" }];
const layout: LayoutState = {
  tabs, activeTabId: "tab-a",
  layoutsByTabId: {
    "tab-a": {
      root: { type: "split", direction: "horizontal", ratio: 0.5, first: { type: "leaf", leafId: "leaf-a" }, second: { type: "leaf", leafId: "leaf-b" } },
      activeLeafId: "leaf-a", expandedLeafId: null, sessionIdsByLeafId: { "leaf-a": "session-a", "leaf-b": "session-b" },
    },
    "tab-b": { root: { type: "leaf", leafId: "leaf-review" }, activeLeafId: "leaf-review", expandedLeafId: null, sessionIdsByLeafId: { "leaf-review": "session-b" } },
  },
};
const sessions: Record<string, TerminalSession> = Object.fromEntries(["a", "b"].map((id) => [
  `session-${id}`, { id: `session-${id}`, cwd: "/fixture", workspaceId: "demo", backendSessionId: null, lifecycle: "exited" },
]));

function Fixture() {
  const [newTabs, setNewTabs] = useState(0);
  const [selectedTab, setSelectedTab] = useState("tab-a");
  const [selectedPath, setSelectedPath] = useState("/fixture");
  useShortcuts({
    "tab.newTerminal": () => setNewTabs((count) => count + 1),
    "tab.select1": () => setSelectedTab("tab-a"),
    "tab.select2": () => setSelectedTab("tab-b"),
    "workspace.select1": () => setSelectedPath(worktrees[0].path),
    "workspace.select2": () => setSelectedPath(worktrees[1].path),
  }, { isMac });
  return (
    <div className="flex h-screen w-screen bg-background text-foreground">
      <ShortcutHints isMac={isMac} enabledActions={SHORTCUTS.map((shortcut) => shortcut.id)}
        getContext={() => ({ tabIds: tabs.map((tab) => tab.id), closeTabId: null, worktrees })} />
      <Sidebar projects={[{ workspaceId: "demo", repoRoot: "/fixture", gitRoot: "/fixture" }]} activeProjectId="demo"
        worktrees={worktrees} activePath={selectedPath} agents={[]} onSelectWorktree={(row) => setSelectedPath(row.path)}
        onCreateWorktree={() => undefined} onOpenSettings={() => undefined} onToggle={() => undefined} />
      <main className="flex min-w-0 flex-1 flex-col">
        <div className="p-2 text-xs text-muted-foreground" data-qa-new-tabs={newTabs}>
          Real React components; native IPC stubbed. New tabs: {newTabs}
        </div>
        <BrowserToolbar tab={{ id: "browser-qa", kind: "browser", label: "Browser", browserId: "qa", url: "https://example.com", canGoBack: true, canGoForward: false, loading: false }}
          onNavigate={() => undefined} onReload={() => undefined} />
        <TerminalSplitView layout={{ ...layout, activeTabId: selectedTab }} sessions={sessions}
          onActivateTab={setSelectedTab} onAddTab={() => setNewTabs((count) => count + 1)} />
      </main>
    </div>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<Fixture />);
