import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { muteSessionPush } from "../lib/pushSubscription";
import { PANEL_PRESENCE_HEARTBEAT_MS, reportPanelVisible, usePanelPresence } from "./panelPresence";

function bodies(request: ReturnType<typeof vi.fn>): boolean[] {
  return request.mock.calls.map(([, init]) => JSON.parse(String((init as RequestInit).body)).visible);
}

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, value: state });
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("panel presence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  });
  afterEach(() => {
    vi.useRealTimers();
    Reflect.deleteProperty(document, "visibilityState");
  });

  it("reports shown at once, heartbeats well inside the gateway's 120 s memory, and hidden on unmount", () => {
    const request = vi.fn(async () => new Response(null, { status: 204 }));
    const { unmount } = renderHook(() => usePanelPresence(request, true));
    expect(request).toHaveBeenCalledWith("/api/v1/push/panel-presence", expect.objectContaining({ method: "POST" }));
    expect(bodies(request)).toEqual([true]);
    expect(PANEL_PRESENCE_HEARTBEAT_MS).toBeLessThan(120_000 / 2);

    vi.advanceTimersByTime(PANEL_PRESENCE_HEARTBEAT_MS * 2);
    expect(bodies(request)).toEqual([true, true, true]);
    unmount();
    expect(bodies(request)).toEqual([true, true, true, false]);
    vi.advanceTimersByTime(PANEL_PRESENCE_HEARTBEAT_MS * 3);
    expect(request).toHaveBeenCalledTimes(4);
  });

  it("is hidden while the page is in the background and shown again on return", () => {
    const request = vi.fn(async () => new Response(null, { status: 204 }));
    renderHook(() => usePanelPresence(request, true));
    setVisibility("hidden");
    expect(bodies(request)).toEqual([true, false]);
    vi.advanceTimersByTime(PANEL_PRESENCE_HEARTBEAT_MS * 2);
    expect(request).toHaveBeenCalledTimes(2);
    setVisibility("visible");
    expect(bodies(request)).toEqual([true, false, true]);
  });

  it("reports nothing while the panel is not the screen, and hidden when it stops being", () => {
    const request = vi.fn(async () => new Response(null, { status: 204 }));
    const { rerender } = renderHook(({ shown }) => usePanelPresence(request, shown), { initialProps: { shown: false } });
    expect(request).not.toHaveBeenCalled();
    rerender({ shown: true });
    expect(bodies(request)).toEqual([true]);
    rerender({ shown: false });
    expect(bodies(request)).toEqual([true, false]);
  });

  it("reports nothing without a request (not paired)", () => {
    renderHook(() => usePanelPresence(null, true));
    vi.advanceTimersByTime(PANEL_PRESENCE_HEARTBEAT_MS * 2);
  });

  it("treats a failed report as best effort", async () => {
    await expect(reportPanelVisible(vi.fn(async () => new Response(null, { status: 500 })), true)).resolves.toBe(false);
    await expect(reportPanelVisible(vi.fn(async () => { throw new Error("offline"); }), true)).resolves.toBe(false);
    await expect(reportPanelVisible(vi.fn(async () => new Response(null, { status: 204 })), true)).resolves.toBe(true);
  });
});

describe("muteSessionPush", () => {
  it("posts the exact session ref and the asked-for state", async () => {
    const request = vi.fn(async () => new Response(null, { status: 204 }));
    await expect(muteSessionPush(request, "mac:s1", true)).resolves.toBe(true);
    expect(request).toHaveBeenCalledWith("/api/v1/push/session-mute", expect.objectContaining({
      method: "POST",
      body: JSON.stringify({ sessionRef: "mac:s1", muted: true }),
    }));
    await muteSessionPush(request, "mac:s1", false);
    expect(JSON.parse(String((request.mock.calls[1][1] as RequestInit).body))).toEqual({ sessionRef: "mac:s1", muted: false });
  });

  it("reports false when the gateway refuses or cannot be reached, so the row keeps its real state", async () => {
    await expect(muteSessionPush(vi.fn(async () => new Response(null, { status: 403 })), "mac:s1", true)).resolves.toBe(false);
    await expect(muteSessionPush(vi.fn(async () => { throw new Error("offline"); }), "mac:s1", true)).resolves.toBe(false);
  });
});
