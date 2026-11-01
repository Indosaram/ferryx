import { act, cleanup, fireEvent, render, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";

import {
  APPEARANCE_SETTINGS_EVENT,
  saveAppearanceSettings,
} from "../../lib/appearanceSettings";
import { Toaster, useToastTheme } from "./sonner";

describe("sonner Toaster and useToastTheme", () => {
  afterEach(() => {
    cleanup();
    localStorage.clear();
    act(() => {
      toast.dismiss();
    });
  });

  it("maps appearance themes to sonner themes", () => {
    saveAppearanceSettings({ theme: "charcoal" });
    const { result } = renderHook(() => useToastTheme());
    expect(result.current).toBe("dark");

    act(() => {
      saveAppearanceSettings({ theme: "light" });
    });
    expect(result.current).toBe("light");

    act(() => {
      saveAppearanceSettings({ theme: "dark" });
    });
    expect(result.current).toBe("dark");

    act(() => {
      saveAppearanceSettings({ theme: "system" });
    });
    expect(result.current).toBe("system");
  });

  it("updates theme on APPEARANCE_SETTINGS_EVENT custom event", () => {
    const { result } = renderHook(() => useToastTheme());

    act(() => {
      window.dispatchEvent(
        new CustomEvent(APPEARANCE_SETTINGS_EVENT, {
          detail: { theme: "light", accentColor: "default", density: "compact" },
        }),
      );
    });

    expect(result.current).toBe("light");
  });

  it("renders Toaster without crashing", () => {
    const { container } = render(<Toaster />);
    expect(container).toBeDefined();
  });

  it("hides the clear-all control when no toasts are visible", () => {
    const { container } = render(<Toaster />);
    expect(container.querySelector("[data-testid='toast-clear-all']")).toBeNull();
  });

  it("reserves bottom offset for the clear-all row from the very first render in which a toast is shown", async () => {
    render(<Toaster />);

    let firstOffset: string | null = null;
    const observer = new MutationObserver(() => {
      const ol = document.querySelector<HTMLElement>("[data-sonner-toaster]");
      if (ol && firstOffset === null) {
        firstOffset = ol.style.getPropertyValue("--offset-bottom");
        observer.disconnect();
      }
    });
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["style"],
    });
    try {
      act(() => {
        toast("link actions ready");
      });
      await waitFor(() => expect(firstOffset).not.toBeNull());
      expect(firstOffset).toBe("60px");
    } finally {
      observer.disconnect();
    }
  });

  it("shows the clear-all control while toasts are visible and dismisses all of them on click", async () => {
    const { container } = render(<Toaster />);

    act(() => {
      toast.error("first error", { duration: Infinity });
      toast.error("second error", { duration: Infinity });
    });

    const clearAll = await waitFor(() => {
      expect(container.querySelectorAll("[data-sonner-toast]").length).toBe(2);
      const el = container.querySelector<HTMLButtonElement>("[data-testid='toast-clear-all']");
      if (!el) throw new Error("clear-all control not rendered");
      return el;
    });

    fireEvent.click(clearAll);

    await waitFor(() => {
      expect(container.querySelectorAll("[data-sonner-toast]").length).toBe(0);
      expect(container.querySelector("[data-testid='toast-clear-all']")).toBeNull();
    });
  });

  it("places toasts at the top below the safe area on a phone, clear of the bottom tab bar and composer", async () => {
    const original = window.matchMedia;
    const matchMedia = vi.fn((media: string) =>
      Object.assign(new EventTarget(), {
        media,
        matches: media === "(max-width: 600px)",
        onchange: null,
        addListener() {},
        removeListener() {},
      }),
    );
    Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: matchMedia });
    try {
      const { container } = render(<Toaster />);
      act(() => {
        toast.error("This session can't be opened from here right now", { duration: Infinity });
      });
      await waitFor(() => {
        const toaster = document.querySelector<HTMLElement>("[data-sonner-toaster]");
        expect(toaster).toHaveAttribute("data-y-position", "top");
        expect(toaster?.style.getPropertyValue("--mobile-offset-top")).toContain("safe-area-inset-top");
      });
      const clearAll = container.querySelector<HTMLElement>("[data-testid='toast-clear-all']");
      expect(clearAll).toHaveAttribute("data-placement", "phone-top");
      expect(clearAll?.style.bottom).toBe("");
    } finally {
      Object.defineProperty(window, "matchMedia", { configurable: true, writable: true, value: original });
    }
  });

  it("has sonner styles imported in index.css", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const indexCss = fs.readFileSync(path.resolve(__dirname, "../../index.css"), "utf8");
    expect(indexCss).toContain("sonner/dist/styles.css");
  });
});
