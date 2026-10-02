import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";

import "../../ui/src/index.css";
import { TerminalPane } from "../../ui/src/components/TerminalPane";
import { reconnectSshSession, type RetryRemoteSessionResult } from "../../ui/src/lib/sshRebootRecovery";
import { toIpcError } from "./ssh-reboot-recovery-tauri-mock";
import type { RemoteFailure, TerminalSession } from "../../ui/src/lib/types";

export type QaScenarioName =
  | "expired_recovery_success"
  | "expired_recovery_failure"
  | "unavailable_record_safety";

export interface QaCallRecord {
  timestamp: number;
  type: "reconnect" | "openNewShell";
  sessionId: string;
}

export interface QaDeferredSignal {
  promise: Promise<RetryRemoteSessionResult>;
  resolve: () => void;
  reject: (err: unknown) => void;
}

export interface QaHarnessState {
  scenario: QaScenarioName;
  session: TerminalSession;
  isPending: boolean;
  recovered: boolean;
  failed: boolean;
  lastError: unknown;
  reconnectCallCount: number;
  openShellCallCount: number;
  callHistory: QaCallRecord[];
}

declare global {
  interface Window {
    __FERRYX_REBOOT_QA__?: {
      getScenario: () => QaScenarioName;
      setScenario: (scenario: QaScenarioName) => void;
      getState: () => QaHarnessState;
      resolveRecovery: () => void;
      rejectRecovery: (error?: { code?: string; message?: string }) => void;
      reset: () => void;
    };
  }
}

export const FIXTURE_EXPIRED_AGENT_SESSION: TerminalSession = {
  id: "pane-ssh-reboot-1",
  workspaceId: "ssh:omarchy:omo-native-rs",
  cwd: "/home/indo/projects/omo-native-rs",
  worktreePath: "/home/indo/projects/omo-native-rs",
  worktree: null,
  backendSessionId: "backend-ssh-stable-42",
  lifecycle: "exited",
  reconnectLifecycle: "idle",
  remoteConnectionState: "expired",
  remoteGeneration: 1,
  remoteFailure: {
    kind: "expired",
    message: "Remote process not found on host after machine reboot",
  },
  agentType: "omo",
  agentSessionId: "01a0f117-dfe2-7de2-8c71-3343ae3ab192",
  providerSession: {
    key: "session_id",
    id: "01a0f117-dfe2-7de2-8c71-3343ae3ab192",
  },
  daemonEpoch: "epoch-100",
  lastOutputSequence: "1024",
};

export const FIXTURE_UNAVAILABLE_RECORD_SESSION: TerminalSession = {
  id: "pane-ssh-prefeature",
  workspaceId: "ssh:omarchy:omo-native-rs",
  cwd: "/home/indo/projects/omo-native-rs",
  worktreePath: "/home/indo/projects/omo-native-rs",
  worktree: null,
  backendSessionId: null,
  lifecycle: "exited",
  reconnectLifecycle: "idle",
  remoteConnectionState: "expired",
  remoteGeneration: 1,
  remoteFailure: {
    kind: "expired",
    message: "The remote process has exited or is no longer available on the host.",
  },
  agentType: null,
  agentSessionId: null,
  providerSession: null,
};

function initialSessionForScenario(scenario: QaScenarioName): TerminalSession {
  switch (scenario) {
    case "expired_recovery_success":
    case "expired_recovery_failure":
      return { ...FIXTURE_EXPIRED_AGENT_SESSION };
    case "unavailable_record_safety":
      return { ...FIXTURE_UNAVAILABLE_RECORD_SESSION };
  }
}

export function SshRebootRecoveryHarness(): JSX.Element {
  const queryParams = new URLSearchParams(window.location.search);
  const initialScenario = (queryParams.get("scenario") as QaScenarioName) || "expired_recovery_success";

  const [scenario, setScenario] = useState<QaScenarioName>(initialScenario);
  const [session, setSession] = useState<TerminalSession>(() => initialSessionForScenario(initialScenario));
  const [isPending, setIsPending] = useState(false);
  const [recovered, setRecovered] = useState(false);
  const [failed, setFailed] = useState(false);
  const [lastError, setLastError] = useState<unknown>(null);
  const [reconnectCallCount, setReconnectCallCount] = useState(0);
  const [openShellCallCount, setOpenShellCallCount] = useState(0);
  const [callHistory, setCallHistory] = useState<QaCallRecord[]>([]);

  const deferredRef = useRef<QaDeferredSignal | null>(null);

  const resetState = useCallback((targetScenario: QaScenarioName) => {
    setScenario(targetScenario);
    setSession(initialSessionForScenario(targetScenario));
    setIsPending(false);
    setRecovered(false);
    setFailed(false);
    setLastError(null);
    setReconnectCallCount(0);
    setOpenShellCallCount(0);
    setCallHistory([]);
    deferredRef.current = null;
  }, []);

  const handleReconnect = useCallback((sessionId: string): Promise<void> => {
    setReconnectCallCount((c) => c + 1);
    setCallHistory((h) => [...h, { timestamp: Date.now(), type: "reconnect", sessionId }]);

    return reconnectSshSession(sessionId, {
      getSessions: () => ({ [session.id]: session }),
      retryRemoteSession: async (_backendSessionId: string) => {
        setIsPending(true);
        let res!: (val: RetryRemoteSessionResult) => void;
        let rej!: (err: unknown) => void;
        const promise = new Promise<RetryRemoteSessionResult>((resolve, reject) => {
          res = resolve;
          rej = reject;
        });

        deferredRef.current = {
          promise,
          resolve: () => {
            res({ type: "retryRemoteSessionOk" });
          },
          reject: (err) => {
            const failure: RemoteFailure = {
              kind: "expired",
              message: typeof err === "string" ? err : ((err as Record<string, unknown>)?.message ? String((err as Record<string, unknown>).message) : "Remote process not found on host after machine reboot"),
            };
            res({ type: "remoteSessionError", failure });
          },
        };

        return promise;
      },
      toIpcError,
      reportRuntimeError: (err) => {
        setLastError(err);
      },
    }).then(() => {
      setSession((prev) => ({
        ...prev,
        remoteConnectionState: "connected",
        remoteFailure: null,
        lifecycle: "running",
      }));
      setIsPending(false);
      setRecovered(true);
      setFailed(false);
    }).catch((err) => {
      setIsPending(false);
      setFailed(true);
      setLastError(err);
      throw err;
    });
  }, [session]);

  const handleOpenNewShell = useCallback((sessionId: string) => {
    setOpenShellCallCount((c) => c + 1);
    setCallHistory((h) => [...h, { timestamp: Date.now(), type: "openNewShell", sessionId }]);
  }, []);

  const resolveRecovery = useCallback(() => {
    if (deferredRef.current) {
      deferredRef.current.resolve();
      deferredRef.current = null;
    }
  }, []);

  const rejectRecovery = useCallback((error?: { code?: string; message?: string }) => {
    if (deferredRef.current) {
      deferredRef.current.reject(error);
      deferredRef.current = null;
    }
  }, []);

  useEffect(() => {
    window.__FERRYX_REBOOT_QA__ = {
      getScenario: () => scenario,
      setScenario: (sc: QaScenarioName) => resetState(sc),
      getState: () => ({
        scenario,
        session,
        isPending,
        recovered,
        failed,
        lastError,
        reconnectCallCount,
        openShellCallCount,
        callHistory,
      }),
      resolveRecovery,
      rejectRecovery,
      reset: () => resetState(scenario),
    };
    return () => {
      delete window.__FERRYX_REBOOT_QA__;
    };
  }, [callHistory, failed, isPending, lastError, openShellCallCount, reconnectCallCount, recovered, rejectRecovery, resetState, resolveRecovery, scenario, session]);

  return (
    <div className="flex h-screen w-screen flex-col bg-background font-sans text-foreground antialiased select-none">
      <div data-testid="qa-reconnect-count" data-count={reconnectCallCount} className="hidden" />
      <div data-testid="qa-shell-count" data-count={openShellCallCount} className="hidden" />
      {isPending ? <div data-testid="qa-signal-pending" className="hidden" /> : null}
      {recovered ? <div data-testid="qa-signal-recovered" className="hidden" /> : null}
      {failed ? <div data-testid="qa-signal-failed" className="hidden" /> : null}

      <div
        data-testid="qa-debug-toolbar"
        className="flex h-6 shrink-0 items-center justify-between border-b border-border bg-card/95 px-2 text-[10px] text-muted-foreground select-none"
      >
        <div className="flex items-center gap-1.5 overflow-hidden">
          <span className="font-semibold text-foreground shrink-0">QA:</span>
          <div
            data-testid="qa-harness-ready"
            data-scenario={scenario}
            data-pending={String(isPending)}
            data-recovered={String(recovered)}
            data-failed={String(failed)}
            data-session-id={session.id}
            data-backend-id={session.backendSessionId ?? "none"}
            className="inline-flex items-center gap-1 rounded bg-muted px-1.5 py-0.5 font-mono text-[10px] text-foreground shrink-0"
          >
            <span className="size-1.5 rounded-full bg-emerald-400" />
            <span>{scenario}</span>
          </div>
          <span className="truncate text-muted-foreground/80">R({reconnectCallCount}) S({openShellCallCount})</span>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          <button
            type="button"
            data-testid="qa-btn-scenario-success"
            onClick={() => resetState("expired_recovery_success")}
            className="rounded bg-muted px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            Success
          </button>
          <button
            type="button"
            data-testid="qa-btn-scenario-failure"
            onClick={() => resetState("expired_recovery_failure")}
            className="rounded bg-muted px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            Failure
          </button>
          <button
            type="button"
            data-testid="qa-btn-scenario-unavailable"
            onClick={() => resetState("unavailable_record_safety")}
            className="rounded bg-muted px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            Unavailable
          </button>
          {isPending ? (
            <>
              <button
                type="button"
                data-testid="qa-btn-trigger-resolve"
                onClick={resolveRecovery}
                className="rounded bg-emerald-700 px-1.5 py-0.5 text-white hover:bg-emerald-600"
              >
                Resolve
              </button>
              <button
                type="button"
                data-testid="qa-btn-trigger-reject"
                onClick={() => rejectRecovery()}
                className="rounded bg-rose-700 px-1.5 py-0.5 text-white hover:bg-rose-600"
              >
                Reject
              </button>
            </>
          ) : null}
        </div>
      </div>

      <div data-testid="qa-pane-viewport" className="relative flex-1 min-h-0 w-full overflow-hidden">
        <TerminalPane
          session={session}
          sessions={{ [session.id]: session }}
          active={true}
          onReconnect={handleReconnect}
          onOpenNewShell={handleOpenNewShell}
        />
      </div>
    </div>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  ReactDOM.createRoot(rootEl).render(<SshRebootRecoveryHarness />);
}
