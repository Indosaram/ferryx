import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  logoutAccountSession,
  requestGrant,
  allocateSession,
  issueEnrollmentCode,
  AccountSessionError,
  storeAccountSessionToken,
  getStoredAccountSessionToken,
  clearStoredAccountSessionToken,
  getAccountLastSelectedTarget,
  setAccountLastSelectedTarget,
  clearAccountLastSelectedTarget,
  type AccountMachineView,
} from "./accountSession";

describe("Account Follow-ups: Sign-out, Auth Classification, and Last Target Persistence", () => {
  const originalFetch = globalThis.fetch;
  const relayUrl = "https://relay.ferryx.dev";

  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    localStorage.clear();
    sessionStorage.clear();
    vi.restoreAllMocks();
  });

  describe("Item A: Server-side sign-out (logoutAccountSession)", () => {
    it("sends POST /api/account/v1/logout with Bearer token and resolves cleanly on 204", async () => {
      let requestedUrl = "";
      let requestedMethod = "";
      let authHeader = "";

      globalThis.fetch = vi.fn().mockImplementation(async (url: string, init?: RequestInit) => {
        requestedUrl = url;
        requestedMethod = init?.method ?? "GET";
        authHeader = (init?.headers as Record<string, string>)?.Authorization ?? "";
        return new Response(null, { status: 204 });
      });

      await logoutAccountSession(relayUrl, "test-session-token-123");

      expect(requestedUrl).toBe("https://relay.ferryx.dev/api/account/v1/logout");
      expect(requestedMethod).toBe("POST");
      expect(authHeader).toBe("Bearer test-session-token-123");
    });

    it("never throws when the server responds with 500 or 401 (best-effort)", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () => {
        return new Response("Internal server error", { status: 500 });
      });

      await expect(logoutAccountSession(relayUrl, "test-session-token-123")).resolves.toBeUndefined();
    });

    it("never throws when network fetch rejects (best-effort)", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () => {
        throw new Error("Network offline");
      });

      await expect(logoutAccountSession(relayUrl, "test-session-token-123")).resolves.toBeUndefined();
    });

    it("does nothing if token or origin is empty", async () => {
      const fetchMock = vi.fn();
      globalThis.fetch = fetchMock;

      await logoutAccountSession("", "tok");
      await logoutAccountSession(relayUrl, "");
      expect(fetchMock).not.toHaveBeenCalled();
    });
  });

  describe("Item B: Consistent auth classification across grant, allocate, enrollment", () => {
    const mockMachine: AccountMachineView = {
      machineRecordId: "rec-1",
      machineId: "mach-1",
      displayName: "Box",
      publicKey: "pk-1",
      attachPublicKey: "apk-1",
      relayOrigin: relayUrl,
      platform: "linux",
      online: true,
      enrollmentEpoch: "epoch-1",
      lastSeenAt: Date.now(),
    };

    it("requestGrant: HTML 403 and text 401 do NOT yield UNAUTHORIZED, keeping GRANT_FAILED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("<html>Cloudflare WAF 403</html>", {
          status: 403,
          headers: { "Content-Type": "text/html" },
        })
      );

      let err403: AccountSessionError | null = null;
      try {
        await requestGrant(relayUrl, "tok", mockMachine, "client-apk");
      } catch (err) {
        if (err instanceof AccountSessionError) err403 = err;
      }
      expect(err403?.status).toBe(403);
      expect(err403?.code).toBe("GRANT_FAILED");

      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("Gateway Proxy 401", {
          status: 401,
          headers: { "Content-Type": "text/plain" },
        })
      );

      let err401: AccountSessionError | null = null;
      try {
        await requestGrant(relayUrl, "tok", mockMachine, "client-apk");
      } catch (err) {
        if (err instanceof AccountSessionError) err401 = err;
      }
      expect(err401?.status).toBe(401);
      expect(err401?.code).toBe("GRANT_FAILED");
    });

    it("requestGrant: structured 401 with code UNAUTHORIZED yields UNAUTHORIZED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "Session expired" }), {
          status: 401,
          headers: { "Content-Type": "application/json" },
        })
      );

      let err: AccountSessionError | null = null;
      try {
        await requestGrant(relayUrl, "tok", mockMachine, "client-apk");
      } catch (e) {
        if (e instanceof AccountSessionError) err = e;
      }
      expect(err?.status).toBe(401);
      expect(err?.code).toBe("UNAUTHORIZED");
    });

    it("allocateSession: HTML 403 and text 401 do NOT yield UNAUTHORIZED, keeping ALLOCATE_SESSION_FAILED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("<html>Forbidden 403</html>", {
          status: 403,
          headers: { "Content-Type": "text/html" },
        })
      );

      let err403: AccountSessionError | null = null;
      try {
        await allocateSession(relayUrl, "tok", "mach-1");
      } catch (err) {
        if (err instanceof AccountSessionError) err403 = err;
      }
      expect(err403?.status).toBe(403);
      expect(err403?.code).toBe("ALLOCATE_SESSION_FAILED");

      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("Unauthorized text 401", {
          status: 401,
          headers: { "Content-Type": "text/plain" },
        })
      );

      let err401: AccountSessionError | null = null;
      try {
        await allocateSession(relayUrl, "tok", "mach-1");
      } catch (err) {
        if (err instanceof AccountSessionError) err401 = err;
      }
      expect(err401?.status).toBe(401);
      expect(err401?.code).toBe("ALLOCATE_SESSION_FAILED");
    });

    it("allocateSession: structured 401 with code UNAUTHORIZED yields UNAUTHORIZED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "Session expired" }), {
          status: 401,
          headers: { "Content-Type": "application/json" },
        })
      );

      let err: AccountSessionError | null = null;
      try {
        await allocateSession(relayUrl, "tok", "mach-1");
      } catch (e) {
        if (e instanceof AccountSessionError) err = e;
      }
      expect(err?.status).toBe(401);
      expect(err?.code).toBe("UNAUTHORIZED");
    });

    it("issueEnrollmentCode: HTML 403 and text 401 do NOT yield UNAUTHORIZED, keeping ENROLLMENT_CODE_FAILED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("<html>Forbidden 403</html>", {
          status: 403,
          headers: { "Content-Type": "text/html" },
        })
      );

      let err403: AccountSessionError | null = null;
      try {
        await issueEnrollmentCode(relayUrl, "tok");
      } catch (err) {
        if (err instanceof AccountSessionError) err403 = err;
      }
      expect(err403?.status).toBe(403);
      expect(err403?.code).toBe("ENROLLMENT_CODE_FAILED");

      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response("Unauthorized text 401", {
          status: 401,
          headers: { "Content-Type": "text/plain" },
        })
      );

      let err401: AccountSessionError | null = null;
      try {
        await issueEnrollmentCode(relayUrl, "tok");
      } catch (err) {
        if (err instanceof AccountSessionError) err401 = err;
      }
      expect(err401?.status).toBe(401);
      expect(err401?.code).toBe("ENROLLMENT_CODE_FAILED");
    });

    it("issueEnrollmentCode: structured 401 with code UNAUTHORIZED yields UNAUTHORIZED", async () => {
      globalThis.fetch = vi.fn().mockImplementation(async () =>
        new Response(JSON.stringify({ code: "UNAUTHORIZED", message: "Session expired" }), {
          status: 401,
          headers: { "Content-Type": "application/json" },
        })
      );

      let err: AccountSessionError | null = null;
      try {
        await issueEnrollmentCode(relayUrl, "tok");
      } catch (e) {
        if (e instanceof AccountSessionError) err = e;
      }
      expect(err?.status).toBe(401);
      expect(err?.code).toBe("UNAUTHORIZED");
    });
  });

  describe("Item C: Last selected worktree target storage and clearing", () => {
    it("stores, retrieves, and clears last selected target under ferryx.account.last_target key", () => {
      expect(getAccountLastSelectedTarget(relayUrl)).toBeNull();

      setAccountLastSelectedTarget(relayUrl, {
        machineId: "m1",
        workspaceId: "ws1",
        worktreeSlug: "feat-a",
        worktreeLabel: "Feature A",
      });

      const retrieved = getAccountLastSelectedTarget(relayUrl);
      expect(retrieved).toEqual({
        machineId: "m1",
        workspaceId: "ws1",
        worktreeSlug: "feat-a",
        worktreeLabel: "Feature A",
      });

      clearAccountLastSelectedTarget(relayUrl);
      expect(getAccountLastSelectedTarget(relayUrl)).toBeNull();
    });

    it("clearAccountLastSelectedTarget removes last_target keys globally or per relayUrl", () => {
      setAccountLastSelectedTarget("https://relay-a.test", {
        machineId: "m1",
        workspaceId: "ws1",
      });
      setAccountLastSelectedTarget("https://relay-b.test", {
        machineId: "m2",
        workspaceId: "ws2",
      });

      clearAccountLastSelectedTarget("https://relay-a.test");
      expect(getAccountLastSelectedTarget("https://relay-a.test")).toBeNull();
      expect(getAccountLastSelectedTarget("https://relay-b.test")).not.toBeNull();

      clearAccountLastSelectedTarget();
      expect(getAccountLastSelectedTarget("https://relay-b.test")).toBeNull();
    });

    it("rejects corrupted or non-string object types in storage", () => {
      localStorage.setItem(
        "ferryx.account.last_target." + encodeURIComponent("https://relay.ferryx.dev"),
        JSON.stringify({ machineId: { nested: true }, workspaceId: 12345 }),
      );
      expect(getAccountLastSelectedTarget(relayUrl)).toBeNull();
    });

    it("clearStoredAccountSessionToken removes last_target keys", () => {
      storeAccountSessionToken("test-token", relayUrl);
      setAccountLastSelectedTarget(relayUrl, {
        machineId: "m1",
        workspaceId: "ws1",
      });

      clearStoredAccountSessionToken();

      expect(getStoredAccountSessionToken(relayUrl)).toBeNull();
      expect(getAccountLastSelectedTarget(relayUrl)).toBeNull();
    });
  });
});
