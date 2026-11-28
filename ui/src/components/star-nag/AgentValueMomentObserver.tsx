import { useCallback, useEffect, useRef } from "react";

import type { TerminalActivity } from "../../lib/activity";
import {
  STAR_NAG_CHECK_DELAY_MS,
  STAR_NAG_QUIET_WINDOW_MS,
  hasSuccessfulDoneTransition,
} from "../../lib/starNag";

/** Modifier-only key presses are not typing, so they must not restart the quiet window. */
const NON_TYPING_MODIFIER_KEYS = new Set(["Alt", "Control", "Meta", "Shift"]);

function isTypingKeyEvent(event: KeyboardEvent): boolean {
  return !NON_TYPING_MODIFIER_KEYS.has(event.key);
}

function hasActiveAgents(activityBySession: Record<string, TerminalActivity>): boolean {
  for (const activity of Object.values(activityBySession)) {
    if (activity.isAgent && (activity.state === "working" || activity.state === "waiting")) return true;
  }
  return false;
}

/**
 * Watches for the moment an agent finishes real work and then goes quiet, and reports it once per
 * app run. Ported from Orca's `StarNagAgentValueMomentObserver`.
 *
 * Timing is Orca's: a successful `done` transition schedules a check `CHECK_DELAY_MS` later;
 * the check keeps re-arming while any agent is still working or the user typed within
 * `QUIET_WINDOW_MS`. The quiet window exists so the prompt never interrupts follow-up typing.
 *
 * One deliberate difference: Orca also requires the finished turn to have carried a meaningful
 * prompt. Ferryx's activity payload carries no prompt text, so the observer passes
 * `hasMeaningfulPrompt: true` - the clause is unevaluable here, and filtering on it anyway would
 * leave the prompt permanently dead. The clauses Ferryx *can* evaluate are all enforced:
 * `outcome === "error"` is Orca's unclean turn, and `notificationSuppressed` (attention restored
 * from a previous run, not new work) is Orca's session boundary.
 */
export function AgentValueMomentObserver(props: {
  activityBySession: Record<string, TerminalActivity>;
  enabled: boolean;
  onValueMoment: () => void;
}): null {
  const { activityBySession, enabled, onValueMoment } = props;
  const previousRef = useRef<Record<string, TerminalActivity>>({});
  const pendingRef = useRef(false);
  const requestedRef = useRef(false);
  const lastTypingAtRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activityRef = useRef(activityBySession);
  activityRef.current = activityBySession;
  const onValueMomentRef = useRef(onValueMoment);
  onValueMomentRef.current = onValueMoment;

  const scheduleCheck = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      if (!pendingRef.current || requestedRef.current) return;
      const elapsedSinceTyping = Date.now() - lastTypingAtRef.current;
      if (hasActiveAgents(activityRef.current) || elapsedSinceTyping < STAR_NAG_QUIET_WINDOW_MS) {
        scheduleCheck();
        return;
      }
      pendingRef.current = false;
      requestedRef.current = true;
      onValueMomentRef.current();
    }, STAR_NAG_CHECK_DELAY_MS);
  }, []);

  // Any real typing pushes the quiet window forward, so a prompt never lands mid-keystroke.
  useEffect(() => {
    const markTyping = (event: Event) => {
      if (event instanceof KeyboardEvent && !isTypingKeyEvent(event)) return;
      lastTypingAtRef.current = Date.now();
    };
    window.addEventListener("keydown", markTyping, true);
    window.addEventListener("input", markTyping, true);
    return () => {
      window.removeEventListener("keydown", markTyping, true);
      window.removeEventListener("input", markTyping, true);
    };
  }, []);

  useEffect(() => {
    const previous = previousRef.current;
    previousRef.current = activityBySession;
    if (!enabled || requestedRef.current || pendingRef.current) return;
    for (const [sessionId, activity] of Object.entries(activityBySession)) {
      const before = previous[sessionId];
      if (
        hasSuccessfulDoneTransition({
          previousState: before?.state ?? null,
          state: activity.state,
          isAgent: activity.isAgent,
          sessionBoundary: activity.notificationSuppressed === true,
          agentTurnEndedUncleaned: activity.outcome === "error",
          hasMeaningfulPrompt: true,
        })
      ) {
        pendingRef.current = true;
        scheduleCheck();
        return;
      }
    }
  }, [activityBySession, enabled, scheduleCheck]);

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
      timerRef.current = null;
    },
    [],
  );

  return null;
}

