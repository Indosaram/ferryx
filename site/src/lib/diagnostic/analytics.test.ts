import { describe, expect, it } from "bun:test";
import { sendDiagnosticEvent } from "./analytics.ts";
import type { DiagnosticAnalyticsEvent } from "./analytics.ts";

describe("Diagnostic Analytics Privacy Hook", () => {
  describe("Given/When/Then: No-op behavior without configured endpoint", () => {
    it("Given no configured endpoint When sendDiagnosticEvent is called Then returns false without dispatching", () => {
      // Given: test event
      const event: DiagnosticAnalyticsEvent = {
        name: "diagnostic_start",
        lang: "ko",
      };

      // When: invoked with no endpoint
      const result = sendDiagnosticEvent(event, { endpoint: undefined });

      // Then: no-op
      expect(result).toBe(false);
    });
  });

  describe("Given/When/Then: Dispatch with configured endpoint", () => {
    it("Given a configured endpoint and mock transport When sendDiagnosticEvent is called Then dispatches payload containing only allowed privacy fields", () => {
      // Given: test event and captured payload
      const event: DiagnosticAnalyticsEvent = {
        name: "diagnostic_complete",
        resultType: "headless-persistence",
        primaryWinnerId: "herdr",
        isTie: false,
        lang: "ko",
      };

      let capturedUrl = "";
      let capturedPayload: DiagnosticAnalyticsEvent | null = null;

      const mockTransport = (url: string, payload: DiagnosticAnalyticsEvent) => {
        capturedUrl = url;
        capturedPayload = payload;
      };

      // When: invoked with custom transport
      const result = sendDiagnosticEvent(event, {
        endpoint: "https://analytics.example.com/api/event",
        transport: mockTransport,
      });

      // Then: dispatch succeeded and payload matches schema exactly
      expect(result).toBe(true);
      expect(capturedUrl).toBe("https://analytics.example.com/api/event");
      expect(capturedPayload).toEqual({
        name: "diagnostic_complete",
        resultType: "headless-persistence",
        primaryWinnerId: "herdr",
        isTie: false,
        lang: "ko",
      });

      // Ensure no private user data / full answers payload was attached
      const payloadKeys = Object.keys(capturedPayload ?? {});
      expect(payloadKeys).not.toContain("answers");
      expect(payloadKeys).not.toContain("userId");
      expect(payloadKeys).not.toContain("ip");
    });
  });
});
