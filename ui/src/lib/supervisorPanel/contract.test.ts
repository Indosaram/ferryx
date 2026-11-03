import { describe, expect, it } from "vitest";

import { resumeText } from "./contract";
import { panelSnapshotFixture } from "./fixtures";

describe("supervisor panel contract", () => {
  it("fixture has 2 decisions with an in-range recommendation", () => {
    expect(panelSnapshotFixture.decisions).toHaveLength(2);
    for (const decision of panelSnapshotFixture.decisions) {
      expect(decision.recommendedIndex).toBeGreaterThanOrEqual(0);
      expect(decision.recommendedIndex).toBeLessThan(decision.prompt.options.length);
    }
  });

  it("resumes claude with continue", () => {
    expect(resumeText("claude")).toBe("continue");
  });
});
