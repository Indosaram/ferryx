import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SystemPermissionsStatus } from "../../lib/types";
import { PermissionsSection } from "./PermissionsSection";

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
  openPermissionsSystemSettings: vi.fn(),
  requestAccessibilityPermission: vi.fn(),
}));

vi.mock("../../lib/tauri", () => ({
  getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
  openPermissionsSystemSettings: (target: string) => mockTauri.openPermissionsSystemSettings(target),
  requestAccessibilityPermission: () => mockTauri.requestAccessibilityPermission(),
}));

const mockStatusNotGranted: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: false,
  fullDiskAccess: {
    status: "denied",
    granted: false,
    canRequest: false,
    description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.",
  },
  accessibility: {
    status: "denied",
    granted: false,
    canRequest: true,
    description: "Allows global keyboard shortcuts, native terminal focus management, and automation.",
  },
  notifications: {
    status: "denied",
    granted: false,
    canRequest: false,
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
    description: "Allows terminal subagents, worktrees, and git tools to read project files without macOS Photo Library or folder access prompts.",
  },
  accessibility: {
    status: "granted",
    granted: true,
    canRequest: false,
    description: "Allows global keyboard shortcuts, native terminal focus management, and automation.",
  },
  notifications: {
    status: "granted",
    granted: true,
    canRequest: false,
    description: "Allows desktop alerts for agent task completions, background builds, and version updates.",
  },
};

describe("PermissionsSection", () => {
  beforeEach(() => {
    mockTauri.getSystemPermissionsStatus.mockReset();
    mockTauri.openPermissionsSystemSettings.mockReset();
    mockTauri.requestAccessibilityPermission.mockReset();
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
});
