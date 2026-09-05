import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  checkAndPromptPermissions,
  loadPermissionsToastDismissed,
  PERMISSIONS_TOAST_ID,
  resetPermissionsToastDismissed,
  savePermissionsToastDismissed,
} from "./permissionsToast";
import type { SystemPermissionsStatus } from "./types";

const mockToast = vi.hoisted(() => ({
  info: vi.fn(),
  warning: vi.fn(),
  dismiss: vi.fn(),
}));

vi.mock("../components/ui/sonner", () => ({
  toast: mockToast,
}));

const mockTauri = vi.hoisted(() => ({
  getSystemPermissionsStatus: vi.fn(),
}));

vi.mock("./tauri", () => ({
  getSystemPermissionsStatus: () => mockTauri.getSystemPermissionsStatus(),
}));

function createMockStorage(): Storage {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, val: string) => {
      store.set(key, val);
    },
    removeItem: (key: string) => {
      store.delete(key);
    },
    clear: () => store.clear(),
    key: () => null,
    length: store.size,
  };
}

const mockStatusNeedsPermissions: SystemPermissionsStatus = {
  platform: "macos",
  allGranted: false,
  fullDiskAccess: {
    status: "denied",
    granted: false,
    canRequest: false,
    description: "Full disk access needed.",
  },
  accessibility: {
    status: "denied",
    granted: false,
    canRequest: true,
    description: "Accessibility needed.",
  },
  notifications: {
    status: "denied",
    granted: false,
    canRequest: true,
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
    description: "Full disk access granted.",
  },
  accessibility: {
    status: "granted",
    granted: true,
    canRequest: false,
    description: "Accessibility granted.",
  },
  notifications: {
    status: "granted",
    granted: true,
    canRequest: false,
    description: "Notifications granted.",
  },
};

describe("permissionsToast", () => {
  beforeEach(() => {
    mockToast.info.mockReset();
    mockToast.warning.mockReset();
    mockToast.dismiss.mockReset();
    mockTauri.getSystemPermissionsStatus.mockReset();
  });

  it("loads and saves dismissed state in storage", () => {
    const storage = createMockStorage();
    expect(loadPermissionsToastDismissed(storage)).toBe(false);

    savePermissionsToastDismissed(storage);
    expect(loadPermissionsToastDismissed(storage)).toBe(true);

    resetPermissionsToastDismissed(storage);
    expect(loadPermissionsToastDismissed(storage)).toBe(false);
  });

  it("shows toast when permissions are missing and not dismissed", async () => {
    const storage = createMockStorage();
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNeedsPermissions);

    const onOpenSettings = vi.fn();
    const prompted = await checkAndPromptPermissions({ onOpenSettings, storage });

    expect(prompted).toBe(true);
    expect(mockToast.warning).toHaveBeenCalled();
    const callArgs = mockToast.warning.mock.calls[0];
    expect(typeof callArgs[0]).toBe("string");
    expect(callArgs[0].length).toBeGreaterThan(0);
    expect(typeof callArgs[1].action?.label).toBe("string");
    expect(typeof callArgs[1].cancel?.label).toBe("string");

    callArgs[1].action.onClick();
    expect(onOpenSettings).toHaveBeenCalled();
    expect(loadPermissionsToastDismissed(storage)).toBe(false);

    callArgs[1].cancel.onClick();
    expect(loadPermissionsToastDismissed(storage)).toBe(true);
  });

  it("does not prompt when already dismissed unless force is true", async () => {
    const storage = createMockStorage();
    savePermissionsToastDismissed(storage);
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusNeedsPermissions);

    const onOpenSettings = vi.fn();
    const prompted = await checkAndPromptPermissions({ onOpenSettings, storage });
    expect(prompted).toBe(false);
    expect(mockToast.warning).not.toHaveBeenCalled();

    const forced = await checkAndPromptPermissions({ onOpenSettings, storage, force: true });
    expect(forced).toBe(true);
    expect(mockToast.warning).toHaveBeenCalled();
  });

  it("does not prompt when all permissions are granted", async () => {
    const storage = createMockStorage();
    mockTauri.getSystemPermissionsStatus.mockResolvedValue(mockStatusAllGranted);

    const onOpenSettings = vi.fn();
    const prompted = await checkAndPromptPermissions({ onOpenSettings, storage });
    expect(prompted).toBe(false);
    expect(mockToast.warning).not.toHaveBeenCalled();
    expect(mockToast.dismiss).toHaveBeenCalledWith(PERMISSIONS_TOAST_ID);
  });
});
