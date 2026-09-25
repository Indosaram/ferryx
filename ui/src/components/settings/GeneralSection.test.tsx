import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SoftwareUpdateCard } from "./GeneralSection";

const updater = vi.hoisted(() => ({
  checkForUpdate: vi.fn(),
  downloadAndInstallUpdate: vi.fn(),
  getCurrentVersion: vi.fn(),
  getUpdateStatus: vi.fn(),
  relaunchApp: vi.fn(),
  subscribeUpdateStatus: vi.fn(),
  updatesManagedExternally: vi.fn(),
}));

vi.mock("../../lib/updater", () => ({
  checkForUpdate: () => updater.checkForUpdate(),
  downloadAndInstallUpdate: () => updater.downloadAndInstallUpdate(),
  getCurrentVersion: () => updater.getCurrentVersion(),
  getUpdateStatus: () => updater.getUpdateStatus(),
  relaunchApp: () => updater.relaunchApp(),
  subscribeUpdateStatus: (listener: unknown) => updater.subscribeUpdateStatus(listener),
  updatesManagedExternally: () => updater.updatesManagedExternally(),
}));

vi.mock("../../lib/tauri", () => ({
  getCliLauncherStatus: vi.fn(),
  installCliLauncher: vi.fn(),
}));

const WINDOWS_USER_AGENT =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36 Edg/128.0.0.0";
const LINUX_USER_AGENT =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const MAC_USER_AGENT =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";

/**
 * The updater reports one "externally managed" boolean for both the Microsoft Store and the
 * Linux distribution packages, so the card has to read the host platform to name the owner
 * that install actually has. These values are what the desktop webview reports per host.
 */
function stubHostPlatform(userAgent: string, platform: string) {
  Object.defineProperty(window.navigator, "userAgent", { value: userAgent, configurable: true });
  Object.defineProperty(window.navigator, "platform", { value: platform, configurable: true });
}

describe("SoftwareUpdateCard externally managed installs", () => {
  const originalUserAgent = window.navigator.userAgent;
  const originalPlatform = window.navigator.platform;

  beforeEach(() => {
    updater.getUpdateStatus.mockReturnValue({ state: "idle" });
    updater.subscribeUpdateStatus.mockReturnValue(() => undefined);
    updater.getCurrentVersion.mockResolvedValue("2026.924.1");
    updater.updatesManagedExternally.mockResolvedValue(true);
  });

  afterEach(() => {
    cleanup();
    stubHostPlatform(originalUserAgent, originalPlatform);
  });

  it("names the system package manager on a Linux package install, never the Microsoft Store", async () => {
    stubHostPlatform(LINUX_USER_AGENT, "Linux x86_64");

    render(<SoftwareUpdateCard />);

    const message = await screen.findByText(/Updates are managed by your system package manager\./);
    expect(message.textContent).toBe(
      "Current version 2026.924.1. Updates are managed by your system package manager.",
    );
    expect(message.textContent).not.toContain("Microsoft Store");
  });

  it("still names the Microsoft Store on a Windows Store install", async () => {
    stubHostPlatform(WINDOWS_USER_AGENT, "Win32");

    render(<SoftwareUpdateCard />);

    const message = await screen.findByText(/Updates are managed by the Microsoft Store\./);
    expect(message.textContent).toBe(
      "Current version 2026.924.1. Updates are managed by the Microsoft Store.",
    );
  });

  it("keeps a neutral message when the host platform cannot be determined", async () => {
    stubHostPlatform(MAC_USER_AGENT, "MacIntel");

    render(<SoftwareUpdateCard />);

    const message = await screen.findByText(/Updates for this install are managed outside the app\./);
    expect(message.textContent).not.toContain("Microsoft Store");
  });
});
