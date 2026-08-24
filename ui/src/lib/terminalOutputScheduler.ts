import type { Terminal } from "@xterm/xterm";

import { attachTerminal } from "./tauri";
import {
  terminalEventBus,
  type TerminalOutputChunk,
  type TerminalRuntimeReplayGap,
} from "./terminalEvents";
import { decodeBase64 } from "./terminalOutput";
import type { AttachTerminalResponse } from "./types";

// Kept under the existing export name for compatibility with host-manager tests/callers.
// The scheduler now counts raw output bytes rather than decoded UTF-16 characters.
export const MAX_PENDING_OUTPUT_CHARS = 128 * 1024;
const textEncoder = new TextEncoder();

export type TerminalOutputWriter = Pick<Terminal, "write"> & {
  reset?(): void;
};

export type ScheduledOutputSubscriptionOptions = {
  initialSequence?: string | null;
  daemonEpoch?: string | null;
  onSequenceUpdate?: (sequence: string, daemonEpoch?: string | null) => void;
  onGap?: () => void;
  attachFn?: (request: { sessionId: string; afterSequence?: string | null }) => Promise<AttachTerminalResponse>;
};

export type TerminalOutputSchedulerMetrics = {
  receivedChunks: number;
  receivedBytes: number;
  writes: number;
  frameFlushes: number;
  thresholdFlushes: number;
  coalescedChunks: number;
  totalFrameWaitMs: number;
  maxFrameWaitMs: number;
};

const schedulerMetrics: TerminalOutputSchedulerMetrics = {
  receivedChunks: 0,
  receivedBytes: 0,
  writes: 0,
  frameFlushes: 0,
  thresholdFlushes: 0,
  coalescedChunks: 0,
  totalFrameWaitMs: 0,
  maxFrameWaitMs: 0,
};

function schedulerNow(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}

function recordSchedulerFlush(
  reason: "frame" | "threshold",
  chunkCount: number,
  frameWaitMs: number,
) {
  if (!import.meta.env.DEV) return;
  schedulerMetrics.writes += 1;
  schedulerMetrics.coalescedChunks += Math.max(0, chunkCount - 1);
  if (reason === "frame") {
    schedulerMetrics.frameFlushes += 1;
    schedulerMetrics.totalFrameWaitMs += frameWaitMs;
    schedulerMetrics.maxFrameWaitMs = Math.max(schedulerMetrics.maxFrameWaitMs, frameWaitMs);
  } else {
    schedulerMetrics.thresholdFlushes += 1;
  }

  if (schedulerMetrics.writes % 120 === 0) {
    const averageFrameWaitMs = schedulerMetrics.frameFlushes > 0
      ? schedulerMetrics.totalFrameWaitMs / schedulerMetrics.frameFlushes
      : 0;
    console.debug("[terminal-output] scheduler", {
      ...schedulerMetrics,
      averageFrameWaitMs: Number(averageFrameWaitMs.toFixed(2)),
    });
  }
}

export function getTerminalOutputSchedulerMetricsForTest(): TerminalOutputSchedulerMetrics {
  return { ...schedulerMetrics };
}

export function resetTerminalOutputSchedulerMetricsForTest(): void {
  for (const key of Object.keys(schedulerMetrics) as Array<keyof TerminalOutputSchedulerMetrics>) {
    schedulerMetrics[key] = 0;
  }
}

function parseSequence(seq?: string | null): bigint | null {
  if (seq === undefined || seq === null || seq === "") return null;
  try {
    return BigInt(seq);
  } catch {
    return null;
  }
}

function toOutputBytes(data: TerminalOutputChunk): Uint8Array {
  return typeof data === "string" ? textEncoder.encode(data) : data;
}

function concatOutputBytes(chunks: Uint8Array[], totalBytes: number): Uint8Array {
  if (chunks.length === 1) return chunks[0] ?? new Uint8Array();
  const merged = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return merged;
}

/**
 * Batches incoming PTY output chunks per terminal renderer at animation-frame cadence
 * with protocol-v2 typed attachment, history-before-live ordering, and sequence de-duplication.
 *
 * Queue policy:
 * - On initial subscription, calls `attachTerminal` with `initialSequence`.
 * - Decodes base64 history once to bytes and renders it before live output.
 * - Suppresses live output chunks where sequence <= historyEndSequence or sequence <= lastAppliedSequence.
 * - If an explicit gap or daemon epoch mismatch is detected, triggers terminal reset and replays full history.
 * - Runtime replay-gap boundaries discard pending pre-gap output, reset xterm/sequence state, replay the supplied history,
 *   then resume live output after the replay end sequence.
 * - Chunks received between rAF flushes are held in an ordered per-renderer byte buffer.
 * - On frame flush, all buffered chunks coalesce into a single `terminal.write(Uint8Array)` call in arrival order.
 * - If queued bytes reach or exceed `MAX_PENDING_OUTPUT_CHARS` before rAF, the buffer is flushed synchronously
 *   to guarantee memory safety without loss or reordering.
 * - In development builds, lightweight counters record rAF wait, coalescing, and threshold flush behavior.
 * - Teardown (via unlisten or manager destroy) immediately cancels pending rAF callbacks and clears buffered data.
 */
export function attachScheduledOutputSubscription(
  backendSessionId: string,
  terminal: TerminalOutputWriter,
  options?: ScheduledOutputSubscriptionOptions,
): () => void {
  let outputBuffer: Uint8Array[] = [];
  let outputBufferBytes = 0;
  let outputFrame = 0;
  let outputFrameScheduledAt = 0;
  let active = true;
  let attached = false;
  let streamGeneration = 0;
  let lastAppliedSequence: bigint | null = parseSequence(options?.initialSequence);
  let currentDaemonEpoch: string | null = options?.daemonEpoch ?? null;
  let historyEndSequence: bigint | null = null;
  const queuedLiveChunks: Array<{ data: Uint8Array; sequence?: string | null; daemonEpoch?: string | null }> = [];

  const clearPendingOutput = () => {
    if (outputFrame !== 0) {
      cancelAnimationFrame(outputFrame);
      outputFrame = 0;
    }
    outputFrameScheduledAt = 0;
    outputBuffer = [];
    outputBufferBytes = 0;
  };

  const flushOutput = (reason: "frame" | "threshold", frameTimestamp?: number) => {
    if (outputFrame !== 0) {
      cancelAnimationFrame(outputFrame);
      outputFrame = 0;
    }
    if (outputBuffer.length === 0) {
      outputFrameScheduledAt = 0;
      return;
    }

    const chunkCount = outputBuffer.length;
    const coalesced = concatOutputBytes(outputBuffer, outputBufferBytes);
    const frameWaitMs = reason === "frame" && outputFrameScheduledAt > 0
      ? Math.max(0, (frameTimestamp ?? schedulerNow()) - outputFrameScheduledAt)
      : 0;
    outputBuffer = [];
    outputBufferBytes = 0;
    outputFrameScheduledAt = 0;
    terminal.write(coalesced);
    recordSchedulerFlush(reason, chunkCount, frameWaitMs);
  };

  const queueOutput = (chunk: TerminalOutputChunk) => {
    const data = toOutputBytes(chunk);
    if (data.byteLength === 0) return;
    outputBuffer.push(data);
    outputBufferBytes += data.byteLength;
    if (import.meta.env.DEV) {
      schedulerMetrics.receivedChunks += 1;
      schedulerMetrics.receivedBytes += data.byteLength;
    }

    if (outputBufferBytes >= MAX_PENDING_OUTPUT_CHARS) {
      flushOutput("threshold");
    } else if (outputFrame === 0) {
      outputFrameScheduledAt = schedulerNow();
      outputFrame = requestAnimationFrame((timestamp) => flushOutput("frame", timestamp));
    }
  };

  const processLiveChunk = (
    data: Uint8Array,
    sequence?: string | null,
    daemonEpoch?: string | null,
  ) => {
    const seq = parseSequence(sequence);
    if (seq !== null) {
      if (historyEndSequence !== null && seq <= historyEndSequence) {
        return;
      }
      if (lastAppliedSequence !== null && seq <= lastAppliedSequence) {
        return;
      }
      lastAppliedSequence = seq;
      if (daemonEpoch) currentDaemonEpoch = daemonEpoch;
      if (sequence) {
        options?.onSequenceUpdate?.(sequence, currentDaemonEpoch);
      }
    }
    queueOutput(data);
  };

  const applyReplayHistory = (
    history: string,
    startSequence?: string | null,
    endSequence?: string | null,
  ) => {
    if (history) {
      queueOutput(decodeBase64(history));
    }

    const endSeq = parseSequence(endSequence);
    if (endSeq !== null) {
      historyEndSequence = endSeq;
      lastAppliedSequence = endSeq;
      if (endSequence) options?.onSequenceUpdate?.(endSequence, currentDaemonEpoch);
      return;
    }

    const startSeq = parseSequence(startSequence);
    if (startSeq !== null && lastAppliedSequence === null) {
      lastAppliedSequence = startSeq;
    }
  };

  const resetForGap = () => {
    clearPendingOutput();
    queuedLiveChunks.length = 0;
    terminal.reset?.();
    terminalEventBus.resetDecoder(backendSessionId);
    historyEndSequence = null;
    lastAppliedSequence = null;
    options?.onGap?.();
  };

  const handleRuntimeReplayGap = (gap: TerminalRuntimeReplayGap) => {
    if (!active) return;
    streamGeneration += 1;
    attached = true;
    resetForGap();
    if (gap.daemonEpoch) currentDaemonEpoch = gap.daemonEpoch;
    applyReplayHistory(gap.history, gap.startSequence, gap.endSequence);
  };

  const attachCaller = options?.attachFn ?? attachTerminal;

  const performAttach = async () => {
    const generation = streamGeneration;
    try {
      const attachment = await attachCaller({
        sessionId: backendSessionId,
        afterSequence: options?.initialSequence ?? null,
      });
      if (!active || generation !== streamGeneration) return;

      const isEpochMismatch =
        Boolean(attachment.daemonEpoch && currentDaemonEpoch && attachment.daemonEpoch !== currentDaemonEpoch);
      const hasGap = Boolean(attachment.gap);

      if (isEpochMismatch || hasGap) {
        resetForGap();
      }

      if (attachment.daemonEpoch) {
        currentDaemonEpoch = attachment.daemonEpoch;
      }

      applyReplayHistory(
        attachment.history,
        attachment.historyStartSequence,
        attachment.historyEndSequence,
      );

      attached = true;

      const pending = queuedLiveChunks.splice(0, queuedLiveChunks.length);
      for (const chunk of pending) {
        processLiveChunk(chunk.data, chunk.sequence, chunk.daemonEpoch);
      }
    } catch {
      if (!active || generation !== streamGeneration) return;
      attached = true;
      const pending = queuedLiveChunks.splice(0, queuedLiveChunks.length);
      for (const chunk of pending) {
        processLiveChunk(chunk.data, chunk.sequence, chunk.daemonEpoch);
      }
    }
  };

  const unsubscribeOutput = terminalEventBus.subscribeOutput(
    backendSessionId,
    (chunk, sequence, daemonEpoch) => {
      if (!active) return;
      const data = toOutputBytes(chunk);
      const seq = parseSequence(sequence);
      if (seq === null) {
        queueOutput(data);
        return;
      }
      if (!attached) {
        queuedLiveChunks.push({ data, sequence, daemonEpoch });
      } else {
        processLiveChunk(data, sequence, daemonEpoch);
      }
    },
    false,
  );
  const unsubscribeReplayGap = terminalEventBus.subscribeReplayGap(
    backendSessionId,
    handleRuntimeReplayGap,
  );

  void performAttach();

  return () => {
    active = false;
    clearPendingOutput();
    queuedLiveChunks.length = 0;
    unsubscribeOutput();
    unsubscribeReplayGap();
  };
}
