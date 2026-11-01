// Print and Save as PDF from the browser toolbar: the buttons reach the host commands for the
// tab's browser id (the host picks the engine), and a failure shows its code in the toolbar.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserTab } from "../lib/types";

const host = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(async () => undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: host.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));

const { cleanup, fireEvent, render, screen, waitFor } = await import("@testing-library/react");
const { BrowserToolbar } = await import("./BrowserToolbar");
const { printBrowser, saveBrowserPdf } = await import("../lib/browserSystem");

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-1",
  label: "Report",
  browserId: "browser-print",
  url: "https://example.com/report",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

function calls(command: string) {
  return host.invoke.mock.calls.filter(([name]) => name === command);
}

describe("BrowserToolbar print and Save as PDF", () => {
  beforeEach(() => {
    host.invoke.mockReset();
    host.invoke.mockResolvedValue(undefined);
    localStorage.clear();
  });
  afterEach(cleanup);

  it("printBrowser and saveBrowserPdf invoke the host commands with the browser id", async () => {
    await printBrowser("b-7");
    host.invoke.mockResolvedValueOnce(false);
    await expect(saveBrowserPdf("b-7")).resolves.toBe(false);
    expect(host.invoke.mock.calls).toEqual([
      ["cmd_browser_print", { browserId: "b-7" }],
      ["cmd_browser_save_pdf", { browserId: "b-7" }],
    ]);
  });

  it("Print sends cmd_browser_print for this tab", async () => {
    render(<BrowserToolbar tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Print page" }));
    await waitFor(() => expect(calls("cmd_browser_print")).toEqual([["cmd_browser_print", { browserId: "browser-print" }]]));
    expect(calls("cmd_browser_save_pdf")).toEqual([]);
  });

  it("Save as PDF sends cmd_browser_save_pdf for this tab, and a cancelled dialog is not an error", async () => {
    host.invoke.mockImplementation(async (command: string) => (command === "cmd_browser_save_pdf" ? false : undefined));
    render(<BrowserToolbar tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Save page as PDF" }));
    await waitFor(() => expect(calls("cmd_browser_save_pdf")).toEqual([["cmd_browser_save_pdf", { browserId: "browser-print" }]]));
    expect(screen.queryByTestId("browser-toolbar-error")).toBeNull();
  });

  it("shows the host's error code when printing or saving fails", async () => {
    host.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_print") throw { code: "WEBVIEW_NOT_FOUND", message: "gone" };
      if (command === "cmd_browser_save_pdf") throw { code: "BROWSER_NAVIGATION_FAILED", message: "engine stopped" };
      return undefined;
    });
    render(<BrowserToolbar tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Print page" }));
    expect(await screen.findByTestId("browser-toolbar-error")).toHaveTextContent("Print failed: WEBVIEW_NOT_FOUND");
    fireEvent.click(screen.getByRole("button", { name: "Save page as PDF" }));
    await waitFor(() =>
      expect(screen.getByTestId("browser-toolbar-error")).toHaveTextContent("Save as PDF failed: BROWSER_NAVIGATION_FAILED"),
    );
  });

  it("a successful print clears an earlier failure", async () => {
    let fail = true;
    host.invoke.mockImplementation(async (command: string) => {
      if (command === "cmd_browser_print" && fail) throw { code: "WEBVIEW_NOT_FOUND", message: "gone" };
      return undefined;
    });
    render(<BrowserToolbar tab={tab} onNavigate={() => undefined} onReload={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Print page" }));
    expect(await screen.findByTestId("browser-toolbar-error")).toBeInTheDocument();
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Print page" }));
    await waitFor(() => expect(screen.queryByTestId("browser-toolbar-error")).toBeNull());
  });
});
