import { describe, expect, it, vi } from "vitest";

(globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(() => Promise.resolve()), listen: vi.fn(() => Promise.resolve(() => undefined)) }));

import {
  SessionClaimTracker,
  WindowFocusTracker,
  isBadgeLeader,
  publishWindowFocus,
  resetNotificationLeaderForTests,
  shouldNotifyForSession,
  shouldThisWindowNotifyFor,
  startWindowFocusBroadcast,
} from "./notificationLeader";

type FakeWindow = { label: string; focused: boolean; backendSessionIds: Set<string>; claims: SessionClaimTracker };

function fakeWindows(spec: Record<string, { focused?: boolean; sessions?: string[] }>): FakeWindow[] {
  const windows = Object.entries(spec).map(([label, value]) => ({
    label,
    focused: value.focused ?? false,
    backendSessionIds: new Set(value.sessions ?? []),
    claims: new SessionClaimTracker(label),
  }));
  for (const from of windows) {
    for (const to of windows) to.claims.apply({ label: from.label, backendSessionIds: [...from.backendSessionIds] });
  }
  return windows;
}

/** Labels of the windows that fire an OS notification for one backend event delivered to `receivers`. */
function notifiers(backendSessionId: string, receivers: FakeWindow[]): string[] {
  return receivers
    .filter((window) => shouldNotifyForSession({
      ownsSession: window.backendSessionIds.has(backendSessionId),
      isPrimary: window.label === "main",
      claimedByOtherWindow: window.claims.isClaimedByOtherWindow(backendSessionId),
    }))
    .map((window) => window.label);
}

describe("notification ownership rule", () => {
  it("(a) an unfocused owner notifies once while another window is focused", () => {
    const windows = fakeWindows({ main: { focused: true }, "ws-x": { sessions: ["b1"] } });
    // Session-scoped events reach only the owner window.
    expect(notifiers("b1", [windows[1]])).toEqual(["ws-x"]);
  });

  it("(b) a broadcast for a session held by ws-x notifies only ws-x", () => {
    const windows = fakeWindows({ main: { sessions: ["b0"] }, "ws-x": { sessions: ["b1"] } });
    expect(notifiers("b1", windows)).toEqual(["ws-x"]);
  });

  it("(c) a broadcast for a session no window holds notifies only main", () => {
    const windows = fakeWindows({ main: {}, "ws-x": { sessions: ["b1"] } });
    expect(notifiers("b-unknown", windows)).toEqual(["main"]);
  });

  it("(d) keeps single-window behavior: a lone window notifies for its own events", () => {
    const [main] = fakeWindows({ main: { sessions: ["b1"] } });
    expect(notifiers("b1", [main])).toEqual(["main"]);
    resetNotificationLeaderForTests();
    expect(shouldThisWindowNotifyFor("b1", true)).toBe(true);
    startWindowFocusBroadcast();
    publishWindowFocus(true);
    expect(shouldThisWindowNotifyFor("b1", true)).toBe(true);
    expect(shouldThisWindowNotifyFor("b-unknown", false)).toBe(true);
    resetNotificationLeaderForTests();
  });

  it("forgets a closed window's claims so main covers its sessions again", () => {
    const tracker = new SessionClaimTracker("main");
    tracker.apply({ label: "ws-x", backendSessionIds: ["b1"] });
    tracker.apply({ label: "main", backendSessionIds: ["b2"] });
    tracker.apply({ label: "../evil", backendSessionIds: ["b3"] });
    expect(tracker.isClaimedByOtherWindow("b1")).toBe(true);
    expect(tracker.isClaimedByOtherWindow("b2")).toBe(false);
    expect(tracker.isClaimedByOtherWindow("b3")).toBe(false);
    tracker.forget("ws-x");
    expect(tracker.isClaimedByOtherWindow("b1")).toBe(false);
  });
});

describe("dock badge leader", () => {
  it("gives the badge to the focused window, else the primary window", () => {
    expect(isBadgeLeader({ selfFocused: true, anyAppWindowFocused: true, isPrimary: false })).toBe(true);
    expect(isBadgeLeader({ selfFocused: false, anyAppWindowFocused: true, isPrimary: true })).toBe(false);
    expect(isBadgeLeader({ selfFocused: false, anyAppWindowFocused: false, isPrimary: true })).toBe(true);
    expect(isBadgeLeader({ selfFocused: false, anyAppWindowFocused: false, isPrimary: false })).toBe(false);
  });

  it("elects exactly one badge owner across two windows for every focus combination", () => {
    for (const focused of [null, "main", "ws-test"] as const) {
      const main = new WindowFocusTracker("main");
      const secondary = new WindowFocusTracker("ws-test");
      for (const tracker of [main, secondary]) {
        if (focused) tracker.apply({ label: focused, focused: true });
      }
      const badgeOwners = [main, secondary].filter((tracker) => isBadgeLeader({
        selfFocused: tracker.isSelfFocused(),
        anyAppWindowFocused: tracker.isAnyFocused(),
        isPrimary: tracker.selfLabel === "main",
      }));
      expect(badgeOwners.map((tracker) => tracker.selfLabel)).toEqual([focused ?? "main"]);
    }
  });

  it("tracks focus broadcasts and ignores malformed payloads", () => {
    const tracker = new WindowFocusTracker("main");
    const changed = vi.fn();
    tracker.subscribe(changed);
    tracker.apply({ label: "ws-a", focused: true });
    tracker.apply({ label: "ws-a", focused: true });
    tracker.apply({ label: "../evil", focused: true });
    tracker.apply({ label: "ws-b" });
    tracker.apply(null);
    expect(tracker.isAnyFocused()).toBe(true);
    expect(tracker.isSelfFocused()).toBe(false);
    expect(changed).toHaveBeenCalledOnce();
    tracker.forget("ws-a");
    expect(tracker.isAnyFocused()).toBe(false);
  });
});
