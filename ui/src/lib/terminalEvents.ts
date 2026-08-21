import type { TerminalLifecyclePayload, TerminalOutputPayload } from "./types";
import { onTerminalLifecycle, onTerminalOutput } from "./tauri";
import { TerminalOutputDecoderRegistry } from "./terminalOutput";

const MAX_BACKLOG_CHARS = 512 * 1024;
const MAX_OSC_TITLE_CHARS = 8 * 1024;
const OSC_TITLE_START_RE = /\u001b\](?:0|1|2);/g;
const OSC_PARTIAL_PREFIXES = ["\u001b", "\u001b]", "\u001b]0", "\u001b]1", "\u001b]2"] as const;

type OutputListener = (text: string) => void;
type LifecycleListener = (payload: TerminalLifecyclePayload) => void;
type TitleListener = (sessionId: string, title: string) => void;

export type OscTitleScanResult = {
  titles: string[];
  carry: string;
};

/**
 * Extracts OSC 0/1/2 terminal titles from decoded PTY output while preserving an incomplete
 * sequence for the next chunk. This runs independently of xterm, so background/unmounted tabs
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
  private readonly lifecycleListeners = new Set<LifecycleListener>();
  private readonly titleListeners = new Set<TitleListener>();
  private readonly backlog = new Map<string, string>();
  private readonly titleCarry = new Map<string, string>();
  private readonly lastTitle = new Map<string, string>();
  private startPromise: Promise<void> | null = null;

  ensureStarted(): Promise<void> {
    if (this.startPromise) return this.startPromise;
    this.startPromise = Promise.all([
      onTerminalOutput((payload) => this.handleOutput(payload)),
      onTerminalLifecycle((payload) => this.handleLifecycle(payload)),
    ]).then(() => undefined);
    return this.startPromise;
  }

  subscribeOutput(sessionId: string, listener: OutputListener, replay = true) {
    const listeners = this.outputListeners.get(sessionId) ?? new Set<OutputListener>();
    listeners.add(listener);
    this.outputListeners.set(sessionId, listeners);

    if (replay) {
      const existing = this.backlog.get(sessionId);
      if (existing) listener(existing);
    }

    return () => {
      const current = this.outputListeners.get(sessionId);
      current?.delete(listener);
      if (current?.size === 0) this.outputListeners.delete(sessionId);
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

  clearSession(sessionId: string) {
    this.decoderRegistry.reset(sessionId);
    this.backlog.delete(sessionId);
    this.outputListeners.delete(sessionId);
    this.titleCarry.delete(sessionId);
    this.lastTitle.delete(sessionId);
  }

  private handleOutput(payload: TerminalOutputPayload) {
    const text = this.decoderRegistry.decode(payload.sessionId, payload.data);
    if (text) this.publishOutput(payload.sessionId, text);
  }

  private handleLifecycle(payload: TerminalLifecyclePayload) {
    if (payload.state === "exited" || payload.state === "failed") {
      const tail = this.decoderRegistry.finish(payload.sessionId);
      if (tail) this.publishOutput(payload.sessionId, tail);
    }
    for (const listener of this.lifecycleListeners) listener(payload);
  }

  private publishOutput(sessionId: string, text: string) {
    const scan = scanTerminalOscTitles(text, this.titleCarry.get(sessionId) ?? "");
    if (scan.carry) this.titleCarry.set(sessionId, scan.carry);
    else this.titleCarry.delete(sessionId);

    for (const title of scan.titles) {
      if (this.lastTitle.get(sessionId) === title) continue;
      this.lastTitle.set(sessionId, title);
      for (const listener of this.titleListeners) listener(sessionId, title);
    }

    const nextBacklog = `${this.backlog.get(sessionId) ?? ""}${text}`;
    this.backlog.set(sessionId, nextBacklog.slice(-MAX_BACKLOG_CHARS));
    for (const listener of this.outputListeners.get(sessionId) ?? []) listener(text);
  }
}

export const terminalEventBus = new TerminalEventBus();

export function ensureTerminalEvents() {
  return terminalEventBus.ensureStarted();
}