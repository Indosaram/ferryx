// @vitest-environment-options {"url": "http://192.168.1.20:8080/"}
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AccountSessionError,
  getConfiguredAccountOrigin,
  getConfiguredAccountOriginUnchecked,
  getStoredAccountSessionToken,
  requestLogin,
  storeAccountOrigin,
  clearStoredAccountOrigin,
} from "./accountSession";

describe("web build served from an origin that may not receive account credentials", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    clearStoredAccountOrigin();
  });

  it("errors on the invalid page origin instead of defaulting to the hosted relay", async () => {
    expect(window.location.origin).toBe("http://192.168.1.20:8080");
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);

    let error: unknown;
    try {
      getConfiguredAccountOrigin();
    } catch (err) {
      error = err;
    }
    expect(error).toBeInstanceOf(AccountSessionError);
    expect(error).toMatchObject({ code: "INVALID_ACCOUNT_ORIGIN", details: { origin: "http://192.168.1.20:8080" } });
    expect(getConfiguredAccountOriginUnchecked()).toBe("http://192.168.1.20:8080");
    expect(getStoredAccountSessionToken()).toBeNull();
    await expect(requestLogin("", "a@b.test")).rejects.toMatchObject({ code: "INVALID_ACCOUNT_ORIGIN" });
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("uses an explicitly configured account origin", () => {
    storeAccountOrigin("https://account.selfhost.test");
    expect(getConfiguredAccountOrigin()).toBe("https://account.selfhost.test");
  });
});
