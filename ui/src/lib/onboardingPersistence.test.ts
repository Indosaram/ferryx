import { afterEach, expect, it, vi } from "vitest";
import { hydrateOnboardingState, markOnboardingStepsCompleted, loadOnboardingState, ONBOARDING_STEP_ORDER, initialWizardStepIndex } from "./onboarding";
import { ONBOARDING_STORAGE_KEY } from "./storageKeys";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke }));

afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  localStorage.clear();
  ipc.invoke.mockReset();
});

it("hydrates the file or daemon winner instead of trusting the browser cache", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  const saved = { version: 1, completedSteps: ["intro", "features", "agents"], dismissed: false };
  ipc.invoke.mockResolvedValue(saved);
  const state = await hydrateOnboardingState();
  expect(ipc.invoke).toHaveBeenCalledWith("cmd_onboarding_get", { cache: { version: 1, completedSteps: [], dismissed: false } });
  expect(loadOnboardingState()).toEqual(saved);
  expect(state).toEqual(saved);
  expect(initialWizardStepIndex(ONBOARDING_STEP_ORDER, state, {
    permissions: null, agents: null, cli: null, projectCount: 0,
    enrollmentReady: false, skillsReady: false,
  })).toBe(3);
});

it("serializes completion writes before the next hydration", async () => {
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  ipc.invoke.mockImplementation(async (command: string, args: { state?: unknown }) => {
    if (command === "cmd_onboarding_set") return args.state;
    return JSON.parse(localStorage.getItem(ONBOARDING_STORAGE_KEY)!);
  });
  markOnboardingStepsCompleted(["intro"]);
  markOnboardingStepsCompleted(["features"]);
  await hydrateOnboardingState();
  expect(ipc.invoke.mock.calls.map(([command]) => command)).toEqual([
    "cmd_onboarding_set", "cmd_onboarding_set", "cmd_onboarding_get",
  ]);
  expect(loadOnboardingState().completedSteps).toEqual(["intro", "features"]);
});
