import { describe, expect, it, vi } from "vitest";

import { parseForwardedNotificationTarget, routeNotificationTarget } from "./notificationForwarding";

const target = { workspaceId: "w1", sessionId: "s1" };

describe("routeNotificationTarget", () => {
  it("navigates a click the draining window owns and does not forward it", () => {
    const navigate = vi.fn();
    const forward = vi.fn();
    expect(routeNotificationTarget({ target, ownsTarget: () => true, navigate, forward })).toBe("navigated");
    expect(navigate).toHaveBeenCalledOnce();
    expect(forward).not.toHaveBeenCalled();
  });

  it("forwards a click for a session another window owns and does not navigate itself", () => {
    const navigate = vi.fn();
    const forward = vi.fn();
    expect(routeNotificationTarget({ target, ownsTarget: () => false, navigate, forward })).toBe("forwarded");
    expect(forward).toHaveBeenCalledWith(target);
    expect(navigate).not.toHaveBeenCalled();
  });
});

describe("parseForwardedNotificationTarget", () => {
  it("accepts { from, target } and rejects malformed payloads", () => {
    expect(parseForwardedNotificationTarget({ from: "main", target })).toEqual({ from: "main", target });
    expect(parseForwardedNotificationTarget({ from: "main", target: { workspaceId: "w1" } })).toBeNull();
    expect(parseForwardedNotificationTarget({ target })).toBeNull();
    expect(parseForwardedNotificationTarget(null)).toBeNull();
  });
});
