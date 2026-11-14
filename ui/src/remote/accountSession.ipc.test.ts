import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import {
  AccountSessionError,
  clearStoredAccountOrigin,
  getConfiguredAccountOrigin,
  isTerminalLoginPollError,
  listMachines,
  pollLogin,
  requestLogin,
  revokeInstallKey,
  storeAccountOrigin,
} from "./accountSession";
import { BillingApiError, fetchBillingEntitlement } from "../lib/billingApi";
import { DEFAULT_RELAY_ORIGIN } from "../lib/pairedHostInventory";

const SELF_HOSTED = "https://account.selfhost.test";

function answer(status: number, body: unknown) {
  return { status, body: typeof body === "string" ? body : JSON.stringify(body) };
}

describe("desktop account HTTP runs through cmd_account_http_request", () => {
  const fetchSpy = vi.fn();

  beforeEach(() => {
    invokeMock.mockReset();
    fetchSpy.mockReset();
    vi.stubGlobal("fetch", fetchSpy);
    clearStoredAccountOrigin();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
    clearStoredAccountOrigin();
  });

  it("keeps the product default in the native app and honours a self-hosted origin", () => {
    expect(getConfiguredAccountOrigin()).toBe(DEFAULT_RELAY_ORIGIN);
    storeAccountOrigin(`${SELF_HOSTED}/`);
    expect(getConfiguredAccountOrigin()).toBe(SELF_HOSTED);
  });

  it("sends sign-in to a self-hosted origin through IPC, never through webview fetch", async () => {
    invokeMock.mockResolvedValueOnce(answer(200, { loginHandle: "h".repeat(64) }));

    await expect(requestLogin(SELF_HOSTED, " owner@selfhost.test ")).resolves.toEqual({ loginHandle: "h".repeat(64) });

    expect(fetchSpy).not.toHaveBeenCalled();
    expect(invokeMock).toHaveBeenCalledWith("cmd_account_http_request", {
      origin: SELF_HOSTED,
      method: "POST",
      path: "/api/account/v1/login/request",
      bearer: null,
      body: JSON.stringify({ email: "owner@selfhost.test" }),
      timeoutMs: 15_000,
    });
  });

  it("passes the session bearer and keeps the server's structured error envelope", async () => {
    invokeMock.mockResolvedValueOnce(answer(401, { code: "UNAUTHORIZED", message: "unknown or expired session" }));

    const error = await listMachines(SELF_HOSTED, "session-tok").catch((err: unknown) => err);

    expect(error).toBeInstanceOf(AccountSessionError);
    expect(error).toMatchObject({ code: "UNAUTHORIZED", status: 401 });
    expect(invokeMock.mock.calls[0][1]).toMatchObject({
      method: "GET",
      path: "/api/account/v1/machines",
      bearer: "session-tok",
      body: null,
    });
  });

  it("surfaces an unreachable origin as a typed error naming the origin", async () => {
    invokeMock.mockRejectedValue({
      code: "ACCOUNT_UNREACHABLE",
      message: `account service at ${SELF_HOSTED} is unreachable: error sending request`,
      details: { origin: SELF_HOSTED },
    });

    const error = await pollLogin(SELF_HOSTED, "handle").catch((err: unknown) => err);

    expect(error).toBeInstanceOf(AccountSessionError);
    expect(error).toMatchObject({ code: "ACCOUNT_UNREACHABLE", details: { origin: SELF_HOSTED } });
    expect((error as AccountSessionError).message).toContain(SELF_HOSTED);
    expect(isTerminalLoginPollError(error)).toBe(false);

    await expect(fetchBillingEntitlement({ accountOrigin: SELF_HOSTED, accountSessionToken: "tok" })).rejects.toMatchObject({
      code: "ACCOUNT_UNREACHABLE",
      details: { origin: SELF_HOSTED },
    });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("routes billing through IPC and accepts an empty 204 answer", async () => {
    invokeMock.mockResolvedValueOnce(answer(404, { code: "NOT_FOUND", message: "no billing" }));
    const error = await fetchBillingEntitlement({ accountOrigin: SELF_HOSTED, accountSessionToken: "tok" }).catch(
      (err: unknown) => err,
    );
    expect(error).toBeInstanceOf(BillingApiError);
    expect(error).toMatchObject({ code: "NOT_FOUND", status: 404 });
    expect(invokeMock.mock.calls[0][1]).toMatchObject({
      method: "GET",
      path: "/api/account/v1/billing/entitlement",
      bearer: "tok",
    });

    invokeMock.mockResolvedValueOnce(answer(204, ""));
    await expect(revokeInstallKey(SELF_HOSTED, "tok", "key 1")).resolves.toBeUndefined();
    expect(invokeMock.mock.calls[1][1]).toMatchObject({
      method: "POST",
      path: "/api/account/v1/install-keys/key%201/revoke",
    });
  });

  it("refuses an invalid origin before any request is made", async () => {
    await expect(requestLogin("http://account.selfhost.test", "a@b.test")).rejects.toMatchObject({
      code: "INVALID_ACCOUNT_ORIGIN",
      details: { origin: "http://account.selfhost.test" },
    });
    expect(invokeMock).not.toHaveBeenCalled();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("keeps the mobile shell on fetch", async () => {
    vi.stubEnv("VITE_FERRYX_MOBILE", "1");
    fetchSpy.mockResolvedValueOnce(new Response(JSON.stringify({ loginHandle: "m".repeat(64) }), { status: 200 }));

    await requestLogin(SELF_HOSTED, "a@b.test");

    expect(invokeMock).not.toHaveBeenCalled();
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    expect(String(fetchSpy.mock.calls[0][0])).toBe(`${SELF_HOSTED}/api/account/v1/login/request`);
  });
});
