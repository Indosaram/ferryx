import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsDialog } from "./SettingsDialog";

const native = vi.hoisted(() => ({
  getTerminalPreferences: vi.fn(),
  getNotificationPermissionStatus: vi.fn(),
  requestNotificationPermission: vi.fn(),
  probeNotificationDelivery: vi.fn(),
  openNotificationSystemSettings: vi.fn(),
  playNotificationSound: vi.fn(),
  pickNotificationAudio: vi.fn(),
  listRemoteDevices: vi.fn(),
  revokeRemoteDevice: vi.fn(),
  detectAgents: vi.fn(),
  getRemoteStatus: vi.fn(),
  enableRemoteGateway: vi.fn(),
  disableRemoteGateway: vi.fn(),
  createPairingCode: vi.fn(),
  getCliLauncherStatus: vi.fn(),
  installCliLauncher: vi.fn(),
}));

const browserNative = vi.hoisted(() => ({
  setBrowserZoom: vi.fn(),
  focusBrowser: vi.fn(),
  getBrowserState: vi.fn(),
  listBrowsers: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock(import("../lib/browserTauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    setBrowserZoom: browserNative.setBrowserZoom,
    focusBrowser: browserNative.focusBrowser,
    getBrowserState: browserNative.getBrowserState,
    listBrowsers: browserNative.listBrowsers,
    openExternalUrl: browserNative.openExternalUrl,
  };
});

vi.mock(import("../lib/tauri"), async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    getTerminalPreferences: native.getTerminalPreferences,
    getNotificationPermissionStatus: native.getNotificationPermissionStatus,
    requestNotificationPermission: native.requestNotificationPermission,
    probeNotificationDelivery: native.probeNotificationDelivery,
    openNotificationSystemSettings: native.openNotificationSystemSettings,
    playNotificationSound: native.playNotificationSound,
    pickNotificationAudio: native.pickNotificationAudio,
    listRemoteDevices: native.listRemoteDevices,
    revokeRemoteDevice: native.revokeRemoteDevice,
    detectAgents: native.detectAgents,
    getRemoteStatus: native.getRemoteStatus,
    enableRemoteGateway: native.enableRemoteGateway,
    disableRemoteGateway: native.disableRemoteGateway,
    createPairingCode: native.createPairingCode,
    getCliLauncherStatus: native.getCliLauncherStatus,
    installCliLauncher: native.installCliLauncher,
  };
});

afterEach(cleanup);
beforeEach(() => {
  localStorage.clear();
  native.getTerminalPreferences.mockReset();
  native.getTerminalPreferences.mockResolvedValue({
    fontFamily: "Noto Sans KR",
    macosOptionAsAlt: true,
    source: "ghostty",
    status: "imported",
    sourcePath: "/Users/test/.config/ghostty/config",
  });
});

describe("SettingsDialog Escape key scoping & accessibility", () => {
  it("does not close the dialog when Escape is pressed inside a non-empty shortcuts search input", () => {
    const onClose = vi.fn();
    render(<SettingsDialog open onClose={onClose} />);

    fireEvent.click(screen.getByRole("button", { name: "Keyboard Shortcuts" }));

    const searchInput = screen.getByRole("searchbox", { name: "Search keyboard shortcuts" });
    fireEvent.change(searchInput, { target: { value: "split" } });
    expect(searchInput).toHaveValue("split");

    fireEvent.keyDown(searchInput, { key: "Escape", code: "Escape" });

    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes the dialog when Escape is pressed on the dialog body", () => {
    const onClose = vi.fn();
    render(<SettingsDialog open onClose={onClose} />);

    const dialog = screen.getByRole("dialog", { name: "Settings" });
    fireEvent.keyDown(dialog, { key: "Escape", code: "Escape" });

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close the dialog when Escape dismisses an OPEN select, but does close once it is shut", () => {
    const onClose = vi.fn();
    render(<SettingsDialog open onClose={onClose} />);

    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

    const combobox = screen.getByRole("combobox", { name: "Theme mode" });
    combobox.setAttribute("data-state", "open");
    fireEvent.keyDown(combobox, { key: "Escape", code: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    combobox.setAttribute("data-state", "closed");
    fireEvent.keyDown(combobox, { key: "Escape", code: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("sets aria-current='page' on the active nav button", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    const generalBtn = screen.getByRole("button", { name: "General" });
    expect(generalBtn).toHaveAttribute("aria-current", "page");

    const terminalBtn = screen.getByRole("button", { name: "Terminal" });
    expect(terminalBtn).not.toHaveAttribute("aria-current");

    fireEvent.click(terminalBtn);

    expect(terminalBtn).toHaveAttribute("aria-current", "page");
    expect(generalBtn).not.toHaveAttribute("aria-current");
  });

  it("moves initial focus into the dialog on mount", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    const backButton = screen.getByRole("button", { name: "Back to app" });
    expect(document.activeElement).toBe(backButton);
  });
});
