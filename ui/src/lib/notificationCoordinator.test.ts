import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  NotificationCoordinator,
  isWindowForegroundFocused,
  type NotificationCoordinatorOptions,
} from "./notificationCoordinator";
import { DEFAULT_NOTIFICATION_SETTINGS } from "./notificationSettings";
import type { NotificationSettings } from "./notificationSettings";

vi.mock("./tauri", () => ({
  dispatchNotification: vi.fn(() => Promise.resolve()),
  playNotificationSound: vi.fn(() => Promise.resolve()),
  requestNotificationPermission: vi.fn(() => Promise.resolve({ granted: false })),
  getNotificationPermissionStatus: vi.fn(() => Promise.resolve({ authorization: "denied" })),
}));

const {
  dispatchNotification,
  playNotificationSound,
  requestNotificationPermission,
  getNotificationPermissionStatus,
} = await import("./tauri");
const dispatchMock = vi.mocked(dispatchNotification);
const soundMock = vi.mocked(playNotificationSound);
const requestMock = vi.mocked(requestNotificationPermission);
const statusMock = vi.mocked(getNotificationPermissionStatus);

function settings(overrides: Partial<NotificationSettings> = {}): NotificationSettings {
  return { ...DEFAULT_NOTIFICATION_SETTINGS, ...overrides };
}

function coordinator(options: NotificationCoordinatorOptions = {}) {
  const marks = { tabs: [] as string[], worktrees: [] as string[] };
  const instance = new NotificationCoordinator({
    isWindowFocused: () => false,
    getSettings: () => settings({ terminalBell: true }),
    onMarkTabUnread: (tabId) => marks.tabs.push(tabId),
    onMarkWorktreeUnread: (path) => marks.worktrees.push(path),
    ...options,
  });
  return { instance, marks };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(1_000_000));
  dispatchMock.mockClear();
  soundMock.mockClear();
  requestMock.mockClear();
  statusMock.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("isWindowForegroundFocused", () => {
  it("is true when the document is visible and holds focus", () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    expect(isWindowForegroundFocused()).toBe(true);
  });

  it("is false when the document is hidden", () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    expect(isWindowForegroundFocused()).toBe(false);
  });

  it("is false when the visible document has lost focus", () => {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    expect(isWindowForegroundFocused()).toBe(false);
  });
});

describe("pre-focus acceptance decisions", () => {
  it.each([true, false])("accepts attention edges regardless of focus (%s)", (focused) => {
    const { instance } = coordinator({ isWindowFocused: () => focused });
    expect(instance.handleAgentStateChange({ sessionId: "s", previousState: "working", nextState: "done" }))
      .toEqual({ accepted: true });
    expect(instance.handleAgentStateChange({ sessionId: "s", nextState: "done" }))
      .toEqual({ accepted: false });
    expect(instance.handleAgentStateChange({ sessionId: "s", nextState: "waiting", notificationSuppressed: true }))
      .toEqual({ accepted: false, suppressed: true });
    expect(instance.handleAgentStateChange({ sessionId: "baseline", nextState: "done" }))
      .toEqual({ accepted: false });
  });

  it("reports accepted, throttled, and post-completion-suppressed bells before focus", () => {
    const { instance } = coordinator({ isWindowFocused: () => true });
    expect(instance.handleTerminalBell({ sessionId: "bell" })).toEqual({ accepted: true });
    expect(instance.handleTerminalBell({ sessionId: "bell" })).toEqual({ accepted: false });
    instance.handleAgentStateChange({ sessionId: "agent", previousState: "working", nextState: "done" });
    expect(instance.handleTerminalBell({ sessionId: "agent" })).toEqual({ accepted: false, suppressed: true });
    expect(dispatchMock).not.toHaveBeenCalled();
  });
});

describe("terminal bell", () => {
  it("marks the tab and worktree unread and notifies while unfocused", () => {
    const { instance, marks } = coordinator();

    instance.handleTerminalBell({
      sessionId: "s1",
      tabId: "t1",
      worktreePath: "/repo/wt-a",
      worktreeLabel: "wt-a",
      workspaceLabel: "Orca (Build machine)",
      terminalTitle: "vim",
    });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(soundMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledWith({
      source: "terminal-bell",
      sound: "system",
      worktreeLabel: "wt-a",
      workspaceLabel: "Orca (Build machine)",
      terminalTitle: "vim",
    });
  });

  it("stays silent and leaves nothing unread while the window is focused", () => {
    const { instance, marks } = coordinator({ isWindowFocused: () => true });

    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1", worktreePath: "/repo/wt-a" });

    expect(marks).toEqual({ tabs: [], worktrees: [] });
    expect(dispatchMock).not.toHaveBeenCalled();
    expect(soundMock).not.toHaveBeenCalled();
  });

  it("throttles a second bell from the same session inside one second", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });
    vi.advanceTimersByTime(999);
    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("allows the next bell once the throttle window has passed", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });
    vi.advanceTimersByTime(1000);
    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });

    expect(dispatchMock).toHaveBeenCalledTimes(2);
  });

  it("throttles per session so a different session still rings", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });
    instance.handleTerminalBell({ sessionId: "s2", tabId: "t2" });

    expect(dispatchMock).toHaveBeenCalledTimes(2);
  });

  it("suppresses a bell that trails an agent completion on the same session", () => {
    const { instance } = coordinator({
      getSettings: () => settings({ terminalBell: true, agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "running", nextState: "done" });
    dispatchMock.mockClear();
    vi.advanceTimersByTime(1499);
    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });

    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("rings again once the agent suppression window has elapsed", () => {
    const { instance } = coordinator({
      getSettings: () => settings({ terminalBell: true, agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "running", nextState: "done" });
    dispatchMock.mockClear();
    vi.advanceTimersByTime(1500);
    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("accepts an agent completion edge trailing a bell and suppresses duplicate notification", () => {
    const { instance, marks } = coordinator({
      getSettings: () => settings({ terminalBell: true, agentTaskComplete: true }),
    });

    instance.handleTerminalBell({ sessionId: "s1", tabId: "t1" });
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(marks.tabs).toEqual(["t1"]);

    vi.advanceTimersByTime(200);
    const decision = instance.handleAgentStateChange({
      sessionId: "s1",
      tabId: "t1",
      previousState: "working",
      nextState: "waiting",
    });

    expect(decision).toEqual({ accepted: true, suppressed: true });
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(marks.tabs).toEqual(["t1"]);
  });

  it("still marks unread when bell notifications are turned off", () => {
    const { instance, marks } = coordinator({
      getSettings: () => settings({ terminalBell: false }),
    });

    instance.handleTerminalBell({ tabId: "t1", worktreePath: "/repo/wt-a" });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("sends nothing when notifications are disabled entirely", () => {
    const { instance } = coordinator({
      getSettings: () => settings({ enabled: false, terminalBell: true }),
    });

    instance.handleTerminalBell({ sessionId: "s1" });

    expect(dispatchMock).not.toHaveBeenCalled();
    expect(soundMock).not.toHaveBeenCalled();
  });

  it("prefers per-event settings over the coordinator defaults", () => {
    const { instance } = coordinator({ getSettings: () => settings({ terminalBell: false }) });

    instance.handleTerminalBell({ sessionId: "s1", settings: settings({ terminalBell: true }) });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("falls back to the worktree id when no path or label is supplied", () => {
    const { instance, marks } = coordinator();

    instance.handleTerminalBell({ sessionId: "s1", worktreeId: "wt-id-9" });

    expect(marks.worktrees).toEqual(["wt-id-9"]);
    expect(dispatchMock.mock.calls[0][0]).toMatchObject({ worktreeLabel: "wt-id-9" });
  });

  it("plays the configured custom sound", () => {
    const { instance } = coordinator({
      getSettings: () =>
        settings({
          terminalBell: true,
          customSoundId: "chime",
          customSoundPath: "/sounds/chime.wav",
          customSoundVolume: 0.25,
        }),
    });

    instance.handleTerminalBell({ sessionId: "s1" });

    expect(soundMock).toHaveBeenCalledWith({
      soundId: "chime",
      customSoundPath: "/sounds/chime.wav",
      volume: 0.25,
    });
  });
});

describe("agent state change", () => {
  it("sends distinct waiting and completion reasons with an explicit native sound mode", () => {
    const { instance } = coordinator();
    instance.handleAgentStateChange({ sessionId: "waiting", previousState: "working", nextState: "waiting" });
    instance.handleAgentStateChange({ sessionId: "done", previousState: "working", nextState: "done", settings: settings({ customSoundId: "none", customSoundPath: "/old.wav" }) });
    expect(dispatchMock.mock.calls.map(([req]) => [req.attentionReason, req.sound])).toEqual([
      ["waiting", "system"], ["done", "silent"],
    ]);
  });

  it("does not report expected custom sound deduplication as a playback error", async () => {
    const onError = vi.fn();
    soundMock.mockResolvedValueOnce({ played: false, reason: "deduped" });
    const { instance } = coordinator({ onError, getSettings: () => settings({ customSoundId: "custom", customSoundPath: "/bell.wav" }) });
    instance.handleAgentStateChange({ sessionId: "dedupe", previousState: "working", nextState: "done" });
    await soundMock.mock.results[0].value;
    expect(onError).not.toHaveBeenCalled();
  });

  it("consumes restore suppression without sound, unread marks, or a delayed alert", () => {
    const { instance, marks } = coordinator();
    instance.handleAgentStateChange({ sessionId: "restore", nextState: "working" });
    instance.handleAgentStateChange({ sessionId: "restore", tabId: "tab", nextState: "done", notificationSuppressed: true });
    instance.handleAgentStateChange({ sessionId: "restore", tabId: "tab", nextState: "done" });
    expect(dispatchMock).not.toHaveBeenCalled();
    expect(soundMock).not.toHaveBeenCalled();
    expect(marks.tabs).toEqual([]);
    instance.handleAgentStateChange({ sessionId: "restore", nextState: "working" });
    instance.handleAgentStateChange({ sessionId: "restore", nextState: "done" });
    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("notifies on the running to waiting completion edge", () => {
    const { instance, marks } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      tabId: "t1",
      worktreePath: "/repo/wt-a",
      worktreeLabel: "wt-a",
      workspaceLabel: "local-project",
      agentLabel: "codex",
      terminalTitle: "codex run",
      previousState: "running",
      nextState: "waiting",
    });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(dispatchMock).toHaveBeenCalledWith({
      source: "agent-task-complete",
      attentionReason: "waiting",
      sound: "system",
      worktreeLabel: "wt-a",
      workspaceLabel: "local-project",
      terminalTitle: "codex run",
      agentLabel: "codex",
    });
  });

  it("accepts newState as an alias for nextState", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "running", newState: "done" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("ignores a transition that is not a completion edge", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "idle", nextState: "running" });

    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("notifies when a blocked agent finishes (waiting to done is fresh attention)", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "waiting", nextState: "done" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock.mock.calls[0][0]).toMatchObject({ attentionReason: "done" });
  });

  it("notifies when a finished agent needs input again (done to waiting is fresh attention)", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "done", nextState: "waiting" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock.mock.calls[0][0]).toMatchObject({ attentionReason: "waiting" });
  });

  it("dedupes a repeated identical attention state", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "working", nextState: "done" });
    dispatchMock.mockClear();
    instance.handleAgentStateChange({ sessionId: "s1", previousState: "done", nextState: "done" });

    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("ignores the first observed state when no previous state is known", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", nextState: "done" });

    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("derives the previous state from the last observed transition", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", nextState: "running" });
    instance.handleAgentStateChange({ sessionId: "s1", nextState: "done" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
  });

  it("stays silent while the window is focused", () => {
    const { instance, marks } = coordinator({
      isWindowFocused: () => true,
      getSettings: () => settings({ agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      tabId: "t1",
      previousState: "running",
      nextState: "done",
    });

    expect(marks).toEqual({ tabs: [], worktrees: [] });
    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("still marks unread when completion notifications are turned off", () => {
    const { instance, marks } = coordinator({
      getSettings: () => settings({ agentTaskComplete: false }),
    });

    instance.handleAgentStateChange({
      tabId: "t1",
      worktreePath: "/repo/wt-a",
      previousState: "running",
      nextState: "done",
    });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(dispatchMock).not.toHaveBeenCalled();
  });
});

describe("notification navigation target", () => {
  it("attaches a click target with the frontend workspace and session ids on a bell", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ workspaceId: "ws-1", sessionId: "sess-1", tabId: "t1" });

    expect(dispatchMock.mock.calls[0][0]).toMatchObject({
      target: { workspaceId: "ws-1", sessionId: "sess-1" },
    });
  });

  it("attaches a click target on an agent completion", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({
      workspaceId: "ws-2",
      sessionId: "sess-2",
      tabId: "t2",
      previousState: "running",
      nextState: "done",
    });

    expect(dispatchMock.mock.calls[0][0]).toMatchObject({
      target: { workspaceId: "ws-2", sessionId: "sess-2" },
    });
  });

  it("still dispatches but omits the target when the workspace id is missing", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "sess-3", previousState: "running", nextState: "done" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock.mock.calls[0][0]).not.toHaveProperty("target");
  });

  it("still dispatches but omits the target when the session id is missing (probe)", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ workspaceId: "ws-1", tabId: "t1" });

    expect(dispatchMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock.mock.calls[0][0]).not.toHaveProperty("target");
  });
});

describe("reset", () => {
  it("clears the bell throttle so the next bell is delivered immediately", () => {
    const { instance } = coordinator();

    instance.handleTerminalBell({ sessionId: "s1" });
    instance.reset();
    instance.handleTerminalBell({ sessionId: "s1" });

    expect(dispatchMock).toHaveBeenCalledTimes(2);
  });

  it("clears remembered agent state so the next edge needs a fresh baseline", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", nextState: "running" });
    instance.reset();
    instance.handleAgentStateChange({ sessionId: "s1", nextState: "done" });

    expect(dispatchMock).not.toHaveBeenCalled();
  });

  it("reports dispatch or sound failures through onError callback", async () => {
    const errorSpy = vi.fn();
    dispatchMock.mockRejectedValueOnce(new Error("IPC failed"));
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onError: errorSpy,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    await Promise.resolve();
    await Promise.resolve();

    expect(errorSpy).toHaveBeenCalledWith(expect.any(Error), "dispatch");
  });

  it("surfaces a rejected dispatch result (submitted:false) with its reason through onError", async () => {
    const errorSpy = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "backend-error" });
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onError: errorSpy,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(errorSpy).toHaveBeenCalledWith(expect.any(Error), "dispatch");
    expect(String(errorSpy.mock.calls[0]?.[0])).toContain("backend-error");
  });

  it("never requests notification permission before the first notify call", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });
    expect(requestMock).not.toHaveBeenCalled();
    expect(statusMock).not.toHaveBeenCalled();
    instance.dispose();
  });

  it("requests notification permission on the first notify call that needs it", async () => {
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" })
      .mockResolvedValueOnce({ submitted: true });
    requestMock.mockResolvedValueOnce({ granted: true });
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "running", nextState: "done" });
    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(dispatchMock).toHaveBeenCalledTimes(2);
    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(requestMock.mock.invocationCallOrder[0]).toBeGreaterThan(dispatchMock.mock.invocationCallOrder[0]!);
    instance.dispose();
  });

  it("requests permission once and retries dispatch instead of reporting a runtime error", async () => {
    const errorSpy = vi.fn();
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" })
      .mockResolvedValueOnce({ submitted: true });
    requestMock.mockResolvedValueOnce({ granted: true });
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onError: errorSpy,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(errorSpy).not.toHaveBeenCalled();
    expect(dispatchMock).toHaveBeenCalledTimes(2);
  });

  it("coalesces an in-flight permission request and does not toast a denial", async () => {
    const errorSpy = vi.fn();
    let resolveRequest: (value: { granted: boolean }) => void = () => {};
    requestMock.mockImplementation(
      () => new Promise((resolve) => {
        resolveRequest = resolve;
      }),
    );
    dispatchMock.mockResolvedValue({ submitted: false, reason: "permission-required" });
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true, terminalBell: true }),
      onError: errorSpy,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });
    instance.handleTerminalBell({ sessionId: "s2", tabId: "t2" });

    await Promise.resolve();
    await Promise.resolve();

    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(errorSpy).not.toHaveBeenCalled();

    resolveRequest({ granted: false });
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(errorSpy).not.toHaveBeenCalled();
  });

  it("does not treat the default system sound's played:false as an error", async () => {
    const errorSpy = vi.fn();
    soundMock.mockResolvedValueOnce({ played: false });
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onError: errorSpy,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    expect(errorSpy).not.toHaveBeenCalled();
  });

  it("recovers when the user grants permission after an earlier denial", async () => {
    // 1. Initial attempt: permission-required, request returns granted: false.
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" });
    requestMock.mockResolvedValueOnce({ granted: false });
    statusMock.mockResolvedValue({ authorization: "denied" });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledTimes(1);

    // 2. Second attempt while still denied: ensures notification is not retried / requested
    dispatchMock.mockClear();
    requestMock.mockClear();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "permission-required" });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "working",
      nextState: "waiting",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();
    // Dispatch attempted, but ensureNotificationPermission returned false immediately (via recheck or denied latch)
    expect(requestMock).not.toHaveBeenCalled();
    expect(dispatchMock).toHaveBeenCalledTimes(1);

    // 3. User grants permission in OS Settings, window regains focus.
    statusMock.mockResolvedValue({ authorization: "authorized" });
    window.dispatchEvent(new Event("focus"));
    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    // 4. Subsequent notification request should now succeed and deliver!
    dispatchMock.mockClear();
    requestMock.mockClear();
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" })
      .mockResolvedValueOnce({ submitted: true });
    requestMock.mockResolvedValueOnce({ granted: true });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "waiting",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    // Should have checked permission / requested and retried dispatch
    expect(dispatchMock).toHaveBeenCalledTimes(2);
    expect(dispatchMock.mock.calls[1][0]).toMatchObject({
      attentionReason: "done",
    });

    instance.dispose();
  });

  it("recovers at dispatch time when the OS grants permission and no focus event fires", async () => {
    // The focus listener is one recovery path; this covers the other one, where the user grants
    // permission in OS settings while the window never regains focus. The denial latch must be
    // re-checked against the authoritative status at dispatch time, not trusted forever.
    statusMock.mockResolvedValue({ authorization: "denied" });
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "permission-required" });
    requestMock.mockResolvedValueOnce({ granted: false });
    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "running", nextState: "done" });
    for (let i = 0; i < 8; i += 1) await Promise.resolve();
    expect(requestMock).toHaveBeenCalledTimes(1);

    // The OS now reports granted. No focus event is dispatched.
    statusMock.mockResolvedValue({ authorization: "authorized" });
    dispatchMock.mockClear();
    requestMock.mockClear();
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" })
      .mockResolvedValueOnce({ submitted: true });
    requestMock.mockResolvedValueOnce({ granted: true });

    instance.handleAgentStateChange({ sessionId: "s2", previousState: "running", nextState: "done" });
    for (let i = 0; i < 40; i += 1) await Promise.resolve();

    expect(statusMock).toHaveBeenCalled();
    expect(requestMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledTimes(2);

    instance.dispose();
  });
});

describe("onPermissionUnavailable callback", () => {
  it("calls onPermissionUnavailable when permission is required and request is denied", async () => {
    const onPermissionUnavailable = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "permission-required" });
    requestMock.mockResolvedValueOnce({ granted: false });
    statusMock.mockResolvedValueOnce({ authorization: "denied" });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onPermissionUnavailable,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    instance.dispose();
  });

  it("calls onPermissionUnavailable when initial dispatch is blocked-by-system", async () => {
    const onPermissionUnavailable = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "blocked-by-system" });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onPermissionUnavailable,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledTimes(1);
    instance.dispose();
  });

  it("does not call onPermissionUnavailable when dispatch succeeds", async () => {
    const onPermissionUnavailable = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: true });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onPermissionUnavailable,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).not.toHaveBeenCalled();
    instance.dispose();
  });

  it("does not call onPermissionUnavailable when permission is granted and retry succeeds", async () => {
    const onPermissionUnavailable = vi.fn();
    dispatchMock
      .mockResolvedValueOnce({ submitted: false, reason: "permission-required" })
      .mockResolvedValueOnce({ submitted: true });
    requestMock.mockResolvedValueOnce({ granted: true });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onPermissionUnavailable,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).not.toHaveBeenCalled();
    expect(dispatchMock).toHaveBeenCalledTimes(2);
    instance.dispose();
  });

  it("does not call onPermissionUnavailable for general unhandled backend errors", async () => {
    const onPermissionUnavailable = vi.fn();
    const onError = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "backend-crash" });

    const { instance } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
      onPermissionUnavailable,
      onError,
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      previousState: "running",
      nextState: "done",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledTimes(1);
    instance.dispose();
  });

  it("calls onPermissionUnavailable on terminal bell when permission is unavailable", async () => {
    const onPermissionUnavailable = vi.fn();
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "permission-required" });
    requestMock.mockResolvedValueOnce({ granted: false });
    statusMock.mockResolvedValueOnce({ authorization: "denied" });

    const { instance } = coordinator({
      getSettings: () => settings({ terminalBell: true }),
      onPermissionUnavailable,
    });

    instance.handleTerminalBell({
      sessionId: "s1",
      tabId: "t1",
    });

    for (let i = 0; i < 8; i += 1) await Promise.resolve();

    expect(onPermissionUnavailable).toHaveBeenCalledTimes(1);
    instance.dispose();
  });
});

