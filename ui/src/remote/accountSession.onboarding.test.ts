import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionError,
  approveMachineRequest,
  denyMachineRequest,
  issueInstallKey,
  listInstallKeys,
  listPendingMachineRequests,
  lookupMachineRequest,
  normalizeUserCode,
  revokeInstallKey,
} from "./accountSession";

const origin = "https://relay.example.com";
const token = "account-token-1";

const jsonResponse = (payload: unknown, status = 200) =>
  new Response(JSON.stringify(payload), { status, headers: { "Content-Type": "application/json" } });

const requestView = {
  requestId: "req-1",
  machineId: "mach-1",
  displayName: "noye",
  platform: "linux",
  appVersion: "2026.1023.1",
  requesterIp: "203.0.113.42",
  createdAt: 1_700_000_000,
  expiresAt: 1_700_000_900,
};

let fetchMock: ReturnType<typeof vi.fn>;

function lastCall(): { url: string; init: RequestInit } {
  const call = fetchMock.mock.calls.at(-1)!;
  return { url: String(call[0]), init: (call[1] ?? {}) as RequestInit };
}

function authHeader(init: RequestInit): string | null {
  return new Headers(init.headers).get("Authorization");
}

describe("accountSession onboarding client", () => {
  beforeEach(() => {
    fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("issueInstallKey posts label and ttl with the bearer token and returns the one-time key", async () => {
    const issued = {
      keyId: "key-1",
      key: "fxk_" + "a".repeat(64),
      expiresAt: 1_700_003_600,
      installCommand: { sh: "curl ... --key fxk_x", ps1: "& ... -Key fxk_x" },
    };
    fetchMock.mockResolvedValue(jsonResponse(issued));

    await expect(issueInstallKey(origin, token, { label: "noye", ttlSecs: 600 })).resolves.toEqual(issued);

    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/install-keys`);
    expect(init.method).toBe("POST");
    expect(authHeader(init)).toBe(`Bearer ${token}`);
    expect(JSON.parse(String(init.body))).toEqual({ label: "noye", ttlSecs: 600 });
  });

  it("issueInstallKey sends an empty body by default", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({ keyId: "k", key: "fxk_1", expiresAt: 1, installCommand: { sh: "a", ps1: "b" } }),
    );
    await issueInstallKey(origin, token);
    expect(JSON.parse(String(lastCall().init.body))).toEqual({});
  });

  it("issueInstallKey surfaces structured INSTALL_KEY_LIMIT and PLAN_LIMIT_REACHED with details", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ code: "INSTALL_KEY_LIMIT", message: "too many keys" }, 429));
    await expect(issueInstallKey(origin, token)).rejects.toMatchObject({
      code: "INSTALL_KEY_LIMIT",
      message: "too many keys",
      status: 429,
    });

    fetchMock.mockResolvedValueOnce(
      jsonResponse({ code: "PLAN_LIMIT_REACHED", message: "limit", details: { plan: "free", limit: 1, used: 1 } }, 402),
    );
    const err = await issueInstallKey(origin, token).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(AccountSessionError);
    expect((err as AccountSessionError).code).toBe("PLAN_LIMIT_REACHED");
    expect((err as AccountSessionError).details).toEqual({ plan: "free", limit: 1, used: 1 });
  });

  it("issueInstallKey rejects a response without installCommand as INVALID_RESPONSE", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ keyId: "k", key: "fxk_1", expiresAt: 1 }));
    await expect(issueInstallKey(origin, token)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("UNAUTHORIZED is honoured only with HTTP 401", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ code: "UNAUTHORIZED", message: "x" }, 502));
    await expect(issueInstallKey(origin, token)).rejects.toMatchObject({ code: "INSTALL_KEY_FAILED" });
    fetchMock.mockResolvedValueOnce(jsonResponse({ code: "UNAUTHORIZED", message: "x" }, 401));
    await expect(issueInstallKey(origin, token)).rejects.toMatchObject({ code: "UNAUTHORIZED", status: 401 });
  });

  it("listInstallKeys reads the key list and never expects the secret", async () => {
    const keys = [{ keyId: "k1", label: "a", createdAt: 1, expiresAt: 2, consumedAt: null, consumedMachineId: null }];
    fetchMock.mockResolvedValue(jsonResponse(keys));
    await expect(listInstallKeys(origin, token)).resolves.toEqual(keys);
    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/install-keys`);
    expect(init.method).toBeUndefined();
    expect(authHeader(init)).toBe(`Bearer ${token}`);
  });

  it("listInstallKeys rejects a non-array body", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ keys: [] }));
    await expect(listInstallKeys(origin, token)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("revokeInstallKey posts to the encoded revoke path and accepts 204", async () => {
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    await expect(revokeInstallKey(origin, token, "key/1")).resolves.toBeUndefined();
    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/install-keys/key%2F1/revoke`);
    expect(init.method).toBe("POST");
  });

  it("listPendingMachineRequests returns requests with their user code", async () => {
    fetchMock.mockResolvedValue(jsonResponse([{ ...requestView, userCode: "ABCD-EFGH" }]));
    await expect(listPendingMachineRequests(origin, token)).resolves.toEqual([{ ...requestView, userCode: "ABCD-EFGH" }]);
    expect(lastCall().url).toBe(`${origin}/api/account/v1/machine-requests`);
  });

  it("listPendingMachineRequests rejects entries without a user code", async () => {
    fetchMock.mockResolvedValue(jsonResponse([requestView]));
    await expect(listPendingMachineRequests(origin, token)).rejects.toMatchObject({ code: "INVALID_RESPONSE" });
  });

  it("lookupMachineRequest normalizes the code and returns the request view", async () => {
    fetchMock.mockResolvedValue(jsonResponse(requestView));
    await expect(lookupMachineRequest(origin, token, " abcd efgh ")).resolves.toEqual(requestView);
    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/machine-requests/lookup`);
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({ userCode: "ABCD-EFGH" });
  });

  it("lookupMachineRequest surfaces MACHINE_REQUEST_FOREIGN_ACCOUNT by code", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ code: "MACHINE_REQUEST_FOREIGN_ACCOUNT", message: "not yours" }, 403));
    await expect(lookupMachineRequest(origin, token, "ABCD-EFGH")).rejects.toMatchObject({
      code: "MACHINE_REQUEST_FOREIGN_ACCOUNT",
      status: 403,
    });
  });

  it("approveMachineRequest posts the user code to the request's approve path", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ machineId: "mach-1" }));
    await expect(approveMachineRequest(origin, token, "req-1", "ABCDEFGH")).resolves.toEqual({ machineId: "mach-1" });
    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/machine-requests/req-1/approve`);
    expect(JSON.parse(String(init.body))).toEqual({ userCode: "ABCD-EFGH" });
    expect(authHeader(init)).toBe(`Bearer ${token}`);
  });

  it("approveMachineRequest surfaces MACHINE_REQUEST_DECIDED and ACCOUNT_MACHINE_CLAIMED", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ code: "MACHINE_REQUEST_DECIDED", message: "decided" }, 409));
    await expect(approveMachineRequest(origin, token, "req-1", "ABCD-EFGH")).rejects.toMatchObject({ code: "MACHINE_REQUEST_DECIDED" });
    fetchMock.mockResolvedValueOnce(jsonResponse({ code: "ACCOUNT_MACHINE_CLAIMED", message: "claimed" }, 409));
    await expect(approveMachineRequest(origin, token, "req-1", "ABCD-EFGH")).rejects.toMatchObject({ code: "ACCOUNT_MACHINE_CLAIMED" });
  });

  it("denyMachineRequest posts to the deny path and accepts 204", async () => {
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));
    await expect(denyMachineRequest(origin, token, "req-1", "ABCD-EFGH")).resolves.toBeUndefined();
    const { url, init } = lastCall();
    expect(url).toBe(`${origin}/api/account/v1/machine-requests/req-1/deny`);
    expect(JSON.parse(String(init.body))).toEqual({ userCode: "ABCD-EFGH" });
  });

  it("an invalid origin never receives the token", async () => {
    fetchMock.mockResolvedValue(jsonResponse([]));
    await expect(listPendingMachineRequests("http://evil.example", token)).rejects.toMatchObject({
      code: "INVALID_ACCOUNT_ORIGIN",
      details: { origin: "http://evil.example" },
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("normalizeUserCode upper-cases, strips separators and re-inserts the dash", () => {
    expect(normalizeUserCode("abcd-efgh")).toBe("ABCD-EFGH");
    expect(normalizeUserCode(" ab cd ef gh ")).toBe("ABCD-EFGH");
    expect(normalizeUserCode("abc")).toBe("ABC");
  });
});
