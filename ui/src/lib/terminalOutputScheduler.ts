import type { Terminal } from "@xterm/xterm";

import { attachTerminal } from "./tauri";
import { terminalEventBus } from "./terminalEvents";
import { decodeBase64 } from "./terminalOutput";
import type { AttachTerminalResponse } from "./types";

export const MAX_PENDING_OUTPUT_CHARS = 128 * 1024;

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

function parseSequence(seq?: string | null): bigint | null {
  if (seq === undefined || seq === null || seq === "") return null;
  try {
    return BigInt(seq);
  } catch {
    return null;
  }
}

/**
 * Batches incoming PTY output chunks per terminal renderer at animation-frame cadence
 * with protocol-v2 typed attachment, history-before-live ordering, and sequence de-duplication.
 *
 * Queue policy:
 * - On initial subscription, calls `attachTerminal` with `initialSequence`.
 * - Decodes base64 history and renders it before live output.
 * - Suppresses live output chunks where sequence <= historyEndSequence or sequence <= lastAppliedSequence.
 * - If an explicit gap or daemon epoch mismatch is detected, triggers terminal reset and replays full history.
 * - Chunks received between rAF flushes are held in an ordered per-renderer buffer.
 * - On frame flush, all buffered chunks coalesce into a single `terminal.write` call in original arrival order.
 * - If queued characters reach or exceed `MAX_PENDING_OUTPUT_CHARS` before rAF, the buffer is flushed synchronously
 *   to guarantee memory safety without loss or reordering.
 * - Teardown (via unlisten or manager destroy) immediately cancels pending rAF callbacks and clears buffered data.
 */
export function attachScheduledOutputSubscription(
  backendSessionId: string,
  terminal: TerminalOutputWriter,
  options?: ScheduledOutputSubscriptionOptions,
): () => void {
  let outputBuffer: string[] = [];
  let outputBufferChars = 0;
  let outputFrame = 0;
  let active = true;
  let attached = false;
  let lastAppliedSequence: bigint | null = parseSequence(options?.initialSequence);
  let currentDaemonEpoch: string | null = options?.daemonEpoch ?? null;
  let historyEndSequence: bigint | null = null;
  const queuedLiveChunks: Array<{ text: string; sequence?: string | null; daemonEpoch?: string | null }> = [];

  const flushOutput = () => {
    if (outputFrame !== 0) {
      cancelAnimationFrame(outputFrame);
      outputFrame = 0;
    }
    if (outputBuffer.length === 0) return;
    const coalesced = outputBuffer.length === 1 ? (outputBuffer[0] ?? "") : outputBuffer.join("");
    outputBuffer = [];
    outputBufferChars = 0;
    terminal.write(coalesced);
  };

  const queueText = (text: string) => {
    if (!text) return;
    outputBuffer.push(text);
    outputBufferChars += text.length;

    if (outputBufferChars >= MAX_PENDING_OUTPUT_CHARS) {
      flushOutput();
    } else if (outputFrame === 0) {
      outputFrame = requestAnimationFrame(flushOutput);
    }
  };

  const processLiveChunk = (text: string, sequence?: string | null, daemonEpoch?: string | null) => {
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
    queueText(text);
  };

  const attachCaller = options?.attachFn ?? attachTerminal;

  const performAttach = async () => {
    try {
      const attachment = await attachCaller({
        sessionId: backendSessionId,
        afterSequence: options?.initialSequence ?? null,
      });
      if (!active) return;

      const isEpochMismatch =
        Boolean(attachment.daemonEpoch && currentDaemonEpoch && attachment.daemonEpoch !== currentDaemonEpoch);
      const hasGap = Boolean(attachment.gap);

      if (isEpochMismatch || hasGap) {
        terminal.reset?.();
        terminalEventBus.resetDecoder(backendSessionId);
        lastAppliedSequence = null;
        options?.onGap?.();
      }

      if (attachment.daemonEpoch) {
        currentDaemonEpoch = attachment.daemonEpoch;
      }

      if (attachment.history) {
        const decodedBytes = decodeBase64(attachment.history);
        const decodedHistory = new TextDecoder("utf-8", { fatal: false }).decode(decodedBytes);
        if (decodedHistory) {
          queueText(decodedHistory);
        }
      }

      const endSeq = parseSequence(attachment.historyEndSequence);
      if (endSeq !== null) {
        historyEndSequence = endSeq;
        lastAppliedSequence = endSeq;
        if (attachment.historyEndSequence) {
          options?.onSequenceUpdate?.(attachment.historyEndSequence, currentDaemonEpoch);
        }
      } else if (attachment.historyStartSequence && lastAppliedSequence === null) {
        const startSeq = parseSequence(attachment.historyStartSequence);
        if (startSeq !== null) {
          lastAppliedSequence = startSeq;
        }
      }

      attached = true;

      const pending = queuedLiveChunks.splice(0, queuedLiveChunks.length);
      for (const chunk of pending) {
        processLiveChunk(chunk.text, chunk.sequence, chunk.daemonEpoch);
      }
    } catch {
      if (!active) return;
      attached = true;
      const pending = queuedLiveChunks.splice(0, queuedLiveChunks.length);
      for (const chunk of pending) {
        processLiveChunk(chunk.text, chunk.sequence, chunk.daemonEpoch);
      }
    }
  };

  const unsubscribe = terminalEventBus.subscribeOutput(
    backendSessionId,
    (text, sequence, daemonEpoch) => {
      if (!active) return;
      const seq = parseSequence(sequence);
      if (seq === null) {
        queueText(text);
        return;
      }
      if (!attached) {
        queuedLiveChunks.push({ text, sequence, daemonEpoch });
      } else {
        processLiveChunk(text, sequence, daemonEpoch);
      }
    },
    false,
  );

  void performAttach();

  return () => {
    active = false;
    if (outputFrame !== 0) {
      cancelAnimationFrame(outputFrame);
      outputFrame = 0;
    }
    outputBuffer = [];
    outputBufferChars = 0;
    queuedLiveChunks.length = 0;
    unsubscribe();
  };
}
