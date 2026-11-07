import { useEffect } from "react";

import { migrateLegacyBrowserHistory, setHostHistoryEnabled } from "./browserLibrary";
import { useBrowserSettings } from "./browserSettings";

/** The omnibox asks the app to show an already open browser tab instead of navigating. */
export const BROWSER_SWITCH_TO_TAB_EVENT = "ferryx:browser-switch-to-tab";
export type BrowserSwitchToTabDetail = { browserId: string };

export function requestSwitchToBrowserTab(browserId: string): void {
  window.dispatchEvent(new CustomEvent<BrowserSwitchToTabDetail>(BROWSER_SWITCH_TO_TAB_EVENT, { detail: { browserId } }));
}

/**
 * Keeps the host history recorder in step with the "remember browsing history" setting and moves
 * the old localStorage history into the host store once. Mounted once by the desktop App.
 */
export function useBrowserLibrarySync(enabled: boolean, onError: (error: unknown) => void): void {
  const { settings } = useBrowserSettings();
  const remember = settings.rememberBrowsingHistory;
  useEffect(() => {
    if (!enabled) return;
    void Promise.resolve()
      .then(() => setHostHistoryEnabled(remember))
      .catch(onError);
  }, [enabled, onError, remember]);
  useEffect(() => {
    if (!enabled) return;
    void Promise.resolve()
      .then(() => migrateLegacyBrowserHistory())
      .catch(onError);
  }, [enabled, onError]);
}
