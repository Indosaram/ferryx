import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_SETTINGS_EVENT,
  loadAppearanceSettings,
} from "../lib/appearanceSettings";
import { SHORTCUTS, shortcutLabel } from "../lib/shortcuts";
import { TERMINAL_SETTINGS_STORAGE_KEY } from "../lib/terminalSettings";
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
  getTailscaleStatus: vi.fn(),
  detectAgents: vi.fn(),
  getRemoteStatus: vi.fn(),
  enableRemoteGateway: vi.fn(),
  disableRemoteGateway: vi.fn(),
  createPairingCode: vi.fn(),
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

  it("shows the registered shortcuts in the dedicated Keyboard Shortcuts section", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Keyboard Shortcuts" }));

    for (const shortcut of SHORTCUTS) {
      expect(screen.getByText(shortcut.title)).toBeInTheDocument();
      expect(screen.getByText(shortcutLabel(shortcut.id))).toBeInTheDocument();
    }
    expect(screen.getByRole("region", { name: "Keyboard Shortcuts" })).toBeInTheDocument();
  });

  it("navigates to Notifications section and displays toggles, permission status, volume slider, and test button", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));

    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    expect(screen.getByRole("region", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByLabelText(/Enable Notifications/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/Volume/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Send Test Notification/i })).toBeInTheDocument();
    expect(screen.getByText(/authorized/i)).toBeInTheDocument();
  });

  it("toggles Enable Notifications and updates state and localStorage", async () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
    await waitFor(() => expect(native.getNotificationPermissionStatus).toHaveBeenCalled());

    const toggle = screen.getByLabelText(/Enable Notifications/i) as HTMLInputElement;
    const initialChecked = toggle.checked;
    fireEvent.click(toggle);

    expect(toggle.checked).toBe(!initialChecked);
    const storedValues = Object.values(localStorage);
    expect(storedValues.some((val) => val.includes(`"enabled":${!initialChecked}`))).toBe(true);
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

    const soundSelect = screen.queryByLabelText(/sound/i);
    if (soundSelect && soundSelect.tagName === "SELECT") {
      fireEvent.change(soundSelect, { target: { value: "custom" } });
    }

    const pickButton = screen.getByRole("button", { name: /choose|\bbrowse\b|pick|custom/i });
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

    expect(screen.getByText("Ferryx · local desktop")).toBeInTheDocument();
    expect(screen.queryByText(/rorca · local desktop/i)).not.toBeInTheDocument();
    expect(screen.queryByText("Color scheme")).not.toBeInTheDocument();
    expect(screen.queryByText("Density")).not.toBeInTheDocument();
  });

  it("navigates to Appearance section, renders controls, and persists changes to localStorage", () => {
    const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

    expect(screen.getByRole("region", { name: "Appearance" })).toBeInTheDocument();
    const themeSelect = screen.getByLabelText("Theme mode") as HTMLSelectElement;
    const accentSelect = screen.getByLabelText("Accent color") as HTMLSelectElement;
    const densitySelect = screen.getByLabelText("Interface density") as HTMLSelectElement;

    expect(themeSelect.value).toBe("charcoal");
    expect(accentSelect.value).toBe("default");
    expect(densitySelect.value).toBe("compact");

    fireEvent.change(themeSelect, { target: { value: "dark" } });
    fireEvent.change(accentSelect, { target: { value: "emerald" } });
    fireEvent.change(densitySelect, { target: { value: "comfortable" } });

    const stored = JSON.parse(localStorage.getItem("ferryx.settings.appearance")!);
    expect(stored).toEqual({
      theme: "dark",
      accentColor: "emerald",
      density: "comfortable",
    });

    unmount();
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Appearance" }));
    expect((screen.getByLabelText("Theme mode") as HTMLSelectElement).value).toBe("dark");
    expect((screen.getByLabelText("Accent color") as HTMLSelectElement).value).toBe("emerald");
    expect((screen.getByLabelText("Interface density") as HTMLSelectElement).value).toBe("comfortable");

    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect((screen.getByLabelText("Theme mode") as HTMLSelectElement).value).toBe("charcoal");
    expect((screen.getByLabelText("Accent color") as HTMLSelectElement).value).toBe("default");
    expect((screen.getByLabelText("Interface density") as HTMLSelectElement).value).toBe("compact");
    expect(localStorage.getItem("ferryx.settings.appearance")).toBeNull();
  });

  it("navigates to Browser section, changes search engine and zoom, and persists to localStorage", () => {
    const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Browser" }));

    expect(screen.getByRole("region", { name: "Browser" })).toBeInTheDocument();
    const searchSelect = screen.getByLabelText("Default search engine") as HTMLSelectElement;
    const zoomSelect = screen.getByLabelText("Default zoom level") as HTMLSelectElement;
    const restoreToggle = screen.getByLabelText("Restore tabs on launch") as HTMLInputElement;

    expect(searchSelect.value).toBe("duckduckgo");
    expect(zoomSelect.value).toBe("100");
    expect(restoreToggle.checked).toBe(false);

    fireEvent.change(searchSelect, { target: { value: "google" } });
    fireEvent.change(zoomSelect, { target: { value: "125" } });
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
    expect((screen.getByLabelText("Default search engine") as HTMLSelectElement).value).toBe("google");
    expect((screen.getByLabelText("Default zoom level") as HTMLSelectElement).value).toBe("125");
    expect((screen.getByLabelText("Restore tabs on launch") as HTMLInputElement).checked).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Reset to defaults" }));
    expect((screen.getByLabelText("Default search engine") as HTMLSelectElement).value).toBe("duckduckgo");
    expect((screen.getByLabelText("Default zoom level") as HTMLSelectElement).value).toBe("100");
    expect((screen.getByLabelText("Restore tabs on launch") as HTMLInputElement).checked).toBe(false);
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

    const zoomSelect = screen.getByLabelText("Default zoom level") as HTMLSelectElement;
    fireEvent.change(zoomSelect, { target: { value: "125" } });

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
    const claudeToggle = screen.getByLabelText("Enable Claude") as HTMLInputElement;
    expect(claudeToggle.checked).toBe(true);
    fireEvent.click(claudeToggle);
    expect(claudeToggle.checked).toBe(false);

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

  it("persists Appearance through the appearanceSettings API across remount", () => {
    const events: unknown[] = [];
    const listener = (event: Event) => {
      events.push((event as CustomEvent).detail);
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, listener);

    try {
      const { unmount } = render(<SettingsDialog open onClose={vi.fn()} />);
      fireEvent.click(screen.getByRole("button", { name: "Appearance" }));

      fireEvent.change(screen.getByLabelText("Theme mode"), { target: { value: "light" } });
      fireEvent.change(screen.getByLabelText("Accent color"), { target: { value: "blue" } });
      fireEvent.change(screen.getByLabelText("Interface density"), { target: { value: "comfortable" } });

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
      expect((screen.getByLabelText("Theme mode") as HTMLSelectElement).value).toBe("light");
      expect((screen.getByLabelText("Accent color") as HTMLSelectElement).value).toBe("blue");
      expect((screen.getByLabelText("Interface density") as HTMLSelectElement).value).toBe("comfortable");
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

    fireEvent.click(screen.getByRole("button", { name: "Disabled" }));

    const pinButton = await screen.findByTestId("remote-pairing-code");
    expect(pinButton.tagName).toBe("BUTTON");
    expect(pinButton).toHaveTextContent("482916");

    fireEvent.click(pinButton);

    expect(writeText).toHaveBeenCalledWith("482916");
    expect(pinButton).toHaveTextContent(/copied/i);
  });

  it("states that Remote Access stops when Ferryx quits and must be enabled again after relaunch", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Remote Access" }));

    const remote = screen.getByRole("region", { name: "Remote Access" });
    expect(remote).toHaveTextContent(/stops when Ferryx quits/i);
    expect(remote).toHaveTextContent(/enabled again after relaunch/i);
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
    expect(overview.querySelectorAll("li").length).toBeGreaterThanOrEqual(4);
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
