// One session list, four surfaces: the supervisor panel row, the sidebar worktree slot, the tab dot
// and the phone's pane row must all draw the daemon's attention group for each session. Each
// fixture's local state deliberately disagrees with its group, so a surface that judges for itself
// instead of reading `attention.group` fails here.
import { DndContext } from "@dnd-kit/core";
import { SortableContext, horizontalListSortingStrategy } from "@dnd-kit/sortable";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { deriveFocusedTerminal } from "../App";
import { worktreeRowSlot } from "../components/sidebar/sidebarSlots";
import { PanelSurface } from "../components/supervisorPanel/PanelSurface";
import { SortableTab } from "../components/tab-dnd/SortableTab";
import { selectTabActivitySummaries, selectWorktreeActivitySummaries, type WorkspaceState } from "../state/workspaceStore";
import { attentionBySessionIdOf, type AttentionGroup } from "./attentionGroup";
import type { FleetSession, PanelSnapshot } from "./supervisorPanel/contract";
import { buildPanelView } from "./supervisorPanel/model";
import type { WorkspaceTab } from "./types";

afterEach(cleanup);

const NOW = Date.UTC(2026, 10, 12, 9, 0, 0);

type Fixture = { id: string; group: AttentionGroup; state: FleetSession["state"]; local: "working" | "waiting" | "done" };

// Local state and wire state contradict the group on purpose.
const FIXTURES: Fixture[] = [
  { id: "a", group: "needsYou", state: "idle", local: "working" },
  { id: "b", group: "unread", state: "working", local: "waiting" },
  { id: "c", group: "working", state: "blocked", local: "done" },
  { id: "d", group: "seen", state: "blocked", local: "waiting" },
];

function fleetSession(fixture: Fixture, group: AttentionGroup): FleetSession {
  return {
    sessionRef: `mac:backend-${fixture.id}`,
    hostId: "mac",
    hostLabel: "mac",
    title: `session ${fixture.id}`,
    cwd: `/${fixture.id}`,
    agent: "claude",
    state: fixture.state,
    stateSource: "provider",
    stateSinceMs: NOW,
    screenChangedAtMs: NOW,
    pendingPrompt: null,
    lastAssistantLine: null,
    updatedAtMs: NOW,
    stale: false,
    attention: {
      axes: { demand: "none", activity: "unknown", completion: "none", read: "unread", ownership: "owner" },
      group,
    },
    reason: `why ${fixture.id}`,
  };
}

function workspace(): WorkspaceState {
  return {
    workspaceId: "w",
    activeWorktreePath: "/a",
    unreadTabIds: {},
    unreadWorktreePaths: {},
    worktrees: FIXTURES.map((f) => ({ path: `/${f.id}`, head: "abc", branch: `ferryx/w/${f.id}`, bare: false, detached: false, locked: null, prunable: null })),
    sessions: Object.fromEntries(FIXTURES.map((f) => [f.id, {
      id: f.id, cwd: `/${f.id}`, worktreePath: `/${f.id}`, workspaceId: "w", backendSessionId: `backend-${f.id}`, worktree: null, lifecycle: "working",
    }])),
    layout: { tabs: FIXTURES.map((f) => ({ id: `tab-${f.id}`, label: f.id, sessionId: f.id })), activeTabId: "tab-a", layoutsByTabId: {} },
    activityBySessionId: Object.fromEntries(FIXTURES.map((f) => [f.id, { state: f.local, seen: false, title: "", isAgent: true }])),
  } as unknown as WorkspaceState;
}

const PANEL_GLYPH: Record<string, AttentionGroup> = { needsYou: "needsYou", unread: "unread", working: "working", idle: "seen" };
const SLOT: Record<string, AttentionGroup> = { waiting: "needsYou", done: "unread", working: "working", null: "seen" };
const TAB: Record<string, AttentionGroup> = { "tab-waiting-indicator": "needsYou", "tab-done-indicator": "unread", "tab-working-indicator": "working", none: "seen" };
const PHONE: Record<string, AttentionGroup> = { waiting: "needsYou", done: "unread", working: "working", none: "seen" };

function tabIndicator(tabId: string, summary: ReturnType<typeof selectTabActivitySummaries>[string]): string {
  const tab = { id: tabId, label: tabId, sessionId: tabId.slice(4) } as WorkspaceTab;
  const view = render(
    <DndContext>
      <SortableContext items={[`tab:${tab.id}`]} strategy={horizontalListSortingStrategy}>
        <SortableTab
          tab={tab} groupId="g" index={0} active={false} unread={false} activity={summary}
          isRenaming={false} renameValue="" onRenameValueChange={vi.fn()} onCommitRename={vi.fn()}
          onCancelRename={vi.fn()} onActivate={vi.fn()} onClose={vi.fn()} onContextMenu={vi.fn()}
        />
      </SortableContext>
    </DndContext>,
  );
  const dot = view.container.querySelector('[data-testid^="tab-"][data-testid$="-indicator"], [data-testid="tab-unread-dot"]');
  view.unmount();
  return dot?.getAttribute("data-testid") ?? "none";
}

/** The group each surface draws for each fixture session. */
function surfaces(groups: Record<string, AttentionGroup>) {
  const sessions = FIXTURES.map((f) => fleetSession(f, groups[f.id]));
  const snapshot: PanelSnapshot = { decisions: [], handling: [], sessions, log: [], rules: [], paused: false, setupRequired: false };
  const attention = attentionBySessionIdOf(sessions);
  const state = workspace();

  const panelRows = buildPanelView(snapshot, { nowMs: NOW }).sessions;
  const worktreeSummaries = selectWorktreeActivitySummaries(state, attention);
  const tabSummaries = selectTabActivitySummaries(state, attention);
  const phoneTabs = deriveFocusedTerminal("w", state, [], attention)?.terminalTabs ?? [];

  return Object.fromEntries(FIXTURES.map((f) => [f.id, {
    panel: PANEL_GLYPH[panelRows.find((row) => row.sessionRef === `mac:backend-${f.id}`)!.glyph],
    sidebar: SLOT[String(worktreeRowSlot({ activitySummary: worktreeSummaries[`/${f.id}`], unread: false, agentState: undefined }))],
    tab: TAB[tabIndicator(`tab-${f.id}`, tabSummaries[`tab-${f.id}`])],
    phone: PHONE[phoneTabs.find((tab) => tab.sessionId === `backend-${f.id}`)?.activityState ?? "none"],
  }]));
}

const groupsOf = (fixtures: Fixture[]) => Object.fromEntries(fixtures.map((f) => [f.id, f.group]));

describe("one attention group on every surface", () => {
  it("draws each session's daemon group in the panel, sidebar slot, tab and phone row", () => {
    const drawn = surfaces(groupsOf(FIXTURES));
    for (const f of FIXTURES) {
      expect(drawn[f.id]).toEqual({ panel: f.group, sidebar: f.group, tab: f.group, phone: f.group });
    }
  });

  it("moves all four views together when one session's group changes", () => {
    const before = surfaces(groupsOf(FIXTURES));
    const after = surfaces({ ...groupsOf(FIXTURES), a: "working" });
    expect(before.a).toEqual({ panel: "needsYou", sidebar: "needsYou", tab: "needsYou", phone: "needsYou" });
    expect(after.a).toEqual({ panel: "working", sidebar: "working", tab: "working", phone: "working" });
    for (const id of ["b", "c", "d"]) expect(after[id]).toEqual(before[id]);
  });

  it("shows the reason as the panel row's second line", () => {
    const sessions = FIXTURES.map((f) => fleetSession(f, f.group));
    const snapshot: PanelSnapshot = { decisions: [], handling: [], sessions, log: [], rules: [], paused: false, setupRequired: false };
    render(<PanelSurface snapshot={snapshot} actions={{ openSession: vi.fn() } as never} nowMs={NOW} touch />);
    const row = screen.getAllByTestId("session-row").find((r) => r.textContent?.includes("session a"))!;
    expect(row).toHaveAttribute("data-state", "needs");
    expect(row.textContent).toContain("why a");
  });
});

describe("groupOf for older peers", () => {
  it("maps a session without attention from its state", async () => {
    const { groupOf } = await import("./attentionGroup");
    expect(groupOf({ state: "blocked" })).toBe("needsYou");
    expect(groupOf({ state: "waiting", seen: true })).toBe("seen");
    expect(groupOf({ state: "done" })).toBe("unread");
    expect(groupOf({ state: "working" })).toBe("working");
    expect(groupOf({ state: "idle" })).toBe("seen");
    expect(groupOf({ state: "unknown" })).toBe("seen");
    expect(groupOf({ attention: { group: "working" }, state: "blocked" })).toBe("working");
  });
});
