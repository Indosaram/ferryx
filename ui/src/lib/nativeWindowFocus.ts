import { getCurrentWindow } from "@tauri-apps/api/window";
import { reportDesktopWindowFocus } from "./tauri";

const FOCUS_REPORT_INTERVAL_MS = 60_000;

let trackingStarted = false;
let nativeFocused: boolean | null = null;
let unlistenFocusChanged: (() => void) | null = null;
let focusReportTimer: ReturnType<typeof setInterval> | null = null;
let warnedReportFailure = false;

async function reportFocus(focused: boolean): Promise<void> {
  try {
    await reportDesktopWindowFocus(focused);
  } catch (error) {
    if (warnedReportFailure) return;
    warnedReportFailure = true;
    console.warn("Failed to report desktop window focus", error);
  }
}

function syncFocusReporting(focused: boolean | null): void {
  if (focusReportTimer !== null) {
    clearInterval(focusReportTimer);
    focusReportTimer = null;
  }
  if (focused === null) return;
  void reportFocus(focused);
  if (focused) {
    focusReportTimer = setInterval(() => {
      void reportFocus(true);
    }, FOCUS_REPORT_INTERVAL_MS);
  }
}

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
        syncFocusReporting(event.payload);
        if (event.payload && typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent("ferryx:window-focused"));
        }
      });
      const focused = await win.isFocused();
      if (!receivedFocusEvent) {
        nativeFocused = focused;
        syncFocusReporting(focused);
      }
    } catch {
      if (!receivedFocusEvent) {
        nativeFocused = null;
        syncFocusReporting(null);
      }
    }
  })();
}

export function getNativeWindowFocused(): boolean | null {
  return nativeFocused;
}
