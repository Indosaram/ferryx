import { Loader2, RefreshCw, TerminalSquare } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import type { TerminalActivity } from "../lib/activity";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";
import { getAgentReconnectAffordance } from "../lib/agentResumeAffordance";
import { agentDisplayNameForType } from "../lib/agentTitle";
import { Button } from "./ui/button";
import { cn } from "../lib/cn";
import { localSplitIntent, retryLocalSplit, cancelLocalSplit } from "../lib/localSplitLifecycle";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import {
  isSessionAutoResumeHeld,
  isSessionSleeping,
  isStandbyBackendSessionId,
  REMOTE_SESSION_GONE_REASON,
  registerSessionSnapshot,
  resumeRegisteredSession,
  setSessionActive,
  setSessionSleeping,
  useSleepingSessionIds,
  getDaemonSuspension,
} from "../lib/sessionLifecycle";
import { toIpcError, type AgentContinueMode } from "../lib/tauri";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import { TERMINAL_SEARCH_DOCK_HEIGHT } from "../lib/terminalSearchLayout";
import type { TerminalSession } from "../lib/types";
import type { LocalRecoveryAdmissionResult } from "../state/workspaceStore";
import { NativeTerminalPane } from "./NativeTerminalPane";
import { ContinueInNewPaneButtons } from "./ContinueInNewPaneButtons";
import { CompatibilityTerminal } from "./CompatibilityTerminal";
import { isNativeRendererUnavailable, subscribeNativeRendererAvailability } from "../lib/nativeTerminalLifecycle";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";
import { DagPaneBadge } from "./dag/DagPaneBadge";
import { useAdvancedFeatures } from "../lib/advancedFeatures";
import { remoteHostStore } from "../state/remoteHostStore";

type TerminalPaneProps = {
  session: TerminalSession;
  sessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
  active: boolean;
  inputActive?: boolean;
  activity?: TerminalActivity;
  needsAttention?: boolean;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
  onReconnect?: (sessionId: string) => Promise<void> | void;
  onRefreshSessionIdentity?: (sessionId: string, onResult: (result: LocalRecoveryAdmissionResult) => void) => () => void;
  onOpenNewShell?: (sessionId: string) => Promise<void> | void;
  onBackendSessionUnavailable?: (
    sessionId: string,
    backendSessionId: string,
    reason: string,
    bindingKey?: string | null,
  ) => void;
  continuationModes?: readonly AgentContinueMode[];
  onContinueInNewPane?: (mode: AgentContinueMode) => void;
  continuationHeld?: boolean;
};

/** How long a reconnect may run before its notice takes pane space. */
export const OUTAGE_NOTICE_GRACE_MS = 1500;

function friendlyAgentName(agentType: string | null | undefined): string {
  if (!agentType) return "Agent";
  const clean = agentType.trim().toLowerCase();
  const matched = agentDisplayNameForType(clean);
  if (matched) return matched;
  return `${clean.charAt(0).toUpperCase()}${clean.slice(1)}`;
}

function resolveAffordanceErrorDescription(affordance: ReturnType<typeof getAgentReconnectAffordance>): string | null {
  switch (affordance.status) {
    case "conflict":
      if (affordance.conflictingSessionId) {
        return `This session is already active in another pane (${affordance.conflictingSessionId}).`;
      }
      return "This session is already active in another pane.";
    case "missing_reference":
      return "Session reference unavailable.";
    case "unsupported":
      return `${friendlyAgentName(affordance.agentType)} sessions cannot be reconnected.`;
    case "failed": {
      const code = affordance.error?.code;
      if (code === "AGENT_RESUME_INVALID") {
        return "This session has an invalid reconnect reference.";
      }
      if (code === "AGENT_SESSION_CONFLICT") {
        return "This session is already active in another pane.";
      }
      if (code === "AGENT_RESUME_UNSUPPORTED") {
        return `${friendlyAgentName(affordance.agentType)} sessions cannot be reconnected.`;
      }
      if (code === "DAEMON_PROTOCOL_MISMATCH") {
        return "Daemon protocol mismatch while reconnecting.";
      }
      if (code === "DAEMON_UPDATE_PENDING") {
        return "Update installed; this terminal reconnects after the previous version's terminals close.";
      }
      return "Reconnect failed.";
    }
    default:
      return null;
  }
}

export function TerminalPane({
  session,
  sessions,
  active,
  inputActive = active,
  activity,
  needsAttention,
  searchOpen,
  onCloseSearch,
  onReconnect,
  onRefreshSessionIdentity,
  onOpenNewShell,
  onBackendSessionUnavailable,
  continuationModes,
  onContinueInNewPane,
  continuationHeld = false,
}: TerminalPaneProps) {
  const [pendingLocal, setPendingLocal] = useState(false);
  // Linux/Windows native children stack above the webview. Keep search in a
  // separate strip rather than hiding/detaching the terminal it needs to query.
  const dockSearch = Boolean(searchOpen && !isMacShortcutPlatform());
  const advancedFeatures = useAdvancedFeatures();
  const [replacementError, setReplacementError] = useState<string | null>(null);
  const titleId = useId();
  const descId = useId();
  const autoResumeKeyRef = useRef<string | null>(null);
  const admissionDisposeRef = useRef<(() => void) | null>(null);
  // Connection and exit states dock to the pane's bottom edge instead of covering it. The bar's
  // measured height is taken out of the native viewport, so a native child surface (which stacks
  // above the webview on Windows/Linux) never hides it and the last frame stays readable.
  const statusBarRef = useRef<HTMLDivElement | null>(null);
  const [statusBarHeight, setStatusBarHeight] = useState(0);
  const [statusBarHasAction, setStatusBarHasAction] = useState(false);
  const nativeRendererUnavailable = useSyncExternalStore(
    subscribeNativeRendererAvailability,
    isNativeRendererUnavailable,
  );

  useEffect(() => {
    return () => {
      admissionDisposeRef.current?.();
      admissionDisposeRef.current = null;
    };
  }, [session.id, session.backendSessionId]);
  const sleepingSessionIds = useSleepingSessionIds();

  const splitIntent = localSplitIntent(session);
  const splitPending = Boolean(splitIntent && session.reconnectLifecycle !== "idle" && session.reconnectLifecycle !== "failed");

  // An SSH pane whose backend is confirmed gone sits on a standby backend like a lazily restored
  // local pane: it takes the local presentation and is resumed (agent) or reopened (shell) on the
  // same remote host. Only a pane that still owns a remote target uses the SSH reattach overlay.
  const isSshSession = isRemoteWorkspaceId(session.workspaceId) && !isStandbyBackendSessionId(session.backendSessionId);
  const isRemoteGone = session.backendUnavailableReason === REMOTE_SESSION_GONE_REASON &&
    isStandbyBackendSessionId(session.backendSessionId);
  const isSpawning = session.reconnectLifecycle === "spawning" || session.reconnectLifecycle === "validating";
  const remoteState = session.remoteConnectionState;
  const recoveryState = session.recoveryState;
  const isSshReconnecting = isSshSession && recoveryState?.phase !== "blocked" && recoveryState?.phase !== "exited" &&
    (recoveryState?.phase === "recovering" || remoteState === "reconnecting" || isSpawning);
  // `missing` means the daemon we queried does not know this session (a draining predecessor may
  // still own it, or restore has not completed). That is a recoverable transport-level outage, so
  // it takes the same recoverable branch as `disconnected` and never the terminal expired branch.
  const isSshDisconnected =
    isSshSession && recoveryState?.phase !== "recovering" && recoveryState?.phase !== "blocked" && recoveryState?.phase !== "exited" && !isSpawning && (remoteState === "disconnected" || remoteState === "missing" || (!remoteState && (session.backendSessionId === null || session.lifecycle === "exited")));
  const isSshExpired = isSshSession && recoveryState?.phase !== "recovering" && recoveryState?.phase !== "blocked" && !isSpawning && remoteState === "expired";
  const isSshLegacyLost = isSshSession && recoveryState?.phase !== "recovering" && recoveryState?.phase !== "blocked" && recoveryState?.phase !== "exited" && !isSpawning && remoteState === "legacyLost";
  const isRecoveryRecovering = recoveryState?.phase === "recovering";
  const isRecoveryBlocked = recoveryState?.phase === "blocked";
  const isRecoveryExited = recoveryState?.phase === "exited";
  const showSshOverlay = isSshSession && (isRecoveryRecovering || isRecoveryBlocked || isRecoveryExited || isSshReconnecting || isSshDisconnected || isSshExpired || isSshLegacyLost);

  const affordance = getAgentReconnectAffordance(session, sessions);
  // A daemon that has not answered yet is a transport-level outage, not a dead shell. Treat it as
  // reconnecting so the pane never claims "Shell exited" for a session that is still alive. An
  // in-flight reconnect transaction (`affordance.isReconnecting`) already owns its own progress
  // affordance, so it is excluded here and keeps rendering the disabled retry button.
  const isLocalReconnecting =
    !isSshSession && !affordance.isReconnecting && !isRecoveryBlocked && !isRecoveryExited && (remoteState === "reconnecting" || isSpawning || isRecoveryRecovering);
  const isLocalDisconnected = !isSshSession && !isRecoveryRecovering && !isRecoveryBlocked && !isSpawning && (remoteState === "disconnected" || remoteState === "missing");
  const isLocalExited = !isSshSession && (isRecoveryExited || (!isRecoveryBlocked && !isRecoveryRecovering && (splitIntent ? session.lifecycle === "exited" : (
    isLocalReconnecting ||
    isLocalDisconnected ||
    session.backendSessionId === null ||
    isStandbyBackendSessionId(session.backendSessionId) ||
    session.lifecycle === "exited"
  ))));
  const isExited = isSshSession ? showSshOverlay : isRecoveryBlocked || isRecoveryRecovering || isLocalExited;
  const isSuspended =
    !isExited && (sleepingSessionIds.has(session.id) || session.processState === "suspended");
  const isAgentSession = Boolean(
    (session.agentType && session.agentType.trim().length > 0) ||
      affordance.agentType ||
      session.providerSession ||
      (activity?.isAgent && activity?.agentType),
  );
  const effectiveAgentType =
    affordance.agentType ??
    (session.agentType && session.agentType.trim().length > 0 ? session.agentType.trim() : null) ??
    (activity?.isAgent && activity?.agentType ? activity.agentType : null);
  const agentName = friendlyAgentName(effectiveAgentType);
  const logo = resolveAgentLogo(effectiveAgentType);
  const isMonochrome = isMonochromeAgentLogo(effectiveAgentType);

  const isPending = pendingLocal || isSpawning || (isSshSession ? isSshReconnecting : affordance.isReconnecting || isLocalReconnecting);
  // A lazily restored shell carries a frontend-only standby id: no process runs and a new one
  // starts on demand. Nothing crashed, so "Shell exited" told users the wrong story. Name the
  // real state instead: starting, not running, or failed to start.
  const isStandbyShell =
    !isSshSession &&
    !isAgentSession &&
    isStandbyBackendSessionId(session.backendSessionId) &&
    !isLocalReconnecting &&
    !isLocalDisconnected;
  const canReattach = Boolean(
    session.backendSessionId &&
    !isStandbyBackendSessionId(session.backendSessionId) &&
    onReconnect
  );
  const canRetryRecovery =
    (recoveryState?.phase === "recovering" || recoveryState?.phase === "blocked") &&
    Boolean(onRefreshSessionIdentity) &&
    Boolean(session.backendSessionId);
  const affordanceDescription = resolveAffordanceErrorDescription(affordance);
  const errorDescription =
    replacementError ??
    (isSshSession
      ? isRecoveryBlocked ? null : recoveryState?.reason ?? session.remoteFailure?.message ?? null
      : isAgentSession
        ? affordanceDescription ?? session.backendUnavailableReason ?? null
        : session.backendUnavailableReason ?? affordanceDescription ?? null);

  const handleReconnect = async () => {
    if (isPending && !canRetryRecovery) return;
    if (canRetryRecovery) {
      if (onRefreshSessionIdentity) {
        admissionDisposeRef.current?.();
        const dispose = onRefreshSessionIdentity(session.id, () => undefined);
        admissionDisposeRef.current = typeof dispose === "function" ? dispose : null;
        return;
      }
      if (!onReconnect) return;
      setPendingLocal(true);
      setReplacementError(null);
      try {
        await onReconnect(session.id);
      } catch (error) {
        setReplacementError(toIpcError(error).message);
      } finally {
        setPendingLocal(false);
      }
      return;
    }
    if (isSshSession) {
      if (!onReconnect) return;
      setPendingLocal(true);
      setReplacementError(null);
      try {
        await onReconnect(session.id);
      } catch (error) {
        setReplacementError(toIpcError(error).message);
      } finally {
        setPendingLocal(false);
      }
      return;
    }
    if ((!affordance.canReconnect && !canReattach) || !onReconnect) return;
    setPendingLocal(true);
    setReplacementError(null);
    try {
      await onReconnect(session.id);
    } catch (error) {
      setReplacementError(toIpcError(error).message);
    } finally {
      setPendingLocal(false);
    }
  };

  const handleOpenNewShell = async () => {
    // A local reconnecting overlay is itself "pending", but the user must always be able to escape a
    // session that may never come back (e.g. every PTY died with an OS reboot). Only a replacement
    // already in flight (`pendingLocal`) blocks that escape.
    if ((isPending && !isLocalReconnecting) || pendingLocal || !onOpenNewShell) return;
    setPendingLocal(true);
    setReplacementError(null);
    try {
      await onOpenNewShell(session.id);
    } catch (error) {
      setReplacementError(toIpcError(error).message);
    } finally {
      setPendingLocal(false);
    }
  };

  const handleResume = () => {
    void resumeRegisteredSession(session.id).catch((error) => {
      console.warn("Failed to resume suspended session:", error);
    });
  };

  useEffect(() => {
    if (active && splitIntent && !splitIntent.ready && session.backendSessionId &&
        session.reconnectLifecycle === "idle" && !splitIntent.cancelRequested) {
      void retryLocalSplit(session.id).catch((error) => setReplacementError(toIpcError(error).message));
    }
  }, [active, session.id, session.backendSessionId, session.reconnectLifecycle, splitIntent]);

  useEffect(() => {
    registerSessionSnapshot(session, activity?.state);
    setSessionActive(session.id, active);
    if (!isExited && session.backendSessionId && !isStandbyBackendSessionId(session.backendSessionId) && autoResumeKeyRef.current !== null) {
      setSessionSleeping(session.id, false);
      autoResumeKeyRef.current = null;
    }
    return () => setSessionActive(session.id, false);
  }, [active, activity?.state, isExited, session]);

  useEffect(() => {
    // Lazy restore: only the pane the user is looking at may start its shell. A hidden tab
    // stays asleep until it becomes active, so restoring a workspace spawns nothing unseen.
    const confirmedAgentExit = isAgentSession && isRecoveryExited && session.backendSessionId === null;
    if (!active || splitIntent || !isExited || isPending || isRecoveryBlocked || (isRecoveryExited && !confirmedAgentExit) || isSshSession || !(isSessionSleeping(session.id) || isRemoteGone || confirmedAgentExit) || isSessionAutoResumeHeld(session.id)) return;
    const key = `${session.id}:${session.backendSessionId ?? "none"}`;
    if (autoResumeKeyRef.current === key) return;
    if (isAgentSession) {
      if (!affordance.canReconnect || !onReconnect) return;
      autoResumeKeyRef.current = key;
      setPendingLocal(true);
      setReplacementError(null);
      void Promise.resolve(onReconnect(session.id))
        .then(() => setSessionSleeping(session.id, false))
        .catch((error) => setReplacementError(toIpcError(error).message))
        .finally(() => setPendingLocal(false));
      return;
    }
    if (!onOpenNewShell) return;
    autoResumeKeyRef.current = key;
    setPendingLocal(true);
    setReplacementError(null);
    void Promise.resolve(onOpenNewShell(session.id))
      .then(() => setSessionSleeping(session.id, false))
      .catch((error) => setReplacementError(toIpcError(error).message))
      .finally(() => setPendingLocal(false));
  }, [active, affordance.canReconnect, isAgentSession, isExited, isPending, isRecoveryBlocked, isRecoveryExited, isRemoteGone, isSshSession, onOpenNewShell, onReconnect, session.backendSessionId, session.id, session.reconnectLifecycle]);

  const splitStatusShown = Boolean(splitIntent && session.reconnectLifecycle !== "idle");
  // A reconnect that settles within the grace window changes nothing on screen: the docked bar
  // would otherwise resize the PTY (SIGWINCH) twice for a blip. Past the window the notice docks.
  const isTransientOutage = !isRecoveryBlocked && !isRecoveryExited && (isSshSession ? isSshReconnecting : isLocalReconnecting);
  const isRecoverableOutage = !isRecoveryBlocked && !isRecoveryExited &&
    (isSshSession ? isSshReconnecting || isSshDisconnected : isLocalReconnecting || isLocalDisconnected);
  const [graceElapsed, setGraceElapsed] = useState(false);
  useEffect(() => {
    if (!isTransientOutage) {
      setGraceElapsed(false);
      return;
    }
    const timer = setTimeout(() => setGraceElapsed(true), OUTAGE_NOTICE_GRACE_MS);
    return () => clearTimeout(timer);
  }, [isTransientOutage]);
  const statusBarHidden = isExited && isTransientOutage && !graceElapsed;
  // Recoverable outages dock at the top (the session may come back); final states dock at the bottom.
  const statusBarEdge: "top" | "bottom" = isRecoverableOutage ? "top" : "bottom";
  const topDocked = isExited && statusBarEdge === "top";
  const [exitedAt, setExitedAt] = useState<number | null>(null);
  useEffect(() => {
    setExitedAt(isExited ? Date.now() : null);
  }, [isExited]);
  const statusBarShown = (splitStatusShown || isExited || isSuspended) && !statusBarHidden;
  useLayoutEffect(() => {
    const bar = statusBarRef.current;
    if (!statusBarShown || !bar) {
      setStatusBarHeight(0);
      return;
    }
    const measure = () => {
      setStatusBarHeight(Math.ceil(bar.getBoundingClientRect().height));
      setStatusBarHasAction(bar.querySelector("button:not(:disabled)") !== null);
    };
    measure();
    if (typeof ResizeObserver === "undefined" || typeof MutationObserver === "undefined") return;
    // Button availability changes without a size change (pending -> idle), so watch both.
    const mutations = new MutationObserver(measure);
    mutations.observe(bar, { subtree: true, childList: true, attributes: true, attributeFilter: ["disabled"] });
    const observer = new ResizeObserver(measure);
    observer.observe(bar);
    return () => {
      mutations.disconnect();
      observer.disconnect();
    };
  }, [statusBarShown, splitStatusShown, isExited, isSuspended]);

  // If the native paired proxy session has not been established or expired,
  // do not mount local path/PTY tooling or offer SSH/agent respawn.
  if (isPairedWorkspaceId(session.workspaceId) && (!session.backendSessionId || session.remoteConnectionState === "expired")) {
    if (session.reconnectLifecycle === "spawning" || session.lifecycle === "working") {
      return (
        <div data-testid="paired-terminal-spawning" role="status" className="flex h-full items-center justify-center p-6 text-center text-sm text-muted-foreground">
          <div className="flex flex-col items-center gap-2">
            <span className="inline-block size-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
            <h2 className="font-medium text-foreground">Connecting to paired terminal...</h2>
            <p>Establishing native session with the paired daemon.</p>
          </div>
        </div>
      );
    }
    const pairedMachineFeaturesEnabled = remoteHostStore.getState().machineFeaturesEnabled === true;
    if (!pairedMachineFeaturesEnabled || !onOpenNewShell) {
      return <div data-testid="paired-terminal-unavailable" role="status" className="flex h-full items-center justify-center p-6 text-center text-sm text-muted-foreground">
        <div>
          <h2 className="font-medium text-foreground">Paired terminal unavailable</h2>
          <p>Native remote terminal support is not available in this version. The saved pane is retained; no replacement shell has been started.</p>
          {session.remoteConnectionState === "expired" ? <p>The owning daemon reported this session expired.</p> : null}
          <p>Reconnect and file or image actions are disabled. No Local or SSH fallback is used.</p>
        </div>
      </div>;
    }
  }

  return (
    <div
      data-testid="terminal-pane-surface"
      className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
      onKeyDownCapture={(event) => {
        // Enter in an exited or disconnected pane runs the bar's primary action (its first enabled
        // button), so recovery never needs the mouse. A live or still-reconnecting pane keeps Enter.
        if (!active || !isExited || isPending || isTransientOutage || statusBarHidden) return;
        if (event.key !== "Enter" || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey || event.nativeEvent.isComposing) return;
        const bar = statusBarRef.current;
        if (!bar || bar.contains(event.target as Node)) return;
        const primary = bar.querySelector<HTMLButtonElement>("button:not(:disabled)");
        if (!primary) return;
        event.preventDefault();
        event.stopPropagation();
        primary.click();
      }}
    >
      {advancedFeatures ? (
        <DagPaneBadge
          projectPath={session.worktreePath ?? session.cwd}
          workspaceId={session.workspaceId}
          paneId={session.id}
          providerSessionId={session.providerSession?.id ?? null}
          sessions={sessions}
          agentPresent={activity?.isAgent === true}
          agentWorking={activity?.isAgent === true && activity.state === "working"}
        />
      ) : null}
      <div
        data-testid="terminal-native-viewport"
        className="absolute inset-x-0 bottom-0 min-h-0 min-w-0"
        style={{
          top: (dockSearch ? TERMINAL_SEARCH_DOCK_HEIGHT : 0) + (topDocked ? statusBarHeight : 0),
          bottom: topDocked ? 0 : statusBarHeight,
        }}
      >
        {nativeRendererUnavailable && session.backendSessionId && !isStandbyBackendSessionId(session.backendSessionId) ? (
          <CompatibilityTerminal backendSessionId={session.backendSessionId} />
        ) : <NativeTerminalPane
          sessionId={session.id}
          session={session}
          splitAttempt={splitIntent ? { frontendSessionId: session.id, generation: splitIntent.generation } : undefined}
          active={inputActive}
          activity={activity}
          needsAttention={needsAttention}
          onRefreshSessionIdentity={onRefreshSessionIdentity}
          onBackendSessionUnavailable={(backendSessionId, reason, bindingKey) => {
            onBackendSessionUnavailable?.(session.id, backendSessionId, reason, bindingKey);
          }}
        />}
      </div>
      {splitIntent && session.reconnectLifecycle !== "idle" ? (
        <div
          role={splitPending ? "status" : "alert"}
          ref={statusBarRef}
          data-testid="local-split-status"
          data-pane-status-bar="docked"
          className="absolute inset-x-0 bottom-0 z-20 border-t border-border bg-card px-3 py-1.5 text-left"
        >
          <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
            <h2 className="shrink-0 whitespace-nowrap text-[12px] font-medium text-foreground">
              {splitPending
                ? session.backendSessionId
                  ? "Connecting to shell..."
                  : "Starting shell..."
                : session.backendSessionId
                  ? "Connection failed"
                  : "Shell startup unconfirmed"}
            </h2>
            {!splitPending ? (
              <>
                <p className="text-[12px] text-muted-foreground">
                  {replacementError ?? session.reconnectError?.message ?? "Startup was not confirmed."}
                </p>
                <div className="ml-auto flex gap-2">
                  <Button
                    onClick={() =>
                      void retryLocalSplit(session.id).catch((error) =>
                        setReplacementError(toIpcError(error).message),
                      )
                    }
                  >
                    Retry
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() =>
                      void cancelLocalSplit(session.id).catch((error) =>
                        setReplacementError(toIpcError(error).message),
                      )
                    }
                  >
                    Cancel
                  </Button>
                </div>
              </>
            ) : null}
          </div>
        </div>
      ) : null}
      {searchOpen ? (
        <TerminalSearchOverlay
          docked={dockSearch}
          sessionId={session.backendSessionId ?? session.id}
          onClose={onCloseSearch ?? (() => undefined)}
        />
      ) : null}
      {isSuspended ? (
        <div
          role="button"
          tabIndex={0}
          data-testid="terminal-suspended-overlay"
          onClick={handleResume}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              handleResume();
            }
          }}
          ref={statusBarRef}
          data-pane-status-bar="docked"
          className="absolute inset-x-0 bottom-0 z-20 cursor-pointer select-none border-t border-border bg-card px-3 py-1.5 text-left"
        >
          <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
            <h2 className="shrink-0 whitespace-nowrap text-[12px] font-medium text-foreground">Suspended</h2>
            <p className="text-[12px] text-muted-foreground">
              {getDaemonSuspension(session.id)?.suspensionSource ?? "Stop source unconfirmed"}
            </p>
            <div className="ml-auto">
              <Button size="sm" className="h-7 shrink-0 text-[11px]" onClick={(e) => { e.stopPropagation(); handleResume(); }}>
                Resume
              </Button>
            </div>
          </div>
        </div>
      ) : null}
      {isExited ? (
        <div
          role="region"
          aria-labelledby={titleId}
          aria-describedby={errorDescription ? descId : undefined}
          ref={statusBarRef}
          data-testid="terminal-pane-overlay"
          data-pane-status-bar={statusBarHidden ? "pending" : "docked"}
          data-pane-status-edge={statusBarEdge}
          aria-keyshortcuts={isPending || isTransientOutage || !statusBarHasAction ? undefined : "Enter"}
          style={topDocked ? { top: dockSearch ? TERMINAL_SEARCH_DOCK_HEIGHT : 0 } : undefined}
          className={cn(
            "absolute inset-x-0 z-20 border-border bg-card px-3 py-1.5 text-left",
            topDocked ? "border-b" : "bottom-0 border-t",
            statusBarHidden && "sr-only",
          )}
        >
          <div className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1">
            {isAgentSession ? (
              <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-muted/60">
                {logo ? (
                  <img
                    src={logo}
                    alt=""
                    className={cn("size-4", isMonochrome && "agent-tab-logo--monochrome")}
                  />
                ) : (
                  <TerminalSquare className="size-4 text-muted-foreground" aria-hidden="true" />
                )}
              </div>
            ) : (
              <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-muted/60">
                <TerminalSquare className="size-4 text-muted-foreground" aria-hidden="true" />
              </div>
            )}

            <h2 id={titleId} className="min-w-[4rem] shrink-[0.2] truncate text-[12px] font-medium text-foreground">
              {isSshSession
                ? isRecoveryBlocked
                  ? "Session recovery blocked"
                  : isRecoveryExited
                    ? "Remote session exited"
                    : isSshReconnecting
                  ? "Reconnecting SSH..."
                  : isSshExpired
                    ? "Remote session expired"
                    : isSshLegacyLost
                      ? "Legacy SSH session lost"
                      : "SSH disconnected"
                : isLocalReconnecting
                  ? session.reconnectLifecycle === "spawning" || session.reconnectLifecycle === "binding"
                    ? "Starting shell..."
                    : "Reconnecting session..."
                  : isRecoveryBlocked
                    ? "Session recovery blocked"
                    : isStandbyShell
                    ? replacementError
                      ? "Couldn't start shell"
                      : isPending
                        ? "Starting shell..."
                        : "Shell not running"
                    : isAgentSession
                      ? "Session disconnected"
                      : session.exitCode != null && session.exitCode !== 0
                        ? `Shell exited (code ${session.exitCode})`
                        : "Shell exited"}
            </h2>

            {exitedAt !== null && !isRecoverableOutage ? (
              <time dateTime={new Date(exitedAt).toISOString()} className="min-w-0 truncate text-[12px] tabular-nums text-muted-foreground">
                {new Date(exitedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
              </time>
            ) : null}

            {isAgentSession ? (
              <p className="min-w-0 truncate text-[12px] text-muted-foreground">{agentName}</p>
            ) : null}

            {errorDescription ? (
              <p id={descId} role={replacementError ? "alert" : undefined} className="min-w-0 truncate text-[12px] text-muted-foreground" title={errorDescription}>
                {errorDescription}
              </p>
            ) : null}

            {isSshSession ? (
              <div className="ml-auto flex min-w-0 max-w-full flex-wrap items-center justify-end gap-2">
                {isRecoveryBlocked ? (
                  <>
                    <p role="status" className="min-w-0 truncate text-[12px] text-muted-foreground">{recoveryState?.reason ?? "Recovery is blocked."}</p>
                    <div className="flex shrink-0 items-center gap-2">
                      {canRetryRecovery ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          disabled={isPending}
                          aria-busy={isPending}
                          aria-label="Retry session recovery"
                          onClick={handleReconnect}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <RefreshCw className="size-3.5" aria-hidden="true" />
                          <span>Retry</span>
                        </Button>
                      ) : null}
                      {onOpenNewShell ? (
                        <Button
                          type="button"
                          size="sm"
                          variant={canRetryRecovery ? "ghost" : "secondary"}
                          disabled={isPending}
                          aria-label="Open new shell"
                          onClick={handleOpenNewShell}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <span>Open new shell</span>
                        </Button>
                      ) : null}
                    </div>
                  </>
                ) : isRecoveryExited ? (
                  <>
                    <p role="status" className="min-w-0 truncate text-[12px] text-muted-foreground">The remote owner confirmed this session exited.</p>
                    {onOpenNewShell ? (
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        aria-label="Open new shell"
                        onClick={handleOpenNewShell}
                        className="h-7 shrink-0 text-[11px]"
                      >
                        <span>Open new shell</span>
                      </Button>
                    ) : null}
                  </>
                ) : isSshReconnecting ? (
                  <>
                    <div role="status" className="flex min-w-0 items-center gap-2 truncate text-[12px] text-muted-foreground">
                      <Loader2 className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
                      <span>Reconnecting to remote session...</span>
                    </div>
                    {canRetryRecovery ? <Button type="button" size="sm" variant="secondary" className="h-7 shrink-0 text-[11px]" aria-label="Retry session recovery" onClick={handleReconnect}>Retry</Button> : null}
                  </>
                ) : isSshExpired ? (
                  <p className="min-w-0 truncate text-[12px] text-muted-foreground">
                    The remote process has exited or is no longer available on the host.
                  </p>
                ) : isSshLegacyLost ? (
                  <p className="min-w-0 truncate text-[12px] text-muted-foreground">
                    This session was started before process-preserving reconnection was supported and cannot be restored.
                  </p>
                ) : (
                  <>
                    <p className="min-w-0 truncate text-[12px] text-muted-foreground">
                      Remote connection lost. Retry to reattach to the running session.
                    </p>
                    <Button
                      type="button"
                      size="sm"
                      variant="secondary"
                      disabled={isPending || !onReconnect}
                      aria-busy={isPending}
                      aria-label={isPending ? "Reconnecting SSH" : "Reconnect SSH"}
                      onClick={handleReconnect}
                      className="h-7 shrink-0 text-[11px]"
                    >
                      {isPending ? <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" /> : null}
                      <span>{isPending ? "Reconnecting..." : "Reconnect SSH"}</span>
                    </Button>
                  </>
                )}
              </div>
            ) : isAgentSession ? (
              <div className="ml-auto flex min-w-0 max-w-full flex-wrap items-center justify-end gap-2">
                {isRecoveryBlocked ? (
                  <>
                    <p role="status" className="min-w-0 truncate text-[12px] text-muted-foreground">{recoveryState?.reason ?? "Recovery is blocked."}</p>
                    <div className="flex shrink-0 items-center gap-2">
                      {canRetryRecovery ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          disabled={isPending}
                          aria-busy={isPending}
                          aria-label="Retry session recovery"
                          onClick={handleReconnect}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <RefreshCw className="size-3.5" aria-hidden="true" />
                          <span>Retry</span>
                        </Button>
                      ) : null}
                      {onOpenNewShell ? (
                        <Button
                          type="button"
                          size="sm"
                          variant={canRetryRecovery ? "ghost" : "secondary"}
                          disabled={isPending}
                          aria-label="Open new shell"
                          onClick={handleOpenNewShell}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <span>Open new shell</span>
                        </Button>
                      ) : null}
                    </div>
                  </>
                ) : isLocalReconnecting ? (
                  <>
                    <div role="status" className="flex min-w-0 items-center gap-2 truncate text-[12px] text-muted-foreground">
                      <Loader2 className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
                      <span>{recoveryState?.reason ?? "Reconnecting session..."}</span>
                    </div>
                    {canRetryRecovery ? <Button type="button" size="sm" variant="secondary" className="h-7 shrink-0 text-[11px]" aria-label="Retry session recovery" onClick={handleReconnect}>Retry</Button> : null}
                    {onOpenNewShell ? (
                      <Button
                        type="button"
                        size="sm"
                        variant={canRetryRecovery ? "ghost" : "secondary"}
                        aria-label="Open new shell"
                        onClick={handleOpenNewShell}
                        className="h-7 shrink-0 text-[11px]"
                      >
                        <span>Open new shell</span>
                      </Button>
                    ) : null}
                  </>
                ) : canRetryRecovery || affordance.canReconnect ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={isPending}
                    aria-busy={isPending}
                    aria-label={
                      isPending
                        ? `Reconnecting ${agentName} session`
                        : canRetryRecovery || affordance.canRetry
                          ? `Retry ${agentName} session`
                          : `Reconnect ${agentName} session`
                    }
                    onClick={handleReconnect}
                    className="h-7 shrink-0 text-[11px]"
                  >
                    {isPending ? (
                      <>
                        <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
                        <span>Reconnecting session...</span>
                      </>
                    ) : canRetryRecovery || affordance.canRetry ? (
                      <>
                        <RefreshCw className="size-3.5" aria-hidden="true" />
                        <span>Retry</span>
                      </>
                    ) : (
                      <span>Reconnect</span>
                    )}
                  </Button>
                ) : canReattach ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={isPending}
                    aria-busy={isPending}
                    aria-label={isPending ? `Reconnecting ${agentName} session` : `Reconnect ${agentName} session`}
                    onClick={handleReconnect}
                    className="h-7 shrink-0 text-[11px]"
                  >
                    {isPending ? (
                      <>
                        <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
                        <span>Reconnecting session...</span>
                      </>
                    ) : (
                      <>
                        <RefreshCw className="size-3.5" aria-hidden="true" />
                        <span>Reconnect</span>
                      </>
                    )}
                  </Button>
                ) : isPending ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={true}
                    aria-busy={true}
                    aria-label={`Reconnecting ${agentName} session`}
                    className="h-7 shrink-0 text-[11px]"
                  >
                    <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
                    <span>Reconnecting session...</span>
                  </Button>
                ) : onOpenNewShell ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={isPending}
                    aria-label="Open new shell"
                    onClick={handleOpenNewShell}
                    className="h-7 shrink-0 text-[11px]"
                  >
                    <span>Open new shell</span>
                  </Button>
                ) : null}
              </div>
            ) : (
              <div className="ml-auto flex min-w-0 max-w-full flex-wrap items-center justify-end gap-2">
                {isRecoveryBlocked ? (
                  <>
                    <p role="status" className="min-w-0 truncate text-[12px] text-muted-foreground">{recoveryState?.reason ?? "Recovery is blocked."}</p>
                    <div className="flex shrink-0 items-center gap-2">
                      {canRetryRecovery ? (
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          disabled={isPending}
                          aria-busy={isPending}
                          aria-label="Retry session recovery"
                          onClick={handleReconnect}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <RefreshCw className="size-3.5" aria-hidden="true" />
                          <span>Retry</span>
                        </Button>
                      ) : null}
                      {onOpenNewShell ? (
                        <Button
                          type="button"
                          size="sm"
                          variant={canRetryRecovery ? "ghost" : "secondary"}
                          disabled={isPending}
                          aria-label="Open new shell"
                          onClick={handleOpenNewShell}
                          className="h-7 shrink-0 text-[11px]"
                        >
                          <span>Open new shell</span>
                        </Button>
                      ) : null}
                    </div>
                  </>
                ) : isLocalReconnecting ? (
                  <>
                    <div role="status" className="flex min-w-0 items-center gap-2 truncate text-[12px] text-muted-foreground">
                      <Loader2 className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
                      <span>{recoveryState?.reason ?? "Reconnecting to session..."}</span>
                    </div>
                    {canRetryRecovery ? <Button type="button" size="sm" variant="secondary" className="h-7 shrink-0 text-[11px]" aria-label="Retry session recovery" onClick={handleReconnect}>Retry</Button> : null}
                    {onOpenNewShell ? (
                      <Button
                        type="button"
                        size="sm"
                        variant={canRetryRecovery ? "ghost" : "secondary"}
                        aria-label="Open new shell"
                        onClick={handleOpenNewShell}
                        className="h-7 shrink-0 text-[11px]"
                      >
                        <span>Open new shell</span>
                      </Button>
                    ) : null}
                  </>
                ) : isStandbyShell && isPending ? (
                  <div className="flex min-w-0 items-center gap-2 truncate text-[12px] text-muted-foreground">
                    <Loader2 className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
                    <span>Starting a new shell in this directory...</span>
                  </div>
                ) : (
                  <>
                    {canReattach || canRetryRecovery ? (
                      <Button
                        type="button"
                        size="sm"
                        variant="secondary"
                        disabled={isPending}
                        aria-busy={isPending}
                        aria-label={isPending ? "Reconnecting shell" : canRetryRecovery ? "Retry session recovery" : "Reconnect shell"}
                        onClick={handleReconnect}
                        className="h-7 shrink-0 text-[11px]"
                      >
                        {isPending ? (
                          <>
                            <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" />
                            <span>Reconnecting...</span>
                          </>
                        ) : (
                          <>
                            <RefreshCw className="size-3.5" aria-hidden="true" />
                            <span>{canRetryRecovery ? "Retry" : "Reconnect shell"}</span>
                          </>
                        )}
                      </Button>
                    ) : null}
                    <Button
                      type="button"
                      size="sm"
                      variant={canReattach || canRetryRecovery ? "ghost" : "secondary"}
                      disabled={isPending || !onOpenNewShell}
                      aria-label={isStandbyShell ? "Start shell" : "Open new shell"}
                      onClick={handleOpenNewShell}
                      className="h-7 shrink-0 text-[11px]"
                    >
                      <span>{isStandbyShell ? "Start shell" : "Open new shell"}</span>
                    </Button>
                  </>
                )}
              </div>
            )}
            {isPending || isTransientOutage || !statusBarHasAction ? null : (
              <kbd aria-hidden="true" className="hidden shrink-0 rounded border border-border px-1 font-mono text-[10px] text-muted-foreground sm:inline">
                Enter
              </kbd>
            )}
            <ContinueInNewPaneButtons
              modes={continuationModes}
              onContinue={onContinueInNewPane}
              held={continuationHeld}
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}
