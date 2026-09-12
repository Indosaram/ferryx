import { Loader2, RefreshCw, TerminalSquare } from "lucide-react";
import { useId, useState } from "react";
import type { TerminalActivity } from "../lib/activity";
import { isMonochromeAgentLogo, resolveAgentLogo } from "../lib/agentIcon";
import { getAgentReconnectAffordance } from "../lib/agentResumeAffordance";
import { agentDisplayNameForType } from "../lib/agentTitle";
import { Button } from "./ui/button";
import { cn } from "../lib/cn";
import { isRemoteWorkspaceId } from "../lib/remoteProject";
import { toIpcError } from "../lib/tauri";
import type { TerminalSession } from "../lib/types";
import { NativeTerminalPane } from "./NativeTerminalPane";
import { TerminalSearchOverlay } from "./TerminalSearchOverlay";
import { DagPaneBadge } from "./dag/DagPaneBadge";

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

  const isSshSession = isRemoteWorkspaceId(session.workspaceId);
  const isSpawning = session.reconnectLifecycle === "spawning" || session.reconnectLifecycle === "validating";
  const remoteState = session.remoteConnectionState;
  const isSshReconnecting = isSshSession && (remoteState === "reconnecting" || isSpawning);
  const isSshDisconnected =
    isSshSession && !isSpawning && (remoteState === "disconnected" || (!remoteState && (session.backendSessionId === null || session.lifecycle === "exited")));
  const isSshExpired = isSshSession && !isSpawning && (remoteState === "expired" || remoteState === "missing");
  const isSshLegacyLost = isSshSession && !isSpawning && remoteState === "legacyLost";
  const showSshOverlay = isSshSession && (isSshReconnecting || isSshDisconnected || isSshExpired || isSshLegacyLost);
  const isExited = isSshSession ? showSshOverlay : session.backendSessionId === null || session.lifecycle === "exited";
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

  return (
    <div
      data-testid="terminal-pane-surface"
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
