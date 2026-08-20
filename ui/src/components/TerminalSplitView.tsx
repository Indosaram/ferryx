import type { LayoutState, Pane, TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

type TerminalSplitViewProps = {
  layout: LayoutState;
  sessions: Record<string, TerminalSession>;
};

export function TerminalSplitView({ layout, sessions }: TerminalSplitViewProps) {
  const primaryTab = layout.tabs.find((tab) => tab.id === layout.primaryTabId) ?? null;
  const secondaryTab = layout.split !== "none"
    ? layout.tabs.find((tab) => tab.id === layout.secondaryTabId) ?? null
    : null;
  const primarySession = primaryTab ? sessions[primaryTab.sessionId] ?? null : null;
  const secondarySession = secondaryTab ? sessions[secondaryTab.sessionId] ?? null : null;
  const primaryPane: Pane | null = primaryTab ? { id: "primary", tabId: primaryTab.id } : null;
  const secondaryPane: Pane | null = secondaryTab ? { id: "secondary", tabId: secondaryTab.id } : null;

  if (!primaryTab || !primaryPane || !primarySession) {
    return <div className="relative flex-1 overflow-hidden bg-terminal" data-testid="terminal-layout" data-split="none" />;
  }

  return (
    <div
      className={`relative flex flex-1 overflow-hidden ${
        layout.split === "horizontal" ? "flex-row" : layout.split === "vertical" ? "flex-col" : ""
      }`}
      data-testid="terminal-layout"
      data-split={layout.split}
    >
      <div className="relative h-full w-full flex-1 overflow-hidden" data-testid="primary-pane" data-pane-id={primaryPane.id}>
        <TerminalPane key={primaryTab.id} session={primarySession} active />
      </div>

      {layout.split !== "none" && secondaryTab && secondaryPane && secondarySession ? (
        <div
          className={`relative h-full w-full flex-1 overflow-hidden ${
            layout.split === "horizontal" ? "border-l border-border" : "border-t border-border"
          }`}
          data-testid="secondary-pane"
          data-pane-id={secondaryPane.id}
        >
          <TerminalPane key={secondaryTab.id} session={secondarySession} active />
        </div>
      ) : null}
    </div>
  );
}
