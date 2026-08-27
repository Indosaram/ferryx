import type { AgentState } from "./types";

export type TerminalActivityState = "working" | "waiting" | "done";

export type TerminalActivity = {
  state: TerminalActivityState;
  title: string;
  isAgent: boolean;
  agentType?: string;
  source?: "screen" | "title";
  /**
   * The user has already seen this completion, so it must not light an attention dot.
   *
   * A `done` state is a request for attention, not a durable property of the session. It is set
   * when the completion happens on a visible tab and when the user activates the tab, and it is
   * dropped again by the next `working`/`waiting` turn. The activity entry itself survives so the
   * tab keeps its agent brand icon.
   */
  seen?: boolean;
};

export type ActivitySummary = {
  workingCount: number;
  waitingCount: number;
  doneCount: number;
  runningCount: number;
  hasWorking: boolean;
  hasWaiting: boolean;
  hasDone: boolean;
  hasUnread: boolean;
  agentType?: string;
};

export type ActivityIndicatorState = TerminalActivityState | "unread" | null;

function stateRank(state: TerminalActivityState): number {
  if (state === "waiting") return 3;
  if (state === "working") return 2;
  if (state === "done") return 1;
  return 0;
}

export function summarizeActivities(
  activities: Iterable<TerminalActivity>,
  hasUnread = false,
): ActivitySummary {
  let workingCount = 0;
  let waitingCount = 0;
  let doneCount = 0;
  let bestRank = 0;
  let agentType: string | undefined = undefined;

  for (const activity of activities) {
    if (activity.state === "working") workingCount += 1;
    else if (activity.state === "waiting") waitingCount += 1;
    else if (!activity.seen) doneCount += 1;

    if (activity.agentType !== undefined) {
      const rank = stateRank(activity.state);
      if (rank > bestRank) {
        bestRank = rank;
        agentType = activity.agentType;
      }
    }
  }

  return {
    workingCount,
    waitingCount,
    doneCount,
    runningCount: workingCount,
    hasWorking: workingCount > 0,
    hasWaiting: waitingCount > 0,
    hasDone: doneCount > 0,
    hasUnread,
    agentType,
  };
}

function summaryRank(summary: ActivitySummary): number {
  if (summary.hasWaiting) return 3;
  if (summary.hasWorking) return 2;
  if (summary.hasDone) return 1;
  return 0;
}

export function combineActivitySummaries(
  summaries: Iterable<ActivitySummary>,
  hasUnread = false,
): ActivitySummary {
  let workingCount = 0;
  let waitingCount = 0;
  let doneCount = 0;
  let unread = hasUnread;
  let bestRank = 0;
  let agentType: string | undefined = undefined;

  for (const summary of summaries) {
    workingCount += summary.workingCount;
    waitingCount += summary.waitingCount;
    doneCount += summary.doneCount;
    unread ||= summary.hasUnread;

    if (summary.agentType !== undefined) {
      const rank = summaryRank(summary);
      if (rank > bestRank) {
        bestRank = rank;
        agentType = summary.agentType;
      }
    }
  }

  return {
    workingCount,
    waitingCount,
    doneCount,
    runningCount: workingCount,
    hasWorking: workingCount > 0,
    hasWaiting: waitingCount > 0,
    hasDone: doneCount > 0,
    hasUnread: unread,
    agentType,
  };
}

/**
 * Visual precedence mirrors Orca: user attention wins over live work, live work wins over
 * retained notifications, and a completed-but-read agent is shown last.
 */
export function resolveActivityIndicator(summary: ActivitySummary | undefined): ActivityIndicatorState {
  if (!summary) return null;
  if (summary.hasWaiting) return "waiting";
  if (summary.hasWorking) return "working";
  if (summary.hasUnread) return "unread";
  if (summary.hasDone) return "done";
  return null;
}

export function activityStateToAgentState(state: TerminalActivityState): AgentState {
  if (state === "done") return "exited";
  return state;
}
