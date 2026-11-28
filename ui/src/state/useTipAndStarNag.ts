import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { createAgentsSpawnedRecorder, type AgentsSpawnedRecorder } from "../lib/agentsSpawnedCounter";
import { onAgentSpawned } from "../lib/agentSpawned";
import {
  type CompletedFeatureTipState,
  type FeatureTip,
  type FeatureTipAction,
  type FeatureTipId,
  getFeatureTip,
  normalizeFeatureTipIds,
} from "../lib/featureTips";
import { getFeatureTipsAppOpenDecision } from "../lib/featureTipScheduler";
import {
  consumeStarNagValueMoment,
  deferStarNag,
  deferStarNagAfterWebHandoff,
  FERRYX_REPO_URL,
  getStarNagValueMomentDecision,
  hasConsumedStarNagValueMoment,
  isStarNagCooldownActive,
  isStarNagCompleted,
  loadStarNagDeferredUntil,
  markStarNagCompleted,
  shouldShowStarNagThresholdPrompt,
  type StarNagMode,
} from "../lib/starNag";
import { FEATURE_TIPS_SEEN_IDS_STORAGE_KEY } from "../lib/storageKeys";
import { getGitHubStarStatus, starFerryxRepository } from "../lib/tauri";
import { openExternal } from "../lib/externalLinks";

function browserStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

export type TipAndStarNagArgs = {
  /** Desktop primary window only; a remote client has no daemon of its own to count. */
  enabled: boolean;
  featureTipsEnabled: boolean;
  starNagEnabled: boolean;
  onboardingPending: boolean;
  persistedUIReady: boolean;
  activeModalIsNone: boolean;
  /** `null` while the CLI launcher probe is still running. */
  cliInstalled: boolean | null;
  completedTips: CompletedFeatureTipState;
  appVersion: string | null;
  /** Fired when onboarding finishes; the star toast is offered once, right after it. */
  onboardingJustCompleted: boolean;
  onTipAction: (action: FeatureTipAction) => void;
};

export type TipAndStarNagState = {
  tip: FeatureTip | null;
  closeTip: () => void;
  starCardVisible: boolean;
  starCardMode: StarNagMode;
  starCardBusy: boolean;
  starCardStar: (mode: StarNagMode) => Promise<boolean>;
  starCardLater: () => void;
  starCardDismiss: () => void;
  /** The toast's primary action, split the same way as the card's. */
  starToastStar: () => Promise<boolean>;
  starToastOpenWeb: () => Promise<boolean>;
  /** An unresolved toast dismissal runs the same defer path as "Later". */
  starToastDefer: () => void;
  valueMomentEnabled: boolean;
  onValueMoment: () => void;
  /** True while an onboarding-completed star toast should be on screen. */
  starToastPending: boolean;
  /** The path the toast opens with; it may downgrade itself to "web" after a failed star. */
  starToastMode: StarNagMode;
  /** The toast reports every exit; this clears the shared "a prompt is visible" latch. */
  releaseStarPrompt: () => void;
  runTipAction: (action: FeatureTipAction) => void;
};

/**
 * Owns the two Orca behaviours Ferryx did not have: the once-per-app-open feature tip and the
 * star prompt (threshold card, onboarding toast, agent value moment).
 *
 * All state lives here so the rendered components stay presentational, and so a single
 * `promptVisible` flag can keep the three star-nag entries from stacking on each other.
 */
export function useTipAndStarNag(args: TipAndStarNagArgs): TipAndStarNagState {
  const {
    enabled,
    featureTipsEnabled,
    starNagEnabled,
    onboardingPending,
    persistedUIReady,
    activeModalIsNone,
    cliInstalled,
    completedTips,
    appVersion,
    onboardingJustCompleted,
    onTipAction,
  } = args;

  const storage = useMemo(() => browserStorage(), []);
  const recorderRef = useRef<AgentsSpawnedRecorder | null>(null);
  if (recorderRef.current === null && storage) {
    recorderRef.current = createAgentsSpawnedRecorder(storage);
  }

  const [tipId, setTipId] = useState<FeatureTipId | null>(null);
  const promptedThisSessionRef = useRef(false);
  const suppressedByOnboardingRef = useRef(false);

  const [starCardVisible, setStarCardVisible] = useState(false);
  const [starCardMode, setStarCardMode] = useState<StarNagMode>("gh");
  const [starCardBusy, setStarCardBusy] = useState(false);
  const promptVisibleRef = useRef(false);
  const evaluatingRef = useRef(false);

  const [valueMomentArmed, setValueMomentArmed] = useState(false);

  /** Orca's `maybeShow`: verify the star state, then choose the direct or browser path. */
  const showStarPrompt = useCallback(
    async (surface: "card" | "toast"): Promise<boolean> => {
      if (!storage || !starNagEnabled || promptVisibleRef.current || evaluatingRef.current) return false;
      evaluatingRef.current = true;
      try {
        const status = await getGitHubStarStatus().catch(() => null);
        if (isStarNagCompleted(storage)) return false;
        const mode: StarNagMode = status === null || status.starred === null
          ? "web"
          : status.starred
            ? "web"
            : "gh";
        if (status?.starred === true) {
          // Verified externally: retire the prompt instead of asking again.
          markStarNagCompleted(storage);
          return false;
        }
        if (promptVisibleRef.current) return false;
        promptVisibleRef.current = true;
        if (surface === "toast") {
          setStarToastMode(mode);
          setStarToastPending(true);
        } else {
          setStarCardMode(mode);
          setStarCardVisible(true);
        }
        return true;
      } finally {
        evaluatingRef.current = false;
      }
    },
    [starNagEnabled, storage],
  );

  const [starToastPending, setStarToastPending] = useState(false);
  const [starToastMode, setStarToastMode] = useState<StarNagMode>("gh");

  // Threshold prompt: seed the baseline on first sight, then prompt every N spawns.
  const evaluateThreshold = useCallback(() => {
    const recorder = recorderRef.current;
    if (!storage || !recorder || !starNagEnabled) return;
    if (
      !shouldShowStarNagThresholdPrompt(storage, {
        totalAgentsSpawned: recorder.total(),
        promptVisible: promptVisibleRef.current,
        evaluating: evaluatingRef.current,
        now: Date.now(),
      })
    ) {
      return;
    }
    void showStarPrompt("card");
  }, [showStarPrompt, starNagEnabled, storage]);

  useEffect(() => {
    if (!enabled) return;
    evaluateThreshold();
  }, [enabled, evaluateThreshold]);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void onAgentSpawned((event) => {
      if (disposed) return;
      const recorded = recorderRef.current?.record(event.sessionId);
      if (recorded === null || recorded === undefined) return;
      evaluateThreshold();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [enabled, evaluateThreshold]);

  const releasePrompt = useCallback(() => {
    promptVisibleRef.current = false;
    setStarCardVisible(false);
    setStarToastPending(false);
  }, []);

  const starCardStar = useCallback(async (mode: StarNagMode): Promise<boolean> => {
    if (!storage) return false;
    setStarCardBusy(true);
    try {
      // A browser-path prompt never touches `gh`; it hands off to GitHub and only cools down.
      if (mode === "web") {
        const opened = await openExternal(FERRYX_REPO_URL).catch(() => false);
        if (!opened) return false;
        deferStarNagAfterWebHandoff(storage, {
          totalAgentsSpawned: recorderRef.current?.total() ?? 0,
          now: Date.now(),
        });
        releasePrompt();
        return true;
      }
      await starFerryxRepository();
      markStarNagCompleted(storage);
      releasePrompt();
      return true;
    } catch {
      return false;
    } finally {
      setStarCardBusy(false);
    }
  }, [releasePrompt, storage]);

  const starToastStar = useCallback(async (): Promise<boolean> => {
    if (!storage) return false;
    try {
      await starFerryxRepository();
      markStarNagCompleted(storage);
      releasePrompt();
      return true;
    } catch {
      return false;
    }
  }, [releasePrompt, storage]);

  const starToastOpenWeb = useCallback(async (): Promise<boolean> => {
    if (!storage) return false;
    const opened = await openExternal(FERRYX_REPO_URL).catch(() => false);
    if (!opened) return false;
    deferStarNagAfterWebHandoff(storage, {
      totalAgentsSpawned: recorderRef.current?.total() ?? 0,
      now: Date.now(),
    });
    releasePrompt();
    return true;
  }, [releasePrompt, storage]);

  const deferPrompt = useCallback(() => {
    if (!storage) return;
    deferStarNag(storage, {
      totalAgentsSpawned: recorderRef.current?.total() ?? 0,
      now: Date.now(),
    });
    releasePrompt();
  }, [releasePrompt, storage]);

  // Onboarding completion outranks a threshold card hidden behind the wizard, as in Orca - but
  // Orca bails out first when the prompt is already completed or still cooling down, so a recent
  // "Later" is not undone by finishing the wizard.
  useEffect(() => {
    if (!onboardingJustCompleted || !storage || !starNagEnabled) return;
    if (isStarNagCompleted(storage)) return;
    if (isStarNagCooldownActive(loadStarNagDeferredUntil(storage), Date.now())) return;
    promptVisibleRef.current = false;
    setStarCardVisible(false);
    void showStarPrompt("toast");
  }, [onboardingJustCompleted, showStarPrompt, starNagEnabled, storage]);

  // Feature tips: at most one per app open, first pending tip, marked seen before it opens.
  useEffect(() => {
    if (!enabled || !storage || promptedThisSessionRef.current) return;
    if (!featureTipsEnabled) return;
    const seen = normalizeFeatureTipIds(
      (() => {
        try {
          return JSON.parse(storage.getItem(FEATURE_TIPS_SEEN_IDS_STORAGE_KEY) ?? "[]");
        } catch {
          return [];
        }
      })(),
    );
    const decision = getFeatureTipsAppOpenDecision({
      onboardingPending,
      promptedThisSession: promptedThisSessionRef.current,
      suppressedByOnboardingThisSession: suppressedByOnboardingRef.current,
      persistedUIReady,
      activeModalIsNone,
      tipsDisabled: !featureTipsEnabled,
      cliInstalled,
      seenTipIds: seen,
      completed: completedTips,
    });
    if (decision.kind === "suppress-for-onboarding") {
      suppressedByOnboardingRef.current = true;
      return;
    }
    if (decision.kind !== "open") return;
    promptedThisSessionRef.current = true;
    const nextSeen = [...new Set([...seen, decision.tipId])];
    storage.setItem(FEATURE_TIPS_SEEN_IDS_STORAGE_KEY, JSON.stringify(nextSeen));
    setTipId(decision.tipId);
  }, [
    activeModalIsNone,
    cliInstalled,
    completedTips,
    enabled,
    featureTipsEnabled,
    onboardingPending,
    persistedUIReady,
    storage,
  ]);

  // Value moment: armed once per app version, then fired by the activity observer.
  useEffect(() => {
    if (!enabled || !storage || !appVersion || !starNagEnabled) return;
    if (hasConsumedStarNagValueMoment(storage)) return;
    const decision = getStarNagValueMomentDecision(storage, {
      appVersion,
      now: Date.now(),
      evaluating: evaluatingRef.current,
      promptVisible: promptVisibleRef.current,
    });
    if (decision === null) return;
    setValueMomentArmed(true);
  }, [appVersion, enabled, starNagEnabled, storage]);

  const onValueMoment = useCallback(() => {
    if (!storage || !appVersion) return;
    setValueMomentArmed(false);
    consumeStarNagValueMoment(storage, appVersion);
    void showStarPrompt("card");
  }, [appVersion, showStarPrompt, storage]);

  const tip = useMemo(() => (tipId ? getFeatureTip(tipId) ?? null : null), [tipId]);

  return {
    tip,
    closeTip: () => setTipId(null),
    starCardVisible,
    starCardMode,
    starCardBusy,
    starCardStar,
    starCardLater: deferPrompt,
    starCardDismiss: deferPrompt,
    starToastStar,
    starToastOpenWeb,
    starToastDefer: deferPrompt,
    valueMomentEnabled: valueMomentArmed && !starCardVisible && !starToastPending,
    onValueMoment,
    starToastPending,
    starToastMode,
    releaseStarPrompt: releasePrompt,
    runTipAction: onTipAction,
  };
}
