import { describe, expect, it, vi } from "vitest";
import { createLayoutState, layoutReducer, toPaneContent } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import { hydrateLayout, intentForAction, layoutScope, rehydrateWorkspace, LayoutSync, type DaemonLayout, type LayoutReply, type LayoutTransport } from "./layoutSync";
import { tabTransferIntent } from "../state/windowMove";
import { closeTerminal } from "./tauri";
import { withTimeout } from "./withTimeout";

vi.mock("./tauri", () => ({ closeTerminal: vi.fn() }));

function daemon(revision = 1): DaemonLayout {
  return { revision, activeTabId: "tab", focusedGroupId: "main",
    tabGroupLayout: { type: "group", groupId: "main", tabIds: ["tab"], activeTabId: "tab" },
    tabs: [{ tabId: "tab", label: "daemon", pinned: false, activeNodeId: 1, expandedNodeId: null,
      paneTree: { type: "leaf", nodeId: 1, content: { kind: "terminal", backendSessionId: "backend" } } }] };
}
function workspace(): WorkspaceState {
  return { workspaceId: "sync-test", activeWorktreePath: "/repo", worktrees: [],
    sessions: { front: { id: "front", cwd: "/repo", workspaceId: "sync-test", backendSessionId: "backend", lifecycle: "working", worktree: null } },
    layout: createLayoutState([{ id: "tab", label: "local", sessionId: "front" }]),
    worktreeLayouts: {}, unreadTabIds: {}, unreadWorktreePaths: {}, activityBySessionId: {} };
}
function ready(layout: DaemonLayout): LayoutReply {
  return { type: "ready", view: { revision: layout.revision, layout } };
}

describe("daemon layout revisions", () => {
  it("keeps foreign session ids disconnected even when a local backend id matches", () => {
    const state = workspace();
    const saved = daemon();
    saved.tabs[0].paneTree = { type: "leaf", nodeId: 700,
      content: { kind: "disconnectedTerminal", machineId: "other-host", backendSessionId: "backend" } };
    saved.tabs[0].activeNodeId = 700;
    const after = rehydrateWorkspace(state, { revision: 1, layout: saved });
    expect(after.sessions.front).toBe(state.sessions.front);
    const id = Object.values(after.layout.layoutsByTabId.tab.sessionIdsByLeafId)[0];
    expect(id).not.toBe("front");
    expect(after.sessions[id]).toMatchObject({ backendSessionId: null, lifecycle: "failed",
      remoteConnectionState: "disconnected", backendUnavailableReason: "remote_session_gone" });
  });
  it("restores plugin identity without creating or closing terminal sessions", () => {
    const state = workspace();
    const saved = daemon();
    saved.tabs[0].paneTree = { type: "leaf", nodeId: 501,
      content: { kind: "plugin", plugin: "clock", view: "main" } };
    saved.tabs[0].activeNodeId = 501;
    const layout = hydrateLayout({ revision: 1, layout: saved }, state);
    const content = Object.values(layout.layoutsByTabId.tab.contentsByLeafId!)[0];
    expect(content).toEqual({ kind: "plugin", plugin: "clock", view: "main" });
    expect(toPaneContent(content)).toEqual(content);
    expect(Object.values(layout.layoutsByTabId.tab.sessionIdsByLeafId)).toEqual([""]);
  });
  it("selects parked checkout scopes and reconstructs panes without a GUI registry", () => {
    const state = workspace();
    const parked = daemon();
    parked.scope = layoutScope(state);
    parked.tabs[0].paneTree = { type: "split", dir: "horizontal", ratio: 0.5,
      a: { type: "leaf", nodeId: 71, content: { kind: "browser", browserId: "browser-scope", url: "https://example.org" } },
      b: { type: "leaf", nodeId: 72, content: { kind: "file", path: "/repo/file", line: 2, col: 3,
        backendSessionId: "backend", workspaceId: "sync-test", previewId: "owned-preview" } } };
    parked.tabs[0].activeNodeId = 71;
    const root = daemon(); root.scope = JSON.stringify(["other", null]);
    root.parkedLayouts = { [parked.scope]: parked };
    const restored = rehydrateWorkspace({ ...state, sessions: {}, layout: createLayoutState() }, { revision: 1, layout: root });
    const contents = Object.values(restored.layout.layoutsByTabId.tab.contentsByLeafId!);
    expect(contents).toContainEqual(expect.objectContaining({ kind: "browser", browser: expect.objectContaining({ browserId: "browser-scope" }) }));
    expect(contents).toContainEqual(expect.objectContaining({ kind: "file", previewId: "owned-preview", backendSessionId: "backend" }));
    const terminal = daemon(); terminal.scope = parked.scope;
    const admitted = rehydrateWorkspace({ ...state, sessions: {} }, { revision: 1, layout: terminal });
    expect(admitted.sessions.backend.backendSessionId).toBe("backend");
    const other = rehydrateWorkspace({ ...state, activeWorktreePath: "/other" }, { revision: 1, layout: root });
    expect(other.layout.tabs).toHaveLength(0);
  });

  it("encodes transfer and replacement intents without frontend visual identities", async () => {
    const state = workspace();
    expect(tabTransferIntent(state, "tab", "second")).toEqual({ type: "SCOPED", scope: layoutScope(state),
      intent: { type: "TRANSFER_TAB", tabId: "tab", targetWindow: "second", targetScope: layoutScope(state) } });
    const sync = new LayoutSync({ get: async () => ready(daemon()), apply: vi.fn() });
    await sync.refresh();
    const replacement = { id: "new", label: "new", sessionId: "front" };
    const after = { ...state, layout: layoutReducer(state.layout, { type: "CLOSE_TAB", tabId: "tab", replacementTab: replacement }) };
    expect(intentForAction(sync, { type: "CLOSE_TAB", tabId: "tab", replacement: { tab: replacement } }, state, after)).toEqual({
      type: "SCOPED", scope: layoutScope(state), intent: { type: "CLOSE_TAB", tabId: "tab", replacement: {
        tabId: "new", label: "new", content: { kind: "terminal", backendSessionId: "backend" },
      } },
    });
  });

  it("sends resolved collinear seam geometry", async () => {
    const state = workspace();
    state.layout = layoutReducer(state.layout, { type: "SPLIT_PANE", tabId: "tab", direction: "horizontal", newLeafId: "second", sessionId: "front" });
    const sync = new LayoutSync({ get: async () => ready(daemon()), apply: vi.fn() });
    await sync.refresh();
    const action = { type: "SET_PANE_RATIO" as const, tabId: "tab", path: "", ratio: 0.7 };
    expect(intentForAction(sync, action, state, state)).toMatchObject({ type: "SCOPED", intent: {
      type: "SET_PANE_RATIO", seam: { direction: "horizontal", targetPath: "", members: [{ path: "", origin: 0, span: 1 }] },
    } });
  });
  it("applies optimistically then replaces a conflict without closing a PTY", async () => {
    vi.mocked(closeTerminal).mockClear();
    const io: LayoutTransport = { get: vi.fn(async () => ready(daemon())),
      apply: vi.fn(async (): Promise<LayoutReply> => ({ type: "conflict", code: "LAYOUT_CONFLICT", revision: 8, layout: daemon(8) })) };
    const sync = new LayoutSync(io);
    await sync.refresh();
    const before = workspace();
    before.layout = hydrateLayout(sync.current()!, before);
    const action = { type: "RENAME_TAB" as const, tabId: "tab", label: "optimistic" };
    let after = { ...before, layout: layoutReducer(before.layout, action) };
    expect(after.layout.tabs[0].label).toBe("optimistic");
    await sync.send(() => intentForAction(sync, action, before, after), () => {
      after = { ...after, layout: hydrateLayout(sync.current()!, after) };
    });
    expect(io.apply).toHaveBeenCalledWith(1, { type: "SCOPED", scope: layoutScope(before), intent: action });
    expect(after.layout.tabs[0].label).toBe("daemon");
    expect(after.sessions).toBe(before.sessions);
    expect(closeTerminal).not.toHaveBeenCalled();
    expect(sync.revision).toBe(8);
  });

  it("discards queued stale closes on conflict instead of replaying them", async () => {
    let respond!: (reply: LayoutReply) => void;
    let started!: () => void;
    const applied = new Promise<void>((resolve) => { started = resolve; });
    const io: LayoutTransport = { get: async () => ready(daemon()),
      apply: vi.fn(() => { started(); return new Promise<LayoutReply>((done) => { respond = done; }); }) };
    const sync = new LayoutSync(io);
    await sync.refresh();
    const first = sync.send(() => ({ type: "RENAME_TAB", tabId: "tab", label: "a" }), () => {});
    const staleClose = sync.send(() => ({ type: "CLOSE_TAB", tabId: "tab" }), () => {});
    await withTimeout(applied, 1_000, "layout apply signal");
    respond({ type: "conflict", code: "LAYOUT_CONFLICT", revision: 2, layout: daemon(2) });
    await withTimeout(Promise.all([first, staleClose]), 1_000, "layout conflict reconciliation");
    expect(io.apply).toHaveBeenCalledTimes(1);
    expect(sync.current()!.layout.tabs).toHaveLength(1);
  });

  it("keeps leaf and backend identities across a revision refresh", () => {
    const before = workspace();
    const first = hydrateLayout({ revision: 1, layout: daemon() }, before);
    const after = hydrateLayout({ revision: 2, layout: daemon(2) }, { ...before, layout: first });
    expect(after.layoutsByTabId.tab.activeLeafId).toBe(first.layoutsByTabId.tab.activeLeafId);
    expect(after.layoutsByTabId.tab.sessionIdsByLeafId).toEqual(first.layoutsByTabId.tab.sessionIdsByLeafId);
    expect(before.sessions.front.backendSessionId).toBe("backend");
  });

  it("leaves older daemons on the offline layout path", async () => {
    const io: LayoutTransport = { get: async () => ({ type: "unsupported" }), apply: vi.fn() };
    const sync = new LayoutSync(io);
    expect(await sync.refresh()).toBe(false);
    await sync.send(() => ({ type: "CLOSE_TAB", tabId: "tab" }), () => {});
    expect(io.apply).not.toHaveBeenCalled();
  });
});
