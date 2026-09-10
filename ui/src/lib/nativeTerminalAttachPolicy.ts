import { isStructuredIpcError } from "./tauri";

export type NativeTerminalAttachClassification =
  | {
      readonly status: "confirmed-missing";
      readonly sessionId: string;
      readonly reason: string;
    }
  | {
      readonly status: "unverified-missing";
      readonly reason: string;
    }
  | {
      readonly status: "operational-error";
      readonly error: unknown;
    };

function hasContradictoryMetadata(
  details: unknown,
  requestedSessionId: string,
): boolean {
  if (typeof details !== "object" || details === null || Array.isArray(details)) {
    return false;
  }
  const det = details as Record<string, unknown>;
  if ("sessionId" in det && det.sessionId !== requestedSessionId) {
    return true;
  }
  if ("source" in det && det.source !== "daemon_attach") {
    return true;
  }
  if ("kind" in det && det.kind !== "session_not_found") {
    return true;
  }
  return false;
}

export function classifyNativeTerminalAttachError(
  error: unknown,
  requestedSessionId: string,
): NativeTerminalAttachClassification {
  if (isStructuredIpcError(error)) {
    const code = error.code;
    const message = error.message ?? "";
    const details = error.details;

    if (code === "SESSION_NOT_FOUND") {
      if (hasContradictoryMetadata(details, requestedSessionId)) {
        return {
          status: "operational-error",
          error,
        };
      }

      if (
        typeof details === "object" &&
        details !== null &&
        !Array.isArray(details)
      ) {
        const det = details as Record<string, unknown>;
        const hasFullTypeInfo =
          det.source === "daemon_attach" &&
          det.kind === "session_not_found" &&
          det.sessionId === requestedSessionId;

        if (hasFullTypeInfo) {
          return {
            status: "confirmed-missing",
            sessionId: requestedSessionId,
            reason: "daemon-attach-not-found",
          };
        }
      }

      const legacyExpectedMessage = `Session '${requestedSessionId}' not found`;
      if (message === legacyExpectedMessage) {
        return {
          status: "confirmed-missing",
          sessionId: requestedSessionId,
          reason: "legacy-message-match",
        };
      }

      return {
        status: "unverified-missing",
        reason: "unclear-origin",
      };
    }

    if (code === "INTERNAL_ERROR") {
      if (hasContradictoryMetadata(details, requestedSessionId)) {
        return {
          status: "operational-error",
          error,
        };
      }

      const legacyExpectedMessage = `Session '${requestedSessionId}' not found`;
      if (message === legacyExpectedMessage) {
        return {
          status: "confirmed-missing",
          sessionId: requestedSessionId,
          reason: "legacy-internal-error",
        };
      }

      return {
        status: "operational-error",
        error,
      };
    }

    return {
      status: "operational-error",
      error,
    };
  }

  if (typeof error === "string") {
    const expectedMessage = `Session '${requestedSessionId}' not found`;
    if (error === expectedMessage) {
      return {
        status: "unverified-missing",
        reason: "raw-string-match",
      };
    }
  }

  return {
    status: "operational-error",
    error,
  };
}
