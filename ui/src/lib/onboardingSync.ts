import {
  ACCOUNT_SESSION_CHANGED_EVENT, getConfiguredAccountOrigin, getStoredAccountSessionToken,
  syncAccountOnboarding, type AccountOnboardingProgress,
} from "../remote/accountSession";
import {
  hydrateOnboardingState, loadOnboardingState, saveOnboardingState,
  type OnboardingState, type OnboardingStepId,
} from "./onboarding";

const ACCOUNT_STEPS = new Set<OnboardingStepId>(["account", "intro", "features", "supervisor", "phone", "try"]);

export function accountProgress(state: OnboardingState): AccountOnboardingProgress {
  return {
    completedSteps: state.completedSteps.filter((step) => ACCOUNT_STEPS.has(step)),
    dismissed: state.dismissed,
    updatedAt: state.updatedAt ?? 0,
  };
}

export function mergeOnboardingProgress(local: OnboardingState, remote: AccountOnboardingProgress): OnboardingState {
  const incoming = remote.completedSteps.filter((step): step is OnboardingStepId => ACCOUNT_STEPS.has(step as OnboardingStepId));
  const ordered = remote.updatedAt > (local.updatedAt ?? 0)
    ? [...incoming, ...local.completedSteps] : [...local.completedSteps, ...incoming];
  return {
    version: 1,
    completedSteps: [...new Set(ordered)],
    dismissed: local.dismissed || remote.dismissed,
    updatedAt: Math.max(local.updatedAt ?? 0, remote.updatedAt),
  };
}

/** Local/daemon state is authoritative offline. Only whitelisted account step ids go online. */
export function startOnboardingSync(): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;
  let stopped = false;
  let applying = false;
  let queue = Promise.resolve();
  const cancel = () => { clearTimeout(timer); timer = undefined; };
  const report = (error: unknown) => console.warn("Onboarding sync unavailable; keeping local progress", error);
  const enqueue = (pull: boolean) => {
    const epoch = generation;
    const origin = getConfiguredAccountOrigin();
    const token = getStoredAccountSessionToken(origin);
    if (!token || stopped || navigator.onLine === false) return;
    const current = () => !stopped && epoch === generation && getStoredAccountSessionToken(origin) === token;
    queue = queue.then(async () => {
      if (!current()) return;
      if (pull) await hydrateOnboardingState();
      if (!current()) return;
      const remote = await syncAccountOnboarding(origin, token, pull ? undefined : accountProgress(loadOnboardingState()));
      if (!current()) return;
      const merged = mergeOnboardingProgress(loadOnboardingState(), remote);
      applying = true;
      try { saveOnboardingState(merged); } finally { applying = false; }
      if (pull) schedule();
    }).catch(report);
  };
  const schedule = () => {
    if (applying || stopped) return;
    cancel();
    timer = setTimeout(() => { timer = undefined; enqueue(false); }, 2000);
  };
  const session = () => { generation += 1; cancel(); enqueue(true); };
  window.addEventListener("ferryx:onboarding-changed", schedule);
  window.addEventListener(ACCOUNT_SESSION_CHANGED_EVENT, session);
  window.addEventListener("online", session);
  enqueue(true);
  return () => {
    stopped = true;
    cancel();
    window.removeEventListener("ferryx:onboarding-changed", schedule);
    window.removeEventListener(ACCOUNT_SESSION_CHANGED_EVENT, session);
    window.removeEventListener("online", session);
  };
}
