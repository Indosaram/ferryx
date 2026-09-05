import { dispatchNotification, playNotificationSound } from './tauri';
import { type NotificationSettings, loadNotificationSettings } from './notificationSettings';
import type { DispatchNotificationArgs } from './types';

export function isWindowForegroundFocused(): boolean {
  return (
    typeof document !== 'undefined' &&
    document.visibilityState === 'visible' &&
    typeof document.hasFocus === 'function' &&
    document.hasFocus()
  );
}

export interface NotificationCoordinatorOptions {
  isWindowFocused?: () => boolean;
  getSettings?: () => NotificationSettings;
  onMarkTabUnread?: (tabId: string) => void;
  onMarkWorktreeUnread?: (path: string) => void;
  onError?: (error: unknown, source: 'sound' | 'dispatch') => void;
}

export interface TerminalBellEventParams {
  sessionId?: string;
  tabId?: string;
  worktreeId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
  terminalTitle?: string;
  settings?: NotificationSettings;
}

export interface AgentStateChangeEventParams {
  sessionId?: string;
  tabId?: string;
  worktreeId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
  agentLabel?: string;
  terminalTitle?: string;
  previousState?: string;
  nextState?: string;
  newState?: string;
  settings?: NotificationSettings;
}

export class NotificationCoordinator {
  private lastBellTimestamp = new Map<string, number>();
  private lastAgentState = new Map<string, string>();
  private lastAgentCompletionTimestamp = new Map<string, number>();

  private readonly bellThrottleMs = 1000;
  private readonly bellAgentSuppressionMs = 1500;

  constructor(private options: NotificationCoordinatorOptions = {}) {}

  private isFocused(): boolean {
    return this.options.isWindowFocused ? this.options.isWindowFocused() : isWindowForegroundFocused();
  }

  private resolveSettings(override?: NotificationSettings): NotificationSettings {
    if (override) return override;
    if (this.options.getSettings) return this.options.getSettings();
    return loadNotificationSettings();
  }

  handleTerminalBell(params: TerminalBellEventParams): void {
    const key = params.sessionId || params.tabId || 'default';
    const settings = this.resolveSettings(params.settings);

    const now = Date.now();
    const lastBell = this.lastBellTimestamp.get(key) ?? 0;
    if (now - lastBell < this.bellThrottleMs) {
      return;
    }
    this.lastBellTimestamp.set(key, now);

    const lastCompletion = this.lastAgentCompletionTimestamp.get(key) ?? 0;
    if (now - lastCompletion < this.bellAgentSuppressionMs) {
      return;
    }

    const isFocused = this.isFocused();

    if (!isFocused) {
      const tabId = params.tabId;
      const wtPath = params.worktreePath || params.worktreeId;

      if (tabId && this.options.onMarkTabUnread) {
        this.options.onMarkTabUnread(tabId);
      }
      if (wtPath && this.options.onMarkWorktreeUnread) {
        this.options.onMarkWorktreeUnread(wtPath);
      }

      if (settings.enabled && settings.terminalBell) {
        Promise.resolve(
          playNotificationSound({
            soundId: settings.customSoundId ?? 'system',
            customSoundPath: settings.customSoundPath,
            volume: settings.customSoundVolume,
          })
        )
          .then((result) => {
            // A custom sound that did not play is a real failure; the default
            // "system" sound intentionally reports played:false (it rides the OS banner).
            if (result && settings.customSoundPath && !result.played) {
              this.options.onError?.(
                new Error(`notification sound not played (${result.reason ?? 'unknown'})`),
                'sound',
              );
            }
          })
          .catch((err) => {
            this.options.onError?.(err, 'sound');
          });

        const dispatchArgs: DispatchNotificationArgs = {
          source: 'terminal-bell',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
        };
        Promise.resolve(dispatchNotification(dispatchArgs))
          .then((result) => {
            // The backend rejects with an Ok result (e.g. permission-required),
            // not an IPC error — surface the reason or it disappears silently.
            if (result && !result.submitted) {
              this.options.onError?.(
                new Error(`desktop notification not submitted (${result.reason ?? 'unknown'})`),
                'dispatch',
              );
            }
          })
          .catch((err) => {
            this.options.onError?.(err, 'dispatch');
          });
      }
    }
  }

  handleAgentStateChange(params: AgentStateChangeEventParams): void {
    const key = params.sessionId || params.tabId || 'default';
    const next = params.nextState || params.newState || '';
    const settings = this.resolveSettings(params.settings);

    const effectivePrev = params.previousState ?? this.lastAgentState.get(key);
    this.lastAgentState.set(key, next);

    const isCompletionEdge =
      (next === 'waiting' || next === 'done') &&
      effectivePrev !== undefined &&
      effectivePrev !== 'waiting' &&
      effectivePrev !== 'done';

    if (!isCompletionEdge) {
      return;
    }

    const now = Date.now();
    this.lastAgentCompletionTimestamp.set(key, now);

    const isFocused = this.isFocused();

    if (!isFocused) {
      const tabId = params.tabId;
      const wtPath = params.worktreePath || params.worktreeId;

      if (tabId && this.options.onMarkTabUnread) {
        this.options.onMarkTabUnread(tabId);
      }
      if (wtPath && this.options.onMarkWorktreeUnread) {
        this.options.onMarkWorktreeUnread(wtPath);
      }

      if (settings.enabled && settings.agentTaskComplete) {
        Promise.resolve(
          playNotificationSound({
            soundId: settings.customSoundId ?? 'system',
            customSoundPath: settings.customSoundPath,
            volume: settings.customSoundVolume,
          })
        )
          .then((result) => {
            if (result && settings.customSoundPath && !result.played) {
              this.options.onError?.(
                new Error(`notification sound not played (${result.reason ?? 'unknown'})`),
                'sound',
              );
            }
          })
          .catch((err) => {
            this.options.onError?.(err, 'sound');
          });

        const dispatchArgs: DispatchNotificationArgs = {
          source: 'agent-task-complete',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
          agentLabel: params.agentLabel,
        };
        Promise.resolve(dispatchNotification(dispatchArgs))
          .then((result) => {
            if (result && !result.submitted) {
              this.options.onError?.(
                new Error(`desktop notification not submitted (${result.reason ?? 'unknown'})`),
                'dispatch',
              );
            }
          })
          .catch((err) => {
            this.options.onError?.(err, 'dispatch');
          });
      }
    }
  }

  reset(): void {
    this.lastBellTimestamp.clear();
    this.lastAgentState.clear();
    this.lastAgentCompletionTimestamp.clear();
  }
}

export const defaultNotificationCoordinator = new NotificationCoordinator();
