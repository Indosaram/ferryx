import { useEffect, useRef, useState } from "react";

import type { LayoutState, Pane, TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

type TerminalSplitViewProps = {
  layout: LayoutState;
  sessions: Record<string, TerminalSession>;
};

const MIN_PANE_SIZE_PX = 160;

export function TerminalSplitView({ layout, sessions }: TerminalSplitViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const [splitRatio, setSplitRatio] = useState(0.5);
  const primaryTab = layout.tabs.find((tab) => tab.id === layout.primaryTabId) ?? null;
  const secondaryTab = layout.split !== "none"
    ? layout.tabs.find((tab) => tab.id === layout.secondaryTabId) ?? null
    : null;
  const primarySession = primaryTab ? sessions[primaryTab.sessionId] ?? null : null;
  const secondarySession = secondaryTab ? sessions[secondaryTab.sessionId] ?? null : null;
  const primaryPane: Pane | null = primaryTab ? { id: "primary", tabId: primaryTab.id } : null;
  const secondaryPane: Pane | null = secondaryTab ? { id: "secondary", tabId: secondaryTab.id } : null;
  const splitActive = layout.split !== "none" && Boolean(secondaryTab && secondaryPane && secondarySession);

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      if (!draggingRef.current || layout.split === "none") return;
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) return;
      const size = layout.split === "horizontal" ? rect.width : rect.height;
      if (size <= 0) return;
      const pointer = layout.split === "horizontal" ? event.clientX - rect.left : event.clientY - rect.top;
      const minRatio = Math.min(0.5, MIN_PANE_SIZE_PX / size);
      const nextRatio = Math.min(1 - minRatio, Math.max(minRatio, pointer / size));
      setSplitRatio(nextRatio);
    };

    const finishDrag = () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      document.body.style.cursor = "";
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      document.body.style.cursor = "";
    };
  }, [layout.split]);

  if (!primaryTab || !primaryPane || !primarySession) {
    return <div className="relative flex-1 overflow-hidden bg-terminal" data-testid="terminal-layout" data-split="none" />;
  }

  const primaryBasis = splitActive ? formatRatioPercent(splitRatio) : undefined;
  const secondaryBasis = splitActive ? formatRatioPercent(1 - splitRatio) : undefined;
  const paneStyle = (basis: string | undefined) =>
    basis
      ? { flexBasis: basis, flexGrow: 0, flexShrink: 0 }
      : undefined;

  const startDividerDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    draggingRef.current = true;
    document.body.style.cursor = layout.split === "horizontal" ? "col-resize" : "row-resize";
  };

  return (
    <div
      ref={containerRef}
      className={`relative flex flex-1 overflow-hidden ${
        layout.split === "horizontal" ? "flex-row" : layout.split === "vertical" ? "flex-col" : ""
      }`}
      data-testid="terminal-layout"
      data-split={layout.split}
    >
      <div
        className={`relative min-h-0 min-w-0 overflow-hidden ${splitActive ? "" : "flex-1"}`}
        data-testid="primary-pane"
        data-pane-id={primaryPane.id}
        style={paneStyle(primaryBasis)}
      >
        <TerminalPane key={primaryTab.id} session={primarySession} active />
      </div>

      {splitActive && secondaryTab && secondaryPane && secondarySession ? (
        <>
          <div
            role="separator"
            aria-label="Resize terminal panes"
            aria-orientation={layout.split === "horizontal" ? "vertical" : "horizontal"}
            aria-valuemin={MIN_PANE_SIZE_PX}
            aria-valuenow={Math.round(splitRatio * 100)}
            onPointerDown={startDividerDrag}
            className={`absolute z-20 touch-none bg-border transition-colors hover:bg-ring ${
              layout.split === "horizontal"
                ? "inset-y-0 w-1.5 -translate-x-1/2 cursor-col-resize"
                : "inset-x-0 h-1.5 -translate-y-1/2 cursor-row-resize"
            }`}
            style={layout.split === "horizontal" ? { left: formatRatioPercent(splitRatio) } : { top: formatRatioPercent(splitRatio) }}
          />
          <div
            className="relative min-h-0 min-w-0 overflow-hidden"
            data-testid="secondary-pane"
            data-pane-id={secondaryPane.id}
            style={paneStyle(secondaryBasis)}
          >
            <TerminalPane key={secondaryTab.id} session={secondarySession} active />
          </div>
        </>
      ) : null}
    </div>
  );
}

function formatRatioPercent(ratio: number) {
  return `${Number((ratio * 100).toFixed(4))}%`;
}
