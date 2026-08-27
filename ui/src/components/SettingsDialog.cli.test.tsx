import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { JSDOM } from "jsdom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { CliLauncherStatus } from "../lib/types";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", {
    url: "http://localhost:3000",
  });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
}

const getCliLauncherStatus = vi.fn<() => Promise<CliLauncherStatus>>();
const installCliLauncher = vi.fn<() => Promise<CliLauncherStatus>>();

vi.mock("../lib/tauri", () => ({
  DEFAULT_TERMINAL_FONT_STACK: 'MesloLGS NF, "Noto Sans KR", monospace',
  createPairingCode: vi.fn(),
  detectAgents: vi.fn(),
  disableRemoteGateway: vi.fn(),
  enableRemoteGateway: vi.fn(),
  getCliLauncherStatus,
  getNotificationPermissionStatus: vi.fn(),
  getRemoteStatus: vi.fn(),
  getTailscaleStatus: vi.fn(),
  installCliLauncher,
  listRemoteDevices: vi.fn(),
  openNotificationSystemSettings: vi.fn(),
  pickNotificationAudio: vi.fn(),
  playNotificationSound: vi.fn(),
  probeNotificationDelivery: vi.fn(),
  requestNotificationPermission: vi.fn(),
  revokeRemoteDevice: vi.fn(),
}));

vi.mock("../lib/browserTauri", () => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  focusBrowser: vi.fn(async () => {}),
  listBrowsers: vi.fn(async () => []),
  setBrowserZoom: vi.fn(async () => 1),
}));

const missingLauncher: CliLauncherStatus = {
  launcherPath: "/Users/test/.local/bin/ferryx",
  isInstalled: false,
  isSymlink: false,
  currentTarget: null,
  activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
  isSupported: true,
};

const installedLauncher: CliLauncherStatus = {
  ...missingLauncher,
  isInstalled: true,
  isSymlink: true,
  currentTarget: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
};

const unsupportedLauncher: CliLauncherStatus = {
  ...missingLauncher,
  isSupported: false,
};

async function renderGeneralSettings() {
  const { GeneralSettings } = await import("./SettingsDialog");
  return render(<GeneralSettings />);
}

describe("Settings > General Ferryx CLI launcher", () => {
  beforeEach(() => {
    cleanup();
    localStorage.clear();
    getCliLauncherStatus.mockReset();
    installCliLauncher.mockReset();
    getCliLauncherStatus.mockResolvedValue(missingLauncher);
  });

  afterEach(cleanup);

  it("shows the launcher location and opt-in install control when missing", async () => {
    await renderGeneralSettings();

    expect(await screen.findByText("Ferryx CLI")).toBeInTheDocument();
    expect(screen.getByText(missingLauncher.launcherPath)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install Ferryx CLI" })).toBeEnabled();
    expect(getCliLauncherStatus).toHaveBeenCalledTimes(1);
  });

  it("installs the launcher and renders the returned installed state", async () => {
    installCliLauncher.mockResolvedValue(installedLauncher);
    await renderGeneralSettings();

    fireEvent.click(await screen.findByRole("button", { name: "Install Ferryx CLI" }));

    expect(await screen.findByText("Installed")).toBeInTheDocument();
    expect(screen.getByText(`Target: ${installedLauncher.currentTarget}`)).toBeInTheDocument();
    expect(installCliLauncher).toHaveBeenCalledTimes(1);
  });

  it("disables the control and announces progress while installation is pending", async () => {
    let resolveInstall: (status: CliLauncherStatus) => void = () => undefined;
    installCliLauncher.mockImplementation(
      () => new Promise<CliLauncherStatus>((resolve) => {
        resolveInstall = resolve;
      }),
    );
    await renderGeneralSettings();

    fireEvent.click(await screen.findByRole("button", { name: "Install Ferryx CLI" }));

    const installing = await screen.findByRole("button", { name: /installing ferryx cli/i });
    expect(installing).toBeDisabled();

    resolveInstall(installedLauncher);
    expect(await screen.findByText("Installed")).toBeInTheDocument();
  });

  it("shows an accessible backend installation failure", async () => {
    installCliLauncher.mockRejectedValue(new Error("launcher path contains a regular file"));
    await renderGeneralSettings();

    fireEvent.click(await screen.findByRole("button", { name: "Install Ferryx CLI" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("launcher path contains a regular file");
  });

  it("does not offer a desktop-only install outside supported runtimes", async () => {
    getCliLauncherStatus.mockResolvedValue(unsupportedLauncher);
    await renderGeneralSettings();

    expect(await screen.findByText(/available in the ferryx desktop app/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install Ferryx CLI" })).not.toBeInTheDocument();
  });

  it("states that PATH and shell profiles are never changed automatically", async () => {
    await renderGeneralSettings();

    expect(await screen.findByText(/does not alter shell profiles or PATH/i)).toBeInTheDocument();
    expect(screen.getByText(/Ensure/)).toHaveTextContent("~/.local/bin");
    expect(screen.getByText(/open a new terminal/i)).toBeInTheDocument();
  });
});
