import { beforeEach, describe, expect, it, mock, spyOn } from "bun:test";
import { JSDOM } from "jsdom";
import type { BrowserTab } from "../lib/types";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost/",
  pretendToBeVisual: true,
});

const view = dom.window;
const globals = globalThis as unknown as Record<string, unknown>;
globals.window = view;
globals.document = view.document;
globals.navigator = view.navigator;
globals.HTMLElement = view.HTMLElement;
globals.HTMLButtonElement = view.HTMLButtonElement;
globals.HTMLInputElement = view.HTMLInputElement;
globals.Element = view.Element;
globals.Node = view.Node;
globals.DocumentFragment = view.DocumentFragment;
globals.SVGElement = view.SVGElement;
globals.MutationObserver = view.MutationObserver;
globals.getComputedStyle = view.getComputedStyle.bind(view);
globals.requestAnimationFrame = view.requestAnimationFrame.bind(view);
globals.cancelAnimationFrame = view.cancelAnimationFrame.bind(view);
globals.localStorage = view.localStorage;
globals.IS_REACT_ACT_ENVIRONMENT = true;

const invoke = mock(async () => undefined);

mock.module("@tauri-apps/api/core", () => ({
  invoke,
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async () => () => undefined,
}));

const browserTauri = await import("../lib/browserTauri");
const devtoolsSpy = spyOn(browserTauri, "openBrowserDevtools");
const elementPickerSpy = spyOn(browserTauri, "injectBrowserElementPicker");
const removePickerSpy = spyOn(browserTauri, "removeBrowserElementPicker");
const finishPickerSpy = spyOn(browserTauri, "finishBrowserElementPick");
const capabilitySpy = spyOn(browserTauri, "getBrowserSnapshotCapability");

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
    devtoolsSpy.mockClear();
    elementPickerSpy.mockClear();
    removePickerSpy.mockClear();
    finishPickerSpy.mockClear();
    capabilitySpy.mockClear();
    // Default to native snapshot capture being available, which is the macOS behavior these
    // tests exercise; the platform-limited case overrides this per test.
    capabilitySpy.mockResolvedValue({ supported: true, formats: ["png"] });
    view.localStorage.clear();
  });

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
    const onToggleElementPick = mock(() => undefined);
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
