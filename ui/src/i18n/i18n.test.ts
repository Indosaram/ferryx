import { describe, expect, it } from "vitest";

import { ONBOARDING_STEP_ORDER } from "../lib/onboarding";
import en from "./en.json";
import { i18n } from ".";

describe("i18n", () => {
  it("initialises synchronously in English with bundled resources", () => {
    expect(i18n.isInitialized).toBe(true);
    expect(i18n.language).toBe("en");
    expect(i18n.t("onboarding.wizard.title")).toBe("Welcome to Ferryx");
  });

  it("has a title, heading and lede for every onboarding step id", () => {
    for (const step of ONBOARDING_STEP_ORDER) {
      for (const field of ["title", "heading", "lede"] as const) {
        const key = `onboarding.step.${step}.${field}`;
        expect(i18n.exists(key), key).toBe(true);
      }
    }
    expect(Object.keys(en.onboarding.step).sort()).toEqual([...ONBOARDING_STEP_ORDER].sort());
  });

  it("interpolates named values without HTML escaping", () => {
    expect(i18n.t("onboarding.wizard.stepIndicator", { step: 2, total: 5 })).toBe("Step 2 of 5");
    expect(i18n.t("onboarding.wizard.reopenHint")).toBe("You can reopen this from Settings > Permissions.");
  });
});
