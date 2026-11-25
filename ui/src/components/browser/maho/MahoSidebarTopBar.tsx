// Maho sidebar top bar: the collapse toggle plus the back/forward/reload cluster.
//
// Presentational on purpose. Every command arrives as a callback so the sidebar
// decides whether a click drives `browserTabNavigation` or a caller-owned engine,
// and Todo 14's ControlActivity chip drops into `controlActivity` unchanged.

import { ArrowLeft, ArrowRight, PanelLeft, RotateCw } from "lucide-react";
import type { ReactNode } from "react";

export type MahoSidebarTopBarProps = {
  /** Rail state; the toggle reads "Expand sidebar" while collapsed. */
  collapsed?: boolean;
  onToggleSidebar?: () => void;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onBack?: () => void;
  onForward?: () => void;
  onReload?: () => void;
  /** Slot for Todo 14's agent ControlActivity chip. */
  controlActivity?: ReactNode;
};

export function MahoSidebarTopBar({
  collapsed = false,
  onToggleSidebar,
  canGoBack = false,
  canGoForward = false,
  onBack,
  onForward,
  onReload,
  controlActivity,
}: MahoSidebarTopBarProps) {
  return (
    <div className="maho-top-bar" role="toolbar" aria-label="Browser navigation">
      <button
        type="button"
        className="maho-top-bar__button"
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-expanded={!collapsed}
        onClick={onToggleSidebar}
      >
        <PanelLeft aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-top-bar__button"
        aria-label="Back"
        disabled={!canGoBack}
        onClick={onBack}
      >
        <ArrowLeft aria-hidden="true" />
      </button>
      <button
        type="button"
        className="maho-top-bar__button"
        aria-label="Forward"
        disabled={!canGoForward}
        onClick={onForward}
      >
        <ArrowRight aria-hidden="true" />
      </button>
      <button type="button" className="maho-top-bar__button" aria-label="Reload" onClick={onReload}>
        <RotateCw aria-hidden="true" />
      </button>
      {controlActivity ? <div className="maho-top-bar__chip">{controlActivity}</div> : null}
    </div>
  );
}
