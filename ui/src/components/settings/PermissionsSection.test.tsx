import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

describe("PermissionsSection", () => {
  beforeEach(() => {
    mockTauri.getSystemPermissionsStatus.mockReset();
    mockTauri.openPermissionsSystemSettings.mockReset();
    mockTauri.requestAccessibilityPermission.mockReset();
    mockTauri.requestNotificationPermission.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders permission items and shows Photo Library guidance for FDA", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getByText("Full Disk Access")).toBeDefined();
    });

    expect(screen.getAllByText(/Photo Library/i).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Accessibility")).toBeDefined();
    expect(screen.getByText("Desktop Notifications")).toBeDefined();
    expect(screen.getAllByText("Required").length).toBeGreaterThanOrEqual(1);
  });

  it("triggers open system settings when clicking open settings buttons", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);
    mockTauri.openPermissionsSystemSettings.mockResolvedValue({ opened: true, target: "full_disk_access" });

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getByTestId("open-fda-settings")).toBeDefined();
    });

    fireEvent.click(screen.getByTestId("open-fda-settings"));
    expect(mockTauri.openPermissionsSystemSettings).toHaveBeenCalledWith("full_disk_access");
  });

  it("renders all granted status correctly", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusAllGranted);

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getAllByText("Granted").length).toBe(3);
    });

    expect(screen.getByText(/All system permissions granted/i)).toBeDefined();
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
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(canRequestStatus);
    mockTauri.requestNotificationPermission.mockResolvedValue({ granted: true, status: "granted" });

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getByTestId("request-notifications")).toBeDefined();
    });

    fireEvent.click(screen.getByTestId("request-notifications"));

    await waitFor(() => {
      expect(mockTauri.requestNotificationPermission).toHaveBeenCalledTimes(1);
    });
    // Initial load + refetch after request.
    expect(mockTauri.getSystemPermissionsStatus.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("renders Windows notifications-only surface with OS-managed badge", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusWindows);

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getByText("Desktop Notifications")).toBeDefined();
    });

    expect(screen.queryByText("Full Disk Access")).toBeNull();
    expect(screen.queryByText("Accessibility")).toBeNull();
    expect(screen.getByText("Managed by OS")).toBeDefined();
    expect(screen.getByTestId("open-notifications-settings")).toBeDefined();
    expect(screen.queryByTestId("request-notifications")).toBeNull();
  });

  it("resets dismissed key and dispatches open-onboarding event on Re-run Welcome Setup", async () => {
    window.localStorage.setItem("ferryx.permissions.onboarding-dismissed", "true");
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);

    const onEvent = vi.fn();
    window.addEventListener("ferryx:open-permissions-onboarding", onEvent, { once: true });

    render(<PermissionsSection />);

    await waitFor(() => {
      expect(screen.getByTestId("rerun-permissions-onboarding")).toBeDefined();
    });

    fireEvent.click(screen.getByTestId("rerun-permissions-onboarding"));

    expect(window.localStorage.getItem("ferryx.permissions.onboarding-dismissed")).toBeNull();
    expect(onEvent).toHaveBeenCalledTimes(1);
  });
});
