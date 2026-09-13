import { describe, expect, it, vi, beforeEach } from "vitest";

const onFocusChangedMock = vi.fn();
const isFocusedMock = vi.fn().mockResolvedValue(true);

function deferred<T>() {
  let resolve: (value: T) => void = () => { throw new Error("Promise executor did not run"); };
  let reject: (reason: Error) => void = () => { throw new Error("Promise executor did not run"); };
  const promise = new Promise<T>((settle, fail) => { resolve = settle; reject = fail; });
  return { promise, resolve, reject };
}

vi.mock("./tauri", () => ({
  dispatchNotification: vi.fn(() => Promise.resolve({ submitted: true })),
  playNotificationSound: vi.fn(() => Promise.resolve({ played: true })),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: isFocusedMock,
    onFocusChanged: onFocusChangedMock,
  }),
}));

describe("nativeWindowFocus", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.resetAllMocks();
    isFocusedMock.mockResolvedValue(true);
  });

  it("registers focus tracking and dispatches custom event on window focus", async () => {
    let focusCallback: ((event: { payload: boolean }) => void) | undefined;
    const subscribed = deferred<void>();
    onFocusChangedMock.mockImplementation((cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      subscribed.resolve();
      return Promise.resolve(() => {});
    });

    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");
    startNativeWindowFocusTracking();

    await subscribed.promise;

    const focusedListener = vi.fn();
    window.addEventListener("ferryx:window-focused", focusedListener);

    try {
      if (!focusCallback) throw new Error("Focus subscription was not registered");
      focusCallback({ payload: true });
      expect(getNativeWindowFocused()).toBe(true);
      expect(focusedListener).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("ferryx:window-focused", focusedListener);
    }
  });

  it("finishes subscription before querying and retains an event newer than the snapshot", async () => {
    const registration = deferred<() => void>();
    const snapshot = deferred<boolean>();
    const queryStarted = deferred<void>();
    let focusCallback: ((event: { payload: boolean }) => void) | undefined;
    onFocusChangedMock.mockImplementation((cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      return registration.promise;
    });
    isFocusedMock.mockImplementation(() => {
      queryStarted.resolve();
      return snapshot.promise;
    });
    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");

    startNativeWindowFocusTracking();
    startNativeWindowFocusTracking();

    try {
      expect(onFocusChangedMock).toHaveBeenCalledTimes(1);
      expect(isFocusedMock).not.toHaveBeenCalled();
      registration.resolve(() => {});
      await queryStarted.promise;
      if (!focusCallback) throw new Error("Focus subscription was not registered");
      focusCallback({ payload: false });
      snapshot.resolve(true);
      await snapshot.promise;
      expect(getNativeWindowFocused()).toBe(false);
      expect(isFocusedMock).toHaveBeenCalledTimes(1);
    } finally {
      registration.resolve(() => {});
      snapshot.resolve(true);
      await snapshot.promise;
    }
  });

  it.each([
    { focused: false, rejects: false },
    { focused: true, rejects: false },
    { focused: false, rejects: true },
    { focused: true, rejects: true },
  ])("uses latest focus $focused for bell decisions after snapshot rejection=$rejects", async ({ focused, rejects }) => {
    const snapshot = deferred<boolean>();
    const queryStarted = deferred<void>();
    let focusCallback: ((event: { payload: boolean }) => void) | undefined;
    onFocusChangedMock.mockImplementation((cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      return Promise.resolve(() => {});
    });
    isFocusedMock.mockImplementation(() => {
      queryStarted.resolve();
      return snapshot.promise;
    });
    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");
    const { NotificationCoordinator } = await import("./notificationCoordinator");
    const { DEFAULT_NOTIFICATION_SETTINGS } = await import("./notificationSettings");
    const { dispatchNotification, playNotificationSound } = await import("./tauri");
    const unread = vi.fn();
    const coordinator = new NotificationCoordinator({
      isWindowFocused: () => getNativeWindowFocused() ?? false,
      getSettings: () => ({ ...DEFAULT_NOTIFICATION_SETTINGS, enabled: true, terminalBell: true }),
      onMarkTabUnread: unread,
    });
    startNativeWindowFocusTracking();
    await queryStarted.promise;
    try {
      if (!focusCallback) throw new Error("Focus subscription was not registered");
      focusCallback({ payload: !focused });
      focusCallback({ payload: focused });
      if (rejects) snapshot.reject(new Error("Focus query failed"));
      else snapshot.resolve(!focused);
      await snapshot.promise.catch(() => {});
      expect(coordinator.handleTerminalBell({ sessionId: "session", tabId: "tab" })).toEqual({ accepted: true });
      expect(unread).toHaveBeenCalledTimes(focused ? 0 : 1);
      expect(dispatchNotification).toHaveBeenCalledTimes(focused ? 0 : 1);
      expect(playNotificationSound).toHaveBeenCalledTimes(focused ? 0 : 1);
      expect(getNativeWindowFocused()).toBe(focused);
    } finally {
      snapshot.resolve(false);
      await snapshot.promise.catch(() => {});
    }
  });

  it.each([true, false, null])("uses initial snapshot %s when no focus event arrives", async (focused) => {
    const snapshot = deferred<boolean>();
    const queryStarted = deferred<void>();
    onFocusChangedMock.mockResolvedValue(() => {});
    isFocusedMock.mockImplementation(() => {
      queryStarted.resolve();
      return snapshot.promise;
    });
    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");
    startNativeWindowFocusTracking();
    await queryStarted.promise;
    if (focused === null) snapshot.reject(new Error("Focus query failed"));
    else snapshot.resolve(focused);
    await snapshot.promise.catch(() => {});
    expect(getNativeWindowFocused()).toBe(focused);
  });

  it("keeps focus unknown and does not query if subscription fails", async () => {
    const registration = deferred<() => void>();
    onFocusChangedMock.mockReturnValue(registration.promise);
    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");
    startNativeWindowFocusTracking();
    expect(onFocusChangedMock).toHaveBeenCalledTimes(1);
    registration.reject(new Error("Subscription failed"));
    await registration.promise.catch(() => {});
    expect(getNativeWindowFocused()).toBeNull();
    expect(isFocusedMock).not.toHaveBeenCalled();
  });
});
