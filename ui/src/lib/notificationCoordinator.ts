import {
  dispatchNotification,
  getNotificationPermissionStatus,
  playNotificationSound,
  requestNotificationPermission,
  withdrawNotification,
} from './tauri';
import {
  type NotificationSettings,
  NOTIFICATION_SETTINGS_EVENT,
  loadNotificationSettings,
} from './notificationSettings';
import type {
  DispatchNotificationArgs,
  DispatchNotificationResult,
  NotificationPermissionStatus,
  NotificationTarget,
} from './types';
import { shouldThisWindowNotifyFor } from './notificationLeader';

export function isWindowForegroundFocused(): boolean {
  return (
    typeof document !== 'undefined' &&
    document.visibilityState === 'visible' &&
    typeof document.hasFocus === 'function' &&
    document.hasFocus()
  );
}

export interface NotificationDecision {
  accepted: boolean;
  suppressed?: boolean;
}

export interface NotificationCoordinatorOptions {
  isWindowFocused?: () => boolean;
  /**
   * Multi-window ownership gate for OS notifications and sounds (see notificationLeader.ts): the
   * window whose store holds the session notifies; the primary window also notifies for a session
   * no window claims. Unread marks are window-local and are not gated.
   */
  isNotificationLeader?: (event: SessionOwnershipParams) => boolean;
  getSettings?: () => NotificationSettings;
  onMarkTabUnread?: (tabId: string, workspaceId?: string) => void;
  onMarkWorktreeUnread?: (path: string, workspaceId?: string) => void;
  onError?: (error: unknown, source: 'sound' | 'dispatch') => void;
  getPermissionStatus?: () => Promise<NotificationPermissionStatus>;
  requestPermission?: () => Promise<{ granted: boolean; error?: string | null }>;
  /** Recalls a delivered banner; defaults to the native `cmd_notification_withdraw` binding. */
  withdrawNotification?: (target: NotificationTarget) => Promise<void>;
  onPermissionUnavailable?: () => void;
}

/** Multi-window ownership of the event's session, resolved by the caller against its store. */
export interface SessionOwnershipParams {
  /** Backend (daemon) session id from the native event payload. */
  backendSessionId?: string;
  /** Whether this window's workspace store holds a session with that backendSessionId. */
  ownsSession?: boolean;
}

export interface TerminalBellEventParams extends SessionOwnershipParams {
  workspaceId?: string;
  workspaceLabel?: string;
  sessionId?: string;
  tabId?: string;
  worktreeId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
  terminalTitle?: string;
  settings?: NotificationSettings;
}

export interface AgentStateChangeEventParams extends SessionOwnershipParams {
  workspaceId?: string;
  workspaceLabel?: string;
  notificationSuppressed?: boolean;
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
  /** The daemon's attention group. When present it alone decides the edge. */
  group?: 'needsYou' | 'unread' | 'working' | 'seen';
  previousGroup?: 'needsYou' | 'unread' | 'working' | 'seen';
  settings?: NotificationSettings;
}

export class NotificationCoordinator {
  private lastBellTimestamp = new Map<string, number>();
  private lastAgentState = new Map<string, string>();
  private lastAgentCompletionTimestamp = new Map<string, number>();
  private lastGroup = new Map<string, string>();
  private lastTargetByKey = new Map<string, NotificationTarget>();
  /**
   * This coordinator's own dispatch accounting per pane. `sent` counts the dispatches it queued
   * and `cancelledUpTo` the last one a withdrawal has made stale, so a dispatch still waiting on
   * a permission prompt can tell it was overtaken while it waited.
   */
  private localSeqByTarget = new Map<string, { sent: number; cancelledUpTo: number }>();
  /** One chain per pane: a withdrawal must never reach the OS before the dispatch it recalls. */
  private chainByTarget = new Map<string, Promise<void>>();
  private permissionRequest: Promise<boolean> | null = null;
  private permissionStatusQuery: Promise<boolean> | null = null;
  private permissionDenied = false;
  private cleanupListeners: (() => void) | null = null;

  private readonly bellThrottleMs = 1000;
  private readonly bellAgentSuppressionMs = 1500;

  constructor(private options: NotificationCoordinatorOptions = {}) {
    this.installPermissionRecheckListeners();
  }

  private installPermissionRecheckListeners(): void {
    if (typeof window === 'undefined') return;

    const handleRecheck = () => {
      void this.recheckPermissionStatus();
    };

    window.addEventListener('focus', handleRecheck);
    window.addEventListener(NOTIFICATION_SETTINGS_EVENT, handleRecheck);

    this.cleanupListeners = () => {
      window.removeEventListener('focus', handleRecheck);
      window.removeEventListener(NOTIFICATION_SETTINGS_EVENT, handleRecheck);
    };
  }

  /**
   * Authoritatively consults the OS permission status without re-requesting/prompting.
   * If the OS reports granted/authorized, clears any previous `permissionDenied` latch.
   */
  async recheckPermissionStatus(): Promise<boolean> {
    if (!this.permissionStatusQuery) {
      const queryFn = this.options.getPermissionStatus ?? getNotificationPermissionStatus;
      this.permissionStatusQuery = Promise.resolve(queryFn())
        .then((status) => {
          const isGranted =
            status?.authorization === 'authorized' ||
            status?.authorization === 'provisional' ||
            status?.status === 'granted';
          if (isGranted) {
            this.permissionDenied = false;
          }
          return isGranted;
        })
        .catch(() => false)
        .finally(() => {
          this.permissionStatusQuery = null;
        });
    }
    return this.permissionStatusQuery;
  }

  private isExpectedPermissionRejection(reason: string | undefined): boolean {
    return reason === 'permission-required' || reason === 'blocked-by-system';
  }

  private ensureNotificationPermission(): Promise<boolean> {
    // A previous denial is a latch, not a verdict: the user may have granted permission in OS
    // settings since. Re-consult the authoritative status before giving up, but keep the
    // already-granted path synchronous so a retry still lands in the same dispatch turn.
    if (this.permissionDenied) {
      return this.recheckPermissionStatus().then((granted) =>
        granted ? this.requestNotificationPermissionOnce() : false,
      );
    }
    return this.requestNotificationPermissionOnce();
  }

  private requestNotificationPermissionOnce(): Promise<boolean> {
    if (!this.permissionRequest) {
      const requestFn = this.options.requestPermission ?? requestNotificationPermission;
      this.permissionRequest = Promise.resolve(requestFn())
        .then((result) => {
          const granted = result?.granted === true;
          if (!granted && !result?.error) {
            this.permissionDenied = true;
          } else if (granted) {
            this.permissionDenied = false;
          }
          return granted;
        })
        .finally(() => {
          this.permissionRequest = null;
        });
    }
    return this.permissionRequest;
  }

  private deliverDesktopNotification(dispatchArgs: DispatchNotificationArgs): void {
    const target = dispatchArgs.target;
    if (!target) {
      // An identity-less request has nothing to recall, so it stays off the chain.
      void this.runDispatch(dispatchArgs).catch((err) => {
        this.options.onError?.(err, 'dispatch');
      });
      return;
    }
    const tKey = this.targetKey(target);
    const entry = this.localSeqEntry(tKey);
    // Counted synchronously, before the chain is touched: a withdrawal arriving in this same turn
    // must cover this dispatch even though the chain may make it wait.
    entry.sent += 1;
    const mySeq = entry.sent;
    // Dispatched here, in the caller's own turn: a session that goes needsYou -> working ->
    // needsYou inside one turn must reach the OS twice instead of waiting behind its own
    // predecessor on the chain. The promise wrapper tolerates a partially mocked binding that
    // hands back a plain value instead of a promise.
    const initial = Promise.resolve(dispatchNotification(dispatchArgs));
    // The step below may wait on an older dispatch, so a rejection nobody is awaiting yet must not
    // surface as an unhandled rejection while it waits.
    void initial.catch(() => undefined);
    this.enqueueForTarget(tKey, async () => {
      if (mySeq <= entry.cancelledUpTo) return;
      await this.runDispatch(dispatchArgs, () => mySeq > entry.cancelledUpTo, initial);
    });
  }

  /**
   * One dispatch attempt, including the permission-gated retry. `started` is an attempt the caller
   * already began in its own turn; the identity-less path has nothing to recall and starts its own.
   */
  private async runDispatch(
    dispatchArgs: DispatchNotificationArgs,
    isStillWanted?: () => boolean,
    started?: Promise<DispatchNotificationResult | undefined>,
  ): Promise<void> {
    // Wrapped like every other binding here: partially mocked suites hand back a plain value
    // instead of a promise, and awaiting that must never depend on the mock returning one.
    const result = await (started ?? Promise.resolve(dispatchNotification(dispatchArgs)));
    if (!result || result.submitted) return;
    if (result.reason === 'permission-required') {
      const granted = await this.ensureNotificationPermission();
      if (!granted) {
        this.options.onPermissionUnavailable?.();
        return;
      }
      // The permission prompt is exactly where a withdrawal lands: the user went back to the pane
      // while it was up, so re-check before raising the banner they already dismissed.
      if (isStillWanted && !isStillWanted()) return;
      const retried = await Promise.resolve(dispatchNotification(dispatchArgs));
      if (retried && !retried.submitted && !this.isExpectedPermissionRejection(retried.reason)) {
        this.options.onError?.(
          new Error(`desktop notification not submitted (${retried.reason ?? 'unknown'})`),
          'dispatch',
        );
      }
      return;
    }
    if (result.reason === 'blocked-by-system') {
      this.options.onPermissionUnavailable?.();
      return;
    }
    if (this.isExpectedPermissionRejection(result.reason)) return;
    this.options.onError?.(
      new Error(`desktop notification not submitted (${result.reason ?? 'unknown'})`),
      'dispatch',
    );
  }

  targetKey(target: NotificationTarget): string {
    return `${target.workspaceId}\0${target.sessionId}`;
  }

  /**
   * Serializes the notification work for one pane. An idle chain runs the step in the caller's
   * turn, so a banner still dispatches with the event that asked for it; only a step already in
   * flight defers the next one, which is what keeps a recall behind its own dispatch.
   */
  enqueueForTarget(tKey: string, step: () => Promise<void>): void {
    const pending = this.chainByTarget.get(tKey);
    const tail = (pending ? pending.then(step) : step()).catch((err) => {
      this.options.onError?.(err, 'dispatch');
    });
    this.chainByTarget.set(tKey, tail);
    void tail.finally(() => {
      if (this.chainByTarget.get(tKey) === tail) this.chainByTarget.delete(tKey);
    });
  }

  private localSeqEntry(tKey: string): { sent: number; cancelledUpTo: number } {
    const existing = this.localSeqByTarget.get(tKey);
    if (existing) return existing;
    const entry = { sent: 0, cancelledUpTo: 0 };
    this.localSeqByTarget.set(tKey, entry);
    return entry;
  }

  /**
   * The recall binding to use. Read lazily and tolerantly: several suites here mock `./tauri` with
   * a partial factory, and Vitest throws when a mocked module's missing export is read, so an
   * absent binding has to mean "nothing to recall" instead of failing the caller.
   */
  private resolveWithdraw(): ((target: NotificationTarget) => Promise<void>) | undefined {
    if (this.options.withdrawNotification) return this.options.withdrawNotification;
    try {
      return withdrawNotification;
    } catch {
      return undefined;
    }
  }

  /**
   * Recalls the banner for one pane. The recall is queued even when this coordinator dispatched
   * nothing (a reload, or a second window): the banner belongs to the OS, not to the instance
   * that raised it.
   */
  withdrawForTarget(target: NotificationTarget): void {
    const tKey = this.targetKey(target);
    const entry = this.localSeqEntry(tKey);
    // Cancels this coordinator's own pending permission retry before the recall leaves.
    const withdrawAtSent = entry.sent;
    entry.cancelledUpTo = withdrawAtSent;
    this.enqueueForTarget(tKey, async () => {
      // A newer dispatch for this pane arrived while this recall waited behind the dispatch it
      // recalls. That newer banner superseded the one being recalled, so recalling now would
      // delete the banner the user is actually looking at: the dispatch IS the recall.
      if (entry.sent > withdrawAtSent) return;
      const withdraw = this.resolveWithdraw();
      if (!withdraw) return;
      await Promise.resolve(withdraw(target));
    });
  }

  withdrawForSession(workspaceId: string | undefined, sessionId: string | undefined): void {
    if (!workspaceId || !sessionId) return;
    this.withdrawForTarget({ workspaceId, sessionId });
  }

  // A click can only navigate when both the frontend workspace and session identities are
  // known. Probes and identity-less events still dispatch, just without a navigation target.
  private navigationTarget(
    workspaceId: string | undefined,
    sessionId: string | undefined,
  ): Pick<DispatchNotificationArgs, 'target'> {
    if (!workspaceId || !sessionId) return {};
    return { target: { workspaceId, sessionId } };
  }

  private isFocused(): boolean {
    return this.options.isWindowFocused ? this.options.isWindowFocused() : isWindowForegroundFocused();
  }

  private isLeader(event: SessionOwnershipParams): boolean {
    if (this.options.isNotificationLeader) return this.options.isNotificationLeader(event);
    // Callers that predate multi-window only ever pass events their own store resolved.
    return shouldThisWindowNotifyFor(event.backendSessionId, event.ownsSession ?? true);
  }

  private resolveSettings(override?: NotificationSettings): NotificationSettings {
    if (override) return override;
    if (this.options.getSettings) return this.options.getSettings();
    return loadNotificationSettings();
  }

  handleTerminalBell(params: TerminalBellEventParams): NotificationDecision {
    const key = params.sessionId || params.tabId || 'default';
    const settings = this.resolveSettings(params.settings);

    const now = Date.now();
    const lastBell = this.lastBellTimestamp.get(key) ?? 0;
    if (now - lastBell < this.bellThrottleMs) {
      return { accepted: false };
    }
    this.lastBellTimestamp.set(key, now);

    const lastCompletion = this.lastAgentCompletionTimestamp.get(key) ?? 0;
    if (now - lastCompletion < this.bellAgentSuppressionMs) {
      return { accepted: false, suppressed: true };
    }

    const decision = { accepted: true };
    const isFocused = this.isFocused();

    if (!isFocused) {
      const tabId = params.tabId;
      const wtPath = params.worktreePath || params.worktreeId;

      if (tabId && this.options.onMarkTabUnread) {
        this.options.onMarkTabUnread(tabId, params.workspaceId);
      }
      if (wtPath && this.options.onMarkWorktreeUnread) {
        this.options.onMarkWorktreeUnread(wtPath, params.workspaceId);
      }

      if (settings.enabled && settings.terminalBell && this.isLeader(params)) {
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
            if (result && settings.customSoundId !== 'none' && settings.customSoundId !== 'system' && settings.customSoundPath && !result.played && result.reason !== 'deduped') {
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
          workspaceLabel: params.workspaceLabel,
          sound: settings.customSoundId === 'system' ? 'system' : 'silent',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
          ...this.navigationTarget(params.workspaceId, params.sessionId),
        };
        this.deliverDesktopNotification(dispatchArgs);
      }
    }
    return decision;
  }

  handleAgentStateChange(params: AgentStateChangeEventParams): NotificationDecision {
    const key = params.sessionId || params.tabId || 'default';
    const next = params.nextState || params.newState || '';
    const settings = this.resolveSettings(params.settings);

    const effectivePrev = params.previousState ?? this.lastAgentState.get(key);
    this.lastAgentState.set(key, next);

    if (params.workspaceId && params.sessionId) {
      this.lastTargetByKey.set(key, { workspaceId: params.workspaceId, sessionId: params.sessionId });
    }

    // Leaving attention is what retires the banner. A group frame carries its own previous group;
    // a local edge only knows the state words, so the remembered group stands in for the missing
    // half rather than being read as "was not attention".
    const prevGroup = params.group !== undefined
      ? params.previousGroup ?? this.lastGroup.get(key)
      : undefined;
    if (params.group !== undefined) this.lastGroup.set(key, params.group);
    const leftAttention = params.group !== undefined
      ? (prevGroup === 'needsYou' || prevGroup === 'unread') &&
        params.group !== 'needsYou' &&
        params.group !== 'unread'
      : (effectivePrev === 'waiting' || effectivePrev === 'done') && !this.isAttentionWord(next);
    if (leftAttention) {
      const target = params.workspaceId && params.sessionId
        ? { workspaceId: params.workspaceId, sessionId: params.sessionId }
        : this.lastTargetByKey.get(key);
      if (target) this.withdrawForTarget(target);
    }

    // Entering an attention state from a non-attention one is a completion edge — and so is
    // moving BETWEEN distinct attention states (done -> waiting means the agent needs input
    // again; waiting -> done means the blocked turn finished). The store treats those the
    // same way: fresh unseen attention with highlight + unread. Only same-state repeats dedupe.
    const isAttentionState = this.isAttentionWord(next);
    const wasAttentionState = effectivePrev === 'waiting' || effectivePrev === 'done';
    // The daemon's group, when known, is the only judge: entering needsYou or unread (or moving
    // between them) is the edge; the same group again (a reason or screen change) never is.
    const isCompletionEdge = params.group !== undefined
      ? (params.group === 'needsYou' || params.group === 'unread') && params.previousGroup !== params.group
      : isAttentionState &&
        effectivePrev !== undefined &&
        (!wasAttentionState || effectivePrev !== next);

    if (!isCompletionEdge || params.notificationSuppressed) {
      return params.notificationSuppressed
        ? { accepted: false, suppressed: true }
        : { accepted: false };
    }

    const decision: NotificationDecision = { accepted: true };
    const now = Date.now();
    // Suppression must be SYMMETRIC. The bell handler already ignores a bell that
    // lands just after a completion, but most TUI agents ring the bell AS they
    // finish, so the bell arrives a few hundred ms BEFORE the store's transition to
    // waiting/done. That ordering was unguarded: the user got two OS banners and two
    // sounds for one completion, and the tab was marked unread twice. Keep the
    // completion timestamp either way so later bells stay suppressed.
    //
    // However, do NOT drop the attention state itself: the transition to waiting or done
    // must still be accepted so the notification center records the richer agent entry,
    // while the desktop notification (banner and sound) and unread marks are suppressed
    // to avoid duplicating what the bell already announced.
    const lastBell = this.lastBellTimestamp.get(key) ?? 0;
    const bellSuppressed = now - lastBell < this.bellAgentSuppressionMs;
    this.lastAgentCompletionTimestamp.set(key, now);
    if (bellSuppressed) {
      decision.suppressed = true;
    }

    const isFocused = this.isFocused();

    if (!isFocused) {
      const tabId = params.tabId;
      const wtPath = params.worktreePath || params.worktreeId;

      if (!bellSuppressed) {
        if (tabId && this.options.onMarkTabUnread) {
          this.options.onMarkTabUnread(tabId, params.workspaceId);
        }
        if (wtPath && this.options.onMarkWorktreeUnread) {
          this.options.onMarkWorktreeUnread(wtPath, params.workspaceId);
        }
      }

      if (!bellSuppressed && settings.enabled && settings.agentTaskComplete && this.isLeader(params)) {
        Promise.resolve(
          playNotificationSound({
            soundId: settings.customSoundId ?? 'system',
            customSoundPath: settings.customSoundPath,
            volume: settings.customSoundVolume,
          })
        )
          .then((result) => {
            if (result && settings.customSoundId !== 'none' && settings.customSoundId !== 'system' && settings.customSoundPath && !result.played && result.reason !== 'deduped') {
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
          workspaceLabel: params.workspaceLabel,
          attentionReason: next === 'waiting' ? 'waiting' : 'done',
          sound: settings.customSoundId === 'system' ? 'system' : 'silent',
          worktreeLabel: params.worktreeLabel || params.worktreeId,
          terminalTitle: params.terminalTitle,
          agentLabel: params.agentLabel,
          ...this.navigationTarget(params.workspaceId, params.sessionId),
        };
        this.deliverDesktopNotification(dispatchArgs);
      }
    }
    return decision;
  }

  private isAttentionWord(state: string): boolean {
    return state === 'waiting' || state === 'done';
  }

  dispose(): void {
    if (this.cleanupListeners) {
      this.cleanupListeners();
      this.cleanupListeners = null;
    }
  }

  reset(): void {
    this.lastBellTimestamp.clear();
    this.lastAgentState.clear();
    this.lastAgentCompletionTimestamp.clear();
    this.lastGroup.clear();
    this.lastTargetByKey.clear();
    this.localSeqByTarget.clear();
    this.permissionRequest = null;
    this.permissionStatusQuery = null;
    this.permissionDenied = false;
  }
}

export const defaultNotificationCoordinator = new NotificationCoordinator();
