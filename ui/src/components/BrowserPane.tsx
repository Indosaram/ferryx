import { useRef, useEffect } from "react";
import { setBrowserBounds, setBrowserVisible } from "../lib/browserTauri";
import { BrowserToolbar } from "./BrowserToolbar";
import type { BrowserTab } from "../lib/types";

interface BrowserPaneProps {
  tab: BrowserTab;
  visible?: boolean;
  onNavigate: (url: string) => void;
  onReload: () => void;
}

export function BrowserPane({ tab, visible = true, onNavigate, onReload }: BrowserPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);

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
      void setBrowserBounds(tab.browserId, {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
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
      <BrowserToolbar tab={tab} onNavigate={onNavigate} onReload={onReload} />
      <div
        ref={containerRef}
        data-testid="browser-viewport"
        className="flex-1 w-full bg-card relative"
      />
    </div>
  );
}