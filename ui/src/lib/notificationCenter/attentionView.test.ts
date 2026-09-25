import { describe, expect, it } from "vitest";

import {
  actionableCount,
  attentionSectionOf,
  filterEntries,
  groupEntries,
  isActionable,
  isUnread,
  sortEntries,
} from "./attentionView";
import type { NotificationEntry } from "./types";

function entry(overrides: Partial<NotificationEntry> & Pick<NotificationEntry, "id">): NotificationEntry {
  return {
    workspaceId: "ws-1",
    sessionId: "s-1",
    labels: {},
    subject: "agent",
    reason: "waiting",
    firstOccurredAt: 1000,
    lastOccurredAt: 1000,
    updateOrder: 1,
    revision: 1,
    occurrenceCount: 1,
    read: { unread: true },
    ...overrides,
  };
}

describe("attentionSectionOf", () => {
  it("groups by what the user must do", () => {
    expect(attentionSectionOf(entry({ id: "a", reason: "waiting" }))).toBe("needs-you");
    expect(attentionSectionOf(entry({ id: "b", reason: "done" }))).toBe("finished");
    expect(attentionSectionOf(entry({ id: "c", reason: "bell" }))).toBe("alerts");
  });
});

describe("isActionable", () => {
  it("counts a blocked agent", () => {
    expect(isActionable(entry({ id: "a", reason: "waiting", read: { seen: true, seenAt: 5 } }))).toBe(true);
  });

  it("counts finished work only while it is unread", () => {
    expect(isActionable(entry({ id: "b", reason: "done", read: { unread: true } }))).toBe(true);
    expect(isActionable(entry({ id: "c", reason: "done", read: { seen: true, seenAt: 9 } }))).toBe(false);
  });

  it("never counts a plain mention", () => {
    expect(isActionable(entry({ id: "d", reason: "bell", read: { unread: true } }))).toBe(false);
  });

  it("sums across entries", () => {
    expect(
      actionableCount([
        entry({ id: "a", reason: "waiting" }),
        entry({ id: "b", reason: "done", read: { unread: true } }),
        entry({ id: "c", reason: "done", read: { seen: true, seenAt: 1 } }),
        entry({ id: "d", reason: "bell" }),
      ]),
    ).toBe(2);
  });
});

describe("sortEntries", () => {
  it("puts unread first, then most recent, then a stable id tie-break", () => {
    const sorted = sortEntries([
      entry({ id: "old-read", reason: "bell", read: { seen: true, seenAt: 1 }, lastOccurredAt: 10 }),
      entry({ id: "new-read", reason: "bell", read: { seen: true, seenAt: 1 }, lastOccurredAt: 90 }),
      entry({ id: "unread-old", reason: "waiting", read: { unread: true }, lastOccurredAt: 20 }),
      entry({ id: "unread-new", reason: "waiting", read: { unread: true }, lastOccurredAt: 50 }),
      entry({ id: "tie-a", reason: "bell", read: { seen: true, seenAt: 1 }, lastOccurredAt: 5 }),
      entry({ id: "tie-b", reason: "bell", read: { seen: true, seenAt: 1 }, lastOccurredAt: 5 }),
    ]);

    expect(sorted.map((item) => item.id)).toEqual([
      "unread-new",
      "unread-old",
      "new-read",
      "old-read",
      "tie-a",
      "tie-b",
    ]);
  });
});

describe("filterEntries", () => {
  const entries = [
    entry({ id: "waiting", reason: "waiting", read: { unread: true } }),
    entry({ id: "done-unread", reason: "done", read: { unread: true } }),
    entry({ id: "done-seen", reason: "done", read: { seen: true, seenAt: 2 } }),
    entry({ id: "bell", reason: "bell", read: { unread: true } }),
  ];

  it("returns everything for all", () => {
    expect(filterEntries(entries, "all")).toHaveLength(4);
  });

  it("returns unread for unread", () => {
    expect(filterEntries(entries, "unread").map((item) => item.id)).toEqual([
      "waiting",
      "done-unread",
      "bell",
    ]);
  });

  it("returns only actionable for needs-you", () => {
    expect(filterEntries(entries, "needs-you").map((item) => item.id)).toEqual(["waiting", "done-unread"]);
  });
});

describe("groupEntries", () => {
  it("emits non-empty sections in priority order", () => {
    const groups = groupEntries([
      entry({ id: "bell", reason: "bell" }),
      entry({ id: "done", reason: "done", read: { unread: true } }),
      entry({ id: "waiting", reason: "waiting" }),
    ]);

    expect(groups.map((group) => group.section)).toEqual(["needs-you", "finished", "alerts"]);
    expect(groups.map((group) => group.title)).toEqual(["Needs you", "Finished", "Mentions"]);
    expect(groups[0].items.map((item) => item.id)).toEqual(["waiting"]);
  });

  it("omits a section that has no items", () => {
    const groups = groupEntries([entry({ id: "bell", reason: "bell" })]);
    expect(groups.map((group) => group.section)).toEqual(["alerts"]);
  });

  it("reports unread state per entry", () => {
    expect(isUnread(entry({ id: "a", read: { unread: true } }))).toBe(true);
    expect(isUnread(entry({ id: "b", read: { seen: true, seenAt: 3 } }))).toBe(false);
  });
});
