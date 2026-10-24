import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const openUrl = vi.fn<(url: string) => Promise<void>>(() => Promise.resolve());
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import { handleExternalLinkClick, openExternal, safeExternalUrl } from "./externalLinks";

function withTauriInternals(present: boolean) {
  if (present) Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  else Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
}

describe("externalLinks", () => {
  let windowOpen: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    openUrl.mockClear();
    windowOpen = vi.spyOn(window, "open").mockReturnValue(null);
  });
  afterEach(() => {
    cleanup();
    withTauriInternals(false);
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
  });

  it("allows only https and mailto", () => {
    expect(safeExternalUrl("https://ferryx.dev/docs")).toBe("https://ferryx.dev/docs");
    expect(safeExternalUrl("mailto:hi@ferryx.dev")).toBe("mailto:hi@ferryx.dev");
    expect(safeExternalUrl("http://ferryx.dev")).toBeNull();
    expect(safeExternalUrl("javascript:alert(1)")).toBeNull();
    expect(safeExternalUrl("/relative")).toBeNull();
  });

  it("uses the Tauri opener inside the mobile companion", async () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "1");
    withTauriInternals(true);
    await expect(openExternal("https://ferryx.dev/")).resolves.toBe(true);
    expect(openUrl).toHaveBeenCalledWith("https://ferryx.dev/");
    expect(windowOpen).not.toHaveBeenCalled();
  });

  it("opens a new tab on the web", async () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "");
    await expect(openExternal("https://ferryx.dev/")).resolves.toBe(true);
    expect(windowOpen).toHaveBeenCalledWith("https://ferryx.dev/", "_blank", "noopener,noreferrer");
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("refuses a disallowed scheme without opening anything", async () => {
    await expect(openExternal("javascript:alert(1)")).resolves.toBe(false);
    expect(windowOpen).not.toHaveBeenCalled();
    expect(openUrl).not.toHaveBeenCalled();
  });

  it("routes an anchor click through openExternal and keeps the href", () => {
    render(<a href="https://ferryx.dev/x" target="_blank" onClick={handleExternalLinkClick}>link</a>);
    const link = screen.getByText("link");
    const notPrevented = fireEvent.click(link);
    expect(notPrevented).toBe(false);
    expect(link.getAttribute("href")).toBe("https://ferryx.dev/x");
    expect(windowOpen).toHaveBeenCalledWith("https://ferryx.dev/x", "_blank", "noopener,noreferrer");
  });

  it("leaves a refused-scheme link to the browser", () => {
    render(<a href="#section" onClick={handleExternalLinkClick}>img</a>);
    expect(fireEvent.click(screen.getByText("img"))).toBe(true);
    expect(windowOpen).not.toHaveBeenCalled();
  });
});
