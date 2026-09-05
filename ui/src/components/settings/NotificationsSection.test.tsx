import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  NOTIFICATION_SETTINGS_EVENT,
  loadNotificationSettings,
  resetNotificationSettings,
  saveNotificationSettings,
} from "../../lib/notificationSettings";
import { NotificationsSection } from "./NotificationsSection";

const native = vi.hoisted(() => ({
  getNotificationPermissionStatus: vi.fn(),
  openNotificationSystemSettings: vi.fn(),
  pickNotificationAudio: vi.fn(),
  playNotificationSound: vi.fn(),
  probeNotificationDelivery: vi.fn(),
  requestNotificationPermission: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  getNotificationPermissionStatus: native.getNotificationPermissionStatus,
  openNotificationSystemSettings: native.openNotificationSystemSettings,
  pickNotificationAudio: native.pickNotificationAudio,
  playNotificationSound: native.playNotificationSound,
  probeNotificationDelivery: native.probeNotificationDelivery,
  requestNotificationPermission: native.requestNotificationPermission,
}));

describe("NotificationsSection", () => {
  beforeEach(() => {
    resetNotificationSettings();
    vi.clearAllMocks();
    native.getNotificationPermissionStatus.mockResolvedValue({ authorization: "authorized" });
  });

  afterEach(() => {
    cleanup();
    resetNotificationSettings();
  });

  it("never paints non-authoritative permission as authorized and refreshes on return", async () => {
    native.getNotificationPermissionStatus.mockResolvedValue({ authorization: "authorized", authoritative: false, supported: true });
    await act(async () => { render(<NotificationsSection />); });
    expect(screen.getByTestId("notification-permission-status")).toHaveAttribute("data-permission-state", "unknown");
    native.getNotificationPermissionStatus.mockResolvedValue({ authorization: "denied", authoritative: true, supported: true });
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(native.getNotificationPermissionStatus).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId("notification-permission-status")).toHaveAttribute("data-permission-state", "denied");
  });

  it("lets an already-authorized user re-request permission without triggering it on mount or focus", async () => {
    native.getNotificationPermissionStatus.mockResolvedValue({ authorization: "authorized", authoritative: true, supported: true });
    await act(async () => { render(<NotificationsSection />); });

    expect(screen.getByTestId("notification-permission-status")).toHaveAttribute("data-permission-state", "authorized");
    const requestButton = screen.getByRole("button", { name: /request permission/i });
    expect(requestButton).toBeInTheDocument();

    expect(native.requestNotificationPermission).not.toHaveBeenCalled();
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(native.requestNotificationPermission).not.toHaveBeenCalled();

    const statusCallsBeforeClick = native.getNotificationPermissionStatus.mock.calls.length;
    await act(async () => { fireEvent.click(requestButton); });

    expect(native.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(native.getNotificationPermissionStatus.mock.calls.length).toBe(statusCallsBeforeClick + 1);
  });

  it("offers no request permission action when notifications are unavailable on this platform", async () => {
    native.getNotificationPermissionStatus.mockResolvedValue({ authorization: "authorized", authoritative: true, supported: false });
    await act(async () => { render(<NotificationsSection />); });

    expect(screen.getByTestId("notification-permission-status")).toHaveAttribute("data-permission-state", "unavailable");
    expect(screen.queryByRole("button", { name: /request permission/i })).not.toBeInTheDocument();
    expect(native.requestNotificationPermission).not.toHaveBeenCalled();
  });

  it.each([
    ["system", "system"], ["none", "silent"], ["custom", "silent"],
  ])("passes the selected %s sound mode to the native test notification", async (customSoundId, sound) => {
    saveNotificationSettings({ enabled: true, customSoundId, customSoundPath: "/retained.wav" });
    native.probeNotificationDelivery.mockResolvedValue({ outcome: "submitted", testSubmitted: true });
    await act(async () => { render(<NotificationsSection />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: /send test notification/i })); });
    expect(native.probeNotificationDelivery).toHaveBeenCalledWith(true, sound);
  });

  it("disables send test notification button when notifications are disabled", async () => {
    saveNotificationSettings({ enabled: false });

    render(<NotificationsSection />);

    const sendButton = await screen.findByRole("button", { name: /send test notification/i });
    expect(sendButton).toBeDisabled();
  });

  it("disables browse and preview buttons when notifications are disabled with custom sound", async () => {
    saveNotificationSettings({
      enabled: false,
      customSoundId: "custom",
      customSoundPath: "/path/to/sound.mp3",
    });

    render(<NotificationsSection />);

    const browseButton = await screen.findByRole("button", { name: /browse/i });
    const previewButton = await screen.findByRole("button", { name: /preview/i });

    expect(browseButton).toBeDisabled();
    expect(previewButton).toBeDisabled();
  });

  it("reports why a test notification did not appear when the OS withholds permission", async () => {
    saveNotificationSettings({ enabled: true });
    native.probeNotificationDelivery.mockResolvedValue({
      outcome: "permission-required",
      testSubmitted: false,
    });

    render(<NotificationsSection />);

    fireEvent.click(await screen.findByRole("button", { name: /send test notification/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent(/permission/i);
    });
  });

  it("reports a blocked notification instead of silently doing nothing", async () => {
    saveNotificationSettings({ enabled: true });
    native.probeNotificationDelivery.mockResolvedValue({
      outcome: "blocked-by-system",
      testSubmitted: false,
    });

    render(<NotificationsSection />);

    fireEvent.click(await screen.findByRole("button", { name: /send test notification/i }));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent(/blocked/i);
    });
  });

  it("does not play the backend sound for the built-in system sound", async () => {
    saveNotificationSettings({ enabled: true, customSoundId: "system" });
    native.probeNotificationDelivery.mockResolvedValue({
      outcome: "submitted",
      testSubmitted: true,
    });

    render(<NotificationsSection />);

    fireEvent.click(await screen.findByRole("button", { name: /send test notification/i }));

    await waitFor(() => expect(native.probeNotificationDelivery).toHaveBeenCalled());
    expect(native.playNotificationSound).not.toHaveBeenCalled();
  });

  it("displays warning when custom sound is selected without a file chosen", async () => {
    saveNotificationSettings({
      enabled: true,
      customSoundId: "custom",
      customSoundPath: null,
    });

    render(<NotificationsSection />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Choose an audio file to enable the custom sound.");
  });

  it("persists attentionFrame and fires the settings event when toggling the pane attention border switch", async () => {
    saveNotificationSettings({ enabled: true, attentionFrame: true });

    let eventFiredWith: any = null;
    const listener = (e: Event) => {
      eventFiredWith = (e as CustomEvent).detail;
    };
    window.addEventListener(NOTIFICATION_SETTINGS_EVENT, listener);

    try {
      render(<NotificationsSection />);

      const toggle = await screen.findByRole("switch", { name: /pane attention border/i });
      expect(toggle).toBeInTheDocument();
      expect(toggle).toHaveAttribute("data-state", "checked");

      fireEvent.click(toggle);

      expect(loadNotificationSettings().attentionFrame).toBe(false);
      expect(eventFiredWith).not.toBeNull();
      expect(eventFiredWith.attentionFrame).toBe(false);
    } finally {
      window.removeEventListener(NOTIFICATION_SETTINGS_EVENT, listener);
    }
  });
});
