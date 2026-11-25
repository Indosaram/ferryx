import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { OPEN_ONBOARDING_EVENT } from "../../lib/onboarding";
import { ONBOARDING_STORAGE_KEY } from "../../lib/storageKeys";
import type { SystemPermissionsStatus } from "../../lib/types";
import { PermissionsSection } from "./PermissionsSection";

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
  openPermissionsSystemSettings: vi.fn(),
  requestAccessibilityPermission: vi.fn(),
  requestNotificationPermission: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  openPermissionsSystemSettings: (target: string) => mockTauri.openPermissionsSystemSettings(target),
  requestAccessibilityPermission: () => mockTauri.requestAccessibilityPermission(),
  requestNotificationPermission: () => mockTauri.requestNotificationPermission(),
}));

const mockStatusNotGranted: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: false,
  fullDiskAccess: {
    status: "denied",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.",
  },
  accessibility: {
    status: "denied",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Allows global keyboard shortcuts, native terminal focus management, and automation.",
  },
  notifications: {
    status: "denied",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Allows desktop alerts for agent task completions, background builds, and version updates.",
  },
};

const mockStatusAllGranted: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: true,
  fullDiskAccess: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.",
  },
  accessibility: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Allows global keyboard shortcuts, native terminal focus management, and automation.",
  },
  notifications: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Allows desktop alerts for agent task completions, background builds, and version updates.",
  },
};

const mockStatusWindows: SystemPermissionsStatus = {
  platform: "windows",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  notifications: {
    status: "unknown",
    granted: false,
    canRequest: false,
    canOpenSettings: true,
    description: "Windows manages per-app notification access in Settings > System > Notifications.",
  },
};

const mockStatusLinux: SystemPermissionsStatus = {
  platform: "linux",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  notifications: {
    status: "unknown",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Desktop notifications are managed by the desktop environment; most setups need no per-app grant.",
  },
};

const mockStatusWeb: SystemPermissionsStatus = {
  platform: "web",
  allGranted: false,
  fullDiskAccess: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  accessibility: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
  notifications: {
    status: "unsupported",
    granted: false,
    canRequest: false,
    canOpenSettings: false,
    description: "Permissions are managed by the host desktop application.",
  },
};

const savedProcessPlatform = process.platform;
const savedPlatform = Object.getOwnPropertyDescriptor(window.navigator, "platform");
const savedUserAgent = Object.getOwnPropertyDescriptor(window.navigator, "userAgent");

async function renderStatus(status: SystemPermissionsStatus, browserPlatform: string = status.platform) {
  Object.defineProperty(process, "platform", {
    value: browserPlatform === "macos" ? "darwin" : "win32",
  });
  Object.defineProperty(window.navigator, "platform", {
    value: browserPlatform === "macos" ? "MacIntel" : "Win32", configurable: true,
  });
  Object.defineProperty(window.navigator, "userAgent", {
    value: browserPlatform === "macos" ? "Macintosh" : "Windows NT 10.0", configurable: true,
  });
  const ready = new Promise<void>((resolve) => {
    mockTauri.getSystemPermissionsStatus.mockImplementation(() => {
      resolve();
      return Promise.resolve(status);
    });
  });
  render(<PermissionsSection />);
  await act(async () => {
    await ready;
  });
}

describe("PermissionsSection", () => {
  beforeEach(() => {
    mockTauri.getSystemPermissionsStatus.mockReset();
    mockTauri.openPermissionsSystemSettings.mockReset();
    mockTauri.requestAccessibilityPermission.mockReset();
    mockTauri.requestNotificationPermission.mockReset();
    window.localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    window.localStorage.clear();
    Object.defineProperty(process, "platform", { value: savedProcessPlatform });
    if (savedPlatform) Object.defineProperty(window.navigator, "platform", savedPlatform);
    else Reflect.deleteProperty(window.navigator, "platform");
    if (savedUserAgent) Object.defineProperty(window.navigator, "userAgent", savedUserAgent);
    else Reflect.deleteProperty(window.navigator, "userAgent");
  });

  it("renders applicable grant advice when macOS permissions are denied", async () => {
    await renderStatus(mockStatusNotGranted);

    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.getByTestId("open-fda-settings")).toBeEnabled();
    expect(screen.getByTestId("open-accessibility-settings")).toBeEnabled();
    expect(screen.getByText("Accessibility")).toBeDefined();
    expect(screen.getByText("Desktop Notifications")).toBeDefined();
    expect(screen.getAllByText("Required").length).toBeGreaterThanOrEqual(1);
  });

  it("triggers open system settings when clicking open settings buttons", async () => {
    mockTauri.openPermissionsSystemSettings.mockResolvedValue({ opened: true, target: "full_disk_access" });
    await renderStatus(mockStatusNotGranted);

    await act(async () => fireEvent.click(screen.getByTestId("open-fda-settings")));
    expect(mockTauri.openPermissionsSystemSettings).toHaveBeenCalledWith("full_disk_access");
  });

  it("renders all granted status correctly", async () => {
    await renderStatus(mockStatusAllGranted);

    expect(screen.getAllByText("Granted").length).toBe(3);
    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("requests notification permission and refreshes when Enable Notifications is clicked", async () => {
    const canRequestStatus: SystemPermissionsStatus = {
      ...mockStatusNotGranted,
      notifications: {
        ...mockStatusNotGranted.notifications,
        canRequest: true,
        granted: false,
      },
    };
    mockTauri.requestNotificationPermission.mockResolvedValue({ granted: true, status: "granted" });
    await renderStatus(canRequestStatus);
    const refreshed = new Promise<void>((resolve) => {
      mockTauri.getSystemPermissionsStatus.mockImplementationOnce(() => {
        resolve();
        return Promise.resolve(mockStatusAllGranted);
      });
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("request-notifications"));
      await refreshed;
    });

    expect(mockTauri.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(mockTauri.getSystemPermissionsStatus).toHaveBeenCalledTimes(2);
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("replaces Enable Notifications with Open System Settings after a denial refresh", async () => {
    const canRequestStatus: SystemPermissionsStatus = {
      ...mockStatusNotGranted,
      notifications: {
        ...mockStatusNotGranted.notifications,
        status: "not_determined",
        canRequest: true,
        granted: false,
      },
    };
    const deniedStatus: SystemPermissionsStatus = {
      ...canRequestStatus,
      notifications: {
        ...canRequestStatus.notifications,
        status: "denied",
        canRequest: false,
        granted: false,
      },
    };
    mockTauri.requestNotificationPermission.mockResolvedValue({ granted: false });
    await renderStatus(canRequestStatus);
    expect(screen.getByTestId("request-notifications")).toBeDefined();

    mockTauri.getSystemPermissionsStatus.mockResolvedValue(deniedStatus);
    await act(async () => {
      fireEvent.click(screen.getByTestId("request-notifications"));
    });

    expect(await screen.findByTestId("open-notifications-settings")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("shows the structured notification error instead of clearing status", async () => {
    const canRequestStatus: SystemPermissionsStatus = {
      ...mockStatusNotGranted,
      notifications: {
        ...mockStatusNotGranted.notifications,
        status: "not_determined",
        canRequest: true,
        granted: false,
      },
    };
    mockTauri.requestNotificationPermission.mockResolvedValue({
      granted: false,
      error: "notifications require a bundled .app",
    });
    await renderStatus(canRequestStatus);

    await act(async () => {
      fireEvent.click(screen.getByTestId("request-notifications"));
    });

    expect(await screen.findByText("notifications require a bundled .app")).toBeDefined();
    expect(screen.getByText("Desktop Notifications")).toBeDefined();
  });

  it("renders Windows notifications-only surface with OS-managed badge and Show Welcome Setup", async () => {
    await renderStatus(mockStatusWindows);

    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByTestId("open-fda-settings")).toBeNull();
    expect(screen.queryByTestId("open-accessibility-settings")).toBeNull();
    expect(screen.getByTestId("rerun-permissions-onboarding")).toBeDefined();
    expect(screen.getByRole("button", { name: "Show Welcome Setup" })).toBeDefined();
    expect(screen.queryByText("Full Disk Access")).toBeNull();
    expect(screen.queryByText("Accessibility")).toBeNull();
    expect(screen.getByText("Managed by OS")).toBeDefined();
    expect(screen.getByTestId("open-notifications-settings")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("renders Linux notifications-only surface without open button and with Show Welcome Setup", async () => {
    await renderStatus(mockStatusLinux);

    expect(screen.queryByTestId("open-notifications-settings")).toBeNull();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(/Configure macOS permissions/)).toBeNull();
    expect(screen.getByTestId("rerun-permissions-onboarding")).toBeDefined();
    expect(screen.getByRole("button", { name: "Show Welcome Setup" })).toBeDefined();
  });

  it("hides Show Welcome Setup when platform is web", async () => {
    await renderStatus(mockStatusWeb);

    expect(screen.queryByTestId("rerun-permissions-onboarding")).toBeNull();
    expect(screen.queryByRole("button", { name: "Show Welcome Setup" })).toBeNull();
  });

  it("follows the host platform, not the browser OS, for macOS-only copy and controls", async () => {
    await renderStatus(mockStatusWindows, "macos");

    expect(screen.queryByText(/Configure macOS permissions/)).toBeNull();
    expect(screen.getByText(/Windows and Linux manage these permissions at the OS level/)).toBeDefined();
    expect(screen.queryByText("How to grant permissions in macOS:")).toBeNull();
    expect(screen.getByTestId("rerun-permissions-onboarding")).toBeDefined();
    expect(screen.getByRole("button", { name: "Show Welcome Setup" })).toBeDefined();
  });

  it("resets onboarding storage key and dispatches ferryx:open-onboarding event on Show Welcome Setup", async () => {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify({ version: 1, completedSteps: ["intro"], dismissed: true }));
    // A dismissed setup panel must come back with the wizard, or the dismissal is permanent.
    window.localStorage.setItem("ferryx.hints.setupProgress", "dismissed");
    const onEvent = vi.fn();
    window.addEventListener(OPEN_ONBOARDING_EVENT, onEvent, { once: true });

    await renderStatus(mockStatusNotGranted);

    const button = screen.getByTestId("rerun-permissions-onboarding");
    expect(button.textContent).toBe("Show Welcome Setup");
    fireEvent.click(button);

    expect(window.localStorage.getItem(ONBOARDING_STORAGE_KEY)).toBeNull();
    expect(window.localStorage.getItem("ferryx.hints.setupProgress")).toBeNull();
    expect(onEvent).toHaveBeenCalledTimes(1);
  });
});
