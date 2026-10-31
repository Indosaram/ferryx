import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { saveBrowserSettings } from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";
import { BrowserToolbar } from "./BrowserToolbar";

const browserMocks = vi.hoisted(() => ({
  focusBrowser: vi.fn(async () => undefined),
  getBrowserState: vi.fn(),
  goBackBrowser: vi.fn(async () => undefined),
  goForwardBrowser: vi.fn(async () => undefined),
  openExternalUrl: vi.fn(async () => undefined),
  setBrowserZoom: vi.fn(async () => 1),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
}));

vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return { ...actual, ...browserMocks };
});

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-browser",
  label: "Browser",
  browserId: "browser-1",
  url: "https://start.example.com",
  canGoBack: false,
  canGoForward: false,
  loading: false,
};

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("ferryx.browser.history", JSON.stringify([
    {
      id: "github",
      browserId: "browser-old",
      url: "https://github.com/ferryx/ferryx",
      title: "Ferryx GitHub",
      visitedAt: 200,
    },
    {
      id: "docs",
      browserId: "browser-old",
      url: "https://docs.example.com/browser",
      title: "Browser docs",
      visitedAt: 100,
    },
  ]));
});

afterEach(cleanup);

describe("BrowserToolbar omnibox history", () => {
  it("opens on focus, filters by typed prefix, and navigates the keyboard-selected result", () => {
    const onNavigate = vi.fn();
    render(<BrowserToolbar tab={tab} onNavigate={onNavigate} onReload={vi.fn()} />);
    const input = screen.getByLabelText("URL address bar");

    fireEvent.focus(input);
    expect(screen.getByRole("listbox", { name: "Address bar history" })).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "git" } });
    expect(screen.getByText("Ferryx GitHub")).toBeInTheDocument();
    expect(screen.queryByText("Browser docs")).not.toBeInTheDocument();

    fireEvent.keyDown(input, { key: "ArrowDown" });
    const selected = screen.getByRole("option", { name: /Ferryx GitHub/i });
    expect(selected).toHaveAttribute("aria-selected", "true");
    fireEvent.submit(input.closest("form") as HTMLFormElement);

    expect(onNavigate).toHaveBeenCalledWith("https://github.com/ferryx/ferryx");
  });

  it("closes on Escape and does not expose history suggestions when history is disabled", () => {
    const { unmount } = render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = screen.getByLabelText("URL address bar");
    fireEvent.focus(input);
    expect(screen.getByRole("listbox", { name: "Address bar history" })).toBeInTheDocument();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("listbox", { name: "Address bar history" })).not.toBeInTheDocument();
    unmount();

    saveBrowserSettings({ rememberBrowsingHistory: false });
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    fireEvent.focus(screen.getByLabelText("URL address bar"));
    expect(screen.queryByRole("listbox", { name: "Address bar history" })).not.toBeInTheDocument();
  });

  it("keeps half-typed omnibox text when the page url changes, and syncs again after blur", () => {
    const props = { onNavigate: vi.fn(), onReload: vi.fn() };
    const { rerender } = render(<BrowserToolbar tab={tab} {...props} />);
    const input = screen.getByLabelText("URL address bar") as HTMLInputElement;

    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "docs.exa" } });
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://start.example.com/redirected" }} {...props} />);
    expect(input.value).toBe("docs.exa");

    fireEvent.blur(input);
    rerender(<BrowserToolbar tab={{ ...tab, url: "https://start.example.com/later" }} {...props} />);
    expect(input.value).toBe("https://start.example.com/later");
  });

  it("exposes the combobox pattern with an active descendant only while the list is open", () => {
    render(<BrowserToolbar tab={tab} onNavigate={vi.fn()} onReload={vi.fn()} />);
    const input = screen.getByRole("combobox", { name: "URL address bar" });
    expect(input).toHaveAttribute("aria-autocomplete", "list");
    expect(input).toHaveAttribute("aria-expanded", "false");
    expect(input).not.toHaveAttribute("aria-controls");

    fireEvent.focus(input);
    const listbox = screen.getByRole("listbox", { name: "Address bar history" });
    expect(input).toHaveAttribute("aria-expanded", "true");
    expect(input.getAttribute("aria-controls")).toBe(listbox.id);
    expect(input).not.toHaveAttribute("aria-activedescendant");

    fireEvent.keyDown(input, { key: "ArrowDown" });
    const selected = screen.getByRole("option", { selected: true });
    expect(selected.id).not.toBe("");
    expect(input.getAttribute("aria-activedescendant")).toBe(selected.id);

    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).not.toHaveAttribute("aria-controls");
    expect(input).not.toHaveAttribute("aria-activedescendant");
  });
});
