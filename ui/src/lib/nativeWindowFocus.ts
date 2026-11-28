import { getCurrentWindow } from "@tauri-apps/api/window";
import { publishWindowFocus, startWindowFocusBroadcast } from "./notificationLeader";
import { reportDesktopWindowFocus } from "./tauri";

const FOCUS_REPORT_INTERVAL_MS = 60_000;

let trackingStarted = false;
let disposed = false;
let trackingGeneration = 0;
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
  if (disposed || focused === null) return;
  publishWindowFocus(focused);
  void reportFocus(focused);
  if (focused) {
    focusReportTimer = setInterval(() => {
      void reportFocus(true);
    }, FOCUS_REPORT_INTERVAL_MS);
  }
}

export function stopNativeWindowFocusTracking(): void {
  disposed = true;
  trackingGeneration += 1;
  if (focusReportTimer !== null) {
    clearInterval(focusReportTimer);
    focusReportTimer = null;
  }
  if (unlistenFocusChanged !== null) {
    const unlisten = unlistenFocusChanged;
    unlistenFocusChanged = null;
    unlisten();
  }
  trackingStarted = false;
}

export function startNativeWindowFocusTracking(): void {
  if (trackingStarted || unlistenFocusChanged) return;
  disposed = false;
  trackingStarted = true;
  const generation = ++trackingGeneration;
  const isStale = () => disposed || generation !== trackingGeneration;
  startWindowFocusBroadcast();

  void (async () => {
    let receivedFocusEvent = false;
    try {
      const win = getCurrentWindow();
      const unlisten = await win.onFocusChanged((event) => {
        if (isStale()) return;
        receivedFocusEvent = true;
        nativeFocused = event.payload;
        syncFocusReporting(event.payload);
        if (event.payload && typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent("ferryx:window-focused"));
        }
      });
      if (isStale()) {
        unlisten();
        return;
      }
      unlistenFocusChanged = unlisten;
      const focused = await win.isFocused();
      if (isStale()) return;
      if (!receivedFocusEvent) {
        nativeFocused = focused;
        syncFocusReporting(focused);
      }
    } catch {
      if (isStale()) return;
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

if (import.meta.hot) {
  import.meta.hot.dispose(() => stopNativeWindowFocusTracking());
}
