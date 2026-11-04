import type { TerminalActivity } from "../lib/activity";
import { isTerminalTab, type LayoutState, type TerminalSession, type WorkspaceTab } from "../lib/types";
import {
  disambiguateTabTexts,
  isTransientAgentStatusTask,
  resolveTerminalTabText,
} from "../lib/tabTitle";
import { focusedPaneSessionId } from "./layout";

export type TerminalTabDisplay = {
  text: string;
  tooltip?: string;
};

// Module-level on purpose: transient agent titles resolve back to this text so labels do not flicker.
const lastAgentTextBySessionId = new Map<string, string>();

export function __resetTabDisplayCacheForTests(): void {
  lastAgentTextBySessionId.clear();
}

function projectNameForSession(session: TerminalSession | undefined): string | undefined {
  const path = session?.worktreePath || session?.cwd;
  if (!path) return undefined;
  const base = path
    .replace(/[\\/]+$/, "")
    .split(/[\\/]/)
    .filter(Boolean)
    .pop();
  return base || undefined;
}

// Terminal tabs only (browser/file tabs get no entry); duplicate texts are disambiguated
// within this same list, first occurrence bare, later ones " (2)", " (3)".
export function computeTerminalTabDisplay(
  tabs: WorkspaceTab[],
  layout: LayoutState,
  sessions: Record<string, TerminalSession>,
  activityBySessionId: Record<string, TerminalActivity | undefined> = {},
): Record<string, TerminalTabDisplay> {
  for (const sessionId of Array.from(lastAgentTextBySessionId.keys())) {
    if (!(sessionId in sessions)) lastAgentTextBySessionId.delete(sessionId);
  }

  const resolved: Array<{ tabId: string; text: string; tooltip?: string }> = [];
  for (const tab of tabs) {
    if (!isTerminalTab(tab)) continue;
    const sessionId = focusedPaneSessionId(layout, tab) ?? tab.sessionId;
    const activity = activityBySessionId[sessionId];
    const result = resolveTerminalTabText({
      customLabel: isTerminalTab(tab) ? tab.customLabel : undefined,
      baseLabel: tab.label,
      sessionName: sessions[sessionId]?.name,
      activity: activity
        ? { title: activity.title, isAgent: activity.isAgent, agentType: activity.agentType }
        : undefined,
      projectName: projectNameForSession(sessions[sessionId]),
      previousAgentText: lastAgentTextBySessionId.get(sessionId),
    });
    if (result.source === "agent" && !isTransientAgentStatusTask(result.text)) {
      lastAgentTextBySessionId.set(sessionId, result.text);
    }
    resolved.push({ tabId: tab.id, text: result.text, tooltip: result.tooltip });
  }

  const texts = disambiguateTabTexts(resolved.map((entry) => entry.text));
  const display: Record<string, TerminalTabDisplay> = {};
  resolved.forEach((entry, index) => {
    display[entry.tabId] = { text: texts[index], tooltip: entry.tooltip };
  });
  return display;
}
