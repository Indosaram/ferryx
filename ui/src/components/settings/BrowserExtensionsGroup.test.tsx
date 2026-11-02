import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserExtensionsGroup } from "./BrowserExtensionsGroup";
import { CHROME_WEB_STORE_URL, OPEN_BROWSER_URL_EVENT } from "../../lib/browserExtensions";

const ext = vi.hoisted(() => ({
  listBrowserExtensions: vi.fn(),
  openBrowserExtensionPage: vi.fn(),
  setBrowserExtensionEnabled: vi.fn(),
  setBrowserExtensionPinned: vi.fn(),
  removeBrowserExtension: vi.fn(),
}));

vi.mock("../../lib/browserExtensions", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/browserExtensions")>()),
  ...ext,
}));

const base = {
  version: "1.0.0",
  description: null,
  enabled: true,
  iconDataUrl: null,
  optionsUrl: null,
  homepageUrl: null,
  fromWebStore: true,
  pinned: false,
  removable: true,
};

function listing(extensions: object[], overrides: object = {}) {
  return { profile: "default", engineInstalled: true, engineRunning: true, supported: true, extensions, ...overrides };
}

async function row(id: string): Promise<HTMLElement> {
  const rows = await screen.findAllByTestId("browser-extension-row");
  const found = rows.find((candidate) => candidate.getAttribute("data-extension-id") === id);
  if (!found) throw new Error(`no row for ${id}`);
  return found;
}

describe("BrowserExtensionsGroup", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    window.localStorage.clear();
    ext.openBrowserExtensionPage.mockResolvedValue({});
  });
  afterEach(cleanup);

  it("renders a row per extension and opens details and options", async () => {
    ext.listBrowserExtensions.mockResolvedValue(
      listing([
        { ...base, id: "aaa", name: "Alpha", optionsUrl: "chrome-extension://aaa/options.html" },
        { ...base, id: "bbb", name: "Beta", enabled: false },
      ]),
    );
    render(<BrowserExtensionsGroup />);
    const rows = await screen.findAllByTestId("browser-extension-row");
    expect(rows).toHaveLength(2);
    expect(ext.listBrowserExtensions).toHaveBeenCalledWith("default");
    const switches = screen.getAllByRole("switch");
    expect(switches[0]).toBeChecked();
    expect(switches[1]).not.toBeChecked();
    fireEvent.click(screen.getAllByTestId("browser-extension-manage")[0]);
    expect(ext.openBrowserExtensionPage).toHaveBeenCalledWith({ kind: "details", id: "aaa" }, "default");
    expect(screen.getAllByTestId("browser-extension-options")).toHaveLength(1);
    fireEvent.click(screen.getByTestId("browser-extension-options"));
    expect(ext.openBrowserExtensionPage).toHaveBeenCalledWith({ kind: "options", id: "aaa" }, "default");
  });

  it("toggles an extension through the IPC and renders the returned listing", async () => {
    ext.listBrowserExtensions.mockResolvedValue(listing([{ ...base, id: "aaa", name: "Alpha" }]));
    ext.setBrowserExtensionEnabled.mockResolvedValue(listing([{ ...base, id: "aaa", name: "Alpha", enabled: false }]));
    render(<BrowserExtensionsGroup />);
    const toggle = within(await row("aaa")).getByRole("switch");
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(ext.setBrowserExtensionEnabled).toHaveBeenCalledWith("aaa", false, "default");
    await vi.waitFor(() => {
      const updated = screen.getByTestId("browser-extension-row");
      expect(within(updated).getByRole("switch")).not.toBeChecked();
      expect(updated).toHaveAttribute("data-busy", "false");
    });
  });

  it("pins through the IPC and hides the pin button when the pin state is unknown", async () => {
    ext.listBrowserExtensions.mockResolvedValue(
      listing([
        { ...base, id: "aaa", name: "Alpha", pinned: false },
        { ...base, id: "bbb", name: "Beta", pinned: null },
      ]),
    );
    ext.setBrowserExtensionPinned.mockResolvedValue(
      listing([
        { ...base, id: "aaa", name: "Alpha", pinned: true },
        { ...base, id: "bbb", name: "Beta", pinned: null },
      ]),
    );
    render(<BrowserExtensionsGroup />);
    expect(within(await row("bbb")).queryByTestId("browser-extension-pin")).toBeNull();
    const pin = within(await row("aaa")).getByTestId("browser-extension-pin");
    expect(pin).toHaveAttribute("data-pinned", "false");
    fireEvent.click(pin);
    expect(ext.setBrowserExtensionPinned).toHaveBeenCalledWith("aaa", true, "default");
    await vi.waitFor(async () =>
      expect(within(await row("aaa")).getByTestId("browser-extension-pin")).toHaveAttribute("data-pinned", "true"),
    );
  });

  it("removes only after the Ferryx confirm and hides remove for non-removable extensions", async () => {
    ext.listBrowserExtensions.mockResolvedValue(
      listing([
        { ...base, id: "aaa", name: "Alpha" },
        { ...base, id: "bbb", name: "Beta", removable: false },
      ]),
    );
    ext.removeBrowserExtension.mockResolvedValue(listing([{ ...base, id: "bbb", name: "Beta", removable: false }]));
    render(<BrowserExtensionsGroup />);
    expect(within(await row("bbb")).queryByTestId("browser-extension-remove")).toBeNull();
    const alpha = await row("aaa");
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove"));
    expect(ext.removeBrowserExtension).not.toHaveBeenCalled();
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove-confirm-cancel"));
    expect(within(alpha).queryByTestId("browser-extension-remove-confirm")).toBeNull();
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove"));
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove-confirm-yes"));
    expect(ext.removeBrowserExtension).toHaveBeenCalledWith("aaa", "default");
    await vi.waitFor(() => expect(screen.getAllByTestId("browser-extension-row")).toHaveLength(1));
    expect(screen.getByTestId("browser-extension-row")).toHaveAttribute("data-extension-id", "bbb");
  });

  it("shows an IPC error inline on the row it belongs to", async () => {
    ext.listBrowserExtensions.mockResolvedValue(
      listing([
        { ...base, id: "aaa", name: "Alpha" },
        { ...base, id: "bbb", name: "Beta" },
      ]),
    );
    ext.removeBrowserExtension.mockRejectedValue({
      code: "BROWSER_AUTOMATION_FAILED",
      message: "cancelled",
      details: { reason: "cancelled" },
    });
    ext.setBrowserExtensionEnabled.mockRejectedValue({
      code: "UNSUPPORTED_CAPABILITY",
      message: "the Chromium engine is not installed",
      details: { reason: "engineNotInstalled" },
    });
    render(<BrowserExtensionsGroup />);
    const alpha = await row("aaa");
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove"));
    fireEvent.click(within(alpha).getByTestId("browser-extension-remove-confirm-yes"));
    const cancelled = await within(alpha).findByTestId("browser-extension-error");
    expect(cancelled).toHaveAttribute("role", "alert");
    expect(cancelled).toHaveAttribute("data-reason", "cancelled");
    expect(within(await row("bbb")).queryByTestId("browser-extension-error")).toBeNull();

    fireEvent.click(within(await row("bbb")).getByRole("switch"));
    expect(ext.setBrowserExtensionEnabled).toHaveBeenCalledWith("bbb", false, "default");
    const failed = await within(await row("bbb")).findByTestId("browser-extension-error");
    expect(failed).toHaveAttribute("data-reason", "engineNotInstalled");
    expect(within(await row("bbb")).getByRole("switch")).toBeChecked();
  });

  it("opens the extensions manager, the Web Store and refreshes", async () => {
    ext.listBrowserExtensions.mockResolvedValue(listing([]));
    const received: unknown[] = [];
    const onUrl = (event: Event) => received.push((event as CustomEvent).detail);
    window.addEventListener(OPEN_BROWSER_URL_EVENT, onUrl);
    render(<BrowserExtensionsGroup />);
    expect(await screen.findByTestId("browser-extensions-empty")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("browser-extensions-manage-all"));
    expect(ext.openBrowserExtensionPage).toHaveBeenCalledWith({ kind: "manage" }, "default");
    fireEvent.click(screen.getByTestId("browser-extensions-store"));
    window.removeEventListener(OPEN_BROWSER_URL_EVENT, onUrl);
    expect(received).toEqual([{ url: CHROME_WEB_STORE_URL, profileId: "default" }]);
    fireEvent.click(screen.getByTestId("browser-extensions-refresh"));
    await vi.waitFor(() => expect(ext.listBrowserExtensions).toHaveBeenCalledTimes(2));
  });

  it("offers the engine install when the engine is missing", async () => {
    ext.listBrowserExtensions.mockResolvedValue(listing([], { engineInstalled: false, engineRunning: false }));
    const root = document.createElement("div");
    root.id = "browser-engine";
    const target = document.createElement("button");
    target.setAttribute("data-testid", "browser-engine-action");
    root.appendChild(target);
    root.scrollIntoView = vi.fn();
    document.body.appendChild(root);
    render(<BrowserExtensionsGroup />);
    fireEvent.click(await screen.findByTestId("browser-extensions-install-engine"));
    expect(root.scrollIntoView).toHaveBeenCalled();
    expect(document.activeElement).toBe(target);
    root.remove();
    expect(screen.queryByTestId("browser-extension-row")).toBeNull();
    expect(screen.queryByTestId("browser-extensions-store")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("explains an unsupported platform", async () => {
    ext.listBrowserExtensions.mockResolvedValue(listing([], { engineInstalled: false, engineRunning: false, supported: false }));
    render(<BrowserExtensionsGroup />);
    expect(await screen.findByTestId("browser-extensions-unsupported")).toBeInTheDocument();
    expect(screen.queryByTestId("browser-extensions-install-engine")).toBeNull();
  });

  it("shows the structured error message in an alert", async () => {
    ext.listBrowserExtensions.mockRejectedValue({ code: "INTERNAL_ERROR", message: "profile is locked", details: {} });
    render(<BrowserExtensionsGroup />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByTestId("browser-extensions-error")).toHaveTextContent("profile is locked");
  });
});
