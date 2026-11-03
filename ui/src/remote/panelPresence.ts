// Tells the gateway whether this client shows the panel. While any client does, decision pushes
// are held back (plan D13: no push while a client shows the panel). The gateway forgets a client
// that goes quiet after PANEL_PRESENCE_STALE (120 s), so a heartbeat well inside that keeps a
// shown panel counted and a crashed or suspended page stops counting by itself.
import { useEffect } from "react";

import { jsonPost, type PushRequest } from "../lib/pushSubscription";

export const PANEL_PRESENCE_HEARTBEAT_MS = 45_000;

/** One report. Resolves false on any failure: presence is best effort, never user-facing. */
export async function reportPanelVisible(request: PushRequest, visible: boolean): Promise<boolean> {
  try {
    const res = await request("/api/v1/push/panel-presence", jsonPost({ visible }));
    return res.ok;
  } catch {
    return false;
  }
}

/**
 * Reports `shown` (the panel is the screen) while the page is in the foreground: on show, on a
 * heartbeat, and a final hidden on hide, background or unmount. `request` null reports nothing.
 */
export function usePanelPresence(request: PushRequest | null, shown: boolean): void {
  useEffect(() => {
    if (!request) return;
    let reporting = false;
    let timer: ReturnType<typeof setInterval> | undefined;
    const sync = () => {
      const visible = shown && document.visibilityState !== "hidden";
      if (visible && !reporting) {
        reporting = true;
        void reportPanelVisible(request, true);
        timer = setInterval(() => void reportPanelVisible(request, true), PANEL_PRESENCE_HEARTBEAT_MS);
      } else if (!visible && reporting) {
        reporting = false;
        clearInterval(timer);
        void reportPanelVisible(request, false);
      }
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      clearInterval(timer);
      if (reporting) void reportPanelVisible(request, false);
    };
  }, [request, shown]);
}
