import { describe, expect, it } from "vitest";
import { createAgentsSpawnedRecorder, loadAgentsSpawnedTotal } from "./agentsSpawnedCounter";

function createStorage(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
  };
}

describe("agents spawned counter", () => {
  it("counts each session once and persists the running total", () => {
    const storage = createStorage();
    const recorder = createAgentsSpawnedRecorder(storage);
    expect(recorder.record("a")).toBe(1);
    expect(recorder.record("a")).toBe(null);
    expect(recorder.record("b")).toBe(2);
    expect(loadAgentsSpawnedTotal(storage)).toBe(2);
  });

  it("resumes from the stored total", () => {
    const storage = createStorage({ "ferryx.agentsSpawnedTotal": "41" });
    const recorder = createAgentsSpawnedRecorder(storage);
    expect(recorder.total()).toBe(41);
    expect(recorder.record("a")).toBe(42);
  });

  it("ignores a corrupt stored value", () => {
    expect(loadAgentsSpawnedTotal(createStorage({ "ferryx.agentsSpawnedTotal": "nope" }))).toBe(0);
    expect(loadAgentsSpawnedTotal(createStorage({ "ferryx.agentsSpawnedTotal": "-3" }))).toBe(0);
  });
});
