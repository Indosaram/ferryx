import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { accountProgress, mergeOnboardingProgress, startOnboardingSync } from "./onboardingSync";
import { loadOnboardingState, markOnboardingStepsCompleted } from "./onboarding";
import { ACCOUNT_SESSION_CHANGED_EVENT } from "../remote/accountSession";

const api = vi.hoisted(() => ({ token: null as string | null, sync: vi.fn() }));
vi.mock("../remote/accountSession", () => ({
  ACCOUNT_SESSION_CHANGED_EVENT: "ferryx:account-session",
  getConfiguredAccountOrigin: () => "https://relay.ferryx.dev",
  getStoredAccountSessionToken: () => api.token,
  syncAccountOnboarding: api.sync,
}));

let stop: (() => void) | undefined;
beforeEach(() => {
  localStorage.clear();
  api.token = null;
  api.sync.mockReset();
  Object.defineProperty(navigator, "onLine", { configurable: true, value: true });
});
afterEach(() => { stop?.(); stop = undefined; vi.useRealTimers(); });

function nextSave(): Promise<void> {
  return new Promise((resolve) => {
    window.addEventListener("ferryx:onboarding-changed", () => resolve(), { once: true });
  });
}

it("unions progress, ORs dismissal, and uses newest ordering without exporting machine steps", () => {
  const merged = mergeOnboardingProgress({
    version: 1, completedSteps: ["cli", "intro", "skills", "agents", "project", "enroll"],
    dismissed: true, updatedAt: 10,
  }, { completedSteps: ["phone", "features", "enroll"], dismissed: false, updatedAt: 20 });
  expect(merged.completedSteps).toEqual(["phone", "features", "cli", "intro", "skills", "agents", "project", "enroll"]);
  expect(accountProgress(merged)).toEqual({ completedSteps: ["phone", "features", "intro"], dismissed: true, updatedAt: 20 });
  expect(mergeOnboardingProgress(merged, { completedSteps: ["account"], dismissed: false, updatedAt: 5 })
    .completedSteps).toEqual([...merged.completedSteps, "account"]);
});

it("pulls at signed-in startup and pushes each change after a two-second debounce", async () => {
  vi.useFakeTimers();
  api.token = "token";
  api.sync.mockImplementation(async (_origin, _token, body) => body ?? {
    completedSteps: ["phone"], dismissed: true, updatedAt: 1,
  });
  const pulled = nextSave();
  stop = startOnboardingSync();
  await pulled;
  expect(api.sync).toHaveBeenCalledWith("https://relay.ferryx.dev", "token", undefined);
  markOnboardingStepsCompleted(["intro", "cli"]);
  await vi.advanceTimersByTimeAsync(1999);
  expect(api.sync).toHaveBeenCalledTimes(1);
  markOnboardingStepsCompleted(["features"]);
  await vi.advanceTimersByTimeAsync(1999);
  expect(api.sync).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(api.sync).toHaveBeenCalledTimes(2);
  expect(api.sync.mock.calls[1][2]).toMatchObject({ completedSteps: ["intro", "features", "phone"], dismissed: true });
  expect(loadOnboardingState().completedSteps).toContain("cli");
});

it("keeps unsigned and offline changes local and pulls on sign-in and reconnection", async () => {
  vi.useFakeTimers();
  api.sync.mockResolvedValue({ completedSteps: ["intro"], dismissed: false, updatedAt: 1 });
  stop = startOnboardingSync();
  markOnboardingStepsCompleted(["cli"]);
  await vi.advanceTimersByTimeAsync(2000);
  expect(api.sync).not.toHaveBeenCalled();
  api.token = "signed-in";
  let pulled = nextSave();
  window.dispatchEvent(new Event(ACCOUNT_SESSION_CHANGED_EVENT));
  await pulled;
  expect(api.sync).toHaveBeenCalledTimes(1);
  Object.defineProperty(navigator, "onLine", { configurable: true, value: false });
  markOnboardingStepsCompleted(["features"]);
  await vi.advanceTimersByTimeAsync(2000);
  expect(api.sync).toHaveBeenCalledTimes(1);
  Object.defineProperty(navigator, "onLine", { configurable: true, value: true });
  pulled = nextSave();
  window.dispatchEvent(new Event("online"));
  await pulled;
  expect(api.sync).toHaveBeenCalledTimes(2);
  expect(loadOnboardingState().completedSteps).toContain("features");
});

it("does not apply a late response after logout", async () => {
  let finish!: (value: { completedSteps: string[]; dismissed: boolean; updatedAt: number }) => void;
  let requested!: () => void;
  const started = new Promise<void>((resolve) => { requested = resolve; });
  const response = new Promise<{ completedSteps: string[]; dismissed: boolean; updatedAt: number }>((resolve) => { finish = resolve; });
  api.token = "old-token";
  api.sync.mockImplementation(() => { requested(); return response; });
  stop = startOnboardingSync();
  await started;
  api.token = null;
  window.dispatchEvent(new Event(ACCOUNT_SESSION_CHANGED_EVENT));
  finish({ completedSteps: ["phone"], dismissed: true, updatedAt: 2 });
  await response;
  expect(loadOnboardingState().completedSteps).toEqual([]);
});
