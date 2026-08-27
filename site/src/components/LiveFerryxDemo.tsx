import React, { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Sidebar } from "@ui/components/Sidebar";
import { TerminalSplitView } from "@ui/components/TerminalSplitView";
import { useWorkspaceRuntime } from "@ui/state/workspaceRuntime";
import { useWorkspaceStore } from "@ui/state/workspaceStore";
import { registerProject } from "@ui/lib/tauri";
import type { RegisteredProject } from "@ui/lib/tauri";
import { DemoOmoAgent } from "@/components/DemoOmoAgent";
import type { BrowserTab } from "@ui/lib/types";

const project: RegisteredProject = { workspaceId: "ferryx-demo", repoRoot: ".", gitRoot: null };
// Third-party pages refuse framing via X-Frame-Options, so preview our own docs route.
const DEMO_BROWSER_URL = `${import.meta.env.BASE_URL.replace(/\/$/, "")}/docs/introduction/`;
// Mirrors the desktop new-tab menu, which lists agents resolved as enabled + available.
const DEMO_AGENTS = [
  { name: "claude", available: true, enabled: true, command: "claude", args: "" },
  { name: "codex", available: true, enabled: true, command: "codex", args: "" },
  { name: "gemini", available: true, enabled: true, command: "gemini", args: "" },
];

export default function LiveFerryxDemo() {
  return <Boundary><DemoWindow /></Boundary>;
}
function Boundary({ children }: { children: React.ReactNode }) {
  const [error, setError] = useState(false);
  if (error) return <div className="flex h-[560px] items-center justify-center rounded-[20px] border border-line bg-background text-sm text-ink-soft">Ferryx preview is unavailable right now.</div>;
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
  // Phones get a terminal-only hero: the 236px sidebar plus a side-by-side split leaves
  // ~80px per pane on a 390px viewport, so narrow visitors see just the terminal.
  // Decided once at mount; the demo never re-stages, so rotation keeps the initial shape.
  const [isNarrow] = useState(() => window.matchMedia("(max-width: 639px)").matches);
  const [projects] = useState([project]);
  const store = useWorkspaceStore({ workspaceId: project.workspaceId });
  const { state } = store;
  const { refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({ workspaceId: project.workspaceId, activeWorktreePath: state.activeWorktreePath, syncWorktrees: store.syncWorktrees, ensureTabForWorktree: store.ensureTabForWorktree });
  useEffect(() => { void registerProject({ workspaceId: project.workspaceId, repoPath: "." }).then(refreshWorktrees).catch(reportRuntimeError); }, [refreshWorktrees, reportRuntimeError]);
  const splitStagedRef = useRef(false);

  // Stage the shipped side-by-side layout once a terminal tab exists: a browser tab
  // dropped into its own group is the same path the desktop app takes for a split.
  // The guard checks live state, not just a ref: StrictMode double-invokes this effect
  // and the await below leaves a window where a ref alone still admits a second tab.
  useEffect(() => {
    if (splitStagedRef.current) return;
    if (isNarrow) {
      splitStagedRef.current = true;
      return;
    }
    if (state.layout.tabs.some((tab) => tab.kind === "browser")) {
      splitStagedRef.current = true;
      return;
    }
    const terminalTab = state.layout.tabs.find((tab) => tab.kind !== "browser");
    const groupId = state.layout.focusedGroupId ?? Object.keys(state.layout.tabGroups ?? {})[0];
    if (!terminalTab || !groupId) return;
    splitStagedRef.current = true;
    void (async () => {
      const browserTabId = await store.createBrowserTab(DEMO_BROWSER_URL, "Preview");
      if (browserTabId) store.moveTabToSplit(browserTabId, groupId, "horizontal", "second");
    })().catch(reportRuntimeError);
  }, [isNarrow, state.layout, store, reportRuntimeError]);

  const browser = useMemo(() => state.layout.tabs.find((tab): tab is BrowserTab => tab.kind === "browser"), [state.layout]);

  // TerminalPane always mounts the native libghostty surface, which stays blank without a
  // Tauri host, so the web demo portals its own surface into that pane element.
  const [terminalHost, setTerminalHost] = useState<HTMLElement | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  useEffect(() => {
    const findHost = () => {
      const host = document.querySelector<HTMLElement>('[data-testid="terminal-pane-surface"]');
      if (host) setTerminalHost(host);
      return Boolean(host);
    };
    if (findHost()) return;
    const observer = new MutationObserver(() => {
      if (findHost()) observer.disconnect();
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [state.layout]);
  // The browser pane mounts after this effect's first run, so retry until it exists.
  useEffect(() => {
    if (isNarrow) return;
    const attach = () => {
      const viewport = document.querySelector('[data-testid="browser-viewport"]');
      if (!viewport) return false;
      let frame = viewport.querySelector("iframe") as HTMLIFrameElement | null;
      if (!frame) { frame = document.createElement("iframe"); frame.className = "absolute inset-0 h-full w-full border-0 bg-white"; frame.title = "Embedded browser"; frame.setAttribute("sandbox", "allow-scripts allow-same-origin"); viewport.appendChild(frame); }
      const next = browser?.url ?? "about:blank";
      if (frame.src !== next) frame.src = next;
      return true;
    };
    if (attach()) return;
    const observer = new MutationObserver(() => {
      if (attach()) observer.disconnect();
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [isNarrow, browser?.url, state.layout]);
  const action = (fn: (...args: any[]) => any) => (...args: any[]) => void Promise.resolve(fn(...args)).catch(reportRuntimeError);
  return <div className="demo-locked-split relative h-[560px] overflow-hidden rounded-[20px] ring-1 ring-black/10 shadow-window bg-background">
    <div className="flex h-full min-h-0 select-none font-sans text-foreground"><Sidebar open={!isNarrow} projects={projects} activeProjectId={project.workspaceId} worktrees={state.worktrees} agents={store.agents} activePath={state.activeWorktreePath ?? ""} onSelectWorktree={(w) => void store.ensureTabForWorktree(w)} onCreateWorktree={() => undefined} onOpenCommandPalette={() => undefined} />
      <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background"><TerminalSplitView layout={state.layout} sessions={state.sessions} unreadTabIds={state.unreadTabIds} activityByTabId={store.tabActivity} agents={DEMO_AGENTS} defaultAgentId="claude" leadingSpacer={isNarrow ? 72 : 0} onOpenSettings={() => setSettingsOpen(true)} onLaunchAgent={() => { const w = state.worktrees[0]; if (w) void store.openTab(w); }} onActivateTab={store.activateTab} onCloseTab={action(store.closeTab)} onMoveTabToGroup={store.moveTabToGroup} onRenameTab={store.renameTab} onToggleTabPin={store.setTabPinned} onCloseOtherTabs={store.closeOtherTabs} onCloseTabsToRight={store.closeTabsToRight} onCloseTabsToLeft={store.closeTabsToLeft} onSplitPane={() => undefined} onClosePane={() => undefined} onMoveTabToSplit={() => undefined} onSetRatio={store.setPaneRatio} onSetGroupRatio={store.setTabGroupRatio} onSwapPanes={store.swapPanes} onFocusPane={store.focusPane} onAddTab={() => { const w = state.worktrees[0]; if (w) void store.openTab(w); }} onAddBrowserTab={(url) => void store.createBrowserTab(url ?? DEMO_BROWSER_URL)} onNavigateBrowserTab={(id, url) => void store.navigateBrowserTab(id, url)} onReloadBrowserTab={(id) => void store.reloadBrowserTab(id)} onTitleChange={(id, title, session) => store.updateSessionTitleActivity(id, title, session)} /></main>
    </div>
    {/* The desktop window gets native macOS controls; the sidebar (mobile: the tab strip's
       leading spacer) reserves a 72px pad, and the demo draws the lights into it. */}
    <div className="pointer-events-none absolute left-0 top-0 z-30 flex h-titlebar items-center gap-2 pl-3.5">
      <i className="h-3 w-3 rounded-full bg-[#ff5f57]" />
      <i className="h-3 w-3 rounded-full bg-[#febc2e]" />
      <i className="h-3 w-3 rounded-full bg-[#28c840]" />
    </div>
    {settingsOpen ? (
      <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-3 border-t border-border bg-card px-4 py-2 text-xs text-muted-foreground">
        <span>Settings live in the desktop app.</span>
        <button type="button" onClick={() => setSettingsOpen(false)} className="rounded px-2 py-1 text-foreground hover:bg-accent">Dismiss</button>
      </div>
    ) : null}
    {terminalHost ? createPortal(<DemoOmoAgent />, terminalHost) : null}
  </div>;
}
