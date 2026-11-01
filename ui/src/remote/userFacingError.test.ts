import { describe, expect, it } from "vitest";
import { TunnelError } from "./attachTunnel";
import { userFacingCode, userFacingError } from "./userFacingError";

class Coded extends Error {
  constructor(readonly code: string, message: string, readonly status?: number) {
    super(message);
  }
}

describe("userFacingError", () => {
  it("never leaks the raw provider message, code or id", () => {
    const raw = new Coded(
      "MAIL_FAILED",
      'Resend API error 422 Unprocessable Entity: {"message":"Invalid `to` field","statusCode":422}',
    );
    const text = userFacingError(raw, { action: "send the sign-in link" });
    expect(text).toBe("Couldn't send the sign-in link. We couldn't send the sign-in email. Check the address and try again.");
    expect(text).not.toMatch(/MAIL_FAILED|Resend|422|statusCode/);
  });

  it("maps closed-stream families and session-specific messages to plain copy", () => {
    const err = new Coded("ATTACH_STREAM_CLOSED", "STREAM_CLOSED: session 0c9e1f2a-77 dropped");
    expect(userFacingCode(err)).toBe("STREAM_CLOSED");
    expect(userFacingError(err)).not.toContain("0c9e1f2a");
  });

  it("classifies by HTTP status when there is no code", () => {
    expect(userFacingCode({ status: 401 })).toBe("UNAUTHORIZED");
    expect(userFacingError({ status: 503 })).toBe("Something went wrong on our side. Try again in a moment.");
  });

  it("treats a fetch TypeError as a network failure", () => {
    expect(userFacingCode(new TypeError("Failed to fetch"))).toBe("NETWORK");
    expect(userFacingError(new TypeError("Load failed"))).not.toContain("Load failed");
  });

  it("falls back to generic copy for unknown errors without echoing them", () => {
    const text = userFacingError(new Error("daemon epoch changed for 7f3a"), { action: "open this worktree" });
    expect(text).toBe("Couldn't open this worktree. Something went wrong. Try again.");
  });

  it("maps structured tunnel errors to plain copy", () => {
    expect(userFacingCode(new TunnelError("STREAM_CLOSED", "transport has been closed"))).toBe("STREAM_CLOSED");
    expect(userFacingCode(new TunnelError("PREMATURE_CLOSE", "socket closed before handshake completed"))).toBe("STREAM_CLOSED");
    expect(userFacingCode(new TunnelError("HTTP_TIMEOUT", "no complete response"))).toBe("TIMEOUT");
    expect(userFacingCode(new TunnelError("HANDSHAKE_TIMEOUT", "handshake timed out"))).toBe("TIMEOUT");
    expect(userFacingCode(new TunnelError("WEBSOCKET_UPGRADE_FAILED", "server returned status 401", 401))).toBe("UNAUTHORIZED");
    expect(userFacingError(new TunnelError("STREAM_CLOSED", "transport has been closed"))).toBe(
      "The connection to your computer dropped. Reconnecting usually fixes this.",
    );
  });
});
