import { useEffect, useState, type ReactNode } from "react";

import { cn } from "../lib/cn";
import { rdStats, type RdLatencyStats, type RdSessionStats } from "../lib/remoteDesktop";

/**
 * Delay between one settled `cmd_rd_stats` and the next ask. Deliberately a chained timeout and
 * never an interval: a slow backend must not be able to stack overlapping requests.
 */
const POLL_INTERVAL_MS = 500;
/** Structured code the backend returns once a viewer session no longer exists. */
const SESSION_GONE_CODE = "RD_NOT_FOUND";

const TRANSPORT_LABELS: Record<string, string> = {
  quic_datagram: "QUIC datagram",
  ws_direct: "Direct WS",
  ws_relay: "Relay WS",
};

/** A reading only counts while it belongs to the session currently on screen. */
type OverlayReading =
  | { phase: "live"; rdSessionId: string; stats: RdSessionStats }
  | { phase: "ended"; rdSessionId: string };

function ipcErrorCode(error: unknown): string | null {
  if (error && typeof error === "object" && "code" in error) {
    const code = (error as { code?: unknown }).code;
    if (typeof code === "string") return code;
  }
  return null;
}

/** Microseconds as a one-decimal millisecond figure (`13333` -> `13.3`). */
function formatMillis(micros: number): string {
  return (micros / 1000).toFixed(1);
}

function formatLatency(latency: RdLatencyStats | null): string {
  if (latency === null) return "—";
  return `${formatMillis(latency.p50Us)} / ${formatMillis(latency.p95Us)} / ${formatMillis(latency.p99Us)} ms`;
}

function formatBitrate(stats: RdSessionStats): string {
  // Both sides are already bits/s: the ABR target is negotiated bits/s and the receiver reports
  // `udp_receive_bps` as interval bytes * 8 / elapsed seconds. Prefer the negotiated target.
  const bitsPerSecond = stats.abrTargetBitrate ?? stats.udpReceiveBps;
  return bitsPerSecond === null ? "—" : `${(bitsPerSecond / 1_000_000).toFixed(1)} Mbps`;
}

function formatLoss(ratio: number | null): string {
  return ratio === null ? "—" : `${(ratio * 100).toFixed(1)}% loss`;
}

function formatScale(permille: number): string {
  const percent = permille / 10;
  return `${Number.isInteger(percent) ? percent.toFixed(0) : percent.toFixed(1)}%`;
}

/** Success while the stream keeps up with the panel's refresh target, warning when it lags. */
function fpsTone(stats: RdSessionStats): string {
  if (stats.targetFps <= 0) return "text-foreground";
  return stats.presentFps >= stats.targetFps * 0.9 ? "text-status-success" : "text-status-warning";
}

function Metric({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span className="whitespace-nowrap text-right tabular-nums text-foreground">{children}</span>
    </div>
  );
}

export type RemoteDesktopStatsOverlayProps = {
  rdSessionId: string | null;
  /** Toggled by the viewer toolbar or its stats keyboard shortcut. */
  open: boolean;
  /** False while the owning tab is hidden. */
  visible: boolean;
  className?: string;
};

/**
 * Compact telemetry HUD for one viewer session. It renders top-right inside a `relative` pane and
 * never takes pointer input, so it cannot steal a click or a wheel tick from the streamed desktop.
 * `cmd_rd_stats` is re-read on a chained 500 ms timer that only exists while
 * `open && visible && rdSessionId` hold: hiding the tab, closing the HUD, switching sessions, or
 * unmounting all stop the chain, and `RD_NOT_FOUND` ends it for good.
 */
export function RemoteDesktopStatsOverlay({ rdSessionId, open, visible, className }: RemoteDesktopStatsOverlayProps) {
  const [reading, setReading] = useState<OverlayReading | null>(null);
  const active = open && visible && Boolean(rdSessionId);

  useEffect(() => {
    if (!open || !visible || !rdSessionId) return;
    const sessionId = rdSessionId;
    let cancelled = false;
    let timer: number | undefined;

    const poll = async () => {
      if (cancelled) return;
      try {
        const next = await rdStats(sessionId);
        if (cancelled) return;
        setReading({ phase: "live", rdSessionId: sessionId, stats: next });
      } catch (error) {
        if (cancelled) return;
        if (ipcErrorCode(error) === SESSION_GONE_CODE) {
          // The session ended underneath the HUD: stop asking instead of polling a dead id.
          cancelled = true;
          setReading({ phase: "ended", rdSessionId: sessionId });
          return;
        }
        // A transient failure keeps the last reading on screen and retries on the next tick.
      }
      timer = window.setTimeout(() => {
        void poll();
      }, POLL_INTERVAL_MS);
    };

    void poll();

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [open, visible, rdSessionId]);

  if (!active) return null;

  const current = reading && reading.rdSessionId === rdSessionId ? reading : null;
  const stats = current && current.phase === "live" ? current.stats : null;

  return (
    <div
      data-testid="remote-desktop-stats-overlay"
      data-native-terminal-yield="off"
      className={cn(
        "pointer-events-none absolute right-2 top-2 z-20 select-none rounded-md border border-border bg-background/85 px-2.5 py-2 font-mono text-[10px] leading-tight text-foreground shadow-elevation-2 backdrop-blur-sm",
        className,
      )}
    >
      {stats ? (
        <>
          <div className={cn("text-[11px] font-semibold tabular-nums", fpsTone(stats))}>
            {`${stats.presentFps.toFixed(1)} / ${stats.targetFps} Hz`}
          </div>
          <div className="mt-0.5 text-muted-foreground">
            {`${stats.presentBackend} · ${stats.hwAccel} · ${stats.codec}`}
          </div>

          <div className="mt-1 flex flex-col gap-0.5">
            <Metric label="Transport">{TRANSPORT_LABELS[stats.transportMode] ?? stats.transportMode}</Metric>
            <Metric label="Resolution">
              {`${stats.frameWidth}×${stats.frameHeight} (${formatScale(stats.scalePermille)})`}
            </Metric>
            <Metric label="Bitrate">{`${formatBitrate(stats)} · ${formatLoss(stats.packetLossRatio)}`}</Metric>
          </div>

          <div className="my-1 border-t border-border/60" />

          <div className="text-muted-foreground">p50 / p95 / p99 ms</div>
          <div className="mt-0.5 flex flex-col gap-0.5">
            <Metric label="Decode">{formatLatency(stats.decodeUs)}</Metric>
            <Metric label="Present">{formatLatency(stats.presentUs)}</Metric>
            <Metric label="Network">{formatLatency(stats.networkUs)}</Metric>
            <Metric label="Capture→Present">{formatLatency(stats.captureToPresentUs)}</Metric>
          </div>

          <div className="my-1 border-t border-border/60" />

          <Metric label="Dropped frames">{stats.droppedFrames}</Metric>
          <Metric label="Audio">
            {`${stats.audioActive ? "On" : "Off"} · ${stats.audioUnderruns} underruns · ${stats.audioDrops} drops`}
          </Metric>
        </>
      ) : (
        <span className="text-muted-foreground">
          {current && current.phase === "ended" ? "Session ended" : "Waiting for telemetry…"}
        </span>
      )}
    </div>
  );
}
