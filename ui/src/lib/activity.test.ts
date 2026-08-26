import { describe, expect, it } from "vitest";

import {
  combineActivitySummaries,
  resolveActivityIndicator,
  summarizeActivities,
  type ActivitySummary,
} from "./activity";

describe("activity summaries", () => {
  it("deduplicates display semantics into working/waiting/done counts", () => {
    const summary = summarizeActivities([
      { state: "working", title: "omo working", isAgent: true },
      { state: "working", title: "codex working", isAgent: true },
      { state: "waiting", title: "claude needs input", isAgent: true },
      { state: "done", title: "aider done", isAgent: true },
    ]);

    expect(summary).toEqual({
      workingCount: 2,
      waitingCount: 1,
      doneCount: 1,
      runningCount: 2,
      hasWorking: true,
      hasWaiting: true,
      hasDone: true,
      hasUnread: false,
    });
  });

  it("uses attention-first visual precedence", () => {
    const base: ActivitySummary = {
      workingCount: 1,
      waitingCount: 1,
      doneCount: 1,
      runningCount: 1,
      hasWorking: true,
      hasWaiting: true,
      hasDone: true,
      hasUnread: true,
    };
    expect(resolveActivityIndicator(base)).toBe("waiting");
    expect(resolveActivityIndicator({ ...base, waitingCount: 0, hasWaiting: false })).toBe("working");
    expect(resolveActivityIndicator({ ...base, waitingCount: 0, hasWaiting: false, workingCount: 0, runningCount: 0, hasWorking: false })).toBe("unread");
    expect(resolveActivityIndicator({ ...base, waitingCount: 0, hasWaiting: false, workingCount: 0, runningCount: 0, hasWorking: false, hasUnread: false })).toBe("done");
  });

  it("combines child summaries without losing unread attention", () => {
    const combined = combineActivitySummaries([
      summarizeActivities([{ state: "working", title: "one", isAgent: true }]),
      summarizeActivities([{ state: "done", title: "two", isAgent: true }], true),
    ]);
    expect(combined.workingCount).toBe(1);
    expect(combined.doneCount).toBe(1);
    expect(combined.runningCount).toBe(1);
    expect(combined.hasUnread).toBe(true);
  });

  it("picks the highest precedence agentType (waiting > working > done) and returns undefined when none carry one", () => {
    const mixed = summarizeActivities([
      { state: "done", title: "done agent", isAgent: true, agentType: "aider" },
      { state: "working", title: "working agent", isAgent: true, agentType: "codex" },
      { state: "waiting", title: "waiting agent", isAgent: true, agentType: "omo" },
    ]);
    expect(mixed.agentType).toBe("omo");

    const tiesFirstWins = summarizeActivities([
      { state: "working", title: "first working", isAgent: true, agentType: "first" },
      { state: "working", title: "second working", isAgent: true, agentType: "second" },
    ]);
    expect(tiesFirstWins.agentType).toBe("first");

    const none = summarizeActivities([
      { state: "working", title: "make test", isAgent: false },
    ]);
    expect(none.agentType).toBeUndefined();

    const combined = combineActivitySummaries([
      summarizeActivities([{ state: "working", title: "working", isAgent: true, agentType: "worker" }]),
      summarizeActivities([{ state: "waiting", title: "waiting", isAgent: true, agentType: "waiter" }]),
    ]);
    expect(combined.agentType).toBe("waiter");
  });
});