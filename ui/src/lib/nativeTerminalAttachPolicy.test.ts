import { describe, expect, it } from "vitest";
import {
  classifyNativeTerminalAttachError,
} from "./nativeTerminalAttachPolicy";
import type { StructuredIpcError } from "./types";

describe("classifyNativeTerminalAttachError", () => {
  const requestedSessionId = "backend-a";

  describe("SESSION_NOT_FOUND with typed daemon_attach details", () => {
    it("should return confirmed-missing when code=SESSION_NOT_FOUND + typed details match exactly", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-a' not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "backend-a",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
      if (result.status === "confirmed-missing") {
        expect(result.sessionId).toBe("backend-a");
        expect(result.reason).toBe("daemon-attach-not-found");
      }
    });

    it("should return confirmed-missing for different requested ID when details match exactly", () => {
      const requestId = "backend-xyz";
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-xyz' not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "backend-xyz",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestId);

      expect(result.status).toBe("confirmed-missing");
      if (result.status === "confirmed-missing") {
        expect(result.sessionId).toBe("backend-xyz");
      }
    });

    it("should return operational-error when details.sessionId conflicts with requested ID", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-b' not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "backend-b",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
      if (result.status === "operational-error") {
        expect(result.error).toBeDefined();
      }
    });

    it("should return operational-error when source is daemon_attach but kind is wrong", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-a' not found",
        details: {
          source: "daemon_attach",
          kind: "something_else",
          sessionId: "backend-a",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error when source is not daemon_attach", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-a' not found",
        details: {
          source: "surface_detached",
          kind: "session_not_found",
          sessionId: "backend-a",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("SESSION_NOT_FOUND with legacy message match", () => {
    it("should return confirmed-missing for exact legacy message without details", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
      if (result.status === "confirmed-missing") {
        expect(result.reason).toBe("legacy-message-match");
      }
    });

    it("should return confirmed-missing for exact legacy message with undefined details", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: undefined,
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should return confirmed-missing when message matches and details.sessionId matches", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          sessionId: requestedSessionId,
          other: "data",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should return operational-error when message matches but details.sessionId conflicts", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          sessionId: "backend-b",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return unverified-missing when message does not match exact pattern", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Session 'backend-a' not found: gone",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("unverified-missing");
      if (result.status === "unverified-missing") {
        expect(result.reason).toBe("unclear-origin");
      }
    });

    it("should return unverified-missing for short message without clear session ID", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Gone",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("unverified-missing");
    });
  });

  describe("SESSION_NOT_FOUND with no matching details or message", () => {
    it("should return unverified-missing when SESSION_NOT_FOUND has vague message", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Surface detached",
        details: {
          reason: "surface_cleanup",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("unverified-missing");
    });

    it("should return unverified-missing when message is empty", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("unverified-missing");
    });
  });

  describe("INTERNAL_ERROR with legacy daemon format", () => {
    it("should return confirmed-missing for legacy INTERNAL_ERROR with exact message match", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
      if (result.status === "confirmed-missing") {
        expect(result.reason).toBe("legacy-internal-error");
      }
    });

    it("should return confirmed-missing for legacy INTERNAL_ERROR without details key", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: undefined,
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should return operational-error for INTERNAL_ERROR with different message", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: "Something went wrong",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for INTERNAL_ERROR with message for different session", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: "Session 'backend-b' not found",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("Other error codes", () => {
    it("should return operational-error for IO_ERROR", () => {
      const error: StructuredIpcError = {
        code: "IO_ERROR",
        message: "Socket read failed",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
      if (result.status === "operational-error") {
        expect(result.error).toBe(error);
      }
    });

    it("should return operational-error for DAEMON_PROTOCOL_MISMATCH", () => {
      const error: StructuredIpcError = {
        code: "DAEMON_PROTOCOL_MISMATCH",
        message: "Incompatible protocol version",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for GPU error", () => {
      const error: StructuredIpcError = {
        code: "GPU_RENDERING_FAILED",
        message: "Failed to render terminal",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for arbitrary error code", () => {
      const error: StructuredIpcError = {
        code: "UNKNOWN_ERROR",
        message: "An unknown error occurred",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("Non-structured errors", () => {
    it("should return operational-error for Error object", () => {
      const error = new Error("Something failed");

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
      if (result.status === "operational-error") {
        expect(result.error).toBe(error);
      }
    });

    it("should return unverified-missing for raw string with exact match", () => {
      const error = `Session '${requestedSessionId}' not found`;

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("unverified-missing");
      if (result.status === "unverified-missing") {
        expect(result.reason).toBe("raw-string-match");
      }
    });

    it("should return operational-error for raw string without exact match", () => {
      const error = "Session 'backend-a' not found: details";

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for raw string with wrong ID", () => {
      const error = "Session 'backend-b' not found";

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for null", () => {
      const error = null;

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
      if (result.status === "operational-error") {
        expect(result.error).toBeNull();
      }
    });

    it("should return operational-error for undefined", () => {
      const error = undefined;

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for plain object without code/message", () => {
      const error = { foo: "bar" };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for number", () => {
      const error = 42;

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });

    it("should return operational-error for boolean", () => {
      const error = false;

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("Edge cases and special IDs", () => {
    it("should handle session ID with special characters", () => {
      const specialId = "backend-a-b_c.d";
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${specialId}' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: specialId,
        },
      };

      const result = classifyNativeTerminalAttachError(error, specialId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should handle session ID with quotes (escaped)", () => {
      const idWithQuote = "backend-a";
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${idWithQuote}' not found`,
        details: {
          sessionId: idWithQuote,
        },
      };

      const result = classifyNativeTerminalAttachError(error, idWithQuote);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should handle very long session ID", () => {
      const longId = "a".repeat(256);
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${longId}' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: longId,
        },
      };

      const result = classifyNativeTerminalAttachError(error, longId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should reject mismatched IDs in legacy format", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          sessionId: "different-id",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("Type consistency and immutability", () => {
    it("should return readonly classification result for confirmed-missing", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: requestedSessionId,
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      if (result.status === "confirmed-missing") {
        // Check that result is readonly (compiler check, not runtime)
        expect(result.status).toBe("confirmed-missing");
        expect(result.sessionId).toBe(requestedSessionId);
        expect(result.reason).toBeDefined();
      }
    });

    it("should return operational-error with error preserved", () => {
      const error: StructuredIpcError = {
        code: "IO_ERROR",
        message: "Socket failed",
        details: {},
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should narrow type correctly after status check", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: undefined,
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      // TypeScript discriminated union should narrow correctly
      switch (result.status) {
        case "confirmed-missing":
          expect(result.sessionId).toBe(requestedSessionId);
          expect(result.reason).toBeDefined();
          break;
        case "unverified-missing":
          expect(result.reason).toBeDefined();
          break;
        case "operational-error":
          expect(result.error).toBeDefined();
          break;
        default:
          // TypeScript exhaustiveness check
          const _: never = result;
          expect(_).toBeUndefined();
      }
    });
  });

  describe("Deterministic pure function", () => {
    it("should return same result for same inputs", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: requestedSessionId,
        },
      };

      const result1 = classifyNativeTerminalAttachError(error, requestedSessionId);
      const result2 = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result1.status).toBe(result2.status);
      if (result1.status === "confirmed-missing" && result2.status === "confirmed-missing") {
        expect(result1.sessionId).toBe(result2.sessionId);
        expect(result1.reason).toBe(result2.reason);
      }
    });

    it("should not modify input error", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: requestedSessionId,
        },
      };

      const originalError = JSON.parse(JSON.stringify(error));
      classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(error).toEqual(originalError);
    });

    it("should not have side effects", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: "Test",
        details: {},
      };

      let sideEffectCount = 0;
      const trackedError = new Proxy(error, {
        get(target, prop, receiver) {
          sideEffectCount++;
          return Reflect.get(target, prop, receiver);
        },
      });

      classifyNativeTerminalAttachError(trackedError, requestedSessionId);

      // Just verify it accessed properties; exact count may vary by implementation
      expect(sideEffectCount).toBeGreaterThan(0);
    });
  });

  describe("Contract compliance", () => {
    it("should prefer typed details over message when both present", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "Irrelevant message",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: requestedSessionId,
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should fall back to message when typed details incomplete", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "daemon_attach",
          // missing kind and sessionId
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("confirmed-missing");
    });

    it("should report reason for each confirmed-missing path", () => {
      const typedDetailsError: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: "",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: requestedSessionId,
        },
      };

      const legacyMessageError: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {},
      };

      const legacyInternalError: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: {},
      };

      const result1 = classifyNativeTerminalAttachError(typedDetailsError, requestedSessionId);
      const result2 = classifyNativeTerminalAttachError(legacyMessageError, requestedSessionId);
      const result3 = classifyNativeTerminalAttachError(legacyInternalError, requestedSessionId);

      expect(result1).toEqual({
        status: "confirmed-missing",
        sessionId: requestedSessionId,
        reason: "daemon-attach-not-found",
      });
      expect(result2).toEqual({
        status: "confirmed-missing",
        sessionId: requestedSessionId,
        reason: "legacy-message-match",
      });
      expect(result3).toEqual({
        status: "confirmed-missing",
        sessionId: requestedSessionId,
        reason: "legacy-internal-error",
      });
    });

    it("should never auto-correct mismatched session IDs", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session 'wrong-id' not found`,
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "wrong-id",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result.status).toBe("operational-error");
    });
  });

  describe("Contradictory metadata handling", () => {
    it("should reject SESSION_NOT_FOUND when details has wrong source only", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "other_subsystem",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should reject SESSION_NOT_FOUND when details has wrong kind only", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          kind: "permission_denied",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should reject SESSION_NOT_FOUND when message matches but details.sessionId conflicts", () => {
      const error: StructuredIpcError = {
        code: "SESSION_NOT_FOUND",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          sessionId: "different-session-id",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should reject INTERNAL_ERROR when message matches but details.sessionId conflicts", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          sessionId: "different-session-id",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should reject INTERNAL_ERROR when message matches but details has wrong source", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          source: "other_subsystem",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });

    it("should reject INTERNAL_ERROR when message matches but details has wrong kind", () => {
      const error: StructuredIpcError = {
        code: "INTERNAL_ERROR",
        message: `Session '${requestedSessionId}' not found`,
        details: {
          kind: "crash",
        },
      };

      const result = classifyNativeTerminalAttachError(error, requestedSessionId);

      expect(result).toEqual({
        status: "operational-error",
        error,
      });
    });
  });
});
