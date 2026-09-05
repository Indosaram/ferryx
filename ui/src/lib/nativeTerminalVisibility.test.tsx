import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  NativeTerminalVisibilityProvider,
  useNativeTerminalVisibility,
} from "./nativeTerminalVisibility";

/**
 * These tests exercise the hook against real MutationObserver delivery. jsdom
 * delivers observer callbacks on the microtask queue, so wrapping each DOM
 * mutation in `await act(async () => ...)` flushes both the observer callback
 * and the resulting React state update deterministically - no fixed sleeps,
 * polling, or waitFor. The hook installs its observer synchronously during
 * renderHook, so the subscription exists before any mutation is emitted.
 */
describe("useNativeTerminalVisibility", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("returns true when no dialog is mounted", () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);
  });

  it("yields surface (returns false) when a standard modal dialog mounts", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);

    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    await act(async () => {
      document.body.appendChild(dialog);
    });
    expect(result.current).toBe(false);

    await act(async () => {
      document.body.removeChild(dialog);
    });
    expect(result.current).toBe(true);
  });

  it("does not yield surface (returns true) when dialog opts out on itself", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);

    const popover = document.createElement("div");
    popover.setAttribute("role", "dialog");
    popover.setAttribute("data-native-terminal-yield", "off");
    await act(async () => {
      document.body.appendChild(popover);
    });
    expect(result.current).toBe(true);

    await act(async () => {
      document.body.removeChild(popover);
    });
    expect(result.current).toBe(true);
  });

  it("does not yield surface when dialog descends from an opted-out ancestor", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);

    const container = document.createElement("div");
    container.setAttribute("data-native-terminal-yield", "off");
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    container.appendChild(dialog);

    await act(async () => {
      document.body.appendChild(container);
    });
    expect(result.current).toBe(true);
  });

  it("still yields for a real modal even when another opted-out dialog is open", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());

    const optedOut = document.createElement("div");
    optedOut.setAttribute("role", "dialog");
    optedOut.setAttribute("data-native-terminal-yield", "off");
    await act(async () => {
      document.body.appendChild(optedOut);
    });
    expect(result.current).toBe(true);

    const realModal = document.createElement("div");
    realModal.setAttribute("role", "dialog");
    await act(async () => {
      document.body.appendChild(realModal);
    });
    expect(result.current).toBe(false);
  });

  it("yields for a search surface that has not opted out", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());

    const search = document.createElement("div");
    search.setAttribute("role", "search");
    await act(async () => {
      document.body.appendChild(search);
    });
    expect(result.current).toBe(false);
  });

  it("keeps provider visible=false winning regardless of surface state", async () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <NativeTerminalVisibilityProvider visible={false}>
        {children}
      </NativeTerminalVisibilityProvider>
    );
    const { result } = renderHook(() => useNativeTerminalVisibility(), {
      wrapper,
    });
    // No surface mounted, yet the owner override forces invisible.
    expect(result.current).toBe(false);

    const optedOut = document.createElement("div");
    optedOut.setAttribute("role", "dialog");
    optedOut.setAttribute("data-native-terminal-yield", "off");
    await act(async () => {
      document.body.appendChild(optedOut);
    });
    expect(result.current).toBe(false);

    const realModal = document.createElement("div");
    realModal.setAttribute("role", "dialog");
    await act(async () => {
      document.body.appendChild(realModal);
    });
    expect(result.current).toBe(false);
  });

  it("disconnects its observer on unmount, leaving nothing subscribed", () => {
    const observeSpy = vi.spyOn(MutationObserver.prototype, "observe");
    const disconnectSpy = vi.spyOn(MutationObserver.prototype, "disconnect");

    const { unmount } = renderHook(() => useNativeTerminalVisibility());
    expect(observeSpy).toHaveBeenCalledTimes(1);
    expect(disconnectSpy).not.toHaveBeenCalled();

    unmount();
    expect(disconnectSpy).toHaveBeenCalledTimes(1);

    observeSpy.mockRestore();
    disconnectSpy.mockRestore();
  });
});
