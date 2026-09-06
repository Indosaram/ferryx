import { describe, expect, it } from "vitest";

import {
  loadPermissionsOnboardingDismissed,
  resetPermissionsOnboardingDismissed,
  savePermissionsOnboardingDismissed,
  shouldShowPermissionsOnboarding,
} from "./permissionsOnboarding";
import type { SystemPermissionsStatus } from "./types";

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
  ...mockStatusNeedsPermissions,
  allGranted: true,
};

const mockStatusWeb: SystemPermissionsStatus = {
  ...mockStatusNeedsPermissions,
  platform: "web",
};

describe("permissionsOnboarding", () => {
  it("loads, saves, and resets dismissed state in storage", () => {
    const storage = createMockStorage();
    expect(loadPermissionsOnboardingDismissed(storage)).toBe(false);

    savePermissionsOnboardingDismissed(storage);
    expect(loadPermissionsOnboardingDismissed(storage)).toBe(true);

    resetPermissionsOnboardingDismissed(storage);
    expect(loadPermissionsOnboardingDismissed(storage)).toBe(false);
  });

  it("is null-safe when storage is null", () => {
    expect(loadPermissionsOnboardingDismissed(null)).toBe(false);
    expect(() => savePermissionsOnboardingDismissed(null)).not.toThrow();
    expect(() => resetPermissionsOnboardingDismissed(null)).not.toThrow();
  });

  describe("shouldShowPermissionsOnboarding", () => {
    it("returns false when dismissed", () => {
      expect(shouldShowPermissionsOnboarding(mockStatusNeedsPermissions, true)).toBe(false);
    });

    it("returns false when status is null", () => {
      expect(shouldShowPermissionsOnboarding(null, false)).toBe(false);
    });

    it("returns false when all granted", () => {
      expect(shouldShowPermissionsOnboarding(mockStatusAllGranted, false)).toBe(false);
    });

    it("returns false when platform is not macos", () => {
      expect(shouldShowPermissionsOnboarding(mockStatusWeb, false)).toBe(false);
    });

    it("returns true on macos with missing permissions and not dismissed", () => {
      expect(shouldShowPermissionsOnboarding(mockStatusNeedsPermissions, false)).toBe(true);
    });
  });
});
