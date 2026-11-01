import { useEffect, useState } from "react";
import { CircleAlert, X } from "lucide-react";

import { onBrowserEngineFallback, takeBrowserEngineFallback } from "../lib/browserEngineFallback";

/**
 * Says why a tab runs on the system webview when the Chromium engine is installed but failed
 * to open it. The backend keeps the notice until the pane takes it, and also emits it live.
 */
export function BrowserEngineFallbackNotice({ browserId }: { browserId: string }) {
  const [notice, setNotice] = useState<{ browserId: string; message: string } | null>(null);
  const message = notice?.browserId === browserId ? notice.message : null;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserEngineFallback((notice) => {
      if (notice.browserId !== browserId) return;
      setNotice({ browserId, message: notice.message });
      // The event and the stored notice describe the same failure: drop the stored copy.
      void takeBrowserEngineFallback(browserId).catch(() => undefined);
    }).then(
      (cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      },
      () => undefined,
    );
    void takeBrowserEngineFallback(browserId).then(
      (notice) => {
        if (!disposed && notice) setNotice({ browserId, message: notice.message });
      },
      () => undefined,
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [browserId]);

  if (!message) return null;
  return (
    <div
      role="status"
      data-testid="browser-engine-fallback"
      className="flex items-center gap-2 border-b border-amber-500/20 bg-amber-500/10 px-3 py-1.5 text-xs text-amber-600"
    >
      <CircleAlert className="size-3.5 shrink-0" />
      <span className="min-w-0 flex-1 truncate" title={message}>
        Chromium could not open this tab, so it uses the system browser engine: {message}
      </span>
      <button type="button" aria-label="Dismiss engine notice" onClick={() => setNotice(null)} className="rounded p-1 hover:bg-accent">
        <X className="size-3.5" />
      </button>
    </div>
  );
}
