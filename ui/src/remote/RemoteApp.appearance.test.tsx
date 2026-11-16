import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyAppearanceSettings,
  DEFAULT_APPEARANCE_SETTINGS,
  loadAppearanceSettings,
  saveAppearanceSettings,
  useApplyAppearanceSettings,
} from "../lib/appearanceSettings";
import { APPEARANCE_SETTINGS_STORAGE_KEY } from "../lib/storageKeys";
import { RemoteAppearanceButton, RemoteAppearanceSheet } from "./appearance/RemoteAppearanceSheet";
import { RemoteApp } from "./RemoteApp";

vi.mock("./RemoteTerminal", () => ({ RemoteTerminal: () => <div /> }));

function Surface() {
  useApplyAppearanceSettings();
  const [open, setOpen] = useState(false);
  return <>
    <RemoteAppearanceButton open={open} onOpen={() => setOpen(true)} />
    <div data-chat-scope><button type="button">Chat action</button></div>
    <RemoteAppearanceSheet open={open} onOpenChange={setOpen} />
  </>;
}

function openAppearance() {
  fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
  return screen.getByRole("dialog", { name: "Appearance" });
}

const savedTheme = {
  version: 1 as const, id: "remote-light", name: "Remote light", base: "light" as const,
  tokens: { "--background": "#ffffff" },
};

describe("RemoteApp appearance", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: false, media: query, addEventListener: vi.fn(), removeEventListener: vi.fn(),
    }));
  });

  afterEach(() => {
    cleanup();
    applyAppearanceSettings(DEFAULT_APPEARANCE_SETTINGS);
    localStorage.clear();
    vi.unstubAllGlobals();
  });

  it("opens Appearance from the actual remote top bar", async () => {
    localStorage.setItem("ferryx_remote_token", "test-token");
    vi.stubGlobal("WebSocket", class {
      close = vi.fn();
      onmessage = null;
    });
    vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input instanceof Request ? input.url : input);
      const body = url.includes("/api/v1/socket-ticket")
        ? { ticket: "appearance-ticket", expiresAt: 9999999999 }
        : { activeContext: { workspaceId: "ferryx", worktreeSlug: "main", activeTabId: "tab-1",
          activeTerminal: { sessionId: "sess-1", title: "Shell", running: true },
          terminalTabs: [{ id: "tab-1", sessionId: "sess-1", label: "Shell", agentType: "shell" }] } };
      return { ok: true, status: 200, json: async () => body, text: async () => JSON.stringify(body) } as Response;
    }));
    render(<RemoteApp />);
    await screen.findByTestId("mobile-top-bar");
    expect(openAppearance()).toBeInTheDocument();
  });

  it("selects light immediately and restores it after remount from the canonical storage key", () => {
    const first = render(<Surface />);
    openAppearance();
    fireEvent.change(screen.getByRole("combobox", { name: "Theme" }), { target: { value: "light" } });
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(JSON.parse(localStorage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY)!).theme).toBe("light");
    first.unmount();
    document.documentElement.dataset.theme = "dark";
    render(<Surface />);
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("offers all built-ins and six accents but no density control, with 44px controls", () => {
    render(<Surface />);
    expect(screen.getByRole("button", { name: "Appearance" })).toHaveClass("size-11");
    openAppearance();
    const theme = screen.getByRole("combobox", { name: "Theme" });
    expect(Array.from(theme.querySelectorAll("option"), (option) => option.value)).toEqual(["charcoal", "dark", "light", "system"]);
    const accent = screen.getByRole("combobox", { name: "Accent" });
    expect(accent.querySelectorAll("option")).toHaveLength(6);
    expect(theme).toHaveClass("min-h-11");
    expect(accent).toHaveClass("min-h-11");
    expect(screen.queryByLabelText(/density/i)).not.toBeInTheDocument();
    fireEvent.change(accent, { target: { value: "rose" } });
    expect(document.documentElement.dataset.accent).toBe("rose");
    expect(loadAppearanceSettings().accentColor).toBe("rose");
  });

  it("applies the same saved token and effective base as the shared desktop applier and cycles out of it", () => {
    saveAppearanceSettings({ customThemes: [savedTheme] });
    render(<Surface />);
    openAppearance();
    fireEvent.change(screen.getByRole("combobox", { name: "Theme" }), { target: { value: "custom:remote-light" } });
    const remoteBackground = document.documentElement.style.getPropertyValue("--background");
    expect(remoteBackground).toBe("#ffffff");
    expect(document.documentElement.dataset.theme).toBe("light");
    const desktopRoot = document.createElement("div");
    applyAppearanceSettings(loadAppearanceSettings(), desktopRoot);
    expect(desktopRoot.style.getPropertyValue("--background")).toBe(remoteBackground);
    fireEvent.click(screen.getByRole("button", { name: "Cycle theme" }));
    expect(loadAppearanceSettings().activeCustomThemeId).toBeNull();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("--background")).toBe("");
  });

  it("shows retained corrupt-storage diagnostics while booting into charcoal", () => {
    localStorage.setItem(APPEARANCE_SETTINGS_STORAGE_KEY, "{broken");
    render(<Surface />);
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    openAppearance();
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(localStorage.getItem(APPEARANCE_SETTINGS_STORAGE_KEY)).toBe("{broken");
  });

  it("persists the math toggle, refuses unsafe CSS atomically, and keeps recovery outside chat", () => {
    render(<Surface />);
    openAppearance();
    fireEvent.click(screen.getByRole("checkbox", { name: "Render math in chat" }));
    expect(loadAppearanceSettings().renderMath).toBe(false);
    const editor = screen.getByRole("textbox", { name: "Custom CSS" });
    fireEvent.change(editor, { target: { value: "@import url(https://evil.test);" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply custom CSS" }));
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(loadAppearanceSettings().customCss).toBe("");
    expect(document.querySelector("style[data-custom-theme-css]")).toBeNull();
    fireEvent.change(editor, { target: { value: "button { display: none }" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply custom CSS" }));
    expect(loadAppearanceSettings().customCss).toBe("button { display: none }");
    const recovery = screen.getByRole("button", { name: "Remove custom CSS" });
    expect(recovery.closest("[data-chat-scope]")).toBeNull();
    expect(recovery).toBeVisible();
    fireEvent.click(recovery);
    expect(loadAppearanceSettings().customCss).toBe("");
    expect(document.querySelector("style[data-custom-theme-css]")).toBeNull();
  });
});
