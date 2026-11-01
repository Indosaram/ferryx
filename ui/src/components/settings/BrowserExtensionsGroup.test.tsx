import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserExtensionsGroup } from "./BrowserExtensionsGroup";
import { CHROME_WEB_STORE_URL, OPEN_BROWSER_URL_EVENT } from "../../lib/browserExtensions";

const ext = vi.hoisted(() => ({
  listBrowserExtensions: vi.fn(),
  openBrowserExtensionPage: vi.fn(),
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
};

describe("BrowserExtensionsGroup", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    window.localStorage.clear();
    ext.openBrowserExtensionPage.mockResolvedValue({});
  });
  afterEach(cleanup);

  it("renders a row per extension and opens details through Manage", async () => {
    ext.listBrowserExtensions.mockResolvedValue({
      profile: "default",
      engineInstalled: true,
      supported: true,
      extensions: [
        { ...base, id: "aaa", name: "Alpha", optionsUrl: "chrome-extension://aaa/options.html" },
        { ...base, id: "bbb", name: "Beta", enabled: false },
      ],
    });
    render(<BrowserExtensionsGroup />);
    const rows = await screen.findAllByTestId("browser-extension-row");
    expect(rows).toHaveLength(2);
    expect(ext.listBrowserExtensions).toHaveBeenCalledWith("default");
    const statuses = screen.getAllByTestId("browser-extension-status");
    expect(statuses[0]).toHaveAttribute("data-enabled", "true");
    expect(statuses[1]).toHaveAttribute("data-enabled", "false");
    fireEvent.click(screen.getAllByTestId("browser-extension-manage")[0]);
    expect(ext.openBrowserExtensionPage).toHaveBeenCalledWith({ kind: "details", id: "aaa" }, "default");
    expect(screen.getAllByTestId("browser-extension-options")).toHaveLength(1);
    fireEvent.click(screen.getByTestId("browser-extension-options"));
    expect(ext.openBrowserExtensionPage).toHaveBeenCalledWith({ kind: "options", id: "aaa" }, "default");
  });

  it("opens the extensions manager, the Web Store and refreshes", async () => {
    ext.listBrowserExtensions.mockResolvedValue({ profile: "default", engineInstalled: true, supported: true, extensions: [] });
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
    ext.listBrowserExtensions.mockResolvedValue({ profile: "default", engineInstalled: false, supported: true, extensions: [] });
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
  });

  it("explains an unsupported platform", async () => {
    ext.listBrowserExtensions.mockResolvedValue({ profile: "default", engineInstalled: false, supported: false, extensions: [] });
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
