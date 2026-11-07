import { describe, expect, it } from "vitest";

import { fixtureOption } from "./phoneHomeFixtures";
import { optionForSession } from "./phoneHomeModel";

describe("optionForSession", () => {
  it("finds only an available option on the same machine", () => {
    const options = [
      fixtureOption({ machineId: "m1", sessionId: "s" }),
      fixtureOption({ machineId: "m2", sessionId: "s", unavailable: true }),
    ];
    expect(optionForSession(options, "m1", "s")?.machineId).toBe("m1");
    expect(optionForSession(options, "m2", "s")).toBeNull();
    expect(optionForSession(options, "m1", null)).toBeNull();
  });
});
