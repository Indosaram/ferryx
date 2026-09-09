import { describe, expect, it, vi, beforeEach } from "vitest";

const onFocusChangedMock = vi.fn();
const isFocusedMock = vi.fn().mockResolvedValue(true);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: isFocusedMock,
    onFocusChanged: onFocusChangedMock,
  }),
}));

describe("nativeWindowFocus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("registers focus tracking and dispatches custom event on window focus", async () => {
    let focusCallback: ((event: { payload: boolean }) => void) | undefined;
    onFocusChangedMock.mockImplementation((cb: (event: { payload: boolean }) => void) => {
      focusCallback = cb;
      return Promise.resolve(() => {});
    });

    const { startNativeWindowFocusTracking, getNativeWindowFocused } = await import("./nativeWindowFocus");
    startNativeWindowFocusTracking();

    await vi.waitFor(() => {
      expect(isFocusedMock).toHaveBeenCalled();
    });

    const focusedListener = vi.fn();
    window.addEventListener("ferryx:window-focused", focusedListener);

    expect(focusCallback).toBeDefined();
    focusCallback!({ payload: true });

    expect(getNativeWindowFocused()).toBe(true);
    expect(focusedListener).toHaveBeenCalledTimes(1);

    window.removeEventListener("ferryx:window-focused", focusedListener);
  });
});
