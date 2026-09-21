import { Loader2, RefreshCw, TerminalSquare } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import type { TerminalActivity } from "../lib/activity";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";
import { getAgentReconnectAffordance } from "../lib/agentResumeAffordance";
import { agentDisplayNameForType } from "../lib/agentTitle";
import { Button } from "./ui/button";
import { cn } from "../lib/cn";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import {
  isSessionAutoResumeHeld,
  isSessionSleeping,
  isStandbyBackendSessionId,
  registerSessionSnapshot,
  resumeRegisteredSession,
  setSessionActive,
  setSessionSleeping,
  useSleepingSessionIds,
} from "../lib/sessionLifecycle";
import { toIpcError } from "../lib/tauri";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane } from "./NativeTerminalPane";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";
import { DagPaneBadge } from "./dag/DagPaneBadge";
import { remoteHostStore } from "../state/remoteHostStore";

type TerminalPaneProps = {
  session: TerminalSession;
  sessions?: Readonly<Record<string, TerminalSession>> | readonly TerminalSession[];
  active: boolean;
  activity?: TerminalActivity;
  needsAttention?: boolean;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
  onReconnect?: (sessionId: string) => Promise<void> | void;
  onOpenNewShell?: (sessionId: string) => Promise<void> | void;
  onBackendSessionUnavailable?: (
    sessionId: string,
    backendSessionId: string,
    reason: string,
    bindingKey?: string | null,
  ) => void;
};

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
  activity,
  needsAttention,
  searchOpen,
  onCloseSearch,
  onReconnect,
  onOpenNewShell,
  onBackendSessionUnavailable,
}: TerminalPaneProps) {
  const [pendingLocal, setPendingLocal] = useState(false);
  const [replacementError, setReplacementError] = useState<string | null>(null);
  const titleId = useId();
  const descId = useId();
  const autoResumeKeyRef = useRef<string | null>(null);
  const sleepingSessionIds = useSleepingSessionIds();

  const isSshSession = isRemoteWorkspaceId(session.workspaceId);
  const isSpawning = session.reconnectLifecycle === "spawning" || session.reconnectLifecycle === "validating";
  const remoteState = session.remoteConnectionState;
  const isSshReconnecting = isSshSession && (remoteState === "reconnecting" || isSpawning);
  // `missing` means the daemon we queried does not know this session (a draining predecessor may
  // still own it, or restore has not completed). That is a recoverable transport-level outage, so
  // it takes the same recoverable branch as `disconnected` and never the terminal expired branch.
  const isSshDisconnected =
    isSshSession && !isSpawning && (remoteState === "disconnected" || remoteState === "missing" || (!remoteState && (session.backendSessionId === null || session.lifecycle === "exited")));
  const isSshExpired = isSshSession && !isSpawning && remoteState === "expired";
  const isSshLegacyLost = isSshSession && !isSpawning && remoteState === "legacyLost";
  const showSshOverlay = isSshSession && (isSshReconnecting || isSshDisconnected || isSshExpired || isSshLegacyLost);
  const isExited = isSshSession ? showSshOverlay : session.backendSessionId === null || isStandbyBackendSessionId(session.backendSessionId) || session.lifecycle === "exited";
  const isSuspended =
    !isExited && (sleepingSessionIds.has(session.id) || session.processState === "suspended");
  const affordance = getAgentReconnectAffordance(session, sessions);
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

  const isPending = pendingLocal || isSpawning || (isSshSession ? isSshReconnecting : affordance.isReconnecting);
  const errorDescription =
    replacementError ?? (isSshSession ? session.remoteFailure?.message ?? null : resolveAffordanceErrorDescription(affordance));

  const handleReconnect = async () => {
    if (isPending) return;
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
    if (!affordance.canReconnect || !onReconnect) return;
    setPendingLocal(true);
    try {
      await onReconnect(session.id);
    } finally {
      setPendingLocal(false);
    }
  };

  const handleOpenNewShell = async () => {
    if (isPending || !onOpenNewShell) return;
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
    if (active && isSuspended && !isSessionAutoResumeHeld(session.id)) {
      void resumeRegisteredSession(session.id).catch((error) => {
        console.warn("Failed to auto-resume active suspended session:", error);
      });
    }
  }, [active, isSuspended, session.id]);

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
    if (!isExited || isPending || isSshSession || !isSessionSleeping(session.id) || isSessionAutoResumeHeld(session.id)) return;
    const key = `${session.id}:${session.backendSessionId ?? "none"}:${session.reconnectLifecycle ?? "idle"}`;
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
  }, [active, affordance.canReconnect, isAgentSession, isExited, isPending, isSshSession, onOpenNewShell, onReconnect, session.backendSessionId, session.id, session.reconnectLifecycle]);

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
      onClick={isSuspended ? handleResume : undefined}
      className="relative h-full w-full min-h-0 min-w-0 overflow-hidden"
    >
      <DagPaneBadge
        projectPath={session.worktreePath ?? session.cwd}
        paneId={session.id}
        providerSessionId={session.providerSession?.id ?? null}
        sessions={sessions}
        agentPresent={activity?.isAgent === true}
        agentWorking={activity?.isAgent === true && activity.state === "working"}
      />
      <NativeTerminalPane
        sessionId={session.id}
        session={session}
        active={active}
        activity={activity}
        needsAttention={needsAttention}
        onBackendSessionUnavailable={(backendSessionId, reason, bindingKey) => {
          onBackendSessionUnavailable?.(session.id, backendSessionId, reason, bindingKey);
        }}
      />
      {searchOpen ? (
        <TerminalSearchOverlay
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
          className="absolute inset-0 z-20 flex cursor-pointer items-center justify-center bg-background/85 px-6 text-center select-none"
        >
          <div className="flex max-w-sm flex-col items-center rounded-lg border border-border bg-card p-5 shadow-lg">
            <div className="mb-3 flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/60">
              <span className="text-lg">💤</span>
            </div>
            <h2 className="text-sm font-medium text-foreground">Suspended</h2>
            <p className="mt-1 text-xs text-muted-foreground">Click to resume</p>
          </div>
        </div>
      ) : null}
      {isExited ? (
        <div
          role="region"
          aria-labelledby={titleId}
          aria-describedby={errorDescription ? descId : undefined}
          data-testid="terminal-pane-overlay"
          className="absolute inset-0 z-20 flex items-center justify-center bg-background/85 px-6 text-center"
        >
          <div className="flex max-w-sm flex-col items-center rounded-lg border border-border bg-card p-5 shadow-lg">
            {isAgentSession ? (
              <div className="mb-3 flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/60">
                {logo ? (
                  <img
                    src={logo}
                    alt=""
                    className={cn("size-5", isMonochrome && "agent-tab-logo--monochrome")}
                  />
                ) : (
                  <TerminalSquare className="size-5 text-muted-foreground" aria-hidden="true" />
                )}
              </div>
            ) : (
              <div className="mb-3 flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-muted/60">
                <TerminalSquare className="size-5 text-muted-foreground" aria-hidden="true" />
              </div>
            )}

            <h2 id={titleId} className="text-sm font-medium text-foreground">
              {isSshSession
                ? isSshReconnecting
                  ? "Reconnecting SSH..."
                  : isSshExpired
                    ? "Remote session expired"
                    : isSshLegacyLost
                      ? "Legacy SSH session lost"
                      : "SSH disconnected"
                : isAgentSession
                  ? "Session disconnected"
                  : "Shell exited"}
            </h2>

            {isAgentSession ? (
              <p className="mt-1 text-xs text-muted-foreground">{agentName}</p>
            ) : null}

            {errorDescription ? (
              <p id={descId} role={replacementError ? "alert" : undefined} className="mt-1 break-words text-xs text-muted-foreground">
                {errorDescription}
              </p>
            ) : null}

            {isSshSession ? (
              <div className="mt-4 flex w-full flex-col items-center gap-2">
                {isSshReconnecting ? (
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <Loader2 className="size-4 animate-spin text-muted-foreground motion-reduce:animate-none" aria-hidden="true" />
                    <span>Reconnecting to remote session...</span>
                  </div>
                ) : isSshExpired ? (
                  <p className="text-xs text-muted-foreground">
                    The remote process has exited or is no longer available on the host.
                  </p>
                ) : isSshLegacyLost ? (
                  <p className="text-xs text-muted-foreground">
                    This session was started before process-preserving reconnection was supported and cannot be restored.
                  </p>
                ) : (
                  <>
                    <p className="text-xs text-muted-foreground">
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
                      className="w-full max-w-[220px]"
                    >
                      {isPending ? <Loader2 className="size-3.5 animate-spin motion-reduce:animate-none" aria-hidden="true" /> : null}
                      <span>{isPending ? "Reconnecting..." : "Reconnect SSH"}</span>
                    </Button>
                  </>
                )}
              </div>
            ) : isAgentSession ? (
              <div className="mt-4 flex w-full flex-col items-center gap-2">
                {affordance.canReconnect ? (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={isPending}
                    aria-busy={isPending}
                    aria-label={
                      isPending
                        ? `Reconnecting ${agentName} session`
                        : affordance.canRetry
                          ? `Retry ${agentName} session`
                          : `Reconnect ${agentName} session`
                    }
                    onClick={handleReconnect}
                    className="w-full max-w-[220px]"
                  >
                    {isPending ? (
                      <>
                        <Loader2 className="size-3.5 animate-spin" aria-hidden="true" />
                        <span>Reconnecting session...</span>
                      </>
                    ) : affordance.canRetry ? (
                      <>
                        <RefreshCw className="size-3.5" aria-hidden="true" />
                        <span>Retry</span>
                      </>
                    ) : (
                      <span>Reconnect</span>
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
                    className="w-full max-w-[220px]"
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
                    className="w-full max-w-[220px]"
                  >
                    <span>Open new shell</span>
                  </Button>
                ) : null}
              </div>
            ) : (
              <div className="mt-4 flex w-full flex-col items-center">
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  disabled={isPending || !onOpenNewShell}
                  aria-label="Open new shell"
                  onClick={handleOpenNewShell}
                  className="w-full max-w-[220px]"
                >
                  <span>Open new shell</span>
                </Button>
              </div>
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
