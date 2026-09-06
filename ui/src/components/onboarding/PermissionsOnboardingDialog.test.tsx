import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SystemPermissionsStatus } from "../../lib/types";
import { PermissionsOnboardingDialog } from "./PermissionsOnboardingDialog";

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
    description: "Full disk access needed.",
  },
  accessibility: {
    status: "denied",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Accessibility needed.",
  },
  notifications: {
    status: "denied",
    granted: false,
    canRequest: true,
    canOpenSettings: true,
    description: "Notifications needed.",
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
    description: "Full disk access granted.",
  },
  accessibility: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Accessibility granted.",
  },
  notifications: {
    status: "granted",
    granted: true,
    canRequest: false,
    canOpenSettings: true,
    description: "Notifications granted.",
  },
};

describe("PermissionsOnboardingDialog", () => {
  beforeEach(() => {
    mockTauri.getSystemPermissionsStatus.mockReset();
    mockTauri.openPermissionsSystemSettings.mockReset();
    mockTauri.requestAccessibilityPermission.mockReset();
    mockTauri.requestNotificationPermission.mockReset();
  });

  afterEach(() => {
    cleanup();
  });

  it("renders the three permission rows and footer buttons when permissions missing", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);

    render(<PermissionsOnboardingDialog open onClose={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByText("Full Disk Access")).toBeDefined();
    });
    expect(screen.getByText("Accessibility")).toBeDefined();
    expect(screen.getByText("Notifications")).toBeDefined();
    expect(screen.getByTestId("onboarding-dont-show-again")).toBeDefined();
    expect(screen.getByTestId("onboarding-remind-later")).toBeDefined();
  });

  it("calls onClose(true) when clicking Don't show again", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);
    const onClose = vi.fn();

    render(<PermissionsOnboardingDialog open onClose={onClose} />);

    const btn = await screen.findByTestId("onboarding-dont-show-again");
    fireEvent.click(btn);
    expect(onClose).toHaveBeenCalledWith(true);
  });

  it("calls onClose(false) when clicking Remind Me Later", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNotGranted);
    const onClose = vi.fn();

    render(<PermissionsOnboardingDialog open onClose={onClose} />);

    const btn = await screen.findByTestId("onboarding-remind-later");
    fireEvent.click(btn);
    expect(onClose).toHaveBeenCalledWith(false);
  });

  it("renders Get Started and calls onClose(true) when all granted", async () => {
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusAllGranted);
    const onClose = vi.fn();

    render(<PermissionsOnboardingDialog open onClose={onClose} />);

    const btn = await screen.findByTestId("onboarding-get-started");
    fireEvent.click(btn);
    expect(onClose).toHaveBeenCalledWith(true);
  });
});
