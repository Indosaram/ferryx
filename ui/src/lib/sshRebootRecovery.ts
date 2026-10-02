import type { RemoteFailure, StructuredIpcError, TerminalSession } from "./types";

export type RetryRemoteSessionResult =
  | { type: "retryRemoteSessionOk" }
  | { type: "remoteSessionError"; failure: RemoteFailure };

export type RetryRemoteSessionFn = (sessionId: string) => Promise<RetryRemoteSessionResult>;

export type SshReconnectDependencies = {
  getSessions: () => Readonly<Record<string, TerminalSession>>;
  retryRemoteSession: RetryRemoteSessionFn;
  toIpcError: (error: unknown) => StructuredIpcError;
  reportRuntimeError?: (error: unknown) => void;
};

const inFlightSshReconnects = new Map<string, Promise<void>>();

export function cannotRestoreError(): StructuredIpcError {
  return {
    code: "REMOTE_SESSION_CANNOT_RESTORE",
    message: "Remote session cannot be restored without a valid reference. Open a new shell instead.",
    details: {},
  };
}

export function reconnectSshSession(
  sessionId: string,
  dependencies: SshReconnectDependencies,
): Promise<void> {
  const existing = inFlightSshReconnects.get(sessionId);
  if (existing) return existing;

  const task = (async () => {
    const sessions = dependencies.getSessions();
    const session = sessions[sessionId];
    if (!session) {
      const notFound = dependencies.toIpcError({
        code: "SESSION_NOT_FOUND",
        message: `Terminal session ${sessionId} not found`,
        details: {},
      });
      dependencies.reportRuntimeError?.(notFound);
      throw notFound;
    }

    if (!session.backendSessionId) {
      const unrecoverable = dependencies.toIpcError(cannotRestoreError());
      dependencies.reportRuntimeError?.(unrecoverable);
      throw unrecoverable;
    }

    let res: RetryRemoteSessionResult;
    try {
      res = await dependencies.retryRemoteSession(session.backendSessionId);
    } catch (error) {
      const typed = dependencies.toIpcError(error);
      dependencies.reportRuntimeError?.(typed);
      throw typed;
    }

    if (res.type === "retryRemoteSessionOk") {
      return;
    }

    const structuredFailure: StructuredIpcError = {
      code: `REMOTE_${res.failure.kind.toUpperCase()}`,
      message: res.failure.message,
      details: { kind: res.failure.kind },
    };
    const error = dependencies.toIpcError(structuredFailure);
    dependencies.reportRuntimeError?.(error);
    throw error;
  })();

  inFlightSshReconnects.set(sessionId, task);
  void task
    .catch(() => undefined)
    .then(() => {
      if (inFlightSshReconnects.get(sessionId) === task) inFlightSshReconnects.delete(sessionId);
    });
  return task;
}

export function clearSshReconnectInflightForTests(): void {
  inFlightSshReconnects.clear();
}
