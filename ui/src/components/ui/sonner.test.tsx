import { act, cleanup, render, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import {
  APPEARANCE_SETTINGS_EVENT,
  saveAppearanceSettings,
} from "../../lib/appearanceSettings";
import { Toaster, useToastTheme } from "./sonner";

describe("sonner Toaster and useToastTheme", () => {
  afterEach(() => {
    cleanup();
    localStorage.clear();
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
});
