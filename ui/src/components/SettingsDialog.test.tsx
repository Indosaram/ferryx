import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_SETTINGS_EVENT,
  loadAppearanceSettings,
} from "../lib/appearanceSettings";
import { SHORTCUTS, shortcutLabel } from "../lib/shortcuts";
import { SIDEBAR_OPEN_STORAGE_KEY } from "../lib/storageKeys";
import { TERMINAL_SETTINGS_STORAGE_KEY } from "../lib/terminalSettings";
import { SettingsDialog } from "./SettingsDialog";

async function selectRadixOption(triggerLabel: string | RegExp, optionText: string | RegExp) {
  const trigger = screen.getByRole("combobox", { name: triggerLabel });
  fireEvent.pointerDown(trigger, { pointerId: 1, button: 0 });
  fireEvent.click(trigger);
  const option = await screen.findByRole("option", { name: optionText });
  fireEvent.click(option);
}

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
  getTailscaleStatus: vi.fn(),
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
    getTailscaleStatus: native.getTailscaleStatus,
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
  native.getNotificationPermissionStatus.mockReset();
  native.getNotificationPermissionStatus.mockResolvedValue({
    platform: "macos",
    supported: true,
    authorization: "authorized",
    alertsEnabled: true,
    soundsEnabled: true,
    requested: true,
    authoritative: true,
    canOpenSettings: true,
  });
  native.requestNotificationPermission.mockReset();
  native.requestNotificationPermission.mockResolvedValue({
    granted: true,
    status: {},
  });
  native.probeNotificationDelivery.mockReset();
  native.probeNotificationDelivery.mockResolvedValue({
    outcome: "submitted",
    status: {},
    testSubmitted: true,
  });
  native.getCliLauncherStatus.mockReset();
  native.getCliLauncherStatus.mockResolvedValue({
    launcherPath: "/Users/test/.local/bin/ferryx",
    isInstalled: true,
    isSymlink: true,
    currentTarget: null,
    activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
    isSupported: true,
  });
  native.installCliLauncher.mockReset();
  native.openNotificationSystemSettings.mockReset();
  native.openNotificationSystemSettings.mockResolvedValue({ opened: true });
  native.playNotificationSound.mockReset();
  native.playNotificationSound.mockResolvedValue({ played: true });
  native.pickNotificationAudio.mockReset();
  native.pickNotificationAudio.mockResolvedValue({
    path: "/custom/bell.mp3",
    displayName: "bell.mp3",
    extension: "mp3",
    sizeBytes: 1024,
  });
  native.listRemoteDevices.mockReset();
  native.listRemoteDevices.mockResolvedValue([
    {
      id: "device-1",
      name: "iPhone 15 Pro",
      permission: "control",
      createdAt: 1700000000000,
      lastSeenAt: 1700000500000,
      revoked: false,
    },
    {
      id: "device-2",
      name: "Pixel 8",
      permission: "view",
      createdAt: 1690000000000,
      lastSeenAt: 1690000500000,
      revoked: false,
    },
  ]);
  native.revokeRemoteDevice.mockReset();
  native.revokeRemoteDevice.mockResolvedValue(true);
  native.getTailscaleStatus.mockReset();
  native.getTailscaleStatus.mockResolvedValue({
    installed: true,
    running: true,
    tailnetName: "orca-mesh",
    selfDns: "orca-host.tailscale.net",
    serveActive: true,
  });
  const disabledRemoteStatus = {
    enabled: false,
    mode: "off" as const,
    port: 43821,
    boundAddress: null,
    localIp: null,
    tailscale: {
      installed: true,
      running: true,
      tailnetName: "orca-mesh",
      selfDns: "orca-host.tailscale.net",
      serveActive: true,
    },
  };
  native.getRemoteStatus.mockReset();
  native.getRemoteStatus.mockResolvedValue(disabledRemoteStatus);
  native.enableRemoteGateway.mockReset();
  native.enableRemoteGateway.mockImplementation(async (request?: { mode?: string }) => {
    const enabledStatus = {
      ...disabledRemoteStatus,
      enabled: true,
      mode: request?.mode ?? "localNetwork",
      boundAddress: "0.0.0.0:43821",
      localIp: "10.0.0.8",
    };
    native.getRemoteStatus.mockResolvedValue(enabledStatus);
    return enabledStatus;
  });
  native.disableRemoteGateway.mockReset();
  native.disableRemoteGateway.mockResolvedValue(disabledRemoteStatus);
  native.createPairingCode.mockReset();
  native.createPairingCode.mockResolvedValue({
    code: "482916",
    expiresInSeconds: 300,
  });
  native.detectAgents.mockReset();
  native.detectAgents.mockResolvedValue([
    { name: "claude", available: true },
    { name: "codex", available: false },
    { name: "gemini", available: false },
    { name: "opencode", available: true },
    { name: "aider", available: false },
    { name: "cursor-agent", available: false },
    { name: "droid", available: false },
    { name: "crush", available: false },
  ]);
  browserNative.setBrowserZoom.mockReset();
  browserNative.setBrowserZoom.mockResolvedValue(1.25);
  browserNative.listBrowsers.mockReset();
  browserNative.listBrowsers.mockResolvedValue([
    {
      browserId: "b-1",
      webviewLabel: "webview-1",
      url: "http://localhost:3000",
      title: "Local App",
      visible: true,
    },
  ]);
  browserNative.focusBrowser.mockReset();
  browserNative.focusBrowser.mockResolvedValue(undefined);
});

describe("SettingsDialog", () => {
  it("is a full-view original-Orca-style nav/detail surface", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    expect(screen.getByRole("dialog", { name: "Settings" })).toHaveAttribute("aria-modal", "false");
    expect(screen.getByTestId("settings-nav")).toHaveClass("w-[280px]");
    expect(screen.getByRole("button", { name: "Back to app" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "General" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Appearance" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Keyboard Shortcuts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Workspace" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Browser" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
  });

  it("persists confirm before closing a tab checkbox in General settings", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "General" })).toBeInTheDocument();

    const checkbox = screen.getByLabelText("Confirm before closing a tab");
    expect(checkbox).not.toBeChecked();

    fireEvent.click(checkbox);
    expect(checkbox).toBeChecked();
    expect(localStorage.getItem("ferryx.settings.general")).toContain('"confirmCloseTab":true');
  });

  it("moves the Ferryx CLI launcher card into the General section", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "General" })).toBeInTheDocument();
    expect(await screen.findByText("Ferryx CLI")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Browser" }));
    expect(screen.queryByText("Ferryx CLI")).not.toBeInTheDocument();
  });

  it("persists the show-sidebar-on-startup toggle", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "General" })).toBeInTheDocument();

    const checkbox = screen.getByLabelText("Show sidebar on startup");
    expect(checkbox).toBeChecked();

    fireEvent.click(checkbox);
    expect(checkbox).not.toBeChecked();
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");

    fireEvent.click(checkbox);
    expect(checkbox).toBeChecked();
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("true");
  });

  it("fetches Ghostty preferences and shows the effective terminal value/source", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Terminal" }));

    await waitFor(() => expect(native.getTerminalPreferences).toHaveBeenCalled());
    expect(screen.getByLabelText("Font family")).toHaveValue("Noto Sans KR");
    expect(screen.getByText(/Ghostty · Imported/i)).toBeInTheDocument();
    expect(screen.getByText("/Users/test/.config/ghostty/config")).toBeInTheDocument();
    expect(screen.getByLabelText("macOS Option as Alt")).toBeChecked();
  });

  it("persists explicit local terminal overrides above Ghostty", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Terminal" }));
    await waitFor(() => expect(native.getTerminalPreferences).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("Font family"), { target: { value: "JetBrains Mono" } });
    fireEvent.change(screen.getByLabelText("Font size"), { target: { value: "17" } });
    fireEvent.change(screen.getByLabelText("Scrollback"), { target: { value: "30000" } });

    expect(JSON.parse(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)!)).toMatchObject({
      fontFamily: "JetBrains Mono",
      fontSize: 17,
      scrollback: 30_000,
    });
    expect(screen.getByText("Local override")).toBeInTheDocument();
  });

  it("labels a font-size-only override as a local override and clears it with Use imported", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Terminal" }));
    await waitFor(() => expect(native.getTerminalPreferences).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText("Font size"), { target: { value: "18" } });

    expect(screen.getByText("Local override")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Use imported/i }));

    expect(JSON.parse(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)!)).toMatchObject({
      fontFamily: null,
      fontSize: null,
      macosOptionAsAlt: null,
    });
    await waitFor(() => expect(screen.getByText(/Ghostty · Imported/i)).toBeInTheDocument());
  });

  it("shows the registered shortcuts in the dedicated Keyboard Shortcuts section", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Keyboard Shortcuts" }));

    for (const shortcut of SHORTCUTS) {
      const titleEl = screen.getByText(shortcut.title);
      expect(titleEl).toBeInTheDocument();
      const row = titleEl.closest(".min-h-10") ?? (titleEl.parentElement?.parentElement as HTMLElement);
      expect(row).not.toBeNull();
      expect(within(row as HTMLElement).getByText(shortcutLabel(shortcut.id))).toBeInTheDocument();
    }
    expect(screen.getByRole("region", { name: "Keyboard Shortcuts" })).toBeInTheDocument();
  });

  it("navigates to Notifications section and displays toggles, permission status, volume slider, and test button", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));

    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByLabelText(/Enable Notifications/i)).toBeInTheDocument();
    expect(screen.getByRole("slider")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Send Test Notification/i })).toBeInTheDocument();
    expect(screen.getByText(/authorized/i)).toBeInTheDocument();
  });

  it("toggles Enable Notifications and updates state and localStorage", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    const toggle = screen.getByRole("switch", { name: /Enable Notifications/i });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);

    expect(toggle).not.toBeChecked();
    const storedValues = Object.values(localStorage);
    expect(storedValues.some((val) => val.includes(`"enabled":false`))).toBe(true);
  });

  it("triggers probeNotificationDelivery when clicking Send Test Notification", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    const testButton = screen.getByRole("button", { name: /Send Test Notification/i });
    fireEvent.click(testButton);

    await waitFor(() => expect(native.probeNotificationDelivery).toHaveBeenCalled());
  });

  it("picks custom sound and triggers pickNotificationAudio", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    await selectRadixOption("Notification Sound", /custom audio file/i);

    const pickButton = await screen.findByRole("button", { name: /choose|\bbrowse\b|pick|custom/i });
    fireEvent.click(pickButton);

    await waitFor(() => expect(native.pickNotificationAudio).toHaveBeenCalled());
    await waitFor(() => {
      expect(
        screen.queryByText(/bell\.mp3/i) || screen.queryByDisplayValue(/bell\.mp3|\/custom\/bell\.mp3/i)
      ).toBeInTheDocument();
    });
  });

  it("keeps General free of the duplicate appearance summary", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    expect(screen.queryByText(/local desktop/i)).not.toBeInTheDocument();
    expect(screen.queryByText("Color scheme")).not.toBeInTheDocument();
    expect(screen.queryByText("Density")).not.toBeInTheDocument();
  });

  it("navigates to Appearance section, renders controls, and persists changes to localStorage", async () => {
    const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

    expect(screen.getByRole("region", { name: "Appearance" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Theme mode" })).toHaveTextContent(/charcoal/i);
    expect(screen.getByRole("combobox", { name: "Accent color" })).toHaveTextContent(/slate/i);
    expect(screen.getByRole("combobox", { name: "Interface density" })).toHaveTextContent(/compact/i);

    await selectRadixOption("Theme mode", /dark/i);
    await selectRadixOption("Accent color", /emerald/i);
    await selectRadixOption("Interface density", /comfortable/i);

    const stored = JSON.parse(localStorage.getItem("ferryx.settings.appearance")!);
    expect(stored).toEqual({
      theme: "dark",
      accentColor: "emerald",
      density: "comfortable",
    });

    unmount();
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    expect(screen.getByRole("combobox", { name: "Theme mode" })).toHaveTextContent(/dark/i);
    expect(screen.getByRole("combobox", { name: "Accent color" })).toHaveTextContent(/emerald/i);
    expect(screen.getByRole("combobox", { name: "Interface density" })).toHaveTextContent(/comfortable/i);

    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect(screen.getByRole("combobox", { name: "Theme mode" })).toHaveTextContent(/charcoal/i);
    expect(screen.getByRole("combobox", { name: "Accent color" })).toHaveTextContent(/slate/i);
    expect(screen.getByRole("combobox", { name: "Interface density" })).toHaveTextContent(/compact/i);
    expect(localStorage.getItem("ferryx.settings.appearance")).toBeNull();
  });

  it("navigates to Browser section, changes search engine and zoom, and persists to localStorage", async () => {
    const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Browser" }));

    expect(screen.getByRole("region", { name: "Browser" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Default search engine" })).toHaveTextContent(/duckduckgo/i);
    expect(screen.getByRole("combobox", { name: "Default zoom level" })).toHaveTextContent(/100%/i);
    const restoreToggle = screen.getByRole("switch", { name: "Restore tabs on launch" });
    expect(restoreToggle).not.toBeChecked();

    await selectRadixOption("Default search engine", /google/i);
    await selectRadixOption("Default zoom level", /125%/i);
    fireEvent.click(restoreToggle);

    const stored = JSON.parse(localStorage.getItem("ferryx.settings.browser")!);
    expect(stored).toEqual({
      searchEngine: "google",
      defaultZoom: 125,
      restoreTabsOnLaunch: true,
    });

    unmount();
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Browser" }));
    expect(screen.getByRole("combobox", { name: "Default search engine" })).toHaveTextContent(/google/i);
    expect(screen.getByRole("combobox", { name: "Default zoom level" })).toHaveTextContent(/125%/i);
    expect(screen.getByRole("switch", { name: "Restore tabs on launch" })).toBeChecked();

    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect(screen.getByRole("combobox", { name: "Default search engine" })).toHaveTextContent(/duckduckgo/i);
    expect(screen.getByRole("combobox", { name: "Default zoom level" })).toHaveTextContent(/100%/i);
    expect(screen.getByRole("switch", { name: "Restore tabs on launch" })).not.toBeChecked();
    expect(localStorage.getItem("ferryx.settings.browser")).toBeNull();
  });

  it("navigates to Remote Access section, renders paired devices and Tailscale status, and revokes device on confirmation", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));

    await waitFor(() => expect(native.listRemoteDevices).toHaveBeenCalled());
    await waitFor(() => expect(native.getTailscaleStatus).toHaveBeenCalled());

    expect(screen.getByText("iPhone 15 Pro")).toBeInTheDocument();
    expect(screen.getByText("Pixel 8")).toBeInTheDocument();
    expect(screen.getByText(/orca-mesh/i)).toBeInTheDocument();

    const revokeBtn = screen.getByRole("button", { name: "Revoke device iPhone 15 Pro" });
    fireEvent.click(revokeBtn);

    const confirmBtn = screen.getByRole("button", { name: "Confirm revoke iPhone 15 Pro" });
    fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(native.revokeRemoteDevice).toHaveBeenCalledWith("device-1");
    });
  });

  it("navigates to Browser section and applies zoom to active browsers using setBrowserZoom", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Browser" }));

    await waitFor(() => expect(browserNative.listBrowsers).toHaveBeenCalled());

    await selectRadixOption("Default zoom level", /125%/i);

    await waitFor(() => {
      expect(browserNative.setBrowserZoom).toHaveBeenCalledWith("b-1", 1.25);
    });
  });

  it("navigates to Agents section, displays detected agents, configures default agent and overrides", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Agents" }));

    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());

    expect(screen.getByText("2 detected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Toggle Claude configuration" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Toggle Opencode configuration" })).toBeInTheDocument();

    // Default agent selection
    const claudeDefaultBtn = screen.getByRole("button", { name: "Claude" });
    fireEvent.click(claudeDefaultBtn);

    let stored = JSON.parse(localStorage.getItem("ferryx.agents.v1")!);
    expect(stored.defaultAgentId).toBe("claude");

    // Toggle enabled checkbox
    const claudeToggle = screen.getByRole("switch", { name: "Enable Claude" });
    expect(claudeToggle).toBeChecked();
    fireEvent.click(claudeToggle);
    expect(claudeToggle).not.toBeChecked();

    stored = JSON.parse(localStorage.getItem("ferryx.agents.v1")!);
    expect(stored.overrides.claude.enabled).toBe(false);

    // Expand configuration and edit command & arguments
    fireEvent.click(screen.getByRole("button", { name: "Toggle Claude configuration" }));

    const cmdInput = screen.getByLabelText("Claude command");
    const argsInput = screen.getByLabelText("Claude arguments");

    fireEvent.change(cmdInput, { target: { value: "claude-custom" } });
    fireEvent.blur(cmdInput);
    fireEvent.change(argsInput, { target: { value: "--dangerously-skip-permissions" } });
    fireEvent.blur(argsInput);

    stored = JSON.parse(localStorage.getItem("ferryx.agents.v1")!);
    expect(stored.overrides.claude.command).toBe("claude-custom");
    expect(stored.overrides.claude.args).toBe("--dangerously-skip-permissions");

    // Refresh button
    native.detectAgents.mockClear();
    fireEvent.click(screen.getByRole("button", { name: /Refresh/i }));
    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());
  });

  it("persists Appearance through the appearanceSettings API across remount", async () => {
    const events: unknown[] = [];
    const listener = (event: Event) => {
      events.push((event as CustomEvent).detail);
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, listener);

    try {
      const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
      fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

      await selectRadixOption("Theme mode", /light/i);
      await selectRadixOption("Accent color", /ocean blue/i);
      await selectRadixOption("Interface density", /comfortable/i);

      expect(loadAppearanceSettings()).toEqual({
        theme: "light",
        accentColor: "blue",
        density: "comfortable",
      });
      expect(events.at(-1)).toEqual({
        theme: "light",
        accentColor: "blue",
        density: "comfortable",
      });

      unmount();
      render(<SettingsDialog open onClose={vi.fn()} />);
      fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
      expect(screen.getByRole("combobox", { name: "Theme mode" })).toHaveTextContent(/light/i);
      expect(screen.getByRole("combobox", { name: "Accent color" })).toHaveTextContent(/ocean blue/i);
      expect(screen.getByRole("combobox", { name: "Interface density" })).toHaveTextContent(/comfortable/i);
    } finally {
      window.removeEventListener(APPEARANCE_SETTINGS_EVENT, listener);
    }
  });

  it("copies the enabled Remote pairing PIN and shows copied feedback", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));
    await waitFor(() => expect(native.getRemoteStatus).toHaveBeenCalled());

    const remoteToggle = screen.getByRole("switch", { name: "Remote Access" });
    fireEvent.click(remoteToggle);

    const pinButton = await screen.findByTestId("remote-pairing-code");
    expect(pinButton.tagName).toBe("BUTTON");
    expect(pinButton).toHaveTextContent("482916");

    fireEvent.click(pinButton);

    expect(writeText).toHaveBeenCalledWith("482916");
    expect(pinButton).toHaveTextContent(/copied/i);
  });

  it("states that authorized browser profiles reconnect while Remote remains enabled and only require re-pairing when storage cleared, revoked, or profile changed", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));

    const remote = screen.getByRole("region", { name: "Remote Access" });
    expect(remote).toHaveTextContent(/authorized browser profiles reconnect/i);
    expect(remote).toHaveTextContent(/re-pair only after browser storage is cleared, a device is revoked, or a different browser profile\/device is used/i);
  });

  it("automatically generates and displays a new QR code when Remote Access is already Active with paired devices present", async () => {
    const activeRemoteStatus = {
      enabled: true,
      mode: "localNetwork" as const,
      port: 43821,
      boundAddress: "0.0.0.0:43821",
      localIp: "192.168.1.50",
      tailscale: {
        installed: true,
        running: true,
        tailnetName: "orca-mesh",
        selfDns: "orca-host.tailscale.net",
        serveActive: true,
      },
    };
    native.getRemoteStatus.mockResolvedValue(activeRemoteStatus);
    native.listRemoteDevices.mockResolvedValue([
      {
        id: "dev-phone-1",
        name: "Mobile Device",
        permission: "control",
        createdAt: Date.now() - 60000,
        lastSeenAt: Date.now() - 10000,
        revoked: false,
      },
    ]);
    native.createPairingCode.mockResolvedValueOnce({
      code: "719340",
      expiresInSeconds: 60,
    });

    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));

    await waitFor(() => expect(native.getRemoteStatus).toHaveBeenCalled());
    await waitFor(() => expect(native.createPairingCode).toHaveBeenCalledTimes(1));

    const pinButton = await screen.findByTestId("remote-pairing-code");
    expect(pinButton).toHaveTextContent("719340");
    const qrImg = await screen.findByAltText("Pairing QR Code");
    expect(qrImg).toBeInTheDocument();
    expect(screen.queryByText("Generating...")).not.toBeInTheDocument();

    // Verify refreshing / generating a new code works while active and paired devices exist
    native.createPairingCode.mockResolvedValueOnce({
      code: "882194",
      expiresInSeconds: 60,
    });
    const newCodeButton = screen.getByRole("button", { name: /New Code/i });
    fireEvent.click(newCodeButton);

    await waitFor(() => expect(native.createPairingCode).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("PIN: 882194")).toBeInTheDocument();
  });

  it("surfaces a QR generation failure with a retry option rather than leaving indefinite Generating... loading", async () => {
    const activeRemoteStatus = {
      enabled: true,
      mode: "localNetwork" as const,
      port: 43821,
      boundAddress: "0.0.0.0:43821",
      localIp: "192.168.1.50",
      tailscale: {
        installed: false,
        running: false,
        tailnetName: null,
        selfDns: null,
        serveActive: false,
      },
    };
    native.getRemoteStatus.mockResolvedValue(activeRemoteStatus);
    native.createPairingCode.mockRejectedValueOnce(new Error("Pairing creation failed on daemon"));

    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));

    await waitFor(() => expect(native.getRemoteStatus).toHaveBeenCalled());
    expect(await screen.findByText(/Pairing creation failed on daemon/i)).toBeInTheDocument();
    expect(screen.queryByText("Generating...")).not.toBeInTheDocument();

    // Retry button recovers
    native.createPairingCode.mockResolvedValueOnce({
      code: "654321",
      expiresInSeconds: 60,
    });
    const retryButton = screen.getByRole("button", { name: /Retry/i });
    fireEvent.click(retryButton);

    expect(await screen.findByTestId("remote-pairing-code")).toHaveTextContent("654321");
    expect(await screen.findByAltText("Pairing QR Code")).toBeInTheDocument();
  });

  it("describes Default Agent as first in the New Tab list with a Default label, not auto-launch", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Agents" }));
    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());

    expect(screen.getByRole("group", { name: "Default Agent" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Auto" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "None" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Claude" })).toBeInTheDocument();

    const agents = screen.getByRole("region", { name: "Agents" });
    expect(agents).toHaveTextContent(/New Tab/i);
    expect(agents).toHaveTextContent(/Default label/i);
    expect(agents).toHaveTextContent(/enabled and available/i);
    expect(agents).toHaveTextContent(/does not auto-launch/i);
    expect(agents).toHaveTextContent(/clicking a listed agent still launches that agent/i);
    expect(agents).toHaveTextContent(/unavailable, disabled, or missing/i);
    expect(agents).toHaveTextContent(/natural agent order/i);
    expect(agents).not.toHaveTextContent(/New Terminal/i);
    expect(agents).not.toHaveTextContent(/default agent action/i);
    expect(screen.queryByText(/launched by default/i)).not.toBeInTheDocument();
  });

  it("does not expose Quick Commands navigation or section", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    expect(screen.queryByRole("button", { name: "Quick Commands" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Quick Commands" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Enable quick commands")).not.toBeInTheDocument();
  });

  it("renders a non-empty General overview that does not duplicate Appearance controls", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    const overview = screen.getByTestId("settings-general-overview");
    expect(overview).toBeInTheDocument();
    expect(screen.getByLabelText("Confirm before closing a tab")).toBeInTheDocument();
    expect(screen.getByLabelText("Show sidebar on startup")).toBeInTheDocument();
    expect(screen.getByText("Software Update")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "General" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Theme mode")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Accent color")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Interface density")).not.toBeInTheDocument();
    expect(screen.queryByText("Color scheme")).not.toBeInTheDocument();
    expect(screen.queryByText("Density")).not.toBeInTheDocument();
  });

  describe("F-settings-03: deferred hooks when closed", () => {
    it("does not invoke useTerminalSettings or register listeners when open=false", () => {
      native.getTerminalPreferences.mockClear();
      const addEventListenerSpy = vi.spyOn(window, "addEventListener");
      addEventListenerSpy.mockClear();

      render(<SettingsDialog open={false} onClose={vi.fn()} />);

      expect(native.getTerminalPreferences).not.toHaveBeenCalled();
      const registeredTerminalListeners = addEventListenerSpy.mock.calls.filter(
        ([event]) => event === "orca:terminal-settings" || event === "keydown",
      );
      expect(registeredTerminalListeners).toHaveLength(0);

      addEventListenerSpy.mockRestore();
    });

    it("source structure gates useTerminalSettings behind open check via inner component", () => {
      const fs = require("node:fs");
      const path = require("node:path");
      const source = fs.readFileSync(path.resolve(__dirname, "./SettingsDialog.tsx"), "utf8");

      expect(source).toMatch(/export function SettingsDialog\([\s\S]*?\)\s*\{[\s\S]*?if\s*\(!(?:open|props\.open)\)\s*return null;/);
      expect(source).toMatch(/function SettingsDialogBody[\s\S]*?useTerminalSettings\(\)/);
    });
  });
});
