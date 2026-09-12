import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActivityNotificationEvent, WorkspaceState } from "../../state/workspaceStore";
import { NotificationCoordinator, type NotificationDecision } from "../notificationCoordinator";
import { DEFAULT_NOTIFICATION_SETTINGS } from "../notificationSettings";
import { createNotificationCenterStore, type NotificationCenterStore } from "./notificationCenterStore";
import { isNotificationTargetObserved, wireActivityRecording, wireBellRecording, type RecordingListener, type RecordingTarget } from "./activityRecording";

vi.mock("../tauri", () => ({
  dispatchNotification: vi.fn().mockResolvedValue({ submitted: true }),
  playNotificationSound: vi.fn().mockResolvedValue({ played: true }),
}));
const { dispatchNotification } = await import("../tauri");
const event: ActivityNotificationEvent = {
  workspaceId: "project", workspaceLabel: "Orca (Build machine)", sessionId: "session", tabId: "tab",
  worktreePath: "/repo/main", worktreeLabel: "main", agentLabel: "Codex", terminalTitle: "Task",
  previousState: "working", state: "done",
};
function bus<T>() {
  const listeners = new Set<RecordingListener<T>>();
  return {
    events: (listener: RecordingListener<T>) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    emit: (target: Parameters<RecordingListener<T>>[0], decision: NotificationDecision) => { listeners.forEach((listener) => listener(target, decision)); },
  };
}
let store: NotificationCenterStore;
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(10_000);
  vi.mocked(dispatchNotification).mockClear();
  store = createNotificationCenterStore({ storage: null });
});
afterEach(() => { store.dispose(); vi.useRealTimers(); });

function coordinator(focused: boolean, enabled = true) {
  return new NotificationCoordinator({ isWindowFocused: () => focused,
    getSettings: () => ({ ...DEFAULT_NOTIFICATION_SETTINGS, enabled, terminalBell: true }),
  });
}

describe("accepted-event recording adapters", () => {
  it.each([true, false])("records before focus filtering (observed=%s), preserving labels and dispatch", (focused) => {
    const events = bus<ActivityNotificationEvent>();
    const instance = coordinator(focused);
    const unsubscribe = wireActivityRecording({ events: events.events, isObserved: () => focused, store });
    events.emit(event, instance.handleAgentStateChange({ ...event, nextState: event.state }));
    expect(store.getSnapshot().entries).toEqual([expect.objectContaining({
      reason: "done", subject: "agent", labels: { workspaceLabel: "Orca (Build machine)", worktreeLabel: "main", agentLabel: "Codex", terminalTitle: "Task" },
      read: focused ? { seen: true, seenAt: 10_000 } : { unread: true },
    })]);
    expect(dispatchNotification).toHaveBeenCalledTimes(focused ? 0 : 1);
    unsubscribe();
    events.emit({ ...event, state: "waiting" }, { accepted: true });
    expect(store.getSnapshot().entries[0].revision).toBe(1);
  });

  it("records when desktop notifications are disabled, not when acceptance is denied", () => {
    const events = bus<ActivityNotificationEvent>();
    const instance = coordinator(false, false);
    wireActivityRecording({ events: events.events, isObserved: () => false, store });
    events.emit(event, { accepted: false });
    expect(store.getSnapshot().entries).toHaveLength(0);
    events.emit(event, instance.handleAgentStateChange({ ...event, nextState: event.state }));
    expect(store.getSnapshot().entries).toHaveLength(1);
    expect(dispatchNotification).not.toHaveBeenCalled();
  });

  it("rejects suppressed baseline and duplicate edges through the real coordinator", () => {
    const events = bus<ActivityNotificationEvent>();
    const instance = coordinator(false);
    wireActivityRecording({ events: events.events, isObserved: () => false, store });
    const baseline = { ...event, notificationSuppressed: true };
    events.emit(baseline, instance.handleAgentStateChange({ ...baseline, nextState: baseline.state }));
    const duplicate = { ...event, previousState: "done" as const };
    events.emit(duplicate, instance.handleAgentStateChange({ ...duplicate, nextState: duplicate.state }));
    expect(store.getSnapshot().entries).toHaveLength(0);
    expect(dispatchNotification).not.toHaveBeenCalled();
  });

  it("records only bells accepted past throttle and completion suppression, including focused bells", () => {
    const events = bus<RecordingTarget>();
    const instance = coordinator(true);
    const unsubscribe = wireBellRecording({ events: events.events, isObserved: () => true, store });
    const bell = { ...event, agentLabel: undefined };
    events.emit(bell, instance.handleTerminalBell(bell));
    events.emit(bell, instance.handleTerminalBell(bell));
    expect(store.getSnapshot().entries).toEqual([expect.objectContaining({ reason: "bell", subject: "terminal", revision: 1, read: { seen: true, seenAt: 10_000 } })]);
    const agent = { ...event, sessionId: "agent" };
    instance.handleAgentStateChange({ ...agent, nextState: agent.state });
    events.emit(agent, instance.handleTerminalBell(agent));
    expect(store.getSnapshot().entries).toHaveLength(1);
    unsubscribe();
    events.emit({ ...bell, sessionId: "later" }, { accepted: true });
    expect(store.getSnapshot().entries).toHaveLength(1);
  });

  it("preserves source occurrence identity and samples observation at each accepted event", () => {
    const events = bus<ActivityNotificationEvent & { occurrenceId: string; occurredAt: number }>();
    let observed = true;
    wireActivityRecording({ events: events.events, isObserved: () => observed, store });
    events.emit({ ...event, occurrenceId: "first", occurredAt: 1 }, { accepted: true });
    observed = false;
    events.emit({ ...event, occurrenceId: "second", occurredAt: 2 }, { accepted: true });
    events.emit({ ...event, occurrenceId: "second", occurredAt: 3 }, { accepted: true });
    expect(store.getSnapshot().entries[0]).toMatchObject({ revision: 2, lastOccurredAt: 2, read: { unread: true } });
  });
});

describe("observation at recording time", () => {
  const state = {
    workspaceId: "project",
    layout: {
      activeTabId: "tab", tabs: [],
      layoutsByTabId: { tab: {
        root: { type: "split", id: "split", direction: "horizontal", ratio: 0.5,
          first: { type: "leaf", leafId: "one" }, second: { type: "leaf", leafId: "two" } },
        activeLeafId: "one", expandedLeafId: null, sessionIdsByLeafId: { one: "session", two: "background" },
      } },
    },
  } as unknown as WorkspaceState;
  it("requires window focus, workspace identity, visible tab, and the active split leaf", () => {
    expect(isNotificationTargetObserved(state, event, true)).toBe(true);
    expect(isNotificationTargetObserved(state, event, false)).toBe(false);
    expect(isNotificationTargetObserved(state, { ...event, workspaceId: "parked" }, true)).toBe(false);
    expect(isNotificationTargetObserved(state, { ...event, tabId: "hidden" }, true)).toBe(false);
    expect(isNotificationTargetObserved(state, { ...event, sessionId: "background" }, true)).toBe(false);
    expect(isNotificationTargetObserved(state, { ...event, sessionId: "missing" }, true)).toBe(false);
  });
  it("recognizes a visible secondary tab group", () => {
    const grouped = { ...state, layout: { ...state.layout, activeTabId: "other",
      tabGroups: { secondary: { id: "secondary", tabIds: ["tab"], activeTabId: "tab" } },
    } };
    expect(isNotificationTargetObserved(grouped, event, true)).toBe(true);
  });
});
