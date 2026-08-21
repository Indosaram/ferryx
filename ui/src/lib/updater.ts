export type UpdateStatus = {
  state: "idle" | "checking" | "available" | "downloading" | "downloaded" | "error";
  version?: string;
  releaseNotes?: string;
  downloadProgress?: number;
  error?: string;
};

type CloseGuardListener = () => Promise<void> | void;
const closeGuardListeners = new Set<CloseGuardListener>();

export function registerWindowCloseGuard(listener: CloseGuardListener): () => void {
  closeGuardListeners.add(listener);
  return () => {
    closeGuardListeners.delete(listener);
  };
}

export async function flushCloseGuards(): Promise<void> {
  for (const listener of closeGuardListeners) {
    try {
      await listener();
    } catch (e) {
      console.error("Window close guard execution failed:", e);
    }
  }
}

export interface UpdaterApi {
  getStatus(): Promise<UpdateStatus>;
  onStatus(callback: (status: UpdateStatus) => void): () => void;
  check(): Promise<void>;
  download(): Promise<void>;
  quitAndInstall(): Promise<void>;
}

declare global {
  interface Window {
    api?: {
      updater?: UpdaterApi;
    };
  }
}
