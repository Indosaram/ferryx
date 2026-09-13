import { getCurrentWindow } from "@tauri-apps/api/window";

let trackingStarted = false;
let nativeFocused: boolean | null = null;
let unlistenFocusChanged: (() => void) | null = null;

export function startNativeWindowFocusTracking(): void {
  if (trackingStarted || unlistenFocusChanged) return;
  trackingStarted = true;

  void (async () => {
    let receivedFocusEvent = false;
    try {
      const win = getCurrentWindow();
      unlistenFocusChanged = await win.onFocusChanged((event) => {
        receivedFocusEvent = true;
        nativeFocused = event.payload;
        if (event.payload && typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent("ferryx:window-focused"));
        }
      });
      const focused = await win.isFocused();
      if (!receivedFocusEvent) nativeFocused = focused;
    } catch {
      if (!receivedFocusEvent) nativeFocused = null;
    }
  })();
}

export function getNativeWindowFocused(): boolean | null {
  return nativeFocused;
}
