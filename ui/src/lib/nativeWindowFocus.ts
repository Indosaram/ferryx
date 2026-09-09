import { getCurrentWindow } from "@tauri-apps/api/window";

let trackingStarted = false;
let nativeFocused: boolean | null = null;
let unlistenFocusChanged: (() => void) | null = null;

export function startNativeWindowFocusTracking(): void {
  if (trackingStarted || unlistenFocusChanged) return;
  trackingStarted = true;

  void (async () => {
    try {
      const win = getCurrentWindow();
      nativeFocused = await win.isFocused();
      unlistenFocusChanged = await win.onFocusChanged((event) => {
        nativeFocused = event.payload;
        if (event.payload && typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent("ferryx:window-focused"));
        }
      });
    } catch {
      nativeFocused = null;
    }
  })();
}

export function getNativeWindowFocused(): boolean | null {
  return nativeFocused;
}
