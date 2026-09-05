import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { useNativeTerminalVisibility } from "./nativeTerminalVisibility";

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
    act(() => {
      document.body.appendChild(dialog);
    });

    await waitFor(() => {
      expect(result.current).toBe(false);
    });

    act(() => {
      document.body.removeChild(dialog);
    });

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });

  it("does not yield surface (returns true) when dialog has data-native-terminal-yield='off'", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);

    const popover = document.createElement("div");
    popover.setAttribute("role", "dialog");
    popover.setAttribute("data-native-terminal-yield", "off");
    act(() => {
      document.body.appendChild(popover);
    });

    await waitFor(() => {
      expect(result.current).toBe(true);
    });

    act(() => {
      document.body.removeChild(popover);
    });

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });

  it("does not yield surface when descendant of data-native-terminal-yield='off'", async () => {
    const { result } = renderHook(() => useNativeTerminalVisibility());
    expect(result.current).toBe(true);

    const container = document.createElement("div");
    container.setAttribute("data-native-terminal-yield", "off");
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    container.appendChild(dialog);

    act(() => {
      document.body.appendChild(container);
    });

    await waitFor(() => {
      expect(result.current).toBe(true);
    });
  });
});
