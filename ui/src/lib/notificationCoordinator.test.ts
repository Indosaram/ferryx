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
}));

const { dispatchNotification, playNotificationSound } = await import("./tauri");
const dispatchMock = vi.mocked(dispatchNotification);
const soundMock = vi.mocked(playNotificationSound);

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

describe("terminal bell", () => {
  it("marks the tab and worktree unread and notifies while unfocused", () => {
    const { instance, marks } = coordinator();

    instance.handleTerminalBell({
      sessionId: "s1",
      tabId: "t1",
      worktreePath: "/repo/wt-a",
      worktreeLabel: "wt-a",
      terminalTitle: "vim",
    });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(soundMock).toHaveBeenCalledTimes(1);
    expect(dispatchMock).toHaveBeenCalledWith({
      source: "terminal-bell",
      worktreeLabel: "wt-a",
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
  it("notifies on the running to waiting completion edge", () => {
    const { instance, marks } = coordinator({
      getSettings: () => settings({ agentTaskComplete: true }),
    });

    instance.handleAgentStateChange({
      sessionId: "s1",
      tabId: "t1",
      worktreePath: "/repo/wt-a",
      worktreeLabel: "wt-a",
      agentLabel: "codex",
      terminalTitle: "codex run",
      previousState: "running",
      nextState: "waiting",
    });

    expect(marks).toEqual({ tabs: ["t1"], worktrees: ["/repo/wt-a"] });
    expect(dispatchMock).toHaveBeenCalledWith({
      source: "agent-task-complete",
      worktreeLabel: "wt-a",
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

  it("ignores a repeat completion when the agent was already waiting", () => {
    const { instance } = coordinator({ getSettings: () => settings({ agentTaskComplete: true }) });

    instance.handleAgentStateChange({ sessionId: "s1", previousState: "waiting", nextState: "done" });

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
    dispatchMock.mockResolvedValueOnce({ submitted: false, reason: "permission-required" });
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
    expect(String(errorSpy.mock.calls[0]?.[0])).toContain("permission-required");
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
});
