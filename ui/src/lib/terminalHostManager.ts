import {
  createTerminalInstance,
  disposeTerminalInstance,
  fitTerminal,
  type TerminalInstance,
} from "./terminalInstanceFactory";
import { attachScheduledOutputSubscription, MAX_PENDING_OUTPUT_CHARS } from "./terminalOutputScheduler";
import {
  applyTerminalSettings,
  type EffectiveTerminalSettings,
} from "./terminalSettings";
import type { TerminalSession } from "./types";

export type { TerminalInstance };
export { MAX_PENDING_OUTPUT_CHARS };

export const DEFAULT_WARM_CACHE_LIMIT = 2;

type SpawnGeneration = {
  generation: number;
  activeCount: number;
};

class TerminalHostManager {
  private instances = new Map<string, TerminalInstance>();
  private pendingSpawns = new Map<string, Promise<TerminalInstance>>();
  private spawnGenerations = new Map<string, SpawnGeneration>();
  private visibleRefCounts = new Map<string, number>();
  private inactiveLruSessionIds: string[] = [];
  private warmCacheLimit = DEFAULT_WARM_CACHE_LIMIT;

  setWarmCacheLimit(limit: number): void {
    this.warmCacheLimit = Math.max(0, limit);
    this.evictInactiveIfNeeded();
  }

  getWarmCacheLimit(): number {
    return this.warmCacheLimit;
  }

  isSessionVisible(sessionId: string): boolean {
    return (this.visibleRefCounts.get(sessionId) ?? 0) > 0;
  }

  registerVisible(sessionId: string): () => void {
    const current = this.visibleRefCounts.get(sessionId) ?? 0;
    this.visibleRefCounts.set(sessionId, current + 1);
    if (current === 0) {
      this.markSessionVisible(sessionId);
    }
    let unregistered = false;
    return () => {
      if (unregistered) return;
      unregistered = true;
      const count = (this.visibleRefCounts.get(sessionId) ?? 1) - 1;
      if (count <= 0) {
        this.visibleRefCounts.delete(sessionId);
        this.markSessionInactive(sessionId);
      } else {
        this.visibleRefCounts.set(sessionId, count);
      }
    };
  }

  setVisible(sessionId: string, visible: boolean): void {
    if (visible) {
      const current = this.visibleRefCounts.get(sessionId) ?? 0;
      this.visibleRefCounts.set(sessionId, Math.max(1, current));
      this.markSessionVisible(sessionId);
    } else {
      this.visibleRefCounts.delete(sessionId);
      this.markSessionInactive(sessionId);
    }
  }

  private markSessionVisible(sessionId: string): void {
    this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== sessionId);
    const inst = this.instances.get(sessionId);
    if (inst) {
      if (!inst.unsubscribeOutput) {
        if (inst.session.backendSessionId) {
          inst.unsubscribeOutput = attachScheduledOutputSubscription(inst.session.backendSessionId, inst.terminal, {
            initialSequence: inst.session.lastOutputSequence,
            daemonEpoch: inst.session.daemonEpoch,
            onSequenceUpdate: (seq, epoch) => {
              inst.session.lastOutputSequence = seq;
              if (epoch) inst.session.daemonEpoch = epoch;
            },
            onGap: () => {
              inst.session.lastOutputSequence = null;
            },
          });
        }
      }
    }
  }

  private markSessionInactive(sessionId: string): void {
    const inst = this.instances.get(sessionId);
    if (inst) {
      inst.unsubscribeOutput?.();
      inst.unsubscribeOutput = undefined;
    }
    this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== sessionId);
    this.inactiveLruSessionIds.push(sessionId);
    this.evictInactiveIfNeeded();
  }

  private evictInactiveIfNeeded(): void {
    while (this.inactiveLruSessionIds.length > this.warmCacheLimit) {
      const oldestSessionId = this.inactiveLruSessionIds.shift();
      if (!oldestSessionId) break;
      this.disposeFrontendInstance(oldestSessionId);
    }
  }

  private disposeFrontendInstance(sessionId: string): void {
    const inst = this.instances.get(sessionId);
    if (inst) {
      this.instances.delete(sessionId);
      disposeTerminalInstance(inst);
    }
  }

  getInstance(sessionId: string): TerminalInstance | undefined {
    return this.instances.get(sessionId);
  }

  async getOrCreate(
    session: TerminalSession,
    active: boolean,
    onBell?: () => void,
    onTitleChange?: (title: string) => void,
  ): Promise<TerminalInstance> {
    const isVisible = this.isSessionVisible(session.id);
    const existing = this.instances.get(session.id);
    if (existing) {
      existing.session = session;
      existing.active = active;
      existing.onBell = onBell;
      existing.onTitleChange = onTitleChange;

      if (isVisible) {
        this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== session.id);
        if (!existing.unsubscribeOutput) {
          if (session.backendSessionId) {
            existing.unsubscribeOutput = attachScheduledOutputSubscription(session.backendSessionId, existing.terminal, {
              initialSequence: session.lastOutputSequence,
              daemonEpoch: session.daemonEpoch,
              onSequenceUpdate: (seq, epoch) => {
                session.lastOutputSequence = seq;
                existing.session.lastOutputSequence = seq;
                if (epoch) {
                  session.daemonEpoch = epoch;
                  existing.session.daemonEpoch = epoch;
                }
              },
              onGap: () => {
                session.lastOutputSequence = null;
                existing.session.lastOutputSequence = null;
              },
            });
          }
        }
      } else {
        if (existing.unsubscribeOutput) {
          existing.unsubscribeOutput();
          existing.unsubscribeOutput = undefined;
        }
        this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== session.id);
        this.inactiveLruSessionIds.push(session.id);
        this.evictInactiveIfNeeded();
      }
      return existing;
    }

    const pending = this.pendingSpawns.get(session.id);
    if (pending) return pending;

    const currentGen = this.spawnGenerations.get(session.id);
    const generation = (currentGen ? currentGen.generation : 0) + 1;
    const activeCount = (currentGen ? currentGen.activeCount : 0) + 1;
    this.spawnGenerations.set(session.id, { generation, activeCount });

    const spawnPromise = createTerminalInstance({
      session,
      active,
      isVisible: this.isSessionVisible(session.id),
      onBell,
      onTitleChange,
      getInstance: (id) => this.instances.get(id),
    });
    this.pendingSpawns.set(session.id, spawnPromise);

    try {
      const instance = await spawnPromise;
      if (this.spawnGenerations.get(session.id)?.generation !== generation) {
        disposeTerminalInstance(instance);
        return instance;
      }

      this.instances.set(session.id, instance);

      const stillVisible = this.isSessionVisible(session.id);
      if (stillVisible) {
        this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== session.id);
        if (!instance.unsubscribeOutput && session.backendSessionId) {
          instance.unsubscribeOutput = attachScheduledOutputSubscription(session.backendSessionId, instance.terminal, {
            initialSequence: session.lastOutputSequence,
            daemonEpoch: session.daemonEpoch,
            onSequenceUpdate: (seq, epoch) => {
              session.lastOutputSequence = seq;
              instance.session.lastOutputSequence = seq;
              if (epoch) {
                session.daemonEpoch = epoch;
                instance.session.daemonEpoch = epoch;
              }
            },
            onGap: () => {
              session.lastOutputSequence = null;
              instance.session.lastOutputSequence = null;
            },
          });
        }
      } else {
        if (instance.unsubscribeOutput) {
          instance.unsubscribeOutput();
          instance.unsubscribeOutput = undefined;
        }
        this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== session.id);
        this.inactiveLruSessionIds.push(session.id);
        this.evictInactiveIfNeeded();
      }
      return instance;
    } finally {
      const tracking = this.spawnGenerations.get(session.id);
      if (tracking) {
        tracking.activeCount -= 1;
        if (tracking.activeCount <= 0) {
          this.spawnGenerations.delete(session.id);
        }
      }
      if (this.pendingSpawns.get(session.id) === spawnPromise) {
        this.pendingSpawns.delete(session.id);
      }
    }
  }

  updateSession(sessionId: string, session: TerminalSession) {
    const inst = this.instances.get(sessionId);
    if (inst) {
      const prevBackendId = inst.session.backendSessionId;
      inst.session = session;
      const isVisible = this.isSessionVisible(sessionId);
      if (isVisible) {
        if (session.backendSessionId !== prevBackendId) {
          inst.unsubscribeOutput?.();
          inst.unsubscribeOutput = undefined;
          if (session.backendSessionId) {
            inst.unsubscribeOutput = attachScheduledOutputSubscription(session.backendSessionId, inst.terminal, {
              initialSequence: session.lastOutputSequence,
              daemonEpoch: session.daemonEpoch,
              onSequenceUpdate: (seq, epoch) => {
                session.lastOutputSequence = seq;
                inst.session.lastOutputSequence = seq;
                if (epoch) {
                  session.daemonEpoch = epoch;
                  inst.session.daemonEpoch = epoch;
                }
              },
              onGap: () => {
                session.lastOutputSequence = null;
                inst.session.lastOutputSequence = null;
              },
            });
          }
        }
      } else {
        if (inst.unsubscribeOutput) {
          inst.unsubscribeOutput();
          inst.unsubscribeOutput = undefined;
        }
      }
    }
  }

  applyInstanceSettings(sessionId: string, settings: EffectiveTerminalSettings) {
    const inst = this.instances.get(sessionId);
    if (inst) {
      applyTerminalSettings(inst.terminal, settings);
      if (inst.element.clientWidth > 0 && inst.element.clientHeight > 0) {
        fitTerminal(inst.terminal, inst.fitAddon);
      }
    }
  }

  applySettings(settings: EffectiveTerminalSettings) {
    for (const inst of this.instances.values()) {
      applyTerminalSettings(inst.terminal, settings);
      if (inst.element.clientWidth > 0 && inst.element.clientHeight > 0) {
        fitTerminal(inst.terminal, inst.fitAddon);
      }
    }
  }

  destroy(sessionId: string) {
    const tracking = this.spawnGenerations.get(sessionId);
    if (tracking) {
      tracking.generation += 1;
    }
    this.visibleRefCounts.delete(sessionId);
    this.inactiveLruSessionIds = this.inactiveLruSessionIds.filter((id) => id !== sessionId);
    this.pendingSpawns.delete(sessionId);
    this.disposeFrontendInstance(sessionId);
  }
}

export const terminalHostManager = new TerminalHostManager();
