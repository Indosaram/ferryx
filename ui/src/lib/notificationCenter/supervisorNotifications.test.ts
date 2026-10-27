import { afterEach, describe, expect, it } from "vitest";

import { buildAttentionRows } from "../../features/ferryx/attention/attentionModel";
import {
  SUPERVISOR_STOP_REASON_LABELS,
  SUPERVISOR_STOP_REASONS,
  supervisorStopReasonLabel,
  type SupervisorGoalView,
  type SupervisorLane,
  type SupervisorSnapshot,
} from "../supervisorModel";
import { restoreNotificationInbox } from "./notificationCenterPersistence";
import { createNotificationCenterStore } from "./notificationCenterStore";
import { syncSupervisorNotifications, type SupervisorPaneResolver } from "./supervisorRecording";
import { notificationEntryId } from "./types";

const KEY = "ferryx.notifications.history:v1";

const goal = (overrides: Partial<SupervisorGoalView> = {}): SupervisorGoalView => ({
  mode: "supervised", turnsUsed: 3, turnLimit: 20, revision: 2, requirements: [],
  stopReason: null, proposal: null, pendingRevisions: [], ...overrides,
});
const lane = (goalContract: SupervisorGoalView | null | undefined, overrides: Partial<SupervisorLane> = {}): SupervisorLane => ({
  key: "ledger", title: "Ledger", status: "idle", sessionId: "hub-pty-1", cwd: "/repo", goal: "ship",
  paused: false, lastActivityAt: "", goalContract, ...overrides,
});
const snapshot = (...lanes: SupervisorLane[]): SupervisorSnapshot => ({
  hub: { available: true, running: true }, lanes, watched: [], actions: [],
});
const resolvePane: SupervisorPaneResolver = (hubSessionId) => hubSessionId === "hub-pty-1"
  ? { workspaceId: "ws", sessionId: "pane-1", labels: { workspaceLabel: "ferryx", worktreeLabel: "main" } }
  : null;

const stores: ReturnType<typeof createNotificationCenterStore>[] = [];
function create(options: Parameters<typeof createNotificationCenterStore>[0] = { storage: null }) {
  const store = createNotificationCenterStore(options);
  stores.push(store);
  return store;
}
afterEach(() => { stores.splice(0).forEach((store) => store.dispose()); localStorage.clear(); });

describe("supervisor stop-reason labels", () => {
  it("labels every Rust StopReason string", () => {
    expect(SUPERVISOR_STOP_REASONS).toHaveLength(7);
    for (const reason of SUPERVISOR_STOP_REASONS) {
      const label = SUPERVISOR_STOP_REASON_LABELS[reason];
      expect(typeof label).toBe("string");
      expect(label.length).toBeGreaterThan(0);
      expect(supervisorStopReasonLabel(reason, false)).toBe(label);
    }
    expect(new Set(Object.keys(SUPERVISOR_STOP_REASON_LABELS))).toEqual(new Set(SUPERVISOR_STOP_REASONS));
  });

  it("uses the fixed wording", () => {
    expect(supervisorStopReasonLabel("delivery_uncertain")).toBe("Instruction may not have arrived");
    expect(supervisorStopReasonLabel(null, true)).toBe("Next step ready for review");
    expect(supervisorStopReasonLabel(null, "awaiting_approval")).toBe("Next step ready for review");
    expect(supervisorStopReasonLabel(null, false)).toBeNull();
  });
});

describe("supervisor hand-offs in the Inbox", () => {
  it("records one supervisor entry for a stop snapshot", () => {
    const store = create();
    expect(syncSupervisorNotifications(snapshot(lane(goal({ stopReason: "turn_limit" }))), resolvePane, store, 500)).toBe(1);
    const entries = store.getSnapshot().entries;
    expect(entries).toHaveLength(1);
    expect(entries[0]).toMatchObject({
      id: notificationEntryId("ws", "pane-1"), reason: "supervisor", subject: "agent", read: { unread: true },
      labels: { agentLabel: "Ledger", workspaceLabel: "ferryx", worktreeLabel: "main" },
      supervisor: { laneKey: "ledger", stopReason: "turn_limit", awaitingApproval: false, goalRevision: 2 },
    });
  });

  it("keeps a single entry when the same revision arrives twice, even after it was read", () => {
    const store = create();
    const stopped = snapshot(lane(goal({ stopReason: "no_further_progress" })));
    syncSupervisorNotifications(stopped, resolvePane, store, 500);
    expect(syncSupervisorNotifications(stopped, resolvePane, store, 600)).toBe(0);
    store.markAllRead(700);
    expect(syncSupervisorNotifications(stopped, resolvePane, store, 800)).toBe(0);
    expect(store.getSnapshot().entries).toHaveLength(1);
    expect(store.getSnapshot().entries[0]).toMatchObject({ occurrenceCount: 1, read: { seen: true, seenAt: 700 } });
  });

  it("records a new hand-off for a new revision or an awaiting approval", () => {
    const store = create();
    syncSupervisorNotifications(snapshot(lane(goal({ stopReason: "turn_limit" }))), resolvePane, store, 500);
    expect(syncSupervisorNotifications(snapshot(lane(goal({
      revision: 3, proposal: { text: "run tests", state: "awaiting_approval", gapIds: [] },
    }))), resolvePane, store, 600)).toBe(1);
    expect(store.getSnapshot().entries).toHaveLength(1);
    expect(store.getSnapshot().entries[0]).toMatchObject({
      occurrenceCount: 2, supervisor: { stopReason: null, awaitingApproval: true, goalRevision: 3 },
    });
  });

  it("ignores contract-less, running, unbound and unmatched lanes", () => {
    const store = create();
    const recorded = syncSupervisorNotifications(snapshot(
      lane(undefined),
      lane(null, { key: "a" }),
      lane(goal(), { key: "b" }),
      lane(goal({ proposal: { text: "x", state: "pending", gapIds: [] } }), { key: "c" }),
      lane(goal({ stopReason: "turn_limit" }), { key: "d", sessionId: null }),
      lane(goal({ stopReason: "turn_limit" }), { key: "e", sessionId: "elsewhere" }),
    ), resolvePane, store, 500);
    expect(recorded).toBe(0);
    expect(store.getSnapshot().entries).toEqual([]);
  });

  it("shows the fixed label on the attention row", () => {
    const store = create();
    syncSupervisorNotifications(snapshot(lane(goal({ stopReason: "delivery_uncertain" }))), resolvePane, store, 500);
    const [row] = buildAttentionRows(store.getSnapshot().entries, () => ({ state: "done" }));
    expect(row).toMatchObject({ state: "needs-you", sessionId: "pane-1", supervisorLabel: "Instruction may not have arrived" });
  });
});

describe("supervisor entry persistence", () => {
  it("round-trips a supervisor entry with its payload", () => {
    const store = create({ storage: localStorage });
    syncSupervisorNotifications(snapshot(lane(goal({ stopReason: "judge_unavailable" }))), resolvePane, store, 500);
    store.flush();
    const restored = create({ storage: localStorage });
    expect(restored.getSnapshot()).toEqual(store.getSnapshot());
    expect(syncSupervisorNotifications(snapshot(lane(goal({ stopReason: "judge_unavailable" }))), resolvePane, restored, 900)).toBe(0);
  });

  it("drops unknown reasons and corrupted supervisor payloads without crashing", () => {
    const base = {
      workspaceId: "ws", labels: {}, subject: "agent", firstOccurredAt: 1, lastOccurredAt: 1,
      revision: 1, occurrenceCount: 1, read: { unread: true },
    };
    const row = (sessionId: string, updateOrder: number, extra: Record<string, unknown>) => ({
      ...base, id: notificationEntryId("ws", sessionId), sessionId, updateOrder, ...extra,
    });
    const raw = JSON.stringify({ version: 1, nextUpdateOrder: 1, entries: [
      row("unknown", 1, { reason: "escalated" }),
      row("missing", 2, { reason: "supervisor" }),
      row("bad-lane", 3, { reason: "supervisor", supervisor: { laneKey: 7, stopReason: null, awaitingApproval: true } }),
      row("bad-stop", 4, { reason: "supervisor", supervisor: { laneKey: "l", stopReason: 5, awaitingApproval: false } }),
      row("not-object", 5, { reason: "supervisor", supervisor: "turn_limit" }),
      row("ok", 6, { reason: "supervisor", supervisor: { laneKey: "l", stopReason: "turn_limit", awaitingApproval: false, goalRevision: 1 } }),
      row("plain", 7, { reason: "waiting" }),
    ] });
    let restored: ReturnType<typeof restoreNotificationInbox> | undefined;
    expect(() => { restored = restoreNotificationInbox(raw); }).not.toThrow();
    expect(restored!.entries.map((entry) => entry.sessionId)).toEqual(["plain", "ok"]);
    expect(restored!.entries.find((entry) => entry.sessionId === "plain")!.supervisor).toBeUndefined();

    localStorage.setItem(KEY, raw);
    const store = create({ storage: localStorage });
    expect(store.getSnapshot().entries).toHaveLength(2);
  });
});
