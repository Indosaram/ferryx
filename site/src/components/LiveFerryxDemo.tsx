import React, { useEffect, useMemo, useState } from "react";
import { Sidebar } from "@ui/components/Sidebar";
import { TerminalSplitView } from "@ui/components/TerminalSplitView";
import { useWorkspaceRuntime } from "@ui/state/workspaceRuntime";
import { useWorkspaceStore } from "@ui/state/workspaceStore";
import { registerProject } from "@ui/lib/tauri";
import type { RegisteredProject } from "@ui/lib/tauri";
import type { BrowserTab } from "@ui/lib/types";

const project: RegisteredProject = { workspaceId: "ferryx-demo", repoRoot: "." };

export default function LiveFerryxDemo() {
  return <Boundary><DemoWindow /></Boundary>;
}
function Boundary({ children }: { children: React.ReactNode }) {
  const [error, setError] = useState(false);
  if (error) return <div className="flex h-[560px] items-center justify-center rounded-xl border border-zinc-800 bg-zinc-950 text-sm text-zinc-400">Ferryx preview is unavailable right now.</div>;
  return <ErrorCatcher onError={() => setError(true)}>{children}</ErrorCatcher>;
}
function ErrorCatcher({ children, onError }: { children: React.ReactNode; onError: () => void }) {
  useEffect(() => undefined, []);
  // Error boundaries must be class components; this tiny adapter keeps the fallback local.
  return <Catcher onError={onError}>{children}</Catcher>;
}
class Catcher extends React.Component<{ onError: () => void; children: React.ReactNode }> {
  static getDerivedStateFromError() { return { failed: true }; }
  state = { failed: false };
  componentDidCatch() { this.props.onError(); }
  render() { return this.state.failed ? null : this.props.children; }
}
function DemoWindow() {
  const [projects] = useState([project]);
  const store = useWorkspaceStore({ workspaceId: project.workspaceId });
  const { state } = store;
  const { refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({ workspaceId: project.workspaceId, activeWorktreePath: state.activeWorktreePath, syncWorktrees: store.syncWorktrees, ensureTabForWorktree: store.ensureTabForWorktree });
  useEffect(() => { void registerProject({ workspaceId: project.workspaceId, repoPath: "." }).then(refreshWorktrees).catch(reportRuntimeError); }, [refreshWorktrees, reportRuntimeError]);
  const browser = useMemo(() => state.layout.tabs.find((tab): tab is BrowserTab => tab.kind === "browser" && tab.id === state.layout.activeTabId), [state.layout]);
  useEffect(() => {
    const viewport = document.querySelector('[data-testid="browser-viewport"]');
    if (!viewport) return;
    let frame = viewport.querySelector("iframe") as HTMLIFrameElement | null;
    if (!frame) { frame = document.createElement("iframe"); frame.className = "absolute inset-0 h-full w-full border-0 bg-white"; frame.title = "Embedded browser"; viewport.appendChild(frame); }
    frame.src = browser?.url ?? "about:blank";
  }, [browser?.url]);
  const action = (fn: (...args: any[]) => any) => (...args: any[]) => void Promise.resolve(fn(...args)).catch(reportRuntimeError);
  return <div className="h-[560px] overflow-hidden rounded-xl border border-zinc-700 bg-[#23262d] shadow-2xl">
    <div className="flex h-9 items-center gap-2 border-b border-zinc-700 bg-zinc-900 px-4"><i className="h-3 w-3 rounded-full bg-red-400" /><i className="h-3 w-3 rounded-full bg-yellow-400" /><i className="h-3 w-3 rounded-full bg-green-400" /><span className="ml-2 text-xs text-zinc-400">Ferryx</span></div>
    <div className="flex h-[524px] min-h-0"><Sidebar open projects={projects} activeProjectId={project.workspaceId} worktrees={state.worktrees} agents={store.agents} activePath={state.activeWorktreePath ?? ""} onSelectWorktree={(w) => void store.ensureTabForWorktree(w)} onCreateWorktree={() => undefined} onOpenCommandPalette={() => undefined} />
      <main className="flex min-w-0 flex-1"><TerminalSplitView layout={state.layout} sessions={state.sessions} unreadTabIds={state.unreadTabIds} activityByTabId={store.tabActivity} onActivateTab={store.activateTab} onCloseTab={action(store.closeTab)} onAddTab={() => { const w = state.worktrees[0]; if (w) void store.openTab(w); }} onAddBrowserTab={(url) => void store.createBrowserTab(url ?? "https://example.com")} onNavigateBrowserTab={(id, url) => void store.navigateBrowserTab(id, url)} onReloadBrowserTab={(id) => void store.reloadBrowserTab(id)} onTitleChange={(id, title, session) => store.updateSessionTitleActivity(id, title, session)} /></main>
    </div>
  </div>;
}
