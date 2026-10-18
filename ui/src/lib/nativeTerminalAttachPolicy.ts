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

const SESSION_AUTHORITY_SOURCES = new Set([
  "daemon_attach",
  "surface_host",
  "session_service",
  "remote_gateway",
  "daemon",
]);

function isPresentationError(details: unknown): boolean {
  if (typeof details !== "object" || details === null || Array.isArray(details)) {
    return false;
  }
  const det = details as Record<string, unknown>;
  return (
    det.stage === "presentation" ||
    det.kind === "presentation_error" ||
    det.kind === "presentation_failure" ||
    det.source === "presentation" ||
    det.source === "wgpu"
  );
}

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
  if (isPresentationError(det)) {
    return true;
  }
  if ("source" in det && typeof det.source === "string" && !SESSION_AUTHORITY_SOURCES.has(det.source)) {
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
  expectedIncarnation?: string | null,
  expectedEpoch?: string | null,
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
        const isAuthoritySource = typeof det.source === "string" && SESSION_AUTHORITY_SOURCES.has(det.source);
        const hasFullTypeInfo =
          isAuthoritySource &&
          det.kind === "session_not_found" &&
          det.sessionId === requestedSessionId;

        if (hasFullTypeInfo) {
          const authority = det.recoveryAuthority as Record<string, unknown> | undefined;
          const authorityIncarnation = typeof authority?.incarnation === "string" ? authority.incarnation : null;
          const directIncarnation = typeof det.incarnation === "string" ? det.incarnation : null;
          const observedIncarnation = authorityIncarnation ?? directIncarnation;

          const authorityEpoch = typeof authority?.daemonEpoch === "string" ? authority.daemonEpoch : null;
          const directEpoch = typeof det.daemonEpoch === "string" ? det.daemonEpoch : null;
          const observedEpoch = authorityEpoch ?? directEpoch;

          const isAffirmativeStatus =
            det.affirmativeExit === true ||
            (typeof authority === "object" && authority !== null && authority.status === "affirmative_exit");

          // Affirmative exit proof requires explicit affirmative status AND verified matching incarnation context AND matching epoch context (when provided).
          // When expectedIncarnation is omitted/unspecified, absence of proven matching context MUST remain unverified (fail-closed, INV-1).
          const matchesExpectedIncarnation = Boolean(
            expectedIncarnation && observedIncarnation && observedIncarnation === expectedIncarnation
          );
          const matchesExpectedEpoch = expectedEpoch
            ? Boolean(observedEpoch && observedEpoch === expectedEpoch)
            : true;
          const matchesExpectedContext = matchesExpectedIncarnation && matchesExpectedEpoch;

          if (isAffirmativeStatus && matchesExpectedContext) {
            return {
              status: "confirmed-missing",
              sessionId: requestedSessionId,
              reason: "daemon-attach-affirmative-exit",
            };
          }
          // Scoped attach absence without affirmative exit proof is unconfirmed uncertainty (INV-1)
          return {
            status: "unverified-missing",
            reason: det.source === "daemon_attach" ? "daemon-attach-not-found" : "authority-session-not-found",
          };
        }
      }

      // P07: Literal-string fallback is dropped when structured code SESSION_NOT_FOUND is present.
      // Classification is driven strictly by code + typed details.
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

      // Backward tolerance: legacy daemons without structured error responses
      // previously emitted INTERNAL_ERROR with prose messages.
      // Under consistent recovery (INV-1), legacy prose without affirmative proof
      // must NOT be classified as process death; it is unverified-missing.
      const legacyExpectedMessage = `Session '${requestedSessionId}' not found`;
      const ptyExpectedMessage = `PTY session '${requestedSessionId}' not found`;
      if (message === legacyExpectedMessage || message === ptyExpectedMessage) {
        return {
          status: "unverified-missing",
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

  // Backward tolerance: legacy callers or raw strings where no structured error code exists yet.
  if (typeof error === "string") {
    const expectedMessage = `Session '${requestedSessionId}' not found`;
    const ptyExpectedMessage = `PTY session '${requestedSessionId}' not found`;
    if (error === expectedMessage || error === ptyExpectedMessage) {
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
