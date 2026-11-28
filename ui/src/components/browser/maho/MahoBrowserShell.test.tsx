import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserStateChangedPayload } from "../../../lib/browserTauri";

const host = vi.hoisted(() => ({
  invoke: vi.fn(),
  emit: vi.fn(async () => {}),
  stateChanged: null as ((event: BrowserStateChangedPayload) => void) | null,
}));
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()), invoke: host.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: host.emit }));
vi.mock("../../../lib/browserTauri", async (original) => ({
  ...(await original<typeof import("../../../lib/browserTauri")>()),
  onBrowserStateChanged: vi.fn(async (callback: (event: BrowserStateChangedPayload) => void) => {
    host.stateChanged = callback;
    return () => { host.stateChanged = null; };
  }),
}));
vi.mock("./BrowserModeCanvas", () => ({ BrowserModeCanvas: () => null }));
vi.mock("./MahoBrowserSidebar", () => ({ MahoBrowserSidebar: () => null }));
vi.mock("./mahoAutoArchive", () => ({ useBrowserAutoArchive: () => {} }));
vi.mock("../../ui/sonner", () => ({ toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() } }));

import { BROWSER_MODE_SHORTCUT_EVENT, useMahoBrowserShell } from "./MahoBrowserShell";
import type { BrowserAgentActivity } from "./MahoAgentControl";
import { createBrowserStore, type BrowserStore } from "../../../state/browserStore";

const activity: BrowserAgentActivity = {
  activeTasks: [], pendingApprovals: [], controlledBrowserIds: [], handoffRequests: [],
  badgeCount: 0, errorCode: null, busy: false, controllers: [],
  controllerLabelFor: () => null, controllerIdFor: () => null,
  takeOver: async () => {}, doneHandoff: async () => {}, revokeController: async () => {}, resolveApproval: async () => {},
};
function Shell({ store }: { store: BrowserStore }) {
  return useMahoBrowserShell({ active: true, drawerOpen: true, activity, store }).overlays;
}
function seed(count: number) {
  const store = createBrowserStore({ storage: null, now: () => 1 });
  for (let index = 0; index < count; index += 1) {
    store.upsertTab({ browserId: `tab-${index}`, title: `Page ${index}`, url: `https://page-${index}.test` });
  }
  return store;
}
function shortcut(action: string) {
  act(() => window.dispatchEvent(new CustomEvent(BROWSER_MODE_SHORTCUT_EVENT, { detail: { action } })));
}

beforeEach(() => {
  window.localStorage.clear();
  host.emit.mockClear();
  host.invoke.mockReset();
  host.invoke.mockResolvedValue(null);
  host.stateChanged = null;
});

describe("Maho browser shell parity wiring", () => {
  it("opens all tabs from the switcher's more slot and selects an untouched tab", async () => {
    const store = seed(10);
    render(<Shell store={store} />);
    await act(async () => {});
    shortcut("switcher-next");
    const options = screen.getAllByRole("option");
    fireEvent.click(options[options.length - 1]);
    expect(screen.getByRole("dialog", { name: "Maho command bar" })).toBeInTheDocument();
    expect(screen.getAllByRole("option")).toHaveLength(10);
    fireEvent.click(screen.getAllByRole("option").find((option) => option.getAttribute("data-browser-id") === "tab-9")!);
    expect(store.getState().mru[0]).toBe("tab-9");
  });

  it("refreshes cached thumbnails on deactivation and load finish", async () => {
    const store = seed(2);
    store.touch("tab-0");
    host.invoke.mockImplementation(async (command, args) => command === "cmd_browser_capture_thumbnail"
      ? `data:image/png;base64,${(args as { browserId: string }).browserId}` : null);
    render(<Shell store={store} />);
    await act(async () => {});
    await act(async () => { store.touch("tab-1"); });
    expect(host.invoke).toHaveBeenCalledWith("cmd_browser_capture_thumbnail", { browserId: "tab-0" });
    await act(async () => { host.stateChanged?.({ browserId: "tab-1", url: "https://page-1.test", loading: false }); });
    expect(host.invoke).toHaveBeenCalledWith("cmd_browser_capture_thumbnail", { browserId: "tab-1" });
    shortcut("switcher-next");
    expect(document.querySelectorAll(".maho-switcher-thumbnail")).toHaveLength(2);
  });

  it("routes split commands to the app's real shortcut handler", async () => {
    const store = seed(1);
    render(<Shell store={store} />);
    await act(async () => {});
    shortcut("command-bar");
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: ">split right" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(host.emit).toHaveBeenCalledWith("browser_shortcut_requested", expect.objectContaining({ action: "split-right" }));
  });

  it("executes the shell reload command", async () => {
    const store = seed(1);
    store.touch("tab-0");
    render(<Shell store={store} />);
    await act(async () => {});
    shortcut("command-bar");
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: ">reload" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(host.invoke).toHaveBeenCalledWith("cmd_browser_reload", expect.objectContaining({ browserId: "tab-0" }));
  });
});
