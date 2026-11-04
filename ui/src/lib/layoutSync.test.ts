import { describe, expect, it, vi } from "vitest";
import { createLayoutState, layoutReducer } from "../state/layout";
import type { WorkspaceState } from "../state/workspaceStore";
import { hydrateLayout, intentForAction, LayoutSync, type DaemonLayout, type LayoutReply, type LayoutTransport } from "./layoutSync";
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
    sessions: { front: { id: "front", cwd: "/repo", workspaceId: "sync-test", backendSessionId: "backend", lifecycle: "working" } },
    layout: createLayoutState([{ id: "tab", label: "local", sessionId: "front" }]),
    worktreeLayouts: {}, unreadTabIds: {}, unreadWorktreePaths: {}, activityBySessionId: {} };
}
function ready(layout: DaemonLayout): LayoutReply {
  return { type: "ready", view: { revision: layout.revision, layout } };
}

describe("daemon layout revisions", () => {
  it("applies optimistically then replaces a conflict without closing a PTY", async () => {
    vi.mocked(closeTerminal).mockClear();
    const io: LayoutTransport = { get: vi.fn(async () => ready(daemon())),
      apply: vi.fn(async () => ({ type: "conflict", code: "LAYOUT_CONFLICT", revision: 8, layout: daemon(8) })) };
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
    expect(io.apply).toHaveBeenCalledWith(1, action);
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

