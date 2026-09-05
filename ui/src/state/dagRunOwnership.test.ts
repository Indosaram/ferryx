import { beforeEach, describe, expect, it } from "vitest";
import { dagRunOwnership } from "./dagRunOwnership";

describe("dagRunOwnership", () => {
  beforeEach(() => {
    dagRunOwnership.reset();
  });

  it("claims a run and reports its owner via ownerOf", () => {
    expect(dagRunOwnership.ownerOf("run-1")).toBeUndefined();

    dagRunOwnership.claim("run-1", "pane-1");
    expect(dagRunOwnership.ownerOf("run-1")).toBe("pane-1");

    // First claimer wins
    dagRunOwnership.claim("run-1", "pane-2");
    expect(dagRunOwnership.ownerOf("run-1")).toBe("pane-1");
  });

  it("isolates retain across projects when allKnownRunIdsInScope is provided", () => {
    // Project A claims run-A1
    dagRunOwnership.claim("run-A1", "pane-A");
    // Project B claims run-B1
    dagRunOwnership.claim("run-B1", "pane-B");

    // Project A retains active run-A1 within scope [run-A1, run-A0_completed]
    dagRunOwnership.retain(["run-A1"], ["run-A1", "run-A0_completed"]);

    // Verify run-B1 is STILL claimed by Project B
    expect(dagRunOwnership.ownerOf("run-B1")).toBe("pane-B");
    expect(dagRunOwnership.ownerOf("run-A1")).toBe("pane-A");
  });

  it("prunes completed run in Project A when scoped", () => {
    // Project A claims run-A0
    dagRunOwnership.claim("run-A0", "pane-A");

    // Project A runs retain with run-A1 active, run-A0 and run-A1 in scope
    dagRunOwnership.retain(["run-A1"], ["run-A0", "run-A1"]);

    // Verify run-A0 is deleted, run-A1 is retained (not claimed yet, but not in owners)
    expect(dagRunOwnership.ownerOf("run-A0")).toBeUndefined();
  });

  it("prunes completed run and preserves active run in Project A", () => {
    dagRunOwnership.claim("run-A0", "pane-A0");
    dagRunOwnership.claim("run-A1", "pane-A1");

    dagRunOwnership.retain(["run-A1"], ["run-A0", "run-A1"]);

    expect(dagRunOwnership.ownerOf("run-A0")).toBeUndefined();
    expect(dagRunOwnership.ownerOf("run-A1")).toBe("pane-A1");
  });

  it("preserves backwards compatibility when allKnownRunIdsInScope is omitted", () => {
    dagRunOwnership.claim("run-A", "pane-A");
    dagRunOwnership.claim("run-B", "pane-B");

    dagRunOwnership.retain(["run-A"]);

    expect(dagRunOwnership.ownerOf("run-A")).toBe("pane-A");
    expect(dagRunOwnership.ownerOf("run-B")).toBeUndefined();
  });
});
