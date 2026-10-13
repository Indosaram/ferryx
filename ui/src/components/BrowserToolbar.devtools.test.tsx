import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BrowserTab } from "../lib/types";

const { invoke } = vi.hoisted(() => ({
  invoke: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

vi.mock("../lib/browserTauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/browserTauri")>()),
}));

const browserTauri = await import("../lib/browserTauri");
let devtoolsSpy = vi.spyOn(browserTauri, "openBrowserDevtools");
let elementPickerSpy = vi.spyOn(browserTauri, "injectBrowserElementPicker");
let removePickerSpy = vi.spyOn(browserTauri, "removeBrowserElementPicker");
let finishPickerSpy = vi.spyOn(browserTauri, "finishBrowserElementPick");
let capabilitySpy = vi.spyOn(browserTauri, "getBrowserSnapshotCapability");

const { act, cleanup, fireEvent, render, screen, waitFor } = await import("@testing-library/react");
const { BrowserToolbar } = await import("./BrowserToolbar");

const tab: BrowserTab = {
  kind: "browser",
  id: "tab-1",
  label: "Example",
  browserId: "browser-42",
  url: "https://example.com",
  loading: false,
  canGoBack: false,
  canGoForward: false,
};

function renderToolbar(extra?: {
  readonly onToggleElementPick?: () => void;
  readonly elementPicking?: boolean;
}) {
  return render(
    <BrowserToolbar
      tab={tab}
      onNavigate={() => undefined}
      onReload={() => undefined}
      onToggleElementPick={extra?.onToggleElementPick}
      elementPicking={extra?.elementPicking}
    />,
  );
}

describe("BrowserToolbar devtools", () => {
  beforeEach(() => {
    cleanup();
    invoke.mockClear();
    devtoolsSpy = vi.spyOn(browserTauri, "openBrowserDevtools");
    elementPickerSpy = vi.spyOn(browserTauri, "injectBrowserElementPicker");
    removePickerSpy = vi.spyOn(browserTauri, "removeBrowserElementPicker");
    finishPickerSpy = vi.spyOn(browserTauri, "finishBrowserElementPick");
    capabilitySpy = vi.spyOn(browserTauri, "getBrowserSnapshotCapability");
    devtoolsSpy.mockClear();
    elementPickerSpy.mockClear();
    removePickerSpy.mockClear();
    finishPickerSpy.mockClear();
    capabilitySpy.mockClear();
    capabilitySpy.mockResolvedValue({ supported: true, formats: ["png"] });
    localStorage.clear();
  });

  afterEach(cleanup);

  it("calls openBrowserDevtools with the current browser id when DevTools is clicked", async () => {
    renderToolbar();

    fireEvent.click(screen.getByRole("button", { name: "DevTools" }));

    expect(devtoolsSpy).toHaveBeenCalledWith("browser-42");
    expect(invoke).toHaveBeenCalledWith("cmd_browser_open_devtools", {
      browserId: "browser-42",
    });
  });

  it("injects the element picker when selection is off", () => {
    renderToolbar({ elementPicking: false });
    fireEvent.click(screen.getByRole("button", { name: "Select element" }));
    expect(elementPickerSpy).toHaveBeenCalledWith("browser-42");
    expect(invoke).toHaveBeenCalledWith("cmd_browser_inject_element_picker", {
      browserId: "browser-42",
    });
    expect(removePickerSpy).not.toHaveBeenCalled();
  });

  it("removes the element picker when selection is already on", () => {
    const onToggleElementPick = vi.fn(() => undefined);
    renderToolbar({ onToggleElementPick, elementPicking: true });

    const picker = screen.getByRole("button", { name: "Select element" });
    expect(picker.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(picker);

    expect(onToggleElementPick).toHaveBeenCalledTimes(1);
    expect(removePickerSpy).toHaveBeenCalledWith("browser-42");
    expect(invoke).toHaveBeenCalledWith("cmd_browser_remove_element_picker", {
      browserId: "browser-42",
    });
    expect(elementPickerSpy).not.toHaveBeenCalled();
  });

  it("disables element picking when the snapshot capability reports no native capture", async () => {
    capabilitySpy.mockResolvedValue({ supported: false, formats: [] });
    renderToolbar();

    const picker = screen.getByRole("button", { name: "Select element" }) as HTMLButtonElement;
    await waitFor(() => expect(picker.disabled).toBe(true));
    expect(capabilitySpy).toHaveBeenCalledTimes(1);
  });

  it("keeps element picking enabled and working when the snapshot capability reports support", async () => {
    renderToolbar();

    const picker = screen.getByRole("button", { name: "Select element" }) as HTMLButtonElement;
    await act(async () => undefined);
    expect(capabilitySpy).toHaveBeenCalledTimes(1);
    expect(picker.disabled).toBe(false);

    fireEvent.click(picker);
    expect(elementPickerSpy).toHaveBeenCalledWith("browser-42");
  });
});
