import { act, cleanup, fireEvent, renderHook } from "@testing-library/react";
import { useEffect } from "react";
import { Toaster, toast } from "sonner";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isMacShortcutPlatform } from "./shortcuts";

vi.mock("./shortcuts", () => ({
  isMacShortcutPlatform: vi.fn(() => false),
}));

import {
  NativeTerminalVisibilityProvider,
  useNativeTerminalVisibility,
  useNativeTerminalVisibilityState,
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
  beforeEach(() => {
    vi.mocked(isMacShortcutPlatform).mockReturnValue(false);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it.each([
    { platform: "non-Mac", mac: false, ownerVisible: true, toastVisible: true, interactive: true },
    { platform: "Mac", mac: true, ownerVisible: true, toastVisible: true, interactive: true },
    { platform: "hidden owner", mac: false, ownerVisible: false, toastVisible: false, interactive: false },
  ])("keeps terminal interactive and visible when an error toast is displayed ($platform)", async ({ mac, ownerVisible, toastVisible, interactive }) => {
    // Given: an empty real Toaster and an already-subscribed visibility owner.
    vi.mocked(isMacShortcutPlatform).mockReturnValue(mac);
    vi.useFakeTimers(); // Sonner's deferred mount and exit-animation scheduler.
    const released = vi.fn();
    const toastId = `native-visibility-${mac}-${ownerVisible}`;
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <NativeTerminalVisibilityProvider visible={ownerVisible}>
        {children}
        <Toaster theme="light" position="bottom-right" closeButton />
      </NativeTerminalVisibilityProvider>
    );
    const { result, unmount } = renderHook(() => {
      useEffect(() => released, []);
      return useNativeTerminalVisibilityState();
    }, { wrapper });
    try {
      expect(document.querySelector('section[aria-live="polite"]')).not.toBeNull();
      expect(result.current).toEqual({ visible: ownerVisible, interactive: ownerVisible });
      await act(async () => {
        toast.error("Persistent terminal error", { id: toastId, duration: Infinity });
        await vi.runAllTimersAsync();
      });
      expect(document.querySelector("[data-sonner-toast]")).not.toBeNull();
      // Toasts must NEVER block terminal typing or hide the surface.
      expect(result.current).toEqual({ visible: toastVisible, interactive });
      expect(released).not.toHaveBeenCalled();
      const closeButton = document.querySelector<HTMLButtonElement>("[data-sonner-toast] [data-close-button]");
      if (!closeButton) throw new Error("Real Sonner close button was not mounted");

      // When: dismiss through the real button, completing Sonner's exit animation.
      await act(async () => {
        fireEvent.click(closeButton);
        await vi.runAllTimersAsync();
      });

      // Then: the empty toaster stays mounted and the same owner is restored.
      expect(document.querySelector("[data-sonner-toast]")).toBeNull();
      expect(document.querySelector('section[aria-live="polite"]')).not.toBeNull();
      expect(result.current).toEqual({ visible: ownerVisible, interactive: ownerVisible });
      expect(released).not.toHaveBeenCalled();
      unmount();
      expect(released).toHaveBeenCalledTimes(1);
    } finally {
      // Own only this toast; drain its removal work even when the RED assertion fails.
      await act(async () => {
        toast.dismiss(toastId);
        await vi.runAllTimersAsync();
      });
      cleanup();
      await vi.runAllTimersAsync();
      expect(vi.getTimerCount()).toBe(0);
      vi.useRealTimers();
    }
  });

  it("returns true when no dialog is mounted", () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);
  });

  it.each(["dialog", "search"])("keeps macOS terminal visible behind a %s overlay", async (role) => {
    vi.mocked(isMacShortcutPlatform).mockReturnValue(true);
    const { result } = renderHook(() => useNativeTerminalVisibilityState());
    const overlay = document.createElement("div");
    overlay.setAttribute("role", role);

    await act(async () => {
      document.body.appendChild(overlay);
    });

    expect(result.current).toEqual({ visible: true, interactive: false });
  });

  it.each(["dialog", "search"])("masks the macOS browser surface while a %s overlay owns input", async (role) => {
    vi.mocked(isMacShortcutPlatform).mockReturnValue(true);
    const { result } = renderHook(() => useNativeTerminalVisibility());
    const overlay = document.createElement("div");
    overlay.setAttribute("role", role);
    await act(async () => {
      document.body.appendChild(overlay);
    });
    expect(result.current).toBe(false);
    await act(async () => {
      overlay.remove();
    });
    expect(result.current).toBe(true);
  });

  it("masks the macOS browser surface during owner drop occlusion", () => {
    vi.mocked(isMacShortcutPlatform).mockReturnValue(true);
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <NativeTerminalVisibilityProvider visible occluded>{children}</NativeTerminalVisibilityProvider>
    );
    const { result } = renderHook(() => useNativeTerminalVisibility(), { wrapper });
    expect(result.current).toBe(false);
  });

  it("still hides an explicitly hidden macOS owner", () => {
    vi.mocked(isMacShortcutPlatform).mockReturnValue(true);
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <NativeTerminalVisibilityProvider visible={false}>
        {children}
      </NativeTerminalVisibilityProvider>
    );
    const { result } = renderHook(() => useNativeTerminalVisibility(), { wrapper });
    expect(result.current).toBe(false);
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
