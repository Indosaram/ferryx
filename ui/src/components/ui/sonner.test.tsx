import { act, cleanup, fireEvent, render, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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

  it("has sonner styles imported in index.css", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const indexCss = fs.readFileSync(path.resolve(__dirname, "../../index.css"), "utf8");
    expect(indexCss).toContain("sonner/dist/styles.css");
  });
});
