import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
}));

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
});

describe("SettingsDialog", () => {
  it("is a full-view original-Orca-style nav/detail surface", () => {
    render(<SettingsDialog open onClose={vi.fn()} />);

    expect(screen.getByRole("dialog", { name: "Settings" })).toHaveAttribute("aria-modal", "false");
    expect(screen.getByTestId("settings-nav")).toHaveClass("w-[280px]");
    expect(screen.getByRole("button", { name: "Back to app" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "General" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Notifications" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Keyboard Shortcuts" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Workspace" })).toBeInTheDocument();
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

    const pickButton = screen.getByRole("button", { name: /choose|browse|pick|custom/i });
    fireEvent.click(pickButton);

    await waitFor(() => expect(native.pickNotificationAudio).toHaveBeenCalled());
    await waitFor(() => {
      expect(
        screen.queryByText(/bell\.mp3/i) || screen.queryByDisplayValue(/bell\.mp3|\/custom\/bell\.mp3/i)
      ).toBeInTheDocument();
    });
  });
});