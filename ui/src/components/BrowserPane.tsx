import { useRef, useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { setBrowserBounds, setBrowserVisible } from "../lib/browserTauri";
import { recordBrowserHistory } from "../lib/browserHistory";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

interface BrowserPaneProps {
  tab: BrowserTab;
  visible?: boolean;
  onNavigate: (url: string) => void;
  onReload: () => void;
}

type BrowserStateChangedPayload = {
  browserId: string;
  generation: number;
  url: string;
  title?: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  zoomFactor: number;
  loadError?: string | null;
};

export function BrowserPane({ tab, visible = true, onNavigate, onReload }: BrowserPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const [liveTab, setLiveTab] = useState<BrowserTab>(tab);

  useEffect(() => {
    setLiveTab((current) => ({ ...current, ...tab }));
  }, [tab]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    const applyState = (payload: BrowserStateChangedPayload) => {
      if (payload.browserId !== tab.browserId) return;
      setLiveTab((current) => ({
        ...current,
        url: payload.url || current.url,
        title: payload.title ?? current.title,
        loading: payload.loading,
        canGoBack: payload.canGoBack,
        canGoForward: payload.canGoForward,
      }));
      if (!payload.loading && payload.url) {
        recordBrowserHistory({
          browserId: payload.browserId,
          url: payload.url,
          title: payload.title ?? null,
        });
      }
    };

    void listen<BrowserStateChangedPayload>("browser_state_changed", (event) => {
      applyState(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [tab.browserId]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateVisibility = (nextVisible: boolean) => {
      void setBrowserVisible(tab.browserId, nextVisible).catch(() => undefined);
    };

    const updateBounds = () => {
      if (!visible) {
        updateVisibility(false);
        return;
      }

      const rect = el.getBoundingClientRect();
      const toolbarRect = toolbarRef.current?.getBoundingClientRect();
      const minTop = toolbarRect ? toolbarRect.bottom : rect.y;
      const clampedY = Math.max(rect.y, minTop);
      const heightReduction = clampedY - rect.y;
      const clampedHeight = Math.max(0, rect.height - heightReduction);

      void setBrowserBounds(tab.browserId, {
        x: rect.x,
        y: clampedY,
        width: rect.width,
        height: clampedHeight,
      }).catch(() => undefined);
      updateVisibility(true);
    };

    updateBounds();

    const resizeObserver = new ResizeObserver(() => {
      updateBounds();
    });

    resizeObserver.observe(el);
    window.addEventListener("resize", updateBounds);

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener("resize", updateBounds);
      // Native child webviews outlive React DOM nodes; cleanup also covers Fast Refresh remounts.
      updateVisibility(false);
    };
  }, [tab.browserId, visible]);

  return (
    <div className="flex flex-col w-full h-full bg-background overflow-hidden">
      <div ref={toolbarRef}>
        <BrowserToolbar tab={liveTab} onNavigate={onNavigate} onReload={onReload} />
      </div>
      <div
        ref={containerRef}
        data-testid="browser-viewport"
        className="flex-1 w-full bg-card relative"
      />
    </div>
  );
}
