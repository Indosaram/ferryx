import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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

describe("SettingsDialog SSH Navigation (seam verification)", () => {
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
    native.getNotificationPermissionStatus.mockReset();
    native.getNotificationPermissionStatus.mockResolvedValue({
      platform: "macos",
      supported: true,
      authorization: "authorized",
      alertsEnabled: true,
      badgeEnabled: true,
      soundsEnabled: true,
      provisional: false,
      criticalAlertsEnabled: false,
    });
    native.listRemoteDevices.mockResolvedValue([]);
    native.detectAgents.mockResolvedValue([]);
    native.getRemoteStatus.mockResolvedValue(null);
    native.getCliLauncherStatus.mockResolvedValue({
      launcherPath: "/Users/test/.local/bin/ferryx",
      isInstalled: true,
      isSymlink: true,
      currentTarget: "/Applications/Ferryx.app",
      activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
      isSupported: true,
    });
    browserNative.listBrowsers.mockResolvedValue([]);
  });

  it("renders an 'SSH Machines' nav item and switches to SSH Machines section", () => {
    render(<SettingsDialog open={true} onClose={vi.fn()} />);

    // Should find the "SSH Machines" button in settings-nav
    const sshNavButton = screen.getByRole("button", { name: "SSH Machines" });
    expect(sshNavButton).toBeInTheDocument();

    // Clicking switches to SSH section
    fireEvent.click(sshNavButton);

    // Should display the SSH Machines header/section within main
    const main = screen.getByRole("main");
    expect(within(main).getByText("SSH Machines")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "SSH Machines" })).toBeInTheDocument();
  });

  it("mounts directly to 'ssh' section when initialSection is 'ssh'", () => {
    render(<SettingsDialog open={true} onClose={vi.fn()} initialSection="ssh" />);

    const sshNavButton = screen.getByRole("button", { name: "SSH Machines" });
    expect(sshNavButton).toHaveAttribute("aria-current", "page");
    const main = screen.getByRole("main");
    expect(within(main).getByText("SSH Machines")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "SSH Machines" })).toBeInTheDocument();
  });
});
