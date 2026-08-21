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
        void playNotificationSound({
          soundId: settings.customSoundId ?? 'system',
          customSoundPath: settings.customSoundPath,
          volume: settings.customSoundVolume,
        });

        const dispatchArgs: DispatchNotificationArgs = {
          source: 'terminal-bell',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
        };
        void dispatchNotification(dispatchArgs);
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
        void playNotificationSound({
          soundId: settings.customSoundId ?? 'system',
          customSoundPath: settings.customSoundPath,
          volume: settings.customSoundVolume,
        });

        const dispatchArgs: DispatchNotificationArgs = {
          source: 'agent-task-complete',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
          agentLabel: params.agentLabel,
        };
        void dispatchNotification(dispatchArgs);
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
