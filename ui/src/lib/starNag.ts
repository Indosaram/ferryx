/**
 * Star prompt, ported from Orca's `main/star-nag` service with its exact numbers.
 *
 * Every function is pure over an injected `Storage` and clock so the timing rules can be tested
 * without a browser. The two entry points Orca runs are reproduced verbatim:
 * - threshold: after `STAR_NAG_INITIAL_THRESHOLD` cumulative agent spawns, then `x2` per defer,
 * - agent value moment: once per app version, after a successful `done` transition.
 */

import {
  AGENTS_SPAWNED_TOTAL_STORAGE_KEY,
  STAR_NAG_BASELINE_AGENTS_STORAGE_KEY,
  STAR_NAG_COMPLETED_STORAGE_KEY,
  STAR_NAG_DEFERRED_UNTIL_STORAGE_KEY,
  STAR_NAG_DISABLED_STORAGE_KEY,
  STAR_NAG_NEXT_THRESHOLD_STORAGE_KEY,
  STAR_NAG_VALUE_MOMENT_APP_VERSION_STORAGE_KEY,
} from "./storageKeys";

/** Cumulative agent spawns before the first star prompt. */
export const STAR_NAG_INITIAL_THRESHOLD = 35;
/** Silence after any dismiss/later before the next threshold prompt can fire. */
export const STAR_NAG_COOLDOWN_DAYS = 3;
export const STAR_NAG_COOLDOWN_MS = STAR_NAG_COOLDOWN_DAYS * 24 * 60 * 60 * 1000;
/** A successful agent turn must be followed by this much quiet before the value moment fires. */
export const STAR_NAG_QUIET_WINDOW_MS = 1200;
export const STAR_NAG_CHECK_DELAY_MS = 1200;

export const FERRYX_REPO = "Indosaram/ferryx";
export const FERRYX_REPO_URL = "https://github.com/Indosaram/ferryx";

export type StarNagMode = "gh" | "web";

type ReadableStorage = Pick<Storage, "getItem" | "setItem">;
type WritableStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function readNumber(storage: ReadableStorage, key: string): number | null {
  const raw = storage.getItem(key);
  if (raw === null) return null;
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : null;
}

function readFlag(storage: ReadableStorage, key: string): boolean {
  return storage.getItem(key) === "true";
}

export function isStarNagDisabled(storage: ReadableStorage): boolean {
  return readFlag(storage, STAR_NAG_DISABLED_STORAGE_KEY);
}

export function isStarNagCompleted(storage: ReadableStorage): boolean {
  return readFlag(storage, STAR_NAG_COMPLETED_STORAGE_KEY);
}

export function loadAgentsSpawnedTotal(storage: ReadableStorage): number {
  return readNumber(storage, AGENTS_SPAWNED_TOTAL_STORAGE_KEY) ?? 0;
}

export function loadStarNagBaselineAgents(storage: ReadableStorage): number | null {
  return readNumber(storage, STAR_NAG_BASELINE_AGENTS_STORAGE_KEY);
}

export function loadStarNagNextThreshold(storage: ReadableStorage): number | null {
  return readNumber(storage, STAR_NAG_NEXT_THRESHOLD_STORAGE_KEY);
}

export function loadStarNagDeferredUntil(storage: ReadableStorage): number | null {
  return readNumber(storage, STAR_NAG_DEFERRED_UNTIL_STORAGE_KEY);
}

/** Orca's `isCooldownActive`: a numeric deadline strictly in the future. */
export function isStarNagCooldownActive(
  deferredUntil: number | null | undefined,
  now: number,
): boolean {
  return typeof deferredUntil === "number" && deferredUntil > now;
}

/**
 * Orca's `ensureStarNagBaseline`: the first evaluation only records where counting starts, so a
 * user who already spawned hundreds of agents is not prompted on install day.
 */
export function ensureStarNagBaseline(storage: WritableStorage, total: number): void {
  if (storage.getItem(STAR_NAG_BASELINE_AGENTS_STORAGE_KEY) !== null) return;
  storage.setItem(STAR_NAG_BASELINE_AGENTS_STORAGE_KEY, String(total));
  if (storage.getItem(STAR_NAG_NEXT_THRESHOLD_STORAGE_KEY) === null) {
    storage.setItem(STAR_NAG_NEXT_THRESHOLD_STORAGE_KEY, String(STAR_NAG_INITIAL_THRESHOLD));
  }
}

export type StarNagThresholdArgs = {
  totalAgentsSpawned: number;
  promptVisible: boolean;
  evaluating: boolean;
  now: number;
};

/**
 * Orca's `shouldShowStarNagThresholdPrompt`. Returns false while a prompt is up or a GitHub check
 * is in flight, and always seeds the baseline on first call instead of prompting.
 */
export function shouldShowStarNagThresholdPrompt(
  storage: WritableStorage,
  args: StarNagThresholdArgs,
): boolean {
  if (args.promptVisible || args.evaluating) return false;
  if (isStarNagCompleted(storage) || isStarNagDisabled(storage)) return false;
  if (isStarNagCooldownActive(loadStarNagDeferredUntil(storage), args.now)) return false;
  if (storage.getItem(STAR_NAG_BASELINE_AGENTS_STORAGE_KEY) === null) {
    ensureStarNagBaseline(storage, args.totalAgentsSpawned);
    return false;
  }
  const baseline = loadStarNagBaselineAgents(storage) ?? 0;
  const threshold = loadStarNagNextThreshold(storage) ?? STAR_NAG_INITIAL_THRESHOLD;
  return args.totalAgentsSpawned - baseline >= threshold;
}

/**
 * Orca's `defer`: double the threshold, move the baseline to today's total, and start the cooldown.
 * Used by both `dismiss` and `later`, and (cooldown only) by the web handoff.
 */
export function deferStarNag(
  storage: WritableStorage,
  args: { totalAgentsSpawned: number; now: number; cooldownMs?: number },
): void {
  const threshold = loadStarNagNextThreshold(storage) ?? STAR_NAG_INITIAL_THRESHOLD;
  storage.setItem(STAR_NAG_NEXT_THRESHOLD_STORAGE_KEY, String(threshold * 2));
  storage.setItem(STAR_NAG_BASELINE_AGENTS_STORAGE_KEY, String(args.totalAgentsSpawned));
  storage.setItem(
    STAR_NAG_DEFERRED_UNTIL_STORAGE_KEY,
    String(args.now + (args.cooldownMs ?? STAR_NAG_COOLDOWN_MS)),
  );
}

/**
 * Orca's `deferAfterStarNagWebHandoff`: opening the repo in a browser only earns a cooldown; the
 * prompt is not marked complete, because the user may not have starred.
 */
export function deferStarNagAfterWebHandoff(
  storage: WritableStorage,
  args: { totalAgentsSpawned: number; now: number },
): void {
  deferStarNag(storage, { ...args, cooldownMs: STAR_NAG_COOLDOWN_MS });
}

/** Orca's `markCompleted`: never prompt again, and clear any pending cooldown. */
export function markStarNagCompleted(storage: WritableStorage): void {
  storage.setItem(STAR_NAG_COMPLETED_STORAGE_KEY, "true");
  storage.removeItem(STAR_NAG_DEFERRED_UNTIL_STORAGE_KEY);
}

/** Orca's `disable`: the settings switch. Turning it off retires the prompt for good. */
export function disableStarNag(storage: WritableStorage): void {
  storage.setItem(STAR_NAG_DISABLED_STORAGE_KEY, "true");
  markStarNagCompleted(storage);
}

export function enableStarNag(storage: WritableStorage): void {
  storage.removeItem(STAR_NAG_DISABLED_STORAGE_KEY);
}

/** Browser storage for settings surfaces, which must not touch `localStorage` directly. */
export function browserStarNagStorage(): WritableStorage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** Applies the settings toggle: off retires the prompt for good, on clears only the off switch. */
export function applyStarNagEnabled(enabled: boolean): void {
  const storage = browserStarNagStorage();
  if (!storage) return;
  if (enabled) enableStarNag(storage);
  else disableStarNag(storage);
}

export function loadStarNagValueMomentAppVersion(storage: ReadableStorage): string | null {
  return storage.getItem(STAR_NAG_VALUE_MOMENT_APP_VERSION_STORAGE_KEY);
}

/**
 * Orca's `wasConsumed`: `Boolean(starNagAgentValueMomentAppVersion)`. Any stored value means the
 * moment is spent, so this prompt fires at most once for the lifetime of the install - it does not
 * re-arm on a new version even though a version string is what gets stored.
 */
export function hasConsumedStarNagValueMoment(storage: ReadableStorage): boolean {
  return loadStarNagValueMomentAppVersion(storage) !== null;
}

export function consumeStarNagValueMoment(storage: WritableStorage, appVersion: string): void {
  storage.setItem(STAR_NAG_VALUE_MOMENT_APP_VERSION_STORAGE_KEY, appVersion);
}

export type StarNagValueMomentArgs = {
  appVersion: string | null;
  now: number;
  evaluating: boolean;
  promptVisible: boolean;
};

/**
 * Orca's `agent-value-moment.prepare` decision, minus the async GitHub check: returns the mode the
 * caller should verify, or `null` when the moment must be skipped.
 */
export function getStarNagValueMomentDecision(
  storage: ReadableStorage,
  args: StarNagValueMomentArgs,
): StarNagMode | null {
  if (args.evaluating) return null;
  if (hasConsumedStarNagValueMoment(storage)) return null;
  if (
    isStarNagCompleted(storage) ||
    isStarNagDisabled(storage) ||
    isStarNagCooldownActive(loadStarNagDeferredUntil(storage), args.now) ||
    args.promptVisible
  ) {
    return null;
  }
  return "gh";
}

/**
 * A `done` transition counts only when it is a real agent turn ending in the same pane.
 *
 * Orca additionally requires the finished turn to have carried a meaningful prompt. Ferryx's
 * activity payload has no prompt text, so the caller states whether it could see the turn's
 * content at all (`hasMeaningfulPrompt`); leaving it undefined means "cannot tell" and does not
 * count, rather than silently counting every completion.
 *
 * Orca also compares `stateStartedAt` so a repeat notification for an older `done` cannot count.
 * Ferryx's activity carries no timestamp, so the guard here is only "the previous state was not
 * already done", which covers the same case for every transition the store actually emits.
 */
export type AgentDoneTransition = {
  previousState: string | null | undefined;
  state: string;
  isAgent: boolean;
  sessionBoundary?: boolean;
  agentTurnEndedUncleaned?: boolean;
  hasMeaningfulPrompt?: boolean;
};

export function hasSuccessfulDoneTransition(transition: AgentDoneTransition): boolean {
  if (transition.previousState === "done" || transition.state !== "done") return false;
  if (!transition.isAgent) return false;
  if (transition.sessionBoundary === true) return false;
  if (transition.agentTurnEndedUncleaned === true) return false;
  return transition.hasMeaningfulPrompt === true;
}
