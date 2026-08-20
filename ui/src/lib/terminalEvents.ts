import type { TerminalLifecyclePayload, TerminalOutputPayload } from "./types";
import { onTerminalLifecycle, onTerminalOutput } from "./tauri";
import { TerminalOutputDecoderRegistry } from "./terminalOutput";

const MAX_BACKLOG_CHARS = 512 * 1024;

type OutputListener = (text: string) => void;
type LifecycleListener = (payload: TerminalLifecyclePayload) => void;

class TerminalEventBus {
  private readonly decoderRegistry = new TerminalOutputDecoderRegistry();
  private readonly outputListeners = new Map<string, Set<OutputListener>>();
  private readonly lifecycleListeners = new Set<LifecycleListener>();
  private readonly backlog = new Map<string, string>();
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

  clearSession(sessionId: string) {
    this.decoderRegistry.reset(sessionId);
    this.backlog.delete(sessionId);
    this.outputListeners.delete(sessionId);
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
    const nextBacklog = `${this.backlog.get(sessionId) ?? ""}${text}`;
    this.backlog.set(sessionId, nextBacklog.slice(-MAX_BACKLOG_CHARS));
    for (const listener of this.outputListeners.get(sessionId) ?? []) listener(text);
  }
}

export const terminalEventBus = new TerminalEventBus();

export function ensureTerminalEvents() {
  return terminalEventBus.ensureStarted();
}
