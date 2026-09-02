import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  handleUpdateStatusChange,
  initUpdateToasts,
  loadDismissedUpdateVersion,
  saveDismissedUpdateVersion,
  UPDATE_TOAST_ID,
} from "./updateToast";
import { DISMISSED_UPDATE_VERSION_STORAGE_KEY } from "./storageKeys";

let statusSubscriber: ((status: any) => void) | null = null;
const mockDownloadAndInstallUpdate = vi.fn().mockResolvedValue(undefined);

vi.mock("./updater", () => ({
  subscribeUpdateStatus: vi.fn((listener: (status: any) => void) => {
    statusSubscriber = listener;
    return () => {
      if (statusSubscriber === listener) statusSubscriber = null;
    };
  }),
  downloadAndInstallUpdate: () => mockDownloadAndInstallUpdate(),
}));

const mockToast = {
  info: vi.fn(),
  loading: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  dismiss: vi.fn(),
};

vi.mock("../components/ui/sonner", () => ({
  toast: {
    info: (message: any, options?: any) => mockToast.info(message, options),
    loading: (message: any, options?: any) => mockToast.loading(message, options),
    success: (message: any, options?: any) => mockToast.success(message, options),
    error: (message: any, options?: any) => mockToast.error(message, options),
    dismiss: (id?: any) => mockToast.dismiss(id),
  },
}));

function createMockStorage(): Storage {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
    removeItem: (key: string) => store.delete(key),
    clear: () => store.clear(),
    key: (index: number) => Array.from(store.keys())[index] ?? null,
    get length() {
      return store.size;
    },
  };
}

describe("updateToast", () => {
  let storage: Storage;

  beforeEach(() => {
    storage = createMockStorage();
    vi.clearAllMocks();
    statusSubscriber = null;
  });

  describe("storage helpers", () => {
    it("loads and saves dismissed update version", () => {
      expect(loadDismissedUpdateVersion(storage)).toBeNull();
      saveDismissedUpdateVersion("1.2.3", storage);
      expect(loadDismissedUpdateVersion(storage)).toBe("1.2.3");
      expect(storage.getItem(DISMISSED_UPDATE_VERSION_STORAGE_KEY)).toBe("1.2.3");
    });
  });

  describe("handleUpdateStatusChange", () => {
    it("shows toast.info on state === 'available' and triggers update on action click", () => {
      handleUpdateStatusChange({ state: "available", version: "2026.830.3" }, storage);

      expect(mockToast.info).toHaveBeenCalledWith(
        "Update available",
        expect.objectContaining({
          id: UPDATE_TOAST_ID,
          description: "Ferryx v2026.830.3 is ready. Sessions won't be interrupted.",
          duration: Infinity,
          action: expect.objectContaining({
            label: "Update",
          }),
        }),
      );

      const call = mockToast.info.mock.calls[0];
      const action = call[1].action;
      action.onClick();
      expect(mockDownloadAndInstallUpdate).toHaveBeenCalledTimes(1);
    });

    it("persists dismissed version when toast is dismissed", () => {
      handleUpdateStatusChange({ state: "available", version: "2026.830.3" }, storage);

      const call = mockToast.info.mock.calls[0];
      const onDismiss = call[1].onDismiss;
      onDismiss();

      expect(loadDismissedUpdateVersion(storage)).toBe("2026.830.3");
    });

    it("skips toast when available version matches persisted dismissed version", () => {
      saveDismissedUpdateVersion("2026.830.3", storage);

      handleUpdateStatusChange({ state: "available", version: "2026.830.3" }, storage);
      expect(mockToast.info).not.toHaveBeenCalled();

      // But shows for a newer version
      handleUpdateStatusChange({ state: "available", version: "2026.830.4" }, storage);
      expect(mockToast.info).toHaveBeenCalledTimes(1);
    });

    it("updates toast with downloading progress percentage using same id", () => {
      handleUpdateStatusChange(
        { state: "downloading", version: "2026.830.3", downloadProgress: 0.45 },
        storage,
      );

      expect(mockToast.loading).toHaveBeenCalledWith(
        "Downloading update",
        expect.objectContaining({
          id: UPDATE_TOAST_ID,
          description: "Downloading version 2026.830.3… 45%",
          duration: Infinity,
        }),
      );
    });

    it("shows success toast on downloaded with auto-dismiss duration", () => {
      handleUpdateStatusChange({ state: "downloaded", version: "2026.830.3" }, storage);

      expect(mockToast.success).toHaveBeenCalledWith(
        "Version 2026.830.3 installed. Relaunching…",
        expect.objectContaining({
          id: UPDATE_TOAST_ID,
          duration: 8000,
        }),
      );
    });

    it("shows error toast on error with duration Infinity", () => {
      handleUpdateStatusChange(
        { state: "error", version: "2026.830.3", error: "Network timeout" },
        storage,
      );

      expect(mockToast.error).toHaveBeenCalledWith(
        "Network timeout",
        expect.objectContaining({
          id: UPDATE_TOAST_ID,
          duration: Infinity,
        }),
      );
    });
  });

  describe("initUpdateToasts", () => {
    it("subscribes and handles status changes, allowing unsubscription", () => {
      const unsub = initUpdateToasts();
      expect(statusSubscriber).toBeTypeOf("function");

      statusSubscriber?.({ state: "available", version: "2026.9.1" });
      expect(mockToast.info).toHaveBeenCalledTimes(1);

      unsub();
      expect(statusSubscriber).toBeNull();
    });
  });
});
