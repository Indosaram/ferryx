import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

const savedProcessPlatform = process.platform;
const savedPlatform = Object.getOwnPropertyDescriptor(window.navigator, "platform");
const savedUserAgent = Object.getOwnPropertyDescriptor(window.navigator, "userAgent");

async function renderStatus(status: SystemPermissionsStatus) {
  Object.defineProperty(process, "platform", {
    value: status.platform === "macos" ? "darwin" : "win32",
  });
  Object.defineProperty(window.navigator, "platform", {
    value: status.platform === "macos" ? "MacIntel" : "Win32", configurable: true,
  });
  Object.defineProperty(window.navigator, "userAgent", {
    value: status.platform === "macos" ? "Macintosh" : "Windows NT 10.0", configurable: true,
  });
  // Subscribe to the exact IPC request before mounting; Vitest bounds the await.
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
  });

  afterEach(() => {
    cleanup();
    Object.defineProperty(process, "platform", { value: savedProcessPlatform });
    if (savedPlatform) Object.defineProperty(window.navigator, "platform", savedPlatform);
    else Reflect.deleteProperty(window.navigator, "platform");
    if (savedUserAgent) Object.defineProperty(window.navigator, "userAgent", savedUserAgent);
    else Reflect.deleteProperty(window.navigator, "userAgent");
  });

  it("renders applicable grant advice when macOS permissions are denied", async () => {
    // Given / When: mount with denied macOS permissions from OS IPC.
    await renderStatus(mockStatusNotGranted);

    // Then: applicable advice and grant controls remain available.
    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.getByTestId("open-fda-settings")).toBeEnabled();
    expect(screen.getByTestId("open-accessibility-settings")).toBeEnabled();
    expect(screen.getByText("Accessibility")).toBeDefined();
    expect(screen.getByText("Desktop Notifications")).toBeDefined();
    expect(screen.getAllByText("Required").length).toBeGreaterThanOrEqual(1);
  });

  it("triggers open system settings when clicking open settings buttons", async () => {
    // Given
    mockTauri.openPermissionsSystemSettings.mockResolvedValue({ opened: true, target: "full_disk_access" });
    await renderStatus(mockStatusNotGranted);

    // When
    await act(async () => fireEvent.click(screen.getByTestId("open-fda-settings")));
    // Then
    expect(mockTauri.openPermissionsSystemSettings).toHaveBeenCalledWith("full_disk_access");
  });

  it("renders all granted status correctly", async () => {
    // Given / When
    await renderStatus(mockStatusAllGranted);

    // Then
    expect(screen.getAllByText("Granted").length).toBe(3);
    expect(screen.getByRole("alert")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("requests notification permission and refreshes when Enable Notifications is clicked", async () => {
    // Given
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

    // When
    await act(async () => {
      fireEvent.click(screen.getByTestId("request-notifications"));
      await refreshed;
    });

    // Then
    expect(mockTauri.requestNotificationPermission).toHaveBeenCalledTimes(1);
    expect(mockTauri.getSystemPermissionsStatus).toHaveBeenCalledTimes(2);
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("renders Windows notifications-only surface with OS-managed badge", async () => {
    // Given / When: non-authoritative Windows capabilities arrive through IPC.
    await renderStatus(mockStatusWindows);

    // Then: no macOS advice or grant actions, but OS settings remain available.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByTestId("open-fda-settings")).toBeNull();
    expect(screen.queryByTestId("open-accessibility-settings")).toBeNull();
    expect(screen.queryByTestId("rerun-permissions-onboarding")).toBeNull();
    expect(screen.queryByText("Full Disk Access")).toBeNull();
    expect(screen.queryByText("Accessibility")).toBeNull();
    expect(screen.getByText("Managed by OS")).toBeDefined();
    expect(screen.getByTestId("open-notifications-settings")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("resets dismissed key and dispatches open-onboarding event on Re-run Welcome Setup", async () => {
    // Given
    const nav = window.navigator;
    const savedPlatform = Object.getOwnPropertyDescriptor(nav, "platform");
    const savedUserAgent = Object.getOwnPropertyDescriptor(nav, "userAgent");
    Object.defineProperty(nav, "platform", { value: "MacIntel", configurable: true });
    Object.defineProperty(nav, "userAgent", {
      value: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
      configurable: true,
    });
    try {
      window.localStorage.setItem("ferryx.permissions.onboarding-dismissed", "true");
      const onEvent = vi.fn();
      window.addEventListener("ferryx:open-permissions-onboarding", onEvent, { once: true });

      await renderStatus(mockStatusNotGranted);

      // When
      fireEvent.click(screen.getByTestId("rerun-permissions-onboarding"));

      // Then
      expect(window.localStorage.getItem("ferryx.permissions.onboarding-dismissed")).toBeNull();
      expect(onEvent).toHaveBeenCalledTimes(1);
    } finally {
      if (savedPlatform) Object.defineProperty(nav, "platform", savedPlatform);
      else Reflect.deleteProperty(nav, "platform");
      if (savedUserAgent) Object.defineProperty(nav, "userAgent", savedUserAgent);
      else Reflect.deleteProperty(nav, "userAgent");
    }
  });
});
