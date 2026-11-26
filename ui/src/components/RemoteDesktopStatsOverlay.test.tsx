import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RdSessionStats } from "../lib/remoteDesktop";

const { rdStats } = vi.hoisted(() => ({ rdStats: vi.fn() }));

// The HUD talks to the backend only through `rdStats`; every other import is type-only.
vi.mock("../lib/remoteDesktop", () => ({ rdStats }));

import { RemoteDesktopStatsOverlay } from "./RemoteDesktopStatsOverlay";

// Captured before fake timers install: the unhandled-rejection check needs one real macrotask
// turn (Node reports a leaked rejection at a macrotask boundary), not a wall-clock sleep.
const nativeSetTimeout = globalThis.setTimeout.bind(globalThis);

function latency(p50Us: number, p95Us: number, p99Us: number) {
  return { frames: 75, p50Us, p95Us, p99Us, maxUs: p99Us * 2 };
}

function stats(overrides: Partial<RdSessionStats> = {}): RdSessionStats {
  return {
    rdSessionId: "rd-1",
    targetFps: 75,
    presentFps: 75,
    droppedFrames: 3,
    codec: "hevc",
    hwAccel: "videotoolbox",
    presentBackend: "wgpu",
    transportMode: "quic_datagram",
    playoutMode: "adaptive",
    frameWidth: 1920,
    frameHeight: 1080,
    scalePermille: 1000,
    audioActive: true,
    audioUnderruns: 2,
    audioDrops: 1,
    decodeUs: latency(1_200, 2_400, 3_600),
    presentUs: latency(800, 1_600, 2_400),
    totalClientUs: null,
    udpReceiveBps: null,
    packetLossRatio: 0,
    abrTargetBitrate: 24_500_000,
    hostEncodeUs: null,
    networkUs: latency(4_000, 8_000, 12_000),
    captureToPresentUs: latency(13_000, 26_000, 39_000),
    inputToDrawSubmitUs: null,
    audioVideoSourceOffsetUs: -1_000,
    ...overrides,
  };
}

/** Advances the fake clock and lets every promise chain it starts settle. */
async function tick(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
    await Promise.resolve();
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  rdStats.mockReset();
  rdStats.mockResolvedValue(stats());
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("RemoteDesktopStatsOverlay", () => {
  it("does not ask for stats unless the HUD is open, visible, and bound to a session", async () => {
    const closed = render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open={false} visible />);
    await tick(2_000);
    expect(rdStats).not.toHaveBeenCalled();
    expect(screen.queryByTestId("remote-desktop-stats-overlay")).not.toBeInTheDocument();
    closed.unmount();

    const hidden = render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible={false} />);
    await tick(2_000);
    expect(rdStats).not.toHaveBeenCalled();
    expect(screen.queryByTestId("remote-desktop-stats-overlay")).not.toBeInTheDocument();
    hidden.unmount();

    render(<RemoteDesktopStatsOverlay rdSessionId={null} open visible />);
    await tick(2_000);
    expect(rdStats).not.toHaveBeenCalled();
    expect(screen.queryByTestId("remote-desktop-stats-overlay")).not.toBeInTheDocument();
  });

  it("renders FPS, pipeline, transport, resolution, bitrate, and p50/p95/p99 per stage", async () => {
    render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);

    expect(rdStats).toHaveBeenCalledWith("rd-1");
    const overlay = screen.getByTestId("remote-desktop-stats-overlay");
    expect(screen.getByText("75.0 / 75 Hz")).toBeInTheDocument();
    expect(overlay).toHaveTextContent("wgpu · videotoolbox · hevc");
    expect(overlay).toHaveTextContent("QUIC datagram");
    expect(overlay).toHaveTextContent("1920×1080 (100%)");
    expect(overlay).toHaveTextContent("24.5 Mbps · 0.0% loss");
    expect(overlay).toHaveTextContent("p50 / p95 / p99 ms");
    // Every stage prints all three percentiles, so a dropped p99 regression fails here.
    expect(overlay).toHaveTextContent("1.2 / 2.4 / 3.6 ms");
    expect(overlay).toHaveTextContent("0.8 / 1.6 / 2.4 ms");
    expect(overlay).toHaveTextContent("4.0 / 8.0 / 12.0 ms");
    expect(overlay).toHaveTextContent("13.0 / 26.0 / 39.0 ms");
    expect(overlay).toHaveTextContent("Dropped frames");
    expect(overlay).toHaveTextContent("On · 2 underruns · 1 drops");
  });

  it("labels each transport mode in human terms", async () => {
    const view = render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);
    const overlay = () => screen.getByTestId("remote-desktop-stats-overlay");

    rdStats.mockResolvedValue(stats({ transportMode: "ws_direct" }));
    await tick(500);
    expect(overlay()).toHaveTextContent("Direct WS");

    rdStats.mockResolvedValue(stats({ transportMode: "ws_relay" }));
    await tick(500);
    expect(overlay()).toHaveTextContent("Relay WS");

    view.unmount();
  });

  it("falls back to the receiver's bits/s rate and shows an em dash when both are absent", async () => {
    const view = render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);
    const overlay = () => screen.getByTestId("remote-desktop-stats-overlay");

    // `udp_receive_bps` is already bits/s (interval bytes * 8 / elapsed seconds), so no *8 here:
    // a bytes/s misread would print 196.0 Mbps instead of 24.5 Mbps.
    rdStats.mockResolvedValue(stats({ abrTargetBitrate: null, udpReceiveBps: 24_500_000 }));
    await tick(500);
    expect(overlay()).toHaveTextContent("24.5 Mbps · 0.0% loss");
    expect(overlay()).not.toHaveTextContent("196.0 Mbps");

    // The negotiated ABR target still wins when the host has confirmed it.
    rdStats.mockResolvedValue(stats({ abrTargetBitrate: 12_000_000, udpReceiveBps: 24_500_000 }));
    await tick(500);
    expect(overlay()).toHaveTextContent("12.0 Mbps");

    rdStats.mockResolvedValue(stats({ abrTargetBitrate: null, udpReceiveBps: null }));
    await tick(500);
    expect(overlay()).toHaveTextContent("— · 0.0% loss");

    view.unmount();
  });

  it("never overlaps cmd_rd_stats: the next ask is scheduled only after the previous settles", async () => {
    let settleFirst: (value: RdSessionStats) => void = () => undefined;
    rdStats.mockImplementationOnce(
      () => new Promise<RdSessionStats>((resolve) => {
        settleFirst = resolve;
      }),
    );

    render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);
    expect(rdStats).toHaveBeenCalledTimes(1);

    // Several 500 ms periods pass while the first ask is unresolved: a chained timeout cannot
    // fire, and an interval (or a fire-and-forget loop) would already have stacked calls.
    await tick(2_000);
    expect(rdStats).toHaveBeenCalledTimes(1);

    await act(async () => {
      settleFirst(stats());
    });
    // Settling schedules the next ask; it must not have fired yet.
    await tick(0);
    expect(rdStats).toHaveBeenCalledTimes(1);

    await tick(500);
    expect(rdStats).toHaveBeenCalledTimes(2);
  });

  it("stops polling cleanly when cmd_rd_stats reports RD_NOT_FOUND", async () => {
    const unhandled: unknown[] = [];
    const recordUnhandled = (reason: unknown) => {
      unhandled.push(reason);
    };
    process.on("unhandledRejection", recordUnhandled);
    try {
      rdStats.mockRejectedValue({ code: "RD_NOT_FOUND", message: "ended", details: {} });
      render(<RemoteDesktopStatsOverlay rdSessionId="rd-gone" open visible />);
      await tick(0);
      expect(rdStats).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("remote-desktop-stats-overlay")).toHaveTextContent("Session ended");

      await tick(2_000);
      expect(rdStats).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId("remote-desktop-stats-overlay")).toHaveTextContent("Session ended");

      // A leaked rejection is reported at the next macrotask boundary; hand the loop one real turn.
      await new Promise<void>((resolve) => {
        nativeSetTimeout(resolve, 0);
      });
      expect(unhandled).toEqual([]);
    } finally {
      process.off("unhandledRejection", recordUnhandled);
    }
  });

  it("stops polling when the pane is hidden, the session changes, or the HUD unmounts", async () => {
    const view = render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);
    expect(rdStats).toHaveBeenCalledTimes(1);

    view.rerender(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible={false} />);
    await tick(2_000);
    expect(rdStats).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("remote-desktop-stats-overlay")).not.toBeInTheDocument();

    view.rerender(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);
    expect(rdStats).toHaveBeenCalledTimes(2);
    expect(rdStats).toHaveBeenLastCalledWith("rd-1");

    view.rerender(<RemoteDesktopStatsOverlay rdSessionId="rd-2" open visible />);
    await tick(0);
    expect(rdStats).toHaveBeenLastCalledWith("rd-2");

    view.unmount();
    const callsAfterUnmount = rdStats.mock.calls.length;
    await tick(2_000);
    expect(rdStats).toHaveBeenCalledTimes(callsAfterUnmount);
  });

  it("floats without taking input and only uses semantic theme tokens", async () => {
    render(<RemoteDesktopStatsOverlay rdSessionId="rd-1" open visible />);
    await tick(0);

    const overlay = screen.getByTestId("remote-desktop-stats-overlay");
    expect(overlay).toHaveAttribute("data-native-terminal-yield", "off");
    expect(overlay.className).toContain("pointer-events-none");
    expect(overlay.className).toContain("select-none");
    expect(overlay.className).toContain("bg-background/85");
    expect(overlay.className).toContain("border-border");
    expect(overlay.className).toContain("text-foreground");
    expect(overlay.className).toContain("font-mono");
    // Hex values and raw palette classes would break the appearance theme contract.
    expect(overlay.className).not.toMatch(/#[0-9a-f]{3,8}/i);
    expect(overlay.className).not.toMatch(/\b(?:bg|text|border)-(?:black|white|neutral|slate|gray|zinc)\b/);
  });
});
