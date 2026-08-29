import { Channel, invoke, isTauri } from "@tauri-apps/api/core";

import { onTerminalLifecycle, onTerminalOutput } from "./tauri";
import {
  decodeBase64,
  decodeTerminalOutputFrame,
  type DecodedTerminalOutputFrame,
  TerminalOutputDecoderRegistry,
} from "./terminalOutput";
import { metricsNow, terminalThroughputMetricsEnabled } from "./terminalThroughputMetrics";
import type { TerminalLifecyclePayload, TerminalOutputPayload } from "./types";

const MAX_BACKLOG_BYTES = 512 * 1024;
const MAX_OSC_TITLE_CHARS = 8 * 1024;
const OSC_TITLE_START_RE = /\u001b\](?:0|1|2);/g;
const OSC_PARTIAL_PREFIXES = ["\u001b", "\u001b]", "\u001b]0", "\u001b]1", "\u001b]2"] as const;

export type TerminalOutputChunk = Uint8Array | string;
type OutputListener = (
  data: TerminalOutputChunk,
  sequence?: string | null,
  daemonEpoch?: string | null,
  receivedAtMs?: number,
) => void;
type LifecycleListener = (payload: TerminalLifecyclePayload) => void;
type TitleListener = (sessionId: string, title: string) => void;

type TerminalControlPayload = TerminalOutputPayload & {
  kind?: "output" | "replayGap";
  requestedAfterSequence?: string | null;
  availableFromSequence?: string | null;
  startSequence?: string | null;
  endSequence?: string | null;
};

export type TerminalRuntimeReplayGap = {
  requestedAfterSequence: string;
  availableFromSequence: string;
  startSequence?: string | null;
  endSequence?: string | null;
  history: string;
  daemonEpoch?: string | null;
};

type ReplayGapListener = (gap: TerminalRuntimeReplayGap) => void;

type SessionBacklog = {
  chunks: Uint8Array[];
  totalBytes: number;
};

export type OscTitleScanResult = {
  titles: string[];
  carry: string;
};

/**
 * Extracts OSC 0/1/2 terminal titles from decoded PTY output while preserving an incomplete
 * sequence for the next chunk. This runs independently of the active terminal view, so background/unmounted tabs
 * still produce activity title updates.
 */
export function scanTerminalOscTitles(text: string, carry = ""): OscTitleScanResult {
  const source = `${carry}${text}`;
  const titles: string[] = [];
  let searchFrom = 0;

  OSC_TITLE_START_RE.lastIndex = 0;
  while (true) {
    OSC_TITLE_START_RE.lastIndex = searchFrom;
    const match = OSC_TITLE_START_RE.exec(source);
    if (!match) break;

    const valueStart = match.index + match[0].length;
    const terminator = findOscTerminator(source, valueStart);
    if (!terminator) {
      return {
        titles,
        carry: source.slice(Math.max(match.index, source.length - MAX_OSC_TITLE_CHARS)),
      };
    }

    titles.push(source.slice(valueStart, terminator.index));
    searchFrom = terminator.index + terminator.length;
  }

  return { titles, carry: findPartialOscPrefix(source.slice(searchFrom)) };
}

function findOscTerminator(source: string, from: number): { index: number; length: number } | null {
  const candidates = [
    { index: source.indexOf("\u0007", from), length: 1 },
    { index: source.indexOf("\u001b\\", from), length: 2 },
    { index: source.indexOf("\u009c", from), length: 1 },
  ].filter((candidate) => candidate.index >= 0);
  if (candidates.length === 0) return null;
  return candidates.reduce((earliest, candidate) => candidate.index < earliest.index ? candidate : earliest);
}

function findPartialOscPrefix(source: string): string {
  for (const prefix of [...OSC_PARTIAL_PREFIXES].sort((a, b) => b.length - a.length)) {
    if (source.endsWith(prefix)) return prefix;
  }
  return "";
}

class TerminalEventBus {
  private readonly decoderRegistry = new TerminalOutputDecoderRegistry();
  private readonly outputListeners = new Map<string, Set<OutputListener>>();
  private readonly replayGapListeners = new Map<string, Set<ReplayGapListener>>();
  private readonly lifecycleListeners = new Set<LifecycleListener>();
  private readonly titleListeners = new Set<TitleListener>();
  private readonly backlog = new Map<string, SessionBacklog>();
  private readonly titleCarry = new Map<string, string>();
  private readonly lastTitle = new Map<string, string>();
  private binaryOutputChannel: Channel<ArrayBuffer> | null = null;
  private startPromise: Promise<void> | null = null;

  ensureStarted(): Promise<void> {
    if (this.startPromise) return this.startPromise;
    this.startPromise = Promise.all([
      onTerminalOutput((payload) => this.handleControlOutput(payload)),
      onTerminalLifecycle((payload) => this.handleLifecycle(payload)),
      this.ensureBinaryOutputChannel(),
    ]).then(() => undefined);
    return this.startPromise;
  }

  subscribeOutput(sessionId: string, listener: OutputListener, replay = true) {
    const listeners = this.outputListeners.get(sessionId) ?? new Set<OutputListener>();
    listeners.add(listener);
    this.outputListeners.set(sessionId, listeners);

    if (replay) {
      const existing = this.backlog.get(sessionId);
      if (existing && existing.totalBytes > 0) {
        for (const chunk of existing.chunks) {
          listener(chunk);
        }
      }
    }

    return () => {
      const current = this.outputListeners.get(sessionId);
      current?.delete(listener);
      if (current?.size === 0) this.outputListeners.delete(sessionId);
    };
  }

  subscribeReplayGap(sessionId: string, listener: ReplayGapListener) {
    const listeners = this.replayGapListeners.get(sessionId) ?? new Set<ReplayGapListener>();
    listeners.add(listener);
    this.replayGapListeners.set(sessionId, listeners);
    return () => {
      const current = this.replayGapListeners.get(sessionId);
      current?.delete(listener);
      if (current?.size === 0) this.replayGapListeners.delete(sessionId);
    };
  }

  subscribeLifecycle(listener: LifecycleListener) {
    this.lifecycleListeners.add(listener);
    return () => {
      this.lifecycleListeners.delete(listener);
    };
  }

  subscribeTitle(listener: TitleListener) {
    this.titleListeners.add(listener);
    return () => {
      this.titleListeners.delete(listener);
    };
  }

  resetDecoder(sessionId: string) {
    this.decoderRegistry.reset(sessionId);
  }

  clearSession(sessionId: string) {
    this.decoderRegistry.reset(sessionId);
    this.backlog.delete(sessionId);
    this.outputListeners.delete(sessionId);
    this.replayGapListeners.delete(sessionId);
    this.titleCarry.delete(sessionId);
    this.lastTitle.delete(sessionId);
  }

  getBacklogMetricsForTest(sessionId?: string): { sessions: number; chunks: number; chars: number } {
    if (sessionId) {
      const entry = this.backlog.get(sessionId);
      if (!entry) return { sessions: 0, chunks: 0, chars: 0 };
      return {
        sessions: 1,
        chunks: entry.chunks.length,
        chars: entry.totalBytes,
      };
    }
    let totalChunks = 0;
    let totalBytes = 0;
    for (const entry of this.backlog.values()) {
      totalChunks += entry.chunks.length;
      totalBytes += entry.totalBytes;
    }
    return {
      sessions: this.backlog.size,
      chunks: totalChunks,
      chars: totalBytes,
    };
  }

  private async ensureBinaryOutputChannel(): Promise<void> {
    if (!isTauri() || this.binaryOutputChannel) return;

    const channel = new Channel<ArrayBuffer>((frame) => {
      const receivedAtMs = terminalThroughputMetricsEnabled() ? metricsNow() : undefined;
      try {
        this.handleBinaryOutput(decodeTerminalOutputFrame(frame), receivedAtMs);
      } catch (error) {
        console.error("Failed to decode terminal output channel frame", error);
      }
    });

    try {
      await invoke<void>("cmd_terminal_output_channel", { channel });
      this.binaryOutputChannel = channel;
    } catch (error) {
      // The Rust side keeps the legacy terminal_output event as a no-loss fallback.
      console.warn("Failed to register terminal output channel; using event fallback", error);
    }
  }

  private handleControlOutput(payload: TerminalControlPayload) {
    if (payload.kind === "replayGap") {
      this.decoderRegistry.reset(payload.sessionId);
      this.backlog.delete(payload.sessionId);
      this.titleCarry.delete(payload.sessionId);
      this.lastTitle.delete(payload.sessionId);
      const gap: TerminalRuntimeReplayGap = {
        requestedAfterSequence: payload.requestedAfterSequence ?? "0",
        availableFromSequence: payload.availableFromSequence ?? "0",
        startSequence: payload.startSequence ?? null,
        endSequence: payload.endSequence ?? null,
        history: payload.data,
        daemonEpoch: payload.daemonEpoch ?? null,
      };
      for (const listener of this.replayGapListeners.get(payload.sessionId) ?? []) {
        listener(gap);
      }
      return;
    }

    // Compatibility/failure fallback from Rust. Normal stdout arrives on the binary channel.
    this.handleBinaryOutput({
      sessionId: payload.sessionId,
      data: decodeBase64(payload.data),
      sequence: payload.sequence,
      daemonEpoch: payload.daemonEpoch,
    });
  }

  private handleBinaryOutput(payload: DecodedTerminalOutputFrame, receivedAtMs?: number) {
    const decodedText = this.decoderRegistry.decode(payload.sessionId, payload.data);
    if (decodedText) this.trackTitles(payload.sessionId, decodedText);
    if (payload.data.byteLength > 0) {
      this.publishOutput(
        payload.sessionId,
        payload.data,
        payload.sequence,
        payload.daemonEpoch,
        receivedAtMs,
      );
    }
  }

  private handleLifecycle(payload: TerminalLifecyclePayload) {
    if (payload.state === "exited" || payload.state === "failed") {
      const tail = this.decoderRegistry.finish(payload.sessionId);
      if (tail) this.trackTitles(payload.sessionId, tail);
    }
    for (const listener of this.lifecycleListeners) listener(payload);
  }

  private trackTitles(sessionId: string, text: string) {
    const scan = scanTerminalOscTitles(text, this.titleCarry.get(sessionId) ?? "");
    if (scan.carry) this.titleCarry.set(sessionId, scan.carry);
    else this.titleCarry.delete(sessionId);

    for (const title of scan.titles) {
      if (this.lastTitle.get(sessionId) === title) continue;
      this.lastTitle.set(sessionId, title);
      for (const listener of this.titleListeners) listener(sessionId, title);
    }
  }

  private publishOutput(
    sessionId: string,
    data: Uint8Array,
    sequence?: string | null,
    daemonEpoch?: string | null,
    receivedAtMs?: number,
  ) {
    if (data.byteLength > 0) {
      let entry = this.backlog.get(sessionId);
      if (!entry) {
        entry = { chunks: [], totalBytes: 0 };
        this.backlog.set(sessionId, entry);
      }

      entry.chunks.push(data);
      entry.totalBytes += data.byteLength;

      while (entry.totalBytes > MAX_BACKLOG_BYTES && entry.chunks.length > 0) {
        const oldest = entry.chunks[0];
        if (oldest === undefined) break;
        const overflow = entry.totalBytes - MAX_BACKLOG_BYTES;
        if (oldest.byteLength <= overflow) {
          entry.chunks.shift();
          entry.totalBytes -= oldest.byteLength;
        } else {
          entry.chunks[0] = oldest.subarray(overflow);
          entry.totalBytes -= overflow;
          break;
        }
      }
    }

    for (const listener of this.outputListeners.get(sessionId) ?? []) {
      listener(data, sequence, daemonEpoch, receivedAtMs);
    }
  }
}

export const terminalEventBus = new TerminalEventBus();

export function getBacklogMetricsForTest(sessionId?: string) {
  return terminalEventBus.getBacklogMetricsForTest(sessionId);
}

export function ensureTerminalEvents() {
  return terminalEventBus.ensureStarted();
}
