import { referenceTurnIsOpen } from "./referenceHistorySync";
import type { ReferenceTurn } from "./referenceTypes";

/**
 * One queued follow-up per idle observation.
 *
 * The desktop omits activityState once a turn is seen and not working. That omission, and an
 * explicit idle or done mark, is a positive idle observation. waiting (the daemon's blocked)
 * and any other explicit mark are not. A delivery records the observation sequence; the next
 * row waits for a later entry into idle, or for history to show a new closed assistant tail,
 * not the same observation the send just consumed.
 */
export function referencePanePositivelyIdle(activity: string | null): boolean {
  return activity === null || activity === "idle" || activity === "done";
}

export function referenceChatObservedIdle(input: {
  readonly tabPresent: boolean;
  readonly activity: string | null;
  readonly promptPresent: boolean;
  readonly promptKnown: boolean;
}): boolean {
  return (
    input.tabPresent &&
    !input.promptPresent &&
    input.promptKnown &&
    referencePanePositivelyIdle(input.activity)
  );
}

export interface FollowUpIdleClock {
  readonly seq: number;
  readonly wasIdle: boolean;
  readonly targetKey: string | null;
  readonly answeredKey: string | null;
}

/**
 * Entering idle, including after a target change, is a new observation. So is a new answered
 * key while already idle: a closed assistant tail whose identity differs from the one recorded
 * at delivery. A user tail and an open assistant tail pass null and do not bump. Staying idle
 * on the same answered key does not bump.
 */
export function advanceFollowUpIdleClock(
  clock: FollowUpIdleClock,
  idle: boolean,
  targetKey: string | null,
  historyAnswerKey: string | null,
): FollowUpIdleClock {
  const targetChanged = targetKey !== clock.targetKey;
  const wasIdle = targetChanged ? false : clock.wasIdle;
  const answeredKey = targetChanged ? null : clock.answeredKey;
  const entered = idle && !wasIdle;
  const answered = idle && historyAnswerKey !== null && historyAnswerKey !== answeredKey;
  return {
    seq: clock.seq + (entered ? 1 : 0) + (answered ? 1 : 0),
    wasIdle: idle,
    targetKey,
    answeredKey: historyAnswerKey !== null ? historyAnswerKey : answeredKey,
  };
}

/** True only for an idle observation that has not already delivered a row. */
export function followUpIdleMayDeliver(seq: number, consumedSeq: number, idle: boolean): boolean {
  return idle && seq > consumedSeq;
}

/** Identity of one turn: id plus part fingerprint, so a rewrite of the same id still differs. */
export function followUpNewestTurnKey(turn: ReferenceTurn | null | undefined): string | null {
  if (!turn) return null;
  const id = typeof turn.id === "string" ? turn.id : "";
  return `${turn.role}\0${id}\0${JSON.stringify(turn.parts)}`;
}

/**
 * The newest turn, when it is a closed assistant tail. A user turn is never an answer. An
 * assistant turn with a running or unanswered tool call, or commentary without a final answer,
 * is not an answer yet. The newest-text growth rule is not applied here: almost every finished
 * turn ends in text, and the caller only counts this key while the pane is positively idle.
 *
 * The turn must also end in text that is not labelled commentary. A turn whose last part is a
 * tool result or thinking sits between two model calls, and a provider that labels its text
 * (Codex) is still mid-turn while its last text is commentary. A provider that labels nothing
 * cannot tell an interim text tail from its answer; that case still relies on the idle mark.
 */
export function followUpHistoryAnswerKey(
  turns: readonly ReferenceTurn[] | null | undefined,
): string | null {
  if (!turns || turns.length === 0) return null;
  const newest = turns[turns.length - 1];
  if (!newest || newest.role !== "assistant" || referenceTurnIsOpen(newest, false)) return null;
  const last = newest.parts[newest.parts.length - 1];
  if (!last || last.kind !== "text" || last.phase === "commentary") return null;
  return followUpNewestTurnKey(newest);
}
