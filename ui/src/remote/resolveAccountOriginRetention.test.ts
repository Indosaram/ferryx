import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  resolveAccountOrigin,
  DEFAULT_ACCOUNT_ORIGIN,
  ACCOUNT_ORIGIN_PROBE_STORAGE_KEY,
  storeAccountSessionToken,
  getStoredAccountSessionToken,
} from "./accountSession";

describe("resolveAccountOrigin Contract & Session Retention Regressions", () => {
  const originalFetch = globalThis.fetch;

  beforeEach(() => {
    sessionStorage.clear();
    localStorage.clear();
    vi.restoreAllMocks();
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    sessionStorage.clear();
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("Scenario 1: Confirmed 404 falls back to DEFAULT_ACCOUNT_ORIGIN and caches the fallback", async () => {
    const pageOrigin = "http://127.0.0.1:43821";
    globalThis.fetch = vi.fn().mockImplementation(async () =>
      new Response(JSON.stringify({ code: "NOT_FOUND" }), { status: 404 })
    );

    const resolved = await resolveAccountOrigin(pageOrigin);
    expect(resolved).toBe(DEFAULT_ACCOUNT_ORIGIN);

    const cached = sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${pageOrigin}`);
    expect(cached).toBe(DEFAULT_ACCOUNT_ORIGIN);
  });

  it("Scenario 2: Transient network error preserves candidate origin, does NOT cache fallback, and subsequent 200 recovers", async () => {
    const candidate = "https://relay.ferryx.dev";
    let callCount = 0;
    globalThis.fetch = vi.fn().mockImplementation(async () => {
      callCount += 1;
      if (callCount === 1) {
        return Promise.reject(new Error("Network connection lost"));
      }
      return new Response(JSON.stringify({ status: "ok" }), { status: 200 });
    });

    const first = await resolveAccountOrigin(candidate);
    expect(first).toBe(candidate);
    expect(sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`)).toBeNull();

    const second = await resolveAccountOrigin(candidate);
    expect(second).toBe(candidate);
    expect(callCount).toBe(2);
    expect(sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`)).toBe(candidate);
  });

  it.each([401, 403, 429, 500, 502, 503])(
    "Scenario 3: Transient HTTP %i error preserves candidate origin, does NOT cache fallback, and subsequent 200 recovers",
    async (status) => {
      const candidate = "https://relay.ferryx.dev";
      let callCount = 0;
      globalThis.fetch = vi.fn().mockImplementation(async () => {
        callCount += 1;
        if (callCount === 1) {
          return new Response("Transient error", { status });
        }
        return new Response(JSON.stringify({ status: "ok" }), { status: 200 });
      });

      const first = await resolveAccountOrigin(candidate);
      expect(first).toBe(candidate);
      expect(sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`)).toBeNull();

      const second = await resolveAccountOrigin(candidate);
      expect(second).toBe(candidate);
      expect(callCount).toBe(2);
      expect(sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`)).toBe(candidate);
    }
  );

  it("Scenario 4: Previously cached fallback != candidate re-probes safely, calls fetch exactly once, and recovers to candidate when healthy", async () => {
    const candidate = "https://relay.example.com";
    expect(candidate).not.toBe(DEFAULT_ACCOUNT_ORIGIN);
    sessionStorage.setItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`, DEFAULT_ACCOUNT_ORIGIN);

    const fetchSpy = vi.fn().mockImplementation(async () =>
      new Response(JSON.stringify({ status: "ok" }), { status: 200 })
    );
    globalThis.fetch = fetchSpy;

    const resolved = await resolveAccountOrigin(candidate);
    expect(resolved).toBe(candidate);
    expect(fetchSpy).toHaveBeenCalledTimes(1);

    const cached = sessionStorage.getItem(`${ACCOUNT_ORIGIN_PROBE_STORAGE_KEY}:${candidate}`);
    expect(cached).toBe(candidate);
  });

  it("Scenario 5: 200 OK probe caches candidate and subsequent lookups remain fast without refetching", async () => {
    const candidate = "https://relay.ferryx.dev";
    const fetchSpy = vi.fn().mockImplementation(async () =>
      new Response(JSON.stringify({ status: "ok" }), { status: 200 })
    );
    globalThis.fetch = fetchSpy;

    const first = await resolveAccountOrigin(candidate);
    expect(first).toBe(candidate);
    expect(fetchSpy).toHaveBeenCalledTimes(1);

    const second = await resolveAccountOrigin(candidate);
    expect(second).toBe(candidate);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it("Scenario 6: Module integration login store and restore under transient candidate health error", async () => {
    const productionOrigin = "https://relay.ferryx.dev";

    globalThis.fetch = vi.fn().mockImplementation(async (url: string) => {
      if (url.includes("/api/account/v1/health")) {
        return Promise.reject(new Error("Transient mobile timeout"));
      }
      return new Response("Not found", { status: 404 });
    });

    const origin = await resolveAccountOrigin(productionOrigin);
    expect(origin).toBe(productionOrigin);

    storeAccountSessionToken("test-session-token-30d", origin);

    const restored = getStoredAccountSessionToken(productionOrigin);
    expect(restored).toBe("test-session-token-30d");
  });
});
